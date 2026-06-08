//! Verify — 독립 검증 단계.
//!
//! 옛 Review의 NEXT/PREV를 PASS/FAIL로 대체한다. 단, 작업을 만든 에이전트가 아니라
//! **독립된 컨텍스트**(별도 세션, 설정 시 다른 모델)가 판정한다는 점이 핵심이다.
//! 두 축으로 검증한다 — (1) 객관 증거: workspace.toml `verify_commands`/`test_command`를
//! 실제 실행, (2) 적대적 판단: diff + DoD를 받은 검증자 LLM이 "정말 완료·정확한가" 심사.
//! 테스트가 하나라도 실패하면 LLM 판단 없이 즉시 FAIL (객관 증거 우선).

use anyhow::{Context, Result};
use std::path::Path;

use crate::claude::runner::ClaudeRunner;
use crate::session::v0_7::ExecutionResult;

/// 검증 판정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub pass: bool,
    pub feedback: String,
}

impl Verdict {
    pub fn pass() -> Self {
        Verdict { pass: true, feedback: String::new() }
    }
    pub fn fail(feedback: impl Into<String>) -> Self {
        Verdict { pass: false, feedback: feedback.into() }
    }
}

/// 검증자 LLM의 원문 응답에서 PASS/FAIL 판정을 추출한다.
///
/// 우선순위: ① JSON 객체 `{"verdict":"PASS"|"FAIL","feedback":"..."}`
/// ② 폴백 — 마지막 비어있지 않은 줄의 PASS/FAIL 토큰.
/// 어느 쪽도 못 찾으면 보수적으로 FAIL.
pub fn parse_verdict(raw: &str) -> Verdict {
    if let Some(v) = parse_verdict_json(raw) {
        return v;
    }
    // 폴백: 토큰 스캔 (마지막 줄 우선)
    for line in raw.lines().rev() {
        let t = line.trim().trim_matches(|c: char| !c.is_ascii_alphabetic());
        if t.eq_ignore_ascii_case("PASS") {
            return Verdict::pass();
        }
        if t.eq_ignore_ascii_case("FAIL") {
            return Verdict::fail(extract_feedback_fallback(raw));
        }
    }
    Verdict::fail("검증자 응답에서 PASS/FAIL 판정을 찾을 수 없습니다. (보수적 FAIL)")
}

fn parse_verdict_json(raw: &str) -> Option<Verdict> {
    let json_str = extract_json_object(raw)?;
    let value: serde_json::Value = serde_json::from_str(&json_str).ok()?;
    let verdict = value.get("verdict")?.as_str()?;
    let feedback = value
        .get("feedback")
        .and_then(|f| f.as_str())
        .unwrap_or("")
        .to_string();
    match verdict.to_ascii_uppercase().as_str() {
        "PASS" => Some(Verdict::pass()),
        "FAIL" => Some(Verdict::fail(if feedback.is_empty() {
            "검증자가 FAIL을 반환했으나 사유를 명시하지 않았습니다.".to_string()
        } else {
            feedback
        })),
        _ => None,
    }
}

/// 텍스트에서 첫 번째 균형 잡힌 JSON 객체 문자열을 추출한다 (코드펜스 포함 대응).
fn extract_json_object(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let bytes = raw.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        let c = b as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(raw[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_feedback_fallback(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "검증자가 FAIL을 반환했습니다.".to_string();
    }
    trimmed.chars().take(1000).collect()
}

/// 실행 결과 목록에서 실패한 명령을 골라 사람이 읽을 요약을 만든다.
/// 실패가 없으면 None.
pub fn summarize_command_failures(results: &[ExecutionResult]) -> Option<String> {
    let failures: Vec<&ExecutionResult> = results.iter().filter(|r| r.exit_code != 0).collect();
    if failures.is_empty() {
        return None;
    }
    let mut lines = vec![format!("{}개 검증 명령이 실패했습니다:", failures.len())];
    for r in failures {
        let detail = if !r.stderr.trim().is_empty() {
            r.stderr.trim()
        } else {
            r.stdout.trim()
        };
        let snippet: String = detail.chars().take(400).collect();
        lines.push(format!(
            "- `{} {}` (exit={}): {}",
            r.command,
            r.args.join(" "),
            r.exit_code,
            snippet
        ));
    }
    Some(lines.join("\n"))
}

/// 검증자 LLM에게 전달할 프롬프트를 만든다.
pub fn build_verify_prompt(
    task_id: &str,
    task_title: &str,
    dod: &[String],
    diff: &str,
    command_results: &[ExecutionResult],
) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(
        "당신은 엄격한 독립 코드 검증자입니다. 당신은 이 코드를 작성하지 않았습니다. \
         아래 변경이 주어진 작업과 완료 기준(DoD)을 정말로 충족하는지 적대적으로 심사하세요. \
         의심스러우면 FAIL로 판정하세요 (관대함보다 엄격함을 우선)."
            .to_string(),
    );

    parts.push(format!("=== 작업 ===\n{}: {}", task_id, task_title));

    if !dod.is_empty() {
        let lines = dod.iter().map(|d| format!("- {}", d)).collect::<Vec<_>>().join("\n");
        parts.push(format!("=== 완료 기준 (DoD) ===\n{}", lines));
    }

    if !command_results.is_empty() {
        let lines = command_results
            .iter()
            .map(|r| {
                format!(
                    "- `{} {}` → exit={}",
                    r.command,
                    r.args.join(" "),
                    r.exit_code
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!("=== 검증 명령 실행 결과 ===\n{}", lines));
    }

    let diff_block = if diff.trim().is_empty() {
        "(변경된 내용이 없습니다 — 이 경우 FAIL이어야 합니다)".to_string()
    } else {
        diff.to_string()
    };
    parts.push(format!("=== 변경 내역 (git diff) ===\n{}", diff_block));

    parts.push(
        "=== 출력 형식 ===\n\
         반드시 아래 JSON 객체 하나만 출력하세요 (다른 텍스트 금지):\n\
         {\"verdict\": \"PASS\" 또는 \"FAIL\", \"feedback\": \"FAIL인 경우 수정해야 할 구체적 사항\"}"
            .to_string(),
    );

    parts.join("\n\n")
}

/// 전체 검증 수행: 실제 명령 실행 → (실패 시 즉시 FAIL) → 독립 검증자 LLM 심사.
#[allow(clippy::too_many_arguments)]
pub fn run_verification(
    worktree_path: &Path,
    task_id: &str,
    task_title: &str,
    dod: &[String],
    diff: &str,
    command_results: &[ExecutionResult],
    runner: &ClaudeRunner,
    verifier_model: Option<&str>,
) -> Result<Verdict> {
    // 변경이 전혀 없으면 작업 미수행 — 즉시 FAIL
    if diff.trim().is_empty() {
        return Ok(Verdict::fail(
            "에이전트가 어떤 파일도 변경하지 않았습니다. 작업이 수행되지 않았습니다.",
        ));
    }

    // 객관 증거 우선: 검증 명령이 하나라도 실패하면 LLM 판단 없이 FAIL
    if let Some(summary) = summarize_command_failures(command_results) {
        return Ok(Verdict::fail(summary));
    }

    // 독립 검증자 LLM 심사 (worktree 안에서 실행하여 diff 외 파일도 참조 가능)
    let prompt = build_verify_prompt(task_id, task_title, dod, diff, command_results);
    let raw = runner
        .run_agentic(&prompt, worktree_path, verifier_model)
        .context("검증자 실행 실패")?;
    Ok(parse_verdict(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(command: &str, exit_code: i32) -> ExecutionResult {
        ExecutionResult {
            command: command.to_string(),
            args: vec![],
            purpose: "test".to_string(),
            exit_code,
            stdout: String::new(),
            stderr: if exit_code != 0 { "boom".to_string() } else { String::new() },
            duration_ms: 0,
            truncated: false,
        }
    }

    #[test]
    fn parse_verdict_json_pass() {
        let v = parse_verdict(r#"{"verdict":"PASS","feedback":""}"#);
        assert!(v.pass);
    }

    #[test]
    fn parse_verdict_json_fail_with_feedback() {
        let v = parse_verdict(r#"여기 결과: {"verdict":"FAIL","feedback":"테스트 누락"} 끝"#);
        assert!(!v.pass);
        assert_eq!(v.feedback, "테스트 누락");
    }

    #[test]
    fn parse_verdict_json_in_code_fence() {
        let raw = "분석...\n```json\n{\"verdict\": \"PASS\", \"feedback\": \"\"}\n```\n";
        assert!(parse_verdict(raw).pass);
    }

    #[test]
    fn parse_verdict_fallback_token() {
        assert!(parse_verdict("리뷰 내용\n\nPASS").pass);
        assert!(!parse_verdict("리뷰 내용\n\nFAIL").pass);
    }

    #[test]
    fn parse_verdict_unknown_is_conservative_fail() {
        let v = parse_verdict("아무 판정 없음");
        assert!(!v.pass);
    }

    #[test]
    fn parse_verdict_fail_without_feedback_gets_default() {
        let v = parse_verdict(r#"{"verdict":"FAIL"}"#);
        assert!(!v.pass);
        assert!(!v.feedback.is_empty());
    }

    #[test]
    fn json_object_wins_over_trailing_token() {
        // JSON이 PASS인데 본문에 FAIL 단어가 섞여 있어도 JSON 우선
        let raw = "이 변경은 FAIL할 뻔했지만\n{\"verdict\":\"PASS\",\"feedback\":\"\"}";
        assert!(parse_verdict(raw).pass);
    }

    #[test]
    fn summarize_failures_none_when_all_pass() {
        let results = vec![exec("cargo", 0), exec("clippy", 0)];
        assert!(summarize_command_failures(&results).is_none());
    }

    #[test]
    fn summarize_failures_lists_failing_commands() {
        let results = vec![exec("cargo", 0), exec("clippy", 1)];
        let summary = summarize_command_failures(&results).unwrap();
        assert!(summary.contains("clippy"));
        assert!(summary.contains("1개"));
    }

    #[test]
    fn build_verify_prompt_flags_empty_diff() {
        let prompt = build_verify_prompt("M1-T01", "작업", &["테스트".to_string()], "", &[]);
        assert!(prompt.contains("변경된 내용이 없습니다"));
        assert!(prompt.contains("M1-T01"));
        assert!(prompt.contains("DoD") || prompt.contains("완료 기준"));
    }

    #[test]
    fn build_verify_prompt_includes_diff_and_format() {
        let prompt = build_verify_prompt("M1-T01", "작업", &[], "diff --git a/x", &[]);
        assert!(prompt.contains("diff --git a/x"));
        assert!(prompt.contains("verdict"));
    }
}
