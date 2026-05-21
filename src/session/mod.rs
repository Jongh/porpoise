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
use colored::Colorize;
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
    let latest = matching.into_iter().last()?;

    // Skip RESP and LIMIT sessions — they must not be reused as cache.
    // RESP: re-running should re-execute the role (user provided hints).
    // LIMIT: token quota may have reset; role should retry, not replay the limit message.
    if let Ok(content) = std::fs::read_to_string(&latest) {
        if let Ok(env) = serde_json::from_str::<crate::session::envelope::SessionEnvelope>(&content) {
            if let Some(ref output) = env.output {
                if matches!(output.status(), ExitCode::Resp | ExitCode::Limit) {
                    return None;
                }
            }
        }
    }

    Some(latest)
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

/// Returns true when the project uses JSON session mode.
///
/// Fresh projects created by `porpoise --new` always create `.porpoise/sessions`,
/// so the legacy report/message router is reserved for pre-session-directory
/// workspaces that must remain compatible with older Porpoise releases.
pub fn is_new_format(path: &Path) -> bool {
    path.join(".porpoise").join("sessions").is_dir()
}

/// Removes session JSON files based on workspace.toml [sessions] policy.
///
/// - `max_session_age_days = 0`: no deletion (unlimited retention).
/// - `max_session_age_days = N` (default 30): files older than N days are deleted.
/// - `keep_completed_milestone_sessions = false` (default): sessions belonging to
///   completed milestones are also deleted regardless of age.
pub fn cleanup_sessions(path: &Path, workspace: &crate::config::workspace::WorkspaceConfig) {
    let sessions_dir = path.join(".porpoise").join("sessions");
    if !sessions_dir.is_dir() {
        return;
    }

    let max_age_days = workspace.session_max_age_days();
    let keep_completed = workspace.session_keep_completed();

    // Parse completed task IDs from project.md
    let completed_task_ids: std::collections::HashSet<String> = if !keep_completed {
        let project_md = path.join(".porpoise").join("project.md");
        std::fs::read_to_string(&project_md)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("- [x]") {
                    // "- [x] M1-T01: title" → extract "M1-T01"
                    trimmed.strip_prefix("- [x]")
                        .and_then(|rest| rest.trim().split(':').next())
                        .map(|id| id.trim().to_string())
                } else {
                    None
                }
            })
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let now = std::time::SystemTime::now();
    let age_threshold = if max_age_days > 0 {
        Some(std::time::Duration::from_secs(max_age_days as u64 * 86400))
    } else {
        None
    };

    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut deleted = 0usize;
    for entry in entries.flatten() {
        let file_path = entry.path();
        if !file_path.is_file() { continue; }
        let name = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if !name.ends_with(".json") { continue; }

        // Extract task_id from filename: "M1-T01-planning-C1-R0.json"
        let task_id_in_file = name.split('-')
            .take(2)
            .collect::<Vec<_>>()
            .join("-");

        let should_delete_by_task = !keep_completed
            && completed_task_ids.contains(&task_id_in_file);

        let should_delete_by_age = age_threshold.map(|threshold| {
            entry.metadata()
                .and_then(|m| m.modified())
                .map(|mtime| now.duration_since(mtime).unwrap_or_default() > threshold)
                .unwrap_or(false)
        }).unwrap_or(false);

        if should_delete_by_task || should_delete_by_age {
            let _ = std::fs::remove_file(&file_path);
            deleted += 1;
        }
    }

    if deleted > 0 {
        println!(
            "  {} 세션 정리: {}개 파일 삭제 (workspace.toml [sessions] 정책 적용)",
            "ℹ".cyan(),
            deleted
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::planning::PlanningOutput;
    use crate::session::output::ExitCode;
    use crate::session::input::SessionInput;

    #[test]
    fn is_new_format_requires_sessions_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();

        assert!(!is_new_format(path));

        std::fs::create_dir_all(path.join(".porpoise").join("sessions")).unwrap();

        assert!(is_new_format(path));
    }

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

        let limit = ExitCode::Limit;
        assert_eq!(serde_json::to_string(&limit).unwrap(), "\"LIMIT\"");
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

    #[test]
    fn find_latest_session_skips_resp() {
        use crate::session::envelope::SessionEnvelope;
        use crate::session::planning::PlanningOutput;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let sessions_dir = path.join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Write a RESP session
        let envelope = SessionEnvelope {
            schema_version: "1".to_string(),
            task_id: "M1-T01".to_string(),
            role: "planning".to_string(),
            cycle: 1,
            retry: 0,
            timestamp: "2026-05-13T10:00:00Z".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            adapter: "claude_code".to_string(),
            input: SessionInput::default(),
            output: Some(RoleOutputData::Planning(PlanningOutput {
                role: "planning".to_string(),
                task_id: "M1-T01".to_string(),
                cycle: 1,
                status: ExitCode::Resp,
                summary: "needs clarification".to_string(),
                questions: vec![],
                prev_reason: None,
                implementation_plan: vec![],
                dod_checklist: vec![],
                risks: vec![],
            })),
            raw_text: None,
        };
        let name = session_filename("M1-T01", "planning", 1, 0);
        let content = serde_json::to_string(&envelope).unwrap();
        std::fs::write(sessions_dir.join(&name), &content).unwrap();

        // RESP session should be skipped
        let result = find_latest_session(path, "M1-T01", "planning");
        assert!(result.is_none(), "RESP 세션은 캐시로 재사용되어서는 안 됩니다");
    }

    #[test]
    fn find_latest_session_skips_limit() {
        use crate::session::envelope::SessionEnvelope;
        use crate::session::planning::PlanningOutput;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let sessions_dir = path.join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let envelope = SessionEnvelope {
            schema_version: "1".to_string(),
            task_id: "M1-T01".to_string(),
            role: "planning".to_string(),
            cycle: 1,
            retry: 0,
            timestamp: "2026-05-13T10:00:00Z".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            adapter: "claude_code".to_string(),
            input: SessionInput::default(),
            output: Some(RoleOutputData::Planning(PlanningOutput {
                role: "planning".to_string(),
                task_id: "M1-T01".to_string(),
                cycle: 1,
                status: ExitCode::Limit,
                summary: "token limit hit".to_string(),
                questions: vec![],
                prev_reason: None,
                implementation_plan: vec![],
                dod_checklist: vec![],
                risks: vec![],
            })),
            raw_text: None,
        };
        let name = session_filename("M1-T01", "planning", 1, 0);
        std::fs::write(sessions_dir.join(&name), serde_json::to_string(&envelope).unwrap()).unwrap();

        // LIMIT 세션은 캐시로 재사용되어서는 안 됨 — 한도 해제 후 재실행 가능해야 함
        assert!(
            find_latest_session(path, "M1-T01", "planning").is_none(),
            "LIMIT 세션은 캐시로 재사용되어서는 안 됩니다"
        );
    }

    #[test]
    fn find_latest_session_returns_next_session() {
        use crate::session::envelope::SessionEnvelope;
        use crate::session::planning::PlanningOutput;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let sessions_dir = path.join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Write a NEXT session
        let envelope = SessionEnvelope {
            schema_version: "1".to_string(),
            task_id: "M1-T01".to_string(),
            role: "planning".to_string(),
            cycle: 1,
            retry: 0,
            timestamp: "2026-05-13T10:00:00Z".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            adapter: "claude_code".to_string(),
            input: SessionInput::default(),
            output: Some(RoleOutputData::Planning(PlanningOutput {
                role: "planning".to_string(),
                task_id: "M1-T01".to_string(),
                cycle: 1,
                status: ExitCode::Next,
                summary: "plan complete".to_string(),
                questions: vec![],
                prev_reason: None,
                implementation_plan: vec![],
                dod_checklist: vec![],
                risks: vec![],
            })),
            raw_text: None,
        };
        let name = session_filename("M1-T01", "planning", 1, 0);
        let content = serde_json::to_string(&envelope).unwrap();
        std::fs::write(sessions_dir.join(&name), &content).unwrap();

        // NEXT session should be returned
        let result = find_latest_session(path, "M1-T01", "planning");
        assert!(result.is_some(), "NEXT 세션은 캐시로 반환되어야 합니다");
    }

    #[test]
    fn cleanup_sessions_removes_completed_task_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let sessions_dir = path.join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // project.md에 M1-T01 완료, M1-T02 미완료
        let project_md = path.join(".porpoise").join("project.md");
        std::fs::write(&project_md, "- [x] M1-T01: 완료된 태스크\n- [ ] M1-T02: 진행 중 태스크\n").unwrap();

        std::fs::write(sessions_dir.join("M1-T01-planning-C1-R0.json"), "{}").unwrap();
        std::fs::write(sessions_dir.join("M1-T02-planning-C1-R0.json"), "{}").unwrap();

        let workspace = crate::config::workspace::WorkspaceConfig::default();
        // 기본값: keep_completed=false, max_age_days=30
        // age 기반 삭제는 발생 안 하지만 completed 삭제는 발생
        cleanup_sessions(path, &workspace);

        // M1-T01 세션은 삭제되어야 함
        assert!(!sessions_dir.join("M1-T01-planning-C1-R0.json").exists(), "완료 태스크 세션이 삭제되지 않음");
        // M1-T02 세션은 보존되어야 함
        assert!(sessions_dir.join("M1-T02-planning-C1-R0.json").exists(), "미완료 태스크 세션이 삭제됨");
    }

    #[test]
    fn cleanup_sessions_keeps_when_keep_completed_true() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let sessions_dir = path.join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let project_md = path.join(".porpoise").join("project.md");
        std::fs::write(&project_md, "- [x] M1-T01: 완료된 태스크\n").unwrap();
        std::fs::write(sessions_dir.join("M1-T01-planning-C1-R0.json"), "{}").unwrap();

        let workspace = crate::config::workspace::WorkspaceConfig {
            sessions: Some(crate::config::workspace::WorkspaceSessions {
                keep_completed_milestone_sessions: Some(true),
                max_session_age_days: Some(0), // 나이 기반 삭제 비활성
            }),
            ..Default::default()
        };
        cleanup_sessions(path, &workspace);

        // keep_completed=true이므로 세션 보존
        assert!(sessions_dir.join("M1-T01-planning-C1-R0.json").exists(), "keep_completed=true 시 세션이 삭제됨");
    }

    #[test]
    fn cleanup_sessions_skips_deletion_when_max_age_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let sessions_dir = path.join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // project.md 없음 → completed_task_ids 비어있음
        std::fs::write(sessions_dir.join("M2-T01-planning-C1-R0.json"), "{}").unwrap();

        let workspace = crate::config::workspace::WorkspaceConfig {
            sessions: Some(crate::config::workspace::WorkspaceSessions {
                keep_completed_milestone_sessions: Some(true),
                max_session_age_days: Some(0), // 0 = 무제한, 삭제 안 함
            }),
            ..Default::default()
        };
        cleanup_sessions(path, &workspace);

        // max_age=0이므로 나이 기반 삭제 없음, completed도 keep=true이므로 삭제 없음
        assert!(sessions_dir.join("M2-T01-planning-C1-R0.json").exists(), "max_age_days=0 설정 시 세션이 삭제됨");
    }
}
