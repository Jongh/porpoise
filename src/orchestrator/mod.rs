pub mod checkpoint;
pub mod report;
pub mod roles;
pub mod state;

use anyhow::Result;
use colored::Colorize;
use dialoguer::{Confirm, Select};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::time::Duration;

use crate::logger::Logger;
use crate::token::monitor::{TokenMonitor, TokenWarningLevel};
use crate::Args;

use checkpoint::{save_checkpoint, Checkpoint};
use report::{save_report, ReviewStatus};
use roles::{build_context, RoleExecutor};
use state::{load_state, Role};

pub fn run(path: &Path, args: &Args) -> Result<()> {
    let logger = Logger::new(path, args.verbose)?;

    println!();
    println!("{}", "=== Porpoise Orchestration ===".green().bold());
    println!();

    let thresholds: Vec<u8> = args
        .token_warn
        .split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .collect();
    let token_monitor = TokenMonitor::new(thresholds, path);

    let token_level = token_monitor.check_usage();
    token_monitor.display_warning(&token_level);
    if matches!(token_level, TokenWarningLevel::Critical(_)) {
        println!("{}", "Token usage critical. Consider archiving old reports.".red().bold());
        logger.warn("orchestrator", "Token usage critical at session start");
    }

    let mut state = load_state(path)?;
    logger.info("orchestrator", &format!("Loaded state: cycle={}", state.cycle));

    // --from override
    if let Some(ref from_role) = args.from {
        match Role::from_str(from_role) {
            Some(role) => {
                logger.info("orchestrator", &format!("--from override: {}", role));
                let start_idx = Role::all()
                    .iter()
                    .position(|r| r == &role)
                    .unwrap_or(0);
                state.completed_roles = Role::all()[..start_idx].to_vec();
                state.current_role = Some(role);
            }
            None => anyhow::bail!(
                "Unknown role: '{}'. Valid: pm, developer, tester, reviewer",
                from_role
            ),
        }
    }

    print_resume_summary(&state);

    if args.dry_run {
        println!("{}", "[DRY RUN MODE — no execution will happen]".yellow().bold());
        println!();
    }

    let executor = RoleExecutor::new();
    let mut history: Vec<String> = Vec::new();

    loop {
        let current_role = match &state.current_role {
            Some(r) => r.clone(),
            None => {
                println!("{}", "All roles completed for this cycle.".green().bold());
                logger.info("orchestrator", "All roles completed");
                break;
            }
        };

        println!(
            "{}",
            format!(
                "\n[ Cycle {} ] ─── {} ───",
                state.cycle,
                current_role.display_name()
            )
            .bold()
        );

        // Token check
        let token_level = token_monitor.check_usage();
        token_monitor.display_warning(&token_level);
        if matches!(token_level, TokenWarningLevel::Critical(_)) {
            logger.warn(&current_role.to_string(), "Token critical before role exec");
            let save_and_exit = Confirm::new()
                .with_prompt("Token usage critical. Save checkpoint and exit?")
                .default(true)
                .interact()?;
            if save_and_exit {
                save_current_checkpoint(&state, &current_role, path)?;
                println!("{}", "Checkpoint saved. Run 'porpoise' to resume.".cyan());
                break;
            }
        }

        // Confirm before each role (skip in dry-run)
        if !args.dry_run {
            let proceed = Confirm::new()
                .with_prompt(format!("Execute {}?", current_role.display_name()))
                .default(true)
                .interact()?;
            if !proceed {
                logger.info(&current_role.to_string(), "Skipped by user");
                println!("{}", "Skipped. Run 'porpoise' to resume later.".yellow());
                break;
            }
        }

        save_current_checkpoint(&state, &current_role, path)?;
        logger.role_start(&current_role.to_string(), state.cycle);

        let context = build_context(&current_role, state.cycle, path);
        logger.debug(
            &current_role.to_string(),
            &format!(
                "context: {} project docs, {} prev reports",
                context.project_docs.len(),
                context.previous_reports.len()
            ),
        );

        // Execute role
        let report_result = if args.dry_run {
            executor.execute_role(&current_role, &context, path, true)
        } else {
            let spinner = make_spinner(&format!("Running {} ...", current_role.display_name()));
            let result = executor.execute_role(&current_role, &context, path, false);
            spinner.finish_and_clear();
            result
        };

        let report = match report_result {
            Ok(r) => {
                logger.role_end(&current_role.to_string(), state.cycle, true);
                r
            }
            Err(e) => {
                logger.role_end(&current_role.to_string(), state.cycle, false);
                logger.error(&current_role.to_string(), &e.to_string());
                println!("{} {}", "Error executing role:".red().bold(), e);
                let retry = Confirm::new()
                    .with_prompt("Retry this role?")
                    .default(true)
                    .interact()?;
                if retry {
                    continue;
                } else {
                    break;
                }
            }
        };

        if !args.dry_run {
            let report_path = save_report(&report, path)?;
            history.push(format!(
                "Cycle {} | {} → {}",
                state.cycle,
                current_role.display_name(),
                report_path.file_name().unwrap_or_default().to_string_lossy()
            ));
            println!(
                "  {} Report: {}",
                "✓".green(),
                report_path.file_name().unwrap_or_default().to_string_lossy().dimmed()
            );
            logger.info(
                &current_role.to_string(),
                &format!("Report saved: {}", report_path.display()),
            );
        }

        // Handle user-input requests
        if report.requires_user_input {
            println!();
            println!("{}", "⚠  User input required".yellow().bold());
            for (i, q) in report.questions.iter().enumerate() {
                println!("  {}. {}", i + 1, q.yellow());
            }
            logger.warn(&current_role.to_string(), "User input required");

            let cont = Confirm::new()
                .with_prompt("Have you addressed the above? Continue?")
                .default(false)
                .interact()?;
            if !cont {
                println!("{}", "Paused. Run 'porpoise' to resume.".cyan());
                break;
            }
        }

        // Critical bug handling
        if report.has_critical_bugs {
            println!("{}", "\n✗  Critical bugs detected — routing back to Developer".red().bold());
            logger.warn("tester", "Critical bugs found, restarting Developer");
            state.completed_roles.retain(|r| *r != Role::Developer);
            state.current_role = Some(Role::Developer);
            continue;
        }

        // Reviewer decision flow
        if current_role == Role::Reviewer {
            println!();
            match report.review_status.as_ref() {
                Some(ReviewStatus::Approved) => {
                    println!("{}", "✓  Review: APPROVED".green().bold());
                    logger.info("reviewer", "APPROVED");

                    if report.milestone_complete {
                        println!("{}", "🎉 Milestone complete!".green().bold());
                        logger.info("reviewer", "Milestone complete");
                    }

                    print_history(&history);

                    let new_cycle = Confirm::new()
                        .with_prompt("Start a new development cycle?")
                        .default(false)
                        .interact()?;
                    if new_cycle {
                        state.cycle += 1;
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                        logger.info("orchestrator", &format!("New cycle: {}", state.cycle));
                        println!("\n{}", format!("Starting cycle {}...", state.cycle).cyan());
                        continue;
                    } else {
                        println!("{}", "Done. Run 'porpoise' to start a new cycle.".green());
                        break;
                    }
                }
                Some(ReviewStatus::ChangesRequested) => {
                    println!("{}", "⚠  Review: CHANGES REQUESTED".yellow().bold());
                    logger.info("reviewer", "CHANGES_REQUESTED");

                    let options = &["Developer (address review comments)", "PM (re-scope)", "Cancel"];
                    let choice = Select::new()
                        .with_prompt("Route back to:")
                        .items(options)
                        .default(0)
                        .interact()?;

                    match choice {
                        0 => {
                            state.completed_roles.retain(|r| *r != Role::Developer && *r != Role::Tester);
                            state.current_role = Some(Role::Developer);
                            logger.info("orchestrator", "Routing back to Developer");
                            continue;
                        }
                        1 => {
                            state.completed_roles = vec![];
                            state.current_role = Some(Role::PM);
                            logger.info("orchestrator", "Routing back to PM");
                            continue;
                        }
                        _ => {
                            println!("{}", "Paused. Run 'porpoise' to resume.".cyan());
                            break;
                        }
                    }
                }
                Some(ReviewStatus::Rejected) | None => {
                    println!("{}", "✗  Review: REJECTED — fundamental redesign required".red().bold());
                    logger.warn("reviewer", "REJECTED");

                    let options = &["Restart from PM", "Exit"];
                    let choice = Select::new()
                        .with_prompt("Action:")
                        .items(options)
                        .default(1)
                        .interact()?;

                    if choice == 0 {
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                        logger.info("orchestrator", "Restarting from PM after REJECTED");
                        continue;
                    } else {
                        break;
                    }
                }
            }
        }

        // Advance
        state.completed_roles.push(current_role.clone());
        state.current_role = current_role.next();

        if let Some(ref next) = state.current_role {
            println!("  {} Next: {}", "→".cyan(), next.display_name().cyan());
        }
    }

    logger.info("orchestrator", "Session ended");
    if args.verbose {
        println!("{} {}", "Log:".dimmed(), logger.log_path().display().to_string().dimmed());
    }
    Ok(())
}

fn print_resume_summary(state: &state::OrchestratorState) {
    let completed_str = if state.completed_roles.is_empty() {
        "none".dimmed().to_string()
    } else {
        state
            .completed_roles
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join(", ")
            .green()
            .to_string()
    };
    let next_str = state
        .current_role
        .as_ref()
        .map(|r| r.display_name().cyan().to_string())
        .unwrap_or_else(|| "none".dimmed().to_string());

    println!("  Cycle      : {}", state.cycle.to_string().cyan());
    println!("  Completed  : {}", completed_str);
    println!("  Next role  : {}", next_str);
    println!();
}

fn save_current_checkpoint(
    state: &state::OrchestratorState,
    current_role: &Role,
    path: &Path,
) -> Result<()> {
    let next_role = current_role
        .next()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "none".to_string());

    let cp = Checkpoint::new(
        state.cycle,
        &current_role.to_string(),
        state.completed_roles.iter().map(|r| r.to_string()).collect(),
        &next_role,
        vec![],
    );
    save_checkpoint(&cp, path)
}

fn make_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

fn print_history(history: &[String]) {
    if history.is_empty() {
        return;
    }
    println!();
    println!("{}", "─── Session History ───".dimmed());
    for entry in history {
        println!("  {}", entry.dimmed());
    }
}
