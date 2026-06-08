//! 의존성 그래프 스케줄링 (M24) — ready-set 계산 + 순환·dangling 검증.
//!
//! task가 `dependencies`(선행 task id)를 가질 때, conductor가 **독립(ready) task만** 배치하도록
//! 한다. ready = 미완료이면서 모든 선행이 완료된 task. 모든 함수는 순수 함수라 테스트가 용이하다.

use std::collections::{HashMap, HashSet};

use crate::orchestrator::state::Task;

/// pending(미완료) task 중 **모든 선행이 완료된** ready task만 입력 순서대로 반환한다.
pub fn ready_tasks(pending: &[Task], completed_ids: &HashSet<String>) -> Vec<Task> {
    // 존재하는 모든 task id (pending + completed). dangling(존재하지 않는) 의존성은 영원히
    // 완료될 수 없으므로 '만족된 것'으로 취급한다 — validate_dependencies의 "무시됨" 안내와 일치시켜
    // 오타 의존성이 task를 영구 차단하지 않게 한다.
    let all_ids: HashSet<&str> = pending
        .iter()
        .map(|t| t.id.as_str())
        .chain(completed_ids.iter().map(|s| s.as_str()))
        .collect();
    pending
        .iter()
        .filter(|t| {
            t.dependencies
                .iter()
                .all(|d| completed_ids.contains(d) || !all_ids.contains(d.as_str()))
        })
        .cloned()
        .collect()
}

/// 존재하지 않는 task id를 가리키는 의존성(dangling)을 (task_id, missing_dep)로 반환한다.
pub fn dangling_deps(tasks: &[Task]) -> Vec<(String, String)> {
    let ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let mut out = Vec::new();
    for t in tasks {
        for d in &t.dependencies {
            if !ids.contains(d.as_str()) {
                out.push((t.id.clone(), d.clone()));
            }
        }
    }
    out
}

/// 의존성 그래프에 순환(자기 의존 포함)이 있으면 true. (DFS white/gray/black)
pub fn has_cycle(tasks: &[Task]) -> bool {
    let ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let adj: HashMap<&str, Vec<&str>> = tasks
        .iter()
        .map(|t| {
            let deps: Vec<&str> = t
                .dependencies
                .iter()
                .map(|d| d.as_str())
                .filter(|d| ids.contains(d)) // dangling은 순환 판정에서 제외
                .collect();
            (t.id.as_str(), deps)
        })
        .collect();

    let mut state: HashMap<&str, u8> = HashMap::new(); // 0=white,1=gray,2=black
    for t in tasks {
        if dfs_cycle(t.id.as_str(), &adj, &mut state) {
            return true;
        }
    }
    false
}

fn dfs_cycle<'a>(node: &'a str, adj: &HashMap<&'a str, Vec<&'a str>>, state: &mut HashMap<&'a str, u8>) -> bool {
    match state.get(node) {
        Some(2) => return false, // 이미 처리됨
        Some(1) => return true,  // gray 재방문 → 순환
        _ => {}
    }
    state.insert(node, 1);
    if let Some(deps) = adj.get(node) {
        for d in deps {
            if dfs_cycle(d, adj, state) {
                return true;
            }
        }
    }
    state.insert(node, 2);
    false
}

/// 의존성 그래프를 검증한다. 순환이면 Err(거부). dangling은 경고 메시지로 반환(Ok).
pub fn validate_dependencies(tasks: &[Task]) -> Result<Vec<String>, String> {
    if has_cycle(tasks) {
        return Err("의존성 그래프에 순환(cycle)이 있습니다. (deps:)를 확인하세요 — 무한 대기를 방지하기 위해 거부합니다.".to_string());
    }
    let warnings: Vec<String> = dangling_deps(tasks)
        .into_iter()
        .map(|(t, d)| format!("{}의 의존성 '{}'이 존재하지 않습니다 (무시됨)", t, d))
        .collect();
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, deps: &[&str]) -> Task {
        Task {
            id: id.to_string(),
            title: format!("{} 작업", id),
            completed: false,
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn completed(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ready_includes_no_dep_tasks() {
        let pending = vec![task("M1-T01", &[]), task("M1-T02", &[])];
        let ready = ready_tasks(&pending, &completed(&[]));
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn ready_excludes_unmet_deps() {
        // T02는 T01에 의존 — T01 미완료면 T02는 ready 아님
        let pending = vec![task("M1-T01", &[]), task("M1-T02", &["M1-T01"])];
        let ready = ready_tasks(&pending, &completed(&[]));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "M1-T01");
    }

    #[test]
    fn ready_includes_when_deps_completed() {
        // T01 완료 → T02 ready
        let pending = vec![task("M1-T02", &["M1-T01"])];
        let ready = ready_tasks(&pending, &completed(&["M1-T01"]));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "M1-T02");
    }

    #[test]
    fn no_cycle_in_dag() {
        let tasks = vec![task("M1-T01", &[]), task("M1-T02", &["M1-T01"]), task("M1-T03", &["M1-T01", "M1-T02"])];
        assert!(!has_cycle(&tasks));
        assert!(validate_dependencies(&tasks).is_ok());
    }

    #[test]
    fn detects_cycle() {
        let tasks = vec![task("M1-T01", &["M1-T02"]), task("M1-T02", &["M1-T01"])];
        assert!(has_cycle(&tasks));
        assert!(validate_dependencies(&tasks).is_err());
    }

    #[test]
    fn detects_self_dependency_as_cycle() {
        let tasks = vec![task("M1-T01", &["M1-T01"])];
        assert!(has_cycle(&tasks));
    }

    #[test]
    fn ready_treats_dangling_dep_as_satisfied() {
        // dangling 의존(존재하지 않는 M1-T99)은 차단하지 않음 — validate의 "무시됨"과 일치
        let pending = vec![task("M1-T02", &["M1-T99"])];
        let ready = ready_tasks(&pending, &completed(&[]));
        assert_eq!(ready.len(), 1, "dangling 의존성은 task를 차단하지 않아야 함");
    }

    #[test]
    fn dangling_dep_detected_and_warned_not_errored() {
        let tasks = vec![task("M1-T02", &["M1-T99"])]; // M1-T99 없음
        assert_eq!(dangling_deps(&tasks).len(), 1);
        // 순환 아니므로 validate는 Ok(경고)
        let warnings = validate_dependencies(&tasks).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("M1-T99"));
    }
}
