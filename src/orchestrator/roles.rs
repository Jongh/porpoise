use std::path::{Path, PathBuf};

use super::state::TaskId;

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
        assert!(find_latest_report(reports_dir, "planning", "M2-T9").is_some());
        assert!(find_latest_report(reports_dir, "planning", "M2-T09").is_some());
        assert!(find_latest_report(reports_dir, "development", "M2-T9").is_none());
    }
}
