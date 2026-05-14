use anyhow::Result;

use crate::session::input::SessionInput;
use crate::session::output::RoleOutputData;

/// API 어댑터(anthropic_api, openai_compatible)가 공유하는 컨텍스트 빌더.
/// Claude Code 어댑터는 === 스타일 헤더를 사용하므로 claude_code::build_context_from_input()을 별도로 유지한다.
pub fn build_context_text(input: &SessionInput) -> String {
    let mut parts = Vec::new();

    if !input.project_summary.is_empty() {
        parts.push(format!("## 프로젝트 정보\n\n{}", input.project_summary));
    }

    // 마일스톤 정보
    {
        let m = &input.milestone;
        if !m.id.is_empty() || !m.title.is_empty() || !m.goal.is_empty() {
            let mut lines = Vec::new();
            if !m.id.is_empty() {
                if !m.version.is_empty() {
                    lines.push(format!("**ID**: {}  **버전**: {}", m.id, m.version));
                } else {
                    lines.push(format!("**ID**: {}", m.id));
                }
            }
            if !m.title.is_empty() {
                lines.push(format!("**제목**: {}", m.title));
            }
            let mut section = format!("## 마일스톤\n\n{}", lines.join("\n"));
            if !m.goal.is_empty() {
                section.push_str(&format!("\n\n### 목표\n{}", m.goal));
            }
            parts.push(section);
        }
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
        let rendered = crate::session::renderer::render_planning(prev, input);
        parts.push(format!("## 기획 보고서\n\n{}", rendered));
    }
    if let Some(prev) = &input.previous_reports.development {
        let rendered = crate::session::renderer::render_development(prev, input);
        parts.push(format!("## 개발 보고서\n\n{}", rendered));
    }
    if let Some(prev) = &input.previous_reports.testing {
        let rendered = crate::session::renderer::render_testing(prev, input);
        parts.push(format!("## 테스트 보고서\n\n{}", rendered));
    }
    if let Some(prev) = &input.previous_reports.review {
        let rendered = crate::session::renderer::render_review(prev, input);
        parts.push(format!("## 리뷰 보고서\n\n{}", rendered));
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

pub fn parse_role_output_from_value(v: &serde_json::Value, role: &str) -> Result<RoleOutputData> {
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

/// API 어댑터 시스템 프롬프트 하단에 추가할 JSON 출력 형식 안내.
/// 마일스톤은 tmpl에 JSON 섹션이 없어 필수적이며, 나머지 역할은 강화용이다.
pub fn api_json_format_hint(role: &str) -> &'static str {
    match role {
        "milestone" => r#"

---

## API 모드 출력 형식 (submit_report 함수 — 필수)

**주의**: 이 세션은 API 모드입니다. 마일스톤 파일을 직접 생성하지 마세요.
대신 submit_report 함수를 호출하여 아래 필드를 **모두** 채워 제출하세요 (시스템이 파일을 자동 생성합니다):

- `role` = "milestone" (고정값)
- `milestone_id` = 마일스톤 ID (예: "M1") — **필수**
- `status` = "NEXT" (생성 완료) | "RESP" (추가 정보 필요)
- `title` = 마일스톤 제목 — **필수, 빈 값 금지**
- `version` = 버전 문자열 (예: "v0.1.0"), 없으면 빈 문자열 ""
- `goal` = 마일스톤 전체 목표 서술 — **필수**
- `summary` = 마일스톤 생성 요약 한 문단
- `background` = 배경 설명 (없으면 null)
- `constraints` = 제약사항 목록 (없으면 [])
- `tasks` = 작업 항목 배열 — **필수**, 형식: `[{"id": "M1-T01", "title": "작업 제목"}, ...]`
- `questions` = RESP 시 확인 질문 목록 (없으면 [])"#,

        "planning" => r#"

---

## API 출력 형식 요약 (submit_report 함수)

submit_report 호출 시 모든 필드를 채우세요:
- `role` = "planning", `task_id`, `cycle`, `status` ("NEXT"/"PREV"/"RESP")
- `summary` = 작업 요약 (필수), `implementation_plan` = 구현 단계 배열
- `dod_checklist` = 완료 기준 배열, `risks` = 리스크 목록"#,

        "development" => r#"

---

## API 출력 형식 요약 (submit_report 함수)

submit_report 호출 시 모든 필드를 채우세요:
- `role` = "development", `task_id`, `cycle`, `status` ("NEXT"/"PREV"/"RESP")
- `summary` = 구현 완료 요약 (필수), `files_changed` = 변경 파일 목록"#,

        "testing" => r#"

---

## API 출력 형식 요약 (submit_report 함수)

submit_report 호출 시 모든 필드를 채우세요:
- `role` = "testing", `task_id`, `cycle`, `status` ("NEXT"/"PREV"/"RESP")
- `summary` = 테스트 결과 요약 (필수), `test_cases` = 테스트 케이스 배열"#,

        "review" => r#"

---

## API 출력 형식 요약 (submit_report 함수)

submit_report 호출 시 모든 필드를 채우세요:
- `role` = "review", `task_id`, `cycle`, `status` ("NEXT"/"PREV"/"RESP")
- `review_status` = "APPROVED" | "CHANGES_REQUESTED"
- `summary` = 리뷰 요약 (필수), `findings` = 발견 사항 배열, `milestone_complete` = true/false"#,

        _ => "",
    }
}

/// JSON 파싱 시도 (전체 JSON → ```json 블록 → 첫 번째 { ... } 추출 순서로 시도).
pub fn try_parse_json_output(raw: &str, role: &str) -> Option<RoleOutputData> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::input::{MilestoneInfo, SessionInput};

    #[test]
    fn build_context_text_includes_milestone_when_present() {
        let mut input = SessionInput::default();
        input.role = "planning".to_string();
        input.task_id = "M5-T01".to_string();
        input.milestone = MilestoneInfo {
            id: "M5".to_string(),
            title: "파이프라인 완결".to_string(),
            version: "v0.8.0".to_string(),
            goal: "7개 이슈 해결".to_string(),
        };
        let text = build_context_text(&input);
        assert!(text.contains("## 마일스톤"), "마일스톤 섹션 없음");
        assert!(text.contains("M5"), "마일스톤 ID 없음");
        assert!(text.contains("v0.8.0"), "버전 없음");
        assert!(text.contains("파이프라인 완결"), "제목 없음");
        assert!(text.contains("7개 이슈 해결"), "목표 없음");
    }

    #[test]
    fn build_context_text_omits_milestone_when_empty() {
        let mut input = SessionInput::default();
        input.role = "planning".to_string();
        input.task_id = "M0-T00".to_string();
        let text = build_context_text(&input);
        assert!(!text.contains("## 마일스톤"), "빈 마일스톤이 출력됨");
    }

    #[test]
    fn build_context_text_omits_goal_when_empty() {
        let mut input = SessionInput::default();
        input.milestone = MilestoneInfo {
            id: "M5".to_string(),
            title: "제목만".to_string(),
            version: String::new(),
            goal: String::new(),
        };
        let text = build_context_text(&input);
        assert!(text.contains("## 마일스톤"));
        assert!(!text.contains("### 목표"), "goal이 비어있으면 목표 섹션 생략");
    }

    #[test]
    fn try_parse_json_output_whole_json() {
        let json = r#"{"role":"planning","task_id":"M1-T01","cycle":1,"status":"NEXT","summary":"test","questions":[],"prev_reason":null,"implementation_plan":[],"dod_checklist":[],"risks":[]}"#;
        let result = try_parse_json_output(json, "planning");
        assert!(result.is_some());
        if let Some(RoleOutputData::Planning(p)) = result {
            use crate::session::output::ExitCode;
            assert_eq!(p.status, ExitCode::Next);
        }
    }

    #[test]
    fn try_parse_json_output_code_block() {
        let raw = "Some text\n```json\n{\"role\":\"planning\",\"task_id\":\"M1-T01\",\"cycle\":1,\"status\":\"NEXT\",\"summary\":\"test\",\"questions\":[],\"prev_reason\":null,\"implementation_plan\":[],\"dod_checklist\":[],\"risks\":[]}\n```\nMore text";
        let result = try_parse_json_output(raw, "planning");
        assert!(result.is_some());
    }
}
