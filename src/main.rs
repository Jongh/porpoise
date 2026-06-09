mod claude;
mod conductor;
mod config;
mod doctor;
mod init;
mod logger;
mod milestone;
mod model;
mod orchestrator;
mod session;
mod status;
mod utils;
mod workspace;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::Path;

use config::Config;

#[derive(Subcommand, Debug)]
pub enum UpdateCommands {
    /// Regenerate .porpoise/prompts/ files based on current adapter type
    Prompt,
    /// Re-select language and model in workspace.toml
    Config,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Archive old messages to .porpoise/messages/archive/
    Clean {
        /// Reports older than this many days are archived (default: from porpoise.toml or 30)
        #[arg(long)]
        days: Option<u32>,
        /// Print what would be moved without actually moving
        #[arg(long)]
        dry_run: bool,
    },
    /// Create a verdict file in .porpoise/reports/ for the current role
    Approve {
        /// Verdict: NEXT or PREV (default: NEXT)
        #[arg(default_value = "NEXT")]
        verdict: String,
    },
    /// Update prompts or config without full re-initialization
    Update {
        #[command(subcommand)]
        subcommand: UpdateCommands,
    },
    /// Migrate a legacy project to the JSON session format
    Migrate,
    /// Diagnose workspace configuration, adapter, API keys, and dependencies
    Doctor,
    /// Show current orchestration status (milestone, task, stage, session count)
    Status,
    /// Aggregate conductor audit records into a fleet execution report
    Report {
        /// Limit to a specific milestone number (e.g. 25)
        #[arg(long)]
        milestone: Option<u32>,
        /// Also export the report as Markdown
        #[arg(long)]
        markdown: bool,
        /// Output path for the Markdown report (default: .porpoise/reports/run-M{N}.md)
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Parser, Debug)]
#[command(
    name = "porpoise",
    version,
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
            Commands::Approve { verdict } => {
                return run_approve(&current_dir, verdict);
            }
            Commands::Update { subcommand } => match subcommand {
                UpdateCommands::Prompt => return init::run_update_prompt(&current_dir),
                UpdateCommands::Config => return init::run_update_config(&current_dir),
            },
            Commands::Migrate => return run_migrate(&current_dir),
            Commands::Doctor => {
                let failures = doctor::run_doctor(&current_dir);
                if failures > 0 {
                    std::process::exit(1);
                }
                return Ok(());
            }
            Commands::Status => {
                status::run_status(&current_dir);
                return Ok(());
            }
            Commands::Report {
                milestone,
                markdown,
                out,
            } => {
                return conductor::report::run_report(
                    &current_dir,
                    *milestone,
                    *markdown,
                    out.as_deref(),
                );
            }
        }
    }

    // --new 초기화 시에는 workspace.toml이 아직 없으므로 의존성 검사 스킵
    if !args.new && utils::deps::check_and_report(&current_dir) {
        std::process::exit(1);
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
        // Immediately proceed to orchestration so the first milestone session starts
        // without requiring a separate `porpoise` invocation.
        orchestrator::run(&current_dir, &args, &config)?;
    }

    Ok(())
}

fn run_approve(path: &Path, verdict: &str) -> Result<()> {
    use chrono::Local;
    use colored::Colorize;

    let verdict_upper = verdict.trim().to_uppercase();
    if verdict_upper != "NEXT" && verdict_upper != "PREV" {
        anyhow::bail!("유효한 판정값: NEXT 또는 PREV (입력값: {})", verdict);
    }

    let messages_dir = path.join(".porpoise").join("messages");
    let reports_dir = path.join(".porpoise").join("reports");

    if !messages_dir.exists() {
        let sessions_dir = path.join(".porpoise").join("sessions");
        if sessions_dir.exists() {
            println!("{}", "이 기능은 레거시 프로젝트에서만 사용 가능합니다.".yellow());
        } else {
            println!("{}", "messages/ 폴더가 없습니다. 먼저 porpoise를 실행하세요.".yellow());
        }
        return Ok(());
    }

    // messages/ 에는 있고 reports/ 에는 없는 최신 파일을 찾아 판정 파일 생성
    let entries = std::fs::read_dir(&messages_dir)?;
    let mut candidates: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") && !name.starts_with("checkpoint") && !name.contains("-hints") {
                let rpt_path = reports_dir.join(&name);
                if !rpt_path.exists() {
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    candidates.sort();

    match candidates.last() {
        Some(name) => {
            std::fs::create_dir_all(&reports_dir)?;
            let report_file = reports_dir.join(name);
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let content = format!(
                "# 수동 판정\n\n작성 시각: {}\n판정: {}\n\n{}\n",
                timestamp, verdict_upper, verdict_upper
            );
            std::fs::write(&report_file, &content)?;
            println!("  {} 판정 파일 생성: {}", "✓".green(), name.dimmed());
            println!("  {} {}", "판정:".cyan(), verdict_upper.yellow().bold());
        }
        None => {
            println!("{}", "판정 대상 파일이 없습니다 (messages/ 파일이 없거나 이미 reports/ 파일이 존재합니다).".yellow());
        }
    }

    Ok(())
}

fn run_migrate(path: &Path) -> Result<()> {
    let porpoise_dir = path.join(".porpoise");
    let sessions_dir = porpoise_dir.join("sessions");

    if sessions_dir.exists() {
        println!("{}", "이미 신규 포맷(sessions/)이 존재합니다. 마이그레이션이 필요하지 않습니다.".green());
        return Ok(());
    }

    let has_legacy = porpoise_dir.join("messages").exists() || porpoise_dir.join("reports").exists();
    if !has_legacy {
        println!(
            "{}",
            ".porpoise/messages/ 또는 .porpoise/reports/ 폴더가 없습니다. 마이그레이션 대상 프로젝트가 아닙니다.".yellow()
        );
        return Ok(());
    }

    println!("{}", "\n=== 레거시 프로젝트 마이그레이션 ===".cyan().bold());
    println!("  레거시 보고서 폴더(.porpoise/messages/, .porpoise/reports/)가 감지되었습니다.");
    println!("  신규 JSON 세션 포맷으로 전환합니다...");
    println!();

    std::fs::create_dir_all(&sessions_dir)
        .map_err(|e| anyhow::anyhow!("sessions/ 디렉토리 생성 실패: {}", e))?;

    println!("  {} .porpoise/sessions/ 디렉토리 생성 완료", "✓".green());
    println!();
    println!("{}", "마이그레이션 완료. 'porpoise'를 다시 실행하면 신규 포맷으로 시작합니다.".green().bold());
    println!(
        "{}",
        "  기존 레거시 파일(.porpoise/messages/, .porpoise/reports/)은 보존됩니다.".dimmed()
    );

    Ok(())
}

fn run_clean(path: &Path, days: Option<u32>, dry_run: bool, config: &Config) -> Result<()> {
    use chrono::Local;

    let effective_days = days.unwrap_or(config.archive_after_days());
    let reports_dir = path.join(".porpoise").join("messages");

    if !reports_dir.exists() {
        println!("메세지 디렉토리가 없습니다: {}", reports_dir.display());
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
