//! halt task 재투입 오버라이드 (M37) — 대시보드에서 보낸 재투입 요청을 소비한다.
//!
//! `max_redispatch` 소진으로 halt된 task는 incomplete로 남아, 다음 실행에서 어차피 재시도된다.
//! 오버라이드의 역할은 **재투입 예산을 상향**해 같은 한도에서 또 즉시 halt되지 않게 하는 것이다.
//!
//! 프로토콜(게이트 채널 재사용, 파일 매개·무결합):
//! 1. 대시보드가 `POST /api/control {decision:"redispatch", gate_id:<task_id>}` → conductor
//!    프로젝트의 `.porpoise/control/redispatch-<task_id>.json`(`{extra_budget:N}`)을 쓴다.
//! 2. conductor가 해당 task를 처리하기 직전 이 파일을 **소비(삭제)** 하고, 유효 재투입 한도를
//!    `base + extra_budget`으로 올린다. halt 힌트 파일도 정리한다.
//!
//! cleanup_stale_controls(gate.rs)는 redispatch-*.json을 지우지 않으므로, 실행 시작 청소에서도
//! 살아남아 해당 task에 도달할 때 소비된다.

use std::path::{Path, PathBuf};

use crate::logger::Logger;

/// 유효 재투입 한도의 안전 상한 (오버라이드 누적으로 무한 루프가 되지 않도록).
pub const MAX_EFFECTIVE_REDISPATCH: u32 = 20;
/// extra_budget 미지정·이상치일 때의 기본 추가 예산.
pub const DEFAULT_EXTRA_BUDGET: u32 = 1;

fn control_dir(path: &Path) -> PathBuf {
    path.join(".porpoise").join("control")
}

/// task별 재투입 오버라이드 파일 경로.
pub fn override_file(path: &Path, task_id: &str) -> PathBuf {
    control_dir(path).join(format!("redispatch-{}.json", task_id))
}

/// 오버라이드 본문에서 extra_budget을 해석한다 (순수). 손상·미지정이면 기본값.
pub fn parse_extra_budget(content: &str) -> u32 {
    let trimmed = content.trim_start_matches('\u{feff}');
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(v) => v
            .get("extra_budget")
            .and_then(|b| b.as_u64())
            .filter(|b| *b >= 1)
            .map(|b| b as u32)
            .unwrap_or(DEFAULT_EXTRA_BUDGET),
        Err(_) => DEFAULT_EXTRA_BUDGET,
    }
}

/// base 한도와 추가 예산을 합쳐 유효 한도를 구한다 (상한 클램프). (순수)
pub fn effective_max_redispatch(base: u32, extra: u32) -> u32 {
    base.saturating_add(extra).min(MAX_EFFECTIVE_REDISPATCH)
}

/// task의 재투입 오버라이드가 있으면 **소비(삭제)** 하고 extra_budget을 반환한다.
/// 없으면 None. 소비 시 halt 힌트 파일도 함께 정리한다.
pub fn consume_override(path: &Path, task_id: &str, logger: &Logger) -> Option<u32> {
    let file = override_file(path, task_id);
    let content = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file); // 소비
    let extra = parse_extra_budget(&content);
    clear_halt_hint(path, task_id);
    logger.info(
        "conductor",
        &format!("task {} 재투입 오버라이드 소비 — 재투입 예산 +{}", task_id, extra),
    );
    Some(extra)
}

/// halt 힌트 파일(`{task_id}-conductor-halt.md`)을 제거한다 (재투입으로 무효화됨).
fn clear_halt_hint(path: &Path, task_id: &str) {
    let hint = path
        .join(".porpoise")
        .join("hints")
        .join(format!("{}-conductor-halt.md", task_id));
    let _ = std::fs::remove_file(hint);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise").join("control")).unwrap();
        tmp
    }

    #[test]
    fn parse_extra_budget_variants() {
        assert_eq!(parse_extra_budget(r#"{"extra_budget":3}"#), 3);
        // 미지정 → 기본값
        assert_eq!(parse_extra_budget(r#"{"decision":"redispatch"}"#), DEFAULT_EXTRA_BUDGET);
        // 0·음수·손상 → 기본값
        assert_eq!(parse_extra_budget(r#"{"extra_budget":0}"#), DEFAULT_EXTRA_BUDGET);
        assert_eq!(parse_extra_budget("garbage"), DEFAULT_EXTRA_BUDGET);
    }

    #[test]
    fn effective_max_redispatch_clamps() {
        assert_eq!(effective_max_redispatch(2, 1), 3);
        assert_eq!(effective_max_redispatch(2, 0), 2);
        // 상한 클램프
        assert_eq!(effective_max_redispatch(18, 5), MAX_EFFECTIVE_REDISPATCH);
        // 오버플로 방지
        assert_eq!(effective_max_redispatch(u32::MAX, 5), MAX_EFFECTIVE_REDISPATCH);
    }

    #[test]
    fn consume_override_reads_and_deletes() {
        let tmp = dir();
        let path = tmp.path();
        let logger = Logger::new(path, false).unwrap();
        std::fs::write(override_file(path, "M37-T01"), r#"{"extra_budget":2}"#).unwrap();

        let extra = consume_override(path, "M37-T01", &logger);
        assert_eq!(extra, Some(2));
        // 소비됨 — 두 번째는 None
        assert!(consume_override(path, "M37-T01", &logger).is_none());
        assert!(!override_file(path, "M37-T01").exists(), "오버라이드는 소비(삭제)되어야 함");
    }

    #[test]
    fn consume_override_clears_halt_hint() {
        let tmp = dir();
        let path = tmp.path();
        let logger = Logger::new(path, false).unwrap();
        let hints = path.join(".porpoise").join("hints");
        std::fs::create_dir_all(&hints).unwrap();
        let hint = hints.join("M37-T02-conductor-halt.md");
        std::fs::write(&hint, "halt").unwrap();
        std::fs::write(override_file(path, "M37-T02"), r#"{"extra_budget":1}"#).unwrap();

        consume_override(path, "M37-T02", &logger);
        assert!(!hint.exists(), "재투입 시 halt 힌트가 정리되어야 함");
    }

    #[test]
    fn consume_override_absent_is_none() {
        let tmp = dir();
        let logger = Logger::new(tmp.path(), false).unwrap();
        assert!(consume_override(tmp.path(), "NOPE-T99", &logger).is_none());
    }
}
