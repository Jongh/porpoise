use serde::{Deserialize, Serialize};
use crate::session::output::{ExitCode, default_exit_code_next};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestingOutput {
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
    pub test_cases: Vec<TestCase>,
    #[serde(default)]
    pub overall_result: String,
    #[serde(default)]
    pub issues_found: Vec<String>,
    pub regression_check: Option<RegressionCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestCase {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub result: String,
    pub command: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegressionCheck {
    #[serde(default)]
    pub total_tests: u32,
    #[serde(default)]
    pub passed: u32,
    #[serde(default)]
    pub failed: u32,
}
