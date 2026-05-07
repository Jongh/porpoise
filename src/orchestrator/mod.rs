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

use crate::config::Config;
use crate::logger::Logger;
use crate::milestone::update_task_status;
use crate::utils::fs::write_file;
use crate::utils::input::{collect_multiline_input, confirm_or_default};
use crate::Args;

use checkpoint::{save_checkpoint, Checkpoint};
use report::{count_existing_reports, parse_exit_code, parse_prev_target, report_filename, ExitCode, Report};
use roles::{build_context, find_latest_report, RoleContext, RoleExecutor};
use state::{load_state, parse_tasks_from_project_md, OrchestratorState, Role};

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
            milestone_session::run_milestone_session(path, args.dry_run, &logger, effective_model.as_deref())?;
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
                    RoleOutcome::Report(_) => {}
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
                                logger.warn("reviewer", &format!("Auto-commit failed: {}", e));
                            }
                        }
                        if let Err(e) = mark_task_complete(path, &state.current_task_id, &logger) {
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
                            false,
                            args.yes,
                        )?;

                        if create_new {
                            milestone_session::run_milestone_session(path, false, &logger, effective_model.as_deref())?;
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

fn auto_commit(task_id: &str, task_title: &str) -> Result<()> {
    let message = format!("[{}] {}", task_id, task_title);
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

fn mark_task_complete(path: &Path, task_id: &str, logger: &Logger) -> Result<()> {
    let project_md_path = path.join(".porpoise").join("project.md");
    let content = std::fs::read_to_string(&project_md_path)
        .with_context(|| format!("project.md 읽기 실패: {}", project_md_path.display()))?;

    let marker = format!("- [ ] {}:", task_id);
    let replacement = format!("- [x] {}:", task_id);
    let new_content = content.replace(&marker, &replacement);

    write_file(&project_md_path, &new_content, path).context("project.md 업데이트 실패")?;

    update_task_status(path, task_id, true, logger);

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
