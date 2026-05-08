#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelTemplate {
    pub display_name: &'static str,
    pub adapter: &'static str,
    pub model_id: Option<&'static str>,
    pub api_base_url: Option<&'static str>,
    pub api_key_env: Option<&'static str>,
    pub structured_output_mode: Option<&'static str>,
    pub snapshot_token_budget: Option<u32>,
}

pub static CLAUDE_CODE_DEFAULT: ModelTemplate = ModelTemplate {
    display_name: "Claude Code CLI 기본값",
    adapter: "claude_code",
    model_id: None,
    api_base_url: None,
    api_key_env: None,
    structured_output_mode: None,
    snapshot_token_budget: None,
};

pub static ANTHROPIC_CLAUDE_SONNET: ModelTemplate = ModelTemplate {
    display_name: "Anthropic API / Claude Sonnet",
    adapter: "anthropic_api",
    model_id: Some("claude-sonnet-4-6"),
    api_base_url: None,
    api_key_env: Some("ANTHROPIC_API_KEY"),
    structured_output_mode: None,
    snapshot_token_budget: Some(80_000),
};

pub static OPENAI_CODEX: ModelTemplate = ModelTemplate {
    display_name: "OpenAI-compatible / OpenAI Codex",
    adapter: "openai_compatible",
    model_id: Some("codex-mini-latest"),
    api_base_url: Some("https://api.openai.com/v1"),
    api_key_env: Some("OPENAI_API_KEY"),
    structured_output_mode: Some("function_calling"),
    snapshot_token_budget: Some(80_000),
};

pub static OLLAMA_LOCAL: ModelTemplate = ModelTemplate {
    display_name: "OpenAI-compatible / Ollama 로컬 모델",
    adapter: "openai_compatible",
    model_id: Some("gemma4:e4b"),
    api_base_url: Some("http://localhost:11434/v1"),
    api_key_env: Some(""),
    structured_output_mode: Some("json_mode"),
    snapshot_token_budget: Some(12_000),
};

pub static ALL_TEMPLATES: &[&ModelTemplate] = &[
    &CLAUDE_CODE_DEFAULT,
    &ANTHROPIC_CLAUDE_SONNET,
    &OPENAI_CODEX,
    &OLLAMA_LOCAL,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_templates_have_required_fields() {
        for t in ALL_TEMPLATES {
            assert!(!t.display_name.is_empty());
            assert!(!t.adapter.is_empty());
        }
    }

    #[test]
    fn includes_required_choice_count_before_custom() {
        assert_eq!(ALL_TEMPLATES.len(), 4);
    }
}
