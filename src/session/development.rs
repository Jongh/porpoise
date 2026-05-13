use serde::{Deserialize, Serialize};
use crate::session::output::{ExitCode, default_exit_code_next};
use crate::session::v0_7::{FileOperation, VerifyCommand, AppliedOperationsSummary};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DevelopmentOutput {
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
    pub changes: Vec<FileChange>,
    #[serde(default)]
    pub test_instructions: String,
    #[serde(default)]
    pub known_issues: Vec<String>,
    // v0.7.0 준비 필드
    pub file_operations: Option<Vec<FileOperation>>,
    pub verify_commands: Option<Vec<VerifyCommand>>,
    pub applied_operations_summary: Option<AppliedOperationsSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileChange {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub change_type: String,
    #[serde(default)]
    pub description: String,
}
