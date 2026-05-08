use serde::{Deserialize, Serialize};
use crate::session::output::ExitCode;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestingOutput {
    pub role: String,
    pub task_id: String,
    pub cycle: u32,
    pub status: ExitCode,
    pub summary: String,
    pub questions: Vec<String>,
    pub prev_reason: Option<String>,
    pub test_cases: Vec<TestCase>,
    pub overall_result: String,
    pub issues_found: Vec<String>,
    pub regression_check: Option<RegressionCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestCase {
    pub name: String,
    pub result: String,
    pub command: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegressionCheck {
    pub total_tests: u32,
    pub passed: u32,
    pub failed: u32,
}
