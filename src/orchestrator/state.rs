use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// Task identifier with enforced 2-digit zero-pad on the T-number (e.g. "M1-T1" → "M1-T01").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    pub fn new(s: &str) -> Self {
        TaskId(normalize_task_id(s))
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn normalize_task_id(s: &str) -> String {
    if let Some((m_part, t_rest)) = s.split_once("-T") {
        if m_part.starts_with('M') {
            let digits: String = t_rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                if let Ok(n) = digits.parse::<u32>() {
                    let suffix = &t_rest[digits.len()..];
                    return format!("{}-T{:02}{}", m_part, n, suffix);
                }
            }
        }
    }
    s.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    PM,
    Developer,
    Tester,
    Reviewer,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::PM => write!(f, "planning"),
            Role::Developer => write!(f, "development"),
            Role::Tester => write!(f, "testing"),
            Role::Reviewer => write!(f, "review"),
        }
    }
}

impl Role {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "planning" | "pm" => Some(Role::PM),
            "development" | "developer" | "dev" => Some(Role::Developer),
            "testing" | "tester" | "test" => Some(Role::Tester),
            "review" | "reviewer" => Some(Role::Reviewer),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Role::PM => "Planning",
            Role::Developer => "Development",
            Role::Tester => "Testing",
            Role::Reviewer => "Review",
        }
    }

    pub fn next(&self) -> Option<Role> {
        match self {
            Role::PM => Some(Role::Developer),
            Role::Developer => Some(Role::Tester),
            Role::Tester => Some(Role::Reviewer),
            Role::Reviewer => None,
        }
    }

    pub fn all() -> Vec<Role> {
        vec![Role::PM, Role::Developer, Role::Tester, Role::Reviewer]
    }
}

#[derive(Debug, Clone, Default)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub completed: bool,
    /// 선행 task id 목록 (M24). `(deps: M{n}-T01, ...)` 표기에서 파싱. 없으면 빈 vec.
    pub dependencies: Vec<String>,
}

/// task 제목에서 `(deps: id, id, ...)` 표기를 분리한다. (정제된 제목, 정규화된 의존성 id 목록) 반환.
/// 표기가 없으면 (원래 제목, 빈 vec). (M24)
pub fn parse_task_deps(title: &str) -> (String, Vec<String>) {
    if let Some(start) = title.rfind("(deps:") {
        if let Some(end_rel) = title[start..].find(')') {
            let end = start + end_rel;
            let inner = &title[start + "(deps:".len()..end];
            let deps: Vec<String> = inner
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| TaskId::new(s).to_string())
                .collect();
            let clean = format!("{}{}", &title[..start], &title[end + 1..])
                .trim()
                .to_string();
            return (clean, deps);
        }
    }
    (title.to_string(), vec![])
}

/// Parses M{n}-T{nn} task items from .porpoise/project.md.
/// Returns empty vec if project.md is absent or has no M-T formatted tasks.
/// Lines inside markdown code fences (``` blocks) are skipped to prevent
/// example task items in documentation from being parsed as real tasks.
pub fn parse_tasks_from_project_md(path: &Path) -> Vec<Task> {
    let project_md = path.join(".porpoise").join("project.md");
    let content = match std::fs::read_to_string(&project_md) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut tasks = Vec::new();
    let mut in_code_block = false;

    for line in content.lines() {
        if line.trim().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.starts_with("- [ ] ") || trimmed.starts_with("- [x] ") {
            let completed = trimmed.starts_with("- [x] ");
            let rest = &trimmed[6..]; // skip "- [ ] " or "- [x] "

            if let Some(colon_pos) = rest.find(": ") {
                let id_part = rest[..colon_pos].trim();
                let raw_title = rest[colon_pos + 2..].trim();
                // Only accept M{n}-T{nn} format
                if id_part.starts_with('M') && id_part.contains("-T") {
                    let (title, dependencies) = parse_task_deps(raw_title);
                    tasks.push(Task {
                        id: TaskId::new(id_part).to_string(),
                        title,
                        completed,
                        dependencies,
                    });
                }
            }
        }
    }

    tasks
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorState {
    pub cycle: u32,
    pub current_role: Option<Role>,
    pub completed_roles: Vec<Role>,
    pub project_path: PathBuf,
    pub current_task_id: String,
    pub current_task_title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_execution_results: Vec<crate::session::v0_7::ExecutionResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prev_reasons: Vec<String>,
}

impl OrchestratorState {
    pub fn new(path: &Path) -> Self {
        OrchestratorState {
            cycle: 1,
            current_role: Some(Role::PM),
            completed_roles: vec![],
            project_path: path.to_path_buf(),
            current_task_id: "M0-T00".to_string(),
            current_task_title: "미지정".to_string(),
            pending_execution_results: vec![],
            prev_reasons: vec![],
        }
    }
}

pub fn load_state(path: &Path) -> Result<OrchestratorState> {
    // Try checkpoint at new path (.porpoise/checkpoint.json) and legacy paths
    let has_checkpoint = path.join(".porpoise").join("checkpoint.json").exists()
        || path.join(".porpoise").join("messages").join("checkpoint.json").exists()
        || path.join(".porpoise").join("messages").join("checkpoint.md").exists();

    if has_checkpoint {
        if let Ok(checkpoint) = super::checkpoint::load_checkpoint(path) {
            let completed = checkpoint
                .completed_roles
                .iter()
                .filter_map(|r| Role::from_str(r))
                .collect::<Vec<_>>();

            let next_role = Role::from_str(&checkpoint.next_role);

            // Resolve task_id: checkpoint > project.md first uncompleted
            let (task_id, task_title) =
                resolve_task_id(&checkpoint.current_task_id, path);

            return Ok(OrchestratorState {
                cycle: checkpoint.cycle,
                current_role: next_role,
                completed_roles: completed,
                project_path: path.to_path_buf(),
                current_task_id: task_id,
                current_task_title: task_title,
                pending_execution_results: vec![],
                prev_reasons: checkpoint.prev_reasons.clone(),
            });
        }
    }

    // Fallback: infer state from report filenames.
    // Check both messages/ (Porpoise output) and reports/ (Claude's formatted reports).
    let mut report_files: Vec<String> = Vec::new();
    let fallback_dirs = [
        path.join(".porpoise").join("messages"),
        path.join(".porpoise").join("reports"),
    ];
    for dir in &fallback_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if (name.ends_with("-report.md") || name.ends_with(".md"))
                    && name != "checkpoint.md"
                    && !name.starts_with("checkpoint")
                    && !report_files.contains(&name)
                {
                    report_files.push(name);
                }
            }
        }
    }
    report_files.sort();

    if report_files.is_empty() {
        return Ok(build_state_with_tasks(OrchestratorState::new(path), path));
    }

    let all_roles = Role::all();
    let mut completed_role_list: Vec<Role> = Vec::new();
    for filename in &report_files {
        if let Some(role) = extract_role_from_filename(filename) {
            if !completed_role_list.contains(&role) {
                completed_role_list.push(role);
            }
        }
    }

    let next_role = all_roles
        .iter()
        .find(|r| !completed_role_list.contains(r))
        .cloned();

    let (cycle, current_role, completed) = if next_role.is_none() {
        (2, Some(Role::PM), vec![])
    } else {
        (1, next_role, completed_role_list)
    };

    let state = OrchestratorState {
        cycle,
        current_role,
        completed_roles: completed,
        project_path: path.to_path_buf(),
        current_task_id: "M0-T00".to_string(),
        current_task_title: "미지정".to_string(),
        pending_execution_results: vec![],
        prev_reasons: vec![],
    };
    Ok(build_state_with_tasks(state, path))
}

/// Fills current_task_id/title from project.md if the provided id is empty or "M0-T00".
fn build_state_with_tasks(mut state: OrchestratorState, path: &Path) -> OrchestratorState {
    if state.current_task_id.is_empty() || state.current_task_id == "M0-T00" {
        let tasks = parse_tasks_from_project_md(path);
        if let Some(first_open) = tasks.iter().find(|t| !t.completed) {
            state.current_task_id = first_open.id.clone();
            state.current_task_title = first_open.title.clone();
        }
    }
    state
}

/// Returns (task_id, task_title). Prefers checkpoint value; falls back to
/// first uncompleted task from project.md; then default "M0-T00".
fn resolve_task_id(checkpoint_task_id: &str, path: &Path) -> (String, String) {
    if !checkpoint_task_id.is_empty() && checkpoint_task_id != "M0-T00" {
        let tasks = parse_tasks_from_project_md(path);
        let title = tasks
            .iter()
            .find(|t| t.id == checkpoint_task_id)
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "미지정".to_string());
        return (checkpoint_task_id.to_string(), title);
    }

    let tasks = parse_tasks_from_project_md(path);
    if let Some(first_open) = tasks.iter().find(|t| !t.completed) {
        return (first_open.id.clone(), first_open.title.clone());
    }

    ("M0-T00".to_string(), "미지정".to_string())
}

fn extract_role_from_filename(filename: &str) -> Option<Role> {
    // New format: {task_id}-{role}-C{n}-R{n}.md
    // e.g. M1-T01-pm-C1-R0.md
    if let Some(role) = extract_from_new_format(filename) {
        return Some(role);
    }
    // Old format: {timestamp}-{role}-report.md
    extract_from_old_format(filename)
}

fn extract_from_new_format(filename: &str) -> Option<Role> {
    let without_ext = filename.strip_suffix(".md")?;
    // Find -C{n}-R{n} suffix
    let c_pos = without_ext.rfind("-C")?;
    let before_cycle = &without_ext[..c_pos];
    // The role is the last segment before -C
    let role_str = before_cycle.rsplit('-').next()?;
    Role::from_str(role_str)
}

fn extract_from_old_format(filename: &str) -> Option<Role> {
    let without_ext = filename.strip_suffix(".md")?;
    let without_report = without_ext.strip_suffix("-report")?;
    let parts: Vec<&str> = without_report.splitn(3, '-').collect();
    if parts.len() >= 3 {
        return Role::from_str(parts[2]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_task_deps_none() {
        let (title, deps) = parse_task_deps("add 함수 추가");
        assert_eq!(title, "add 함수 추가");
        assert!(deps.is_empty());
    }

    #[test]
    fn parse_task_deps_extracts_and_strips() {
        let (title, deps) = parse_task_deps("sub 함수 추가 (deps: M24-T01, M24-T02)");
        assert_eq!(title, "sub 함수 추가");
        assert_eq!(deps, vec!["M24-T01", "M24-T02"]);
    }

    #[test]
    fn parse_task_deps_normalizes_ids() {
        // T1 → T01 정규화 (zero-pad)
        let (_t, deps) = parse_task_deps("작업 (deps: M24-T1)");
        assert_eq!(deps, vec!["M24-T01"]);
    }

    #[test]
    fn parse_tasks_from_project_md_with_deps() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join(".porpoise");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(
            docs.join("project.md"),
            "## 작업 목록\n- [ ] M1-T01: 첫 작업\n- [ ] M1-T02: 둘째 작업 (deps: M1-T01)\n",
        )
        .unwrap();
        let tasks = parse_tasks_from_project_md(temp.path());
        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].dependencies.is_empty());
        assert_eq!(tasks[1].dependencies, vec!["M1-T01"]);
        assert_eq!(tasks[1].title, "둘째 작업"); // deps 표기 제거됨
    }

    #[test]
    fn role_from_str_all_variants() {
        // new names
        assert_eq!(Role::from_str("planning"), Some(Role::PM));
        assert_eq!(Role::from_str("development"), Some(Role::Developer));
        assert_eq!(Role::from_str("testing"), Some(Role::Tester));
        assert_eq!(Role::from_str("review"), Some(Role::Reviewer));
        // backward-compat aliases
        assert_eq!(Role::from_str("pm"), Some(Role::PM));
        assert_eq!(Role::from_str("PM"), Some(Role::PM));
        assert_eq!(Role::from_str("developer"), Some(Role::Developer));
        assert_eq!(Role::from_str("dev"), Some(Role::Developer));
        assert_eq!(Role::from_str("tester"), Some(Role::Tester));
        assert_eq!(Role::from_str("test"), Some(Role::Tester));
        assert_eq!(Role::from_str("reviewer"), Some(Role::Reviewer));
        assert_eq!(Role::from_str("unknown"), None);
        assert_eq!(Role::from_str(""), None);
    }

    #[test]
    fn role_next_sequence() {
        assert_eq!(Role::PM.next(), Some(Role::Developer));
        assert_eq!(Role::Developer.next(), Some(Role::Tester));
        assert_eq!(Role::Tester.next(), Some(Role::Reviewer));
        assert_eq!(Role::Reviewer.next(), None);
    }

    #[test]
    fn role_display() {
        assert_eq!(Role::PM.to_string(), "planning");
        assert_eq!(Role::Developer.to_string(), "development");
        assert_eq!(Role::Tester.to_string(), "testing");
        assert_eq!(Role::Reviewer.to_string(), "review");
    }

    #[test]
    fn role_all_has_four_elements() {
        let all = Role::all();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0], Role::PM);
        assert_eq!(all[3], Role::Reviewer);
    }

    #[test]
    fn extract_role_new_format() {
        // new names
        assert_eq!(
            extract_from_new_format("M1-T01-planning-C1-R0.md"),
            Some(Role::PM)
        );
        assert_eq!(
            extract_from_new_format("M1-T01-development-C2-R1.md"),
            Some(Role::Developer)
        );
        assert_eq!(
            extract_from_new_format("M1-T01-testing-C1-R0.md"),
            Some(Role::Tester)
        );
        assert_eq!(
            extract_from_new_format("M1-T01-review-C1-R0.md"),
            Some(Role::Reviewer)
        );
        // backward-compat aliases in filenames
        assert_eq!(
            extract_from_new_format("M1-T01-pm-C1-R0.md"),
            Some(Role::PM)
        );
        assert_eq!(
            extract_from_new_format("M1-T01-developer-C2-R1.md"),
            Some(Role::Developer)
        );
        assert_eq!(
            extract_from_new_format("M1-T01-tester-C1-R0.md"),
            Some(Role::Tester)
        );
        assert_eq!(
            extract_from_new_format("M1-T01-reviewer-C1-R0.md"),
            Some(Role::Reviewer)
        );
    }

    #[test]
    fn extract_role_old_format() {
        assert_eq!(
            extract_from_old_format("20260421-120000-pm-report.md"),
            Some(Role::PM)
        );
        assert_eq!(
            extract_from_old_format("20260421-120000-developer-report.md"),
            Some(Role::Developer)
        );
    }

    #[test]
    fn parse_tasks_empty_when_no_project_md() {
        let tasks = parse_tasks_from_project_md(std::path::Path::new("/nonexistent/path"));
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_tasks_ignores_non_task_format() {
        // Old format "- [ ] 마일스톤 1: 초기 구현" has no M{n}-T{nn} prefix → ignored
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join(".porpoise");
        std::fs::create_dir_all(&docs).unwrap();
        let project_md = docs.join("project.md");
        std::fs::write(
            &project_md,
            "## 마일스톤\n- [ ] 마일스톤 1: 초기 구현\n",
        )
        .unwrap();
        let tasks = parse_tasks_from_project_md(temp.path());
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_tasks_skips_lines_inside_code_block() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join(".porpoise");
        std::fs::create_dir_all(&docs).unwrap();
        let project_md = docs.join("project.md");
        // Example task lines inside a code fence must not be parsed
        std::fs::write(
            &project_md,
            "## 예시\n```\n- [ ] M1-T01: 예시 작업\n- [x] M1-T02: 완료 예시\n```\n\n## 실제 작업\n- [ ] M2-T01: 실제 작업\n",
        )
        .unwrap();
        let tasks = parse_tasks_from_project_md(temp.path());
        assert_eq!(tasks.len(), 1, "코드 블록 내 예시 항목은 파싱되지 않아야 합니다");
        assert_eq!(tasks[0].id, "M2-T01");
        assert_eq!(tasks[0].title, "실제 작업");
    }

    #[test]
    fn parse_tasks_code_block_toggle_multiple_fences() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join(".porpoise");
        std::fs::create_dir_all(&docs).unwrap();
        let project_md = docs.join("project.md");
        // Two code blocks, real tasks between and after
        std::fs::write(
            &project_md,
            "```\n- [ ] M1-T01: 블록1 내 예시\n```\n- [ ] M1-T01: 실제1\n```\n- [ ] M1-T02: 블록2 내 예시\n```\n- [ ] M1-T02: 실제2\n",
        )
        .unwrap();
        let tasks = parse_tasks_from_project_md(temp.path());
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "M1-T01");
        assert_eq!(tasks[1].id, "M1-T02");
    }

    #[test]
    fn parse_tasks_with_milestone_format() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join(".porpoise");
        std::fs::create_dir_all(&docs).unwrap();
        let project_md = docs.join("project.md");
        std::fs::write(
            &project_md,
            "## Milestone 1: 초기 구현\n- [ ] M1-T01: 파일 연산 정책\n- [x] M1-T02: 로깅 개선\n",
        )
        .unwrap();
        let tasks = parse_tasks_from_project_md(temp.path());
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "M1-T01");
        assert_eq!(tasks[0].title, "파일 연산 정책");
        assert!(!tasks[0].completed);
        assert_eq!(tasks[1].id, "M1-T02");
        assert!(tasks[1].completed);
    }
}
