use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::sync::Mutex;

use crate::model::adapter::{ModelAdapter, ModelConfig};
use crate::model::context::{api_json_format_hint, build_context_text, parse_role_output_from_value, try_parse_json_output};
use crate::session::input::SessionInput;
use crate::session::output::RoleOutputData;

// role별 JSON schema — AnthropicApiAdapter와 동일 파일 재사용
const PLANNING_SCHEMA: &str = include_str!("schemas/planning_schema.json");
const DEVELOPMENT_SCHEMA: &str = include_str!("schemas/development_schema.json");
const TESTING_SCHEMA: &str = include_str!("schemas/testing_schema.json");
const REVIEW_SCHEMA: &str = include_str!("schemas/review_schema.json");
const MILESTONE_SCHEMA: &str = include_str!("schemas/milestone_schema.json");

// 단계 프롬프트 템플릿 임베딩
const PLANNING_PROMPT: &str = include_str!("../init/prompts/01-planning-api.tmpl");
const DEVELOPMENT_PROMPT: &str = include_str!("../init/prompts/02-development-api.tmpl");
const TESTING_PROMPT: &str = include_str!("../init/prompts/03-testing-api.tmpl");
const REVIEW_PROMPT: &str = include_str!("../init/prompts/04-review-api.tmpl");
const MILESTONE_PROMPT: &str = include_str!("../init/prompts/05-milestone-api.tmpl");

fn get_role_system_prompt(role: &str) -> &'static str {
    match role {
        "planning" => PLANNING_PROMPT,
        "development" => DEVELOPMENT_PROMPT,
        "testing" => TESTING_PROMPT,
        "review" => REVIEW_PROMPT,
        "milestone" => MILESTONE_PROMPT,
        _ => "",
    }
}

fn resolve_role_system_prompt(role: &str, role_extra: &str) -> String {
    let base = get_role_system_prompt(role)
        .replace("{{role_extra}}", role_extra)
        .trim()
        .to_string();
    let hint = api_json_format_hint(role);
    if hint.is_empty() {
        base
    } else {
        format!("{}{}", base, hint)
    }
}

pub struct OpenAiCompatibleAdapter {
    api_base_url: String,
    api_key_env: Option<String>,
    structured_output_mode: String,
    raw_text: Mutex<Option<String>>,
}

impl OpenAiCompatibleAdapter {
    pub fn new(
        api_base_url: String,
        api_key_env: Option<String>,
        structured_output_mode: String,
    ) -> Self {
        OpenAiCompatibleAdapter {
            api_base_url,
            api_key_env,
            structured_output_mode,
            raw_text: Mutex::new(None),
        }
    }

    fn get_api_key(&self) -> Option<String> {
        match &self.api_key_env {
            None => std::env::var("OPENAI_API_KEY").ok(),
            Some(env_name) if env_name.is_empty() => None,  // 무인증 (Ollama)
            Some(env_name) => std::env::var(env_name).ok(),
        }
    }

    fn chat_completions_url(&self) -> String {
        let base = self.api_base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    }

    fn get_tool_schema(role: &str) -> &'static str {
        match role {
            "planning" => PLANNING_SCHEMA,
            "development" => DEVELOPMENT_SCHEMA,
            "testing" => TESTING_SCHEMA,
            "review" => REVIEW_SCHEMA,
            "milestone" => MILESTONE_SCHEMA,
            _ => PLANNING_SCHEMA,
        }
    }
}

pub fn check_ollama_availability(base_url: &str, model_id: &str) -> Result<()> {
    // api_tags_url: http://host:port/api/tags
    let origin = extract_origin(base_url);
    let tags_url = format!("{}/api/tags", origin);

    let response = ureq::get(&tags_url)
        .timeout(std::time::Duration::from_secs(3))
        .call();

    match response {
        Err(_) => bail!(
            "Ollama 서버 미응답: {}\n  'ollama serve' 실행 후 재시도하세요.",
            tags_url
        ),
        Ok(resp) => {
            let body: serde_json::Value = resp.into_json()
                .context("Ollama /api/tags 응답 파싱 실패")?;
            let models = body["models"].as_array();
            let found = models.map(|arr| {
                arr.iter().any(|m| {
                    m["name"].as_str().map(|n| n == model_id || n.starts_with(&format!("{}:", model_id))).unwrap_or(false)
                })
            }).unwrap_or(false);
            if !found {
                bail!(
                    "Ollama 모델 '{}' 없음\n  'ollama pull {}' 실행 후 재시도하세요.",
                    model_id, model_id
                );
            }
            Ok(())
        }
    }
}

pub fn is_ollama_endpoint(url: &str) -> bool {
    url.contains(":11434") || url.to_lowercase().contains("ollama")
}

fn extract_origin(url: &str) -> String {
    // http://host:port/v1 → http://host:port
    if let Some(pos) = url.find("://") {
        let after = &url[pos + 3..];
        let end = after.find('/').map(|i| pos + 3 + i).unwrap_or(url.len());
        url[..end].to_string()
    } else {
        url.to_string()
    }
}

impl ModelAdapter for OpenAiCompatibleAdapter {
    fn execute(&self, input: &SessionInput, config: &ModelConfig) -> Result<RoleOutputData> {
        let url = self.chat_completions_url();
        let context_text = build_context_text(input);
        let tool_schema_str = Self::get_tool_schema(&input.role);
        let tool_schema: serde_json::Value = serde_json::from_str(tool_schema_str)?;

        let mode = self.structured_output_mode.as_str();

        // auto: function_calling → json_mode → text_extraction
        let result = if mode == "auto" || mode == "function_calling" {
            let r = try_function_calling(&url, self.get_api_key().as_deref(), &config.model_id, &context_text, &tool_schema, input, config.verbose);
            if mode == "function_calling" { return r; }
            match r {
                Ok(o) => return Ok(o),
                Err(_) => {},
            }
            // json_mode fallback
            let r2 = try_json_mode(&url, self.get_api_key().as_deref(), &config.model_id, &context_text, input, is_ollama_endpoint(&url), config.verbose);
            match r2 {
                Ok(o) => return Ok(o),
                Err(_) => {},
            }
            // text_extraction fallback
            try_text_extraction(&url, self.get_api_key().as_deref(), &config.model_id, &context_text, input, &mut *self.raw_text.lock().unwrap(), config.verbose)
        } else if mode == "json_mode" {
            try_json_mode(&url, self.get_api_key().as_deref(), &config.model_id, &context_text, input, is_ollama_endpoint(&url), config.verbose)
        } else {
            // text_extraction
            try_text_extraction(&url, self.get_api_key().as_deref(), &config.model_id, &context_text, input, &mut *self.raw_text.lock().unwrap(), config.verbose)
        };

        result
    }

    fn adapter_name(&self) -> &str { "openai_compatible" }

    fn supports_structured_output(&self) -> bool { true }

    fn last_raw_text(&self) -> Option<String> {
        self.raw_text.lock().unwrap().clone()
    }
}

fn try_function_calling(
    url: &str,
    api_key: Option<&str>,
    model_id: &str,
    context_text: &str,
    tool_schema: &serde_json::Value,
    input: &SessionInput,
    verbose: bool,
) -> Result<RoleOutputData> {
    let system_prompt = resolve_role_system_prompt(&input.role, &input.role_extra);
    let max_tokens: u32 = if input.role == "development" { 16384 } else { 4096 };
    let body = serde_json::json!({
        "model": model_id,
        "max_tokens": max_tokens,
        "messages": [
            {"role": "system", "content": if system_prompt.is_empty() { "단계에 맞는 결과를 submit_report 함수로 반드시 제출하세요.".to_string() } else { system_prompt }},
            {"role": "user", "content": context_text}
        ],
        "tools": [{"type": "function", "function": {
            "name": "submit_report",
            "description": "단계 완료 결과를 구조화된 JSON으로 제출합니다",
            "parameters": tool_schema
        }}],
        "tool_choice": {"type": "function", "function": {"name": "submit_report"}}
    });

    let response_json = post_json(url, api_key, &body)?;
    let arguments = response_json["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .context("function_calling: tool_calls[0].function.arguments 없음")?;
    if verbose {
        println!("\n{}", "[AI 응답 (function_calling)]".dimmed());
        println!("{}", arguments.dimmed());
    }
    let v: serde_json::Value = serde_json::from_str(arguments)?;
    parse_role_output_from_value(&v, &input.role)
}

fn try_json_mode(
    url: &str,
    api_key: Option<&str>,
    model_id: &str,
    context_text: &str,
    input: &SessionInput,
    is_ollama: bool,
    verbose: bool,
) -> Result<RoleOutputData> {
    let system_prompt = resolve_role_system_prompt(&input.role, &input.role_extra);
    let max_tokens: u32 = if input.role == "development" { 16384 } else { 4096 };
    let mut body = serde_json::json!({
        "model": model_id,
        "max_tokens": max_tokens,
        "messages": [
            {"role": "system", "content": if system_prompt.is_empty() { "JSON 형식으로만 응답하세요.".to_string() } else { system_prompt }},
            {"role": "user", "content": context_text}
        ],
        "response_format": {"type": "json_object"}
    });

    if is_ollama {
        body["format"] = serde_json::json!("json");
    }

    let response_json = post_json(url, api_key, &body)?;
    let content = response_json["choices"][0]["message"]["content"]
        .as_str()
        .context("json_mode: choices[0].message.content 없음")?;
    if verbose {
        println!("\n{}", "[AI 응답 (json_mode)]".dimmed());
        println!("{}", content.dimmed());
    }
    let v: serde_json::Value = serde_json::from_str(content)
        .context("json_mode: content가 유효한 JSON이 아님")?;
    parse_role_output_from_value(&v, &input.role)
}

fn try_text_extraction(
    url: &str,
    api_key: Option<&str>,
    model_id: &str,
    context_text: &str,
    input: &SessionInput,
    raw_text_out: &mut Option<String>,
    verbose: bool,
) -> Result<RoleOutputData> {
    let system_prompt = resolve_role_system_prompt(&input.role, &input.role_extra);
    let mut messages = vec![serde_json::json!({"role": "user", "content": context_text})];
    if !system_prompt.is_empty() {
        messages.insert(0, serde_json::json!({"role": "system", "content": system_prompt}));
    }
    let max_tokens: u32 = if input.role == "development" { 16384 } else { 4096 };
    let body = serde_json::json!({
        "model": model_id,
        "max_tokens": max_tokens,
        "messages": messages
    });

    let response_json = post_json(url, api_key, &body)?;
    let content = response_json["choices"][0]["message"]["content"]
        .as_str()
        .context("text_extraction: choices[0].message.content 없음")?;
    *raw_text_out = Some(content.to_string());
    if verbose {
        println!("\n{}", "[AI 응답 (text_extraction)]".dimmed());
        println!("{}", content.dimmed());
    }

    try_parse_json_output(content, &input.role)
        .ok_or_else(|| anyhow::anyhow!("text_extraction: JSON 파싱 실패"))
}

fn post_json(
    url: &str,
    api_key: Option<&str>,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let mut request = ureq::post(url)
        .set("content-type", "application/json");

    if let Some(key) = api_key {
        request = request.set("Authorization", &format!("Bearer {}", key));
    }

    let response = request
        .send_json(body)
        .context(format!("POST {} 실패", url))?;

    response
        .into_json()
        .context(format!("POST {} 응답 JSON 파싱 실패", url))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::input::SessionInput;

    #[test]
    fn missing_api_key_env_uses_openai_default() {
        let original = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        let adapter = OpenAiCompatibleAdapter::new(
            "https://api.openai.com/v1".to_string(),
            None,
            "auto".to_string(),
        );
        assert!(adapter.get_api_key().is_none());
        if let Some(k) = original { std::env::set_var("OPENAI_API_KEY", k); }
    }

    #[test]
    fn no_auth_header_when_empty_key_env() {
        let adapter = OpenAiCompatibleAdapter::new(
            "http://localhost:11434/v1".to_string(),
            Some(String::new()),
            "json_mode".to_string(),
        );
        assert!(adapter.get_api_key().is_none());
    }

    #[test]
    fn is_ollama_endpoint_detects_port() {
        assert!(is_ollama_endpoint("http://localhost:11434/v1"));
        assert!(is_ollama_endpoint("http://127.0.0.1:11434"));
        assert!(!is_ollama_endpoint("https://api.openai.com/v1"));
    }

    #[test]
    fn chat_completions_url_with_v1_suffix() {
        let a = OpenAiCompatibleAdapter::new(
            "http://localhost:11434/v1".to_string(), None, "auto".to_string(),
        );
        assert_eq!(a.chat_completions_url(), "http://localhost:11434/v1/chat/completions");
    }

    #[test]
    fn chat_completions_url_without_v1_suffix() {
        let a = OpenAiCompatibleAdapter::new(
            "https://api.openai.com".to_string(), None, "auto".to_string(),
        );
        assert!(a.chat_completions_url().ends_with("/chat/completions"));
    }

    #[test]
    fn build_context_text_includes_snapshot() {
        use crate::session::v0_7::{SnapshotFile, WorkspaceSnapshot};
        let mut input = SessionInput::default();
        input.role = "development".to_string();
        input.task_id = "M2-T01".to_string();
        input.workspace_snapshot = Some(WorkspaceSnapshot {
            file_tree: String::new(),
            files: vec![SnapshotFile {
                path: "src/main.rs".to_string(),
                content: Some("fn main() {}".to_string()),
                summary: None, size_bytes: 12,
                last_modified: String::new(),
            }],
            recent_git_diff: None,
            untracked_files: vec![],
        });
        let text = build_context_text(&input);
        assert!(text.contains("src/main.rs"));
        assert!(text.contains("fn main()"));
    }
}
