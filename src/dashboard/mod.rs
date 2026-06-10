//! 로컬 웹 대시보드 (M30) — read-only 관측. tiny_http + 임베디드 프론트엔드.
//!
//! `porpoise dashboard`로 로컬 서버를 띄워 `.porpoise/` 데이터(리포트·비용·의존성 그래프)를
//! 브라우저에서 본다. conductor 로직은 건드리지 않는다(파일 쓰기 없음).

pub mod api;

use std::path::Path;

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

/// 요청 URL을 라우팅한다 (순수 — 파일은 읽되 쓰지 않음).
pub fn route(path: &Path, url: &str) -> RouteResponse {
    let (p, params) = parse_path_and_query(url);
    match p.as_str() {
        "/" | "/index.html" => html(INDEX_HTML),
        "/static/app.js" => js(APP_JS),
        "/static/chart.js" => js(CHART_JS),
        "/static/style.css" => RouteResponse {
            status: 200,
            content_type: "text/css; charset=utf-8",
            body: STYLE_CSS.to_string(),
        },
        "/api/milestones" => json_resp(api::milestones_json(path)),
        "/api/report" => json_resp(api::report_json(path, param_milestone(&params))),
        "/api/tasks" => json_resp(api::tasks_json(path)),
        _ => not_found(),
    }
}

/// 대시보드 서버를 기동한다 (블로킹). Ctrl-C로 종료.
pub fn run_dashboard(path: &Path, port: u16, open: bool) -> Result<()> {
    if !path.join(".porpoise").exists() {
        anyhow::bail!(
            ".porpoise/ 가 없습니다. porpoise 프로젝트 디렉터리에서 실행하세요."
        );
    }
    let addr = format!("127.0.0.1:{}", port);
    let server = tiny_http::Server::http(&addr)
        .map_err(|e| anyhow::anyhow!("대시보드 서버 기동 실패 ({}): {} — 다른 --port를 시도하세요.", addr, e))?;

    let url = format!("http://{}", addr);
    println!();
    println!("{} {}", "▶ Porpoise 대시보드".green().bold(), url.cyan());
    println!("{}", "  read-only 관측 — Ctrl-C로 종료".dimmed());

    if open {
        if let Err(e) = webbrowser::open(&url) {
            eprintln!("  ⚠ 브라우저 자동 열기 실패: {} (위 URL을 직접 여세요)", e);
        }
    }

    for request in server.incoming_requests() {
        let r = route(path, request.url());
        let header =
            tiny_http::Header::from_bytes(&b"Content-Type"[..], r.content_type.as_bytes())
                .expect("유효한 헤더");
        let response = tiny_http::Response::from_string(r.body)
            .with_status_code(r.status)
            .with_header(header);
        let _ = request.respond(response);
    }
    Ok(())
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
}
