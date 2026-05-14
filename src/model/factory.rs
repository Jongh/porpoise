use anyhow::Result;
use crate::config::workspace::WorkspaceConfig;
use crate::model::adapter::{AdapterType, ModelAdapter, ModelConfig};
use crate::model::anthropic_api::AnthropicApiAdapter;
use crate::model::claude_code::ClaudeCodeAdapter;
use crate::model::openai_compatible::{OpenAiCompatibleAdapter, check_ollama_availability, is_ollama_endpoint};
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
        AdapterType::OpenAiCompatible => {
            let base_url = workspace.openai_api_base_url()
                .unwrap_or("https://api.openai.com/v1")
                .to_string();
            let api_key_env = workspace.openai_api_key_env().map(str::to_string);
            let mode = workspace.structured_output_mode().to_string();

            // Ollama 서버 사전 확인
            let model_id = workspace.model_id_for_role(&Role::Developer)
                .or_else(|| workspace.model.as_ref().and_then(|m| m.model_id.as_deref()))
                .unwrap_or("unknown")
                .to_string();

            if is_ollama_endpoint(&base_url) {
                if let Err(e) = check_ollama_availability(&base_url, &model_id) {
                    return Err(e);
                }
            }

            Ok(Box::new(OpenAiCompatibleAdapter::new(base_url, api_key_env, mode)))
        }
    }
}

pub fn make_model_config(workspace: &WorkspaceConfig, role: &Role) -> ModelConfig {
    let adapter_type = workspace.model_adapter_type();
    let default_model = match adapter_type {
        AdapterType::ClaudeCode => String::new(),
        AdapterType::AnthropicApi => "claude-sonnet-4-6".to_string(),
        AdapterType::OpenAiCompatible => String::new(),
    };
    let model_id = workspace.model_id_for_role(role)
        .map(str::to_string)
        .unwrap_or(default_model);

    ModelConfig {
        model_id,
        adapter: adapter_type,
        timeout_secs: 300,
        verbose: false,
    }
}
