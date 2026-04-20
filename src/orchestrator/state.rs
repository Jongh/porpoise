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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorState {
    pub cycle: u32,
    pub current_role: Option<Role>,
    pub completed_roles: Vec<Role>,
    pub project_path: PathBuf,
}

impl OrchestratorState {
    pub fn new(path: &Path) -> Self {
        OrchestratorState {
            cycle: 1,
            current_role: Some(Role::PM),
            completed_roles: vec![],
            project_path: path.to_path_buf(),
        }
    }
}

pub fn load_state(path: &Path) -> Result<OrchestratorState> {
    let reports_dir = path.join(".docs").join("reports");

    if !reports_dir.exists() {
        return Ok(OrchestratorState::new(path));
    }

    // Scan reports directory for existing reports
    let mut report_files: Vec<String> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&reports_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with("-report.md") && name != "checkpoint.md" {
                report_files.push(name);
            }
        }
    }

    report_files.sort(); // Sort by timestamp prefix

    if report_files.is_empty() {
        return Ok(OrchestratorState::new(path));
    }

    // Infer cycle from report filenames
    let mut max_cycle: u32 = 1;
    let mut completed_roles: Vec<(u32, Role)> = Vec::new();

    for filename in &report_files {
        // Expected format: YYYYMMDD-HHMMSS-{role}-report.md
        let parts: Vec<&str> = filename.splitn(4, '-').collect();
        if parts.len() >= 3 {
            // The role is the third part (index 2), but we need to handle
            // filenames like "20240101-120000-pm-report.md"
            // parts[0] = date, parts[1] = time prefix, parts[2] = role part
            // Actually split differently: "20240101-120000-pm-report.md"
            // Let's find the role by checking known role names
            let role_opt = extract_role_from_filename(filename);
            if let Some(role) = role_opt {
                completed_roles.push((1, role)); // Cycle tracking simplified
            }
        }
    }

    // Try to load checkpoint for more accurate state
    let checkpoint_path = reports_dir.join("checkpoint.md");
    if checkpoint_path.exists() {
        if let Ok(checkpoint) = super::checkpoint::load_checkpoint(path) {
            max_cycle = checkpoint.cycle;
            let completed = checkpoint
                .completed_roles
                .iter()
                .filter_map(|r| Role::from_str(r))
                .collect::<Vec<_>>();

            let next_role = Role::from_str(&checkpoint.next_role);

            return Ok(OrchestratorState {
                cycle: max_cycle,
                current_role: next_role,
                completed_roles: completed,
                project_path: path.to_path_buf(),
            });
        }
    }

    // Infer state from completed roles
    let all_roles = Role::all();
    let completed_role_list: Vec<Role> = completed_roles.into_iter().map(|(_, r)| r).collect();

    // Find next role to execute
    let next_role = all_roles
        .iter()
        .find(|r| !completed_role_list.contains(r))
        .cloned();

    // If all roles completed, start new cycle
    let (cycle, current_role, completed) = if next_role.is_none() {
        (max_cycle + 1, Some(Role::PM), vec![])
    } else {
        (max_cycle, next_role, completed_role_list)
    };

    Ok(OrchestratorState {
        cycle,
        current_role,
        completed_roles: completed,
        project_path: path.to_path_buf(),
    })
}

fn extract_role_from_filename(filename: &str) -> Option<Role> {
    // filename format: YYYYMMDD-HHMMSS-{role}-report.md
    let without_ext = filename.strip_suffix(".md")?;
    let without_report = without_ext.strip_suffix("-report")?;

    // Find the role part - it's after the second dash from the start
    // Format: date(8)-time(6)-role
    // "20240101-120000-pm" -> split by '-' gives ["20240101", "120000", "pm"]
    let parts: Vec<&str> = without_report.splitn(3, '-').collect();
    if parts.len() >= 3 {
        return Role::from_str(parts[2]);
    }
    None
}
