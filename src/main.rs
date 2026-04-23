mod init;
mod logger;
mod orchestrator;
mod token;
mod claude;
mod utils;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;

#[derive(Parser, Debug)]
#[command(
    name = "porpoise",
    version = "0.1.2",
    about = "Software development orchestration tool powered by Claude Code",
    long_about = None
)]
pub struct Args {
    /// Force new initialization even if project already exists
    #[arg(long)]
    pub new: bool,

    /// Start from a specific role (pm/developer/tester/reviewer)
    #[arg(long, value_name = "ROLE")]
    pub from: Option<String>,

    /// Show plan without executing
    #[arg(long)]
    pub dry_run: bool,

    /// Token warning thresholds (comma-separated percentages)
    #[arg(long, value_name = "THRESHOLDS", default_value = "70,85,95")]
    pub token_warn: String,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

fn main() {
    if let Err(e) = run() {
        utils::error::print_error(&e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let current_dir = std::env::current_dir()?;

    let project_md = current_dir.join(".docs").join("project.md");
    let is_resume = project_md.exists() && !args.new;

    if is_resume {
        println!("{}", "Resuming existing Porpoise project...".cyan().bold());
        orchestrator::run(&current_dir, &args)?;
    } else {
        if args.new {
            println!("{}", "Forcing new initialization...".yellow());
        } else {
            println!("{}", "Initializing new Porpoise project...".green().bold());
        }
        init::run(&current_dir, &args)?;
    }

    Ok(())
}
