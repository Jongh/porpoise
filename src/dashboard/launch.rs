//! 함대 실행 백엔드 (M37) — `POST /api/launch`.
//!
//! 대시보드가 conductor 런을 **직접 기동**한다. M35까지는 conductor가 대시보드를 내장
//! 기동(serve_in_background)했으나, 런처는 그 방향을 역전한다 — 독립 실행 중인 대시보드가
//! `porpoise` 자식 프로세스를 spawn해 **프로세스 수명을 소유**한다.
//!
//! 설계:
//! - **detached spawn**: stdin=null, stdout/stderr → `.porpoise/launch.log`. 새 프로세스 그룹으로
//!   띄워 대시보드를 닫아도(또는 터미널 Ctrl-C) 런이 죽지 않는다.
//! - **런 락**: live.json `run_active` + 신선한 `.porpoise/run.lock`(spawn~live::start 사이 공백
//!   보호, 시간 기반 자가 만료). 이미 실행 중이면 409.
//! - **공존**: spawn된 conductor가 gate 모드면 자기 대시보드를 띄우려다 PortInUse로 기존(런처)
//!   대시보드와 공존(M35 경로). 새 코드 불필요.
//! - 보안: M33 Origin 검증·M32 프로젝트 스코프(라우터에서 해석)를 상속한다.

use std::path::{Path, PathBuf};

use crate::dashboard::control::origin_allowed;

/// run.lock을 "실행 중"으로 간주하는 신선도(초). spawn 직후 자식이 live::start를 호출하기
/// 전까지의 공백을 덮는다. 이보다 오래된 락은 stale로 보고 무시(자가 만료).
pub const LOCK_FRESH_SECS: i64 = 30;

/// 실행 결과 (상태코드 + 본문).
pub struct LaunchOutcome {
    pub status: u16,
    pub body: String,
}

fn outcome(status: u16, body: &str) -> LaunchOutcome {
    LaunchOutcome { status, body: body.to_string() }
}

fn run_lock_path(project: &Path) -> PathBuf {
    project.join(".porpoise").join("run.lock")
}

fn launch_log_path(project: &Path) -> PathBuf {
    project.join(".porpoise").join("launch.log")
}

/// live.json 기준 실행 중 여부.
fn live_run_active(project: &Path) -> bool {
    crate::conductor::live::load(project).map(|s| s.run_active).unwrap_or(false)
}

/// run.lock의 타임스탬프(rfc3339, 첫 줄)를 파싱한다 (순수).
pub fn parse_lock_time(content: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let first = content.lines().next()?.trim();
    chrono::DateTime::parse_from_rfc3339(first).ok()
}

/// 락이 신선한가 (지금으로부터 fresh_secs 이내) — 순수 판정.
pub fn lock_is_fresh(
    lock_time: chrono::DateTime<chrono::FixedOffset>,
    now: chrono::DateTime<chrono::Local>,
    fresh_secs: i64,
) -> bool {
    let age = now.signed_duration_since(lock_time).num_seconds();
    (0..fresh_secs).contains(&age) || age < 0 // 미래 타임스탬프(시계 차)는 신선으로 간주
}

/// 지금 런을 띄울 수 없는 상태인가 — live.json run_active 또는 신선한 run.lock.
fn launch_blocked(project: &Path) -> bool {
    if live_run_active(project) {
        return true;
    }
    let Ok(content) = std::fs::read_to_string(run_lock_path(project)) else {
        return false;
    };
    match parse_lock_time(&content) {
        Some(t) => lock_is_fresh(t, chrono::Local::now(), LOCK_FRESH_SECS),
        None => false, // 손상 락은 무시
    }
}

/// run.lock에 시작 시각·PID를 기록한다 (정보·공백 보호용).
fn write_run_lock(project: &Path, pid: u32) {
    let content = format!("{}\npid={}\n", chrono::Local::now().to_rfc3339(), pid);
    let _ = std::fs::write(run_lock_path(project), content);
}

/// 요청 본문에서 `--yes`(자동 승인) 여부를 해석한다 (순수). 빈 본문·손상이면 false(게이트 대기).
pub fn parse_yes(body: &str) -> bool {
    if body.trim().is_empty() {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(body.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|v| v.get("yes").and_then(|y| y.as_bool()))
        .unwrap_or(false)
}

/// conductor 프로세스를 detached로 기동한다. 성공 시 자식 PID.
/// 플랫폼 detach 분기는 여기로 격리한다 (단위 테스트 대상 외 — 통합 하니스로 검증).
fn spawn_conductor(project: &Path, yes: bool) -> std::io::Result<u32> {
    let exe = std::env::current_exe()?;
    let log = std::fs::File::create(launch_log_path(project))?;
    let err = log.try_clone()?;

    let mut cmd = std::process::Command::new(exe);
    cmd.current_dir(project)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(err));
    if yes {
        cmd.arg("--yes");
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 새 프로세스 그룹 + 콘솔 없음 + 분리 — 부모(대시보드) 종료·Ctrl-C와 수명 분리.
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 새 프로세스 그룹 — 부모의 터미널 시그널(Ctrl-C 등)이 자식에 전파되지 않게.
        cmd.process_group(0);
    }

    let child = cmd.spawn()?;
    Ok(child.id())
}

/// 실행 요청을 처리한다 — 검증 후 conductor 프로세스를 detached spawn.
/// `project`는 라우터가 이미 스코프 해석한 경로(등록된 프로젝트만 도달).
pub fn handle_launch(project: &Path, body: &str, origin: Option<&str>) -> LaunchOutcome {
    if !origin_allowed(origin) {
        return outcome(403, r#"{"error":"forbidden origin"}"#);
    }
    if !project.join(".porpoise").exists() {
        return outcome(404, r#"{"error":"not a porpoise project"}"#);
    }
    if launch_blocked(project) {
        return outcome(409, r#"{"error":"a run is already active"}"#);
    }
    let yes = parse_yes(body);
    // 락을 spawn **전에** 선점해 동시 요청의 TOCTOU 창을 닫는다 (두 번째 요청은 신선한 락을
    // 보고 409). spawn 실패 시 선점 락을 되돌린다.
    write_run_lock(project, 0);
    match spawn_conductor(project, yes) {
        Ok(pid) => {
            write_run_lock(project, pid);
            outcome(200, &format!(r#"{{"ok":true,"pid":{},"log":".porpoise/launch.log"}}"#, pid))
        }
        Err(e) => {
            let _ = std::fs::remove_file(run_lock_path(project)); // 선점 락 롤백
            let msg = e.to_string().replace('"', "'");
            outcome(500, &format!(r#"{{"error":"spawn failed: {}"}}"#, msg))
        }
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
    fn parse_yes_variants() {
        assert!(!parse_yes(""));
        assert!(!parse_yes("   "));
        assert!(parse_yes(r#"{"yes":true}"#));
        assert!(!parse_yes(r#"{"yes":false}"#));
        assert!(!parse_yes(r#"{}"#));
        assert!(!parse_yes("garbage"));
    }

    #[test]
    fn parse_lock_time_reads_first_line() {
        let now = chrono::Local::now();
        let content = format!("{}\npid=1234\n", now.to_rfc3339());
        assert!(parse_lock_time(&content).is_some());
        assert!(parse_lock_time("not a time").is_none());
        assert!(parse_lock_time("").is_none());
    }

    #[test]
    fn lock_freshness() {
        let now = chrono::Local::now();
        let fresh = (now - chrono::Duration::seconds(5)).fixed_offset();
        let stale = (now - chrono::Duration::seconds(120)).fixed_offset();
        assert!(lock_is_fresh(fresh, now, LOCK_FRESH_SECS), "5초 전 락은 신선");
        assert!(!lock_is_fresh(stale, now, LOCK_FRESH_SECS), "120초 전 락은 stale");
        // 미래 타임스탬프(시계 차)는 신선으로 간주
        let future = (now + chrono::Duration::seconds(5)).fixed_offset();
        assert!(lock_is_fresh(future, now, LOCK_FRESH_SECS));
    }

    #[test]
    fn rejects_forbidden_origin() {
        let tmp = project_dir();
        let r = handle_launch(tmp.path(), "", Some("http://evil.example.com"));
        assert_eq!(r.status, 403);
    }

    #[test]
    fn rejects_non_project() {
        let tmp = tempfile::tempdir().unwrap(); // .porpoise 없음
        let r = handle_launch(tmp.path(), "", None);
        assert_eq!(r.status, 404);
    }

    #[test]
    fn blocks_when_live_run_active() {
        let tmp = project_dir();
        // run_active=true 상태를 만들면 409
        crate::conductor::live::start(tmp.path(), "sequential", None);
        let r = handle_launch(tmp.path(), "", None);
        assert_eq!(r.status, 409, "실행 중이면 이중 기동 차단");
    }

    #[test]
    fn blocks_when_fresh_lock_present() {
        let tmp = project_dir();
        // 신선한 run.lock만 있어도(live는 아직 idle) 409 — spawn~live::start 공백 보호
        write_run_lock(tmp.path(), 9999);
        assert!(launch_blocked(tmp.path()));
        let r = handle_launch(tmp.path(), "", None);
        assert_eq!(r.status, 409);
    }

    #[test]
    fn stale_lock_does_not_block() {
        let tmp = project_dir();
        // 오래된 타임스탬프 락은 무시되어야 함
        let old = (chrono::Local::now() - chrono::Duration::seconds(120)).to_rfc3339();
        std::fs::write(run_lock_path(tmp.path()), format!("{}\npid=1\n", old)).unwrap();
        assert!(!launch_blocked(tmp.path()), "stale 락은 차단하지 않음");
    }
}
