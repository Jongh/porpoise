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
    #[serde(default, deserialize_with = "deserialize_issues_found")]
    pub issues_found: Vec<IssueFound>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IssueFound {
    #[serde(default)]
    pub severity: String,
    pub location: Option<String>,
    #[serde(default)]
    pub description: String,
}

fn deserialize_issues_found<'de, D>(deserializer: D) -> Result<Vec<IssueFound>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|v| match v {
            serde_json::Value::String(s) => IssueFound {
                severity: "Unknown".to_string(),
                location: None,
                description: s,
            },
            obj => serde_json::from_value::<IssueFound>(obj).unwrap_or_default(),
        })
        .collect())
}
