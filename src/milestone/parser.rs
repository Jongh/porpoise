use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::orchestrator::state::Task;

#[derive(Debug, Clone)]
pub struct Milestone {
    pub id: u32,
    pub title: String,
    pub version: Option<String>,
    pub tasks: Vec<Task>,
    #[allow(dead_code)]
    pub metadata: HashMap<String, String>,
    #[allow(dead_code)]
    pub raw_sections: HashMap<String, String>,
    #[allow(dead_code)]
    pub file_path: PathBuf,
}

pub fn parse_milestone_file(path: &Path) -> Result<Milestone> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read milestone file: {}", path.display()))?;
    parse_milestone_content(&content, path)
}

#[allow(dead_code)]
pub fn load_milestone(milestones_dir: &Path, milestone_id: u32) -> Result<Option<Milestone>> {
    let file_path = milestones_dir.join(format!("M{}.md", milestone_id));
    if !file_path.exists() {
        return Ok(None);
    }
    parse_milestone_file(&file_path).map(Some)
}

pub fn load_all_milestones(milestones_dir: &Path) -> Result<Vec<Milestone>> {
    if !milestones_dir.exists() {
        return Ok(vec![]);
    }

    let entries = std::fs::read_dir(milestones_dir)
        .with_context(|| format!("Failed to read milestones directory: {}", milestones_dir.display()))?;

    let mut milestones = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.starts_with('M') && stem.len() > 1 && stem[1..].parse::<u32>().is_ok() {
            milestones.push(parse_milestone_file(&path)?);
        }
    }

    milestones.sort_by_key(|m| m.id);
    Ok(milestones)
}

fn parse_milestone_content(content: &str, path: &Path) -> Result<Milestone> {
    let mut lines = content.lines();

    let title_line = lines.next().unwrap_or("").trim();
    let (id, title, version) = parse_title(title_line)
        .with_context(|| format!("Invalid milestone title line: {:?}", title_line))?;

    let mut sections: HashMap<String, String> = HashMap::new();
    let mut current_section: Option<String> = None;
    let mut current_content = String::new();

    for line in lines {
        if let Some(sec_name) = line.strip_prefix("## ") {
            if let Some(sec) = current_section.take() {
                sections.insert(sec, current_content.trim().to_string());
            }
            current_content = String::new();
            current_section = Some(sec_name.trim().to_string());
        } else if current_section.is_some() {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }
    if let Some(sec) = current_section {
        sections.insert(sec, current_content.trim().to_string());
    }

    let tasks = sections
        .remove("작업 목록")
        .map(|c| parse_tasks(&c))
        .unwrap_or_default();

    let metadata = sections
        .remove("메타데이터")
        .map(|c| parse_metadata(&c))
        .unwrap_or_default();

    Ok(Milestone {
        id,
        title,
        version,
        tasks,
        metadata,
        raw_sections: sections,
        file_path: path.to_path_buf(),
    })
}

fn parse_title(line: &str) -> Option<(u32, String, Option<String>)> {
    let rest = line.strip_prefix("# M")?;
    let colon_pos = rest.find(": ")?;
    let id: u32 = rest[..colon_pos].trim().parse().ok()?;
    let title_part = rest[colon_pos + 2..].trim();

    if let Some(paren_pos) = title_part.rfind(" (") {
        let after_paren = &title_part[paren_pos + 2..];
        if after_paren.ends_with(')') && after_paren.starts_with('v') {
            let version = after_paren[..after_paren.len() - 1].to_string();
            let title = title_part[..paren_pos].to_string();
            return Some((id, title, Some(version)));
        }
    }

    Some((id, title_part.to_string(), None))
}

fn parse_tasks(content: &str) -> Vec<Task> {
    let mut tasks = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- [ ] ") && !trimmed.starts_with("- [x] ") {
            continue;
        }
        let completed = trimmed.starts_with("- [x] ");
        let rest = &trimmed[6..];
        if let Some(colon_pos) = rest.find(": ") {
            let id_part = rest[..colon_pos].trim();
            let title = rest[colon_pos + 2..].trim();
            if id_part.starts_with('M') && id_part.contains("-T") {
                tasks.push(Task {
                    id: id_part.to_string(),
                    title: title.to_string(),
                    completed,
                });
            }
        }
    }
    tasks
}

fn parse_metadata(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("- ") {
            if let Some(colon_pos) = rest.find(": ") {
                let key = rest[..colon_pos].trim().to_string();
                let value = rest[colon_pos + 2..].trim().to_string();
                map.insert(key, value);
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# M1: 마일스톤 생성 세션 구현 (v0.1.3)

## 목표
사용자가 대화형으로 새 마일스톤을 정의하고, Claude 세션이 구조화된
milestone.md를 자동 생성할 수 있는 흐름을 구현한다.

## 배경
현재 project.md에서 작업 목록만 관리된다.

## 제약사항
- Rust stable 컴파일러만 사용 (nightly 금지)

## 작업 목록
- [ ] M1-T01: milestone.md 파일 형식 정의 및 파서 구현
- [x] M1-T02: 00-orche.md 마일스톤 생성 세션 프롬프트 작성
- [ ] M1-T03: 사용자 다중 줄 입력 수집 기능 구현

## 메타데이터
- created: 2026-04-23
- version: v0.1.3
- status: in-progress
";

    #[test]
    fn test_parse_title_with_version() {
        let (id, title, version) = parse_title("# M1: 마일스톤 생성 세션 구현 (v0.1.3)").unwrap();
        assert_eq!(id, 1);
        assert_eq!(title, "마일스톤 생성 세션 구현");
        assert_eq!(version, Some("v0.1.3".to_string()));
    }

    #[test]
    fn test_parse_title_without_version() {
        let (id, title, version) = parse_title("# M2: 두 번째 마일스톤").unwrap();
        assert_eq!(id, 2);
        assert_eq!(title, "두 번째 마일스톤");
        assert!(version.is_none());
    }

    #[test]
    fn test_parse_title_invalid() {
        assert!(parse_title("## 잘못된 헤더").is_none());
        assert!(parse_title("# 콜론없음").is_none());
        assert!(parse_title("# Mnot: 숫자아님").is_none());
    }

    #[test]
    fn test_parse_tasks() {
        let content = "- [ ] M1-T01: 파서 구현\n- [x] M1-T02: 프롬프트 작성\n- [ ] M1-T03: 입력 기능";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].id, "M1-T01");
        assert!(!tasks[0].completed);
        assert_eq!(tasks[1].id, "M1-T02");
        assert!(tasks[1].completed);
        assert_eq!(tasks[2].title, "입력 기능");
    }

    #[test]
    fn test_parse_tasks_ignores_non_mt_format() {
        let content = "- [ ] T01: 잘못된 형식\n- [x] M1-T01: 올바른 형식";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "M1-T01");
    }

    #[test]
    fn test_parse_metadata() {
        let content = "- created: 2026-04-23\n- status: in-progress\n- version: v0.1.3";
        let meta = parse_metadata(content);
        assert_eq!(meta.get("created"), Some(&"2026-04-23".to_string()));
        assert_eq!(meta.get("status"), Some(&"in-progress".to_string()));
        assert_eq!(meta.get("version"), Some(&"v0.1.3".to_string()));
    }

    #[test]
    fn test_full_parse_from_string() {
        let tmp = std::env::temp_dir().join("porpoise_test_M1.md");
        std::fs::write(&tmp, SAMPLE).unwrap();

        let m = parse_milestone_file(&tmp).unwrap();
        assert_eq!(m.id, 1);
        assert_eq!(m.title, "마일스톤 생성 세션 구현");
        assert_eq!(m.version, Some("v0.1.3".to_string()));
        assert_eq!(m.tasks.len(), 3);
        assert!(!m.tasks[0].completed);
        assert!(m.tasks[1].completed);
        assert_eq!(m.metadata.get("status"), Some(&"in-progress".to_string()));
        assert!(m.raw_sections.contains_key("목표"));
        assert!(m.raw_sections.contains_key("배경"));
        assert!(m.raw_sections.contains_key("제약사항"));
        assert!(!m.raw_sections.contains_key("작업 목록"));
        assert!(!m.raw_sections.contains_key("메타데이터"));

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_load_milestone_not_found() {
        let tmp_dir = std::env::temp_dir().join("porpoise_test_ms_empty");
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let result = load_milestone(&tmp_dir, 99).unwrap();
        assert!(result.is_none());

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn test_load_all_milestones() {
        let tmp_dir = std::env::temp_dir().join("porpoise_test_ms_all");
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let m1 = "# M1: 첫 번째\n\n## 작업 목록\n- [ ] M1-T01: 작업1\n\n## 메타데이터\n- status: active\n";
        let m2 = "# M2: 두 번째 (v0.2.0)\n\n## 작업 목록\n- [x] M2-T01: 완료\n\n## 메타데이터\n- status: done\n";
        std::fs::write(tmp_dir.join("M2.md"), m2).unwrap();
        std::fs::write(tmp_dir.join("M1.md"), m1).unwrap();

        let milestones = load_all_milestones(&tmp_dir).unwrap();
        assert_eq!(milestones.len(), 2);
        assert_eq!(milestones[0].id, 1);
        assert_eq!(milestones[1].id, 2);
        assert_eq!(milestones[1].version, Some("v0.2.0".to_string()));

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn test_load_all_milestones_nonexistent_dir() {
        let result = load_all_milestones(Path::new("/nonexistent/path/to/milestones"));
        assert!(result.unwrap().is_empty());
    }
}
