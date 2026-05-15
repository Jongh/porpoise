pub mod context;
pub mod generator;
pub mod lang_template;
pub mod model_template;
pub mod template;
pub mod tree;

use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::Confirm;
use std::path::Path;

use crate::config::workspace::WorkspaceConfig;
use crate::init::model_template::ResolvedModel;
use crate::Args;

pub fn run(path: &Path, args: &Args) -> Result<()> {
    println!();
    println!(
        "{}",
        "=== Porpoise Project Initialization ===".green().bold()
    );
    println!();

    if args.verbose {
        println!("{} {}", "Working directory:".dimmed(), path.display());
        println!();
    }

    // --new 플래그이고 기존 .porpoise/가 존재하면 덮어쓰기 여부를 한 번만 확인한다.
    if args.new && path.join(".porpoise").exists() {
        let overwrite = Confirm::new()
            .with_prompt(
                "기존 .porpoise/ 디렉토리가 존재합니다. 덮어쓰면 이전 작업 이력이 소실됩니다. 계속하시겠습니까?"
            )
            .default(false)
            .interact()?;
        if !overwrite {
            println!("{}", "초기화를 취소했습니다.".yellow());
            return Ok(());
        }
    }

    // Print directory tree
    println!("{}", "Project structure:".yellow());
    tree::print_tree(path)?;
    println!();

    // Collect tree output for context
    let tree_output = tree::get_tree_string(path)?;

    let ctx = context::collect_project_context(&tree_output)?;

    // 언어 템플릿 선택 (비대화형이거나 --yes면 건너뜀)
    let lang_template = select_lang_template(args.yes, path);
    let model_template = select_model_template(args.yes, path);

    // Load workspace config (preserves existing .porpoise/workspace.toml if present)
    let workspace = WorkspaceConfig::load(path)?;

    // Generate docs
    println!();
    println!("{}", "Generating documentation...".cyan());
    generator::generate_docs(&ctx, path, &workspace, lang_template, model_template.as_ref())?;

    println!();
    println!("{}", "Initialization complete!".green().bold());
    println!(
        "{}",
        "신규 프로젝트는 JSON session mode로 실행됩니다.".green()
    );

    if let Some(ref m) = model_template {
        let display_id = m.model_id.as_deref()
            .or(m.template.model_id)
            .unwrap_or("");
        if display_id.is_empty() {
            println!("모델: {}", m.template.display_name.cyan());
        } else {
            println!("모델: {} ({})", m.template.display_name.cyan(), display_id.dimmed());
        }

        let key_env = m.api_key_env.as_deref().or(m.template.api_key_env).unwrap_or("");
        if !key_env.is_empty() {
            println!("  → {} 환경변수를 설정하세요.", key_env.yellow());
        }
    }

    println!("{}", "마일스톤 생성 세션을 시작합니다...".cyan());

    Ok(())
}

fn select_lang_template(yes: bool, path: &Path) -> Option<&'static lang_template::LangTemplate> {
    // --yes 이거나 workspace.toml이 이미 있으면 선택 건너뜀
    if yes || path.join(".porpoise").join("workspace.toml").exists() {
        return None;
    }

    // 비대화형 환경 감지
    if !is_interactive() {
        return None;
    }

    let items: Vec<&str> = lang_template::ALL_TEMPLATES
        .iter()
        .map(|t| t.display_name)
        .chain(std::iter::once("커스텀 (선택 안 함)"))
        .collect();

    match dialoguer::Select::new()
        .with_prompt("언어/프레임워크 템플릿을 선택하세요")
        .items(&items)
        .default(0)
        .interact_opt()
    {
        Ok(Some(idx)) if idx < lang_template::ALL_TEMPLATES.len() => {
            Some(lang_template::ALL_TEMPLATES[idx])
        }
        _ => None,
    }
}

fn select_model_template(yes: bool, path: &Path) -> Option<ResolvedModel> {
    // --yes 이거나 workspace.toml이 이미 있으면 선택 건너뜀
    if yes || path.join(".porpoise").join("workspace.toml").exists() {
        return None;
    }

    // 비대화형 환경 감지
    if !is_interactive() {
        return None;
    }

    let items: Vec<&str> = model_template::ALL_TEMPLATES
        .iter()
        .map(|t| t.display_name)
        .chain(std::iter::once("커스텀 (workspace.toml에서 직접 설정)"))
        .collect();

    let idx = match dialoguer::Select::new()
        .with_prompt("모델 템플릿을 선택하세요")
        .items(&items)
        .default(0)
        .interact_opt()
    {
        Ok(Some(i)) if i < model_template::ALL_TEMPLATES.len() => i,
        _ => return None,
    };

    let template = model_template::ALL_TEMPLATES[idx];

    // Secondary Input (yes이면 진입 안 함, 위에서 이미 return)
    // 여기는 is_interactive() == true이고 yes == false인 경우만 도달
    let (model_id, api_key_env, api_base_url) = get_model_overrides(template);

    Some(ResolvedModel {
        template,
        model_id,
        api_key_env,
        api_base_url,
    })
}

fn get_model_overrides(
    template: &'static model_template::ModelTemplate,
) -> (Option<String>, Option<String>, Option<String>) {
    use model_template::{
        ANTHROPIC_CLAUDE_SONNET, CLAUDE_CODE_DEFAULT, GEMINI, GROQ, OLLAMA_LOCAL, OPENAI_CODEX,
    };

    if std::ptr::eq(template, &CLAUDE_CODE_DEFAULT) {
        return (None, None, None);
    }

    let prompt_input = |prompt: &str, default: &str| -> String {
        dialoguer::Input::<String>::new()
            .with_prompt(prompt)
            .default(default.to_string())
            .interact_text()
            .unwrap_or_else(|_| default.to_string())
    };

    if std::ptr::eq(template, &ANTHROPIC_CLAUDE_SONNET) {
        let model_id = prompt_input("모델 ID", template.model_id.unwrap_or("claude-sonnet-4-6"));
        let api_key_env = prompt_input("API 키 환경변수", template.api_key_env.unwrap_or("ANTHROPIC_API_KEY"));
        return (Some(model_id), Some(api_key_env), None);
    }

    if std::ptr::eq(template, &GROQ) {
        let model_id = prompt_input("모델 ID", template.model_id.unwrap_or("llama-3.3-70b-versatile"));
        let api_key_env = prompt_input("API 키 환경변수", template.api_key_env.unwrap_or("GROQ_API_KEY"));
        return (Some(model_id), Some(api_key_env), None);
    }

    if std::ptr::eq(template, &GEMINI) {
        let model_id = prompt_input("모델 ID", template.model_id.unwrap_or("gemini-2.0-flash"));
        let api_key_env = prompt_input("API 키 환경변수", template.api_key_env.unwrap_or("GEMINI_API_KEY"));
        return (Some(model_id), Some(api_key_env), None);
    }

    if std::ptr::eq(template, &OPENAI_CODEX) {
        let model_id = prompt_input("모델 ID", template.model_id.unwrap_or("codex-mini-latest"));
        let api_key_env = prompt_input("API 키 환경변수", template.api_key_env.unwrap_or("OPENAI_API_KEY"));
        let api_base_url = prompt_input("API Base URL", template.api_base_url.unwrap_or("https://api.openai.com/v1"));
        return (Some(model_id), Some(api_key_env), Some(api_base_url));
    }

    if std::ptr::eq(template, &OLLAMA_LOCAL) {
        let model_id = prompt_input("모델 ID", template.model_id.unwrap_or("gemma4:e4b"));
        let api_base_url = prompt_input("Ollama 서버 URL", template.api_base_url.unwrap_or("http://localhost:11434/v1"));
        return (Some(model_id), None, Some(api_base_url));
    }

    (None, None, None)
}

fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

pub fn run_update_prompt(path: &Path) -> Result<()> {
    println!("{}", "\n=== 프롬프트 재생성 ===".cyan().bold());

    let workspace = WorkspaceConfig::load(path)?;
    generator::generate_prompts_only(path, &workspace)?;

    println!("{}", "\n프롬프트 재생성 완료.".green().bold());
    Ok(())
}

pub fn run_update_config(path: &Path) -> Result<()> {
    println!("{}", "\n=== 설정 업데이트 ===".cyan().bold());

    let ws_path = path.join(".porpoise").join("workspace.toml");
    if !ws_path.exists() {
        anyhow::bail!(
            "workspace.toml을 찾을 수 없습니다. 먼저 porpoise를 실행하여 프로젝트를 초기화하세요."
        );
    }

    let mut content = std::fs::read_to_string(&ws_path)?;

    if let Some(lang) = select_lang_for_update() {
        content = update_language_in_toml(&content, lang);
        println!("  {} 언어: {}", "✓".green(), lang);
    }

    if let Some(resolved) = select_model_for_update() {
        let new_section = generator::model_toml_section(Some(&resolved));
        content = replace_model_section_in_toml(&content, &new_section);
        println!("  {} 모델: {}", "✓".green(), resolved.template.display_name);
    }

    std::fs::write(&ws_path, &content)
        .with_context(|| format!("workspace.toml 쓰기 실패: {}", ws_path.display()))?;
    println!("{}", "\n설정 업데이트 완료.".green().bold());

    Ok(())
}

fn select_lang_for_update() -> Option<&'static str> {
    if !is_interactive() {
        return None;
    }
    let items = ["한국어 (ko)", "English (en)"];
    match dialoguer::Select::new()
        .with_prompt("언어를 선택하세요")
        .items(&items)
        .default(0)
        .interact_opt()
    {
        Ok(Some(0)) => Some("ko"),
        Ok(Some(1)) => Some("en"),
        _ => None,
    }
}

fn select_model_for_update() -> Option<ResolvedModel> {
    if !is_interactive() {
        return None;
    }
    let items: Vec<&str> = model_template::ALL_TEMPLATES
        .iter()
        .map(|t| t.display_name)
        .chain(std::iter::once("변경 안 함"))
        .collect();

    let idx = match dialoguer::Select::new()
        .with_prompt("모델 템플릿을 선택하세요")
        .items(&items)
        .default(0)
        .interact_opt()
    {
        Ok(Some(i)) if i < model_template::ALL_TEMPLATES.len() => i,
        _ => return None,
    };

    let template = model_template::ALL_TEMPLATES[idx];
    let (model_id, api_key_env, api_base_url) = get_model_overrides(template);

    Some(ResolvedModel { template, model_id, api_key_env, api_base_url })
}

fn update_language_in_toml(content: &str, new_lang: &str) -> String {
    let mut result: Vec<String> = Vec::new();
    let mut in_general = false;
    let mut replaced = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_general = trimmed == "[general]";
        }
        if in_general && !replaced && trimmed.starts_with("language") && trimmed.contains('=') {
            result.push(format!("language = \"{}\"", new_lang));
            replaced = true;
        } else {
            result.push(line.to_string());
        }
    }

    if !replaced {
        let mut prefix = format!("[general]\nlanguage = \"{}\"\n\n", new_lang);
        prefix.push_str(&result.join("\n"));
        if content.ends_with('\n') && !prefix.ends_with('\n') {
            prefix.push('\n');
        }
        return prefix;
    }

    let joined = result.join("\n");
    if content.ends_with('\n') { format!("{}\n", joined) } else { joined }
}

fn replace_model_section_in_toml(content: &str, new_section: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut model_start: Option<usize> = None;
    let mut model_end = lines.len();

    for (i, &line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == "[model]" {
            let start = if i > 0 && lines[i - 1].trim().starts_with('#') {
                i - 1
            } else {
                i
            };
            model_start = Some(start);
        } else if model_start.is_some() && trimmed.starts_with('[') && !trimmed.is_empty() {
            model_end = i;
            break;
        }
    }

    let before: Vec<&str> = match model_start {
        Some(start) => lines[..start].to_vec(),
        None => lines.clone(),
    };
    let after: Vec<&str> = if model_start.is_some() && model_end < lines.len() {
        lines[model_end..].to_vec()
    } else {
        vec![]
    };

    let mut result = before.join("\n");
    if !new_section.is_empty() {
        if !result.ends_with('\n') && !result.is_empty() {
            result.push('\n');
        }
        result.push_str(new_section.trim_end_matches('\n'));
        result.push('\n');
    }
    if !after.is_empty() {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&after.join("\n"));
    }
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn update_language_replaces_existing() {
        let toml = "[general]\nlanguage = \"ko\"\n";
        let result = update_language_in_toml(toml, "en");
        assert!(result.contains("language = \"en\""));
        assert!(!result.contains("language = \"ko\""));
    }

    #[test]
    fn update_language_prepends_when_absent() {
        let toml = "[dod]\nitems = []\n";
        let result = update_language_in_toml(toml, "en");
        assert!(result.contains("[general]"));
        assert!(result.contains("language = \"en\""));
    }

    #[test]
    fn replace_model_section_removes_old_and_appends_new() {
        let toml = "[general]\nlanguage = \"ko\"\n\n# === 모델 설정 ===\n[model]\nadapter = \"claude_code\"\n";
        let new_section = "\n# === 모델 설정 — Anthropic ===\n[model]\nadapter = \"anthropic_api\"\n";
        let result = replace_model_section_in_toml(toml, new_section);
        assert!(!result.contains("claude_code"));
        assert!(result.contains("anthropic_api"));
        assert!(result.contains("[general]"));
    }

    #[test]
    fn replace_model_section_appends_when_absent() {
        let toml = "[general]\nlanguage = \"ko\"\n";
        let new_section = "\n[model]\nadapter = \"anthropic_api\"\n";
        let result = replace_model_section_in_toml(toml, new_section);
        assert!(result.contains("[model]"));
        assert!(result.contains("anthropic_api"));
        assert!(result.contains("[general]"));
    }
}
