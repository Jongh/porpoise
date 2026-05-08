use serde::{Deserialize, Serialize};
use crate::session::output::ExitCode;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewOutput {
    pub role: String,
    pub task_id: String,
    pub cycle: u32,
    pub status: ExitCode,
    pub summary: String,
    pub questions: Vec<String>,
    pub prev_reason: Option<String>,
    pub review_status: String,
    pub findings: Vec<ReviewFinding>,
    pub completed_tasks: Vec<String>,
    pub milestone_complete: bool,
    pub prev_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewFinding {
    pub severity: String,
    pub file: Option<String>,
    pub description: String,
    pub suggestion: Option<String>,
}
