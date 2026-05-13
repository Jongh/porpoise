use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashSet;
use std::path::Path;

use crate::claude::runner::ClaudeRunner;
use crate::config::workspace::WorkspaceConfig;
use crate::init::template::apply_template;
use crate::logger::Logger;
use crate::milestone::{load_all_milestones, Milestone};
use crate::model::adapter::AdapterType;
use crate::orchestrator::report::{parse_report, ExitCode};
use crate::utils::fs::write_file;
use crate::utils::input::collect_multiline_input;

const MAX_MILESTONE_RETRY: u32 = 5;

/// 마일스톤 생성 세션을 실행합니다.
pub fn run_milestone_session(
    path: &Path,
    dry_run: bool,
    logger: &Logger,
    model: Option<&str>,
    workspace: &WorkspaceConfig,
) -> Result<()> {
    println!("{}", "\n=== 마일스톤 생성 세션 ===".cyan().bold());

    if dry_run {
        println!("{}", "  [dry-run] 마일스톤 생성 세션 스킵".dimmed());
        return Ok(());
    }

    let milestones_dir = path.join(".porpoise").join("milestones");
    let max_id = load_all_milestones(&milestones_dir)
        .unwrap_or_default()
        .iter()
        .map(|m| m.id)
        .max()
        .unwrap_or(0);
    let next_id = max_id + 1;

    match workspace.model_adapter_type() {
        AdapterType::ClaudeCode => run_milestone_via_claude_runner(path, logger, model, next_id),
        _ => run_milestone_via_api(path, logger, model, next_id, workspace),
    }
}

fn run_milestone_via_claude_runner(
    path: &Path,
    logger: &Logger,
    model: Option<&str>,
    next_id: u32,
) -> Result<()> {
    let milestones_dir = path.join(".porpoise").join("milestones");
    let user_input_path = path.join(".porpoise").join("user_input.md");
    let project_md_path = path.join(".porpoise").join("project.md");
    let milestone_prompt_path = path.join(".porpoise").join("prompts").join("05-milestone.md");

    let prompt_template = std::fs::read_to_string(&milestone_prompt_path)
        .with_context(|| format!("05-milestone.md 읽기 실패: {}", milestone_prompt_path.display()))?;
    let rendered_prompt = apply_template(&prompt_template, &[("next_milestone_id", &next_id.to_string())]);

    let runner = ClaudeRunner::new()?;

    for attempt in 0..MAX_MILESTONE_RETRY {
        println!();
        println!("마일스톤 정보를 입력하세요. 예시:");
        println!("  제목: 새 마일스톤 제목");
        println!("  버전: v0.2.4");
        println!("  설명: 마일스톤 설명");
        println!("  작업:");
        println!("  - 작업 1 설명");
        println!("  - 작업 2 설명");

        let user_input = collect_multiline_input("마일스톤 정보")?;
        if user_input.trim().is_empty() {
            println!("{}", "입력이 없습니다. 취소됨.".yellow());
            return Ok(());
        }

        let input_content = format!("# 마일스톤 생성 요청\n\n{}\n", user_input);
        write_file(&user_input_path, &input_content, path)
            .context("user_input.md 저장 실패")?;

        let before_ids: HashSet<u32> = load_all_milestones(&milestones_dir)
            .unwrap_or_default()
            .iter()
            .map(|m| m.id)
            .collect();

        logger.info(
            "milestone_session",
            &format!("Claude 세션 실행 attempt={} next_id=M{}", attempt, next_id),
        );
        println!("{}", "  Claude 세션 실행 중...".cyan());

        let context_files = vec![project_md_path.clone(), user_input_path.clone()];

        let output = runner.run_with_prompt_str(
            &rendered_prompt,
            &context_files,
            None,
            model,
        )?;

        let report = parse_report(&output, "milestone_session");
        let exit_code = report.exit_code.clone().unwrap_or(ExitCode::Next);

        match exit_code {
            ExitCode::Next => {
                let new_milestones: Vec<Milestone> = load_all_milestones(&milestones_dir)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|m| !before_ids.contains(&m.id))
                    .collect();

                match new_milestones.into_iter().next() {
                    Some(m) => {
                        logger.info(
                            "milestone_session",
                            &format!("마일스톤 생성: M{} — {}", m.id, m.title),
                        );
                        append_milestone_to_project_md(path, &m)?;
                        println!(
                            "  {} M{}: {}",
                            "✓ 마일스톤 생성 완료".green(),
                            m.id,
                            m.title
                        );
                        let _ = std::fs::remove_file(&user_input_path);
                        let milestone_file = path.join(".porpoise").join("milestones").join(format!("M{}.md", m.id));
                        if let Ok(content) = std::fs::read_to_string(&milestone_file) {
                            println!();
                            for line in content.lines() {
                                println!("{}", line);
                            }
                        }
                        return Ok(());
                    }
                    None => {
                        logger.warn(
                            "milestone_session",
                            "NEXT 반환됐지만 새 마일스톤 파일 없음",
                        );
                        anyhow::bail!(
                            "Claude가 NEXT를 반환했지만 마일스톤 파일이 생성되지 않았습니다.\n\
                             .porpoise/milestones/ 디렉토리를 확인하세요."
                        );
                    }
                }
            }

            ExitCode::Resp => {
                println!("{}", "\n⚠  사용자 확인 필요 (RESP)".yellow().bold());
                for (i, q) in report.questions.iter().enumerate() {
                    println!("  {}. {}", i + 1, q.yellow());
                }
                if attempt + 1 >= MAX_MILESTONE_RETRY {
                    println!(
                        "{}",
                        format!("⚠  최대 재시도 횟수({})에 도달했습니다.", MAX_MILESTONE_RETRY)
                            .yellow()
                    );
                    break;
                }
                println!("\n수정된 정보를 다시 입력하세요.");
            }

            ExitCode::Prev => {
                logger.warn("milestone_session", "마일스톤 세션에서 예기치 않은 PREV");
                anyhow::bail!(
                    "마일스톤 생성 세션에서 예기치 않은 PREV 코드가 반환되었습니다."
                );
            }
        }
    }

    println!("{}", "마일스톤 생성 세션이 완료되지 않았습니다.".yellow());
    Ok(())
}

fn run_milestone_via_api(
    path: &Path,
    logger: &Logger,
    model: Option<&str>,
    next_id: u32,
    workspace: &WorkspaceConfig,
) -> Result<()> {
    use crate::model::factory::{make_adapter, make_model_config};
    use crate::orchestrator::state::Role;
    use crate::session::input::{MilestoneInfo, SessionInput};
    use crate::session::output::{ExitCode as SessionExitCode, RoleOutputData};

    let project_md_path = path.join(".porpoise").join("project.md");
    let project_summary = std::fs::read_to_string(&project_md_path).unwrap_or_default();

    let adapter = make_adapter(workspace, path)
        .context("마일스톤 생성 어댑터 초기화 실패")?;

    let mut config = make_model_config(workspace, &Role::PM);
    if let Some(m) = model {
        if !m.is_empty() {
            config.model_id = m.to_string();
        }
    }

    for attempt in 0..MAX_MILESTONE_RETRY {
        println!();
        println!("마일스톤 정보를 입력하세요. 예시:");
        println!("  제목: 새 마일스톤 제목");
        println!("  버전: v0.2.4");
        println!("  설명: 마일스톤 설명");
        println!("  작업:");
        println!("  - 작업 1 설명");
        println!("  - 작업 2 설명");

        let user_input = collect_multiline_input("마일스톤 정보")?;
        if user_input.trim().is_empty() {
            println!("{}", "입력이 없습니다. 취소됨.".yellow());
            return Ok(());
        }

        let input = SessionInput {
            role: "milestone".to_string(),
            task_id: format!("M{}-plan", next_id),
            task_title: format!("M{} 마일스톤 계획", next_id),
            cycle: 1,
            retry: attempt,
            language: workspace.language().to_string(),
            project_summary: project_summary.clone(),
            hints: vec![user_input.clone()],
            milestone: MilestoneInfo {
                id: format!("M{}", next_id),
                title: String::new(),
                version: String::new(),
                goal: String::new(),
            },
            ..SessionInput::default()
        };

        logger.info(
            "milestone_session",
            &format!("API 어댑터 마일스톤 생성 attempt={} next_id=M{}", attempt, next_id),
        );
        println!("{}", "  API 어댑터로 마일스톤 생성 중...".cyan());

        let output = adapter.execute(&input, &config)
            .context("마일스톤 생성 API 호출 실패")?;

        match output {
            RoleOutputData::Milestone(ref m) if m.status == SessionExitCode::Next => {
                let mut m = if let RoleOutputData::Milestone(m) = output { m } else { unreachable!() };
                if m.role.is_empty() {
                    m.role = "milestone".to_string();
                }
                if m.title.is_empty() {
                    anyhow::bail!(
                        "API 어댑터가 마일스톤 제목을 반환하지 않았습니다.\n\
                         milestone 역할 프롬프트(05-milestone.md)와 스키마를 확인하세요."
                    );
                }
                let milestone_id = m.milestone_id.trim_start_matches('M').parse::<u32>().unwrap_or(next_id);
                write_milestone_file(path, &m, milestone_id)
                    .context("M{n}.md 파일 생성 실패")?;
                let milestone = milestone_output_to_milestone(&m, next_id);
                append_milestone_to_project_md(path, &milestone)?;
                logger.info(
                    "milestone_session",
                    &format!("마일스톤 생성 완료 (API): M{} — {}", milestone_id, m.title),
                );
                println!(
                    "  {} M{}: {}",
                    "✓ 마일스톤 생성 완료".green(),
                    milestone_id,
                    m.title
                );
                let milestone_file = path.join(".porpoise").join("milestones").join(format!("M{}.md", milestone_id));
                if let Ok(content) = std::fs::read_to_string(&milestone_file) {
                    println!();
                    for line in content.lines() {
                        println!("{}", line);
                    }
                }
                return Ok(());
            }
            RoleOutputData::Milestone(ref m) if m.status == SessionExitCode::Resp => {
                println!("{}", "\n⚠  추가 정보 필요 (RESP)".yellow().bold());
                for (i, q) in m.questions.iter().enumerate() {
                    println!("  {}. {}", i + 1, q.yellow());
                }
                if attempt + 1 >= MAX_MILESTONE_RETRY {
                    println!(
                        "{}",
                        format!("⚠  최대 재시도 횟수({})에 도달했습니다.", MAX_MILESTONE_RETRY)
                            .yellow()
                    );
                    break;
                }
                println!("\n수정된 정보를 다시 입력하세요.");
            }
            _ => {
                anyhow::bail!(
                    "API 어댑터가 Milestone 출력을 반환하지 않았습니다.\n\
                     role='milestone'로 설정됐는지 확인하세요."
                );
            }
        }
    }

    println!("{}", "마일스톤 생성 세션이 완료되지 않았습니다.".yellow());
    Ok(())
}

fn write_milestone_file(
    path: &Path,
    output: &crate::session::milestone::MilestoneOutput,
    id: u32,
) -> Result<()> {
    let milestones_dir = path.join(".porpoise").join("milestones");
    std::fs::create_dir_all(&milestones_dir).context("milestones/ 디렉토리 생성 실패")?;

    let version_suffix = if output.version.is_empty() {
        String::new()
    } else {
        format!(" ({})", output.version)
    };

    let mut content = format!("# M{}: {}{}\n", id, output.title, version_suffix);

    if !output.goal.is_empty() {
        content.push_str(&format!("\n## 목표\n{}\n", output.goal));
    }

    if !output.background.as_deref().unwrap_or("").is_empty() {
        content.push_str(&format!("\n## 배경\n{}\n", output.background.as_deref().unwrap_or("")));
    }

    if !output.constraints.is_empty() {
        content.push_str("\n## 제약사항\n");
        for c in &output.constraints {
            content.push_str(&format!("- {}\n", c));
        }
    }

    content.push_str("\n## 작업 목록\n");
    for task in &output.tasks {
        content.push_str(&format!("- [ ] {}: {}\n", task.id, task.title));
    }

    let file_path = milestones_dir.join(format!("M{}.md", id));
    std::fs::write(&file_path, &content)
        .with_context(|| format!("M{}.md 파일 쓰기 실패: {}", id, file_path.display()))?;
    Ok(())
}

fn milestone_output_to_milestone(
    output: &crate::session::milestone::MilestoneOutput,
    next_id: u32,
) -> Milestone {
    use crate::orchestrator::state::Task;
    use std::collections::HashMap;

    let id = output.milestone_id.trim_start_matches('M').parse::<u32>().unwrap_or(next_id);
    let version = if output.version.is_empty() {
        None
    } else {
        Some(output.version.clone())
    };
    let tasks: Vec<Task> = output.tasks.iter().map(|t| Task {
        id: t.id.clone(),
        title: t.title.clone(),
        completed: false,
    }).collect();

    Milestone {
        id,
        title: output.title.clone(),
        version,
        tasks,
        metadata: HashMap::new(),
        raw_sections: HashMap::new(),
        file_path: std::path::PathBuf::new(),
    }
}

/// 파싱된 마일스톤을 project.md 끝에 Milestone 섹션으로 추가합니다.
pub fn append_milestone_to_project_md(path: &Path, milestone: &Milestone) -> Result<()> {
    let project_md_path = path.join(".porpoise").join("project.md");
    let content = std::fs::read_to_string(&project_md_path)
        .with_context(|| format!("project.md 읽기 실패: {}", project_md_path.display()))?;

    let version_suffix = milestone
        .version
        .as_deref()
        .map(|v| format!(" ({})", v))
        .unwrap_or_default();

    if content.contains(&format!("## Milestone {}:", milestone.id)) {
        return Ok(());
    }

    let mut new_section = format!(
        "\n## Milestone {}: {}{}\n",
        milestone.id, milestone.title, version_suffix
    );
    for task in &milestone.tasks {
        let checkbox = if task.completed { "[x]" } else { "[ ]" };
        new_section.push_str(&format!("- {} {}: {}\n", checkbox, task.id, task.title));
    }

    let new_content = format!("{}{}", content, new_section);
    write_file(&project_md_path, &new_content, path).context("project.md 업데이트 실패")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::state::Task;
    use std::collections::HashMap;

    fn make_milestone(id: u32, title: &str, version: Option<&str>, tasks: Vec<Task>) -> Milestone {
        Milestone {
            id,
            title: title.to_string(),
            version: version.map(str::to_string),
            tasks,
            metadata: HashMap::new(),
            raw_sections: HashMap::new(),
            file_path: std::path::PathBuf::new(),
        }
    }

    fn make_task(id: &str, title: &str, completed: bool) -> Task {
        Task { id: id.to_string(), title: title.to_string(), completed }
    }

    fn setup_project_md(dir: &std::path::Path, content: &str) {
        let docs = dir.join(".porpoise");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("project.md"), content).unwrap();
    }

    fn read_project_md(dir: &std::path::Path) -> String {
        std::fs::read_to_string(dir.join(".porpoise").join("project.md")).unwrap()
    }

    #[test]
    fn append_no_version() {
        let dir = tempfile::tempdir().unwrap();
        setup_project_md(dir.path(), "# 프로젝트\n");

        let m = make_milestone(2, "두 번째 마일스톤", None, vec![
            make_task("M2-T01", "작업1", false),
            make_task("M2-T02", "작업2", true),
        ]);
        append_milestone_to_project_md(dir.path(), &m).unwrap();

        let content = read_project_md(dir.path());
        assert!(content.contains("## Milestone 2: 두 번째 마일스톤\n"));
        assert!(content.contains("- [ ] M2-T01: 작업1\n"));
        assert!(content.contains("- [x] M2-T02: 작업2\n"));
        assert!(!content.contains("(v"));
    }

    #[test]
    fn append_with_version() {
        let dir = tempfile::tempdir().unwrap();
        setup_project_md(dir.path(), "# 기존\n");

        let m = make_milestone(3, "세 번째", Some("v0.3.0"), vec![]);
        append_milestone_to_project_md(dir.path(), &m).unwrap();

        let content = read_project_md(dir.path());
        assert!(content.contains("## Milestone 3: 세 번째 (v0.3.0)\n"));
    }

    #[test]
    fn append_preserves_existing_content() {
        let original = "# 프로젝트\n\n## 기존 섹션\n기존 내용\n";
        let dir = tempfile::tempdir().unwrap();
        setup_project_md(dir.path(), original);

        let m = make_milestone(1, "첫 번째", None, vec![]);
        append_milestone_to_project_md(dir.path(), &m).unwrap();

        let content = read_project_md(dir.path());
        assert!(content.starts_with(original));
        assert!(content.contains("## Milestone 1: 첫 번째\n"));
    }

    #[test]
    fn append_duplicate_milestone_skipped() {
        let dir = tempfile::tempdir().unwrap();
        setup_project_md(dir.path(), "# 프로젝트\n");

        let m = make_milestone(1, "첫 번째", None, vec![
            make_task("M1-T01", "작업1", false),
        ]);
        append_milestone_to_project_md(dir.path(), &m).unwrap();
        append_milestone_to_project_md(dir.path(), &m).unwrap();

        let content = read_project_md(dir.path());
        let count = content.matches("## Milestone 1:").count();
        assert_eq!(count, 1, "중복 섹션이 추가되어서는 안 됩니다");
    }

    #[test]
    fn mark_task_complete_updates_milestone_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_project_md(dir.path(), "# 프로젝트\n\n- [ ] M1-T01: 작업1\n");

        let milestones_dir = dir.path().join(".porpoise").join("milestones");
        std::fs::create_dir_all(&milestones_dir).unwrap();
        std::fs::write(
            milestones_dir.join("M1.md"),
            "# M1: 테스트\n\n- [ ] M1-T01: 작업1\n",
        )
        .unwrap();

        let logger = crate::logger::Logger::new(dir.path(), false).unwrap();
        crate::milestone::update_task_status(dir.path(), "M1-T01", true, &logger);

        let m1_content =
            std::fs::read_to_string(milestones_dir.join("M1.md")).unwrap();
        assert!(m1_content.contains("- [x] M1-T01:"));
    }

    #[test]
    fn mark_task_complete_missing_milestone_file() {
        let dir = tempfile::tempdir().unwrap();
        setup_project_md(dir.path(), "# 프로젝트\n\n- [ ] M1-T01: 작업1\n");
        std::fs::create_dir_all(dir.path().join(".porpoise").join("milestones")).unwrap();

        let logger = crate::logger::Logger::new(dir.path(), false).unwrap();
        crate::milestone::update_task_status(dir.path(), "M1-T01", true, &logger);
    }

    #[test]
    fn milestone_output_to_milestone_basic() {
        use crate::session::milestone::{MilestoneOutput, MilestoneTask};
        use crate::session::output::ExitCode as SessionExitCode;

        let output = MilestoneOutput {
            milestone_id: "M3".to_string(),
            title: "테스트 마일스톤".to_string(),
            version: "v0.9.0".to_string(),
            tasks: vec![
                MilestoneTask {
                    id: "M3-T01".to_string(),
                    title: "작업 1".to_string(),
                    description: None,
                },
            ],
            status: SessionExitCode::Next,
            ..MilestoneOutput::default()
        };
        let m = milestone_output_to_milestone(&output, 3);
        assert_eq!(m.id, 3);
        assert_eq!(m.title, "테스트 마일스톤");
        assert_eq!(m.version, Some("v0.9.0".to_string()));
        assert_eq!(m.tasks.len(), 1);
        assert_eq!(m.tasks[0].id, "M3-T01");
        assert!(!m.tasks[0].completed);
    }

    #[test]
    fn milestone_output_to_milestone_no_version() {
        use crate::session::milestone::MilestoneOutput;
        use crate::session::output::ExitCode as SessionExitCode;

        let output = MilestoneOutput {
            milestone_id: "M5".to_string(),
            title: "버전 없는 마일스톤".to_string(),
            version: String::new(),
            status: SessionExitCode::Next,
            ..MilestoneOutput::default()
        };
        let m = milestone_output_to_milestone(&output, 5);
        assert_eq!(m.id, 5);
        assert!(m.version.is_none());
    }

    #[test]
    fn write_milestone_file_creates_parseable_file() {
        use crate::milestone::parser::{load_all_milestones, parse_milestone_file};
        use crate::session::milestone::{MilestoneOutput, MilestoneTask};
        use crate::session::output::ExitCode as SessionExitCode;

        let dir = tempfile::tempdir().unwrap();
        let output = MilestoneOutput {
            milestone_id: "M2".to_string(),
            title: "두 번째 마일스톤".to_string(),
            version: "v0.9.0".to_string(),
            goal: "목표 내용".to_string(),
            tasks: vec![
                MilestoneTask { id: "M2-T01".to_string(), title: "작업 A".to_string(), description: None },
                MilestoneTask { id: "M2-T02".to_string(), title: "작업 B".to_string(), description: None },
            ],
            status: SessionExitCode::Next,
            ..MilestoneOutput::default()
        };

        write_milestone_file(dir.path(), &output, 2).unwrap();

        let file_path = dir.path().join(".porpoise").join("milestones").join("M2.md");
        assert!(file_path.exists());

        let parsed = parse_milestone_file(&file_path).unwrap();
        assert_eq!(parsed.id, 2);
        assert_eq!(parsed.title, "두 번째 마일스톤");
        assert_eq!(parsed.version, Some("v0.9.0".to_string()));
        assert_eq!(parsed.tasks.len(), 2);
        assert_eq!(parsed.tasks[0].id, "M2-T01");
        assert_eq!(parsed.tasks[1].id, "M2-T02");
        assert!(!parsed.tasks[0].completed);
    }

    #[test]
    fn write_milestone_file_enables_correct_next_id() {
        use crate::milestone::parser::load_all_milestones;
        use crate::session::milestone::{MilestoneOutput, MilestoneTask};
        use crate::session::output::ExitCode as SessionExitCode;

        let dir = tempfile::tempdir().unwrap();
        let milestones_dir = dir.path().join(".porpoise").join("milestones");

        // M1 생성
        let m1 = MilestoneOutput {
            milestone_id: "M1".to_string(),
            title: "첫 번째".to_string(),
            tasks: vec![MilestoneTask { id: "M1-T01".to_string(), title: "T01".to_string(), description: None }],
            status: SessionExitCode::Next,
            ..MilestoneOutput::default()
        };
        write_milestone_file(dir.path(), &m1, 1).unwrap();

        // 다음 ID 계산
        let max_id = load_all_milestones(&milestones_dir).unwrap()
            .iter().map(|m| m.id).max().unwrap_or(0);
        assert_eq!(max_id + 1, 2, "M1.md가 생성되었으므로 next_id는 2여야 함");
    }
}
