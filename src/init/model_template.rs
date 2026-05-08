#[derive(Debug, Clone, Copy)]
pub struct ModelTemplate {
    pub display_name: &'static str,
    pub adapter: &'static str,
    pub model_id: &'static str,
    pub api_base_url: Option<&'static str>,
    pub api_key_env: Option<&'static str>,
    pub structured_output_mode: Option<&'static str>,
    pub snapshot_token_budget: Option<u32>,
}

pub const CLAUDE_CODE: ModelTemplate = ModelTemplate {
    display_name: "Claude Code (Sonnet 4.6)",
    adapter: "claude_code",
    model_id: "claude-sonnet-4-6",
    api_base_url: None,
    api_key_env: None,
    structured_output_mode: None,
    snapshot_token_budget: None,
};

pub const OPENAI_CODEX: ModelTemplate = ModelTemplate {
    display_name: "OpenAI-compatible (OpenAI Codex)",
    adapter: "openai_compatible",
    model_id: "codex-mini-latest",
    api_base_url: Some("https://api.openai.com/v1"),
    api_key_env: Some("OPENAI_API_KEY"),
    structured_output_mode: Some("function_calling"),
    snapshot_token_budget: Some(80_000),
};

pub const OLLAMA_GEMMA: ModelTemplate = ModelTemplate {
    display_name: "OpenAI-compatible (Ollama gemma4:e4b)",
    adapter: "openai_compatible",
    model_id: "gemma4:e4b",
    api_base_url: Some("http://localhost:11434/v1"),
    api_key_env: Some(""),
    structured_output_mode: Some("json_mode"),
    snapshot_token_budget: Some(12_000),
};

pub const ALL_TEMPLATES: &[ModelTemplate] = &[CLAUDE_CODE, OPENAI_CODEX, OLLAMA_GEMMA];
