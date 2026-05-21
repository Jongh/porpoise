#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub template: &'static ModelTemplate,
    pub model_id: Option<String>,      // Some이면 template.model_id 대신 사용
    pub api_key_env: Option<String>,   // Some이면 template.api_key_env 대신 사용
    pub api_base_url: Option<String>,  // Some이면 template.api_base_url 대신 사용
}

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
    // Claude Code는 빈 model_id를 명시해 CLI 기본 모델을 사용한다.
    model_id: Some(""),
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

pub static GROQ: ModelTemplate = ModelTemplate {
    display_name: "Groq / Llama 3.3 70B (무료 티어)",
    adapter: "openai_compatible",
    model_id: Some("llama-3.3-70b-versatile"),
    api_base_url: Some("https://api.groq.com/openai/v1"),
    api_key_env: Some("GROQ_API_KEY"),
    structured_output_mode: Some("json_mode"),
    snapshot_token_budget: Some(80_000),
};

pub static GEMINI: ModelTemplate = ModelTemplate {
    display_name: "Google Gemini API / Gemini 2.5 Flash (무료 티어)",
    adapter: "openai_compatible",
    model_id: Some("gemini-2.5-flash"),
    api_base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai"),
    api_key_env: Some("GEMINI_API_KEY"),
    structured_output_mode: Some("json_mode"),
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
    &GROQ,
    &GEMINI,
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
        assert_eq!(ALL_TEMPLATES.len(), 6);
    }

    #[test]
    fn groq_template_fields() {
        assert_eq!(GROQ.adapter, "openai_compatible");
        assert_eq!(GROQ.model_id, Some("llama-3.3-70b-versatile"));
        assert_eq!(GROQ.api_base_url, Some("https://api.groq.com/openai/v1"));
        assert_eq!(GROQ.api_key_env, Some("GROQ_API_KEY"));
        assert_eq!(GROQ.structured_output_mode, Some("json_mode"));
        assert!(GROQ.snapshot_token_budget.is_some());
    }

    #[test]
    fn gemini_template_fields() {
        assert_eq!(GEMINI.adapter, "openai_compatible");
        assert_eq!(GEMINI.model_id, Some("gemini-2.5-flash"));
        assert_eq!(
            GEMINI.api_base_url,
            Some("https://generativelanguage.googleapis.com/v1beta/openai")
        );
        assert_eq!(GEMINI.api_key_env, Some("GEMINI_API_KEY"));
        assert_eq!(GEMINI.structured_output_mode, Some("json_mode"));
        assert!(GEMINI.snapshot_token_budget.is_some());
    }

    #[test]
    fn groq_gemini_appear_in_all_templates() {
        let has_groq = ALL_TEMPLATES.iter().any(|t| t.display_name.contains("Groq"));
        let has_gemini = ALL_TEMPLATES.iter().any(|t| t.display_name.contains("Gemini"));
        assert!(has_groq, "ALL_TEMPLATES에 Groq 템플릿이 없음");
        assert!(has_gemini, "ALL_TEMPLATES에 Gemini 템플릿이 없음");
    }
}
