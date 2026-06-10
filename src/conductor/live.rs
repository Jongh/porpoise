//! 라이브 실행 상태 기록 (M31) — `.porpoise/live.json` (스키마 live-1).
//!
//! conductor가 "지금 무엇을 하는 중"인지를 파일로 남긴다. 대시보드는 이 파일의 변화를
//! 감지해 SSE로 push할 뿐, **conductor는 대시보드의 존재를 모른다**(파일 매개, 결합 없음).
//!
//! 모든 기록 함수는 실패해도 실행을 막지 않는다(조용히 무시 — 라이브 표시는 부가 기능).
//! 부분 읽기를 막기 위해 temp 파일에 쓴 뒤 rename으로 원자적 교체한다.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 진행 중인 task 하나의 라이브 상태.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LiveTask {
    pub task_id: String,
    /// "brief" | "dispatch" | "verify" | "integrate" | "merged" | "halted"
    pub phase: String,
    pub redispatch: u32,
}

/// 승인 대기 게이트 (M33). 게이트 모드에서 conductor가 응답을 기다리는 동안 설정된다.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PendingGate {
    pub id: String,
    pub prompt: String,
    /// M34: 게이트 종류 — "confirm"(승인/정지) | "text"(자유 텍스트) | "confirm_text"(승인+선택 텍스트).
    /// 구 기록엔 없으므로 default(빈 문자열)는 confirm으로 해석한다.
    #[serde(default)]
    pub kind: String,
}

/// `.porpoise/live.json` 전체 상태 (live-1).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LiveState {
    pub schema_version: String,
    pub run_active: bool,
    pub started_at: String,
    pub updated_at: String,
    /// "sequential" | "parallel"
    pub mode: String,
    pub total_cost_usd: f64,
    pub budget_usd: Option<f64>,
    pub tasks: Vec<LiveTask>,
    /// M33: 승인 대기 게이트 (없으면 직렬화 생략 — live-1 하위호환)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_gate: Option<PendingGate>,
}

fn live_path(path: &Path) -> PathBuf {
    path.join(".porpoise").join("live.json")
}

/// live.json을 읽는다 (없거나 손상이면 None).
pub fn load(path: &Path) -> Option<LiveState> {
    let content = std::fs::read_to_string(live_path(path)).ok()?;
    serde_json::from_str(content.trim_start_matches('\u{feff}')).ok()
}

/// updated_at을 갱신하고 원자적으로 기록한다 (temp → rename). 실패는 무시.
fn write_atomic(path: &Path, mut state: LiveState) {
    state.updated_at = chrono::Local::now().to_rfc3339();
    let target = live_path(path);
    let tmp = target.with_extension("json.tmp");
    let Ok(json) = serde_json::to_string_pretty(&state) else { return };
    if std::fs::write(&tmp, json).is_err() {
        return;
    }
    // Windows에서 rename은 기존 파일을 덮지 못할 수 있어 실패 시 제거 후 재시도.
    if std::fs::rename(&tmp, &target).is_err() {
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::rename(&tmp, &target);
    }
}

/// 기존 상태를 읽어 수정 후 기록한다 (없으면 기본값에서 시작).
fn update(path: &Path, f: impl FnOnce(&mut LiveState)) {
    let mut state = load(path).unwrap_or_default();
    if state.schema_version.is_empty() {
        state.schema_version = "live-1".to_string();
    }
    f(&mut state);
    write_atomic(path, state);
}

/// 실행 시작 — 새 상태로 초기화한다.
pub fn start(path: &Path, mode: &str, budget_usd: Option<f64>) {
    let state = LiveState {
        schema_version: "live-1".to_string(),
        run_active: true,
        started_at: chrono::Local::now().to_rfc3339(),
        updated_at: String::new(), // write_atomic이 채움
        mode: mode.to_string(),
        total_cost_usd: 0.0,
        budget_usd,
        tasks: Vec::new(),
        pending_gate: None,
    };
    write_atomic(path, state);
}

/// task의 현재 단계를 갱신한다 (목록에 없으면 추가).
pub fn set_task(path: &Path, task_id: &str, phase: &str, redispatch: u32) {
    update(path, |s| {
        if let Some(t) = s.tasks.iter_mut().find(|t| t.task_id == task_id) {
            t.phase = phase.to_string();
            t.redispatch = redispatch;
        } else {
            s.tasks.push(LiveTask {
                task_id: task_id.to_string(),
                phase: phase.to_string(),
                redispatch,
            });
        }
    });
}

/// 병렬 배치 전체를 같은 단계로 기록한다 (이전 배치 목록은 대체).
pub fn set_batch(path: &Path, task_ids: &[String], phase: &str) {
    update(path, |s| {
        s.tasks = task_ids
            .iter()
            .map(|id| LiveTask { task_id: id.clone(), phase: phase.to_string(), redispatch: 0 })
            .collect();
    });
}

/// 누적 비용을 갱신한다.
pub fn set_total_cost(path: &Path, total_cost_usd: f64) {
    update(path, |s| s.total_cost_usd = total_cost_usd);
}

/// 승인 대기 게이트를 설정/해제한다 (M33).
pub fn set_pending_gate(path: &Path, gate: Option<PendingGate>) {
    update(path, |s| s.pending_gate = gate);
}

/// 실행 종료 — run_active=false (마지막 상태는 보존해 "마지막 실행 요약"으로 쓰인다).
/// 대기 게이트도 함께 해제한다.
pub fn finish(path: &Path) {
    update(path, |s| {
        s.run_active = false;
        s.pending_gate = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise")).unwrap();
        tmp
    }

    #[test]
    fn start_then_load_roundtrip() {
        let tmp = dir();
        start(tmp.path(), "sequential", Some(1.5));
        let s = load(tmp.path()).unwrap();
        assert_eq!(s.schema_version, "live-1");
        assert!(s.run_active);
        assert_eq!(s.mode, "sequential");
        assert_eq!(s.budget_usd, Some(1.5));
        assert!(!s.started_at.is_empty());
        assert!(!s.updated_at.is_empty());
    }

    #[test]
    fn set_task_adds_then_updates() {
        let tmp = dir();
        start(tmp.path(), "sequential", None);
        set_task(tmp.path(), "M1-T01", "dispatch", 0);
        set_task(tmp.path(), "M1-T01", "verify", 0);
        set_task(tmp.path(), "M1-T02", "brief", 1);

        let s = load(tmp.path()).unwrap();
        assert_eq!(s.tasks.len(), 2);
        assert_eq!(s.tasks[0].task_id, "M1-T01");
        assert_eq!(s.tasks[0].phase, "verify", "같은 task는 갱신");
        assert_eq!(s.tasks[1].redispatch, 1);
    }

    #[test]
    fn set_batch_replaces_tasks() {
        let tmp = dir();
        start(tmp.path(), "parallel", None);
        set_task(tmp.path(), "OLD-T01", "merged", 0);
        set_batch(tmp.path(), &["M1-T01".into(), "M1-T02".into()], "dispatch");

        let s = load(tmp.path()).unwrap();
        assert_eq!(s.tasks.len(), 2, "배치가 이전 목록을 대체");
        assert!(s.tasks.iter().all(|t| t.phase == "dispatch"));
    }

    #[test]
    fn cost_and_finish() {
        let tmp = dir();
        start(tmp.path(), "sequential", Some(1.0));
        set_total_cost(tmp.path(), 0.42);
        finish(tmp.path());

        let s = load(tmp.path()).unwrap();
        assert!(!s.run_active, "종료 후 run_active=false");
        assert!((s.total_cost_usd - 0.42).abs() < 1e-9);
        assert_eq!(s.budget_usd, Some(1.0), "기존 필드 보존");
    }

    #[test]
    fn load_missing_or_corrupt_is_none() {
        let tmp = dir();
        assert!(load(tmp.path()).is_none());
        std::fs::write(tmp.path().join(".porpoise").join("live.json"), "not json").unwrap();
        assert!(load(tmp.path()).is_none());
    }
}
