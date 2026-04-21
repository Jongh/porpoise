use anyhow::Result;
use chrono::Local;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub role: String,
    pub timestamp: String,
    pub content: String,
    pub requires_user_input: bool,
    pub has_critical_bugs: bool,
    pub review_status: Option<ReviewStatus>,
    pub milestone_complete: bool,
    pub questions: Vec<String>,
}

impl Report {
    pub fn stub(role: &str) -> Self {
        Report {
            role: role.to_string(),
            timestamp: Local::now().format("%Y%m%d-%H%M%S").to_string(),
            content: format!("[DRY RUN] {} role execution stub", role),
            requires_user_input: false,
            has_critical_bugs: false,
            review_status: None,
            milestone_complete: false,
            questions: vec![],
        }
    }
}

pub fn parse_report(content: &str, role: &str) -> Report {
    let content_upper = content.to_uppercase();

    // Check review status
    let review_status = if content_upper.contains("APPROVED") && !content_upper.contains("NOT APPROVED") {
        Some(ReviewStatus::Approved)
    } else if content_upper.contains("CHANGES_REQUESTED") {
        Some(ReviewStatus::ChangesRequested)
    } else if content_upper.contains("REJECTED") {
        Some(ReviewStatus::Rejected)
    } else {
        None
    };

    // Check for critical bugs
    let has_critical_bugs = content.contains("Critical") || content.contains("CRITICAL");

    // Check for user input required
    let requires_user_input = content.contains("사용자 개입 필요")
        || content.contains("USER_INPUT_REQUIRED")
        || content.contains("USER INPUT REQUIRED");

    // Check milestone completion
    let milestone_complete = content.contains("마일스톤 완료")
        || content.contains("MILESTONE_COMPLETE")
        || content.contains("milestone complete");

    // Extract questions - look for lines starting with "?" or "Q:"
    let questions: Vec<String> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with('?') || trimmed.starts_with("Q:") || trimmed.starts_with("질문:")
        })
        .map(|line| line.trim().to_string())
        .collect();

    Report {
        role: role.to_string(),
        timestamp: Local::now().format("%Y%m%d-%H%M%S").to_string(),
        content: content.to_string(),
        requires_user_input,
        has_critical_bugs,
        review_status,
        milestone_complete,
        questions,
    }
}

pub fn save_report(report: &Report, path: &Path) -> Result<PathBuf> {
    let filename = format!("{}-{}-report.md", report.timestamp, report.role);
    let report_path = path.join(".docs").join("reports").join(&filename);

    write_file(&report_path, &report.content, path)?;

    Ok(report_path)
}

