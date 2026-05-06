use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};

use super::report::{parse_report, report_filename, Report};
use super::state::{Role, TaskId};
use crate::claude::runner::ClaudeRunner;

#[derive(Debug, Clone)]
pub struct RoleContext {
    pub previous_reports: Vec<PathBuf>,
    pub project_docs: Vec<PathBuf>,
}

impl RoleContext {
    pub fn new() -> Self {
        RoleContext {
            previous_reports: vec![],
            project_docs: vec![],
        }
    }

    pub fn with_previous_report(mut self, path: PathBuf) -> Self {
        self.previous_reports.push(path);
        self
    }

    pub fn with_project_doc(mut self, path: PathBuf) -> Self {
        self.project_docs.push(path);
        self
    }
}

pub struct RoleExecutor {
    runner: Option<ClaudeRunner>,
    model: Option<String>,
}

impl RoleExecutor {
    pub fn new(model: Option<String>) -> Self {
        let runner = ClaudeRunner::new().ok();
        RoleExecutor { runner, model }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_role(
        &self,
        role: &Role,
        context: &RoleContext,
        path: &Path,
        dry_run: bool,
        task_id: &str,
        cycle: u32,
        retry: u32,
    ) -> Result<Report> {
        let prompt_file = path
            .join(".porpoise")
            .join("prompts")
            .join(role.prompt_file());

        if dry_run {
            println!(
                "  {} Would execute role: {}",
                "[DRY RUN]".yellow(),
                role.display_name().cyan()
            );
            println!(
                "  {} Prompt file: {}",
                "[DRY RUN]".yellow(),
                prompt_file.display()
            );
            println!(
                "  {} Context files: {}",
                "[DRY RUN]".yellow(),
                context.previous_reports.len()
            );
            return Ok(Report::stub(&role.to_string()));
        }

        let mut context_files: Vec<PathBuf> = Vec::new();

        for doc in &context.project_docs {
            if doc.exists() {
                context_files.push(doc.clone());
            }
        }

        for report in &context.previous_reports {
            if report.exists() {
                context_files.push(report.clone());
            }
        }

        let output_filename = report_filename(task_id, &role.to_string(), cycle, retry);
        let output_file = path
            .join(".porpoise")
            .join("reports")
            .join(&output_filename);

        let runner = match &self.runner {
            Some(r) => r,
            None => {
                anyhow::bail!(
                    "Claude CLI not found. Please install Claude Code and ensure 'claude' is in your PATH."
                );
            }
        };

        println!(
            "  {} {}",
            "Executing:".cyan(),
            role.display_name().bold()
        );

        let output = runner.run_with_prompt(&prompt_file, &context_files, &output_file, self.model.as_deref())?;
        let report = parse_report(&output, &role.to_string());

        Ok(report)
    }
}

impl Default for RoleExecutor {
    fn default() -> Self {
        Self::new(None)
    }
}

/// Build context for a role by collecting relevant previous reports and project docs.
/// Supports both new ({task_id}-{role}-C{n}-R{n}.md) and old ({ts}-{role}-report.md) formats.
pub fn build_context(role: &Role, cycle: u32, path: &Path, task_id: &str) -> RoleContext {
    let mut ctx = RoleContext::new();

    let project_md = path.join(".porpoise").join("project.md");
    if project_md.exists() {
        ctx = ctx.with_project_doc(project_md);
    }

    let claude_md = path.join("CLAUDE.md");
    if claude_md.exists() {
        ctx = ctx.with_project_doc(claude_md);
    }

    let reports_dir = path.join(".porpoise").join("reports");
    if !reports_dir.exists() {
        return ctx;
    }

    let predecessor_roles: Vec<&str> = match role {
        // PM in subsequent cycles gets previous cycle's reports for context
        Role::PM if cycle > 1 => vec!["review", "testing", "development"],
        Role::PM => vec![],
        Role::Developer => vec!["planning"],
        Role::Tester => vec!["planning", "development"],
        Role::Reviewer => vec!["planning", "development", "testing"],
    };

    for prev_role in &predecessor_roles {
        if let Some(latest) = find_latest_report(&reports_dir, prev_role, task_id) {
            ctx = ctx.with_previous_report(latest);
        }
    }

    // For PM: include user-provided additional instructions from PREV cycles
    if matches!(role, Role::PM) {
        for f in find_prev_additional_files(&reports_dir, task_id) {
            ctx = ctx.with_project_doc(f);
        }
    }

    // Include hint files for the current role (AI questions from prior RESP rounds)
    let hints_dir = path.join(".porpoise").join("hints");
    let hint_files = find_hint_files(&hints_dir, task_id, &role.to_string());
    for hint_file in hint_files {
        ctx = ctx.with_project_doc(hint_file);
    }

    ctx
}

/// Find the latest report for a given role and task_id (new format preferred, old format fallback).
pub(super) fn find_latest_report(reports_dir: &Path, role: &str, task_id: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(reports_dir).ok()?;

    let mut matching: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if filename_matches_role(&name, role, task_id) && !name.contains("-resp") {
                Some(e.path())
            } else {
                None
            }
        })
        .collect();

    matching.sort();
    matching.into_iter().last()
}

fn filename_matches_role(name: &str, role: &str, task_id: &str) -> bool {
    let normalized = TaskId::new(task_id);
    // New format: {task_id}-{role}-C{n}-R{n}.md
    let new_pat = format!("-{}-C", role);
    let matches_new = name.starts_with(&format!("{}-", normalized)) && name.contains(&new_pat);
    // Old format: {timestamp}-{role}-report.md (backward compat)
    let matches_old = name.contains(&format!("-{}-report.md", role));
    matches_new || matches_old
}

/// Find user-provided additional instruction files saved during PREV cycles.
fn find_prev_additional_files(reports_dir: &Path, task_id: &str) -> Vec<PathBuf> {
    let normalized = TaskId::new(task_id);
    let prefix = format!("{}-prev-additional-", normalized);
    if let Ok(entries) = std::fs::read_dir(reports_dir) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) && name.ends_with(".md") {
                    Some(e.path())
                } else {
                    None
                }
            })
            .collect();
        files.sort();
        files
    } else {
        vec![]
    }
}

/// Find hint files for the current role and task (sorted by name).
fn find_hint_files(hints_dir: &Path, task_id: &str, role: &str) -> Vec<PathBuf> {
    let normalized = TaskId::new(task_id);
    let prefix = format!("{}-{}-", normalized, role);
    if let Ok(entries) = std::fs::read_dir(hints_dir) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) && name.contains("-hints") && name.ends_with(".md") {
                    Some(e.path())
                } else {
                    None
                }
            })
            .collect();
        files.sort();
        files
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_matches_role_normalizes_task_id() {
        assert!(filename_matches_role("M1-T01-planning-C1-R0.md", "planning", "M1-T1"));
        assert!(filename_matches_role("M2-T09-testing-C1-R0.md", "testing", "M2-T9"));
        assert!(filename_matches_role("M2-T10-review-C1-R0.md", "review", "M2-T10"));
        assert!(!filename_matches_role("M1-T01-planning-C1-R0.md", "development", "M1-T1"));
        assert!(!filename_matches_role("M1-T02-planning-C1-R0.md", "planning", "M1-T1"));
        // old-name files still matchable for backward compat
        assert!(filename_matches_role("M1-T01-pm-C1-R0.md", "pm", "M1-T1"));
    }

    #[test]
    fn find_latest_report_normalizes_task_id() {
        let temp = tempfile::tempdir().unwrap();
        let reports_dir = temp.path();
        std::fs::write(reports_dir.join("M2-T09-planning-C1-R0.md"), "content").unwrap();
        // 패딩 없는 "M2-T9"로도 찾아야 함
        assert!(find_latest_report(reports_dir, "planning", "M2-T9").is_some());
        assert!(find_latest_report(reports_dir, "planning", "M2-T09").is_some());
        assert!(find_latest_report(reports_dir, "development", "M2-T9").is_none());
    }
}
