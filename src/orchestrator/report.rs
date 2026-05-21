use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReviewStatus {
    Approved,
    ChangesRequested,
    Rejected,
}

impl std::fmt::Display for ReviewStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewStatus::Approved => write!(f, "APPROVED"),
            ReviewStatus::ChangesRequested => write!(f, "CHANGES_REQUESTED"),
            ReviewStatus::Rejected => write!(f, "REJECTED"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitCode {
    Prev,
    Next,
    Resp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub role: String,
    pub content: String,
    pub review_status: Option<ReviewStatus>,
    pub milestone_complete: bool,
    pub questions: Vec<String>,
    pub exit_code: Option<ExitCode>,
    /// PREV 시 복귀 대상 단계 (PORPOISE_META prev_target 필드)
    pub prev_target: Option<String>,
    /// Review 단계에서 동시 완료를 확인한 task ID 목록 (PORPOISE_META completed_tasks 필드)
    pub completed_tasks: Vec<String>,
}


/// Parses the last non-empty line of content for a PREV/NEXT/RESP exit code.
pub fn parse_exit_code(content: &str) -> Option<ExitCode> {
    let last = content.lines().rev().find(|l| !l.trim().is_empty())?;
    match last.trim() {
        "NEXT" => Some(ExitCode::Next),
        "PREV" => Some(ExitCode::Prev),
        "RESP" => Some(ExitCode::Resp),
        _ => None,
    }
}

struct MetaBlock {
    status: Option<ReviewStatus>,
    milestone_complete: bool,
    prev_target: Option<String>,
    completed_tasks: Vec<String>,
}

fn parse_meta_block(content: &str) -> Option<MetaBlock> {
    let start = content.find("<!-- PORPOISE_META")?;
    let after_tag = start + "<!-- PORPOISE_META".len();
    let end_offset = content[after_tag..].find("-->")?;
    let block = &content[after_tag..after_tag + end_offset];

    let mut status = None;
    let mut milestone_complete = false;
    let mut prev_target = None;
    let mut completed_tasks: Vec<String> = Vec::new();

    for line in block.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("status:") {
            status = match val.trim() {
                "APPROVED" => Some(ReviewStatus::Approved),
                "CHANGES_REQUESTED" => Some(ReviewStatus::ChangesRequested),
                "REJECTED" => Some(ReviewStatus::Rejected),
                _ => None,
            };
        } else if let Some(val) = line.strip_prefix("milestone_complete:") {
            milestone_complete = val.trim() == "true";
        } else if let Some(val) = line.strip_prefix("prev_target:") {
            let t = val.trim().to_string();
            if !t.is_empty() {
                prev_target = Some(t);
            }
        } else if let Some(val) = line.strip_prefix("completed_tasks:") {
            completed_tasks = val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }

    Some(MetaBlock {
        status,
        milestone_complete,
        prev_target,
        completed_tasks,
    })
}

/// Extracts the `prev_target` role string from a PORPOISE_META block, if present.
pub fn parse_prev_target(content: &str) -> Option<String> {
    parse_meta_block(content).and_then(|m| m.prev_target)
}

pub fn parse_report(content: &str, role: &str) -> Report {
    let (review_status, milestone_complete, prev_target, completed_tasks) =
        if let Some(meta) = parse_meta_block(content) {
            (meta.status, meta.milestone_complete, meta.prev_target, meta.completed_tasks)
        } else {
            let content_upper = content.to_uppercase();
            let review_status = if content_upper.contains("APPROVED")
                && !content_upper.contains("NOT APPROVED")
            {
                Some(ReviewStatus::Approved)
            } else if content_upper.contains("CHANGES_REQUESTED") {
                Some(ReviewStatus::ChangesRequested)
            } else if content_upper.contains("REJECTED") {
                Some(ReviewStatus::Rejected)
            } else {
                None
            };
            let milestone_complete = content.contains("마일스톤 완료")
                || content.contains("MILESTONE_COMPLETE")
                || content.contains("milestone complete");
            (review_status, milestone_complete, None, vec![])
        };

    let exit_code = parse_exit_code(content);

    let questions: Vec<String> = content
        .split("## 사용자 확인 필요")
        .nth(1)
        .unwrap_or("")
        .split("\n##")
        .next()
        .unwrap_or("")
        .lines()
        .filter_map(|l| {
            let trimmed = l.trim();
            // Only accept bullet lines to avoid picking up exit codes (NEXT/PREV/RESP)
            if trimmed.starts_with("- ") {
                let q = trimmed
                    .trim_start_matches("- ")
                    .trim_start_matches("Q:")
                    .trim();
                if !q.is_empty() {
                    Some(q.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    Report {
        role: role.to_string(),
        content: content.to_string(),
        review_status,
        milestone_complete,
        questions,
        exit_code,
        prev_target,
        completed_tasks,
    }
}

/// Extracts the `completed_tasks` list from a PORPOISE_META block.
pub fn parse_completed_tasks(content: &str) -> Vec<String> {
    parse_meta_block(content)
        .map(|m| m.completed_tasks)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exit_code_next() {
        assert_eq!(parse_exit_code("some content\n\nNEXT"), Some(ExitCode::Next));
    }

    #[test]
    fn parse_exit_code_prev() {
        assert_eq!(parse_exit_code("content\nPREV"), Some(ExitCode::Prev));
    }

    #[test]
    fn parse_exit_code_resp() {
        assert_eq!(parse_exit_code("content\nRESP\n"), Some(ExitCode::Resp));
    }

    #[test]
    fn parse_exit_code_trailing_whitespace() {
        assert_eq!(parse_exit_code("content\nNEXT  \n  "), Some(ExitCode::Next));
    }

    #[test]
    fn parse_exit_code_none_when_missing() {
        assert_eq!(parse_exit_code("content without code"), None);
    }

    #[test]
    fn parse_meta_block_approved() {
        let content = "Some content\n<!-- PORPOISE_META\nstatus: APPROVED\nmilestone_complete: true\n-->\nMore content\n\nNEXT";
        let report = parse_report(content, "reviewer");
        assert!(matches!(report.review_status, Some(ReviewStatus::Approved)));
        assert!(report.milestone_complete);
        assert_eq!(report.exit_code, Some(ExitCode::Next));
    }

    #[test]
    fn parse_meta_block_changes_requested() {
        let content = "<!-- PORPOISE_META\nstatus: CHANGES_REQUESTED\nmilestone_complete: false\n-->\n\nPREV";
        let report = parse_report(content, "reviewer");
        assert!(matches!(
            report.review_status,
            Some(ReviewStatus::ChangesRequested)
        ));
        assert_eq!(report.exit_code, Some(ExitCode::Prev));
    }

    #[test]
    fn parse_meta_block_rejected() {
        let content = "<!-- PORPOISE_META\nstatus: REJECTED\nmilestone_complete: false\n-->\n\nPREV";
        let report = parse_report(content, "reviewer");
        assert!(matches!(report.review_status, Some(ReviewStatus::Rejected)));
        assert_eq!(report.exit_code, Some(ExitCode::Prev));
    }

    #[test]
    fn parse_meta_overrides_heuristics() {
        let content = "APPROVED everywhere<!-- PORPOISE_META\nstatus: REJECTED\nmilestone_complete: false\n-->\n\nPREV";
        let report = parse_report(content, "reviewer");
        assert!(matches!(report.review_status, Some(ReviewStatus::Rejected)));
    }

    #[test]
    fn parse_questions_from_resp_section() {
        let content = "Report\n## 사용자 확인 필요\n- Q: 배포 환경은?\n- Q: 버전 태그?\n\nRESP";
        let report = parse_report(content, "pm");
        assert_eq!(report.exit_code, Some(ExitCode::Resp));
        assert_eq!(report.questions.len(), 2);
        assert!(report.questions[0].contains("배포 환경"));
    }

    #[test]
    fn parse_completed_tasks_single() {
        let content = "<!-- PORPOISE_META\nstatus: APPROVED\ncompleted_tasks: M1-T01\n-->\n\nNEXT";
        let tasks = parse_completed_tasks(content);
        assert_eq!(tasks, vec!["M1-T01".to_string()]);
    }

    #[test]
    fn parse_completed_tasks_multiple() {
        let content = "<!-- PORPOISE_META\nstatus: APPROVED\ncompleted_tasks: M1-T01, M1-T02, M1-T03\nmilestone_complete: false\n-->\n\nNEXT";
        let tasks = parse_completed_tasks(content);
        assert_eq!(tasks, vec!["M1-T01", "M1-T02", "M1-T03"]);
    }

    #[test]
    fn parse_completed_tasks_absent() {
        let content = "<!-- PORPOISE_META\nstatus: APPROVED\n-->\n\nNEXT";
        let tasks = parse_completed_tasks(content);
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_completed_tasks_no_meta_block() {
        let content = "Some content without meta block\n\nNEXT";
        let tasks = parse_completed_tasks(content);
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_report_includes_completed_tasks() {
        let content = "<!-- PORPOISE_META\nstatus: APPROVED\ncompleted_tasks: M2-T01, M2-T02\nmilestone_complete: false\n-->\n\nNEXT";
        let report = parse_report(content, "reviewer");
        assert_eq!(report.completed_tasks, vec!["M2-T01", "M2-T02"]);
        assert!(matches!(report.review_status, Some(ReviewStatus::Approved)));
    }

    #[test]
    fn parse_report_completed_tasks_empty_when_no_meta() {
        let content = "Review done. APPROVED\n\nNEXT";
        let report = parse_report(content, "reviewer");
        assert!(report.completed_tasks.is_empty());
    }

    #[test]
    fn parse_prev_target_present() {
        let content = "Review content\n<!-- PORPOISE_META\nstatus: CHANGES_REQUESTED\nprev_target: development\n-->\n\nPREV";
        let target = parse_prev_target(content);
        assert_eq!(target, Some("development".to_string()));
        let report = parse_report(content, "reviewer");
        assert_eq!(report.prev_target, Some("development".to_string()));
        assert_eq!(report.exit_code, Some(ExitCode::Prev));
    }

    #[test]
    fn parse_prev_target_absent() {
        let content = "Review content\n<!-- PORPOISE_META\nstatus: CHANGES_REQUESTED\n-->\n\nPREV";
        assert_eq!(parse_prev_target(content), None);
    }

    #[test]
    fn parse_prev_target_testing() {
        let content = "<!-- PORPOISE_META\nstatus: CHANGES_REQUESTED\nprev_target: testing\nmilestone_complete: false\n-->\n\nPREV";
        assert_eq!(parse_prev_target(content), Some("testing".to_string()));
    }
}
