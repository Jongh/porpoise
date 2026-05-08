// v0.7.0 준비 구조체 - 현재 항상 None/빈 Vec로 사용됨
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceSnapshot {
    pub file_tree: String,
    pub files: Vec<SnapshotFile>,
    pub recent_git_diff: Option<String>,
    pub untracked_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotFile {
    pub path: String,
    pub content: Option<String>,
    pub summary: Option<String>,
    pub size_bytes: u64,
    pub last_modified: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileOperation {
    pub op: String,
    pub path: String,
    pub content: Option<String>,
    pub patch: Option<String>,
    pub patch_format: Option<String>,
    pub new_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerifyCommand {
    pub command: String,
    pub args: Vec<String>,
    pub purpose: String,
    pub expected_exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionResult {
    pub command: String,
    pub args: Vec<String>,
    pub purpose: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppliedOperationsSummary {
    pub files_written: u32,
    pub files_deleted: u32,
    pub files_renamed: u32,
    pub commands_run: u32,
    pub all_commands_passed: bool,
}
