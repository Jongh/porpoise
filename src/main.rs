mod claude;
mod config;
mod init;
mod logger;
mod milestone;
mod orchestrator;
mod token;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::Path;

use config::Config;

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Archive old reports to .porpoise/reports/archive/
    Clean {
        /// Reports older than this many days are archived (default: from porpoise.toml or 30)
        #[arg(long)]
        days: Option<u32>,
        /// Print what would be moved without actually moving
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Parser, Debug)]
#[command(
    name = "porpoise",
    version = "0.2.0",
    about = "Software development orchestration tool powered by Claude Code",
    long_about = None
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Force new initialization even if project already exists
    #[arg(long)]
    pub new: bool,

    /// Start from a specific role (planning/development/testing/review)
    #[arg(long, value_name = "ROLE")]
    pub from: Option<String>,

    /// Show plan without executing
    #[arg(long)]
    pub dry_run: bool,

    /// Token warning thresholds (comma-separated percentages)
    #[arg(long, value_name = "THRESHOLDS", default_value = "70,85,95")]
    pub token_warn: String,

    /// Skip non-critical confirmations (always defaults)
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Override Claude model (e.g. claude-sonnet-4-6)
    #[arg(long)]
    pub model: Option<String>,
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
    let config = Config::load(&current_dir)?;

    if let Some(ref cmd) = args.command {
        match cmd {
            Commands::Clean { days, dry_run } => {
                return run_clean(&current_dir, *days, *dry_run, &config);
            }
        }
    }

    let project_md = current_dir.join(".porpoise").join("project.md");
    let is_resume = project_md.exists() && !args.new;

    if is_resume {
        println!("{}", "Resuming existing Porpoise project...".cyan().bold());
        orchestrator::run(&current_dir, &args, &config)?;
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

fn run_clean(path: &Path, days: Option<u32>, dry_run: bool, config: &Config) -> Result<()> {
    use chrono::Local;

    let effective_days = days.unwrap_or(config.archive_after_days());
    let reports_dir = path.join(".porpoise").join("reports");

    if !reports_dir.exists() {
        println!("리포트 디렉토리가 없습니다: {}", reports_dir.display());
        return Ok(());
    }

    let current_task_id = orchestrator::checkpoint::load_checkpoint(path)
        .ok()
        .and_then(|cp| {
            if cp.current_task_id.is_empty() || cp.current_task_id == "M0-T00" {
                None
            } else {
                Some(cp.current_task_id)
            }
        });

    let threshold = Local::now() - chrono::Duration::days(effective_days as i64);
    let archive_date = Local::now().format("%Y%m%d").to_string();
    let archive_dir = reports_dir.join("archive").join(&archive_date);

    let mut to_archive: Vec<std::path::PathBuf> = Vec::new();

    for entry in std::fs::read_dir(&reports_dir)?.flatten() {
        let file_path = entry.path();
        if !file_path.is_file() {
            continue;
        }
        let name = file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        if name.starts_with("checkpoint.") {
            continue;
        }

        if let Some(ref task_id) = current_task_id {
            if name.contains(task_id.as_str()) {
                continue;
            }
        }

        if let Ok(metadata) = entry.metadata() {
            if let Ok(modified) = metadata.modified() {
                let modified_dt: chrono::DateTime<Local> = modified.into();
                if modified_dt < threshold {
                    to_archive.push(file_path);
                }
            }
        }
    }

    if to_archive.is_empty() {
        println!("이동할 파일이 없습니다 ({}일 미만).", effective_days);
        return Ok(());
    }

    if dry_run {
        println!(
            "이동 대상 ({}일 이상, {}개 파일):",
            effective_days,
            to_archive.len()
        );
        for p in &to_archive {
            println!(
                "  → {}",
                p.file_name().unwrap_or_default().to_string_lossy()
            );
        }
        return Ok(());
    }

    std::fs::create_dir_all(&archive_dir)?;
    let mut moved = 0usize;
    for src in &to_archive {
        let dest = archive_dir.join(src.file_name().unwrap_or_default());
        std::fs::rename(src, &dest)?;
        println!(
            "  {} {}",
            "✓".green(),
            src.file_name().unwrap_or_default().to_string_lossy()
        );
        moved += 1;
    }
    println!(
        "아카이브 완료: {}개 파일 → {}",
        moved,
        archive_dir.display()
    );

    Ok(())
}
