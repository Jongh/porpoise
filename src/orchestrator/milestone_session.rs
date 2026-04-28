use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashSet;
use std::path::Path;

use crate::claude::runner::ClaudeRunner;
use crate::logger::Logger;
use crate::milestone::{load_all_milestones, Milestone};
use crate::orchestrator::report::{parse_report, ExitCode};
use crate::utils::fs::write_file;
use crate::utils::input::collect_multiline_input;

const MAX_RESP_RETRY: u32 = 5;

/// 마일스톤 생성 세션을 실행합니다.
/// 사용자로부터 마일스톤 정보를 수집하고 Claude 세션을 통해 M{id}.md를 생성한 뒤
/// project.md에 Milestone 섹션을 추가합니다.
pub fn run_milestone_session(path: &Path, dry_run: bool, logger: &Logger) -> Result<()> {
    println!("{}", "\n=== 마일스톤 생성 세션 ===".cyan().bold());

    if dry_run {
        println!("{}", "  [dry-run] 마일스톤 생성 세션 스킵".dimmed());
        return Ok(());
    }

    let milestones_dir = path.join(".docs").join("milestones");
    let user_input_path = path.join(".docs").join("user_input.md");
    let prompt_file = path.join(".docs").join("prompts").join("00-orche.md");

    let runner = ClaudeRunner::new()?;

    for attempt in 0..MAX_RESP_RETRY {
        println!();
        println!("마일스톤 정보를 입력하세요. 예시:");
        println!("  제목: 새 마일스톤 제목");
        println!("  버전: v0.2.0");
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

        // 세션 실행 전 기존 마일스톤 ID 스냅샷
        let before_ids: HashSet<u32> = load_all_milestones(&milestones_dir)
            .unwrap_or_default()
            .iter()
            .map(|m| m.id)
            .collect();

        let output_file = path
            .join(".docs")
            .join("reports")
            .join(format!("milestone-session-R{}.md", attempt));

        logger.info(
            "milestone_session",
            &format!("Claude 세션 실행 attempt={}", attempt),
        );
        println!("{}", "  Claude 세션 실행 중...".cyan());

        let output = runner.run_with_prompt(
            &prompt_file,
            &[user_input_path.clone()],
            &output_file,
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
                        return Ok(());
                    }
                    None => {
                        logger.warn(
                            "milestone_session",
                            "NEXT 반환됐지만 새 마일스톤 파일 없음",
                        );
                        anyhow::bail!(
                            "Claude가 NEXT를 반환했지만 마일스톤 파일이 생성되지 않았습니다.\n\
                             .docs/milestones/ 디렉토리를 확인하세요."
                        );
                    }
                }
            }

            ExitCode::Resp => {
                println!("{}", "\n⚠  사용자 확인 필요 (RESP)".yellow().bold());
                for (i, q) in report.questions.iter().enumerate() {
                    println!("  {}. {}", i + 1, q.yellow());
                }
                if attempt + 1 >= MAX_RESP_RETRY {
                    println!(
                        "{}",
                        format!("⚠  최대 재시도 횟수({})에 도달했습니다.", MAX_RESP_RETRY)
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

/// 파싱된 마일스톤을 project.md 끝에 Milestone 섹션으로 추가합니다.
/// `path`는 프로젝트 루트 디렉토리이며, `.docs/project.md`를 기준으로 찾습니다.
pub fn append_milestone_to_project_md(path: &Path, milestone: &Milestone) -> Result<()> {
    let project_md_path = path.join(".docs").join("project.md");
    let content = std::fs::read_to_string(&project_md_path)
        .with_context(|| format!("project.md 읽기 실패: {}", project_md_path.display()))?;

    let version_suffix = milestone
        .version
        .as_deref()
        .map(|v| format!(" ({})", v))
        .unwrap_or_default();

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
        let docs = dir.join(".docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("project.md"), content).unwrap();
    }

    fn read_project_md(dir: &std::path::Path) -> String {
        std::fs::read_to_string(dir.join(".docs").join("project.md")).unwrap()
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
}
