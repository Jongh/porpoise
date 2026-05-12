use serde::{Deserialize, Serialize};
use crate::session::planning::PlanningOutput;
use crate::session::development::DevelopmentOutput;
use crate::session::testing::TestingOutput;
use crate::session::review::ReviewOutput;
use crate::session::v0_7::{WorkspaceSnapshot, ExecutionResult};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionInput {
    pub role: String,
    pub task_id: String,
    pub task_title: String,
    pub cycle: u32,
    pub retry: u32,
    pub language: String,
    pub project_summary: String,
    pub conventions: Vec<String>,
    pub dod: Vec<String>,
    pub milestone: MilestoneInfo,
    pub previous_reports: PreviousReports,
    pub hints: Vec<String>,
    pub prev_reasons: Vec<String>,
    // v0.7.0 필드
    pub workspace_snapshot: Option<WorkspaceSnapshot>,
    pub execution_results: Vec<ExecutionResult>,
    pub tech_context: Option<String>,
    // v0.8.0 필드
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role_extra: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MilestoneInfo {
    pub id: String,
    pub title: String,
    pub version: String,
    pub goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreviousReports {
    pub planning: Option<PlanningOutput>,
    pub development: Option<DevelopmentOutput>,
    pub testing: Option<TestingOutput>,
    pub review: Option<ReviewOutput>,
}
