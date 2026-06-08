use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use crate::config::workspace::WorkspaceConfig;
use crate::config::Config;
use crate::logger::Logger;
use crate::utils::input::confirm_or_default;
use crate::Args;

use super::milestone_session;
use super::roles::find_latest_report;
use super::state::{parse_tasks_from_project_md, OrchestratorState, Role};

pub(super) fn run_new_format(
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
            // 현재 사이클의 세션 없음 → 단계 실행
            if args.dry_run {
                println!("{}", "  [dry-run] 단계 실행 후 NEXT로 처리".dimmed());
                state.completed_roles.push(current_role.clone());
                state.current_role = current_role.next();
                continue;
            }

            if !confirm_or_default(&format!("Execute {}?", current_role.display_name()), true, args.yes)? {
                println!("{}", "Skipped.".yellow());
                break;
            }

            super::save_current_checkpoint(&state, &current_role, path, retry)?;
            logger.role_start(&current_role.to_string(), state.cycle);

            let spinner = super::make_spinner(&format!(
                "[ Cycle {} | {} ] Running {} ...",
                state.cycle, state.current_task_id, current_role.display_name()
            ));

            // SessionInput 구성
            let mut input = build_session_input(&state, path, workspace)?;

            // 이전 Development 단계의 실행 결과 주입
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
                state.cycle, retry, false, args.verbose, effective_model, logger,
            );
            spinner.finish_and_clear();

            match result {
                Ok(mut o) => {
                    logger.role_end(&current_role.to_string(), state.cycle, true);

                    // 파일 미디에이션 후처리: Development 단계 완료 시 파일 적용 + Verify
                    if adapter.requires_file_mediation() && current_role == Role::Developer {
                        // file_operations 검증: changes 있는데 file_operations 없으면 PREV 강제
                        let has_changes = if let crate::session::RoleOutputData::Development(ref dev_o) = o {
                            !dev_o.changes.is_empty()
                        } else { false };
                        let has_ops = o.file_operations().is_some_and(|ops| !ops.is_empty());
                        if has_changes && !has_ops {
                            logger.warn("orchestrator",
                                "API 모드 Development: file_operations 없음 — 실제 파일 내용을 file_operations에 포함해야 합니다.");
                            if let crate::session::RoleOutputData::Development(ref mut dev_o) = o {
                                dev_o.status = session::ExitCode::Prev;
                                dev_o.prev_reason = Some(
                                    "file_operations 배열이 비어 있거나 누락되었습니다. \
                                     API 모드에서는 모든 파일 내용을 file_operations[].content에 포함해야 합니다. \
                                     changes[]는 설명 요약이며 실제 파일을 생성하지 않습니다.".to_string()
                                );
                            }
                        } else {
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

        // 단계 리포트 파일 출력 (fresh 실행·캐시 재사용 공통)
        {
            let reports_dir = path.join(".porpoise").join("reports");
            if let Some(report_path) = find_latest_report(&reports_dir, &current_role.to_string(), &state.current_task_id) {
                if let Ok(content) = std::fs::read_to_string(&report_path) {
                    if !content.is_empty() {
                        println!("{}", format!("\n--- {} 보고서 ---", current_role.display_name()).bold());
                        for line in content.lines() {
                            println!("{}", line);
                        }
                    }
                }
            }
        }

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

                        match super::auto_commit(&commit_tasks) {
                            Ok(()) => println!("  {} 커밋 완료", "✓".green()),
                            Err(e) => println!("{} {}", "⚠ 커밋 실패:".yellow(), e),
                        }
                        let commit_ids: Vec<String> = commit_tasks.iter().map(|(id, _)| id.clone()).collect();
                        let _ = super::mark_tasks_complete(path, &commit_ids, logger);

                        if output_data.milestone_complete() && !super::all_tasks_done(path) {
                            println!("{}", "⚠  Review 단계에서 milestone_complete=true를 반환했지만 project.md에 미완료 작업이 있습니다. project.md를 확인하세요.".yellow());
                            logger.warn("reviewer", "milestone_complete=true 반환, all_tasks_done=false — project.md 불일치");
                        }
                    }

                    if !args.dry_run && super::all_tasks_done(path) {
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
                                state.cycle = 1;
                                logger.info("orchestrator", &format!("New milestone task: {}", state.current_task_id));
                                continue;
                            }
                            println!("{}", "새 마일스톤이 생성되지 않았습니다.".yellow());
                        } else {
                            let _ = super::run_release_flow(config.github_repo());
                        }
                        break;
                    }

                    let tasks = parse_tasks_from_project_md(path);
                    if let Some(next_task) = tasks.iter().find(|t| !t.completed) {
                        state.current_task_id = next_task.id.clone();
                        state.current_task_title = next_task.title.clone();
                        state.completed_roles = vec![];
                        state.current_role = Some(Role::PM);
                        state.cycle = 1;
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

                let target_role = output_data.prev_target().and_then(Role::from_str);
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
                    super::save_resp_hints(&current_role, &state, retry, path, logger)?;
                }
                break;
            }

            session::ExitCode::Limit => {
                println!("{}", "\n⚠  Claude Code 토큰 한도에 도달했습니다.".yellow().bold());
                println!("{}", "   한도가 초기화된 후 'porpoise'를 다시 실행하세요.".dimmed());
                logger.warn("orchestrator", "Token limit reached — session terminated");
                break;
            }
        }
    }

    super::print_history(&history);
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

    // 이전 단계 세션 로드
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
    verbose: bool,
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
    config.verbose = verbose;
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
    use crate::logger::Logger;

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
