pub mod context;
pub mod generator;
pub mod lang_template;
pub mod model_template;
pub mod template;
pub mod tree;

use anyhow::Result;
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
    use model_template::{CLAUDE_CODE_DEFAULT, ANTHROPIC_CLAUDE_SONNET, OPENAI_CODEX, OLLAMA_LOCAL};

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
