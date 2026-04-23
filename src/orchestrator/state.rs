use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

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
            Role::PM => write!(f, "pm"),
            Role::Developer => write!(f, "developer"),
            Role::Tester => write!(f, "tester"),
            Role::Reviewer => write!(f, "reviewer"),
        }
    }
}

impl Role {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pm" => Some(Role::PM),
            "developer" | "dev" => Some(Role::Developer),
            "tester" | "test" => Some(Role::Tester),
            "reviewer" | "review" => Some(Role::Reviewer),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Role::PM => "PM (Product Manager)",
            Role::Developer => "Developer",
            Role::Tester => "Tester",
            Role::Reviewer => "Reviewer",
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

    pub fn prev(&self) -> Option<Role> {
        match self {
            Role::PM => None,
            Role::Developer => Some(Role::PM),
            Role::Tester => Some(Role::Developer),
            Role::Reviewer => Some(Role::Tester),
        }
    }

    pub fn prompt_file(&self) -> &'static str {
        match self {
            Role::PM => "01-pm.md",
            Role::Developer => "02-developer.md",
            Role::Tester => "03-tester.md",
            Role::Reviewer => "04-reviewer.md",
        }
    }

    pub fn all() -> Vec<Role> {
        vec![Role::PM, Role::Developer, Role::Tester, Role::Reviewer]
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub completed: bool,
}

/// Parses M{n}-T{nn} task items from .docs/project.md.
/// Returns empty vec if project.md is absent or has no M-T formatted tasks.
pub fn parse_tasks_from_project_md(path: &Path) -> Vec<Task> {
    let project_md = path.join(".docs").join("project.md");
    let content = match std::fs::read_to_string(&project_md) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut tasks = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [ ] ") || trimmed.starts_with("- [x] ") {
            let completed = trimmed.starts_with("- [x] ");
            let rest = &trimmed[6..]; // skip "- [ ] " or "- [x] "

            if let Some(colon_pos) = rest.find(": ") {
                let id_part = rest[..colon_pos].trim();
                let title = rest[colon_pos + 2..].trim();
                // Only accept M{n}-T{nn} format
                if id_part.starts_with('M') && id_part.contains("-T") {
                    tasks.push(Task {
                        id: id_part.to_string(),
                        title: title.to_string(),
                        completed,
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
        }
    }
}

pub fn load_state(path: &Path) -> Result<OrchestratorState> {
    let reports_dir = path.join(".docs").join("reports");

    if !reports_dir.exists() {
        return Ok(build_state_with_tasks(OrchestratorState::new(path), path));
    }

    let checkpoint_path = reports_dir.join("checkpoint.md");
    if checkpoint_path.exists() {
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
            });
        }
    }

    // Fallback: infer state from report filenames
    let mut report_files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&reports_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if (name.ends_with("-report.md") || name.ends_with(".md"))
                && name != "checkpoint.md"
            {
                report_files.push(name);
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
    fn role_from_str_all_variants() {
        assert_eq!(Role::from_str("pm"), Some(Role::PM));
        assert_eq!(Role::from_str("PM"), Some(Role::PM));
        assert_eq!(Role::from_str("developer"), Some(Role::Developer));
        assert_eq!(Role::from_str("dev"), Some(Role::Developer));
        assert_eq!(Role::from_str("tester"), Some(Role::Tester));
        assert_eq!(Role::from_str("test"), Some(Role::Tester));
        assert_eq!(Role::from_str("reviewer"), Some(Role::Reviewer));
        assert_eq!(Role::from_str("review"), Some(Role::Reviewer));
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
    fn role_prev_sequence() {
        assert_eq!(Role::PM.prev(), None);
        assert_eq!(Role::Developer.prev(), Some(Role::PM));
        assert_eq!(Role::Tester.prev(), Some(Role::Developer));
        assert_eq!(Role::Reviewer.prev(), Some(Role::Tester));
    }

    #[test]
    fn role_display() {
        assert_eq!(Role::PM.to_string(), "pm");
        assert_eq!(Role::Developer.to_string(), "developer");
        assert_eq!(Role::Tester.to_string(), "tester");
        assert_eq!(Role::Reviewer.to_string(), "reviewer");
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
        let docs = temp.path().join(".docs");
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
    fn parse_tasks_with_milestone_format() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join(".docs");
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
