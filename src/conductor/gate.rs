//! 게이트 입력 백엔드 (M33) — 대시보드 승인·정지.
//!
//! `[conductor] approval_mode = "gate"`면 콘솔 프롬프트 대신 이 프로토콜을 쓴다:
//! 1. (사전 정지 확인) `.porpoise/control/stop-next.json`이 있으면 소비하고 즉시 Stop
//! 2. `live.json`에 `pending_gate {id, prompt}` 기록 → 대시보드가 승인 카드 표시
//! 3. `.porpoise/control/gate-<id>.json`을 폴링 — `{decision: approve|stop}` 소비(삭제)
//! 4. `pending_gate` 해제 후 결정 반환
//!
//! 관측(M31)의 역방향이지만 같은 원칙: 파일 매개, conductor는 대시보드의 존재를 모른다.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::conductor::live::{self, PendingGate};

/// 게이트 결정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Stop,
}

fn control_dir(path: &Path) -> PathBuf {
    path.join(".porpoise").join("control")
}

fn gate_file(path: &Path, gate_id: &str) -> PathBuf {
    control_dir(path).join(format!("gate-{}.json", gate_id))
}

fn stop_next_file(path: &Path) -> PathBuf {
    control_dir(path).join("stop-next.json")
}

/// 실행 시작 시 이전 실행의 stale 제어 파일을 청소한다 (M33 리뷰).
///
/// 직전 실행에서 소비되지 않은 `stop-next.json`(게이트 없이 끝난 경우)이 남아 있으면
/// **다음 실행의 첫 게이트가 의도치 않게 즉시 정지**된다. 미소비 `gate-*.json`도 함께
/// 제거한다(이번 실행의 게이트 id와는 무관한 잔여물).
pub fn cleanup_stale_controls(path: &Path) {
    let dir = control_dir(path);
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "stop-next.json" || (name.starts_with("gate-") && name.ends_with(".json")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// task id에서 게이트 id를 만든다 (영숫자·하이픈만 — 경로 안전).
pub fn gate_id_for(task_id: &str) -> String {
    let sanitized: String = task_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    format!("{}-{}", sanitized, chrono::Local::now().format("%H%M%S"))
}

/// 응답 파일 내용을 해석한다 (순수). 손상·미지 decision은 None.
fn parse_decision(content: &str) -> Option<Decision> {
    let v: serde_json::Value = serde_json::from_str(content.trim_start_matches('\u{feff}')).ok()?;
    match v.get("decision").and_then(|d| d.as_str()) {
        Some("approve") => Some(Decision::Approve),
        Some("stop") => Some(Decision::Stop),
        _ => None,
    }
}

/// 파일이 존재하면 읽고 **소비(삭제)** 한다.
fn consume(file: &Path) -> Option<String> {
    let content = std::fs::read_to_string(file).ok()?;
    std::fs::remove_file(file).ok();
    Some(content)
}

/// 사전 정지(stop-next)가 대기 중이면 소비하고 true.
fn consume_stop_next(path: &Path) -> bool {
    consume(&stop_next_file(path)).is_some()
}

/// 한 번의 폴링 틱 — 게이트 응답·사전 정지를 확인한다 (순수에 가깝게 분리, 테스트 용이).
fn poll_once(path: &Path, gate_id: &str) -> Option<Decision> {
    if consume_stop_next(path) {
        return Some(Decision::Stop);
    }
    let file = gate_file(path, gate_id);
    let content = consume(&file)?;
    parse_decision(&content) // 손상이면 None — 이미 삭제됨, 계속 대기
}

/// 게이트 결정을 기다린다 (블로킹 — gate 모드 전용).
///
/// 대시보드가 `POST /api/control`로 응답 파일을 쓰면 소비하고 반환한다.
/// `poll_interval`은 테스트 주입용 (운영 기본 1초).
pub fn gate_decision(path: &Path, task_label: &str, prompt: &str) -> Decision {
    gate_decision_with_interval(path, task_label, prompt, Duration::from_secs(1))
}

pub fn gate_decision_with_interval(
    path: &Path,
    task_label: &str,
    prompt: &str,
    poll_interval: Duration,
) -> Decision {
    // control 디렉터리 보장 (없으면 폴링이 영원히 빈손)
    let _ = std::fs::create_dir_all(control_dir(path));

    let gate_id = gate_id_for(task_label);

    // 1. 사전 정지 — 게이트를 띄우기 전에 즉시 처리
    if consume_stop_next(path) {
        println!("  ⏹ 사전 정지 요청 수신 — 다음 task를 시작하지 않습니다.");
        return Decision::Stop;
    }

    // 2. 대기 상태 공개
    live::set_pending_gate(
        path,
        Some(PendingGate { id: gate_id.clone(), prompt: prompt.to_string() }),
    );
    println!("  ⏸ 대시보드 승인 대기 중... ({} — Ctrl-C로 중단)", prompt);

    // 3. 폴링
    let mut ticks: u64 = 0;
    let decision = loop {
        if let Some(d) = poll_once(path, &gate_id) {
            break d;
        }
        std::thread::sleep(poll_interval);
        ticks += 1;
        // 주기 안내 (운영 1초 간격 기준 30초마다)
        if ticks.is_multiple_of(30) {
            println!("  ⏸ 승인 대기 중... ({}초 경과)", ticks * poll_interval.as_secs().max(1));
        }
    };

    // 4. 대기 해제
    live::set_pending_gate(path, None);
    match decision {
        Decision::Approve => println!("  ▶ 대시보드 승인 — 진행합니다."),
        Decision::Stop => println!("  ⏹ 대시보드 정지 — 세션을 종료합니다."),
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise").join("control")).unwrap();
        tmp
    }

    fn write_gate_response(path: &Path, gate_id: &str, decision: &str) {
        std::fs::write(
            gate_file(path, gate_id),
            format!(r#"{{"decision":"{}"}}"#, decision),
        )
        .unwrap();
    }

    #[test]
    fn parse_decision_variants() {
        assert_eq!(parse_decision(r#"{"decision":"approve"}"#), Some(Decision::Approve));
        assert_eq!(parse_decision(r#"{"decision":"stop"}"#), Some(Decision::Stop));
        assert_eq!(parse_decision(r#"{"decision":"nuke"}"#), None);
        assert_eq!(parse_decision("not json"), None);
    }

    #[test]
    fn gate_id_is_path_safe() {
        let id = gate_id_for("M1-T01");
        assert!(id.starts_with("m1-t01-"));
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        // 위험 문자는 치환
        let id2 = gate_id_for("../evil/한글");
        assert!(id2.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn poll_consumes_approve_response() {
        let tmp = dir();
        write_gate_response(tmp.path(), "g1", "approve");
        assert_eq!(poll_once(tmp.path(), "g1"), Some(Decision::Approve));
        // 소비됨 — 두 번째 폴링은 빈손
        assert_eq!(poll_once(tmp.path(), "g1"), None);
        assert!(!gate_file(tmp.path(), "g1").exists(), "응답 파일은 소비(삭제)되어야 함");
    }

    #[test]
    fn poll_stop_next_takes_priority() {
        let tmp = dir();
        std::fs::write(stop_next_file(tmp.path()), "{}").unwrap();
        write_gate_response(tmp.path(), "g2", "approve");
        // 사전 정지가 우선
        assert_eq!(poll_once(tmp.path(), "g2"), Some(Decision::Stop));
        assert!(!stop_next_file(tmp.path()).exists(), "stop-next는 소비됨");
        // 다음 폴링에선 approve가 보임
        assert_eq!(poll_once(tmp.path(), "g2"), Some(Decision::Approve));
    }

    #[test]
    fn poll_ignores_and_removes_corrupt() {
        let tmp = dir();
        std::fs::write(gate_file(tmp.path(), "g3"), "garbage").unwrap();
        assert_eq!(poll_once(tmp.path(), "g3"), None, "손상은 결정 아님");
        assert!(!gate_file(tmp.path(), "g3").exists(), "손상 파일도 제거(무한 루프 방지)");
    }

    #[test]
    fn cleanup_removes_stale_stop_and_gate_files() {
        // M33 리뷰 회귀: 직전 실행의 stop-next가 다음 실행 첫 게이트를 정지시키지 않아야 함
        let tmp = dir();
        std::fs::write(stop_next_file(tmp.path()), "{}").unwrap();
        write_gate_response(tmp.path(), "old-gate", "approve");
        std::fs::write(control_dir(tmp.path()).join("unrelated.txt"), "keep").unwrap();

        cleanup_stale_controls(tmp.path());

        assert!(!stop_next_file(tmp.path()).exists(), "stale stop-next 제거");
        assert!(!gate_file(tmp.path(), "old-gate").exists(), "미소비 게이트 응답 제거");
        assert!(
            control_dir(tmp.path()).join("unrelated.txt").exists(),
            "제어 파일 외에는 건드리지 않음"
        );
        // control 디렉터리가 없어도 패닉 없음
        let empty = tempfile::tempdir().unwrap();
        cleanup_stale_controls(empty.path());
    }

    #[test]
    fn gate_decision_returns_when_response_preexists() {
        // 블로킹 함수지만 응답을 미리 두면 즉시 반환 — gate_id에 타임스탬프가 있어
        // 미리 알 수 없으므로 사전 정지 경로로 검증
        let tmp = dir();
        std::fs::write(stop_next_file(tmp.path()), "{}").unwrap();
        let d = gate_decision_with_interval(
            tmp.path(), "M1-T01", "테스트 게이트", Duration::from_millis(10),
        );
        assert_eq!(d, Decision::Stop);
        // live의 pending_gate는 설정되지 않았거나 해제됨
        let live = crate::conductor::live::load(tmp.path());
        assert!(live.is_none_or(|s| s.pending_gate.is_none()));
    }

    #[test]
    fn gate_decision_consumes_late_response() {
        // 별도 스레드가 0.05초 후 응답을 쓰는 시나리오 — 폴링 수신 확인.
        // gate_id를 모르는 문제: 스레드가 control 디렉터리를 감시해 gate-*.json 이름을 찾아 응답.
        let tmp = dir();
        let path = tmp.path().to_path_buf();
        let ctrl = control_dir(&path);
        let writer = std::thread::spawn(move || {
            for _ in 0..100 {
                std::thread::sleep(Duration::from_millis(10));
                // live.json의 pending_gate에서 id를 읽는다 (대시보드와 동일한 방법)
                if let Some(s) = crate::conductor::live::load(&path) {
                    if let Some(g) = s.pending_gate {
                        std::fs::write(
                            ctrl.join(format!("gate-{}.json", g.id)),
                            r#"{"decision":"approve"}"#,
                        )
                        .unwrap();
                        return;
                    }
                }
            }
            panic!("pending_gate가 live.json에 나타나지 않음");
        });

        let d = gate_decision_with_interval(
            tmp.path(), "M1-T02", "지휘하시겠습니까?", Duration::from_millis(10),
        );
        writer.join().unwrap();
        assert_eq!(d, Decision::Approve);
        let live = crate::conductor::live::load(tmp.path()).unwrap();
        assert!(live.pending_gate.is_none(), "결정 후 pending_gate 해제");
    }
}
