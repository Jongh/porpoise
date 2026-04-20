use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};

use super::report::{parse_report, Report};
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

    pub fn execute_role(
        &self,
        role: &Role,
        context: &RoleContext,
        path: &Path,
        dry_run: bool,
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

        // Build context files list
        let mut context_files: Vec<PathBuf> = Vec::new();

        // Add project docs
        for doc in &context.project_docs {
            if doc.exists() {
                context_files.push(doc.clone());
            }
        }

        // Add previous reports
        for report in &context.previous_reports {
            if report.exists() {
                context_files.push(report.clone());
            }
        }

        // Output file for this role
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let output_filename = format!("{}-{}-report.md", timestamp, role);
        let output_file = path
            .join(".docs")
            .join("reports")
            .join(&output_filename);

        // Check if runner is available
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

/// Build context for a given role by collecting relevant previous reports
pub fn build_context(role: &Role, _cycle: u32, path: &Path) -> RoleContext {
    let mut ctx = RoleContext::new();

    // Add project docs
    let project_md = path.join(".docs").join("project.md");
    if project_md.exists() {
        ctx = ctx.with_project_doc(project_md);
    }

    let claude_md = path.join("claude.md");
    if claude_md.exists() {
        ctx = ctx.with_project_doc(claude_md);
    }

    // Add previous role reports as context
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

    for prev_role in predecessor_roles {
        if let Ok(entries) = std::fs::read_dir(&reports_dir) {
            let mut matching: Vec<PathBuf> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.contains(&format!("-{}-report.md", prev_role)) {
                        Some(e.path())
                    } else {
                        None
                    }
                })
                .collect();

            matching.sort();
            if let Some(latest) = matching.into_iter().last() {
                ctx = ctx.with_previous_report(latest);
            }
        }
    }

    ctx
}
