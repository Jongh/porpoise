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
    /// PASS이지만 설명 메모를 동반한다 (예: 객관 증거 기반 통과).
    pub fn pass_with_note(feedback: impl Into<String>) -> Self {
        Verdict { pass: true, feedback: feedback.into() }
    }
}

/// Verify 단계의 산출물 — 판정 + 검증자 원문(감사·관측용).
#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub verdict: Verdict,
    /// 마지막 검증자 LLM 응답 원문 (LLM 호출이 없었으면 빈 문자열).
    pub verifier_raw: String,
    /// 검증자 verdict 파싱 실패로 **객관 증거 폴백**(또는 halt)이 발동됐는지.
    /// true + verdict.pass면 "검증자 판정 없이 객관 증거로 통과" → 검토 권장 신호 (M22).
    pub fallback_used: bool,
    /// M40: 검증자 LLM 비용(USD). LLM 미호출(diff 없음·명령 실패·chaos)이면 None.
    /// 1차 심사 + 재질의의 합산.
    pub verifier_cost_usd: Option<f64>,
}

/// 두 비용을 합산한다 — 하나라도 값이 있으면 Some(합). 둘 다 None이면 None (M40).
pub fn add_cost(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (None, None) => None,
        _ => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    }
}

/// 검증자 LLM 원문에서 판정을 추출한다. 추출 불가면 None.
///
/// 우선순위: ① JSON 객체 `{"verdict":"PASS"|"FAIL","feedback":"..."}`
/// ② 폴백 — 마지막 비어있지 않은 줄의 PASS/FAIL 토큰.
/// 호출자(run_verification)는 None일 때 재질의 후 객관 증거로 폴백한다.
pub fn try_parse_verdict(raw: &str) -> Option<Verdict> {
    if let Some(v) = parse_verdict_json(raw) {
        return Some(v);
    }
    for line in raw.lines().rev() {
        let t = line.trim().trim_matches(|c: char| !c.is_ascii_alphabetic());
        if t.eq_ignore_ascii_case("PASS") {
            return Some(Verdict::pass());
        }
        if t.eq_ignore_ascii_case("FAIL") {
            return Some(Verdict::fail(extract_feedback_fallback(raw)));
        }
    }
    None
}

/// 검증자 verdict를 (재질의 포함) 파싱하지 못했을 때 **객관 증거**로 최종 판정한다.
///
/// 이 지점은 검증 명령에 실패가 없을 때만 도달한다(실패는 LLM 호출 전 단계에서 FAIL 처리됨).
/// 따라서 실행된 명령이 있으면 "모두 통과"를 의미하므로 객관 증거 기반 PASS로 처리하여
/// 검증자 출력 비신뢰성으로 인한 false-negative FAIL을 방지한다. 명령이 전혀 없으면
/// 객관 증거가 없으므로 보수적 FAIL.
pub fn fallback_verdict(command_results: &[ExecutionResult]) -> Verdict {
    if command_results.is_empty() {
        Verdict::fail(
            "검증자 verdict를 재질의 후에도 파싱할 수 없고, 객관 증거로 쓸 검증 명령도 없습니다. \
             (보수적 FAIL — workspace.toml [tech]에 test_command/verify_commands를 설정하면 \
             객관 증거 기반 판정이 가능합니다.)",
        )
    } else {
        Verdict::pass_with_note(
            "검증자 verdict를 파싱하지 못했으나(재질의 포함), 모든 검증 명령이 통과하여 \
             객관 증거 기반으로 통과 처리합니다.",
        )
    }
}

/// 검증자가 파싱 불가 응답을 줬을 때 보내는 재질의 프롬프트.
fn build_reask_prompt(original: &str) -> String {
    format!(
        "{}\n\n=== 재요청 ===\n앞선 응답에서 판정(verdict)을 찾을 수 없었습니다. \
         설명·도구 사용·파일 탐색 없이, 아래 JSON 객체 한 줄만 정확히 출력하세요:\n\
         {{\"verdict\": \"PASS\" 또는 \"FAIL\", \"feedback\": \"FAIL인 경우 사유\"}}",
        original
    )
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
        "=== 출력 형식 (엄수) ===\n\
         위에 주어진 diff와 명령 결과만으로 판단하세요. 추가 탐색·도구 사용·장황한 설명을 하지 마세요.\n\
         응답은 **아래 JSON 객체 단 하나**여야 하며, 그 외의 텍스트(분석·코드펜스·머리말)를 절대 포함하지 마세요:\n\
         {\"verdict\": \"PASS\" 또는 \"FAIL\", \"feedback\": \"FAIL인 경우 수정해야 할 구체적 사항\"}"
            .to_string(),
    );

    parts.join("\n\n")
}

/// 전체 검증 수행: 실제 명령 실행 → (실패 시 즉시 FAIL) → 독립 검증자 LLM 심사
/// → (파싱 불가 시) 재질의 1회 → (여전히 불가 시) 객관 증거 폴백.
///
/// 검증자는 격리 worktree 안에서 실행하여 실제 저장소를 건드리지 않는다.
/// 파싱 실패가 즉시 FAIL로 이어지지 않으므로(M21), 검증자 출력 비신뢰성에 의한
/// false-negative FAIL을 방지한다.
/// 검증자 파싱 실패가 재질의 후에도 지속될 때의 최종 판정 (정책 분기, 순수 함수).
/// `halt`면 객관 증거 PASS 대신 사용자 검토를 위해 FAIL — false-positive를 보수적으로 차단(M22).
pub fn resolve_fallback(command_results: &[ExecutionResult], halt: bool) -> Verdict {
    if halt {
        Verdict::fail(
            "검증자 verdict를 재질의 후에도 파싱할 수 없습니다. \
             [conductor] verdict_fallback=\"halt\" 정책에 따라 사용자 검토를 위해 FAIL 처리합니다.",
        )
    } else {
        fallback_verdict(command_results)
    }
}

/// 테스트 전용 혼돈 모드 활성 여부 (`PORPOISE_VERIFY_CHAOS=1`).
fn chaos_active() -> bool {
    std::env::var("PORPOISE_VERIFY_CHAOS").map(|v| v == "1").unwrap_or(false)
}

/// 테스트 전용: 검증자 LLM 호출을 **건너뛰고** 파싱 불가 응답을 주입한다 (M22-T04).
///
/// 프롬프트에 "산문으로 답하라"를 덧붙이는 방식은 강한 검증자 모델이 JSON 출력 계약과의
/// 충돌을 인지해 무시하므로 비결정적이다. 대신 호출 자체를 우회하여 안전망(재질의·폴백)을
/// **결정론적으로** 발동시킨다. 평상시(미설정)엔 영향 없음.
fn chaos_response() -> String {
    eprintln!("  ⚠ PORPOISE_VERIFY_CHAOS=1 — 검증자 호출을 건너뛰고 파싱 불가 응답 주입 (안전망 검증)");
    "[혼돈 모드] 검증자 산문 응답입니다. 구조화된 판정이 없습니다.".to_string()
}

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
    fallback_halt: bool,
    stream: bool,
) -> Result<VerifyOutcome> {
    // 변경이 전혀 없으면 작업 미수행 — 즉시 FAIL (LLM 미호출 → 비용 None)
    if diff.trim().is_empty() {
        return Ok(VerifyOutcome {
            verdict: Verdict::fail(
                "에이전트가 어떤 파일도 변경하지 않았습니다. 작업이 수행되지 않았습니다.",
            ),
            verifier_raw: String::new(),
            fallback_used: false,
            verifier_cost_usd: None,
        });
    }

    // 객관 증거 우선: 검증 명령이 하나라도 실패하면 LLM 판단 없이 FAIL (비용 None)
    if let Some(summary) = summarize_command_failures(command_results) {
        return Ok(VerifyOutcome {
            verdict: Verdict::fail(summary),
            verifier_raw: String::new(),
            fallback_used: false,
            verifier_cost_usd: None,
        });
    }

    let chaos = chaos_active();
    // M40: 검증자 비용 누적 (1차 + 재질의). chaos는 LLM 미호출이라 None 유지.
    let mut verifier_cost: Option<f64> = None;

    // 1차 심사 (chaos면 LLM 호출을 건너뛰고 파싱 불가 응답 주입)
    let prompt = build_verify_prompt(task_id, task_title, dod, diff, command_results);
    let raw = if chaos {
        chaos_response()
    } else {
        let run = runner
            .run_agentic_metered(&prompt, worktree_path, verifier_model, stream)
            .context("검증자 실행 실패")?;
        verifier_cost = add_cost(verifier_cost, run.cost_usd);
        run.output
    };
    if let Some(verdict) = try_parse_verdict(&raw) {
        return Ok(VerifyOutcome { verdict, verifier_raw: raw, fallback_used: false, verifier_cost_usd: verifier_cost });
    }

    // 재질의 1회 (파싱 가능한 verdict만 다시 요청)
    let reask = build_reask_prompt(&prompt);
    let raw2 = if chaos {
        chaos_response()
    } else {
        let run = runner
            .run_agentic_metered(&reask, worktree_path, verifier_model, stream)
            .context("검증자 재질의 실패")?;
        verifier_cost = add_cost(verifier_cost, run.cost_usd);
        run.output
    };
    if let Some(verdict) = try_parse_verdict(&raw2) {
        return Ok(VerifyOutcome { verdict, verifier_raw: raw2, fallback_used: false, verifier_cost_usd: verifier_cost });
    }

    // 여전히 파싱 불가 → 정책에 따른 폴백 (기본: 객관 증거 PASS, halt: 보수 FAIL)
    let combined = format!("[1차 응답]\n{}\n\n[재질의 응답]\n{}", raw, raw2);
    Ok(VerifyOutcome {
        verdict: resolve_fallback(command_results, fallback_halt),
        verifier_raw: combined,
        fallback_used: true,
        verifier_cost_usd: verifier_cost,
    })
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
        let v = try_parse_verdict(r#"{"verdict":"PASS","feedback":""}"#).unwrap();
        assert!(v.pass);
    }

    #[test]
    fn parse_verdict_json_fail_with_feedback() {
        let v = try_parse_verdict(r#"여기 결과: {"verdict":"FAIL","feedback":"테스트 누락"} 끝"#).unwrap();
        assert!(!v.pass);
        assert_eq!(v.feedback, "테스트 누락");
    }

    #[test]
    fn parse_verdict_json_in_code_fence() {
        let raw = "분석...\n```json\n{\"verdict\": \"PASS\", \"feedback\": \"\"}\n```\n";
        assert!(try_parse_verdict(raw).unwrap().pass);
    }

    #[test]
    fn parse_verdict_fallback_token() {
        assert!(try_parse_verdict("리뷰 내용\n\nPASS").unwrap().pass);
        assert!(!try_parse_verdict("리뷰 내용\n\nFAIL").unwrap().pass);
    }

    #[test]
    fn parse_verdict_fail_without_feedback_gets_default() {
        let v = try_parse_verdict(r#"{"verdict":"FAIL"}"#).unwrap();
        assert!(!v.pass);
        assert!(!v.feedback.is_empty());
    }

    #[test]
    fn json_object_wins_over_trailing_token() {
        // JSON이 PASS인데 본문에 FAIL 단어가 섞여 있어도 JSON 우선
        let raw = "이 변경은 FAIL할 뻔했지만\n{\"verdict\":\"PASS\",\"feedback\":\"\"}";
        assert!(try_parse_verdict(raw).unwrap().pass);
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

    // ── M21: 재질의·객관 증거 폴백 ──────────────────────────────────────────

    #[test]
    fn try_parse_verdict_none_when_unparseable() {
        // 라이브 스모크 테스트에서 관찰된 실패 형태: verdict 토큰·JSON 없음
        assert!(try_parse_verdict("음... 변경을 살펴보니 괜찮아 보입니다만 확신이 없네요").is_none());
        assert!(try_parse_verdict("").is_none());
    }

    #[test]
    fn try_parse_verdict_some_when_parseable() {
        assert_eq!(try_parse_verdict(r#"{"verdict":"PASS","feedback":""}"#).unwrap().pass, true);
        assert_eq!(try_parse_verdict("결론\nFAIL").unwrap().pass, false);
    }

    #[test]
    fn fallback_verdict_passes_when_commands_all_passed() {
        // 핵심 회귀 방지: cargo test 통과(exit 0)인데 verdict 파싱 실패 → 즉시 FAIL 금지
        let results = vec![exec("cargo", 0)];
        let v = fallback_verdict(&results);
        assert!(v.pass, "객관 증거(명령 통과)가 있으면 폴백은 PASS여야 함");
        assert!(v.feedback.contains("객관 증거"));
    }

    #[test]
    fn fallback_verdict_fails_when_no_objective_evidence() {
        // 검증 명령이 전혀 없으면 객관 증거가 없으므로 보수적 FAIL
        let v = fallback_verdict(&[]);
        assert!(!v.pass);
        assert!(v.feedback.contains("객관 증거"));
    }

    #[test]
    fn reask_prompt_demands_json_only() {
        let r = build_reask_prompt("원본 프롬프트");
        assert!(r.contains("원본 프롬프트"));
        assert!(r.contains("재요청"));
        assert!(r.contains("verdict"));
    }

    #[test]
    fn chaos_injected_response_is_unparseable() {
        // 주입 응답은 try_parse_verdict가 None이어야 안전망(재질의·폴백)이 결정론적으로 발동한다
        let raw = "[혼돈 모드] 검증자 산문 응답입니다. 구조화된 판정이 없습니다.";
        assert!(try_parse_verdict(raw).is_none());
    }

    #[test]
    fn resolve_fallback_halt_policy_fails() {
        // halt 정책: 검증 명령이 통과해도 폴백은 FAIL (false-positive 보수 차단)
        let results = vec![exec("cargo", 0)];
        let v = resolve_fallback(&results, true);
        assert!(!v.pass);
        assert!(v.feedback.contains("halt"));
    }

    #[test]
    fn resolve_fallback_default_policy_passes_on_objective_evidence() {
        // 기본 정책: 검증 명령 통과면 객관 증거로 PASS
        let results = vec![exec("cargo", 0)];
        assert!(resolve_fallback(&results, false).pass);
        // 객관 증거 없으면 정책 무관 보수 FAIL
        assert!(!resolve_fallback(&[], false).pass);
        assert!(!resolve_fallback(&[], true).pass);
    }

    #[test]
    fn pass_with_note_is_pass_but_has_feedback() {
        let v = Verdict::pass_with_note("객관 증거 기반");
        assert!(v.pass);
        assert!(!v.feedback.is_empty());
    }
}
