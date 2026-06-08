use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::utils::fs::write_file;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub timestamp: String,
    pub cycle: u32,
    pub current_role: String,
    pub completed_roles: Vec<String>,
    pub next_role: String,
    pub pending_tasks: Vec<String>,
    pub current_task_id: String,
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prev_reasons: Vec<String>,
    /// 지휘자(conductor) 루프의 현재 단계: "brief" | "dispatch" | "verify" | "integrate".
    /// 레거시 phase 경로에서는 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conductor_phase: Option<String>,
}

impl Checkpoint {
    // 체크포인트의 모든 필드를 받는 생성자 — 인자 수가 많지만 단순 값 객체라 빌더 도입은 과함.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cycle: u32,
        current_role: &str,
        completed_roles: Vec<String>,
        next_role: &str,
        pending_tasks: Vec<String>,
        current_task_id: &str,
        retry_count: u32,
        prev_reasons: Vec<String>,
    ) -> Self {
        Checkpoint {
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            cycle,
            current_role: current_role.to_string(),
            completed_roles,
            next_role: next_role.to_string(),
            pending_tasks,
            current_task_id: current_task_id.to_string(),
            retry_count,
            prev_reasons,
            conductor_phase: None,
        }
    }

    /// 지휘자 단계를 표시한 사본을 반환한다.
    pub fn with_conductor_phase(mut self, phase: &str) -> Self {
        self.conductor_phase = Some(phase.to_string());
        self
    }
}

pub fn save_checkpoint(checkpoint: &Checkpoint, path: &Path) -> Result<()> {
    let checkpoint_path = path.join(".porpoise").join("checkpoint.json");

    let content = serde_json::to_string_pretty(checkpoint)
        .context("checkpoint JSON 직렬화 실패")?;

    write_file(&checkpoint_path, &content, path)?;

    Ok(())
}

pub fn load_checkpoint(path: &Path) -> Result<Checkpoint> {
    // Try new path first: .porpoise/checkpoint.json
    let new_path = path.join(".porpoise").join("checkpoint.json");
    if new_path.exists() {
        let content = fs::read_to_string(&new_path)
            .with_context(|| format!("checkpoint.json 읽기 실패: {}", new_path.display()))?;
        return serde_json::from_str(&content).context("checkpoint.json 파싱 실패");
    }

    // Migration: try old messages/ paths
    let old_json = path.join(".porpoise").join("messages").join("checkpoint.json");
    if old_json.exists() {
        let content = fs::read_to_string(&old_json)
            .with_context(|| format!("checkpoint.json 읽기 실패: {}", old_json.display()))?;
        return serde_json::from_str(&content).context("checkpoint.json 파싱 실패");
    }

    let old_md = path.join(".porpoise").join("messages").join("checkpoint.md");
    if old_md.exists() {
        let content = fs::read_to_string(&old_md)
            .with_context(|| format!("checkpoint.md 읽기 실패: {}", old_md.display()))?;
        return parse_checkpoint(&content);
    }

    anyhow::bail!("checkpoint 파일을 찾을 수 없습니다.")
}

pub(crate) fn parse_checkpoint(content: &str) -> Result<Checkpoint> {
    let mut timestamp = String::new();
    let mut cycle = 1u32;
    let mut current_role = String::new();
    let mut next_role = String::new();
    let mut completed_roles: Vec<String> = Vec::new();
    let mut pending_tasks: Vec<String> = Vec::new();
    let mut current_task_id = String::new();
    let mut retry_count = 0u32;
    let mut prev_reasons: Vec<String> = Vec::new();

    let mut in_completed = false;
    let mut in_pending = false;
    let mut in_prev_reasons = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## Completed Roles") {
            in_completed = true;
            in_pending = false;
            in_prev_reasons = false;
            continue;
        }
        if trimmed.starts_with("## Pending Tasks") {
            in_pending = true;
            in_completed = false;
            in_prev_reasons = false;
            continue;
        }
        if trimmed.starts_with("## Prev Reasons") {
            in_prev_reasons = true;
            in_completed = false;
            in_pending = false;
            continue;
        }
        if trimmed.starts_with("## ") {
            in_completed = false;
            in_pending = false;
            in_prev_reasons = false;
        }

        if in_completed && trimmed.starts_with("- ") {
            let role = trimmed.trim_start_matches("- ").trim().to_string();
            if role != "(none)" {
                completed_roles.push(role);
            }
            continue;
        }

        if in_pending && trimmed.starts_with("- ") {
            let task = trimmed.trim_start_matches("- ").trim().to_string();
            if task != "(none)" {
                pending_tasks.push(task);
            }
            continue;
        }

        if in_prev_reasons && trimmed.starts_with("- ") {
            let reason = trimmed.trim_start_matches("- ").trim().to_string();
            if reason != "(none)" {
                prev_reasons.push(reason);
            }
            continue;
        }

        if let Some(val) = trimmed.strip_prefix("- timestamp: ") {
            timestamp = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("- cycle: ") {
            cycle = val.trim().parse().unwrap_or(1);
        } else if let Some(val) = trimmed.strip_prefix("- current_role: ") {
            current_role = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("- next_role: ") {
            next_role = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("- current_task_id: ") {
            current_task_id = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("- retry_count: ") {
            retry_count = val.trim().parse().unwrap_or(0);
        }
    }

    Ok(Checkpoint {
        timestamp,
        cycle,
        current_role,
        completed_roles,
        next_role,
        pending_tasks,
        current_task_id,
        retry_count,
        prev_reasons,
        conductor_phase: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_checkpoint_md() -> &'static str {
        r#"# Porpoise Checkpoint

<!-- This file is auto-generated by Porpoise. Do not edit manually. -->

## Metadata
- timestamp: 2026-04-21 10:00:00
- cycle: 3
- current_role: tester
- next_role: reviewer
- current_task_id: M1-T01
- retry_count: 2

## Completed Roles
- pm
- developer

## Pending Tasks
- (none)
"#
    }

    #[test]
    fn parse_checkpoint_normal() {
        let cp = parse_checkpoint(sample_checkpoint_md()).unwrap();
        assert_eq!(cp.cycle, 3);
        assert_eq!(cp.current_role, "tester");
        assert_eq!(cp.next_role, "reviewer");
        assert_eq!(cp.completed_roles, vec!["pm", "developer"]);
        assert!(cp.pending_tasks.is_empty());
        assert_eq!(cp.timestamp, "2026-04-21 10:00:00");
        assert_eq!(cp.current_task_id, "M1-T01");
        assert_eq!(cp.retry_count, 2);
        assert!(cp.prev_reasons.is_empty());
    }

    #[test]
    fn parse_checkpoint_with_prev_reasons() {
        let content = "# Porpoise Checkpoint\n\n## Metadata\n- cycle: 2\n- current_role: planning\n- next_role: development\n- current_task_id: M1-T01\n- retry_count: 0\n\n## Completed Roles\n- (none)\n\n## Pending Tasks\n- (none)\n\n## Prev Reasons\n- 명세 불명확\n- 테스트 누락\n";
        let cp = parse_checkpoint(content).unwrap();
        assert_eq!(cp.prev_reasons, vec!["명세 불명확", "테스트 누락"]);
    }

    #[test]
    fn checkpoint_new_roundtrip_with_prev_reasons() {
        let cp = Checkpoint::new(
            1, "planning", vec![], "development", vec![],
            "M1-T01", 0, vec!["사유1".to_string()],
        );
        assert_eq!(cp.prev_reasons, vec!["사유1"]);
    }

    #[test]
    fn parse_checkpoint_defaults_on_empty() {
        let cp = parse_checkpoint("# Porpoise Checkpoint\n").unwrap();
        assert_eq!(cp.cycle, 1);
        assert!(cp.current_role.is_empty());
        assert!(cp.completed_roles.is_empty());
        assert_eq!(cp.current_task_id, "");
        assert_eq!(cp.retry_count, 0);
    }

    #[test]
    fn parse_checkpoint_backward_compat_no_task_fields() {
        let old_format = "# Porpoise Checkpoint\n\n## Metadata\n- timestamp: 2026-04-21 10:00:00\n- cycle: 1\n- current_role: pm\n- next_role: developer\n\n## Completed Roles\n- (none)\n\n## Pending Tasks\n- (none)\n";
        let cp = parse_checkpoint(old_format).unwrap();
        assert_eq!(cp.cycle, 1);
        assert_eq!(cp.current_task_id, "");
        assert_eq!(cp.retry_count, 0);
    }

    #[test]
    fn save_and_load_checkpoint_new_path() {
        let temp = tempfile::tempdir().unwrap();
        let porpoise_dir = temp.path().join(".porpoise");
        std::fs::create_dir_all(&porpoise_dir).unwrap();

        let cp = Checkpoint::new(
            2, "development", vec!["planning".to_string()], "testing",
            vec![], "M1-T01", 1, vec![],
        );

        save_checkpoint(&cp, temp.path()).unwrap();

        // new path should exist
        assert!(porpoise_dir.join("checkpoint.json").exists());
        // old messages/ path should NOT exist
        assert!(!porpoise_dir.join("messages").join("checkpoint.json").exists());

        let loaded = load_checkpoint(temp.path()).unwrap();
        assert_eq!(loaded.cycle, 2);
        assert_eq!(loaded.current_role, "development");
        assert_eq!(loaded.current_task_id, "M1-T01");
        assert_eq!(loaded.retry_count, 1);
    }

    #[test]
    fn load_checkpoint_migration_from_old_json() {
        let temp = tempfile::tempdir().unwrap();
        let messages_dir = temp.path().join(".porpoise").join("messages");
        std::fs::create_dir_all(&messages_dir).unwrap();

        let cp = Checkpoint::new(
            3, "testing", vec!["planning".to_string(), "development".to_string()],
            "review", vec![], "M2-T02", 0, vec![],
        );
        let content = serde_json::to_string_pretty(&cp).unwrap();
        std::fs::write(messages_dir.join("checkpoint.json"), &content).unwrap();

        let loaded = load_checkpoint(temp.path()).unwrap();
        assert_eq!(loaded.cycle, 3);
        assert_eq!(loaded.current_role, "testing");
    }

    #[test]
    fn load_checkpoint_migration_from_old_md() {
        let temp = tempfile::tempdir().unwrap();
        let messages_dir = temp.path().join(".porpoise").join("messages");
        std::fs::create_dir_all(&messages_dir).unwrap();

        std::fs::write(
            messages_dir.join("checkpoint.md"),
            "# Porpoise Checkpoint\n\n## Metadata\n- cycle: 5\n- current_role: review\n- next_role: planning\n- current_task_id: M3-T01\n- retry_count: 0\n\n## Completed Roles\n- planning\n- development\n- testing\n\n## Pending Tasks\n- (none)\n",
        ).unwrap();

        let loaded = load_checkpoint(temp.path()).unwrap();
        assert_eq!(loaded.cycle, 5);
        assert_eq!(loaded.current_role, "review");
    }

    #[test]
    fn load_checkpoint_not_found() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".porpoise")).unwrap();
        let result = load_checkpoint(temp.path());
        assert!(result.is_err());
    }
}
