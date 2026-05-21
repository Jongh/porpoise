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

// 단계 프롬프트 템플릿 임베딩
const PLANNING_PROMPT: &str = include_str!("../init/prompts/01-planning-api.tmpl");
const DEVELOPMENT_PROMPT: &str = include_str!("../init/prompts/02-development-api.tmpl");
const TESTING_PROMPT: &str = include_str!("../init/prompts/03-testing-api.tmpl");
const REVIEW_PROMPT: &str = include_str!("../init/prompts/04-review-api.tmpl");
const MILESTONE_PROMPT: &str = include_str!("../init/prompts/05-milestone-api.tmpl");

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
    api_key_env: String,
    raw_text: Mutex<Option<String>>,
}

impl AnthropicApiAdapter {
    pub fn new(api_key_env: String) -> Self {
        AnthropicApiAdapter {
            api_key_env,
            raw_text: Mutex::new(None),
        }
    }

    fn get_api_key(&self) -> Result<String> {
        std::env::var(&self.api_key_env)
            .with_context(|| format!(
                "'{}' 환경변수가 설정되지 않았습니다.\n\
                 \x20 Windows (PowerShell): $env:{} = \"실제키값\"\n\
                 \x20 macOS / Linux:        export {}=\"실제키값\"",
                self.api_key_env, self.api_key_env, self.api_key_env
            ))
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
        let api_key = self.get_api_key()?;
        let tool_schema_str = Self::get_tool_schema(&input.role);
        let tool_schema: serde_json::Value = serde_json::from_str(tool_schema_str)?;

        let system_prompt = resolve_role_system_prompt(&input.role, &input.role_extra);
        let context_text = build_context_text(input);

        let max_tokens: u32 = if input.role == "development" { 16384 } else { 4096 };
        let mut request_body = serde_json::json!({
            "model": config.model_id,
            "max_tokens": max_tokens,
            "messages": [{"role": "user", "content": context_text}],
            "tools": [{
                "name": "submit_report",
                "description": "단계 완료 결과를 구조화된 JSON으로 제출합니다",
                "input_schema": tool_schema
            }],
            "tool_choice": {"type": "tool", "name": "submit_report"}
        });

        if !system_prompt.is_empty() {
            request_body["system"] = serde_json::Value::String(system_prompt);
        }

        let response = match ureq::post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", &api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(&request_body)
        {
            Ok(r) => r,
            Err(ureq::Error::Status(code, resp)) => {
                let body_text = resp.into_string().unwrap_or_default();
                anyhow::bail!("Anthropic API HTTP {}: {}", code, body_text);
            }
            Err(e) => return Err(anyhow::Error::new(e).context("Anthropic API 호출 실패")),
        };

        let response_json: serde_json::Value = response
            .into_json()
            .context("Anthropic API 응답 JSON 파싱 실패 — 응답 본문이 유효한 JSON이 아닙니다.")?;
        *self.raw_text.lock().unwrap() = Some(response_json.to_string());

        // tool_use 블록에서 input 추출
        let content = response_json["content"]
            .as_array()
            .context("응답에 content 배열 없음")?;

        // AI 응답 콘솔 출력 (--verbose 시에만)
        if config.verbose {
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
        }

        for block in content {
            if block["type"] == "tool_use" && block["name"] == "submit_report" {
                let pretty = serde_json::to_string_pretty(&block["input"]).unwrap_or_default();
                if config.verbose {
                    println!("\n{}", "[AI submit_report]".dimmed());
                    println!("{}", pretty.dimmed());
                }
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
        let env_name = "ANTHROPIC_API_KEY_TEST_MISSING_XYZ123";
        std::env::remove_var(env_name);
        let adapter = AnthropicApiAdapter::new(env_name.to_string());
        let result = adapter.get_api_key();
        assert!(result.is_err());
    }

    #[test]
    fn custom_api_key_env_is_used() {
        let env_name = "ANTHROPIC_API_KEY_TEST_CUSTOM_XYZ456";
        std::env::set_var(env_name, "test-key-value");
        let adapter = AnthropicApiAdapter::new(env_name.to_string());
        let result = adapter.get_api_key();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-key-value");
        std::env::remove_var(env_name);
    }
}
