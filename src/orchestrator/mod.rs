pub mod checkpoint;
pub mod report;
pub mod roles;
pub mod state;

use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::logger::Logger;
use crate::token::monitor::{TokenMonitor, TokenWarningLevel};
use crate::utils::fs::write_file;
use crate::Args;

use checkpoint::{save_checkpoint, Checkpoint};
use report::{count_existing_reports, save_report, ExitCode, Report};
use roles::{build_context, RoleContext, RoleExecutor};
use state::{load_state, parse_tasks_from_project_md, OrchestratorState, Role};

const MAX_RESP_RETRY: u32 = 5;

enum RoleOutcome {
    Report(Report),
    Retry,
    Stop,
}

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
    logger.info(
        "orchestrator",
        &format!("Loaded state: cycle={} task={}", state.cycle, state.current_task_id),
    );

    // --from override
    if let Some(ref from_role) = args.from {
        match Role::from_str(from_role) {
            Some(role) => {
                logger.info("orchestrator", &format!("--from override: {}", role));
                let start_idx = Role::all().iter().position(|r| r == &role).unwrap_or(0);
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
    let reports_dir = path.join(".docs").join("reports");

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
                "\n[ Cycle {} | {} ] ─── {} ───",
                state.cycle,
                state.current_task_id,
                current_role.display_name()
            )
            .bold()
        );

        // Determine retry number from existing files on disk
        let retry = count_existing_reports(
            &reports_dir,
            &state.current_task_id,
            &current_role.to_string(),
            state.cycle,
        );

        if !check_token_warning(&token_monitor, &current_role, &state, path, args.dry_run, &logger, retry)? {
            break;
        }

        // RESP retry limit guard
        if retry >= MAX_RESP_RETRY {
            println!(
                "{}",
                format!(
                    "⚠  {} 역할이 {}회 재시도 한도에 도달했습니다.",
                    current_role.display_name(),
                    MAX_RESP_RETRY
                )
                .yellow()
                .bold()
            );
            logger.warn(
                &current_role.to_string(),
                &format!("RESP retry limit {} reached", MAX_RESP_RETRY),
            );
            if !args.dry_run {
                let cont = Confirm::new()
                    .with_prompt("강제로 다음 단계로 진행하시겠습니까?")
                    .default(false)
                    .interact()?;
                if !cont {
                    println!("{}", "중단됨. 'porpoise'를 실행하여 재개하세요.".cyan());
                    break;
                }
            }
            state.completed_roles.push(current_role.clone());
            state.current_role = current_role.next();
            continue;
        }

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

        save_current_checkpoint(&state, &current_role, path, retry)?;
        logger.role_start(&current_role.to_string(), state.cycle);

        let context = build_context(&current_role, state.cycle, path, &state.current_task_id);
        logger.debug(
            &current_role.to_string(),
            &format!(
                "context: {} project docs, {} prev reports, task_id={}, retry={}",
                context.project_docs.len(),
                context.previous_reports.len(),
                state.current_task_id,
                retry,
            ),
        );

        match execute_role(
            &executor,
            &current_role,
            &context,
            path,
            state.cycle,
            &state.current_task_id,
            retry,
            args.dry_run,
            &logger,
            &mut history,
        )? {
            RoleOutcome::Retry => continue,
            RoleOutcome::Stop => break,
            RoleOutcome::Report(report) => {
                let exit_code = report.exit_code.clone().unwrap_or_else(|| {
                    logger.warn(&current_role.to_string(), "종료 코드 없음 — NEXT로 폴백");
                    ExitCode::Next
                });

                match exit_code {
                    ExitCode::Next => {
                        if current_role == Role::Reviewer {
                            // F-4: auto commit
                            if !args.dry_run {
                                match auto_commit(&state.current_task_id, &state.current_task_title) {
                                    Ok(()) => {
                                        println!(
                                            "  {} 커밋 완료: [{}] {}",
                                            "✓".green(),
                                            state.current_task_id,
                                            state.current_task_title
                                        );
                                        logger.info(
                                            "reviewer",
                                            &format!("Auto-commit: [{}]", state.current_task_id),
                                        );
                                    }
                                    Err(e) => {
                                        println!("{} {}", "⚠  자동 커밋 실패:".yellow(), e);
                                        logger.warn(
                                            "reviewer",
                                            &format!("Auto-commit failed: {}", e),
                                        );
                                    }
                                }
                                if let Err(e) = mark_task_complete(path, &state.current_task_id) {
                                    logger.warn(
                                        "reviewer",
                                        &format!("Task mark failed: {}", e),
                                    );
                                }
                            } else {
                                println!("{}", "  [dry-run] Reviewer NEXT — 자동 커밋 스킵".dimmed());
                            }

                            // F-5/F-6: check all tasks done
                            if !args.dry_run && all_tasks_done(path) {
                                println!("{}", "\n모든 작업 항목 완료!".green().bold());
                                logger.info("orchestrator", "All tasks completed");
                                print_history(&history);
                                if let Err(e) = run_release_flow(path) {
                                    println!("{} {}", "⚠  릴리즈 플로우 오류:".yellow(), e);
                                    logger.warn(
                                        "orchestrator",
                                        &format!("Release flow error: {}", e),
                                    );
                                }
                                break;
                            }

                            // Advance to next task
                            let tasks = parse_tasks_from_project_md(path);
                            if let Some(next_task) = tasks.iter().find(|t| !t.completed) {
                                println!(
                                    "  {} 다음 작업: {} — {}",
                                    "→".cyan(),
                                    next_task.id.cyan(),
                                    next_task.title
                                );
                                state.current_task_id = next_task.id.clone();
                                state.current_task_title = next_task.title.clone();
                                state.completed_roles = vec![];
                                state.current_role = Some(Role::PM);
                                logger.info(
                                    "orchestrator",
                                    &format!("Next task: {}", state.current_task_id),
                                );
                            } else {
                                // No structured tasks — new cycle
                                if args.dry_run {
                                    println!("{}", "  [dry-run] No structured tasks — stopping after first cycle".dimmed());
                                    break;
                                }
                                state.cycle += 1;
                                state.completed_roles = vec![];
                                state.current_role = Some(Role::PM);
                                logger.info(
                                    "orchestrator",
                                    &format!("New cycle: {}", state.cycle),
                                );
                                println!("\n{}", format!("사이클 {} 시작...", state.cycle).cyan());
                            }
                        } else {
                            state.completed_roles.push(current_role.clone());
                            state.current_role = current_role.next();
                            if let Some(ref next) = state.current_role {
                                println!("  {} Next: {}", "→".cyan(), next.display_name().cyan());
                            }
                        }
                    }

                    ExitCode::Prev => {
                        let prev_role = match current_role.prev() {
                            Some(r) => r,
                            None => {
                                // PM has no predecessor — just retry PM
                                logger.warn("pm", "PREV on PM — retrying PM");
                                state.current_role = Some(Role::PM);
                                continue;
                            }
                        };
                        println!(
                            "{}",
                            format!(
                                "  ← PREV: {} 재작업 필요",
                                prev_role.display_name()
                            )
                            .yellow()
                            .bold()
                        );
                        logger.warn(
                            &current_role.to_string(),
                            &format!("PREV → routing back to {}", prev_role),
                        );
                        state.completed_roles.retain(|r| r != &prev_role && r != &current_role);
                        state.current_role = Some(prev_role);
                    }

                    ExitCode::Resp => {
                        println!("{}", "\n⚠  사용자 확인 필요 (RESP)".yellow().bold());
                        logger.warn(&current_role.to_string(), "RESP — user input required");

                        for (i, q) in report.questions.iter().enumerate() {
                            println!("  {}. {}", i + 1, q.yellow());
                        }

                        if !args.dry_run {
                            let answer = Input::<String>::new()
                                .with_prompt("응답을 입력하세요")
                                .interact_text()?;

                            let resp_filename = format!(
                                "{}-{}-C{}-R{}-resp.md",
                                state.current_task_id,
                                current_role,
                                state.cycle,
                                retry
                            );
                            let resp_path = reports_dir.join(&resp_filename);
                            let resp_content = format!(
                                "# 사용자 응답 — {} 재시도 {}\n\n{}\n",
                                current_role.display_name(),
                                retry,
                                answer
                            );
                            if let Err(e) = write_file(&resp_path, &resp_content, path) {
                                logger.warn(
                                    &current_role.to_string(),
                                    &format!("RESP 저장 실패: {}", e),
                                );
                            }
                        } else {
                            println!("{}", "  [dry-run] RESP — 입력 수집 스킵".dimmed());
                        }
                        // retry++ is automatic (count_existing_reports counts resp files separately)
                    }
                }
            }
        }
    }

    println!();
    println!("{}", "세션 종료. 'porpoise'를 실행하여 재개하세요.".dimmed());
    logger.info("orchestrator", "Session ended");
    if args.verbose {
        println!(
            "{} {}",
            "Log:".dimmed(),
            logger.log_path().display().to_string().dimmed()
        );
    }
    Ok(())
}

fn check_token_warning(
    token_monitor: &TokenMonitor,
    current_role: &Role,
    state: &OrchestratorState,
    path: &Path,
    dry_run: bool,
    logger: &Logger,
    retry: u32,
) -> Result<bool> {
    let token_level = token_monitor.check_usage();
    token_monitor.display_warning(&token_level);
    if matches!(token_level, TokenWarningLevel::Critical(_)) {
        logger.warn(&current_role.to_string(), "Token critical before role exec");
        if dry_run {
            println!(
                "{}",
                "  [dry-run] Token usage critical — skipping checkpoint prompt".yellow()
            );
        } else {
            let save_and_exit = Confirm::new()
                .with_prompt("Token usage critical. Save checkpoint and exit?")
                .default(true)
                .interact()?;
            if save_and_exit {
                save_current_checkpoint(state, current_role, path, retry)?;
                println!("{}", "Checkpoint saved. Run 'porpoise' to resume.".cyan());
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn execute_role(
    executor: &RoleExecutor,
    current_role: &Role,
    context: &RoleContext,
    path: &Path,
    cycle: u32,
    task_id: &str,
    retry: u32,
    dry_run: bool,
    logger: &Logger,
    history: &mut Vec<String>,
) -> Result<RoleOutcome> {
    let report_result = if dry_run {
        executor.execute_role(current_role, context, path, true, task_id, cycle, retry)
    } else {
        let spinner = make_spinner(&format!("Running {} ...", current_role.display_name()));
        let result = executor.execute_role(current_role, context, path, false, task_id, cycle, retry);
        spinner.finish_and_clear();
        result
    };

    let report = match report_result {
        Ok(r) => {
            logger.role_end(&current_role.to_string(), cycle, true);
            r
        }
        Err(e) => {
            logger.role_end(&current_role.to_string(), cycle, false);
            logger.error(&current_role.to_string(), &e.to_string());
            println!("{} {}", "Error executing role:".red().bold(), e);
            if dry_run {
                return Ok(RoleOutcome::Stop);
            }
            let retry_choice = Confirm::new()
                .with_prompt("Retry this role?")
                .default(true)
                .interact()?;
            return Ok(if retry_choice {
                RoleOutcome::Retry
            } else {
                RoleOutcome::Stop
            });
        }
    };

    if !dry_run {
        let report_path = save_report(&report, path, task_id, cycle, retry)?;
        history.push(format!(
            "Cycle {} | {} | {} → {}",
            cycle,
            task_id,
            current_role.display_name(),
            report_path.file_name().unwrap_or_default().to_string_lossy()
        ));
        println!(
            "  {} Report: {}",
            "✓".green(),
            report_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .dimmed()
        );
        logger.info(
            &current_role.to_string(),
            &format!("Report saved: {}", report_path.display()),
        );
    }

    Ok(RoleOutcome::Report(report))
}

fn auto_commit(task_id: &str, task_title: &str) -> Result<()> {
    let message = format!("[{}] {}", task_id, task_title);

    let status = Command::new("git")
        .args(["add", "."])
        .status()
        .context("git add 실행 실패")?;
    if !status.success() {
        anyhow::bail!("git add 실패 (exit code: {})", status.code().unwrap_or(-1));
    }

    let status = Command::new("git")
        .args(["commit", "-m", &message])
        .status()
        .context("git commit 실행 실패")?;
    if !status.success() {
        anyhow::bail!("git commit 실패 (exit code: {})", status.code().unwrap_or(-1));
    }

    Ok(())
}

fn mark_task_complete(path: &Path, task_id: &str) -> Result<()> {
    let project_md_path = path.join(".docs").join("project.md");
    let content = std::fs::read_to_string(&project_md_path)
        .with_context(|| format!("project.md 읽기 실패: {}", project_md_path.display()))?;

    let marker = format!("- [ ] {}:", task_id);
    let replacement = format!("- [x] {}:", task_id);
    let new_content = content.replace(&marker, &replacement);

    write_file(&project_md_path, &new_content, path).context("project.md 업데이트 실패")?;
    Ok(())
}

fn all_tasks_done(path: &Path) -> bool {
    let tasks = parse_tasks_from_project_md(path);
    !tasks.is_empty() && tasks.iter().all(|t| t.completed)
}

fn run_release_flow(path: &Path) -> Result<()> {
    println!("{}", "\n=== 릴리즈 플로우 ===".green().bold());

    let branch_out = Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .context("git branch 실행 실패")?;
    let branch = String::from_utf8_lossy(&branch_out.stdout).trim().to_string();
    println!("  현재 브랜치: {}", branch.cyan());

    let tag_out = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output();
    let current_tag = match tag_out {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => "(태그 없음)".to_string(),
    };
    println!("  현재 버전: {}", current_tag.yellow());

    let new_tag = Input::<String>::new()
        .with_prompt("신규 릴리즈 태그 (비워두면 건너뜀)")
        .allow_empty(true)
        .interact_text()?;

    let new_tag = new_tag.trim().to_string();
    if new_tag.is_empty() {
        println!("{}", "릴리즈 건너뜀.".dimmed());
        return Ok(());
    }

    let status = Command::new("git")
        .args(["tag", "-a", &new_tag, "-m", &new_tag])
        .status()
        .context("git tag 실행 실패")?;
    if !status.success() {
        anyhow::bail!("git tag 실패 (exit code: {})", status.code().unwrap_or(-1));
    }

    let push_branch = if branch.is_empty() { "main" } else { &branch };
    let status = Command::new("git")
        .args(["push", "origin", push_branch, "--tags"])
        .status()
        .context("git push 실행 실패")?;
    if !status.success() {
        anyhow::bail!("git push 실패 (exit code: {})", status.code().unwrap_or(-1));
    }

    println!(
        "{}",
        format!(
            "릴리즈 완료: https://github.com/Jongh/porpoise/releases/tag/{}",
            new_tag
        )
        .green()
    );

    // Suppress unused warning — path may be used for future version-file updates
    let _ = path;
    Ok(())
}

fn print_resume_summary(state: &OrchestratorState) {
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
    println!(
        "  Task       : {} — {}",
        state.current_task_id.cyan(),
        state.current_task_title
    );
    println!("  Completed  : {}", completed_str);
    println!("  Next role  : {}", next_str);
    println!();
}

fn save_current_checkpoint(
    state: &OrchestratorState,
    current_role: &Role,
    path: &Path,
    retry_count: u32,
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
        &state.current_task_id,
        retry_count,
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
