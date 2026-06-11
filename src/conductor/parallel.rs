//! 병렬 함대 — 독립 task N개를 동시에 dispatch·verify하고 순차·충돌 인지로 통합 (M23).
//!
//! 낙관적 동시성: 일단 병렬로 실행하고, 통합 시 충돌이 나면 해당 task를 갱신된 base에서
//! 재투입하여 사실상 직렬화한다. `max_parallel = 1`이면 이 경로를 쓰지 않는다(순차).

use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::claude::runner::ClaudeRunner;
use crate::config::workspace::WorkspaceConfig;
use crate::config::Config;
use crate::logger::Logger;
use crate::orchestrator::state::{parse_tasks_from_project_md, Task};
use crate::session::v0_7::ExecutionResult;
use crate::utils::input::confirm_or_default;
use crate::Args;

use super::brief;
use super::dispatch::Worktree;
use super::integrate::{self, MergeOutcome};
use super::schedule;
use super::verify::{self, VerifyOutcome};

/// 한 task의 병렬 실행 산출물 (통합 전).
struct TaskRun {
    task_id: String,
    task_title: String,
    outcome: VerifyOutcome,
    diff: String,
    command_results: Vec<ExecutionResult>,
    agent_run: crate::claude::runner::AgentRun,
}

/// 다음 배치로 처리할 task 목록을 고른다 (pending 앞에서 최대 max개).
fn select_batch(pending: &[Task], max: usize) -> Vec<Task> {
    pending.iter().take(max).cloned().collect()
}

/// 병렬 함대 루프. orchestrator → run_conductor에서 max_parallel>1일 때 호출된다.
#[allow(clippy::too_many_arguments)]
pub fn run_parallel(
    path: &Path,
    args: &Args,
    config: &Config,
    workspace: &WorkspaceConfig,
    runner: &ClaudeRunner,
    effective_model: Option<&str>,
    dod: &[String],
    max_parallel: u32,
    max_redispatch: u32,
    logger: &Logger,
) -> Result<()> {
    println!(
        "{}",
        format!("  병렬 함대 모드 (max_parallel={})", max_parallel).cyan()
    );
    println!(
        "{}",
        "  ⚠ 병렬은 독립 task 전제입니다. 의존 task는 충돌 시 재투입으로 직렬화됩니다. (토큰 동시 소모 주의)".dimmed()
    );
    // 의존성 그래프 검증은 run_conductor에서 이미 수행됨 (순환 거부, dangling 경고).

    let dispatch_model = effective_model.filter(|s| !s.is_empty());
    let verifier_model = workspace.conductor_verifier_model();
    let fallback_halt = workspace.conductor_verdict_fallback_halt();
    let verify_cmds = workspace.default_verify_commands();
    let allowed = workspace.allowed_command_prefixes();
    let timeout = workspace.verify_timeout_secs();

    // task별 시도 횟수 (충돌·실패 누적) — 무한 루프 방지용 캡
    let mut attempts: HashMap<String, u32> = HashMap::new();
    // M37: 대시보드 재투입 오버라이드로 상향된 task별 추가 재투입 예산 (누적)
    let mut redispatch_bonus: HashMap<String, u32> = HashMap::new();
    // 재투입 시 brief에 주입할 피드백 (충돌·FAIL 사유). 다음 라운드 에이전트가 맥락을 알도록 한다.
    let mut feedbacks: HashMap<String, String> = HashMap::new();
    let mut history: Vec<String> = Vec::new();

    // M28: 예산 거버넌스 — 누적 비용이 상한 도달하면 다음 배치 전 중단.
    let budget = workspace.conductor_budget_usd();
    let mut total_cost = 0.0f64;

    // M31: 라이브 상태 시작 (병렬 모드)
    super::live::start(path, "parallel", budget);

    loop {
        let tasks = parse_tasks_from_project_md(path);
        let completed_ids: HashSet<String> =
            tasks.iter().filter(|t| t.completed).map(|t| t.id.clone()).collect();
        let pending: Vec<Task> = tasks.iter().filter(|t| !t.completed).cloned().collect();
        if pending.is_empty() {
            if !super::handle_all_tasks_done(path, args, config, workspace, effective_model, logger)? {
                break;
            }
            continue;
        }

        // M24: 의존성이 모두 충족된 ready task만 배치 대상
        let ready = schedule::ready_tasks(&pending, &completed_ids);
        // M37: 대시보드 재투입 오버라이드 소비 — ready task의 재투입 한도를 상향
        for t in &ready {
            if let Some(extra) = super::redispatch::consume_override(path, &t.id, logger) {
                let b = redispatch_bonus.entry(t.id.clone()).or_insert(0);
                *b = b.saturating_add(extra);
                println!("  {} [{}] 재투입 요청 수신 — 재투입 한도 +{}", "↻".cyan(), t.id, extra);
            }
        }
        if ready.is_empty() {
            println!(
                "{}",
                "\n⚠ 실행 가능한(의존성 충족) task가 없습니다. 의존성 그래프(deps:)를 확인하세요. 중단합니다.".yellow().bold()
            );
            break;
        }
        // M28: 다음 배치 전 예산 가드 (진행 중 배치는 마치고 정지)
        if super::budget_exceeded(total_cost, budget) {
            println!(
                "{}",
                format!(
                    "\n⛔ 예산 상한 도달 — 누적 ${:.4} / 한도 ${:.4}. 다음 배치를 시작하지 않고 중단합니다.",
                    total_cost,
                    budget.unwrap_or(0.0)
                )
                .yellow()
                .bold()
            );
            break;
        }

        let batch = select_batch(&ready, max_parallel as usize);
        let ids = batch.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ");
        println!(
            "{}",
            format!("\n[ 배치 {}개 ] ─── {} ─── [conductor 병렬]", batch.len(), ids).bold()
        );

        if args.dry_run {
            println!("{}", "  [dry-run] 병렬 배치 실행 계획만 출력".dimmed());
            break;
        }
        // M33: 게이트 모드면 대시보드 승인 대기 (--yes는 양쪽 모두 자동 승인)
        let prompt = format!("{}개 task를 병렬 지휘하시겠습니까?", batch.len());
        let approved = if workspace.conductor_gate_mode() && !args.yes {
            let label = format!("batch-{}", batch.first().map(|t| t.id.as_str()).unwrap_or("x"));
            super::gate::gate_decision(path, &label, &prompt) == super::gate::Decision::Approve
        } else {
            confirm_or_default(&prompt, true, args.yes)?
        };
        if !approved {
            println!("{}", "Skipped.".yellow());
            break;
        }

        // ── Phase 1 (직렬): worktree 생성 ────────────────────────────────────
        let mut worktrees: Vec<Worktree> = Vec::with_capacity(batch.len());
        for t in &batch {
            worktrees.push(Worktree::create(path, &t.id).context("worktree 생성 실패")?);
        }

        // ── Phase 2 (병렬): dispatch + verify ───────────────────────────────
        // M31: 배치 전체를 dispatch 단계로 기록 (스레드 경쟁 회피 — 배치 수준 기록, M36: 제목 포함)
        let batch_tasks: Vec<(String, String)> =
            batch.iter().map(|t| (t.id.clone(), t.title.clone())).collect();
        super::live::set_batch(path, &batch_tasks, "dispatch");
        println!("  {} {}개 task 동시 dispatch·verify 중... (출력은 완료 후 그룹 표시)", "→".cyan(), batch.len());
        let runs = dispatch_batch_parallel(
            &worktrees, &batch, path, workspace, runner, dispatch_model, verifier_model,
            dod, fallback_halt, &verify_cmds, &allowed, timeout, &feedbacks,
        );

        // ── Phase 3 (직렬): 그룹 출력 + 충돌 인지 통합 ──────────────────────
        let mut any_progress = false;
        for (wt, run_result) in worktrees.into_iter().zip(runs) {
            match run_result {
                Ok(run) => {
                    let attempt = *attempts.get(&run.task_id).unwrap_or(&0);
                    print_task_result(&run);
                    super::write_audit_record(
                        path, &run.task_id, attempt, &run.diff, &run.command_results,
                        &run.outcome, &run.agent_run.output, &run.agent_run, logger,
                    );
                    total_cost += run.agent_run.cost_usd.unwrap_or(0.0);
                    super::live::set_total_cost(path, total_cost);

                    if run.outcome.verdict.pass {
                        // 커밋 → 병합 → 정리 순서를 integrate_parallel이 보장 (정리가 브랜치를 삭제하므로 병합이 먼저)
                        let commit_msg = format!("[{}] {}", run.task_id, run.task_title);
                        match integrate::integrate_parallel(wt, path, &commit_msg) {
                            Ok(MergeOutcome::Merged) => {
                                if let Err(e) = crate::orchestrator::mark_tasks_complete(
                                    path, std::slice::from_ref(&run.task_id), logger,
                                ) {
                                    logger.warn("conductor", &format!("task 완료 표시 실패: {}", e));
                                }
                                println!("  {} [{}] 병합 완료", "✓".green(), run.task_id);
                                super::live::set_task(path, &run.task_id, "", "merged", attempt);
                                history.push(format!("[{}] MERGED", run.task_id));
                                attempts.remove(&run.task_id);
                                feedbacks.remove(&run.task_id);
                                any_progress = true;
                            }
                            Ok(MergeOutcome::Conflicted) => {
                                let c = attempts.entry(run.task_id.clone()).or_insert(0);
                                *c += 1;
                                feedbacks.insert(
                                    run.task_id.clone(),
                                    "이전 시도가 다른 task와 병합 충돌했습니다. 그 사이 다른 task의 변경이 \
                                     반영되었으니, 갱신된 현재 코드를 먼저 읽고 그 위에 당신의 작업을 다시 적용하세요. \
                                     (기존 변경을 덮어쓰지 말 것)".to_string(),
                                );
                                println!(
                                    "  {} [{}] 병합 충돌 — 갱신된 base에서 재투입 예정 ({}회)",
                                    "↻".yellow(), run.task_id, c
                                );
                                history.push(format!("[{}] CONFLICT", run.task_id));
                            }
                            Err(e) => {
                                let c = attempts.entry(run.task_id.clone()).or_insert(0);
                                *c += 1;
                                println!("  {} [{}] 통합 오류 ({}회): {}", "✗".red(), run.task_id, c, e);
                                history.push(format!("[{}] INTEGRATE-ERR", run.task_id));
                            }
                        }
                    } else {
                        let c = attempts.entry(run.task_id.clone()).or_insert(0);
                        *c += 1;
                        if !run.outcome.verdict.feedback.is_empty() {
                            feedbacks.insert(run.task_id.clone(), run.outcome.verdict.feedback.clone());
                        }
                        println!(
                            "  {} [{}] Verify FAIL — 재투입 예정 ({}회): {}",
                            "✗".red(), run.task_id, c,
                            run.outcome.verdict.feedback.lines().next().unwrap_or("").dimmed()
                        );
                        history.push(format!("[{}] FAIL", run.task_id));
                        wt.remove();
                    }
                }
                Err(e) => {
                    println!("  {} 실행 오류: {}", "✗".red(), e);
                    wt.remove();
                }
            }
        }

        save_batch_checkpoint(path, &batch, logger);

        // 무한 루프 방지: 시도 한도 초과 task가 있으면 중단 (M37: 재투입 오버라이드로 상향된 한도 반영)
        let stuck: Vec<String> = attempts
            .iter()
            .filter(|(id, c)| {
                let cap = super::redispatch::effective_max_redispatch(
                    max_redispatch,
                    *redispatch_bonus.get(*id).unwrap_or(&0),
                );
                **c > cap
            })
            .map(|(id, _)| id.clone())
            .collect();
        if !stuck.is_empty() {
            println!(
                "{}",
                format!("\n⚠ 다음 task가 시도 한도({})를 초과했습니다: {} — 사용자 개입이 필요합니다.",
                    max_redispatch, stuck.join(", ")).yellow().bold()
            );
            break;
        }
        if !any_progress {
            println!("{}", "\n⚠ 이번 배치에서 진전이 없습니다. 중단합니다.".yellow().bold());
            break;
        }
    }

    super::live::finish(path); // M31: 라이브 상태 종료
    super::print_conductor_history(&history);
    println!();
    println!("{}", "병렬 지휘자 세션 종료.".dimmed());
    Ok(())
}

/// 배치를 스레드로 동시 실행한다. 각 task는 자신의 worktree에서 dispatch·verify하며,
/// 통합(병합)은 호출자가 직렬로 수행한다. 결과는 입력 순서대로 반환된다.
#[allow(clippy::too_many_arguments)]
fn dispatch_batch_parallel(
    worktrees: &[Worktree],
    batch: &[Task],
    path: &Path,
    workspace: &WorkspaceConfig,
    runner: &ClaudeRunner,
    dispatch_model: Option<&str>,
    verifier_model: Option<&str>,
    dod: &[String],
    fallback_halt: bool,
    verify_cmds: &[crate::session::v0_7::VerifyCommand],
    allowed: &[String],
    timeout: u32,
    feedbacks: &HashMap<String, String>,
) -> Vec<Result<TaskRun>> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = worktrees
            .iter()
            .zip(batch.iter())
            .map(|(wt, task)| {
                scope.spawn(move || -> Result<TaskRun> {
                    let mut brief = brief::build_brief(path, &task.id, &task.title, workspace);
                    // 재투입이면 직전 충돌/실패 피드백을 brief에 주입 (M23 수렴 강화)
                    if let Some(fb) = feedbacks.get(&task.id) {
                        brief = brief.with_feedback(fb);
                    }
                    let agent_run = wt.run_agent(runner, &brief, dispatch_model, false)?;
                    let diff = wt.capture_diff();
                    let command_results = if verify_cmds.is_empty() {
                        vec![]
                    } else {
                        crate::workspace::executor::run_verify_commands(&wt.path, verify_cmds, allowed, timeout)
                    };
                    let outcome = verify::run_verification(
                        &wt.path, &task.id, &task.title, dod, &diff, &command_results,
                        runner, verifier_model, fallback_halt, false,
                    )?;
                    Ok(TaskRun {
                        task_id: task.id.clone(),
                        task_title: task.title.clone(),
                        outcome,
                        diff,
                        command_results,
                        agent_run,
                    })
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("dispatch 스레드 패닉"))))
            .collect()
    })
}

/// 한 task의 결과를 그룹으로 출력한다 (병렬 인터리브 방지).
fn print_task_result(run: &TaskRun) {
    let verdict = if run.outcome.verdict.pass {
        if run.outcome.fallback_used { "PASS(폴백)".yellow() } else { "PASS".green() }
    } else {
        "FAIL".red()
    };
    let cmd = run
        .command_results
        .iter()
        .map(|r| format!("{}=exit{}", r.command, r.exit_code))
        .collect::<Vec<_>>()
        .join(" ");
    println!(
        "  {} [{}] {} — diff {}줄, 검증명령 [{}]",
        "▸".cyan(),
        run.task_id,
        verdict,
        run.diff.lines().count(),
        cmd
    );
}

fn save_batch_checkpoint(path: &Path, batch: &[Task], logger: &Logger) {
    use crate::orchestrator::checkpoint::{save_checkpoint, Checkpoint};
    let first = batch.first().map(|t| t.id.as_str()).unwrap_or("");
    let cp = Checkpoint::new(1, "conductor-parallel", vec![], "conductor-parallel", vec![], first, 0, vec![])
        .with_conductor_phase("batch");
    if let Err(e) = save_checkpoint(&cp, path) {
        logger.warn("conductor", &format!("배치 checkpoint 저장 실패: {}", e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> Task {
        Task { id: id.to_string(), title: format!("{} 작업", id), completed: false, ..Default::default() }
    }

    #[test]
    fn select_batch_takes_up_to_max() {
        let pending = vec![task("M1-T01"), task("M1-T02"), task("M1-T03")];
        assert_eq!(select_batch(&pending, 2).len(), 2);
        assert_eq!(select_batch(&pending, 5).len(), 3); // pending보다 크면 전부
        assert_eq!(select_batch(&pending, 1).len(), 1);
        assert_eq!(select_batch(&pending, 2)[0].id, "M1-T01");
    }
}
