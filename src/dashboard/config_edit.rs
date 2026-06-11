//! 설정 편집 백엔드 (M37) — `GET/POST /api/config`.
//!
//! 대시보드에서 `[conductor]` 핵심 설정을 읽고 쓴다. M33은 제어 쓰기를 `.porpoise/control/`로
//! **한정**하고 *"코드·project.md·설정은 일절 쓰지 않는다"*고 못 박았다. 런처는 이 경계를
//! **의도적으로 확장**한다 — 단, `[conductor]`의 **화이트리스트 키**에 한해서만, 필드별
//! 검증을 거쳐서. 임의 TOML 주입은 차단한다(코드·project.md는 여전히 불가).
//!
//! 보안: M33 Origin 검증·M32 프로젝트 스코프(라우터에서 해석)를 상속한다.
//! 쓰기 원자성: 검증 실패 시 어떤 키도 쓰지 않는다. 쓰기는 utils::fs::write_file(루트 경계) 경유.

use std::path::Path;

use serde_json::{json, Value};

use crate::config::workspace::WorkspaceConfig;
use crate::dashboard::control::origin_allowed;

/// 편집 가능한 `[conductor]` 키 (화이트리스트). 이 목록 밖의 키는 거부된다.
pub const EDITABLE_KEYS: &[&str] = &[
    "mode",
    "approval_mode",
    "max_parallel",
    "max_redispatch",
    "serve_dashboard",
    "verifier_model",
    "verdict_fallback",
    "dashboard_port",
];

/// 처리 결과 (상태코드 + 본문).
pub struct ConfigOutcome {
    pub status: u16,
    pub body: String,
}

fn outcome(status: u16, body: &str) -> ConfigOutcome {
    ConfigOutcome { status, body: body.to_string() }
}

/// `GET /api/config` — 현재 `[conductor]` 편집 가능 값을 JSON으로 (effective 값, 항상 존재).
pub fn read_config_json(project: &Path) -> Value {
    let cfg = WorkspaceConfig::load(project).unwrap_or_default();
    json!({
        "conductor": {
            "mode": if cfg.conductor_enabled() { "conductor" } else { "legacy" },
            "approval_mode": if cfg.conductor_gate_mode() { "gate" } else { "console" },
            "max_parallel": cfg.conductor_max_parallel(),
            "max_redispatch": cfg.conductor_max_redispatch(),
            "serve_dashboard": cfg.conductor_serve_dashboard(),
            "verifier_model": cfg.conductor_verifier_model().unwrap_or(""),
            "verdict_fallback": if cfg.conductor_verdict_fallback_halt() { "halt" } else { "pass_if_checks_pass" },
            "dashboard_port": cfg.conductor_dashboard_port(),
        }
    })
}

/// 텍스트 값이 안전한가 — 200자 이하, 제어문자 없음.
fn text_safe(s: &str) -> bool {
    s.len() <= 200 && !s.chars().any(|c| c.is_control())
}

/// 요청 본문(JSON 객체)을 검증해 `[conductor]`에 쓸 (키, toml 값) 목록으로 변환한다 (순수).
/// 화이트리스트 외 키·검증 실패는 Err(사유) — 호출자가 400으로 응답하고 아무것도 쓰지 않는다.
pub fn validate_config_update(body: &str) -> Result<Vec<(String, toml::Value)>, String> {
    let v: Value = serde_json::from_str(body.trim_start_matches('\u{feff}'))
        .map_err(|_| "invalid json body".to_string())?;
    let obj = v.as_object().ok_or_else(|| "body must be a json object".to_string())?;
    if obj.is_empty() {
        return Err("no fields to update".to_string());
    }

    let mut updates: Vec<(String, toml::Value)> = Vec::new();
    for (key, val) in obj {
        if !EDITABLE_KEYS.contains(&key.as_str()) {
            return Err(format!("unknown or non-editable key: {}", key));
        }
        let tv = match key.as_str() {
            "mode" => enum_str(val, key, &["legacy", "conductor"])?,
            "approval_mode" => enum_str(val, key, &["console", "gate"])?,
            "verdict_fallback" => enum_str(val, key, &["pass_if_checks_pass", "halt"])?,
            "max_parallel" => int_in_range(val, key, 1, 8)?,
            "max_redispatch" => int_in_range(val, key, 0, 20)?,
            "dashboard_port" => int_in_range(val, key, 1024, 65535)?,
            "serve_dashboard" => {
                let b = val.as_bool().ok_or_else(|| format!("{} must be a boolean", key))?;
                toml::Value::Boolean(b)
            }
            "verifier_model" => {
                let s = val.as_str().ok_or_else(|| format!("{} must be a string", key))?;
                if !text_safe(s) {
                    return Err(format!("{} too long or contains control chars", key));
                }
                toml::Value::String(s.to_string())
            }
            _ => return Err(format!("unhandled key: {}", key)),
        };
        updates.push((key.clone(), tv));
    }
    Ok(updates)
}

fn enum_str(val: &Value, key: &str, allowed: &[&str]) -> Result<toml::Value, String> {
    let s = val.as_str().ok_or_else(|| format!("{} must be a string", key))?;
    if !allowed.contains(&s) {
        return Err(format!("{} must be one of {:?}", key, allowed));
    }
    Ok(toml::Value::String(s.to_string()))
}

fn int_in_range(val: &Value, key: &str, lo: i64, hi: i64) -> Result<toml::Value, String> {
    let n = val.as_i64().ok_or_else(|| format!("{} must be an integer", key))?;
    if !(lo..=hi).contains(&n) {
        return Err(format!("{} must be in [{}, {}]", key, lo, hi));
    }
    Ok(toml::Value::Integer(n))
}

/// 검증된 toml::Value를 toml_edit 항목으로 변환한다 (화이트리스트 타입만 — string·int·bool).
fn to_edit_item(v: &toml::Value) -> toml_edit::Item {
    match v {
        toml::Value::String(s) => toml_edit::value(s.clone()),
        toml::Value::Integer(i) => toml_edit::value(*i),
        toml::Value::Boolean(b) => toml_edit::value(*b),
        // 화이트리스트는 위 3종뿐 — 방어적 폴백
        other => toml_edit::value(other.to_string()),
    }
}

/// 기존 workspace.toml(없으면 빈)에 `[conductor]` 키만 갱신해 직렬화한다 (순수).
/// M38: `toml_edit`로 교체 — **주석·키 순서·서식을 보존**하면서 해당 키만 set한다.
pub fn apply_updates(existing: &str, updates: &[(String, toml::Value)]) -> Result<String, String> {
    use toml_edit::{DocumentMut, Item, Table};

    let mut doc: DocumentMut = if existing.trim().is_empty() {
        DocumentMut::new()
    } else {
        existing.parse().map_err(|e| format!("existing toml parse error: {}", e))?
    };

    let root = doc.as_table_mut();
    // [conductor] 테이블 확보 (없으면 생성, 다른 타입이면 거부)
    if !root.contains_key("conductor") {
        root.insert("conductor", Item::Table(Table::new()));
    }
    let table = root
        .get_mut("conductor")
        .and_then(|i| i.as_table_mut())
        .ok_or_else(|| "[conductor] is not a table".to_string())?;

    for (k, v) in updates {
        // 기존 키는 값만 교체(키의 prefix 주석·서식 보존), 없으면 새로 삽입.
        match table.get_mut(k.as_str()) {
            Some(item) => *item = to_edit_item(v),
            None => {
                table.insert(k, to_edit_item(v));
            }
        }
    }

    Ok(doc.to_string())
}

/// `POST /api/config` — 검증 후 workspace.toml의 `[conductor]`를 갱신한다.
pub fn handle_config_post(project: &Path, body: &str, origin: Option<&str>) -> ConfigOutcome {
    if !origin_allowed(origin) {
        return outcome(403, r#"{"error":"forbidden origin"}"#);
    }
    let updates = match validate_config_update(body) {
        Ok(u) => u,
        Err(e) => return outcome(400, &json!({ "error": e }).to_string()),
    };

    let ws_path = project.join(".porpoise").join("workspace.toml");
    let existing = std::fs::read_to_string(&ws_path).unwrap_or_default();
    let merged = match apply_updates(&existing, &updates) {
        Ok(m) => m,
        Err(e) => return outcome(400, &json!({ "error": e }).to_string()),
    };

    // .porpoise/ 보장 후 경계 검사 쓰기
    if std::fs::create_dir_all(project.join(".porpoise")).is_err() {
        return outcome(500, r#"{"error":"cannot create .porpoise dir"}"#);
    }
    match crate::utils::fs::write_file(&ws_path, &merged, project) {
        Ok(_) => outcome(200, r#"{"ok":true}"#),
        Err(_) => outcome(500, r#"{"error":"write failed"}"#),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_dir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise")).unwrap();
        tmp
    }

    #[test]
    fn read_config_returns_defaults() {
        let tmp = project_dir();
        let v = read_config_json(tmp.path());
        assert_eq!(v["conductor"]["mode"], "conductor");
        assert_eq!(v["conductor"]["approval_mode"], "console");
        assert_eq!(v["conductor"]["max_parallel"], 1);
        assert_eq!(v["conductor"]["max_redispatch"], 2);
    }

    #[test]
    fn validate_accepts_valid_fields() {
        let u = validate_config_update(
            r#"{"max_parallel":4,"approval_mode":"gate","serve_dashboard":true,"verifier_model":"claude-opus-4-8"}"#,
        )
        .unwrap();
        assert_eq!(u.len(), 4);
    }

    #[test]
    fn validate_rejects_unknown_key() {
        let e = validate_config_update(r#"{"budget_usd":5.0}"#).unwrap_err();
        assert!(e.contains("non-editable"), "화이트리스트 외 키 거부: {}", e);
    }

    #[test]
    fn validate_rejects_bad_values() {
        // 범위 초과
        assert!(validate_config_update(r#"{"max_parallel":99}"#).is_err());
        // 잘못된 열거값
        assert!(validate_config_update(r#"{"approval_mode":"nuke"}"#).is_err());
        assert!(validate_config_update(r#"{"mode":"weird"}"#).is_err());
        assert!(validate_config_update(r#"{"verdict_fallback":"maybe"}"#).is_err());
        // 타입 오류
        assert!(validate_config_update(r#"{"serve_dashboard":"yes"}"#).is_err());
        assert!(validate_config_update(r#"{"max_redispatch":"two"}"#).is_err());
        // 빈 객체
        assert!(validate_config_update(r#"{}"#).is_err());
        // 손상 json
        assert!(validate_config_update("garbage").is_err());
    }

    #[test]
    fn apply_updates_preserves_other_sections() {
        let existing = "[general]\nlanguage = \"en\"\n\n[conductor]\nmode = \"conductor\"\nmax_parallel = 1\n";
        let updates = vec![
            ("max_parallel".to_string(), toml::Value::Integer(4)),
            ("approval_mode".to_string(), toml::Value::String("gate".to_string())),
        ];
        let merged = apply_updates(existing, &updates).unwrap();
        let cfg: WorkspaceConfig = toml::from_str(&merged).unwrap();
        // 갱신됨
        assert_eq!(cfg.conductor_max_parallel(), 4);
        assert!(cfg.conductor_gate_mode());
        // 다른 섹션 보존
        assert_eq!(cfg.language(), "en");
    }

    #[test]
    fn apply_updates_preserves_comments_and_order() {
        // M38: toml_edit — 주석·키 순서·서식 보존
        let existing = "\
# 작업 환경 설정
[general]
language = \"ko\"  # 작업 언어

[conductor]
# 동시 처리 task 수
max_parallel = 1
mode = \"conductor\"
";
        let updates = vec![("max_parallel".to_string(), toml::Value::Integer(4))];
        let merged = apply_updates(existing, &updates).unwrap();
        assert!(merged.contains("# 작업 환경 설정"), "최상단 주석 보존");
        assert!(merged.contains("# 작업 언어"), "인라인 주석 보존");
        assert!(merged.contains("# 동시 처리 task 수"), "[conductor] 주석 보존");
        assert!(merged.contains("max_parallel = 4"), "값은 갱신");
        // 갱신 후에도 유효 toml + 값 반영
        let cfg: WorkspaceConfig = toml::from_str(&merged).unwrap();
        assert_eq!(cfg.conductor_max_parallel(), 4);
        assert_eq!(cfg.language(), "ko");
    }

    #[test]
    fn dashboard_port_validation() {
        // 유효 포트
        assert!(validate_config_update(r#"{"dashboard_port":9000}"#).is_ok());
        // 특권 포트(범위 외) 거부
        assert!(validate_config_update(r#"{"dashboard_port":80}"#).is_err());
        assert!(validate_config_update(r#"{"dashboard_port":70000}"#).is_err());
        // GET에 노출
        let tmp = project_dir();
        let v = read_config_json(tmp.path());
        assert_eq!(v["conductor"]["dashboard_port"], 7878);
    }

    #[test]
    fn apply_updates_creates_conductor_when_absent() {
        let merged = apply_updates(
            "[general]\nlanguage = \"ko\"\n",
            &[("max_redispatch".to_string(), toml::Value::Integer(5))],
        )
        .unwrap();
        let cfg: WorkspaceConfig = toml::from_str(&merged).unwrap();
        assert_eq!(cfg.conductor_max_redispatch(), 5);
        assert_eq!(cfg.language(), "ko");
    }

    #[test]
    fn post_writes_and_rejects() {
        let tmp = project_dir();
        // 유효 쓰기
        let r = handle_config_post(tmp.path(), r#"{"max_parallel":3}"#, None);
        assert_eq!(r.status, 200);
        let cfg = WorkspaceConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.conductor_max_parallel(), 3);

        // 화이트리스트 외 키 — 400, 파일 무변경
        let before = std::fs::read_to_string(tmp.path().join(".porpoise").join("workspace.toml")).unwrap();
        let r2 = handle_config_post(tmp.path(), r#"{"general":"x"}"#, None);
        assert_eq!(r2.status, 400);
        let after = std::fs::read_to_string(tmp.path().join(".porpoise").join("workspace.toml")).unwrap();
        assert_eq!(before, after, "검증 실패 시 파일을 쓰지 않아야 함");

        // 외부 origin — 403
        let r3 = handle_config_post(tmp.path(), r#"{"max_parallel":2}"#, Some("http://evil.com"));
        assert_eq!(r3.status, 403);
    }
}
