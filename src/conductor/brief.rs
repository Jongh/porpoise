//! Brief — 작업 지시서 빌더.
//!
//! Dispatch 단계에 앞서 Porpoise가 결정론적으로 조립하는 단일 작업 지시서.
//! project.md 컨텍스트 · 마일스톤 목표 · DoD · 규약 · 기술 스택 · 현재 task를
//! 하나의 프롬프트로 묶어 실제 코딩 에이전트에게 통째로 위임한다.
//!
//! LLM 호출 없이 순수하게 조립되므로 단위 테스트가 용이하다.

use std::path::Path;

use crate::config::workspace::WorkspaceConfig;

/// 한 task에 대한 작업 지시서. `render()`로 에이전트 프롬프트 문자열을 생성한다.
#[derive(Debug, Clone, Default)]
pub struct Brief {
    pub task_id: String,
    pub task_title: String,
    pub language: String,
    pub project_summary: String,
    pub milestone_id: String,
    pub milestone_title: String,
    pub milestone_version: String,
    pub milestone_goal: String,
    pub tech_context: Option<String>,
    pub conventions: Vec<String>,
    pub dod: Vec<String>,
    /// 재투입(re-dispatch) 시 직전 검증자 피드백 (최신순). 빈 경우 최초 투입.
    pub verifier_feedback: Vec<String>,
}

impl Brief {
    /// 에이전트에게 전달할 프롬프트 문자열을 생성한다.
    pub fn render(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        parts.push(format!(
            "당신은 자율 소프트웨어 엔지니어링 에이전트입니다. 아래 단일 작업(task)을 \
             처음부터 끝까지 책임지고 완수하세요 — 계획 수립, 코드 작성, 테스트, 자기 검토를 \
             당신의 판단으로 수행합니다. 응답 언어: {}.",
            if self.language.is_empty() { "ko" } else { &self.language }
        ));

        parts.push(format!(
            "=== 현재 작업 ===\n{}: {}",
            self.task_id, self.task_title
        ));

        if !self.milestone_id.is_empty() || !self.milestone_goal.is_empty() {
            let mut lines = vec!["=== 마일스톤 ===".to_string()];
            if !self.milestone_id.is_empty() {
                let ver = if self.milestone_version.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", self.milestone_version)
                };
                lines.push(format!("{}{} {}", self.milestone_id, ver, self.milestone_title));
            }
            if !self.milestone_goal.is_empty() {
                lines.push(format!("목표:\n{}", self.milestone_goal));
            }
            parts.push(lines.join("\n"));
        }

        if !self.project_summary.is_empty() {
            parts.push(format!("=== 프로젝트 컨텍스트 (project.md) ===\n{}", self.project_summary));
        }

        if let Some(ref tech) = self.tech_context {
            if !tech.is_empty() {
                parts.push(format!("=== 기술 스택 ===\n{}", tech));
            }
        }

        if !self.conventions.is_empty() {
            let lines = self
                .conventions
                .iter()
                .map(|c| format!("- {}", c))
                .collect::<Vec<_>>()
                .join("\n");
            parts.push(format!("=== 컨벤션 ===\n{}", lines));
        }

        if !self.dod.is_empty() {
            let lines = self
                .dod
                .iter()
                .map(|d| format!("- {}", d))
                .collect::<Vec<_>>()
                .join("\n");
            parts.push(format!(
                "=== 완료 기준 (DoD) ===\n이 작업은 아래 기준을 모두 충족해야 합니다:\n{}",
                lines
            ));
        }

        if !self.verifier_feedback.is_empty() {
            // 최초 피드백이 가장 최신 — 앞에서부터 번호 부여
            let blocks = self
                .verifier_feedback
                .iter()
                .enumerate()
                .map(|(i, f)| format!("[재검토 {}] {}", i + 1, f))
                .collect::<Vec<_>>()
                .join("\n\n");
            parts.push(format!(
                "=== 직전 검증 피드백 (반드시 반영) ===\n이전 시도가 검증을 통과하지 못했습니다. \
                 아래 지적 사항을 우선적으로 해결하세요:\n{}",
                blocks
            ));
        }

        parts.push(
            "=== 지침 ===\n\
             1. 이 task의 범위에만 집중하세요 (다른 task로 범위를 넓히지 마세요).\n\
             2. 프로젝트의 기존 코드 스타일과 컨벤션을 따르세요.\n\
             3. 작업 완료 후 위 DoD를 스스로 점검하세요.\n\
             4. 변경한 파일과 수행한 작업을 마지막에 요약하세요."
                .to_string(),
        );

        parts.join("\n\n")
    }

    /// 재투입을 위해 검증자 피드백을 추가한 사본을 반환한다.
    pub fn with_feedback(&self, feedback: &str) -> Brief {
        let mut next = self.clone();
        next.verifier_feedback.insert(0, feedback.to_string());
        next
    }
}

/// 현재 task에 대한 Brief를 조립한다.
pub fn build_brief(
    path: &Path,
    task_id: &str,
    task_title: &str,
    workspace: &WorkspaceConfig,
) -> Brief {
    let project_summary =
        std::fs::read_to_string(path.join(".porpoise").join("project.md")).unwrap_or_default();

    let milestone_id = task_id.split('-').next().unwrap_or("").to_string();
    let (milestone_title, milestone_version, milestone_goal) =
        load_milestone_info(path, &milestone_id);

    Brief {
        task_id: task_id.to_string(),
        task_title: task_title.to_string(),
        language: workspace.language().to_string(),
        project_summary,
        milestone_id,
        milestone_title,
        milestone_version,
        milestone_goal,
        tech_context: workspace.tech_context(),
        conventions: workspace.convention_lines(),
        dod: workspace.dod_items(),
        verifier_feedback: vec![],
    }
}

fn load_milestone_info(path: &Path, milestone_id: &str) -> (String, String, String) {
    if milestone_id.is_empty() {
        return (String::new(), String::new(), String::new());
    }
    let milestone_file = path
        .join(".porpoise")
        .join("milestones")
        .join(format!("{}.md", milestone_id));
    if !milestone_file.exists() {
        return (String::new(), String::new(), String::new());
    }
    match crate::milestone::parser::parse_milestone_file(&milestone_file) {
        Ok(m) => {
            let goal = m.raw_sections.get("목표").cloned().unwrap_or_default();
            (m.title, m.version.unwrap_or_default(), goal)
        }
        Err(_) => (String::new(), String::new(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_brief() -> Brief {
        Brief {
            task_id: "M10-T01".to_string(),
            task_title: "Brief 빌더 도입".to_string(),
            language: "ko".to_string(),
            project_summary: "포포이즈 프로젝트".to_string(),
            milestone_id: "M10".to_string(),
            milestone_title: "단일 task 지휘 루프".to_string(),
            milestone_version: "v0.19.0".to_string(),
            milestone_goal: "worker→manager 전환".to_string(),
            tech_context: Some("Rust (Cargo)".to_string()),
            conventions: vec!["unwrap 금지".to_string()],
            dod: vec!["테스트 통과".to_string()],
            verifier_feedback: vec![],
        }
    }

    #[test]
    fn render_includes_task_and_dod() {
        let brief = sample_brief();
        let out = brief.render();
        assert!(out.contains("M10-T01"));
        assert!(out.contains("Brief 빌더 도입"));
        assert!(out.contains("완료 기준"));
        assert!(out.contains("테스트 통과"));
        assert!(out.contains("Rust (Cargo)"));
        assert!(out.contains("unwrap 금지"));
        assert!(out.contains("worker→manager 전환"));
    }

    #[test]
    fn render_without_feedback_has_no_redispatch_section() {
        let out = sample_brief().render();
        assert!(!out.contains("직전 검증 피드백"));
    }

    #[test]
    fn with_feedback_adds_section_and_is_most_recent_first() {
        let brief = sample_brief().with_feedback("첫 번째 실패").with_feedback("두 번째 실패");
        let out = brief.render();
        assert!(out.contains("직전 검증 피드백"));
        // 가장 최근 피드백("두 번째 실패")이 [재검토 1]
        let idx_recent = out.find("두 번째 실패").unwrap();
        let idx_old = out.find("첫 번째 실패").unwrap();
        assert!(idx_recent < idx_old, "최신 피드백이 먼저 와야 함");
        assert_eq!(brief.verifier_feedback.len(), 2);
    }

    #[test]
    fn render_defaults_language_to_ko() {
        let mut brief = sample_brief();
        brief.language = String::new();
        assert!(brief.render().contains("ko"));
    }

    #[test]
    fn build_brief_reads_project_md_and_milestone() {
        let tmp = tempfile::tempdir().unwrap();
        let porpoise = tmp.path().join(".porpoise");
        std::fs::create_dir_all(porpoise.join("milestones")).unwrap();
        std::fs::write(porpoise.join("project.md"), "프로젝트 요약 본문").unwrap();
        std::fs::write(
            porpoise.join("milestones").join("M10.md"),
            "# M10: 지휘 루프 (v0.19.0)\n\n## 목표\n전환한다\n\n## 작업 목록\n- [ ] M10-T01: 첫 작업\n\n## 메타데이터\n- status: active\n",
        )
        .unwrap();

        let ws = WorkspaceConfig::default();
        let brief = build_brief(tmp.path(), "M10-T01", "첫 작업", &ws);
        assert_eq!(brief.task_id, "M10-T01");
        assert_eq!(brief.milestone_id, "M10");
        assert_eq!(brief.milestone_title, "지휘 루프");
        assert_eq!(brief.milestone_version, "v0.19.0");
        assert!(brief.milestone_goal.contains("전환한다"));
        assert!(brief.project_summary.contains("프로젝트 요약"));
    }

    #[test]
    fn build_brief_tolerates_missing_milestone() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise")).unwrap();
        let ws = WorkspaceConfig::default();
        let brief = build_brief(tmp.path(), "M99-T01", "고아 작업", &ws);
        assert_eq!(brief.milestone_id, "M99");
        assert_eq!(brief.milestone_title, "");
        // DoD는 기본값이 채워짐
        assert!(!brief.dod.is_empty());
    }
}
