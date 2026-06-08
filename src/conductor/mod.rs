//! 지휘자(Conductor) — 에이전트 함대 지휘 루프 (M10+).
//!
//! Porpoise를 AI worker에서 매니저로 전환하는 신규 실행 경로. task 하나를
//! `Brief → Dispatch → Verify → Integrate` 4단계로 처리한다:
//!   - **Brief**: project.md·DoD·규약·마일스톤 목표를 단일 작업 지시서로 조립
//!   - **Dispatch**: 격리 worktree에서 실제 코딩 에이전트에게 통째로 위임
//!   - **Verify**: 독립 검증자가 실제 테스트 실행 + 적대적 심사로 PASS/FAIL 판정
//!   - **Integrate**: PASS면 병합·완료 처리, FAIL이면 피드백 재투입(한도 내) 또는 중단
//!
//! 본 마일스톤(M10)은 **단일 task·순차 실행**만 다룬다. 병렬 함대(M12)와
//! 계획 두뇌(M13)는 후속 마일스톤 범위다.

pub mod brief;
pub mod dispatch;
pub mod git;
pub mod integrate;
pub mod verify;

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;

use crate::claude::runner::ClaudeRunner;
use crate::config::workspace::WorkspaceConfig;
use crate::config::Config;
use crate::logger::Logger;
use crate::orchestrator::checkpoint::{save_checkpoint, Checkpoint};
use crate::orchestrator::state::{parse_tasks_from_project_md, OrchestratorState};
use crate::utils::input::confirm_or_default;
use crate::Args;

use dispatch::Worktree;
use integrate::IntegrateDecision;

/// 지휘자 루프 진입점. orchestrator::run에서 claude_code 어댑터 + conductor 모드일 때 호출된다.
pub fn run_conductor(
    path: &Path,
    args: &Args,
    config: &Config,
    workspace: &WorkspaceConfig,
    _initial_state: &OrchestratorState,
    effective_model: Option<&str>,
    logger: &Logger,
) -> Result<()> {
    println!();
    println!("{}", "=== Conductor 모드 (에이전트 함대 지휘) ===".green().bold());
    println!("{}", "  task를 에이전트에게 위임하고 독립 검증으로 게이트합니다.".dimmed());

    if !git::is_git_repo(path) {
        anyhow::bail!(
            "conductor 모드는 git 저장소가 필요합니다. 'git init' 후 다시 실행하거나 \
             workspace.toml에 [conductor] mode = \"legacy\"를 설정하세요."
        );
    }

    let runner = ClaudeRunner::new().context(
        "Claude CLI를 찾을 수 없습니다. Claude Code를 설치하고 'claude'가 PATH에 있는지 확인하세요.",
    )?;

    let max_redispatch = workspace.conductor_max_redispatch();
    let verifier_model = workspace.conductor_verifier_model().map(str::to_string);
    let dispatch_model = effective_model.filter(|s| !s.is_empty()).map(str::to_string);
    let dod = workspace.dod_items();

    let mut history: Vec<String> = Vec::new();

    loop {
        let tasks = parse_tasks_from_project_md(path);
        let task = match tasks.iter().find(|t| !t.completed).cloned() {
            Some(t) => t,
            None => {
                // 모든 task 완료 → 마일스톤 생성 세션
                if !handle_all_tasks_done(path, args, config, workspace, effective_model, logger)? {
                    break;
                }
                continue;
            }
        };

        println!(
            "{}",
            format!("\n[ {} ] ─── {} ─── [conductor]", task.id, task.title).bold()
        );

        if args.dry_run {
            println!("{}", "  [dry-run] Brief→Dispatch→Verify→Integrate 실행 계획만 출력".dimmed());
            let b = brief::build_brief(path, &task.id, &task.title, workspace);
            println!("{}", "  --- Brief 미리보기 ---".dimmed());
            for line in b.render().lines().take(12) {
                println!("  {}", line.dimmed());
            }
            println!("{}", "  ...".dimmed());
            break;
        }

        if !confirm_or_default(&format!("'{}' 작업을 지휘하시겠습니까?", task.id), true, args.yes)? {
            println!("{}", "Skipped.".yellow());
            break;
        }

        let outcome = conduct_task(
            path,
            &task.id,
            &task.title,
            workspace,
            &runner,
            dispatch_model.as_deref(),
            verifier_model.as_deref(),
            &dod,
            max_redispatch,
            logger,
        )?;

        history.push(format!("[{}] {} → {}", task.id, task.title, outcome.label()));

        match outcome {
            TaskOutcome::Merged => {
                println!("  {} task 완료 및 병합", "✓".green());
            }
            TaskOutcome::Halted { feedback } => {
                println!("{}", "  ⚠ 검증 실패 — 재투입 한도 소진. 사용자 개입이 필요합니다.".yellow().bold());
                save_halt_hint(path, &task.id, &feedback, logger);
                break;
            }
        }
    }

    print_conductor_history(&history);
    println!();
    println!("{}", "지휘자 세션 종료.".dimmed());
    Ok(())
}

/// 한 task의 전체 지휘 사이클 (Brief→Dispatch→Verify→Integrate, 재투입 포함).
#[allow(clippy::too_many_arguments)]
fn conduct_task(
    path: &Path,
    task_id: &str,
    task_title: &str,
    workspace: &WorkspaceConfig,
    runner: &ClaudeRunner,
    dispatch_model: Option<&str>,
    verifier_model: Option<&str>,
    dod: &[String],
    max_redispatch: u32,
    logger: &Logger,
) -> Result<TaskOutcome> {
    save_phase(path, task_id, "brief", 0, logger);
    let mut brief = brief::build_brief(path, task_id, task_title, workspace);

    let wt = Worktree::create(path, task_id).context("격리 worktree 생성 실패")?;
    logger.info("conductor", &format!("worktree 생성: {}", wt.path.display()));

    let verify_cmds = workspace.default_verify_commands();
    let allowed = workspace.allowed_command_prefixes();
    let timeout = workspace.verify_timeout_secs();

    let mut redispatch = 0u32;

    loop {
        // ── Dispatch ──────────────────────────────────────────────────────
        save_phase(path, task_id, "dispatch", redispatch, logger);
        println!(
            "  {} Dispatch{} — 에이전트에게 위임 중...",
            "→".cyan(),
            if redispatch > 0 { format!(" (재투입 {})", redispatch) } else { String::new() }
        );
        let agent_out = wt.run_agent(runner, &brief, dispatch_model)?;
        logger.info("conductor", &format!("dispatch 출력 {} bytes", agent_out.len()));

        let diff = wt.capture_diff();

        // 객관 증거: 검증 명령 실제 실행 (worktree 안에서)
        let command_results = if verify_cmds.is_empty() {
            vec![]
        } else {
            println!("  {} 검증 명령 실행 중 ({}개)...", "→".cyan(), verify_cmds.len());
            crate::workspace::executor::run_verify_commands(&wt.path, &verify_cmds, &allowed, timeout)
        };

        // ── Verify ────────────────────────────────────────────────────────
        save_phase(path, task_id, "verify", redispatch, logger);
        println!("  {} Verify — 독립 검증자 심사 중...", "→".cyan());
        let verdict = verify::run_verification(
            &wt.path, task_id, task_title, dod, &diff, &command_results, runner, verifier_model,
        )?;

        write_audit_record(path, task_id, redispatch, &diff, &command_results, &verdict, logger);

        // ── Integrate 결정 ────────────────────────────────────────────────
        save_phase(path, task_id, "integrate", redispatch, logger);
        match integrate::decide(&verdict, redispatch, max_redispatch) {
            IntegrateDecision::Merge => {
                println!("  {} Verify PASS", "✓".green());
                break;
            }
            IntegrateDecision::Redispatch { feedback } => {
                println!("  {} Verify FAIL — 피드백 재투입", "↻".yellow());
                println!("    {}", feedback.lines().next().unwrap_or("").dimmed());
                brief = brief.with_feedback(&feedback);
                redispatch += 1;
            }
            IntegrateDecision::Halt { feedback } => {
                println!("  {} Verify FAIL — 한도 소진", "✗".red());
                let branch = wt.branch.clone();
                wt.remove();
                logger.warn("conductor", &format!("task {} 중단: {}", task_id, branch));
                return Ok(TaskOutcome::Halted { feedback });
            }
        }
    }

    // ── Integrate 실행 (PASS) ─────────────────────────────────────────────
    // 커밋 → 병합 → 정리 순서를 finalize가 보장한다 (정리가 브랜치를 삭제하므로 병합이 먼저).
    let commit_msg = format!("[{}] {}", task_id, task_title);
    integrate::finalize(wt, path, &commit_msg)?;
    crate::orchestrator::mark_tasks_complete(path, &[task_id.to_string()], logger)?;

    Ok(TaskOutcome::Merged)
}

/// 한 task 지휘 결과.
enum TaskOutcome {
    Merged,
    Halted { feedback: String },
}

impl TaskOutcome {
    fn label(&self) -> &'static str {
        match self {
            TaskOutcome::Merged => "MERGED",
            TaskOutcome::Halted { .. } => "HALTED",
        }
    }
}

/// 모든 task 완료 시 마일스톤 생성 세션을 실행한다.
/// 새 task가 생기면 true(루프 계속), 아니면 false(종료)를 반환한다.
fn handle_all_tasks_done(
    path: &Path,
    args: &Args,
    config: &Config,
    workspace: &WorkspaceConfig,
    effective_model: Option<&str>,
    logger: &Logger,
) -> Result<bool> {
    println!("{}", "\n모든 작업 항목 완료!".green().bold());
    let create_new = confirm_or_default("새 마일스톤을 생성하시겠습니까?", args.yes, args.yes)?;
    if !create_new {
        let _ = crate::orchestrator::run_release_flow(config.github_repo());
        return Ok(false);
    }
    crate::orchestrator::milestone_session::run_milestone_session(
        path, false, logger, effective_model, workspace,
    )?;
    let new_tasks = parse_tasks_from_project_md(path);
    match new_tasks.iter().find(|t| !t.completed) {
        Some(next) => {
            println!("  {} 다음 작업: {} — {}", "→".cyan(), next.id.cyan(), next.title);
            Ok(true)
        }
        None => {
            println!("{}", "새 마일스톤이 생성되지 않았습니다.".yellow());
            Ok(false)
        }
    }
}

fn save_phase(path: &Path, task_id: &str, phase: &str, retry: u32, logger: &Logger) {
    let cp = Checkpoint::new(1, "conductor", vec![], "conductor", vec![], task_id, retry, vec![])
        .with_conductor_phase(phase);
    if let Err(e) = save_checkpoint(&cp, path) {
        logger.warn("conductor", &format!("checkpoint 저장 실패: {}", e));
    }
}

fn save_halt_hint(path: &Path, task_id: &str, feedback: &str, logger: &Logger) {
    let hints_dir = path.join(".porpoise").join("hints");
    if let Err(e) = std::fs::create_dir_all(&hints_dir) {
        logger.warn("conductor", &format!("hints 디렉토리 생성 실패: {}", e));
        return;
    }
    let filename = format!("{}-conductor-halt.md", task_id);
    let body = format!(
        "# {} 지휘 중단 — 검증 실패\n\n재투입 한도를 소진했습니다. 아래 검증 피드백을 참고해 \
         수동으로 작업하거나 workspace.toml의 max_redispatch를 늘리세요.\n\n## 검증자 피드백\n\n{}\n",
        task_id, feedback
    );
    let hint_path = hints_dir.join(&filename);
    if let Err(e) = crate::utils::fs::write_file(&hint_path, &body, path) {
        logger.warn("conductor", &format!("halt hint 저장 실패: {}", e));
    } else {
        println!("  {} 검증 피드백 저장: {}", "ℹ".cyan(), filename.dimmed());
    }
}

/// 감사 추적용 지휘 기록을 sessions/에 저장한다.
fn write_audit_record(
    path: &Path,
    task_id: &str,
    redispatch: u32,
    diff: &str,
    command_results: &[crate::session::v0_7::ExecutionResult],
    verdict: &verify::Verdict,
    logger: &Logger,
) {
    use chrono::Local;
    let sessions_dir = path.join(".porpoise").join("sessions");
    if std::fs::create_dir_all(&sessions_dir).is_err() {
        return;
    }
    let record = serde_json::json!({
        "schema_version": "conductor-1",
        "task_id": task_id,
        "redispatch": redispatch,
        "timestamp": Local::now().to_rfc3339(),
        "diff_lines": diff.lines().count(),
        "verify_commands": command_results.iter().map(|r| serde_json::json!({
            "command": r.command,
            "args": r.args,
            "exit_code": r.exit_code,
        })).collect::<Vec<_>>(),
        "verdict": if verdict.pass { "PASS" } else { "FAIL" },
        "feedback": verdict.feedback,
    });
    let filename = format!("{}-conductor-R{}.json", task_id, redispatch);
    let content = serde_json::to_string_pretty(&record).unwrap_or_default();
    if let Err(e) = crate::utils::fs::write_file(&sessions_dir.join(&filename), &content, path) {
        logger.warn("conductor", &format!("감사 기록 저장 실패: {}", e));
    }
}

fn print_conductor_history(history: &[String]) {
    if history.is_empty() {
        return;
    }
    println!();
    println!("{}", "─── Conductor History ───".dimmed());
    for entry in history {
        println!("  {}", entry.dimmed());
    }
}
