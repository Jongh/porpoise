//! 제어 API (M33) — `POST /api/control`.
//!
//! 대시보드의 첫 **쓰기** 기능. 쓰기 범위는 해당 프로젝트의 `.porpoise/control/`
//! 게이트 응답 파일로 한정된다. 보호 계층:
//! - M32 허용 목록·프로젝트 스코프 상속 (미등록 프로젝트 404)
//! - gate_id 형식 검증 (영숫자·하이픈만 — 경로 주입 차단)
//! - Origin 검증 (localhost 외 브라우저 cross-origin POST 403 — CSRF 차단)

use std::path::Path;

use serde::Deserialize;

/// 제어 요청 본문.
#[derive(Debug, Deserialize)]
pub struct ControlRequest {
    /// 응답할 게이트 id. 생략 + decision=stop 이면 사전 정지(stop-next).
    #[serde(default)]
    pub gate_id: Option<String>,
    pub decision: String,
    /// M34: 텍스트 게이트 응답 (예: 릴리즈 태그). 4KB 제한·제어문자 거부.
    #[serde(default)]
    pub text: Option<String>,
}

/// 텍스트 응답이 안전한가 (M34) — 4KB 이하, 제어문자 없음.
pub fn text_safe(text: &str) -> bool {
    text.len() <= 4096 && !text.chars().any(|c| c.is_control())
}

/// 제어 처리 결과 (상태코드 + 본문).
pub struct ControlOutcome {
    pub status: u16,
    pub body: String,
}

fn outcome(status: u16, body: &str) -> ControlOutcome {
    ControlOutcome { status, body: body.to_string() }
}

/// Origin 헤더가 허용 가능한가 (없음 = 비브라우저 도구 → 허용, 있으면 localhost만).
pub fn origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(o) => {
            o.starts_with("http://127.0.0.1") || o.starts_with("http://localhost")
        }
    }
}

/// gate_id가 경로-안전한가 (영숫자·하이픈만, 비어 있지 않음).
pub fn gate_id_safe(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// 제어 요청을 처리한다 — 검증 후 게이트 응답 파일을 원자적으로 작성.
pub fn handle_control(project: &Path, body: &str, origin: Option<&str>) -> ControlOutcome {
    if !origin_allowed(origin) {
        return outcome(403, r#"{"error":"forbidden origin"}"#);
    }
    let req: ControlRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(_) => return outcome(400, r#"{"error":"invalid body"}"#),
    };
    if req.decision != "approve" && req.decision != "stop" && req.decision != "redispatch" {
        return outcome(400, r#"{"error":"decision must be approve|stop|redispatch"}"#);
    }
    // M37: halt task 재투입 — 별도 채널(redispatch-<task_id>.json)에 오버라이드를 기록한다.
    if req.decision == "redispatch" {
        return handle_redispatch(project, req.gate_id.as_deref());
    }
    if let Some(ref t) = req.text {
        if !text_safe(t) {
            return outcome(400, r#"{"error":"text too long or contains control chars"}"#);
        }
    }

    let control_dir = project.join(".porpoise").join("control");
    if std::fs::create_dir_all(&control_dir).is_err() {
        return outcome(500, r#"{"error":"cannot create control dir"}"#);
    }

    let filename = match &req.gate_id {
        Some(id) => {
            if !gate_id_safe(id) {
                return outcome(400, r#"{"error":"invalid gate_id"}"#);
            }
            format!("gate-{}.json", id)
        }
        None => {
            // 사전 정지만 게이트 없이 허용
            if req.decision != "stop" {
                return outcome(400, r#"{"error":"gate_id required for approve"}"#);
            }
            "stop-next.json".to_string()
        }
    };

    // serde_json 직렬화 — 텍스트의 따옴표 등 특수문자를 안전하게 이스케이프
    let content = serde_json::json!({ "decision": req.decision, "text": req.text }).to_string();
    let target = control_dir.join(&filename);
    let tmp = control_dir.join(format!("{}.tmp", filename));
    if std::fs::write(&tmp, &content).is_err() {
        return outcome(500, r#"{"error":"write failed"}"#);
    }
    if std::fs::rename(&tmp, &target).is_err() {
        let _ = std::fs::remove_file(&target);
        if std::fs::rename(&tmp, &target).is_err() {
            return outcome(500, r#"{"error":"write failed"}"#);
        }
    }
    outcome(200, r#"{"ok":true}"#)
}

/// M37: 재투입 오버라이드 파일을 쓴다. gate_id에 task id를 받아 검증 후
/// `.porpoise/control/redispatch-<task_id>.json`(`{extra_budget:1}`)을 원자적으로 작성한다.
/// 다음 conductor 실행이 해당 task 처리 직전 이를 소비해 재투입 예산을 상향한다.
fn handle_redispatch(project: &Path, gate_id: Option<&str>) -> ControlOutcome {
    let task_id = match gate_id {
        Some(id) if gate_id_safe(id) => id,
        Some(_) => return outcome(400, r#"{"error":"invalid gate_id"}"#),
        None => return outcome(400, r#"{"error":"gate_id (task id) required for redispatch"}"#),
    };
    let control_dir = project.join(".porpoise").join("control");
    if std::fs::create_dir_all(&control_dir).is_err() {
        return outcome(500, r#"{"error":"cannot create control dir"}"#);
    }
    let content = serde_json::json!({ "extra_budget": 1 }).to_string();
    let filename = format!("redispatch-{}.json", task_id);
    let target = control_dir.join(&filename);
    let tmp = control_dir.join(format!("{}.tmp", filename));
    if std::fs::write(&tmp, &content).is_err() {
        return outcome(500, r#"{"error":"write failed"}"#);
    }
    if std::fs::rename(&tmp, &target).is_err() {
        let _ = std::fs::remove_file(&target);
        if std::fs::rename(&tmp, &target).is_err() {
            return outcome(500, r#"{"error":"write failed"}"#);
        }
    }
    outcome(200, r#"{"ok":true}"#)
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
    fn origin_rules() {
        assert!(origin_allowed(None), "비브라우저(헤더 없음) 허용");
        assert!(origin_allowed(Some("http://127.0.0.1:7878")));
        assert!(origin_allowed(Some("http://localhost:7878")));
        assert!(!origin_allowed(Some("http://evil.example.com")), "외부 origin 차단");
        assert!(!origin_allowed(Some("https://127.0.0.1.evil.com")));
    }

    #[test]
    fn approve_writes_gate_file() {
        let tmp = dir();
        let r = handle_control(tmp.path(), r#"{"gate_id":"m1-t01-120000","decision":"approve"}"#, None);
        assert_eq!(r.status, 200);
        let f = tmp.path().join(".porpoise").join("control").join("gate-m1-t01-120000.json");
        assert!(f.exists());
        assert!(std::fs::read_to_string(f).unwrap().contains("approve"));
    }

    #[test]
    fn stop_without_gate_id_is_stop_next() {
        let tmp = dir();
        let r = handle_control(tmp.path(), r#"{"decision":"stop"}"#, None);
        assert_eq!(r.status, 200);
        assert!(tmp.path().join(".porpoise").join("control").join("stop-next.json").exists());
    }

    #[test]
    fn rejects_invalid_inputs() {
        let tmp = dir();
        // 경로 주입 시도
        let r = handle_control(tmp.path(), r#"{"gate_id":"../../evil","decision":"approve"}"#, None);
        assert_eq!(r.status, 400, "경로 주입 gate_id 거부");
        // 미지 decision
        let r2 = handle_control(tmp.path(), r#"{"gate_id":"g1","decision":"nuke"}"#, None);
        assert_eq!(r2.status, 400);
        // approve엔 gate_id 필수
        let r3 = handle_control(tmp.path(), r#"{"decision":"approve"}"#, None);
        assert_eq!(r3.status, 400);
        // 손상 body
        let r4 = handle_control(tmp.path(), "garbage", None);
        assert_eq!(r4.status, 400);
        // 외부 origin
        let r5 = handle_control(tmp.path(), r#"{"decision":"stop"}"#, Some("http://evil.com"));
        assert_eq!(r5.status, 403);
        // 어떤 거부도 control/ 에 파일을 남기지 않음 (stop-next 제외 검증)
        let ctrl = tmp.path().join(".porpoise").join("control");
        let leftover: Vec<_> = std::fs::read_dir(&ctrl)
            .map(|e| e.flatten().map(|f| f.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        assert!(leftover.is_empty(), "거부 요청은 파일을 만들지 않아야 함: {:?}", leftover);
    }

    #[test]
    fn redispatch_writes_override_file() {
        // M37: decision=redispatch + gate_id(task id) → redispatch-<id>.json 작성
        let tmp = dir();
        let r = handle_control(
            tmp.path(),
            r#"{"gate_id":"M37-T01","decision":"redispatch"}"#,
            None,
        );
        assert_eq!(r.status, 200);
        let f = tmp.path().join(".porpoise").join("control").join("redispatch-M37-T01.json");
        assert!(f.exists(), "재투입 오버라이드 파일이 생성되어야 함");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        assert_eq!(v["extra_budget"], 1);
    }

    #[test]
    fn redispatch_rejects_invalid_inputs() {
        let tmp = dir();
        // gate_id(task id) 누락
        let r1 = handle_control(tmp.path(), r#"{"decision":"redispatch"}"#, None);
        assert_eq!(r1.status, 400, "재투입엔 task id 필수");
        // 경로 주입 task id
        let r2 = handle_control(
            tmp.path(),
            r#"{"gate_id":"../../evil","decision":"redispatch"}"#,
            None,
        );
        assert_eq!(r2.status, 400, "경로 주입 task id 거부");
        // 외부 origin
        let r3 = handle_control(
            tmp.path(),
            r#"{"gate_id":"M37-T01","decision":"redispatch"}"#,
            Some("http://evil.com"),
        );
        assert_eq!(r3.status, 403, "외부 origin 차단(재투입도 쓰기)");
    }

    #[test]
    fn text_response_written_and_validated() {
        // M34: 텍스트 응답 — 특수문자 이스케이프 + 검증 거부
        let tmp = dir();
        let r = handle_control(
            tmp.path(),
            r#"{"gate_id":"rel-1","decision":"approve","text":"v0.31.0 \"quoted\""}"#,
            None,
        );
        assert_eq!(r.status, 200);
        let f = tmp.path().join(".porpoise").join("control").join("gate-rel-1.json");
        let content = std::fs::read_to_string(f).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["text"], "v0.31.0 \"quoted\"", "텍스트가 안전하게 직렬화됨");

        // 제어문자 거부
        let r2 = handle_control(
            tmp.path(),
            "{\"gate_id\":\"rel-2\",\"decision\":\"approve\",\"text\":\"a\\u0000b\"}",
            None,
        );
        assert_eq!(r2.status, 400, "제어문자 텍스트 거부");
        // 4KB 초과 거부
        let long = "x".repeat(5000);
        let body = format!(r#"{{"gate_id":"rel-3","decision":"approve","text":"{}"}}"#, long);
        assert_eq!(handle_control(tmp.path(), &body, None).status, 400);
    }

    #[test]
    fn gate_roundtrip_with_conductor_poll() {
        // 종단: control API가 쓴 응답을 conductor 게이트가 소비
        let tmp = dir();
        std::fs::create_dir_all(tmp.path().join(".porpoise").join("control")).unwrap();
        let r = handle_control(tmp.path(), r#"{"decision":"stop"}"#, Some("http://127.0.0.1:7878"));
        assert_eq!(r.status, 200);
        let d = crate::conductor::gate::gate_decision_with_interval(
            tmp.path(), "M1-T01", "게이트", std::time::Duration::from_millis(10),
        );
        assert_eq!(d, crate::conductor::gate::Decision::Stop, "사전 정지가 게이트에서 소비됨");
    }
}
