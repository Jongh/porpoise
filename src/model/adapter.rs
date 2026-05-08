use anyhow::Result;
use serde::{Deserialize, Serialize};
use crate::session::input::SessionInput;
use crate::session::output::RoleOutputData;

pub trait ModelAdapter: Send + Sync {
    fn execute(&self, input: &SessionInput, config: &ModelConfig) -> Result<RoleOutputData>;
    fn adapter_name(&self) -> &str;
    #[allow(dead_code)]
    fn supports_structured_output(&self) -> bool;
    fn last_raw_text(&self) -> Option<String> { None }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_id: String,
    pub adapter: AdapterType,
    pub timeout_secs: u32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        ModelConfig {
            model_id: String::new(),
            adapter: AdapterType::ClaudeCode,
            timeout_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdapterType {
    ClaudeCode,
    AnthropicApi,
}
