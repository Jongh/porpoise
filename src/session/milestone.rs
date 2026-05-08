use serde::{Deserialize, Serialize};
use crate::session::output::ExitCode;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MilestoneOutput {
    pub role: String,
    pub milestone_id: String,
    pub status: ExitCode,
    pub title: String,
    pub version: String,
    pub goal: String,
    pub summary: String,
    pub background: Option<String>,
    pub constraints: Vec<String>,
    pub tasks: Vec<MilestoneTask>,
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MilestoneTask {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
}
