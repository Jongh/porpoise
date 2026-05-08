use serde::{Deserialize, Serialize};
use crate::session::output::ExitCode;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanningOutput {
    pub role: String,
    pub task_id: String,
    pub cycle: u32,
    pub status: ExitCode,
    pub summary: String,
    pub questions: Vec<String>,
    pub prev_reason: Option<String>,
    pub implementation_plan: Vec<PlanStep>,
    pub dod_checklist: Vec<DodItem>,
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanStep {
    pub step: u32,
    pub description: String,
    pub target_files: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DodItem {
    pub item: String,
    pub verification_method: String,
}
