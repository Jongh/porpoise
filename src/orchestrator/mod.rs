pub mod checkpoint;
pub mod milestone_session;
pub mod new_format;
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
use crate::utils::input::collect_multiline_input;
use crate::Args;

use checkpoint::{save_checkpoint, Checkpoint};
use state::{load_state, parse_tasks_from_project_md, OrchestratorState, Role};

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
                        "⚠  workspace.toml이 프롬프트 파일보다 최신입니다. 'porpoise update prompt'로 프롬프트를 재생성하세요.".yellow()
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

    // Session cleanup based on workspace.toml [sessions] policy
    crate::session::cleanup_sessions(path, &workspace);

    // Milestone session if all tasks done
    {
        let tasks = parse_tasks_from_project_md(path);
        if tasks.iter().all(|t| t.completed) {
            logger.info("orchestrator", "No pending tasks — entering milestone session");
            println!("{}", "\n미완료 작업이 없습니다. 마일스톤 생성 세션을 시작합니다.".cyan().bold());
            milestone_session::run_milestone_session(
                path,
                args.dry_run,
                &logger,
                effective_model.as_deref(),
                &workspace,
            )?;
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

    if crate::session::is_new_format(path) {
        // 지휘자(conductor) 경로: claude_code 어댑터 + conductor 모드일 때만.
        // API 어댑터(anthropic_api/openai_compatible)는 에이전틱 위임이 불가하므로 항상 legacy.
        let use_conductor = workspace.conductor_enabled()
            && workspace.model_adapter_type() == crate::model::adapter::AdapterType::ClaudeCode;
        if use_conductor {
            return crate::conductor::run_conductor(
                path,
                args,
                config,
                &workspace,
                &state,
                effective_model.as_deref(),
                &logger,
            );
        }
        return new_format::run_new_format(
            path,
            args,
            config,
            &workspace,
            &state,
            effective_model.as_deref(),
            &logger,
        );
    }

    // Legacy project detected — guide user to run migrate
    let legacy_exists = path.join(".porpoise").join("messages").exists()
        || path.join(".porpoise").join("reports").exists();
    if legacy_exists {
        println!("{}", "\n⚠  레거시 프로젝트가 감지되었습니다.".yellow().bold());
        println!("{}", "   'porpoise migrate'를 실행하여 신규 JSON 세션 포맷으로 전환하세요.".dimmed());
    } else {
        println!(
            "{}",
            "sessions/ 폴더가 없습니다. 'porpoise --new'로 재초기화하세요.".yellow()
        );
    }
    Ok(())
}

pub(super) fn save_current_checkpoint(
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

pub(super) fn save_resp_hints(
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

pub(super) fn make_spinner(msg: &str) -> ProgressBar {
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

pub(super) fn print_history(history: &[String]) {
    if history.is_empty() {
        return;
    }
    println!();
    println!("{}", "─── Session History ───".dimmed());
    for entry in history {
        println!("  {}", entry.dimmed());
    }
}

pub(super) fn print_resume_summary(state: &OrchestratorState) {
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

pub(super) fn auto_commit(tasks: &[(String, String)]) -> Result<()> {
    // .gitignore에 .porpoise/ 항목 보장
    if let Ok(cwd) = std::env::current_dir() {
        ensure_porpoise_gitignored(&cwd);
    }

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
    let target_paths = ["Cargo.toml", "Cargo.lock", "src/", "README.md", "wix/"];

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

pub(super) fn mark_tasks_complete(path: &Path, task_ids: &[String], logger: &Logger) -> Result<()> {
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

pub(super) fn all_tasks_done(path: &Path) -> bool {
    let tasks = parse_tasks_from_project_md(path);
    !tasks.is_empty() && tasks.iter().all(|t| t.completed)
}

pub(super) fn run_release_flow(github_repo: Option<&str>) -> Result<()> {
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

fn ensure_porpoise_gitignored(project_root: &std::path::Path) {
    let gitignore_path = project_root.join(".gitignore");
    let content = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
    let already_present = content.lines().any(|l| {
        let l = l.trim();
        l == ".porpoise" || l == ".porpoise/"
    });
    if already_present {
        return;
    }
    let entry = if content.ends_with('\n') || content.is_empty() {
        "\n# Porpoise runtime data\n.porpoise/\n"
    } else {
        "\n\n# Porpoise runtime data\n.porpoise/\n"
    };
    if let Err(e) = std::fs::write(&gitignore_path, format!("{}{}", content, entry)) {
        eprintln!("  ⚠  .gitignore 업데이트 실패: {}", e);
    } else {
        println!("  {} .gitignore에 .porpoise/ 추가됨 (세션 데이터 커밋 방지)", "ℹ".cyan());
    }
}
