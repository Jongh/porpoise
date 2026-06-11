//! 적응형 재계획 (M39, 옵트인) — 반복 정지(halt)한 task를 더 작은 하위 task로 자동 분할한다.
//!
//! 재투입(M37)이 "예산만 늘리는" 회복인 반면, 재계획은 반복 실패의 원인(너무 큰 단위)을
//! 손본다: 검증 피드백을 근거로 LLM이 2~4개 하위 task를 제안하고, project.md에서 부모를
//! `[분할됨]` 완료로 치환한 뒤 하위 task(`{parent}-S{k}`)를 추가한다. 하위 task는 순차 deps로
//! 체인되어(같은 부모 작업의 조각이 서로 충돌하지 않게) 일반 루프가 이어받는다.
//!
//! 무한 분할 방지: `-S` 접미(이미 하위 task)는 재분할하지 않는다(깊이 1). 옵트인 설정
//! `[conductor] auto_replan`이 꺼져 있거나 제안이 실패하면 호출자는 일반 파킹으로 폴백한다.

use std::path::Path;

use anyhow::{bail, Result};

use crate::claude::runner::ClaudeRunner;
use crate::logger::Logger;

/// 하위 task로 더 분할 가능한가 — `-S` 접미(이미 하위 task)면 불가 (깊이 1 제한).
pub fn is_replannable(task_id: &str) -> bool {
    !task_id.contains("-S")
}

/// LLM 응답에서 하위 task 제목 목록을 추출한다 (순수). 첫 JSON 문자열 배열을 파싱해
/// 비어 있지 않은 항목만, 2~4개로 클램프한다. 2개 미만이면 None(분할 폐기).
pub fn parse_subtasks(output: &str) -> Option<Vec<String>> {
    let start = output.find('[')?;
    let end = output[start..].rfind(']')? + start;
    let arr: Vec<String> = serde_json::from_str(&output[start..=end]).ok()?;
    let cleaned: Vec<String> = arr
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if cleaned.len() < 2 {
        return None;
    }
    Some(cleaned.into_iter().take(4).collect())
}

fn replan_prompt(parent_id: &str, parent_title: &str, feedback: &str) -> String {
    format!(
        "한 작업이 검증을 반복 실패해 자동 재계획이 필요합니다.\n\n\
         실패한 작업: {parent_id} — {parent_title}\n\n\
         검증자 피드백:\n{feedback}\n\n\
         첨부된 project.md의 맥락을 참고해, 이 작업을 **한 번에 끝낼 수 있는 더 작은 하위 작업 \
         2~4개**로 분할하세요. 각 하위 작업은 순서대로 수행됩니다(뒤 작업이 앞 작업 위에 쌓임).\n\n\
         출력은 **하위 작업 제목의 JSON 문자열 배열만** 내보내세요. 예: [\"파서에 X 필드 추가\", \
         \"X를 직렬화에 반영\", \"X 왕복 테스트\"]. 다른 설명은 쓰지 마세요.",
    )
}

/// LLM에 분할을 요청한다. project.md를 컨텍스트로 첨부해 마일스톤 목표·전체 task를 보게 한다.
/// 실패(호출 오류·파싱 실패·2개 미만)면 Err — 호출자는 일반 파킹으로 폴백한다.
pub fn propose_subtasks(
    runner: &ClaudeRunner,
    path: &Path,
    parent_id: &str,
    parent_title: &str,
    feedback: &str,
    model: Option<&str>,
    logger: &Logger,
) -> Result<Vec<String>> {
    let project_md = path.join(".porpoise").join("project.md");
    let context = if project_md.exists() { vec![project_md] } else { vec![] };
    let prompt = replan_prompt(parent_id, parent_title, feedback);
    let out = runner
        .run_with_prompt_str(&prompt, &context, None, model)
        .map_err(|e| anyhow::anyhow!("재계획 LLM 호출 실패: {}", e))?;
    match parse_subtasks(&out) {
        Some(subs) => {
            logger.info("conductor", &format!("재계획 제안 {}개 하위 task: {}", subs.len(), parent_id));
            Ok(subs)
        }
        None => bail!("재계획 제안 파싱 실패 또는 2개 미만"),
    }
}

/// 하위 task id를 생성한다 (`{parent}-S{k}`, 1-기반). (순수)
pub fn subtask_ids(parent_id: &str, count: usize) -> Vec<String> {
    (1..=count).map(|k| format!("{}-S{}", parent_id, k)).collect()
}

/// project.md에서 부모를 `[분할됨]` 완료로 치환하고 하위 task를 순차 체인으로 추가한다.
/// 반환: 추가된 하위 task id 목록. 부모 미완료 마커가 없으면 Err(호출자는 폴백).
pub fn insert_subtasks(
    path: &Path,
    parent_id: &str,
    subtasks: &[String],
) -> Result<Vec<String>> {
    let project_md = path.join(".porpoise").join("project.md");
    let mut content = std::fs::read_to_string(&project_md)
        .map_err(|e| anyhow::anyhow!("project.md 읽기 실패: {}", e))?;

    let marker = format!("- [ ] {}:", parent_id);
    if !content.contains(&marker) {
        bail!("부모 미완료 마커를 찾을 수 없음: {}", parent_id);
    }

    let ids = subtask_ids(parent_id, subtasks.len());
    // 부모 줄을 완료(+분할 표식)로 치환 — 콜론 이후의 원래 제목은 보존된다(접두만 교체).
    let tag = ids
        .iter()
        .map(|i| i.rsplit('-').next().unwrap_or(i.as_str()))
        .collect::<Vec<_>>()
        .join(",");
    let replacement = format!("- [x] {}: [분할→{}]", parent_id, tag);
    content = content.replace(&marker, &replacement);

    // 하위 task 블록 추가 (순차 deps 체인 — 같은 부모 조각의 충돌 방지)
    let mut block = String::from("\n");
    for (k, (id, title)) in ids.iter().zip(subtasks).enumerate() {
        let deps = if k == 0 {
            String::new()
        } else {
            format!(" (deps: {})", ids[k - 1])
        };
        block.push_str(&format!("- [ ] {}: {}{}\n", id, title, deps));
    }
    content.push_str(&block);

    crate::utils::fs::write_file(&project_md, &content, path)
        .map_err(|e| anyhow::anyhow!("project.md 쓰기 실패: {}", e))?;
    Ok(ids)
}

/// 정지 task를 분할 시도한다 (propose → insert). 성공 시 추가된 하위 task id, 실패 시 Err.
/// 호출자(파킹 경로)는 Err면 일반 파킹으로 폴백한다.
#[allow(clippy::too_many_arguments)]
pub fn try_replan(
    runner: &ClaudeRunner,
    path: &Path,
    parent_id: &str,
    parent_title: &str,
    feedback: &str,
    model: Option<&str>,
    logger: &Logger,
) -> Result<Vec<String>> {
    if !is_replannable(parent_id) {
        bail!("하위 task는 재분할하지 않음(깊이 1): {}", parent_id);
    }
    let subs = propose_subtasks(runner, path, parent_id, parent_title, feedback, model, logger)?;
    insert_subtasks(path, parent_id, &subs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_replannable_rejects_subtasks() {
        assert!(is_replannable("M39-T03"));
        assert!(!is_replannable("M39-T03-S1"), "하위 task는 재분할 불가");
    }

    #[test]
    fn parse_subtasks_extracts_and_clamps() {
        // 정상 — 산문에 섞인 JSON 배열 추출
        let out = "분할 결과입니다:\n[\"A 추가\", \"B 반영\", \"C 테스트\"]\n끝.";
        assert_eq!(parse_subtasks(out).unwrap().len(), 3);
        // 4개 초과 → 4개로 클램프
        let five = r#"["a","b","c","d","e"]"#;
        assert_eq!(parse_subtasks(five).unwrap().len(), 4);
        // 2개 미만 → None
        assert!(parse_subtasks(r#"["only one"]"#).is_none());
        // 빈 항목 제거 후 2개 미만 → None
        assert!(parse_subtasks(r#"["a","  "]"#).is_none());
        // 배열 없음 → None
        assert!(parse_subtasks("no array here").is_none());
    }

    #[test]
    fn subtask_ids_format() {
        assert_eq!(subtask_ids("M39-T03", 2), vec!["M39-T03-S1", "M39-T03-S2"]);
    }

    #[test]
    fn insert_subtasks_replaces_parent_and_appends_chain() {
        let tmp = tempfile::tempdir().unwrap();
        let porpoise = tmp.path().join(".porpoise");
        std::fs::create_dir_all(&porpoise).unwrap();
        std::fs::write(
            porpoise.join("project.md"),
            "## 작업 목록\n- [ ] M39-T03: 큰 작업\n- [ ] M39-T04: 다른 작업\n",
        )
        .unwrap();

        let ids =
            insert_subtasks(tmp.path(), "M39-T03", &["조각1".into(), "조각2".into()]).unwrap();
        assert_eq!(ids, vec!["M39-T03-S1", "M39-T03-S2"]);

        let content = std::fs::read_to_string(porpoise.join("project.md")).unwrap();
        // 부모는 완료(+분할 표식), 원 제목 보존
        assert!(content.contains("- [x] M39-T03: [분할→S1,S2] 큰 작업"), "부모 치환:\n{}", content);
        // 하위 task 추가 + 순차 deps 체인
        assert!(content.contains("- [ ] M39-T03-S1: 조각1\n"));
        assert!(content.contains("- [ ] M39-T03-S2: 조각2 (deps: M39-T03-S1)\n"));
        // 다른 task는 무변경
        assert!(content.contains("- [ ] M39-T04: 다른 작업"));

        // 파서 왕복 — 추가된 하위 task가 정상 파싱 + deps 인식
        let tasks = crate::orchestrator::state::parse_tasks_from_project_md(tmp.path());
        let s2 = tasks.iter().find(|t| t.id == "M39-T03-S2").expect("S2 파싱");
        assert_eq!(s2.dependencies, vec!["M39-T03-S1"]);
        let parent = tasks.iter().find(|t| t.id == "M39-T03").unwrap();
        assert!(parent.completed, "부모는 완료로 파싱");
    }

    #[test]
    fn insert_subtasks_bails_when_parent_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let porpoise = tmp.path().join(".porpoise");
        std::fs::create_dir_all(&porpoise).unwrap();
        std::fs::write(porpoise.join("project.md"), "- [ ] M39-T99: 다른 것\n").unwrap();
        assert!(insert_subtasks(tmp.path(), "M39-T03", &["a".into(), "b".into()]).is_err());
    }
}
