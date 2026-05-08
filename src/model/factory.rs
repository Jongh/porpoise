use anyhow::Result;
use crate::config::workspace::WorkspaceConfig;
use crate::model::adapter::{AdapterType, ModelAdapter, ModelConfig};
use crate::model::anthropic_api::AnthropicApiAdapter;
use crate::model::claude_code::ClaudeCodeAdapter;
use crate::orchestrator::state::Role;
use std::path::Path;

pub fn make_adapter(workspace: &WorkspaceConfig, project_path: &Path) -> Result<Box<dyn ModelAdapter>> {
    let adapter_type = workspace.model_adapter_type();
    match adapter_type {
        AdapterType::ClaudeCode => {
            Ok(Box::new(ClaudeCodeAdapter::new(project_path.to_path_buf())?))
        }
        AdapterType::AnthropicApi => {
            Ok(Box::new(AnthropicApiAdapter::new()))
        }
    }
}

pub fn make_model_config(workspace: &WorkspaceConfig, role: &Role) -> ModelConfig {
    let adapter_type = workspace.model_adapter_type();
    let default_model = match adapter_type {
        AdapterType::ClaudeCode => String::new(),
        AdapterType::AnthropicApi => "claude-sonnet-4-6".to_string(),
    };
    let model_id = workspace.model_id_for_role(role)
        .map(str::to_string)
        .unwrap_or(default_model);

    ModelConfig {
        model_id,
        adapter: adapter_type,
        timeout_secs: 300,
    }
}
