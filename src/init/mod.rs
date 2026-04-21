pub mod tree;
pub mod context;
pub mod generator;

use anyhow::Result;
use colored::Colorize;
use dialoguer::Confirm;
use std::path::Path;

use crate::Args;

pub fn run(path: &Path, args: &Args) -> Result<()> {
    println!();
    println!("{}", "=== Porpoise Project Initialization ===".green().bold());
    println!();

    if args.verbose {
        println!("{} {}", "Working directory:".dimmed(), path.display());
        println!();
    }

    // --new 플래그이고 기존 .docs/가 존재하면 덮어쓰기 여부를 한 번만 확인한다.
    // 기존 작업 이력(.docs/reports/) 전체가 소실될 수 있으므로 명시적 동의 필요.
    if args.new && path.join(".docs").exists() {
        let overwrite = Confirm::new()
            .with_prompt(
                "기존 .docs/ 디렉토리가 존재합니다. 덮어쓰면 이전 작업 이력이 소실됩니다. 계속하시겠습니까?"
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

    // Collect user description
    let ctx = context::collect_user_description(&tree_output)?;

    // Generate docs
    println!();
    println!("{}", "Generating documentation...".cyan());
    generator::generate_docs(&ctx, path)?;

    println!();
    println!("{}", "Initialization complete!".green().bold());
    println!(
        "Run {} again to start the orchestration cycle.",
        "porpoise".cyan()
    );

    Ok(())
}
