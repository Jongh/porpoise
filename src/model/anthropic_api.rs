use anyhow::{Context, Result};
use std::sync::Mutex;

use crate::model::adapter::{ModelAdapter, ModelConfig};
use crate::session::input::SessionInput;
use crate::session::output::RoleOutputData;

// role별 JSON schema - include_str!로 임베딩
const PLANNING_SCHEMA: &str = include_str!("schemas/planning_schema.json");
const DEVELOPMENT_SCHEMA: &str = include_str!("schemas/development_schema.json");
const TESTING_SCHEMA: &str = include_str!("schemas/testing_schema.json");
const REVIEW_SCHEMA: &str = include_str!("schemas/review_schema.json");
const MILESTONE_SCHEMA: &str = include_str!("schemas/milestone_schema.json");

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

        let context_text = build_context_text(input);

        let request_body = serde_json::json!({
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

        let response = ureq::post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", &api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(&request_body)
            .context("Anthropic API 호출 실패")?;

        let response_json: serde_json::Value = response.into_json()?;
        *self.raw_text.lock().unwrap() = Some(response_json.to_string());

        // tool_use 블록에서 input 추출
        let content = response_json["content"]
            .as_array()
            .context("응답에 content 배열 없음")?;

        for block in content {
            if block["type"] == "tool_use" && block["name"] == "submit_report" {
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

fn build_context_text(input: &SessionInput) -> String {
    let mut parts = Vec::new();

    if !input.project_summary.is_empty() {
        parts.push(format!("## 프로젝트 정보\n\n{}", input.project_summary));
    }

    if let Some(tech) = &input.tech_context {
        parts.push(format!("## 기술 스택\n\n{}", tech));
    }

    if !input.dod.is_empty() {
        parts.push(format!("## 완료 기준 (DoD)\n\n{}", input.dod.join("\n")));
    }

    if !input.conventions.is_empty() {
        parts.push(format!("## 코딩 컨벤션\n\n{}", input.conventions.join("\n")));
    }

    parts.push(format!("## 현재 작업\n\n{} — {}", input.task_id, input.task_title));
    parts.push(format!("역할: {}", input.role));

    if let Some(snap) = &input.workspace_snapshot {
        let snap_text = crate::workspace::snapshot::snapshot_to_context_text(snap);
        if !snap_text.is_empty() {
            parts.push(format!("## 소스 코드 스냅샷\n\n{}", snap_text));
        }
    }

    if let Some(prev) = &input.previous_reports.planning {
        parts.push(format!("## 기획 보고서 요약\n\n{}", prev.summary));
    }
    if let Some(prev) = &input.previous_reports.development {
        parts.push(format!("## 개발 보고서 요약\n\n{}", prev.summary));
    }

    if !input.execution_results.is_empty() {
        let er_text: Vec<String> = input.execution_results.iter().map(|r| {
            format!("### {} (exit={})\nstdout: {}\nstderr: {}",
                r.purpose, r.exit_code, r.stdout.trim(), r.stderr.trim())
        }).collect();
        parts.push(format!("## 실행 결과\n\n{}", er_text.join("\n\n")));
    }

    if !input.hints.is_empty() {
        parts.push(format!("## 추가 지시사항\n\n{}", input.hints.join("\n")));
    }

    if !input.prev_reasons.is_empty() {
        parts.push(format!("## PREV 사유\n\n{}", input.prev_reasons.join("\n")));
    }

    parts.join("\n\n")
}

fn parse_role_output_from_value(v: &serde_json::Value, role: &str) -> Result<RoleOutputData> {
    use crate::session::planning::PlanningOutput;
    use crate::session::development::DevelopmentOutput;
    use crate::session::testing::TestingOutput;
    use crate::session::review::ReviewOutput;
    use crate::session::milestone::MilestoneOutput;

    match role {
        "planning" => Ok(RoleOutputData::Planning(serde_json::from_value::<PlanningOutput>(v.clone())?)),
        "development" => Ok(RoleOutputData::Development(serde_json::from_value::<DevelopmentOutput>(v.clone())?)),
        "testing" => Ok(RoleOutputData::Testing(serde_json::from_value::<TestingOutput>(v.clone())?)),
        "review" => Ok(RoleOutputData::Review(serde_json::from_value::<ReviewOutput>(v.clone())?)),
        "milestone" => Ok(RoleOutputData::Milestone(serde_json::from_value::<MilestoneOutput>(v.clone())?)),
        _ => anyhow::bail!("Unknown role: {}", role),
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
