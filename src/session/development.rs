use serde::{Deserialize, Serialize};
use crate::session::output::ExitCode;
use crate::session::v0_7::{FileOperation, VerifyCommand, AppliedOperationsSummary};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DevelopmentOutput {
    pub role: String,
    pub task_id: String,
    pub cycle: u32,
    pub status: ExitCode,
    pub summary: String,
    pub questions: Vec<String>,
    pub prev_reason: Option<String>,
    pub changes: Vec<FileChange>,
    pub test_instructions: String,
    pub known_issues: Vec<String>,
    // v0.7.0 준비 필드
    pub file_operations: Option<Vec<FileOperation>>,
    pub verify_commands: Option<Vec<VerifyCommand>>,
    pub applied_operations_summary: Option<AppliedOperationsSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileChange {
    pub file: String,
    pub change_type: String,
    pub description: String,
}
