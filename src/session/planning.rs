use serde::{Deserialize, Serialize};
use crate::session::output::{ExitCode, default_exit_code_next};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanningOutput {
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
    #[serde(default)]
    pub implementation_plan: Vec<PlanStep>,
    #[serde(default)]
    pub dod_checklist: Vec<DodItem>,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanStep {
    #[serde(default)]
    pub step: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub target_files: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DodItem {
    #[serde(default)]
    pub item: String,
    #[serde(default)]
    pub verification_method: String,
}
