//! 대시보드 JSON API (M30) — 기존 순수 함수를 read-only로 JSON 노출한다.
//!
//! 새 데이터를 만들지 않는다: `report::build_report`·`schedule::ready_tasks`·
//! `parse_tasks_from_project_md`를 그대로 재사용해 직렬화만 한다.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{json, Value};

use crate::conductor::report::{build_report, latest_run_records, load_records};
use crate::conductor::schedule;
use crate::orchestrator::state::parse_tasks_from_project_md;

/// `GET /api/milestones` → `{ "milestones": [{number, title}, ...] }` (최신 번호 우선).
pub fn milestones_json(path: &Path) -> Value {
    let dir = path.join(".porpoise").join("milestones");
    let mut items: Vec<(u32, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // 정식 마일스톤 M{N}.md 만 (하이픈 포함 변형 제외)
            if let Some(stem) = name.strip_prefix('M').and_then(|s| s.strip_suffix(".md")) {
                if let Ok(n) = stem.parse::<u32>() {
                    let title = read_milestone_title(&e.path()).unwrap_or_default();
                    items.push((n, title));
                }
            }
        }
    }
    items.sort_by_key(|x| std::cmp::Reverse(x.0));
    let arr: Vec<Value> = items
        .into_iter()
        .map(|(n, t)| json!({ "number": n, "title": t }))
        .collect();
    json!({ "milestones": arr })
}

/// "# M30: 제목 (v0.27.0)" 첫 줄에서 제목만 추출.
fn read_milestone_title(p: &Path) -> Option<String> {
    let content = std::fs::read_to_string(p).ok()?;
    let line = content.lines().next()?;
    let rest = line.strip_prefix("# M")?;
    let colon = rest.find(": ")?;
    let title = rest[colon + 2..].trim();
    // "(vX.Y.Z)" 접미사 제거
    if let Some(par) = title.rfind(" (") {
        let after = &title[par + 2..];
        if after.ends_with(')') && after.starts_with('v') {
            return Some(title[..par].to_string());
        }
    }
    Some(title.to_string())
}

/// `GET /api/report?milestone=N` → 롤업 포함 실행 리포트. 미지정 시 최신 마일스톤.
pub fn report_json(path: &Path, milestone: Option<u32>) -> Value {
    let report = build_report(path, milestone);
    let tasks = serde_json::to_value(&report.tasks).unwrap_or(Value::Null);
    json!({
        "milestone": report.milestone,
        "total": report.total(),
        "passed": report.passed(),
        "failed": report.failed(),
        "success_rate": report.success_rate(),
        "total_redispatches": report.total_redispatches(),
        "fallback_count": report.fallback_count(),
        "total_cost": report.total_cost(),
        // M40: 총비용을 dispatch / verifier로 분리 노출
        "total_dispatch_cost": report.total_dispatch_cost(),
        "total_verifier_cost": report.total_verifier_cost(),
        "total_input_tokens": report.total_input_tokens(),
        "total_output_tokens": report.total_output_tokens(),
        "parse_errors": report.parse_errors,
        "tasks": tasks,
    })
}

/// 상세 본문의 응답 트렁케이트 한도 (전송량 절제).
const DETAIL_MAX_CHARS: usize = 2000;

fn truncate_chars(s: &str, max: usize) -> String {
    let total = s.chars().count();
    if total <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{}…[{} chars 생략]", head, total - max)
    }
}

/// `GET /api/task?id=M1-T03` → 해당 task **최신 run**(M27 규칙)의 라운드별 상세 (M36).
/// 감사 기록에 이미 저장된 에이전트 보고·검증 피드백의 노출 — 새 수집 0, read-only.
pub fn task_detail_json(path: &Path, task_id: &str) -> Value {
    let (records, _) = load_records(path);
    let rounds: Vec<Value> = latest_run_records(&records, task_id)
        .iter()
        .map(|r| {
            json!({
                "redispatch": r.redispatch,
                "timestamp": r.timestamp,
                "verdict": r.verdict,
                "fallback_used": r.fallback_used,
                "diff_lines": r.diff_lines,
                "cost_usd": r.cost_usd,
                "verifier_cost_usd": r.verifier_cost_usd,
                "feedback": truncate_chars(&r.feedback, DETAIL_MAX_CHARS),
                "dispatch_output": truncate_chars(&r.dispatch_output, DETAIL_MAX_CHARS),
                "verifier_raw": truncate_chars(&r.verifier_raw, DETAIL_MAX_CHARS),
            })
        })
        .collect();
    json!({ "task_id": task_id, "rounds": rounds })
}

/// `GET /api/tasks` → 현재(project.md) 태스크 + 의존성 + 상태(ready/waiting/done).
pub fn tasks_json(path: &Path) -> Value {
    let tasks = parse_tasks_from_project_md(path);
    let completed_ids: HashSet<String> = tasks
        .iter()
        .filter(|t| t.completed)
        .map(|t| t.id.clone())
        .collect();
    let pending: Vec<_> = tasks.iter().filter(|t| !t.completed).cloned().collect();
    let ready_ids: HashSet<String> = schedule::ready_tasks(&pending, &completed_ids)
        .into_iter()
        .map(|t| t.id)
        .collect();

    let arr: Vec<Value> = tasks
        .iter()
        .map(|t| {
            let status = if t.completed {
                "done"
            } else if ready_ids.contains(&t.id) {
                "ready"
            } else {
                "waiting"
            };
            json!({
                "id": t.id,
                "title": t.title,
                "completed": t.completed,
                "dependencies": t.dependencies,
                "status": status,
            })
        })
        .collect();
    json!({ "tasks": arr })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(path: &Path) {
        let porpoise = path.join(".porpoise");
        std::fs::create_dir_all(porpoise.join("milestones")).unwrap();
        std::fs::create_dir_all(porpoise.join("sessions")).unwrap();
        std::fs::write(
            porpoise.join("milestones").join("M30.md"),
            "# M30: 웹 대시보드 (v0.27.0)\n",
        )
        .unwrap();
        std::fs::write(
            porpoise.join("project.md"),
            "# proj\n\n## 작업 목록\n- [x] M30-T01: 서버\n- [ ] M30-T02: API (deps: M30-T01)\n- [ ] M30-T03: 프론트 (deps: M30-T05)\n",
        )
        .unwrap();
        // conductor-4 감사 1건 (비용 포함)
        let rec = json!({
            "schema_version": "conductor-4", "task_id": "M30-T01", "redispatch": 0,
            "timestamp": "2026-06-09T10:00:00Z", "verdict": "PASS", "cost_usd": 0.05,
            "input_tokens": 100, "output_tokens": 50
        });
        std::fs::write(
            porpoise.join("sessions").join("M30-T01-conductor-20260609-100000-R0.json"),
            serde_json::to_string(&rec).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn milestones_json_lists_titles() {
        let tmp = tempfile::tempdir().unwrap();
        setup(tmp.path());
        let v = milestones_json(tmp.path());
        let ms = v["milestones"].as_array().unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0]["number"], 30);
        assert_eq!(ms[0]["title"], "웹 대시보드");
    }

    #[test]
    fn report_json_has_rollup_and_tasks() {
        let tmp = tempfile::tempdir().unwrap();
        setup(tmp.path());
        let v = report_json(tmp.path(), Some(30));
        assert_eq!(v["total"], 1);
        assert_eq!(v["passed"], 1);
        assert_eq!(v["total_cost"], 0.05);
        assert!(v["tasks"].is_array());
        assert_eq!(v["tasks"][0]["task_id"], "M30-T01");
    }

    #[test]
    fn task_detail_exposes_rounds_with_truncation() {
        // M36: 상세 API — 라운드별 본문 노출 + 트렁케이트
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let long_output = "에이전트 보고 ".repeat(500); // > 2000 chars
        let rec = json!({
            "schema_version": "conductor-4", "task_id": "M9-T01", "redispatch": 0,
            "timestamp": "2026-06-10T10:00:00Z", "verdict": "FAIL",
            "feedback": "테스트 누락", "dispatch_output": long_output, "verifier_raw": "{...}"
        });
        std::fs::write(
            sessions.join("M9-T01-conductor-20260610-100000-R0.json"),
            serde_json::to_string(&rec).unwrap(),
        )
        .unwrap();

        let v = task_detail_json(tmp.path(), "M9-T01");
        let rounds = v["rounds"].as_array().unwrap();
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0]["verdict"], "FAIL");
        assert_eq!(rounds[0]["feedback"], "테스트 누락");
        let out = rounds[0]["dispatch_output"].as_str().unwrap();
        assert!(out.contains("chars 생략"), "트렁케이트되어야 함");
        assert!(out.chars().count() < 2100);
        // 없는 task는 빈 배열
        assert!(task_detail_json(tmp.path(), "NOPE")["rounds"].as_array().unwrap().is_empty());
    }

    #[test]
    fn tasks_json_computes_status() {
        let tmp = tempfile::tempdir().unwrap();
        setup(tmp.path());
        let v = tasks_json(tmp.path());
        let tasks = v["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 3);
        // T01 완료 → done, T02(deps T01 완료) → ready, T03(deps M30-T05 dangling) → ready로 취급
        let by_id = |id: &str| tasks.iter().find(|t| t["id"] == id).unwrap().clone();
        assert_eq!(by_id("M30-T01")["status"], "done");
        assert_eq!(by_id("M30-T02")["status"], "ready");
        // T03 deps가 dangling(M30-T05 없음) → schedule가 satisfied 취급 → ready
        assert_eq!(by_id("M30-T03")["status"], "ready");
    }
}
