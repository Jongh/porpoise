//! 로컬 웹 대시보드 (M30) — read-only 관측. tiny_http + 임베디드 프론트엔드.
//!
//! `porpoise dashboard`로 로컬 서버를 띄워 `.porpoise/` 데이터(리포트·비용·의존성 그래프)를
//! 브라우저에서 본다. conductor 로직은 건드리지 않는다(파일 쓰기 없음).

pub mod api;
pub mod control;
pub mod registry;
pub mod sse;

use std::path::{Path, PathBuf};

use anyhow::Result;
use colored::Colorize;

// 프론트엔드 에셋을 바이너리에 임베드 (CDN 의존 0, 오프라인 동작).
const INDEX_HTML: &str = include_str!("static/index.html");
const APP_JS: &str = include_str!("static/app.js");
const CHART_JS: &str = include_str!("static/chart.js");
const STYLE_CSS: &str = include_str!("static/style.css");

/// 라우팅 결과 (순수) — 상태코드·Content-Type·본문.
pub struct RouteResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

/// URL을 (경로, 쿼리 파라미터)로 분리한다.
fn parse_path_and_query(url: &str) -> (String, Vec<(String, String)>) {
    match url.split_once('?') {
        Some((p, q)) => {
            let params = q
                .split('&')
                .filter_map(|kv| {
                    let (k, v) = kv.split_once('=')?;
                    Some((k.to_string(), v.to_string()))
                })
                .collect();
            (p.to_string(), params)
        }
        None => (url.to_string(), Vec::new()),
    }
}

fn param_milestone(params: &[(String, String)]) -> Option<u32> {
    params
        .iter()
        .find(|(k, _)| k == "milestone")
        .and_then(|(_, v)| v.parse().ok())
}

fn param_project(params: &[(String, String)]) -> Option<String> {
    params
        .iter()
        .find(|(k, _)| k == "project")
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

/// `?project=<id>`를 해석한다 (M32). 미지정이면 기본 경로(하위호환), 지정이면
/// **레지스트리에 등록된 경로만** 허용 — 미등록·소멸 경로는 Err(404 응답용).
fn resolve_project_scope(default_path: &Path, params: &[(String, String)]) -> Result<PathBuf, ()> {
    match param_project(params) {
        None => Ok(default_path.to_path_buf()),
        Some(id) => registry::resolve(&id).ok_or(()),
    }
}

fn html(body: &str) -> RouteResponse {
    RouteResponse { status: 200, content_type: "text/html; charset=utf-8", body: body.to_string() }
}
fn js(body: &str) -> RouteResponse {
    RouteResponse {
        status: 200,
        content_type: "application/javascript; charset=utf-8",
        body: body.to_string(),
    }
}
fn json_resp(v: serde_json::Value) -> RouteResponse {
    RouteResponse {
        status: 200,
        content_type: "application/json; charset=utf-8",
        body: v.to_string(),
    }
}
fn not_found() -> RouteResponse {
    RouteResponse { status: 404, content_type: "text/plain; charset=utf-8", body: "Not Found".into() }
}

/// 요청 URL을 라우팅한다 (파일은 읽되 쓰지 않음 — read-only).
///
/// M32: `?project=<id>`가 있으면 레지스트리에 등록된 프로젝트로 스코프를 바꾼다.
/// 미등록·소멸 경로는 404. 미지정이면 기본 경로(기동 디렉터리, 하위호환).
pub fn route(path: &Path, url: &str) -> RouteResponse {
    let (p, params) = parse_path_and_query(url);

    // 정적 에셋·프로젝트 목록은 스코프 무관
    match p.as_str() {
        "/" | "/index.html" => return html(INDEX_HTML),
        "/static/app.js" => return js(APP_JS),
        "/static/chart.js" => return js(CHART_JS),
        "/static/style.css" => {
            return RouteResponse {
                status: 200,
                content_type: "text/css; charset=utf-8",
                body: STYLE_CSS.to_string(),
            }
        }
        "/api/projects" => return json_resp(projects_json(path)),
        _ => {}
    }

    // API는 프로젝트 스코프 해석 후 수행
    let Ok(scope) = resolve_project_scope(path, &params) else {
        return RouteResponse {
            status: 404,
            content_type: "application/json; charset=utf-8",
            body: r#"{"error":"unknown project id (not registered or path gone)"}"#.to_string(),
        };
    };
    let scope = scope.as_path();
    match p.as_str() {
        "/api/milestones" => json_resp(api::milestones_json(scope)),
        "/api/report" => json_resp(api::report_json(scope, param_milestone(&params))),
        "/api/tasks" => json_resp(api::tasks_json(scope)),
        // M31: 단발 라이브 조회 (SSE 폴백·초기 로드)
        "/api/live" => json_resp(sse::live_payload(scope)),
        // M36: task 상세 — 최신 run의 라운드별 작업 보고·검증 피드백
        "/api/task" => {
            let id = params
                .iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            if id.is_empty() {
                return RouteResponse {
                    status: 400,
                    content_type: "application/json; charset=utf-8",
                    body: r#"{"error":"id required"}"#.to_string(),
                };
            }
            json_resp(api::task_detail_json(scope, &id))
        }
        _ => not_found(),
    }
}

/// `GET /api/projects` — 레지스트리 목록 (+ 기동 디렉터리 표시).
/// M35: 실존 항목만 노출 (삭제된 프로젝트의 stale 항목 숨김).
fn projects_json(current: &Path) -> serde_json::Value {
    let reg = registry::load();
    let current_id = registry::project_id(&registry::normalize(current));
    let projects: Vec<serde_json::Value> = registry::list_existing(&reg)
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id, "name": e.name, "path": e.path,
                "current": e.id == current_id,
            })
        })
        .collect();
    serde_json::json!({ "projects": projects })
}


/// 포트 바인딩을 시도한다 (M35) — 실패(대개 포트 사용 중)는 에러가 아닌 None.
fn try_bind(port: u16) -> Option<tiny_http::Server> {
    tiny_http::Server::http(format!("127.0.0.1:{}", port)).ok()
}

/// 요청 수락 루프 (M31: 요청별 스레드 분리 — 장수명 SSE가 다른 요청을 블록하지 않음).
fn serve_loop(server: tiny_http::Server, project: std::path::PathBuf) {
    for request in server.incoming_requests() {
        let p = project.clone();
        std::thread::spawn(move || handle_request(request, &p));
    }
}

/// 백그라운드 기동 결과 (M35).
#[derive(Debug)]
pub enum ServeOutcome {
    /// 새 서버를 스레드로 기동함.
    Started(String),
    /// 포트가 이미 사용 중 — 기존 대시보드와 공존(같은 URL 사용).
    PortInUse(String),
    /// 기동 불가 (프로젝트 아님 등).
    Failed(String),
}

/// 대시보드 서버를 백그라운드 스레드로 기동한다 (M35 — conductor 내장 기동용).
/// 같은 프로세스여도 conductor와의 통신은 파일 매개(live.json·control/) 그대로다.
pub fn serve_in_background(project: &Path, port: u16) -> ServeOutcome {
    if !project.join(".porpoise").exists() {
        return ServeOutcome::Failed(".porpoise/ 없음".to_string());
    }
    if let Err(e) = registry::register(project) {
        eprintln!("  ⚠ 프로젝트 자동 등록 실패: {}", e);
    }
    let url = format!("http://127.0.0.1:{}", port);
    match try_bind(port) {
        Some(server) => {
            let p = project.to_path_buf();
            std::thread::spawn(move || serve_loop(server, p));
            ServeOutcome::Started(url)
        }
        None => ServeOutcome::PortInUse(url),
    }
}

/// 대시보드 서버를 기동한다 (블로킹 CLI 진입점 — `porpoise dashboard`). Ctrl-C로 종료.
pub fn run_dashboard(path: &Path, port: u16, open: bool) -> Result<()> {
    if !path.join(".porpoise").exists() {
        anyhow::bail!(
            ".porpoise/ 가 없습니다. porpoise 프로젝트 디렉터리에서 실행하세요."
        );
    }
    // M32: 현재 프로젝트를 레지스트리에 자동 등록 (멀티 프로젝트 셀렉터 노출)
    if let Err(e) = registry::register(path) {
        eprintln!("  ⚠ 프로젝트 자동 등록 실패: {}", e);
    }

    let addr = format!("127.0.0.1:{}", port);
    let server = try_bind(port).ok_or_else(|| {
        anyhow::anyhow!("대시보드 서버 기동 실패 ({}) — 다른 --port를 시도하세요.", addr)
    })?;

    let url = format!("http://{}", addr);
    println!();
    println!("{} {}", "▶ Porpoise 대시보드".green().bold(), url.cyan());
    println!("{}", "  read-only 관측 — Ctrl-C로 종료".dimmed());

    if open {
        if let Err(e) = webbrowser::open(&url) {
            eprintln!("  ⚠ 브라우저 자동 열기 실패: {} (위 URL을 직접 여세요)", e);
        }
    }

    serve_loop(server, path.to_path_buf());
    Ok(())
}

/// 요청 하나를 처리한다 (요청 전용 스레드에서 실행).
fn handle_request(mut request: tiny_http::Request, project: &Path) {
    let url = request.url().to_string();

    // M33: 제어 POST — 유일한 쓰기 경로 (.porpoise/control/ 한정)
    let (req_path, _) = parse_path_and_query(&url);
    if req_path == "/api/control" && request.method() == &tiny_http::Method::Post {
        let (_, params) = parse_path_and_query(&url);
        let Ok(scope) = resolve_project_scope(project, &params) else {
            respond_json(request, 404, r#"{"error":"unknown project id"}"#);
            return;
        };
        let origin = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Origin"))
            .map(|h| h.value.as_str().to_string());
        let mut body = String::new();
        use std::io::Read;
        if request.as_reader().take(16 * 1024).read_to_string(&mut body).is_err() {
            respond_json(request, 400, r#"{"error":"unreadable body"}"#);
            return;
        }
        let out = control::handle_control(&scope, &body, origin.as_deref());
        respond_json(request, out.status, &out.body);
        return;
    }

    if req_path == "/api/events" {
        // M32: SSE도 ?project= 스코프 적용 (미등록 id는 404)
        let (_, params) = parse_path_and_query(&url);
        let Ok(scope) = resolve_project_scope(project, &params) else {
            let r = tiny_http::Response::from_string(r#"{"error":"unknown project id"}"#)
                .with_status_code(404);
            let _ = request.respond(r);
            return;
        };
        // SSE: 무한 스트림 — Content-Length 없이 청크로 흘려보낸다.
        let stream = sse::SseStream::new(&scope);
        let headers = [
            ("Content-Type", "text/event-stream; charset=utf-8"),
            ("Cache-Control", "no-cache"),
        ]
        .iter()
        .map(|(k, v)| {
            tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("유효한 헤더")
        })
        .collect();
        let response = tiny_http::Response::new(tiny_http::StatusCode(200), headers, stream, None, None);
        let _ = request.respond(response); // 클라이언트가 끊으면 write 실패로 종료
        return;
    }
    let r = route(project, &url);
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], r.content_type.as_bytes())
        .expect("유효한 헤더");
    let response = tiny_http::Response::from_string(r.body)
        .with_status_code(r.status)
        .with_header(header);
    let _ = request.respond(response);
}

/// JSON 응답 헬퍼.
fn respond_json(request: tiny_http::Request, status: u16, body: &str) {
    let header =
        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
            .expect("유효한 헤더");
    let response = tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(header);
    let _ = request.respond(response);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_path_and_query_splits() {
        let (p, params) = parse_path_and_query("/api/report?milestone=30&x=1");
        assert_eq!(p, "/api/report");
        assert_eq!(param_milestone(&params), Some(30));
        let (p2, params2) = parse_path_and_query("/");
        assert_eq!(p2, "/");
        assert_eq!(param_milestone(&params2), None);
    }

    #[test]
    fn route_serves_index_and_assets() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise")).unwrap();

        let idx = route(tmp.path(), "/");
        assert_eq!(idx.status, 200);
        assert!(idx.content_type.starts_with("text/html"));
        assert!(idx.body.contains("<html") || idx.body.contains("<!doctype") || idx.body.contains("<!DOCTYPE"));

        let js = route(tmp.path(), "/static/app.js");
        assert_eq!(js.status, 200);
        assert!(js.content_type.contains("javascript"));

        let css = route(tmp.path(), "/static/style.css");
        assert!(css.content_type.contains("css"));
    }

    #[test]
    fn route_api_returns_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise").join("milestones")).unwrap();
        let r = route(tmp.path(), "/api/milestones");
        assert_eq!(r.status, 200);
        assert!(r.content_type.contains("json"));
        assert!(r.body.contains("milestones"));
    }

    #[test]
    fn route_unknown_is_404() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(route(tmp.path(), "/nope").status, 404);
    }

    #[test]
    fn try_bind_detects_port_conflict() {
        // M35: 같은 포트 이중 바인딩 — 첫째 성공, 둘째 None(공존 경로)
        let first = try_bind(17961).expect("첫 바인딩 성공");
        assert!(try_bind(17961).is_none(), "사용 중인 포트는 None");
        drop(first);
    }

    #[test]
    fn serve_in_background_responds_over_http() {
        // M35: 백그라운드 기동 후 실제 HTTP 응답 (detached 스레드)
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise")).unwrap();

        match serve_in_background(tmp.path(), 17962) {
            ServeOutcome::Started(url) => {
                let resp = ureq::get(&format!("{}/api/live", url)).call().expect("HTTP 응답");
                assert_eq!(resp.status(), 200);
                let body = resp.into_string().unwrap();
                assert!(body.contains("run_active"));
            }
            other => panic!("Started여야 함: {:?}", other),
        }
        // 자동 등록된 tempdir를 사용자 레지스트리에서 정리 (테스트 부작용 방지)
        let _ = registry::unregister(tmp.path());
        // 비-프로젝트 디렉터리는 Failed
        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            serve_in_background(empty.path(), 17963),
            ServeOutcome::Failed(_)
        ));
    }

    #[test]
    fn projects_list_filters_stale_entries() {
        // M35: 소멸 프로젝트는 목록에서 숨김 (registry::list_existing)
        let tmp = tempfile::tempdir().unwrap();
        let alive = tmp.path().join("alive");
        std::fs::create_dir_all(alive.join(".porpoise")).unwrap();
        let reg = registry::Registry {
            projects: vec![
                registry::ProjectEntry {
                    id: "a".into(),
                    name: "alive".into(),
                    path: alive.to_string_lossy().to_string(),
                },
                registry::ProjectEntry {
                    id: "g".into(),
                    name: "ghost".into(),
                    path: tmp.path().join("ghost").to_string_lossy().to_string(),
                },
            ],
        };
        let existing = registry::list_existing(&reg);
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].name, "alive");
    }

    #[test]
    fn route_rejects_unregistered_project_id() {
        // M32 보안 경계: 미등록 project id는 데이터 API 전체에서 404 (허용 목록 강제)
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".porpoise")).unwrap();
        for url in [
            "/api/report?project=0000000000000000",
            "/api/tasks?project=0000000000000000",
            "/api/milestones?project=0000000000000000",
            "/api/live?project=0000000000000000",
        ] {
            let r = route(tmp.path(), url);
            assert_eq!(r.status, 404, "{} 는 404여야 함", url);
        }
        // 미지정은 기존 동작(기동 디렉터리) — 200
        assert_eq!(route(tmp.path(), "/api/tasks").status, 200);
    }
}
