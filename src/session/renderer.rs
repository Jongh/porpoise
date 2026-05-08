use crate::session::envelope::SessionEnvelope;
use crate::session::input::SessionInput;
use crate::session::output::RoleOutputData;
use crate::session::planning::PlanningOutput;
use crate::session::development::DevelopmentOutput;
use crate::session::testing::TestingOutput;
use crate::session::review::ReviewOutput;
use crate::session::milestone::MilestoneOutput;
use anyhow::Result;
use std::path::Path;

pub fn render_and_save_report(path: &Path, envelope: &SessionEnvelope) -> Result<()> {
    use crate::orchestrator::state::TaskId;
    let reports_dir = path.join(".porpoise").join("reports");
    std::fs::create_dir_all(&reports_dir)?;
    let normalized_id = TaskId::new(&envelope.task_id);
    let filename = format!(
        "{}-{}-C{}-R{}.md",
        normalized_id, envelope.role, envelope.cycle, envelope.retry
    );
    let content = render_session(envelope);
    std::fs::write(reports_dir.join(&filename), content)?;
    Ok(())
}

pub fn render_session(envelope: &SessionEnvelope) -> String {
    let header = format!(
        "# {} 보고서: {} / 사이클 {}\n\n**모델**: {}  **어댑터**: {}  **시각**: {}\n",
        envelope.role, envelope.task_id, envelope.cycle,
        envelope.model, envelope.adapter, envelope.timestamp
    );
    let body = match &envelope.output {
        Some(RoleOutputData::Planning(o)) => render_planning(o, &envelope.input),
        Some(RoleOutputData::Development(o)) => render_development(o, &envelope.input),
        Some(RoleOutputData::Testing(o)) => render_testing(o, &envelope.input),
        Some(RoleOutputData::Review(o)) => render_review(o, &envelope.input),
        Some(RoleOutputData::Milestone(o)) => render_milestone(o),
        None => "(세션 미완료)".to_string(),
    };
    format!("{}\n{}", header, body)
}

pub fn render_planning(output: &PlanningOutput, _input: &SessionInput) -> String {
    let mut lines = Vec::new();
    lines.push(format!("## 요약\n{}", output.summary));
    if !output.implementation_plan.is_empty() {
        lines.push("## 구현 계획".to_string());
        for step in &output.implementation_plan {
            lines.push(format!("{}. {}", step.step, step.description));
            for f in &step.target_files {
                lines.push(format!("   - 파일: {}", f));
            }
        }
    }
    if !output.dod_checklist.is_empty() {
        lines.push("## DoD 체크리스트".to_string());
        for item in &output.dod_checklist {
            lines.push(format!("- [ ] {} (검증: {})", item.item, item.verification_method));
        }
    }
    if !output.risks.is_empty() {
        lines.push("## 리스크".to_string());
        for r in &output.risks {
            lines.push(format!("- {}", r));
        }
    }
    lines.push(format!("\n## 상태\n{}", output.status));
    lines.join("\n")
}

pub fn render_development(output: &DevelopmentOutput, _input: &SessionInput) -> String {
    let mut lines = Vec::new();
    lines.push(format!("## 요약\n{}", output.summary));
    if !output.changes.is_empty() {
        lines.push("## 변경 사항".to_string());
        for c in &output.changes {
            lines.push(format!("- `{}` [{}]: {}", c.file, c.change_type, c.description));
        }
    }
    if !output.test_instructions.is_empty() {
        lines.push(format!("## 테스트 지시사항\n{}", output.test_instructions));
    }
    if !output.known_issues.is_empty() {
        lines.push("## 알려진 이슈".to_string());
        for i in &output.known_issues {
            lines.push(format!("- {}", i));
        }
    }
    lines.push(format!("\n## 상태\n{}", output.status));
    lines.join("\n")
}

pub fn render_testing(output: &TestingOutput, _input: &SessionInput) -> String {
    let mut lines = Vec::new();
    lines.push(format!("## 요약\n{}", output.summary));
    lines.push(format!("## 전체 결과\n{}", output.overall_result));
    if !output.test_cases.is_empty() {
        lines.push("## 테스트 케이스".to_string());
        for tc in &output.test_cases {
            let icon = match tc.result.as_str() {
                "pass" => "✓",
                "fail" => "✗",
                _ => "~",
            };
            lines.push(format!("{} {}", icon, tc.name));
        }
    }
    if let Some(ref rc) = output.regression_check {
        lines.push(format!(
            "\n## 회귀 테스트\n전체: {} | 통과: {} | 실패: {}",
            rc.total_tests, rc.passed, rc.failed
        ));
    }
    lines.push(format!("\n## 상태\n{}", output.status));
    lines.join("\n")
}

pub fn render_review(output: &ReviewOutput, _input: &SessionInput) -> String {
    let mut lines = Vec::new();
    lines.push(format!("## 요약\n{}", output.summary));
    lines.push(format!("## 리뷰 결과\n{}", output.review_status));
    if !output.findings.is_empty() {
        lines.push("## 발견사항".to_string());
        for f in &output.findings {
            let file_info = f.file.as_deref().map(|f| format!(" `{}`", f)).unwrap_or_default();
            lines.push(format!("- [{}]{}: {}", f.severity, file_info, f.description));
        }
    }
    if !output.completed_tasks.is_empty() {
        lines.push(format!("## 완료된 작업\n{}", output.completed_tasks.join(", ")));
    }

    // 레거시 호환을 위한 PORPOISE_META 블록
    lines.push(format!(
        "\n<!-- PORPOISE_META\nstatus: {}\nmilestone_complete: {}\nprev_target: {}\ncompleted_tasks: {}\n-->",
        output.review_status,
        output.milestone_complete,
        output.prev_target.as_deref().unwrap_or(""),
        output.completed_tasks.join(", ")
    ));

    lines.push(format!("\n{}", output.status));
    lines.join("\n")
}

pub fn render_milestone(output: &MilestoneOutput) -> String {
    let mut lines = Vec::new();
    lines.push(format!("## 마일스톤: {} — {}", output.milestone_id, output.title));
    lines.push(format!("**버전**: {}\n**목표**: {}", output.version, output.goal));
    if !output.tasks.is_empty() {
        lines.push("## 작업 목록".to_string());
        for t in &output.tasks {
            lines.push(format!("- [ ] {}: {}", t.id, t.title));
        }
    }
    lines.push(format!("\n## 상태\n{}", output.status));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::output::ExitCode;
    use crate::session::planning::{PlanningOutput, PlanStep};

    #[test]
    fn planning_render_contains_plan_steps() {
        let output = PlanningOutput {
            role: "planning".to_string(),
            task_id: "M1-T01".to_string(),
            cycle: 1,
            status: ExitCode::Next,
            summary: "테스트 요약".to_string(),
            implementation_plan: vec![PlanStep {
                step: 1,
                description: "step 1 desc".to_string(),
                target_files: vec!["src/main.rs".to_string()],
                acceptance_criteria: vec![],
            }],
            ..Default::default()
        };
        let input = SessionInput::default();
        let rendered = render_planning(&output, &input);
        assert!(rendered.contains("## 구현 계획"));
        assert!(rendered.contains("step 1 desc"));
        assert!(rendered.contains("NEXT"));
    }

    #[test]
    fn review_render_has_porpoise_meta() {
        use crate::session::review::ReviewOutput;
        let output = ReviewOutput {
            role: "review".to_string(),
            task_id: "M1-T01".to_string(),
            cycle: 1,
            status: ExitCode::Next,
            summary: "OK".to_string(),
            review_status: "APPROVED".to_string(),
            completed_tasks: vec!["M1-T01".to_string()],
            milestone_complete: false,
            ..Default::default()
        };
        let input = SessionInput::default();
        let rendered = render_review(&output, &input);
        assert!(rendered.contains("<!-- PORPOISE_META"));
        assert!(rendered.contains("APPROVED"));
    }

    #[test]
    fn review_render_last_line_is_exit_code() {
        use crate::session::review::ReviewOutput;
        let output = ReviewOutput {
            status: ExitCode::Next,
            review_status: "APPROVED".to_string(),
            ..Default::default()
        };
        let input = SessionInput::default();
        let rendered = render_review(&output, &input);
        let last_line = rendered.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        assert_eq!(last_line.trim(), "NEXT");
    }
}
