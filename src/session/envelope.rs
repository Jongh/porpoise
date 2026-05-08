use serde::{Deserialize, Serialize};
use crate::session::input::SessionInput;
use crate::session::output::RoleOutputData;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEnvelope {
    pub schema_version: String,
    pub task_id: String,
    pub role: String,
    pub cycle: u32,
    pub retry: u32,
    pub timestamp: String,
    pub model: String,
    pub adapter: String,
    pub input: SessionInput,
    pub output: Option<RoleOutputData>,
    pub raw_text: Option<String>,
}
