use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};

use super::report::{parse_report, report_filename, Report};
use super::state::Role;
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
}

impl RoleExecutor {
    pub fn new() -> Self {
        let runner = ClaudeRunner::new().ok();
        RoleExecutor { runner }
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
            .join(".docs")
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
            .join(".docs")
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

        let output = runner.run_with_prompt(&prompt_file, &context_files, &output_file)?;
        let report = parse_report(&output, &role.to_string());

        Ok(report)
    }
}

impl Default for RoleExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Build context for a role by collecting relevant previous reports and project docs.
/// Supports both new ({task_id}-{role}-C{n}-R{n}.md) and old ({ts}-{role}-report.md) formats.
pub fn build_context(role: &Role, _cycle: u32, path: &Path, task_id: &str) -> RoleContext {
    let mut ctx = RoleContext::new();

    let project_md = path.join(".docs").join("project.md");
    if project_md.exists() {
        ctx = ctx.with_project_doc(project_md);
    }

    let claude_md = path.join("claude.md");
    if claude_md.exists() {
        ctx = ctx.with_project_doc(claude_md);
    }

    let reports_dir = path.join(".docs").join("reports");
    if !reports_dir.exists() {
        return ctx;
    }

    let predecessor_roles: Vec<&str> = match role {
        Role::PM => vec![],
        Role::Developer => vec!["pm"],
        Role::Tester => vec!["pm", "developer"],
        Role::Reviewer => vec!["pm", "developer", "tester"],
    };

    for prev_role in &predecessor_roles {
        if let Some(latest) = find_latest_report(&reports_dir, prev_role, task_id) {
            ctx = ctx.with_previous_report(latest);
        }
    }

    // Include RESP answer files for the current role (user answers from prior RESP rounds)
    let resp_files = find_resp_files(&reports_dir, task_id, &role.to_string());
    for resp_file in resp_files {
        ctx = ctx.with_project_doc(resp_file);
    }

    ctx
}

/// Find the latest report for a given role and task_id (new format preferred, old format fallback).
fn find_latest_report(reports_dir: &Path, role: &str, task_id: &str) -> Option<PathBuf> {
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
    // New format: {task_id}-{role}-C{n}-R{n}.md
    let new_pat = format!("-{}-C", role);
    let matches_new = name.starts_with(&format!("{}-", task_id)) && name.contains(&new_pat);
    // Old format: {timestamp}-{role}-report.md (backward compat)
    let matches_old = name.contains(&format!("-{}-report.md", role));
    matches_new || matches_old
}

/// Find RESP answer files for the current role and task (sorted by name).
fn find_resp_files(reports_dir: &Path, task_id: &str, role: &str) -> Vec<PathBuf> {
    let prefix = format!("{}-{}-", task_id, role);
    if let Ok(entries) = std::fs::read_dir(reports_dir) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) && name.contains("-resp") && name.ends_with(".md") {
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
