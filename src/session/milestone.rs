use serde::{Deserialize, Serialize};
use crate::session::output::{ExitCode, default_exit_code_next};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MilestoneOutput {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub milestone_id: String,
    #[serde(default = "default_exit_code_next")]
    pub status: ExitCode,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub summary: String,
    pub background: Option<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<MilestoneTask>,
    #[serde(default)]
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MilestoneTask {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
}
