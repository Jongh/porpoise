use anyhow::Result;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::claude::runner::ClaudeRunner;
use crate::model::adapter::{ModelAdapter, ModelConfig};
use crate::orchestrator::report::{parse_exit_code, parse_completed_tasks, parse_prev_target};
use crate::session::development::DevelopmentOutput;
use crate::session::input::SessionInput;
use crate::session::milestone::MilestoneOutput;
use crate::session::output::{ExitCode, RoleOutputData};
use crate::session::planning::PlanningOutput;
use crate::session::renderer;
use crate::session::review::ReviewOutput;
use crate::session::testing::TestingOutput;

pub struct ClaudeCodeAdapter {
    runner: Option<ClaudeRunner>,
    project_path: PathBuf,
    raw_text: Mutex<Option<String>>,
}

impl ClaudeCodeAdapter {
    pub fn new(project_path: PathBuf) -> Result<Self> {
        let runner = ClaudeRunner::new().ok();
        Ok(ClaudeCodeAdapter {
            runner,
            project_path,
            raw_text: Mutex::new(None),
        })
    }
}

impl ModelAdapter for ClaudeCodeAdapter {
    fn execute(&self, input: &SessionInput, config: &ModelConfig) -> Result<RoleOutputData> {
        let runner = match &self.runner {
            Some(r) => r,
            None => anyhow::bail!(
                "Claude CLI를 찾을 수 없습니다. Claude Code를 설치하고 'claude'가 PATH에 있는지 확인하세요."
            ),
        };

        // 역할 프롬프트 파일 경로
        let prompt_filename = role_to_prompt_file(&input.role);
        let prompt_file = self.project_path.join(".porpoise").join("prompts").join(prompt_filename);

        // SessionInput에서 컨텍스트 파일 없이 프롬프트 문자열 생성
        let context_prefix = build_context_from_input(input);

        // messages/ 폴더에 원본 저장 (레거시 호환)
        let messages_dir = self.project_path.join(".porpoise").join("messages");
        std::fs::create_dir_all(&messages_dir).ok();
        let output_filename = format!(
            "{}-{}-C{}-R{}.md",
            input.task_id, input.role, input.cycle, input.retry
        );
        let output_file = messages_dir.join(&output_filename);

        // 프롬프트 파일 읽기 후 앞에 컨텍스트 붙이기
        let prompt_content = if prompt_file.exists() {
            let role_prompt = std::fs::read_to_string(&prompt_file).unwrap_or_default();
            format!("{}\n\n{}", context_prefix, role_prompt)
        } else {
            context_prefix
        };

        let model_opt = if config.model_id.is_empty() {
            None
        } else {
            Some(config.model_id.as_str())
        };

        let raw = runner.run_with_prompt_str(&prompt_content, &[], &output_file, model_opt)?;

        *self.raw_text.lock().unwrap() = Some(raw.clone());

        // JSON 파싱 시도
        if let Some(output) = try_parse_json_output(&raw, &input.role) {
            return Ok(output);
        }

        // 폴백: 마크다운 파싱
        Ok(fallback_from_markdown(&raw, &input.role, &input.task_id, input.cycle))
    }

    fn adapter_name(&self) -> &str {
        "claude_code"
    }

    fn supports_structured_output(&self) -> bool {
        false
    }

    fn last_raw_text(&self) -> Option<String> {
        self.raw_text.lock().unwrap().clone()
    }
}

fn role_to_prompt_file(role: &str) -> &'static str {
    match role {
        "planning" => "01-planning.md",
        "development" => "02-development.md",
        "testing" => "03-testing.md",
        "review" => "04-review.md",
        "milestone" => "05-milestone.md",
        _ => "00-orche.md",
    }
}

pub fn build_context_from_input(input: &SessionInput) -> String {
    let mut parts = Vec::new();

    if !input.project_summary.is_empty() {
        parts.push(format!("=== project.md ===\n{}", input.project_summary));
    }

    // 이전 역할 보고서들 (마크다운 렌더링)
    if let Some(ref planning) = input.previous_reports.planning {
        let md = renderer::render_planning(planning, input);
        parts.push(format!("=== 이전 planning 보고서 ===\n{}", md));
    }
    if let Some(ref dev) = input.previous_reports.development {
        let md = renderer::render_development(dev, input);
        parts.push(format!("=== 이전 development 보고서 ===\n{}", md));
    }
    if let Some(ref tst) = input.previous_reports.testing {
        let md = renderer::render_testing(tst, input);
        parts.push(format!("=== 이전 testing 보고서 ===\n{}", md));
    }
    if let Some(ref rev) = input.previous_reports.review {
        let md = renderer::render_review(rev, input);
        parts.push(format!("=== 이전 review 보고서 ===\n{}", md));
    }

    // 힌트
    for (i, hint) in input.hints.iter().enumerate() {
        parts.push(format!("=== 추가 지시사항 {} ===\n{}", i + 1, hint));
    }

    // PREV 이유
    for (i, reason) in input.prev_reasons.iter().enumerate() {
        parts.push(format!("=== 이전 사이클 피드백 {} ===\n{}", i + 1, reason));
    }

    parts.join("\n\n")
}

fn try_parse_json_output(raw: &str, role: &str) -> Option<RoleOutputData> {
    // 1) 전체가 JSON인지 시도
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Ok(output) = parse_role_output_from_value(&v, role) {
            return Some(output);
        }
    }

    // 2) ```json ... ``` 블록 탐지
    if let Some(start) = raw.find("```json") {
        let after = &raw[start + 7..];
        if let Some(end) = after.find("```") {
            let json_str = after[..end].trim();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Ok(output) = parse_role_output_from_value(&v, role) {
                    return Some(output);
                }
            }
        }
    }

    // 3) 첫 번째 '{' 부터 마지막 '}' 까지 추출
    if let (Some(start), Some(end)) = (raw.find('{'), raw.rfind('}')) {
        if start < end {
            let json_str = &raw[start..=end];
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Ok(output) = parse_role_output_from_value(&v, role) {
                    return Some(output);
                }
            }
        }
    }

    None
}

fn parse_role_output_from_value(v: &serde_json::Value, role: &str) -> Result<RoleOutputData> {
    match role {
        "planning" => {
            let o: PlanningOutput = serde_json::from_value(v.clone())?;
            Ok(RoleOutputData::Planning(o))
        }
        "development" => {
            let o: DevelopmentOutput = serde_json::from_value(v.clone())?;
            Ok(RoleOutputData::Development(o))
        }
        "testing" => {
            let o: TestingOutput = serde_json::from_value(v.clone())?;
            Ok(RoleOutputData::Testing(o))
        }
        "review" => {
            let o: ReviewOutput = serde_json::from_value(v.clone())?;
            Ok(RoleOutputData::Review(o))
        }
        "milestone" => {
            let o: MilestoneOutput = serde_json::from_value(v.clone())?;
            Ok(RoleOutputData::Milestone(o))
        }
        _ => anyhow::bail!("Unknown role: {}", role),
    }
}

pub fn fallback_from_markdown(raw: &str, role: &str, task_id: &str, cycle: u32) -> RoleOutputData {
    let exit_code = match parse_exit_code(raw) {
        Some(crate::orchestrator::report::ExitCode::Next) => ExitCode::Next,
        Some(crate::orchestrator::report::ExitCode::Prev) => ExitCode::Prev,
        Some(crate::orchestrator::report::ExitCode::Resp) => ExitCode::Resp,
        None => ExitCode::Resp,
    };
    let completed_tasks = parse_completed_tasks(raw);
    let prev_target = parse_prev_target(raw);
    let summary = raw.chars().take(500).collect::<String>();

    match role {
        "planning" => RoleOutputData::Planning(PlanningOutput {
            role: "planning".to_string(),
            task_id: task_id.to_string(),
            cycle,
            status: exit_code,
            summary,
            ..Default::default()
        }),
        "development" => RoleOutputData::Development(DevelopmentOutput {
            role: "development".to_string(),
            task_id: task_id.to_string(),
            cycle,
            status: exit_code,
            summary,
            ..Default::default()
        }),
        "testing" => RoleOutputData::Testing(TestingOutput {
            role: "testing".to_string(),
            task_id: task_id.to_string(),
            cycle,
            status: exit_code,
            summary,
            ..Default::default()
        }),
        "review" => RoleOutputData::Review(ReviewOutput {
            role: "review".to_string(),
            task_id: task_id.to_string(),
            cycle,
            status: exit_code,
            summary,
            completed_tasks,
            prev_target,
            ..Default::default()
        }),
        "milestone" => RoleOutputData::Milestone(MilestoneOutput {
            role: "milestone".to_string(),
            status: exit_code,
            summary,
            ..Default::default()
        }),
        _ => RoleOutputData::Planning(PlanningOutput {
            role: role.to_string(),
            task_id: task_id.to_string(),
            cycle,
            status: exit_code,
            summary,
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_parse_whole_json() {
        let json = r#"{"role":"planning","task_id":"M1-T01","cycle":1,"status":"NEXT","summary":"test","questions":[],"prev_reason":null,"implementation_plan":[],"dod_checklist":[],"risks":[]}"#;
        let result = try_parse_json_output(json, "planning");
        assert!(result.is_some());
        if let Some(RoleOutputData::Planning(p)) = result {
            assert_eq!(p.status, ExitCode::Next);
        }
    }

    #[test]
    fn json_parse_code_block() {
        let raw = "Some text\n```json\n{\"role\":\"planning\",\"task_id\":\"M1-T01\",\"cycle\":1,\"status\":\"NEXT\",\"summary\":\"test\",\"questions\":[],\"prev_reason\":null,\"implementation_plan\":[],\"dod_checklist\":[],\"risks\":[]}\n```\nMore text";
        let result = try_parse_json_output(raw, "planning");
        assert!(result.is_some());
    }

    #[test]
    fn fallback_extracts_next() {
        let raw = "Some content\n\nNEXT";
        let output = fallback_from_markdown(raw, "planning", "M1-T01", 1);
        assert_eq!(output.status(), &ExitCode::Next);
    }

    #[test]
    fn fallback_extracts_prev_with_target() {
        let raw = "Review\n<!-- PORPOISE_META\nstatus: CHANGES_REQUESTED\nprev_target: development\n-->\n\nPREV";
        let output = fallback_from_markdown(raw, "review", "M1-T01", 1);
        assert_eq!(output.status(), &ExitCode::Prev);
        assert_eq!(output.prev_target(), Some("development"));
    }
}
