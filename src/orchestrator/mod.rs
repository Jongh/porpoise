pub mod checkpoint;
pub mod milestone_session;
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

use crate::config::workspace::WorkspaceConfig;
use crate::config::Config;
use crate::logger::Logger;
use crate::milestone::update_task_status;
use crate::utils::fs::write_file;
use crate::utils::input::{collect_multiline_input, confirm_or_default};
use crate::Args;

use checkpoint::{save_checkpoint, Checkpoint};
use report::{count_existing_reports, parse_completed_tasks, parse_exit_code, parse_prev_target, report_filename, ExitCode, Report};
use roles::{build_context, find_latest_report, RoleContext, RoleExecutor};
use state::{load_state, parse_tasks_from_project_md, OrchestratorState, Role};

use crate::session;

enum RoleOutcome {
    Report(Report),
    Retry,
    Stop,
}


pub fn run(path: &Path, args: &Args, config: &Config) -> Result<()> {
    let logger = Logger::new(path, args.verbose)?;

    println!();
    println!("{}", "=== Porpoise Orchestration ===".green().bold());
    println!();

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
                "Unknown role: '{}'. Valid: planning, development, testing, review",
                from_role
            ),
        }
    }

    // Migration guard: detect legacy report/ folder and warn
    {
        let old_report_dir = path.join(".porpoise").join("report");
        if old_report_dir.exists() {
            println!(
                "{}",
                "\n⚠  구 버전 report/ 폴더가 감지되었습니다 (v0.3.1 이전 형식).\
                \n   이 폴더의 파일은 자동 라우팅에 사용되지 않습니다.\
                \n   reports/ 폴더로 이동 후 재실행하세요.\
                \n   Windows: ren .porpoise\\report .porpoise\\reports\
                \n   Unix:    mv .porpoise/report .porpoise/reports"
                    .yellow()
                    .bold()
            );
        }
    }

    let workspace = WorkspaceConfig::load(path).unwrap_or_default();

    // IMP-01: warn if workspace.toml is newer than generated prompt files
    {
        let ws_path = path.join(".porpoise").join("workspace.toml");
        let prompts_dir = path.join(".porpoise").join("prompts");
        if ws_path.exists() {
            if let Ok(ws_mtime) = std::fs::metadata(&ws_path).and_then(|m| m.modified()) {
                let outdated = ["01-planning.md", "02-development.md", "03-testing.md", "04-review.md"]
                    .iter()
                    .any(|f| {
                        std::fs::metadata(prompts_dir.join(f))
                            .and_then(|m| m.modified())
                            .map(|mtime| ws_mtime > mtime)
                            .unwrap_or(false)
                    });
                if outdated {
                    println!(
                        "{}",
                        "⚠  workspace.toml이 프롬프트 파일보다 최신입니다. 'porpoise --new'로 프롬프트를 재생성하세요. (이미 최신이라면 무시하세요.)".yellow()
                    );
                }
            }
        }
    }

    // IMP-03: validate prompt_overrides paths (verbose only)
    if args.verbose {
        for role_key in &["pm", "developer", "tester", "reviewer"] {
            if let Some(full_path) = workspace.resolved_override_path(role_key, path) {
                if !full_path.exists() {
                    println!(
                        "  {} prompt_overrides.{}: 파일 없음 — {}",
                        "⚠".yellow(),
                        role_key,
                        full_path.display()
                    );
                    logger.warn(
                        "orchestrator",
                        &format!("prompt_override {} 파일 없음: {}", role_key, full_path.display()),
                    );
                }
            }
        }
    }

    print_resume_summary(&state);

    if args.dry_run {
        println!("{}", "[DRY RUN MODE — no execution will happen]".yellow().bold());
        println!();
    }

    let effective_model = args.model.clone().or_else(|| config.model().map(str::to_string));

    // M1-T05: 미완료 작업 없으면 마일스톤 생성 세션 자동 진입
    {
        let tasks = parse_tasks_from_project_md(path);
        if tasks.iter().all(|t| t.completed) {
            logger.info("orchestrator", "No pending tasks — entering milestone session");
            println!("{}", "\n미완료 작업이 없습니다. 마일스톤 생성 세션을 시작합니다.".cyan().bold());
            milestone_session::run_milestone_session(path, args.dry_run, &logger, effective_model.as_deref(), &workspace)?;
            let new_tasks = parse_tasks_from_project_md(path);
            match new_tasks.iter().find(|t| !t.completed) {
                Some(next) => {
                    state.current_task_id = next.id.clone();
                    state.current_task_title = next.title.clone();
                    state.completed_roles = vec![];
                    state.current_role = Some(Role::PM);
                    logger.info(
                        "orchestrator",
                        &format!("Milestone session done — first task: {}", state.current_task_id),
                    );
                }
                None => {
                    println!("{}", "실행할 작업이 없습니다. 'porpoise'를 다시 실행하세요.".yellow());
                    return Ok(());
                }
            }
        }
    }

    // T07: 형식 감지 및 분기 — 신규 초기화 프로젝트는 `.porpoise/sessions/`를
    // 항상 생성하므로 JSON 기반 라우팅을 사용한다. 레거시 분기는 해당 폴더가
    // 없는 기존 워크스페이스의 v0.5.0 호환성 경로로만 유지한다.
    if session::is_new_format(path) {
        return run_new_format(path, args, config, &workspace, &state, effective_model.as_deref(), &logger);
    } else {
        println!("{}", "⚠  레거시 모드: sessions/ 폴더가 없습니다. v0.5.0 호환 모드로 실행합니다.".yellow());
        println!("{}", "   새 형식으로 전환하려면 sessions/ 폴더를 생성하세요.".dimmed());
    }

    let executor = RoleExecutor::new(effective_model.clone());
    let mut history: Vec<String> = Vec::new();
    let messages_dir = path.join(".porpoise").join("messages");
    let reports_dir = path.join(".porpoise").join("reports");

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

        // retry = reports/ 또는 messages/ 파일 수 중 큰 값 (두 폴더 모두 반영)
        let retry = std::cmp::max(
            count_existing_reports(&messages_dir, &state.current_task_id, &current_role.to_string(), state.cycle),
            count_existing_reports(&reports_dir, &state.current_task_id, &current_role.to_string(), state.cycle),
        );

        let msg_file = find_latest_report(&messages_dir, &current_role.to_string(), &state.current_task_id);
        let rpt_file = find_latest_report(&reports_dir, &current_role.to_string(), &state.current_task_id);

        // ── 라우팅 결정 ──────────────────────────────────────────────────────
        // reports/ 파일이 있으면 종료 코드로 라우팅 (실행 없음)
        // 없으면 RESP: messages/ 도 없으면 역할 실행 후 RESP
        let (exit_code, rpt_content) = if let Some(ref rf) = rpt_file {
            let content = std::fs::read_to_string(rf).unwrap_or_default();
            // [1] 종료 코드 없음 → NEXT 폴백 금지, 명시적 중단
            match parse_exit_code(&content) {
                Some(code) => {
                    println!(
                        "  {} reports/ 읽음: {} — {}",
                        "→".cyan(),
                        rf.file_name().unwrap_or_default().to_string_lossy().dimmed(),
                        format!("{:?}", code).yellow()
                    );
                    (code, content)
                }
                None => {
                    println!(
                        "{}",
                        format!(
                            "⚠ reports/ 파일 '{}' 에 유효한 종료 코드(NEXT/PREV)가 없습니다.\n  파일 마지막 줄을 NEXT 또는 PREV로 수정 후 재실행하세요.",
                            rf.file_name().unwrap_or_default().to_string_lossy()
                        ).yellow().bold()
                    );
                    logger.warn(&current_role.to_string(), "reports/ 종료 코드 없음 — 세션 중단");
                    break;
                }
            }
        } else {
            // RESP 상황 -------------------------------------------------------
            // messages/ 가 없으면 역할을 먼저 실행한다
            if msg_file.is_none() {
                if args.dry_run {
                    // dry-run: 실행 없이 NEXT로 처리
                    println!("{}", "  [dry-run] 역할 실행 후 RESP — NEXT로 처리".dimmed());
                    state.completed_roles.push(current_role.clone());
                    state.current_role = current_role.next();
                    continue;
                }

                if !confirm_or_default(&format!("Execute {}?", current_role.display_name()), true, args.yes)? {
                    logger.info(&current_role.to_string(), "Skipped by user");
                    println!("{}", "Skipped. Run 'porpoise' to resume later.".yellow());
                    break;
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
                    // Re-enter the loop so the next iteration reads the report Claude wrote
                    // to reports/ and routes based on exit code. If Claude didn't write a
                    // report, the next iteration finds msg_file=Some → RESP break as before.
                    RoleOutcome::Report(_) => continue,
                }
            } else if let Some(ref mf) = msg_file {
                println!(
                    "  {} messages/ 파일 있음: {}",
                    "⚠".yellow(),
                    mf.file_name().unwrap_or_default().to_string_lossy().dimmed()
                );
            }

            // reports/ 없음 → RESP: 사용자에게 hint 수집 후 세션 종료
            println!("{}", "\n💡 RESP — reports/ 파일 없음. 추가 지시사항을 입력하면 hint로 저장합니다.".cyan());
            println!(
                "{}",
                "  (Claude가 reports/ 폴더에 보고서를 저장하면 다음 실행 시 자동 라우팅됩니다.\n   또는 'porpoise approve NEXT|PREV'로 수동 판정을 생성할 수 있습니다.)".dimmed()
            );

            if !args.dry_run {
                save_resp_hints(&current_role, &state, retry, path, &logger)?;
            } else {
                println!("{}", "  [dry-run] RESP — hint 수집 스킵".dimmed());
            }

            break;
        };

        // ── 종료 코드 라우팅 ─────────────────────────────────────────────────
        match exit_code {
            ExitCode::Next => {
                if current_role == Role::Reviewer {
                    if !args.dry_run {
                        // Collect completed task IDs from PORPOISE_META
                        let mut task_ids = parse_completed_tasks(&rpt_content);

                        // BUG-03: deduplicate while preserving order
                        {
                            let mut seen = std::collections::HashSet::new();
                            task_ids.retain(|id| seen.insert(id.clone()));
                        }

                        // R-01: auto-include current_task_id if not listed
                        if !task_ids.contains(&state.current_task_id) {
                            if !task_ids.is_empty() {
                                println!(
                                    "  {} completed_tasks에 현재 작업 ID({})가 없어 자동 추가됩니다.",
                                    "⚠".yellow(),
                                    state.current_task_id
                                );
                            }
                            task_ids.push(state.current_task_id.clone());
                        }

                        // R-05: look up titles from project.md, warn on unknown IDs
                        let project_tasks = parse_tasks_from_project_md(path);
                        let commit_tasks: Vec<(String, String)> = task_ids
                            .iter()
                            .map(|id| {
                                let title = if *id == state.current_task_id {
                                    project_tasks
                                        .iter()
                                        .find(|t| t.id == *id)
                                        .map(|t| t.title.clone())
                                        .unwrap_or_else(|| state.current_task_title.clone())
                                } else {
                                    match project_tasks.iter().find(|t| t.id == *id) {
                                        Some(t) => t.title.clone(),
                                        None => {
                                            println!(
                                                "  {} completed_tasks의 작업 ID '{}'를 project.md에서 찾을 수 없음",
                                                "⚠".yellow(),
                                                id
                                            );
                                            String::new()
                                        }
                                    }
                                };
                                (id.clone(), title)
                            })
                            .collect();

                        match auto_commit(&commit_tasks) {
                            Ok(()) => {
                                let ids_str = commit_tasks
                                    .iter()
                                    .map(|(id, _)| id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                println!("  {} 커밋 완료: [{}]", "✓".green(), ids_str);
                                logger.info("reviewer", &format!("Auto-commit: [{}]", ids_str));
                            }
                            Err(e) => {
                                println!("{} {}", "⚠  자동 커밋 실패:".yellow(), e);
                                logger.warn("reviewer", &format!("Auto-commit failed: {}", e));
                            }
                        }

                        let commit_ids: Vec<String> = commit_tasks.iter().map(|(id, _)| id.clone()).collect();
                        if let Err(e) = mark_tasks_complete(path, &commit_ids, &logger) {
                            logger.warn("reviewer", &format!("Task mark failed: {}", e));
                        }
                    } else {
                        println!("{}", "  [dry-run] Reviewer NEXT — 자동 커밋 스킵".dimmed());
                    }

                    if !args.dry_run && all_tasks_done(path) {
                        println!("{}", "\n모든 작업 항목 완료!".green().bold());
                        logger.info("orchestrator", "All tasks completed");
                        print_history(&history);

                        let create_new = confirm_or_default(
                            "새 마일스톤을 생성하시겠습니까? (아니오: 릴리즈 플로우 진행)",
                            args.yes,
                            args.yes,
                        )?;

                        if create_new {
                            milestone_session::run_milestone_session(path, false, &logger, effective_model.as_deref(), &workspace)?;
                            let new_tasks = parse_tasks_from_project_md(path);
                            if let Some(next) = new_tasks.iter().find(|t| !t.completed) {
                                println!("  {} 다음 작업: {} — {}", "→".cyan(), next.id.cyan(), next.title);
                                state.current_task_id = next.id.clone();
                                state.current_task_title = next.title.clone();
                                state.completed_roles = vec![];
                                state.current_role = Some(Role::PM);
                                logger.info("orchestrator", &format!("New milestone task: {}", state.current_task_id));
                                continue;
                            } else {
                                println!("{}", "새 마일스톤이 생성되지 않았습니다.".yellow());
                                break;
                            }
                        }

                        if let Err(e) = run_release_flow(config.github_repo()) {
                            println!("{} {}", "⚠  릴리즈 플로우 오류:".yellow(), e);
                            logger.warn("orchestrator", &format!("Release flow error: {}", e));
                        }
                        break;
                    }

                    let tasks = parse_tasks_from_project_md(path);
                    if let Some(next_task) = tasks.iter().find(|t| !t.completed) {
                        println!("  {} 다음 작업: {} — {}", "→".cyan(), next_task.id.cyan(), next_task.title);
                        state.current_task_id = next_task.id.clone();
                        state.current_task_title = next_task.title.clone();
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                        logger.info("orchestrator", &format!("Next task: {}", state.current_task_id));
                    } else {
                        if args.dry_run {
                            println!("{}", "  [dry-run] No structured tasks — stopping".dimmed());
                            break;
                        }
                        state.cycle += 1;
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                        logger.info("orchestrator", &format!("New cycle: {}", state.cycle));
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
                // [2] prev_target 메타데이터가 있으면 해당 역할부터 재시작 (사이클 유지)
                let target_role = parse_prev_target(&rpt_content)
                    .and_then(|t| Role::from_str(&t));

                match target_role {
                    Some(ref role) if *role != Role::PM => {
                        let start_idx = Role::all().iter().position(|r| r == role).unwrap_or(0);
                        state.completed_roles = Role::all()[..start_idx].to_vec();
                        state.current_role = Some(role.clone());
                        println!(
                            "{}",
                            format!(
                                "  ← PREV → {} 역할부터 재시작 (사이클 {} 유지)",
                                role.display_name(),
                                state.cycle
                            )
                            .yellow()
                            .bold()
                        );
                        logger.warn(
                            &current_role.to_string(),
                            &format!("PREV → {} (cycle {} retained)", role, state.cycle),
                        );
                    }
                    _ => {
                        state.cycle += 1;
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                        println!(
                            "{}",
                            format!("  ← PREV: 사이클 {} — Planning부터 재시작", state.cycle)
                                .yellow()
                                .bold()
                        );
                        logger.warn(
                            &current_role.to_string(),
                            &format!("PREV → cycle {} restarting from Planning", state.cycle),
                        );
                    }
                }
            }

            ExitCode::Resp => {
                // reports/ 파일이 RESP를 담고 있는 경우: NEXT로 처리
                logger.warn(&current_role.to_string(), "reports/ 파일의 RESP 코드 — NEXT로 처리");
                state.completed_roles.push(current_role.clone());
                state.current_role = current_role.next();
                if let Some(ref next) = state.current_role {
                    println!("  {} Next: {}", "→".cyan(), next.display_name().cyan());
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
        let spinner = make_spinner(&format!("[ Cycle {} | {} ] Running {} ...", cycle, task_id, current_role.display_name()));
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
        let filename = report_filename(task_id, &report.role, cycle, retry);
        history.push(format!(
            "Cycle {} | {} | {} → {}",
            cycle,
            task_id,
            current_role.display_name(),
            filename
        ));
        println!(
            "  {} Report: {}",
            "✓".green(),
            filename.dimmed()
        );
    }

    Ok(RoleOutcome::Report(report))
}

fn collect_stageable_files(paths: &[&str]) -> Result<Vec<String>> {
    let mut files = Vec::new();

    let modified = Command::new("git")
        .args(["ls-files", "--modified", "--"])
        .args(paths)
        .output()
        .context("git ls-files --modified 실행 실패")?;

    let untracked = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "--"])
        .args(paths)
        .output()
        .context("git ls-files --others 실행 실패")?;

    for line in String::from_utf8_lossy(&modified.stdout).lines() {
        if !line.is_empty() {
            files.push(line.to_string());
        }
    }
    for line in String::from_utf8_lossy(&untracked.stdout).lines() {
        if !line.is_empty() {
            files.push(line.to_string());
        }
    }

    let files = filter_gitignored_files(files);
    let files = files
        .into_iter()
        .filter(|f| std::fs::metadata(f).is_ok())
        .collect();
    Ok(files)
}

fn filter_gitignored_files(files: Vec<String>) -> Vec<String> {
    if files.is_empty() {
        return files;
    }

    let mut child = match Command::new("git")
        .args(["check-ignore", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return files,
    };

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        for f in &files {
            let _ = writeln!(stdin, "{}", f);
        }
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => return files,
    };

    let ignored: std::collections::HashSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.replace('\\', "/"))
        .collect();

    files.into_iter().filter(|f| !ignored.contains(&f.replace('\\', "/"))).collect()
}

fn auto_commit(tasks: &[(String, String)]) -> Result<()> {
    if tasks.is_empty() {
        return Ok(());
    }
    let subject = if tasks.len() == 1 {
        format!("[{}] 작업 완료", tasks[0].0)
    } else {
        let ids = tasks.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(", ");
        format!("[{}] 작업 완료", ids)
    };
    let body = tasks
        .iter()
        .map(|(id, title)| {
            if title.is_empty() {
                format!("- {}", id)
            } else {
                format!("- {}: {}", id, title)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let message = format!("{}\n\n{}", subject, body);
    let target_paths = [".porpoise/", "Cargo.toml", "Cargo.lock", "src/", "README.md", "wix/"];

    let files = collect_stageable_files(&target_paths)?;
    if files.is_empty() {
        return Ok(());
    }

    let batch_output = Command::new("git")
        .arg("add")
        .arg("--")
        .args(&files)
        .output()
        .context("git add 실행 실패")?;

    if !batch_output.status.success() {
        let mut staged_count = 0usize;
        let mut failed_files: Vec<String> = Vec::new();
        for file in &files {
            let out = Command::new("git")
                .args(["add", "--", file])
                .output()
                .context("git add 실행 실패")?;
            if out.status.success() {
                staged_count += 1;
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                failed_files.push(format!("{} ({})", file, stderr.trim()));
            }
        }
        if !failed_files.is_empty() {
            println!("{} {}", "⚠  git add 실패 (스킵):".yellow(), failed_files.join(", "));
        }
        if staged_count == 0 {
            println!("{}", "⚠  스테이징된 파일 없음 — 커밋 건너뜀".yellow());
            return Ok(());
        }
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

fn mark_tasks_complete(path: &Path, task_ids: &[String], logger: &Logger) -> Result<()> {
    let project_md_path = path.join(".porpoise").join("project.md");
    let mut content = std::fs::read_to_string(&project_md_path)
        .with_context(|| format!("project.md 읽기 실패: {}", project_md_path.display()))?;

    let mut updated = 0usize;
    for task_id in task_ids {
        let marker = format!("- [ ] {}:", task_id);
        let replacement = format!("- [x] {}:", task_id);
        if content.contains(&marker) {
            content = content.replace(&marker, &replacement);
            updated += 1;
            update_task_status(path, task_id, true, logger);
        } else {
            println!(
                "  {} project.md에서 '{}' 미완료 마커를 찾을 수 없음 (이미 완료 또는 ID 불일치)",
                "⚠".yellow(),
                task_id
            );
            logger.warn("reviewer", &format!("project.md에서 미완료 마커를 찾을 수 없음: {}", task_id));
        }
    }

    if updated > 0 {
        write_file(&project_md_path, &content, path).context("project.md 업데이트 실패")?;
    }

    Ok(())
}

fn all_tasks_done(path: &Path) -> bool {
    let tasks = parse_tasks_from_project_md(path);
    !tasks.is_empty() && tasks.iter().all(|t| t.completed)
}

fn run_release_flow(github_repo: Option<&str>) -> Result<()> {
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
    loop {
        let status = Command::new("git")
            .args(["push", "origin", push_branch, "--tags"])
            .status()
            .context("git push 실행 실패")?;
        if status.success() {
            break;
        }
        println!(
            "{}",
            format!("⚠ git push 실패 (exit code: {})", status.code().unwrap_or(-1)).yellow()
        );
        let retry = Confirm::new()
            .with_prompt("다시 시도하시겠습니까? (아니오: 릴리즈를 건너뜁니다)")
            .default(true)
            .interact()?;
        if !retry {
            println!("{}", "릴리즈 건너뜀.".dimmed());
            return Ok(());
        }
    }

    let base = github_repo
        .map(|repo| format!("https://github.com/{}/releases/tag/", repo))
        .unwrap_or_else(|| "https://github.com/Jongh/porpoise/releases/tag/".to_string());
    println!("{}", format!("릴리즈 완료: {}{}", base, new_tag).green());

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
        state.prev_reasons.clone(),
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

fn save_resp_hints(
    current_role: &Role,
    state: &OrchestratorState,
    retry: u32,
    path: &Path,
    logger: &Logger,
) -> Result<()> {
    let input = collect_multiline_input("추가 지시사항 입력 (hint로 저장)")?;
    if input.trim().is_empty() {
        println!("{}", "  (입력 없음 — hint 저장 건너뜀)".dimmed());
        return Ok(());
    }
    let hints_dir = path.join(".porpoise").join("hints");
    std::fs::create_dir_all(&hints_dir).context("hints 디렉토리 생성 실패")?;
    let filename = format!(
        "{}-{}-C{}-R{}-hints.md",
        state.current_task_id,
        current_role,
        state.cycle,
        retry,
    );
    let hint_path = hints_dir.join(&filename);
    write_file(&hint_path, &input, path)?;
    println!("  {} hint 저장: {}", "✓".green(), filename.dimmed());
    logger.info(&current_role.to_string(), &format!("RESP hint 저장: {}", filename));
    Ok(())
}

// ─── v0.6.0 JSON 기반 라우팅 ──────────────────────────────────────────────────

fn run_new_format(
    path: &Path,
    args: &Args,
    config: &Config,
    workspace: &WorkspaceConfig,
    initial_state: &OrchestratorState,
    effective_model: Option<&str>,
    logger: &Logger,
) -> Result<()> {
    use crate::model::factory::make_adapter;
    use crate::session::{self, find_latest_session};

    let mut state = initial_state.clone();
    let mut history: Vec<String> = Vec::new();

    // IMP-02: JSON 출력 섹션 미존재 경고 (기존 프로젝트가 porpoise --new 미실행 시)
    {
        let prompts_dir = path.join(".porpoise").join("prompts");
        let needs_update = ["01-planning.md", "02-development.md", "03-testing.md", "04-review.md"]
            .iter()
            .any(|f| {
                std::fs::read_to_string(prompts_dir.join(f))
                    .map(|c| !c.contains("JSON 출력 형식"))
                    .unwrap_or(false)
            });
        if needs_update {
            println!(
                "{}",
                "⚠  프롬프트 파일에 JSON 출력 섹션이 없습니다. 'porpoise --new'를 실행해 프롬프트를 최신화하세요.".yellow()
            );
        }
    }

    let adapter = match make_adapter(workspace, path) {
        Ok(a) => a,
        Err(e) => {
            println!("{} {}", "⚠ 어댑터 초기화 실패:".red().bold(), e);
            return Err(e);
        }
    };

    loop {
        let current_role = match &state.current_role {
            Some(r) => r.clone(),
            None => {
                println!("{}", "All roles completed for this cycle.".green().bold());
                break;
            }
        };

        println!(
            "{}",
            format!(
                "\n[ Cycle {} | {} ] ─── {} ─── [JSON mode]",
                state.cycle, state.current_task_id, current_role.display_name()
            ).bold()
        );

        // retry = 현재 사이클의 기존 세션 수
        let retry = session::count_existing_sessions(
            path,
            &state.current_task_id,
            &current_role.to_string(),
            state.cycle,
        );

        // 현재 사이클의 완료된 세션만 재사용 (다른 사이클 세션은 무시)
        let cached_output = find_latest_session(
            path,
            &state.current_task_id,
            &current_role.to_string(),
        ).and_then(|sf| {
            let content = std::fs::read_to_string(&sf).ok()?;
            let env: session::SessionEnvelope = serde_json::from_str(&content).ok()?;
            if env.cycle != state.cycle {
                return None;  // 다른 사이클 세션 — 재사용 금지
            }
            if env.output.is_none() {
                println!("{}", "⚠ 세션 파일이 있지만 output이 없습니다. 재실행합니다.".yellow());
                return None;
            }
            let o = env.output.clone()?;
            let fname = sf.file_name().unwrap_or_default().to_string_lossy().to_string();
            println!(
                "  {} sessions/ 읽음: {} — {}",
                "→".cyan(),
                fname.dimmed(),
                format!("{}", o.status()).yellow()
            );
            Some(o)
        });

        let output_data = if let Some(o) = cached_output {
            o
        } else {
            // 현재 사이클의 세션 없음 → 역할 실행
            if args.dry_run {
                println!("{}", "  [dry-run] 역할 실행 후 NEXT로 처리".dimmed());
                state.completed_roles.push(current_role.clone());
                state.current_role = current_role.next();
                continue;
            }

            if !confirm_or_default(&format!("Execute {}?", current_role.display_name()), true, args.yes)? {
                println!("{}", "Skipped.".yellow());
                break;
            }

            save_current_checkpoint(&state, &current_role, path, retry)?;
            logger.role_start(&current_role.to_string(), state.cycle);

            let spinner = make_spinner(&format!(
                "[ Cycle {} | {} ] Running {} ...",
                state.cycle, state.current_task_id, current_role.display_name()
            ));

            // SessionInput 구성
            let mut input = build_session_input(&state, path, workspace)?;

            // 이전 Developer 역할의 실행 결과 주입
            if !state.pending_execution_results.is_empty() {
                input.execution_results = std::mem::take(&mut state.pending_execution_results);
            }

            // 파일 미디에이션: API 어댑터이면 WorkspaceSnapshot 주입
            if adapter.requires_file_mediation() {
                let target_files = collect_target_files_from_planning(&state, path);
                let budget = workspace.snapshot_token_budget();
                match crate::workspace::snapshot::build_workspace_snapshot(path, &target_files, budget) {
                    Ok(snapshot) => input.workspace_snapshot = Some(snapshot),
                    Err(e) => logger.warn("orchestrator", &format!("snapshot 빌드 실패: {}", e)),
                }
            }

            let result = execute_role_new(
                &*adapter, &current_role, input, path, workspace,
                state.cycle, retry, false, effective_model, logger,
            );
            spinner.finish_and_clear();

            match result {
                Ok(o) => {
                    logger.role_end(&current_role.to_string(), state.cycle, true);

                    // 파일 미디에이션 후처리: Developer 역할 완료 시 파일 적용 + Verify
                    if adapter.requires_file_mediation() && current_role == Role::Developer {
                        if let Some(ops) = o.file_operations() {
                            match crate::workspace::apply::apply_file_operations(path, ops) {
                                Ok(summary) => println!(
                                    "  {} 파일 적용: 작성={} 삭제={} 이동={}",
                                    "✓".green(), summary.files_written, summary.files_deleted, summary.files_renamed
                                ),
                                Err(e) => logger.warn("orchestrator", &format!("파일 적용 실패: {}", e)),
                            }
                        }
                        let verify_cmds = o.verify_commands().cloned()
                            .unwrap_or_else(|| workspace.default_verify_commands());
                        if !verify_cmds.is_empty() {
                            let results = crate::workspace::executor::run_verify_commands(
                                path,
                                &verify_cmds,
                                &workspace.allowed_command_prefixes(),
                                workspace.verify_timeout_secs(),
                            );
                            let passed = results.iter().filter(|r| r.exit_code == 0).count();
                            println!("  {} 검증: {}/{} 통과", "✓".green(), passed, results.len());
                            state.pending_execution_results = results;
                        }
                    }

                    o
                }
                Err(e) => {
                    logger.role_end(&current_role.to_string(), state.cycle, false);
                    println!("{} {}", "Error:".red().bold(), e);
                    let retry_choice = dialoguer::Confirm::new()
                        .with_prompt("Retry?")
                        .default(true)
                        .interact()?;
                    if retry_choice { continue; } else { break; }
                }
            }
        };

        // ── 히스토리 기록 ─────────────────────────────────────────────────────
        history.push(format!(
            "[{} / C{}] {} → {}",
            state.current_task_id, state.cycle,
            current_role.display_name(),
            output_data.status()
        ));

        // ── 라우팅 ────────────────────────────────────────────────────────────
        match output_data.status() {
            session::ExitCode::Next => {
                if current_role == Role::Reviewer {
                    if !args.dry_run {
                        let mut task_ids = output_data.completed_tasks().to_vec();
                        let mut seen = std::collections::HashSet::new();
                        task_ids.retain(|id| seen.insert(id.clone()));
                        if !task_ids.contains(&state.current_task_id) {
                            task_ids.push(state.current_task_id.clone());
                        }
                        let project_tasks = parse_tasks_from_project_md(path);
                        let commit_tasks: Vec<(String, String)> = task_ids.iter().map(|id| {
                            let title = project_tasks.iter().find(|t| t.id == *id)
                                .map(|t| t.title.clone())
                                .unwrap_or_else(|| {
                                    if *id == state.current_task_id {
                                        state.current_task_title.clone()
                                    } else {
                                        String::new()
                                    }
                                });
                            (id.clone(), title)
                        }).collect();

                        match auto_commit(&commit_tasks) {
                            Ok(()) => println!("  {} 커밋 완료", "✓".green()),
                            Err(e) => println!("{} {}", "⚠ 커밋 실패:".yellow(), e),
                        }
                        let commit_ids: Vec<String> = commit_tasks.iter().map(|(id, _)| id.clone()).collect();
                        let _ = mark_tasks_complete(path, &commit_ids, logger);

                        if output_data.milestone_complete() && !all_tasks_done(path) {
                            println!("{}", "⚠  Reviewer가 milestone_complete=true를 반환했지만 project.md에 미완료 작업이 있습니다. project.md를 확인하세요.".yellow());
                            logger.warn("reviewer", "milestone_complete=true 반환, all_tasks_done=false — project.md 불일치");
                        }
                    }

                    if !args.dry_run && all_tasks_done(path) {
                        println!("{}", "\n모든 작업 항목 완료!".green().bold());
                        let create_new = confirm_or_default(
                            "새 마일스톤을 생성하시겠습니까?",
                            args.yes,
                            args.yes,
                        )?;
                        if create_new {
                            milestone_session::run_milestone_session(path, false, logger, effective_model, workspace)?;
                            let new_tasks = parse_tasks_from_project_md(path);
                            if let Some(next) = new_tasks.iter().find(|t| !t.completed) {
                                println!("  {} 다음 작업: {} — {}", "→".cyan(), next.id.cyan(), next.title);
                                state.current_task_id = next.id.clone();
                                state.current_task_title = next.title.clone();
                                state.completed_roles = vec![];
                                state.current_role = Some(Role::PM);
                                logger.info("orchestrator", &format!("New milestone task: {}", state.current_task_id));
                                continue;
                            }
                            println!("{}", "새 마일스톤이 생성되지 않았습니다.".yellow());
                        } else {
                            let _ = run_release_flow(config.github_repo());
                        }
                        break;
                    }

                    let tasks = parse_tasks_from_project_md(path);
                    if let Some(next_task) = tasks.iter().find(|t| !t.completed) {
                        state.current_task_id = next_task.id.clone();
                        state.current_task_title = next_task.title.clone();
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                    } else {
                        state.cycle += 1;
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                    }
                } else {
                    state.completed_roles.push(current_role.clone());
                    state.current_role = current_role.next();
                    if let Some(ref next) = state.current_role {
                        println!("  {} Next: {}", "→".cyan(), next.display_name().cyan());
                    }
                }
            }

            session::ExitCode::Prev => {
                // prev_reason 수집 (최근 3개 유지)
                if let Some(reason) = output_data.prev_reason() {
                    state.prev_reasons.push(reason.to_string());
                    if state.prev_reasons.len() > 3 {
                        state.prev_reasons.remove(0);
                    }
                }

                let target_role = output_data.prev_target().and_then(|t| Role::from_str(t));
                match target_role {
                    Some(ref role) if *role != Role::PM => {
                        invalidate_sessions_from_role(path, &state.current_task_id, role, state.cycle, logger);
                        let start_idx = Role::all().iter().position(|r| r == role).unwrap_or(0);
                        state.completed_roles = Role::all()[..start_idx].to_vec();
                        state.current_role = Some(role.clone());
                        println!("{}", format!("  ← PREV → {}", role.display_name()).yellow().bold());
                    }
                    _ => {
                        state.cycle += 1;
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                        println!("{}", format!("  ← PREV: 사이클 {} — Planning부터 재시작", state.cycle).yellow().bold());
                    }
                }
            }

            session::ExitCode::Resp => {
                if !args.dry_run {
                    save_resp_hints(&current_role, &state, retry, path, logger)?;
                }
                break;
            }
        }
    }

    print_history(&history);
    println!();
    println!("{}", "세션 종료.".dimmed());
    Ok(())
}

fn invalidate_sessions_from_role(
    path: &Path,
    task_id: &str,
    start_role: &Role,
    cycle: u32,
    logger: &Logger,
) {
    use crate::orchestrator::state::TaskId;
    let sessions_dir = path.join(".porpoise").join("sessions");
    let start_idx = Role::all().iter().position(|r| r == start_role).unwrap_or(0);
    let role_strs: Vec<String> = Role::all()[start_idx..]
        .iter()
        .map(|r| r.to_string())
        .collect();
    let normalized = TaskId::new(task_id).to_string();

    let Ok(entries) = std::fs::read_dir(&sessions_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        for role_str in &role_strs {
            let pattern = format!("{}-{}-C{}-R", normalized, role_str, cycle);
            if name.starts_with(&pattern) && name.ends_with(".json") {
                let invalidated = entry.path().with_extension("json.prev-invalidated");
                match std::fs::rename(entry.path(), &invalidated) {
                    Ok(_) => logger.info("orchestrator", &format!("PREV: 세션 무효화 → {}", name)),
                    Err(e) => logger.warn("orchestrator", &format!("세션 무효화 실패 {}: {}", name, e)),
                }
                break;
            }
        }
    }
}

fn collect_target_files_from_planning(state: &OrchestratorState, path: &Path) -> Vec<String> {
    let sf = match crate::session::find_latest_session(path, &state.current_task_id, "planning") {
        Some(p) => p,
        None => return vec![],
    };
    let content = match std::fs::read_to_string(&sf) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let env: crate::session::SessionEnvelope = match serde_json::from_str(&content) {
        Ok(e) => e,
        Err(_) => return vec![],
    };
    if let Some(crate::session::RoleOutputData::Planning(p)) = env.output {
        p.implementation_plan
            .iter()
            .flat_map(|step| step.target_files.iter().cloned())
            .collect()
    } else {
        vec![]
    }
}

fn build_session_input(
    state: &OrchestratorState,
    path: &Path,
    workspace: &WorkspaceConfig,
) -> Result<crate::session::SessionInput> {
    use crate::session::input::{MilestoneInfo, PreviousReports, SessionInput};
    use crate::session::SessionEnvelope;

    let project_summary = std::fs::read_to_string(path.join(".porpoise").join("project.md"))
        .unwrap_or_default();

    // 마일스톤 정보 추출
    let milestone_id = state.current_task_id
        .split('-')
        .next()
        .unwrap_or("M1")
        .to_string();
    let milestone = {
        let milestone_file = path
            .join(".porpoise")
            .join("milestones")
            .join(format!("{}.md", milestone_id));

        let (title, version, goal) = if milestone_file.exists() {
            match crate::milestone::parser::parse_milestone_file(&milestone_file) {
                Ok(m) => {
                    let goal = m.raw_sections.get("목표").cloned().unwrap_or_default();
                    (m.title, m.version.unwrap_or_default(), goal)
                }
                Err(_) => (String::new(), String::new(), String::new()),
            }
        } else {
            (String::new(), String::new(), String::new())
        };

        MilestoneInfo {
            id: milestone_id,
            title,
            version,
            goal,
        }
    };

    // 이전 역할 세션 로드
    let load_output = |role_str: &str| -> Option<crate::session::RoleOutputData> {
        let sf = crate::session::find_latest_session(path, &state.current_task_id, role_str)?;
        let content = std::fs::read_to_string(&sf).ok()?;
        let envelope: SessionEnvelope = serde_json::from_str(&content).ok()?;
        envelope.output
    };

    let previous_reports = {
        let planning = load_output("planning").and_then(|o| {
            if let crate::session::RoleOutputData::Planning(p) = o { Some(p) } else { None }
        });
        let development = load_output("development").and_then(|o| {
            if let crate::session::RoleOutputData::Development(d) = o { Some(d) } else { None }
        });
        let testing = load_output("testing").and_then(|o| {
            if let crate::session::RoleOutputData::Testing(t) = o { Some(t) } else { None }
        });
        let review = if state.cycle > 1 {
            load_output("review").and_then(|o| {
                if let crate::session::RoleOutputData::Review(r) = o { Some(r) } else { None }
            })
        } else {
            None
        };

        PreviousReports { planning, development, testing, review }
    };

    // 힌트 파일 로드
    let hints_dir = path.join(".porpoise").join("hints");
    let role_str = state.current_role.as_ref().map(|r| r.to_string()).unwrap_or_default();
    let hints = std::fs::read_dir(&hints_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with(&format!("{}-{}-", state.current_task_id, role_str))
                        && name.contains("-hints")
                        && name.ends_with(".md")
                    {
                        std::fs::read_to_string(e.path()).ok()
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // role_str("planning"/"development"/"testing"/"review") → workspace key("pm"/"developer"/"tester"/"reviewer")
    let workspace_role_key = match role_str.as_str() {
        "planning" => "pm",
        "development" => "developer",
        "testing" => "tester",
        "review" => "reviewer",
        other => other,
    };
    let role_extra = workspace.role_extra_formatted(workspace_role_key);

    Ok(SessionInput {
        role: role_str,
        task_id: state.current_task_id.clone(),
        task_title: state.current_task_title.clone(),
        cycle: state.cycle,
        retry: 0,
        language: workspace.language().to_string(),
        project_summary,
        conventions: workspace.convention_lines(),
        dod: workspace.dod_items(),
        milestone,
        previous_reports,
        hints,
        prev_reasons: state.prev_reasons.clone(),
        workspace_snapshot: None,
        execution_results: vec![],
        tech_context: workspace.tech_context(),
        role_extra,
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_role_new(
    adapter: &dyn crate::model::adapter::ModelAdapter,
    current_role: &Role,
    mut input: crate::session::SessionInput,
    path: &Path,
    workspace: &WorkspaceConfig,
    cycle: u32,
    retry: u32,
    dry_run: bool,
    effective_model: Option<&str>,
    logger: &Logger,
) -> Result<crate::session::RoleOutputData> {
    use crate::model::factory::make_model_config;
    use crate::session::{save_session, session_filename, SessionEnvelope};
    use crate::session::renderer;
    use chrono::Local;

    if dry_run {
        use crate::session::ExitCode;
        let role_str = current_role.to_string();
        let task_id = input.task_id.clone();
        let status = ExitCode::Next;
        let summary = "[dry-run]".to_string();
        return Ok(match current_role {
            Role::PM => crate::session::RoleOutputData::Planning(crate::session::planning::PlanningOutput {
                role: role_str, task_id, cycle, status, summary, ..Default::default()
            }),
            Role::Developer => crate::session::RoleOutputData::Development(crate::session::development::DevelopmentOutput {
                role: role_str, task_id, cycle, status, summary, ..Default::default()
            }),
            Role::Tester => crate::session::RoleOutputData::Testing(crate::session::testing::TestingOutput {
                role: role_str, task_id, cycle, status, summary, ..Default::default()
            }),
            Role::Reviewer => crate::session::RoleOutputData::Review(crate::session::review::ReviewOutput {
                role: role_str, task_id, cycle, status, summary, ..Default::default()
            }),
        });
    }

    input.retry = retry;

    let mut config = make_model_config(workspace, current_role);
    // args.model 오버라이드
    if let Some(m) = effective_model {
        if !m.is_empty() {
            config.model_id = m.to_string();
        }
    }

    let output = adapter.execute(&input, &config)?;
    let raw_text = adapter.last_raw_text();

    let envelope = SessionEnvelope {
        schema_version: "1".to_string(),
        task_id: input.task_id.clone(),
        role: current_role.to_string(),
        cycle,
        retry,
        timestamp: Local::now().to_rfc3339(),
        model: config.model_id.clone(),
        adapter: adapter.adapter_name().to_string(),
        input: input.clone(),
        output: Some(output.clone()),
        raw_text,
    };

    save_session(path, &envelope)?;
    if let Err(e) = renderer::render_and_save_report(path, &envelope) {
        logger.warn(&current_role.to_string(), &format!("reports/ 마크다운 생성 실패: {}", e));
    }

    let filename = session_filename(&input.task_id, &current_role.to_string(), cycle, retry);
    println!("  {} Session: {}", "✓".green(), filename.dimmed());

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidate_sessions_from_role_renames_files() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();

        std::fs::write(sessions.join("M1-T01-planning-C1-R0.json"), "{}").unwrap();
        std::fs::write(sessions.join("M1-T01-development-C1-R0.json"), "{}").unwrap();
        std::fs::write(sessions.join("M1-T01-testing-C1-R0.json"), "{}").unwrap();

        let logger = Logger::new(dir.path(), false).unwrap();
        invalidate_sessions_from_role(dir.path(), "M1-T01", &Role::Developer, 1, &logger);

        assert!(sessions.join("M1-T01-planning-C1-R0.json").exists());
        assert!(!sessions.join("M1-T01-development-C1-R0.json").exists());
        assert!(sessions.join("M1-T01-development-C1-R0.json.prev-invalidated").exists());
        assert!(!sessions.join("M1-T01-testing-C1-R0.json").exists());
        assert!(sessions.join("M1-T01-testing-C1-R0.json.prev-invalidated").exists());
    }

    #[test]
    fn invalidate_sessions_from_role_skips_other_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();

        std::fs::write(sessions.join("M1-T01-development-C1-R0.json"), "{}").unwrap();
        std::fs::write(sessions.join("M1-T01-development-C2-R0.json"), "{}").unwrap();

        let logger = Logger::new(dir.path(), false).unwrap();
        invalidate_sessions_from_role(dir.path(), "M1-T01", &Role::Developer, 1, &logger);

        assert!(!sessions.join("M1-T01-development-C1-R0.json").exists());
        assert!(sessions.join("M1-T01-development-C1-R0.json.prev-invalidated").exists());
        assert!(sessions.join("M1-T01-development-C2-R0.json").exists());
    }
}
