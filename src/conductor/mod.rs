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

    // worktree·런타임 데이터가 메인 작업 트리를 오염시키지 않도록 .porpoise/ gitignore 보장 (M21)
    crate::orchestrator::ensure_porpoise_gitignored(path);

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
///
/// worktree는 성공·실패·중단 **모든 경로에서 반드시 정리**된다 (M21: 누수 방지).
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
    let brief = brief::build_brief(path, task_id, task_title, workspace);

    let wt = Worktree::create(path, task_id).context("격리 worktree 생성 실패")?;
    logger.info("conductor", &format!("worktree 생성: {}", wt.path.display()));

    let result = conduct_in_worktree(
        &wt, path, task_id, task_title, brief, workspace, runner,
        dispatch_model, verifier_model, dod, max_redispatch, logger,
    );

    // 성공·실패·중단 무관하게 worktree·브랜치 정리
    wt.remove();
    if let Err(ref e) = result {
        logger.warn("conductor", &format!("task {} 실패 — worktree 정리됨: {}", task_id, e));
    }
    result
}

/// 격리 worktree 안에서 Dispatch→Verify→Integrate 루프를 수행한다.
/// 정리는 호출자(conduct_task)가 담당하므로 여기서는 worktree를 소비하지 않는다.
#[allow(clippy::too_many_arguments)]
fn conduct_in_worktree(
    wt: &Worktree,
    path: &Path,
    task_id: &str,
    task_title: &str,
    mut brief: brief::Brief,
    workspace: &WorkspaceConfig,
    runner: &ClaudeRunner,
    dispatch_model: Option<&str>,
    verifier_model: Option<&str>,
    dod: &[String],
    max_redispatch: u32,
    logger: &Logger,
) -> Result<TaskOutcome> {
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
        let outcome = verify::run_verification(
            &wt.path, task_id, task_title, dod, &diff, &command_results, runner, verifier_model,
        )?;

        write_audit_record(
            path, task_id, redispatch, &diff, &command_results, &outcome, &agent_out, logger,
        );

        // ── Integrate 결정 ────────────────────────────────────────────────
        save_phase(path, task_id, "integrate", redispatch, logger);
        match integrate::decide(&outcome.verdict, redispatch, max_redispatch) {
            IntegrateDecision::Merge => {
                if outcome.verdict.feedback.is_empty() {
                    println!("  {} Verify PASS", "✓".green());
                } else {
                    println!(
                        "  {} Verify PASS — {}",
                        "✓".green(),
                        outcome.verdict.feedback.lines().next().unwrap_or("").dimmed()
                    );
                }
                let commit_msg = format!("[{}] {}", task_id, task_title);
                integrate::finalize(wt, path, &commit_msg)?;
                crate::orchestrator::mark_tasks_complete(path, &[task_id.to_string()], logger)?;
                return Ok(TaskOutcome::Merged);
            }
            IntegrateDecision::Redispatch { feedback } => {
                println!("  {} Verify FAIL — 피드백 재투입", "↻".yellow());
                println!("    {}", feedback.lines().next().unwrap_or("").dimmed());
                brief = brief.with_feedback(&feedback);
                redispatch += 1;
            }
            IntegrateDecision::Halt { feedback } => {
                println!("  {} Verify FAIL — 한도 소진", "✗".red());
                logger.warn("conductor", &format!("task {} 중단", task_id));
                return Ok(TaskOutcome::Halted { feedback });
            }
        }
    }
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
///
/// M21: 검증자 원문·dispatch 출력을 포함하고(사후분석), 파일명에 타임스탬프를 넣어
/// 재투입·재실행 간 덮어쓰기로 이력이 소실되지 않게 한다.
#[allow(clippy::too_many_arguments)]
fn write_audit_record(
    path: &Path,
    task_id: &str,
    redispatch: u32,
    diff: &str,
    command_results: &[crate::session::v0_7::ExecutionResult],
    outcome: &verify::VerifyOutcome,
    dispatch_output: &str,
    logger: &Logger,
) {
    use chrono::Local;
    let sessions_dir = path.join(".porpoise").join("sessions");
    if std::fs::create_dir_all(&sessions_dir).is_err() {
        return;
    }
    let now = Local::now();
    let verdict = &outcome.verdict;
    let record = serde_json::json!({
        "schema_version": "conductor-2",
        "task_id": task_id,
        "redispatch": redispatch,
        "timestamp": now.to_rfc3339(),
        "diff_lines": diff.lines().count(),
        "verify_commands": command_results.iter().map(|r| serde_json::json!({
            "command": r.command,
            "args": r.args,
            "exit_code": r.exit_code,
        })).collect::<Vec<_>>(),
        "verdict": if verdict.pass { "PASS" } else { "FAIL" },
        "feedback": verdict.feedback,
        "verifier_raw": truncate_chars(&outcome.verifier_raw, 4000),
        "dispatch_output": truncate_chars(dispatch_output, 4000),
    });
    // 타임스탬프 + R{n}으로 재투입·재실행 이력 보존
    let filename = format!(
        "{}-conductor-{}-R{}.json",
        task_id,
        now.format("%Y%m%d-%H%M%S"),
        redispatch
    );
    let content = serde_json::to_string_pretty(&record).unwrap_or_default();
    if let Err(e) = crate::utils::fs::write_file(&sessions_dir.join(&filename), &content, path) {
        logger.warn("conductor", &format!("감사 기록 저장 실패: {}", e));
    }
}

/// 문자열을 최대 길이로 자르고 생략 표시를 붙인다 (감사 기록 크기 제한).
fn truncate_chars(s: &str, max: usize) -> String {
    let total = s.chars().count();
    if total <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{}…[{} chars 생략]", head, total - max)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::v0_7::ExecutionResult;
    use verify::{Verdict, VerifyOutcome};

    #[test]
    fn truncate_chars_short_unchanged() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        // 정확히 max면 그대로
        assert_eq!(truncate_chars("hello", 5), "hello");
    }

    #[test]
    fn truncate_chars_long_truncated() {
        let s = "a".repeat(20);
        let out = truncate_chars(&s, 5);
        assert!(out.starts_with("aaaaa"));
        assert!(out.contains("15 chars 생략"), "생략 표시 확인: {}", out);
    }

    #[test]
    fn truncate_chars_counts_unicode_not_bytes() {
        // 한글은 멀티바이트지만 char 단위로 세야 한다 (바이트 슬라이싱 패닉 방지)
        let s = "가나다라마"; // 5 chars
        assert_eq!(truncate_chars(s, 5), s);
        let out = truncate_chars(s, 2);
        assert!(out.starts_with("가나"));
        assert!(out.contains("3 chars 생략"), "생략 표시 확인: {}", out);
    }

    fn cmd_ok() -> ExecutionResult {
        ExecutionResult {
            command: "cargo".to_string(),
            args: vec!["test".to_string()],
            purpose: String::new(),
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            truncated: false,
        }
    }

    #[test]
    fn write_audit_record_includes_raw_and_truncates() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        std::fs::create_dir_all(path.join(".porpoise")).unwrap();
        let logger = crate::logger::Logger::new(path, false).unwrap();

        let long_raw = "x".repeat(5000); // > 4000 → 잘림
        let outcome = VerifyOutcome {
            verdict: Verdict::fail("사유 설명"),
            verifier_raw: long_raw,
        };
        let cmds = vec![cmd_ok()];

        write_audit_record(path, "M1-T01", 0, "diff\nline2", &cmds, &outcome, "dispatch 출력", &logger);

        let sessions = path.join(".porpoise").join("sessions");
        let files: Vec<_> = std::fs::read_dir(&sessions).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "감사 파일 1개 생성");
        let name = files[0].file_name().to_string_lossy().to_string();
        assert!(name.starts_with("M1-T01-conductor-"), "파일명: {}", name);
        assert!(name.ends_with("-R0.json"), "파일명: {}", name);

        let content = std::fs::read_to_string(files[0].path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["schema_version"], "conductor-2");
        assert_eq!(v["verdict"], "FAIL");
        assert_eq!(v["feedback"], "사유 설명");
        assert_eq!(v["diff_lines"], 2);
        assert_eq!(v["dispatch_output"], "dispatch 출력");
        assert_eq!(v["verify_commands"][0]["command"], "cargo");
        assert_eq!(v["verify_commands"][0]["exit_code"], 0);
        // 검증자 원문이 잘림 표시와 함께 포함
        assert!(
            v["verifier_raw"].as_str().unwrap().contains("chars 생략"),
            "원문 잘림 확인"
        );
    }

    #[test]
    fn write_audit_record_filename_unique_per_redispatch() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        std::fs::create_dir_all(path.join(".porpoise")).unwrap();
        let logger = crate::logger::Logger::new(path, false).unwrap();
        let outcome = VerifyOutcome { verdict: Verdict::pass(), verifier_raw: String::new() };

        // 서로 다른 redispatch 인덱스는 파일명이 구분되어 이력이 보존됨
        write_audit_record(path, "M1-T01", 0, "d", &[], &outcome, "", &logger);
        write_audit_record(path, "M1-T01", 1, "d", &[], &outcome, "", &logger);

        let sessions = path.join(".porpoise").join("sessions");
        let names: Vec<String> = std::fs::read_dir(&sessions)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(names.iter().any(|n| n.ends_with("-R0.json")));
        assert!(names.iter().any(|n| n.ends_with("-R1.json")));
    }
}
