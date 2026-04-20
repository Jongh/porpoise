pub mod tree;
pub mod context;
pub mod generator;

use anyhow::Result;
use colored::Colorize;
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
