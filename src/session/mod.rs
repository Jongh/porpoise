pub mod envelope;
pub mod input;
pub mod milestone;
pub mod output;
pub mod planning;
pub mod development;
pub mod testing;
pub mod review;
pub mod renderer;
pub mod v0_7;

pub use envelope::SessionEnvelope;
pub use input::SessionInput;
pub use output::{ExitCode, RoleOutputData};

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::orchestrator::state::TaskId;

pub fn session_filename(task_id: &str, role: &str, cycle: u32, retry: u32) -> String {
    let normalized = TaskId::new(task_id);
    format!("{}-{}-C{}-R{}.json", normalized, role, cycle, retry)
}

pub fn save_session(path: &Path, envelope: &SessionEnvelope) -> Result<()> {
    let sessions_dir = path.join(".porpoise").join("sessions");
    std::fs::create_dir_all(&sessions_dir).context("sessions/ 디렉토리 생성 실패")?;
    let filename = session_filename(&envelope.task_id, &envelope.role, envelope.cycle, envelope.retry);
    let file_path = sessions_dir.join(&filename);
    let content = serde_json::to_string_pretty(envelope)?;
    std::fs::write(&file_path, content)?;
    Ok(())
}

#[allow(dead_code)]
pub fn load_session(path: &Path, task_id: &str, role: &str, cycle: u32, retry: u32) -> Result<SessionEnvelope> {
    let sessions_dir = path.join(".porpoise").join("sessions");
    let filename = session_filename(task_id, role, cycle, retry);
    let file_path = sessions_dir.join(&filename);
    let content = std::fs::read_to_string(&file_path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn find_latest_session(path: &Path, task_id: &str, role: &str) -> Option<PathBuf> {
    let sessions_dir = path.join(".porpoise").join("sessions");
    let normalized = TaskId::new(task_id).to_string();
    let prefix = format!("{}-{}-C", normalized, role);

    let entries = std::fs::read_dir(&sessions_dir).ok()?;
    let mut matching: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) && name.ends_with(".json") {
                Some(e.path())
            } else {
                None
            }
        })
        .collect();
    matching.sort();
    matching.into_iter().last()
}

pub fn count_existing_sessions(path: &Path, task_id: &str, role: &str, cycle: u32) -> u32 {
    let sessions_dir = path.join(".porpoise").join("sessions");
    let normalized = TaskId::new(task_id).to_string();
    let prefix = format!("{}-{}-C{}-R", normalized, role, cycle);
    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        entries
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(&prefix) && name.ends_with(".json")
            })
            .count() as u32
    } else {
        0
    }
}

pub fn is_new_format(path: &Path) -> bool {
    path.join(".porpoise").join("sessions").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::planning::PlanningOutput;
    use crate::session::output::ExitCode;
    use crate::session::input::SessionInput;

    #[test]
    fn session_filename_format() {
        assert_eq!(session_filename("M1-T01", "planning", 1, 0), "M1-T01-planning-C1-R0.json");
        assert_eq!(session_filename("M1-T1", "planning", 2, 1), "M1-T01-planning-C2-R1.json");
    }

    #[test]
    fn exit_code_serde_roundtrip() {
        let code = ExitCode::Next;
        let json = serde_json::to_string(&code).unwrap();
        assert_eq!(json, "\"NEXT\"");
        let back: ExitCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ExitCode::Next);

        let prev = ExitCode::Prev;
        assert_eq!(serde_json::to_string(&prev).unwrap(), "\"PREV\"");

        let resp = ExitCode::Resp;
        assert_eq!(serde_json::to_string(&resp).unwrap(), "\"RESP\"");
    }

    #[test]
    fn save_and_load_session_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();

        let envelope = SessionEnvelope {
            schema_version: "1".to_string(),
            task_id: "M1-T01".to_string(),
            role: "planning".to_string(),
            cycle: 1,
            retry: 0,
            timestamp: "2026-05-08T10:00:00Z".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            adapter: "claude_code".to_string(),
            input: SessionInput::default(),
            output: Some(RoleOutputData::Planning(PlanningOutput {
                role: "planning".to_string(),
                task_id: "M1-T01".to_string(),
                cycle: 1,
                status: ExitCode::Next,
                summary: "test".to_string(),
                questions: vec![],
                prev_reason: None,
                implementation_plan: vec![],
                dod_checklist: vec![],
                risks: vec![],
            })),
            raw_text: None,
        };

        save_session(path, &envelope).unwrap();
        let loaded = load_session(path, "M1-T01", "planning", 1, 0).unwrap();
        assert_eq!(loaded.task_id, "M1-T01");
        assert_eq!(loaded.role, "planning");
        assert_eq!(loaded.schema_version, "1");
    }

    #[test]
    fn find_latest_session_picks_highest() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Create fake session files
        for (cycle, retry) in [(1,0),(2,0),(2,1)] {
            let name = session_filename("M1-T01", "planning", cycle, retry);
            std::fs::write(sessions_dir.join(&name), "{}").unwrap();
        }

        let latest = find_latest_session(tmp.path(), "M1-T01", "planning").unwrap();
        let name = latest.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, "M1-T01-planning-C2-R1.json");
    }

    #[test]
    fn is_new_format_false_without_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise")).unwrap();
        assert!(!is_new_format(tmp.path()));
    }

    #[test]
    fn is_new_format_true_with_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise").join("sessions")).unwrap();
        assert!(is_new_format(tmp.path()));
    }
}
