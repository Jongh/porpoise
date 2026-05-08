pub mod context;
pub mod generator;
pub mod lang_template;
pub mod template;
pub mod tree;

use anyhow::Result;
use colored::Colorize;
use dialoguer::Confirm;
use std::path::Path;

use crate::Args;
use crate::config::workspace::WorkspaceConfig;

pub fn run(path: &Path, args: &Args) -> Result<()> {
    println!();
    println!("{}", "=== Porpoise Project Initialization ===".green().bold());
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

    // Load workspace config (preserves existing .porpoise/workspace.toml if present)
    let workspace = WorkspaceConfig::load(path)?;

    // Generate docs
    println!();
    println!("{}", "Generating documentation...".cyan());
    generator::generate_docs(&ctx, path, &workspace, lang_template)?;

    println!();
    println!("{}", "Initialization complete!".green().bold());
    println!(
        "Run {} again to start the orchestration cycle.",
        "porpoise".cyan()
    );

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

fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}
