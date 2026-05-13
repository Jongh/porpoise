use serde::{Deserialize, Serialize};
use crate::session::output::{ExitCode, default_exit_code_next};

fn default_review_status() -> String { "CHANGES_REQUESTED".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewOutput {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub cycle: u32,
    #[serde(default = "default_exit_code_next")]
    pub status: ExitCode,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub questions: Vec<String>,
    pub prev_reason: Option<String>,
    #[serde(default = "default_review_status")]
    pub review_status: String,
    #[serde(default)]
    pub findings: Vec<ReviewFinding>,
    #[serde(default)]
    pub completed_tasks: Vec<String>,
    #[serde(default)]
    pub milestone_complete: bool,
    pub prev_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewFinding {
    #[serde(default)]
    pub severity: String,
    pub file: Option<String>,
    #[serde(default)]
    pub description: String,
    pub suggestion: Option<String>,
}
