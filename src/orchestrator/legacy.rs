use anyhow::Result;
use colored::Colorize;
use dialoguer::Confirm;
use std::path::Path;

use crate::config::workspace::WorkspaceConfig;
use crate::config::Config;
use crate::logger::Logger;
use crate::utils::input::confirm_or_default;
use crate::Args;

use super::milestone_session;
use super::report::{
    count_existing_reports, parse_completed_tasks, parse_exit_code, parse_prev_target,
    report_filename, ExitCode,
};
use super::roles::{build_context, find_latest_report, RoleContext, RoleExecutor};
use super::state::{load_state, parse_tasks_from_project_md, Role};

enum RoleOutcome {
    Report,
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

    super::print_resume_summary(&state);

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
    if crate::session::is_new_format(path) {
        return super::new_format::run_new_format(path, args, config, &workspace, &state, effective_model.as_deref(), &logger);
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
        // 없으면 RESP: messages/ 도 없으면 단계 실행 후 RESP
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
            // messages/ 가 없으면 단계를 먼저 실행한다
            if msg_file.is_none() {
                if args.dry_run {
                    // dry-run: 실행 없이 NEXT로 처리
                    println!("{}", "  [dry-run] 단계 실행 후 RESP — NEXT로 처리".dimmed());
                    state.completed_roles.push(current_role.clone());
                    state.current_role = current_role.next();
                    continue;
                }

                if !confirm_or_default(&format!("Execute {}?", current_role.display_name()), true, args.yes)? {
                    logger.info(&current_role.to_string(), "Skipped by user");
                    println!("{}", "Skipped. Run 'porpoise' to resume later.".yellow());
                    break;
                }

                super::save_current_checkpoint(&state, &current_role, path, retry)?;
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
                    RoleOutcome::Report => continue,
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
                super::save_resp_hints(&current_role, &state, retry, path, &logger)?;
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

                        match super::auto_commit(&commit_tasks) {
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
                        if let Err(e) = super::mark_tasks_complete(path, &commit_ids, &logger) {
                            logger.warn("reviewer", &format!("Task mark failed: {}", e));
                        }
                    } else {
                        println!("{}", "  [dry-run] Review NEXT — 자동 커밋 스킵".dimmed());
                    }

                    if !args.dry_run && super::all_tasks_done(path) {
                        println!("{}", "\n모든 작업 항목 완료!".green().bold());
                        logger.info("orchestrator", "All tasks completed");
                        super::print_history(&history);

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
                                state.cycle = 1;
                                logger.info("orchestrator", &format!("New milestone task: {}", state.current_task_id));
                                continue;
                            } else {
                                println!("{}", "새 마일스톤이 생성되지 않았습니다.".yellow());
                                break;
                            }
                        }

                        if let Err(e) = super::run_release_flow(config.github_repo()) {
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
                        state.cycle = 1;
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
                // [2] prev_target 메타데이터가 있으면 해당 단계부터 재시작 (사이클 유지)
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
                                "  ← PREV → {} 단계부터 재시작 (사이클 {} 유지)",
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
        let spinner = super::make_spinner(&format!("[ Cycle {} | {} ] Running {} ...", cycle, task_id, current_role.display_name()));
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

    let _ = report;
    Ok(RoleOutcome::Report)
}
