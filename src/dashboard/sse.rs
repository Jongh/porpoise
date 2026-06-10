//! SSE 스트리밍 (M31) — `/api/events`.
//!
//! `.porpoise/live.json`(+ sessions 파일 수)의 변화를 폴링으로 감지해 SSE로 push한다.
//! 장수명 연결은 요청별 스레드에서 처리되므로 다른 요청을 블록하지 않는다(mod.rs).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

/// 폴링 주기.
const POLL_MS: u64 = 500;
/// keep-alive 주석(`: ping`) 주기 (폴링 틱 수 — 500ms × 20 = 10초).
const PING_TICKS: u32 = 20;

/// 변화 감지용 스냅샷 키 — live.json 원문 + sessions 파일 수 + 정지 예약 여부.
pub fn snapshot(project: &Path) -> String {
    let live = std::fs::read_to_string(project.join(".porpoise").join("live.json"))
        .unwrap_or_default();
    format!("{}|{}|{}", sessions_count(project), stop_pending(project), live)
}

/// 사전 정지(stop-next)가 대기 중인가 (M34) — 파일 존재만 확인 (read-only).
fn stop_pending(project: &Path) -> bool {
    project
        .join(".porpoise")
        .join("control")
        .join("stop-next.json")
        .exists()
}

fn sessions_count(project: &Path) -> usize {
    std::fs::read_dir(project.join(".porpoise").join("sessions"))
        .map(|entries| entries.flatten().filter(|e| e.path().is_file()).count())
        .unwrap_or(0)
}

/// `/api/live` 단발 응답 (SSE 폴백·초기 로드용).
/// live.json이 없으면 idle(`run_active=false`)을 반환한다.
pub fn live_payload(project: &Path) -> Value {
    let live = crate::conductor::live::load(project)
        .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
        .unwrap_or_else(|| json!({ "run_active": false }));
    json!({
        "live": live,
        "sessions_count": sessions_count(project),
        // M34: 정지 예약 가시화 — 버튼이 눌렸는지 모든 클라이언트가 일관되게 본다
        "stop_pending": stop_pending(project),
    })
}

/// tiny_http의 청크 인코더는 8192B 내부 버퍼(`chunked_transfer::Encoder::new`)가 차야
/// 전송하고, 그 아래 소켓도 1KB `BufWriter`로 감싸져 응답 중간 flush가 없다.
/// SSE 스펙상 무시되는 주석(`:`) 패딩으로 두 버퍼를 강제로 넘쳐 즉시 전송시킨다.
/// (로컬 전용·단계 전환 빈도라 ~8KB 오버헤드는 무의미)
const FLUSH_PADDING_BYTES: usize = 8300;

fn flush_padding() -> String {
    format!(": {}\n\n", "p".repeat(FLUSH_PADDING_BYTES))
}

/// SSE 이벤트 한 건을 직렬화한다 (`event: live` + data 한 줄 + flush 패딩).
pub fn format_event(payload: &Value) -> String {
    format!("event: live\ndata: {}\n\n{}", payload, flush_padding())
}

/// 무한 SSE 스트림 — `Read`를 구현해 tiny_http 청크 응답으로 흘려보낸다.
///
/// 연결 직후 현재 상태를 1회 push하고, 이후 변화 시마다 push한다.
/// 클라이언트가 끊으면 tiny_http의 write 실패로 응답이 종료되고 스레드가 정리된다.
pub struct SseStream {
    project: PathBuf,
    last_snapshot: String,
    buf: Vec<u8>,
    pos: usize,
    ticks: u32,
}

impl SseStream {
    pub fn new(project: &Path) -> Self {
        let payload = live_payload(project);
        SseStream {
            project: project.to_path_buf(),
            last_snapshot: snapshot(project),
            buf: format_event(&payload).into_bytes(),
            pos: 0,
            ticks: 0,
        }
    }

    fn refill(&mut self, data: String) {
        self.buf = data.into_bytes();
        self.pos = 0;
    }
}

impl Read for SseStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        loop {
            // 버퍼에 남은 데이터가 있으면 먼저 내보낸다.
            if self.pos < self.buf.len() {
                let n = std::cmp::min(out.len(), self.buf.len() - self.pos);
                out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            // 변화 폴링 (블로킹 — 요청 전용 스레드에서 돈다)
            std::thread::sleep(Duration::from_millis(POLL_MS));
            let snap = snapshot(&self.project);
            if snap != self.last_snapshot {
                self.last_snapshot = snap;
                self.ticks = 0;
                let payload = live_payload(&self.project);
                self.refill(format_event(&payload));
                continue;
            }
            self.ticks += 1;
            if self.ticks >= PING_TICKS {
                self.ticks = 0;
                // keep-alive도 패딩 포함 (버퍼 통과)
                self.refill(format!(": ping\n\n{}", flush_padding()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise").join("sessions")).unwrap();
        tmp
    }

    #[test]
    fn live_payload_idle_when_missing() {
        let tmp = dir();
        let v = live_payload(tmp.path());
        assert_eq!(v["live"]["run_active"], false);
        assert_eq!(v["sessions_count"], 0);
    }

    #[test]
    fn live_payload_reflects_live_json() {
        let tmp = dir();
        crate::conductor::live::start(tmp.path(), "sequential", Some(2.0));
        crate::conductor::live::set_task(tmp.path(), "M1-T01", "파서 구현", "dispatch", 0);
        std::fs::write(tmp.path().join(".porpoise").join("sessions").join("a.json"), "{}").unwrap();

        let v = live_payload(tmp.path());
        assert_eq!(v["live"]["run_active"], true);
        assert_eq!(v["live"]["mode"], "sequential");
        assert_eq!(v["live"]["tasks"][0]["task_id"], "M1-T01");
        assert_eq!(v["sessions_count"], 1);
    }

    #[test]
    fn snapshot_changes_on_update() {
        let tmp = dir();
        let s1 = snapshot(tmp.path());
        crate::conductor::live::start(tmp.path(), "sequential", None);
        let s2 = snapshot(tmp.path());
        assert_ne!(s1, s2, "live.json 변화가 스냅샷에 반영");
        std::fs::write(tmp.path().join(".porpoise").join("sessions").join("b.json"), "{}").unwrap();
        let s3 = snapshot(tmp.path());
        assert_ne!(s2, s3, "sessions 수 변화도 반영");
    }

    #[test]
    fn stop_pending_reflected_in_payload_and_snapshot() {
        // M34: stop-next.json 존재가 payload·snapshot에 반영 → SSE push 트리거
        let tmp = dir();
        let s1 = snapshot(tmp.path());
        let v1 = live_payload(tmp.path());
        assert_eq!(v1["stop_pending"], false);

        let ctrl = tmp.path().join(".porpoise").join("control");
        std::fs::create_dir_all(&ctrl).unwrap();
        std::fs::write(ctrl.join("stop-next.json"), "{}").unwrap();

        let v2 = live_payload(tmp.path());
        assert_eq!(v2["stop_pending"], true);
        assert_ne!(s1, snapshot(tmp.path()), "정지 예약이 스냅샷 변화로 감지됨");
    }

    #[test]
    fn format_event_is_sse_shaped() {
        let e = format_event(&serde_json::json!({"run_active": true}));
        assert!(e.starts_with("event: live\ndata: "));
        assert!(e.ends_with("\n\n"));
    }

    #[test]
    fn sse_stream_emits_initial_event() {
        let tmp = dir();
        crate::conductor::live::start(tmp.path(), "parallel", None);
        let mut s = SseStream::new(tmp.path());
        let mut buf = vec![0u8; 4096];
        let n = s.read(&mut buf).unwrap();
        let text = String::from_utf8_lossy(&buf[..n]);
        assert!(text.starts_with("event: live"), "연결 직후 현재 상태 push: {}", text);
        assert!(text.contains("parallel"));
    }
}
