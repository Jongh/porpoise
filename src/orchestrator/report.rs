use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::utils::fs::write_file;

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
    pub requires_user_input: bool,
    pub has_critical_bugs: bool,
    pub review_status: Option<ReviewStatus>,
    pub milestone_complete: bool,
    pub questions: Vec<String>,
    pub exit_code: Option<ExitCode>,
}

impl Report {
    pub fn stub(role: &str) -> Self {
        Report {
            role: role.to_string(),
            content: format!("[DRY RUN] {} role execution stub", role),
            requires_user_input: false,
            has_critical_bugs: false,
            review_status: Some(ReviewStatus::Approved),
            milestone_complete: false,
            questions: vec![],
            exit_code: Some(ExitCode::Next),
        }
    }
}

/// Returns the standardised report filename for a given task, role, cycle, and retry.
pub fn report_filename(task_id: &str, role: &str, cycle: u32, retry: u32) -> String {
    format!("{}-{}-C{}-R{}.md", task_id, role, cycle, retry)
}

/// Counts existing report files for the given task+role+cycle combination.
/// Used to determine the next retry number before executing a role.
pub fn count_existing_reports(reports_dir: &Path, task_id: &str, role: &str, cycle: u32) -> u32 {
    let prefix = format!("{}-{}-C{}-R", task_id, role, cycle);
    if let Ok(entries) = std::fs::read_dir(reports_dir) {
        entries
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(&prefix)
                    && name.ends_with(".md")
                    && !name.contains("-resp")
            })
            .count() as u32
    } else {
        0
    }
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
    critical_bugs: bool,
    user_input_required: bool,
    milestone_complete: bool,
}

fn parse_meta_block(content: &str) -> Option<MetaBlock> {
    let start = content.find("<!-- PORPOISE_META")?;
    let after_tag = start + "<!-- PORPOISE_META".len();
    let end_offset = content[after_tag..].find("-->")?;
    let block = &content[after_tag..after_tag + end_offset];

    let mut status = None;
    let mut critical_bugs = false;
    let mut user_input_required = false;
    let mut milestone_complete = false;

    for line in block.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("status:") {
            status = match val.trim() {
                "APPROVED" => Some(ReviewStatus::Approved),
                "CHANGES_REQUESTED" => Some(ReviewStatus::ChangesRequested),
                "REJECTED" => Some(ReviewStatus::Rejected),
                _ => None,
            };
        } else if let Some(val) = line.strip_prefix("critical_bugs:") {
            critical_bugs = val.trim() == "true";
        } else if let Some(val) = line.strip_prefix("user_input_required:") {
            user_input_required = val.trim() == "true";
        } else if let Some(val) = line.strip_prefix("milestone_complete:") {
            milestone_complete = val.trim() == "true";
        }
    }

    Some(MetaBlock {
        status,
        critical_bugs,
        user_input_required,
        milestone_complete,
    })
}

pub fn parse_report(content: &str, role: &str) -> Report {
    let (review_status, has_critical_bugs, requires_user_input, milestone_complete) =
        if let Some(meta) = parse_meta_block(content) {
            (
                meta.status,
                meta.critical_bugs,
                meta.user_input_required,
                meta.milestone_complete,
            )
        } else {
            // Heuristic fallback (no META block): parse review_status from text,
            // but do NOT infer has_critical_bugs from keywords (BUG-A fix).
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
            let requires_user_input = content.contains("사용자 확인 필요")
                || content.contains("USER_INPUT_REQUIRED")
                || content.contains("USER INPUT REQUIRED");
            let milestone_complete = content.contains("마일스톤 완료")
                || content.contains("MILESTONE_COMPLETE")
                || content.contains("milestone complete");
            (review_status, false, requires_user_input, milestone_complete)
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
        requires_user_input,
        has_critical_bugs,
        review_status,
        milestone_complete,
        questions,
        exit_code,
    }
}

pub fn save_report(
    report: &Report,
    path: &Path,
    task_id: &str,
    cycle: u32,
    retry: u32,
) -> Result<PathBuf> {
    let filename = report_filename(task_id, &report.role, cycle, retry);
    let report_path = path.join(".docs").join("reports").join(&filename);
    write_file(&report_path, &report.content, path)?;
    Ok(report_path)
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
        let content = "Some content\n<!-- PORPOISE_META\nstatus: APPROVED\ncritical_bugs: false\nuser_input_required: false\nmilestone_complete: true\n-->\nMore content\n\nNEXT";
        let report = parse_report(content, "reviewer");
        assert!(matches!(report.review_status, Some(ReviewStatus::Approved)));
        assert!(!report.has_critical_bugs);
        assert!(!report.requires_user_input);
        assert!(report.milestone_complete);
        assert_eq!(report.exit_code, Some(ExitCode::Next));
    }

    #[test]
    fn parse_meta_block_changes_requested() {
        let content = "<!-- PORPOISE_META\nstatus: CHANGES_REQUESTED\ncritical_bugs: true\nuser_input_required: true\nmilestone_complete: false\n-->\n\nPREV";
        let report = parse_report(content, "reviewer");
        assert!(matches!(
            report.review_status,
            Some(ReviewStatus::ChangesRequested)
        ));
        assert!(report.has_critical_bugs);
        assert!(report.requires_user_input);
        assert_eq!(report.exit_code, Some(ExitCode::Prev));
    }

    #[test]
    fn parse_meta_block_rejected() {
        let content = "<!-- PORPOISE_META\nstatus: REJECTED\ncritical_bugs: false\nuser_input_required: false\nmilestone_complete: false\n-->\n\nPREV";
        let report = parse_report(content, "reviewer");
        assert!(matches!(report.review_status, Some(ReviewStatus::Rejected)));
        assert_eq!(report.exit_code, Some(ExitCode::Prev));
    }

    #[test]
    fn no_critical_bug_keyword_heuristic() {
        // "Critical" keyword alone must NOT set has_critical_bugs (BUG-A fix)
        let content = "Found Critical issues in the code\n\nNEXT";
        let report = parse_report(content, "tester");
        assert!(!report.has_critical_bugs);
        assert_eq!(report.exit_code, Some(ExitCode::Next));
    }

    #[test]
    fn parse_meta_overrides_heuristics() {
        let content = "APPROVED everywhere<!-- PORPOISE_META\nstatus: REJECTED\ncritical_bugs: false\nuser_input_required: false\nmilestone_complete: false\n-->\n\nPREV";
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
    fn report_filename_format() {
        assert_eq!(
            report_filename("M1-T01", "pm", 1, 0),
            "M1-T01-pm-C1-R0.md"
        );
        assert_eq!(
            report_filename("M1-T01", "developer", 2, 1),
            "M1-T01-developer-C2-R1.md"
        );
    }

    #[test]
    fn stub_has_next_exit_code() {
        let report = Report::stub("pm");
        assert_eq!(report.exit_code, Some(ExitCode::Next));
        assert!(matches!(report.review_status, Some(ReviewStatus::Approved)));
    }
}
