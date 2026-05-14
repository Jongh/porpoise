use anyhow::{Context, Result};
use colored::Colorize;
use std::sync::Mutex;

use crate::model::adapter::{ModelAdapter, ModelConfig};
use crate::model::context::{api_json_format_hint, build_context_text, parse_role_output_from_value};
use crate::session::input::SessionInput;
use crate::session::output::RoleOutputData;

// role별 JSON schema - include_str!로 임베딩
const PLANNING_SCHEMA: &str = include_str!("schemas/planning_schema.json");
const DEVELOPMENT_SCHEMA: &str = include_str!("schemas/development_schema.json");
const TESTING_SCHEMA: &str = include_str!("schemas/testing_schema.json");
const REVIEW_SCHEMA: &str = include_str!("schemas/review_schema.json");
const MILESTONE_SCHEMA: &str = include_str!("schemas/milestone_schema.json");

// 역할 프롬프트 템플릿 임베딩
const PLANNING_PROMPT: &str = include_str!("../init/prompts/01-planning.tmpl");
const DEVELOPMENT_PROMPT: &str = include_str!("../init/prompts/02-development.tmpl");
const TESTING_PROMPT: &str = include_str!("../init/prompts/03-testing.tmpl");
const REVIEW_PROMPT: &str = include_str!("../init/prompts/04-review.tmpl");
const MILESTONE_PROMPT: &str = include_str!("../init/prompts/05-milestone.tmpl");

fn get_role_system_prompt(role: &str) -> &'static str {
    match role {
        "planning" => PLANNING_PROMPT,
        "development" => DEVELOPMENT_PROMPT,
        "testing" => TESTING_PROMPT,
        "review" => REVIEW_PROMPT,
        "milestone" => MILESTONE_PROMPT,
        _ => "",
    }
}

fn resolve_role_system_prompt(role: &str, role_extra: &str) -> String {
    let base = get_role_system_prompt(role)
        .replace("{{role_extra}}", role_extra)
        .trim()
        .to_string();
    let hint = api_json_format_hint(role);
    if hint.is_empty() {
        base
    } else {
        format!("{}{}", base, hint)
    }
}

pub struct AnthropicApiAdapter {
    raw_text: Mutex<Option<String>>,
}

impl AnthropicApiAdapter {
    pub fn new() -> Self {
        AnthropicApiAdapter {
            raw_text: Mutex::new(None),
        }
    }

    pub fn get_api_key() -> Result<String> {
        std::env::var("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY 환경 변수가 설정되지 않았습니다.\n  export ANTHROPIC_API_KEY=your-key 를 실행하세요.")
    }

    fn get_tool_schema(role: &str) -> &'static str {
        match role {
            "planning" => PLANNING_SCHEMA,
            "development" => DEVELOPMENT_SCHEMA,
            "testing" => TESTING_SCHEMA,
            "review" => REVIEW_SCHEMA,
            "milestone" => MILESTONE_SCHEMA,
            _ => PLANNING_SCHEMA,
        }
    }
}

impl ModelAdapter for AnthropicApiAdapter {
    fn execute(&self, input: &SessionInput, config: &ModelConfig) -> Result<RoleOutputData> {
        let api_key = Self::get_api_key()?;
        let tool_schema_str = Self::get_tool_schema(&input.role);
        let tool_schema: serde_json::Value = serde_json::from_str(tool_schema_str)?;

        let system_prompt = resolve_role_system_prompt(&input.role, &input.role_extra);
        let context_text = build_context_text(input);

        let mut request_body = serde_json::json!({
            "model": config.model_id,
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": context_text}],
            "tools": [{
                "name": "submit_report",
                "description": "역할 완료 결과를 구조화된 JSON으로 제출합니다",
                "input_schema": tool_schema
            }],
            "tool_choice": {"type": "tool", "name": "submit_report"}
        });

        if !system_prompt.is_empty() {
            request_body["system"] = serde_json::Value::String(system_prompt);
        }

        let response = ureq::post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", &api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(&request_body)
            .context("Anthropic API 호출 실패")?;

        let response_json: serde_json::Value = response
            .into_json()
            .context("Anthropic API 응답 JSON 파싱 실패 — 응답 본문이 유효한 JSON이 아닙니다.")?;
        *self.raw_text.lock().unwrap() = Some(response_json.to_string());

        // tool_use 블록에서 input 추출
        let content = response_json["content"]
            .as_array()
            .context("응답에 content 배열 없음")?;

        // AI 응답 콘솔 출력
        for block in content {
            if block["type"] == "text" {
                if let Some(text) = block["text"].as_str() {
                    if !text.trim().is_empty() {
                        println!("\n{}", "[AI 텍스트 응답]".dimmed());
                        println!("{}", text.dimmed());
                    }
                }
            }
        }

        for block in content {
            if block["type"] == "tool_use" && block["name"] == "submit_report" {
                let pretty = serde_json::to_string_pretty(&block["input"]).unwrap_or_default();
                println!("\n{}", "[AI submit_report]".dimmed());
                println!("{}", pretty.dimmed());
                let tool_input = &block["input"];
                return parse_role_output_from_value(tool_input, &input.role);
            }
        }

        anyhow::bail!("Anthropic API 응답에서 tool_use 블록을 찾을 수 없습니다")
    }

    fn adapter_name(&self) -> &str {
        "anthropic_api"
    }

    fn supports_structured_output(&self) -> bool {
        true
    }

    fn last_raw_text(&self) -> Option<String> {
        self.raw_text.lock().unwrap().clone()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_api_key_returns_error() {
        // 환경 변수 없는 상태에서 에러 반환 확인
        let original = std::env::var("ANTHROPIC_API_KEY").ok();
        // Windows에서는 환경변수 조작에 unsafe 없이 remove_var 사용
        std::env::remove_var("ANTHROPIC_API_KEY");
        let result = AnthropicApiAdapter::get_api_key();
        assert!(result.is_err());
        // 복원
        if let Some(key) = original {
            std::env::set_var("ANTHROPIC_API_KEY", key);
        }
    }
}
