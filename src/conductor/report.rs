//! 함대 실행 리포트 (M25) — `sessions/*.json`(conductor-3) 감사 기록을 파싱·집계·렌더링.
//!
//! conductor는 매 라운드 `sessions/<task>-conductor-<ts>-R<n>.json`에 감사 기록을 **쓰기만**
//! 했고 아무도 읽지 않았다. 이 모듈은 그 기록을 태스크별·마일스톤별 **실행 요약**으로 합성한다.
//! 파일 I/O(`load_records`)와 집계 로직(`aggregate`)을 분리해 순수 함수 테스트가 용이하다.
//! (M24 `schedule.rs`의 순수 함수 패턴 계승)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use colored::Colorize;
use serde::{Deserialize, Serialize};

/// conductor-3 감사 레코드 중 집계에 필요한 필드. 스키마 변형·구버전에 견디도록 모두 기본값 허용.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AuditRecord {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub redispatch: u32,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub fallback_used: bool,
    #[serde(default)]
    pub diff_lines: u64,
    #[serde(default)]
    pub verify_commands: Vec<VerifyCommandRec>,
    // M28: 비용·토큰 (conductor-4). 구 기록(conductor-3)엔 없으므로 default None.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    // M40: 검증자 LLM 비용 (conductor-5). 구 기록(conductor-4 이하)엔 없으므로 default None.
    #[serde(default)]
    pub verifier_cost_usd: Option<f64>,
    // M36: task 상세 보기용 본문 필드 — 검증 피드백·에이전트 최종 보고·검증자 원문.
    #[serde(default)]
    pub feedback: String,
    #[serde(default)]
    pub dispatch_output: String,
    #[serde(default)]
    pub verifier_raw: String,
}

/// 한 task의 **최신 run**(M27 규칙 — timestamp 정렬 후 마지막 R0부터) 레코드를 반환한다.
/// `/api/task` 상세 보기(M36)와 `aggregate`가 같은 run 정의를 공유한다.
pub fn latest_run_records(records: &[AuditRecord], task_id: &str) -> Vec<AuditRecord> {
    let mut recs: Vec<AuditRecord> = records
        .iter()
        .filter(|r| r.task_id == task_id)
        .cloned()
        .collect();
    recs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let start = recs.iter().rposition(|r| r.redispatch == 0).unwrap_or(0);
    recs.split_off(start)
}

/// 감사 기록의 verify 명령 결과 (집계엔 exit_code만 필요 — 나머지 키는 serde가 무시).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct VerifyCommandRec {
    #[serde(default)]
    pub exit_code: i64,
}

impl AuditRecord {
    /// verdict가 "PASS"(대소문자 무관)면 true.
    pub fn passed(&self) -> bool {
        self.verdict.eq_ignore_ascii_case("PASS")
    }
}

/// 한 태스크의 실행 요약 — 여러 재투입 라운드를 합산한다.
#[derive(Debug, Clone, Serialize)]
pub struct TaskRunSummary {
    pub task_id: String,
    /// 기록된 라운드(시도) 수.
    pub attempts: u32,
    /// 최대 redispatch 값 = 재투입 횟수.
    pub max_redispatch: u32,
    /// 최종(가장 큰 redispatch, 동률이면 최신 timestamp) 라운드의 PASS 여부.
    pub final_verdict: bool,
    /// 어느 라운드든 객관 증거 폴백이 발동했으면 true.
    pub fallback_used: bool,
    /// 최종 라운드의 diff 줄 수.
    pub final_diff_lines: u64,
    /// 최종 라운드 타임스탬프(rfc3339 문자열).
    pub last_timestamp: String,
    /// 최종 라운드의 verify 명령이 (1개 이상 있고) 모두 exit 0이면 true.
    pub verify_all_passed: bool,
    /// M28: 최신 run의 누적 **dispatch** 비용(USD). 비용 미가용이면 None.
    pub cost_usd: Option<f64>,
    /// M40: 최신 run의 누적 **검증자** 비용(USD). 미가용(구 기록·LLM 미호출)이면 None.
    pub verifier_cost_usd: Option<f64>,
    /// M28: 최신 run의 누적 입력/출력 토큰. 미가용이면 None.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// 한 마일스톤(또는 전체)의 실행 롤업.
#[derive(Debug, Clone)]
pub struct MilestoneRunReport {
    /// 한정된 마일스톤 번호. 전체 집계면 None.
    pub milestone: Option<u32>,
    pub tasks: Vec<TaskRunSummary>,
    /// 파싱 실패(손상·비대상) 파일 수.
    pub parse_errors: usize,
}

impl MilestoneRunReport {
    pub fn total(&self) -> usize {
        self.tasks.len()
    }
    pub fn passed(&self) -> usize {
        self.tasks.iter().filter(|t| t.final_verdict).count()
    }
    pub fn failed(&self) -> usize {
        self.total() - self.passed()
    }
    pub fn success_rate(&self) -> f64 {
        if self.tasks.is_empty() {
            0.0
        } else {
            self.passed() as f64 / self.total() as f64 * 100.0
        }
    }
    pub fn total_redispatches(&self) -> u32 {
        self.tasks.iter().map(|t| t.max_redispatch).sum()
    }
    pub fn fallback_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.fallback_used).count()
    }
    /// M28/M40: 마일스톤 총비용(USD) = dispatch + verifier. 비용이 하나도 없으면 None.
    pub fn total_cost(&self) -> Option<f64> {
        sum_opt_f64(
            self.tasks
                .iter()
                .map(|t| crate::conductor::verify::add_cost(t.cost_usd, t.verifier_cost_usd)),
        )
    }
    /// M40: dispatch 비용만 (총비용에서 검증자 비용 분리 노출용).
    pub fn total_dispatch_cost(&self) -> Option<f64> {
        sum_opt_f64(self.tasks.iter().map(|t| t.cost_usd))
    }
    /// M40: 검증자(verifier) 비용만.
    pub fn total_verifier_cost(&self) -> Option<f64> {
        sum_opt_f64(self.tasks.iter().map(|t| t.verifier_cost_usd))
    }
    /// M28: 총 입력/출력 토큰.
    pub fn total_input_tokens(&self) -> Option<u64> {
        sum_opt_u64(self.tasks.iter().map(|t| t.input_tokens))
    }
    pub fn total_output_tokens(&self) -> Option<u64> {
        sum_opt_u64(self.tasks.iter().map(|t| t.output_tokens))
    }
}

/// task_id("M25-T01")에서 마일스톤 번호(25)를 추출.
pub fn milestone_of(task_id: &str) -> Option<u32> {
    let rest = task_id.strip_prefix('M')?;
    let dash = rest.find('-')?;
    rest[..dash].parse().ok()
}

/// Option<f64> 이터레이터를 합산한다. 값이 하나도 없으면 None(미가용), 있으면 Some(합).
fn sum_opt_f64(it: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let vals: Vec<f64> = it.flatten().collect();
    if vals.is_empty() {
        None
    } else {
        Some(vals.iter().sum())
    }
}

/// Option<u64> 이터레이터를 합산한다 (값이 없으면 None).
fn sum_opt_u64(it: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    let vals: Vec<u64> = it.flatten().collect();
    if vals.is_empty() {
        None
    } else {
        Some(vals.iter().sum())
    }
}

/// 레코드 목록을 태스크별 요약으로 집계한다 (순수 함수, task_id 사전순 정렬).
pub fn aggregate(records: &[AuditRecord]) -> Vec<TaskRunSummary> {
    let mut by_task: BTreeMap<String, Vec<&AuditRecord>> = BTreeMap::new();
    for r in records {
        if r.task_id.is_empty() {
            continue;
        }
        by_task.entry(r.task_id.clone()).or_default().push(r);
    }

    let mut out = Vec::new();
    for (task_id, mut recs) in by_task {
        // 재실행-인지 집계(M27): 같은 task를 다시 실행하면 이전 run의 stale 레코드가 섞인다.
        // timestamp 순으로 정렬한 뒤, 마지막 redispatch==0(각 dispatch는 R0에서 시작 = run 경계)
        // 부터 끝까지를 "최신 run"으로 보고 그 run만 집계한다. R0가 없으면 전체를 한 run으로 폴백.
        recs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        let start = recs.iter().rposition(|r| r.redispatch == 0).unwrap_or(0);
        let latest_run = &recs[start..];

        // 최신 run의 마지막(최신 timestamp) 레코드가 최종 결과.
        let final_rec = latest_run.last().expect("by_task 그룹은 비어 있지 않음");
        let fallback_used = latest_run.iter().any(|r| r.fallback_used);
        let max_redispatch = latest_run.iter().map(|r| r.redispatch).max().unwrap_or(0);
        let verify_all_passed = !final_rec.verify_commands.is_empty()
            && final_rec.verify_commands.iter().all(|c| c.exit_code == 0);
        // M28/M40: 최신 run의 비용·토큰 합산 (전부 None이면 None)
        let cost_usd = sum_opt_f64(latest_run.iter().map(|r| r.cost_usd));
        let verifier_cost_usd = sum_opt_f64(latest_run.iter().map(|r| r.verifier_cost_usd));
        let input_tokens = sum_opt_u64(latest_run.iter().map(|r| r.input_tokens));
        let output_tokens = sum_opt_u64(latest_run.iter().map(|r| r.output_tokens));

        out.push(TaskRunSummary {
            task_id,
            attempts: latest_run.len() as u32,
            max_redispatch,
            final_verdict: final_rec.passed(),
            fallback_used,
            final_diff_lines: final_rec.diff_lines,
            last_timestamp: final_rec.timestamp.clone(),
            verify_all_passed,
            cost_usd,
            verifier_cost_usd,
            input_tokens,
            output_tokens,
        });
    }
    out
}

/// 레코드 중 가장 큰 마일스톤 번호를 찾는다 (`--milestone` 미지정 시 기본 대상).
pub fn latest_milestone(records: &[AuditRecord]) -> Option<u32> {
    records.iter().filter_map(|r| milestone_of(&r.task_id)).max()
}

/// `sessions/`에서 conductor 감사 레코드를 모두 읽는다. (손상·비대상은 스킵, 에러 수 반환)
pub fn load_records(project_path: &Path) -> (Vec<AuditRecord>, usize) {
    let sessions_dir = project_path.join(".porpoise").join("sessions");
    let mut records = Vec::new();
    let mut errors = 0;

    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return (records, 0),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        // conductor 감사 기록만: "<task>-conductor-<ts>-R<n>.json"
        if !name.contains("-conductor-") || !name.ends_with(".json") {
            continue;
        }
        let content = match std::fs::read_to_string(&p) {
            Ok(c) => c,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        // 일부 도구가 붙이는 UTF-8 BOM을 제거 — serde_json은 선행 BOM을 거부한다.
        let content = content.trim_start_matches('\u{feff}');
        match serde_json::from_str::<AuditRecord>(content) {
            Ok(rec) => records.push(rec),
            Err(_) => errors += 1,
        }
    }
    (records, errors)
}

/// 감사 기록을 읽어 (선택적으로 마일스톤 한정) 실행 리포트를 만든다.
/// `milestone`이 None이면 기록 중 가장 최근 마일스톤으로 한정한다(없으면 전체).
pub fn build_report(project_path: &Path, milestone: Option<u32>) -> MilestoneRunReport {
    let (records, parse_errors) = load_records(project_path);
    let target = milestone.or_else(|| latest_milestone(&records));

    let filtered: Vec<AuditRecord> = match target {
        Some(m) => records
            .into_iter()
            .filter(|r| milestone_of(&r.task_id) == Some(m))
            .collect(),
        None => records,
    };

    let tasks = aggregate(&filtered);
    MilestoneRunReport {
        milestone: target,
        tasks,
        parse_errors,
    }
}

/// 마일스톤 라벨("M25" 또는 "전체").
fn milestone_label(m: Option<u32>) -> String {
    match m {
        Some(n) => format!("M{}", n),
        None => "전체".to_string(),
    }
}

/// 태스크 최종 결과 마커.
fn verdict_marker(t: &TaskRunSummary) -> &'static str {
    if !t.final_verdict {
        "❌"
    } else if t.fallback_used {
        "⚠"
    } else {
        "✅"
    }
}

/// 콘솔에 실행 리포트를 출력한다.
pub fn render_console(report: &MilestoneRunReport) {
    let sep = "─────────────────────────────────────".dimmed();
    println!();
    println!("{}", "함대 실행 리포트".cyan().bold());
    println!("{}", sep);
    println!("마일스톤 {}", milestone_label(report.milestone).cyan().bold());
    println!(
        "태스크 {} · {} {} · {} {} · 성공률 {}",
        report.total(),
        "PASS".green(),
        report.passed(),
        "FAIL".red(),
        report.failed(),
        format!("{:.1}%", report.success_rate()).bold()
    );
    println!(
        "재투입 합계 {} · 폴백 {}",
        report.total_redispatches(),
        report.fallback_count()
    );
    // M28: 비용·토큰 (가용 시)
    if let Some(cost) = report.total_cost() {
        let tok = match (report.total_input_tokens(), report.total_output_tokens()) {
            (Some(i), Some(o)) => format!(" · 토큰 in {} / out {}", i, o),
            _ => String::new(),
        };
        println!("총비용 {}{}", format!("${:.4}", cost).bold(), tok);
    }
    println!("{}", sep);

    for t in &report.tasks {
        let mut detail = format!("시도 {} · 재투입 {}", t.attempts, t.max_redispatch);
        if t.fallback_used {
            detail = format!("{} · 폴백", detail);
        }
        if let Some(c) = t.cost_usd {
            detail = format!("{} · ${:.4}", detail, c);
        }
        println!(
            "  {} {}: {}",
            verdict_marker(t),
            t.task_id.dimmed(),
            detail.dimmed()
        );
    }

    if report.parse_errors > 0 {
        println!("{}", sep);
        println!(
            "{}",
            format!("⚠ 손상된 감사 기록 {}건 건너뜀", report.parse_errors).yellow()
        );
    }
    println!();
}

/// 실행 리포트를 Markdown 문서 문자열로 렌더링한다 (순수 함수).
pub fn render_markdown(report: &MilestoneRunReport) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "# 함대 실행 리포트 — 마일스톤 {}\n\n",
        milestone_label(report.milestone)
    ));
    s.push_str(&format!(
        "- 태스크: {} (PASS {} / FAIL {})\n",
        report.total(),
        report.passed(),
        report.failed()
    ));
    s.push_str(&format!("- 성공률: {:.1}%\n", report.success_rate()));
    s.push_str(&format!("- 재투입 합계: {}\n", report.total_redispatches()));
    s.push_str(&format!("- 폴백: {}\n", report.fallback_count()));
    if let Some(cost) = report.total_cost() {
        s.push_str(&format!("- 총비용: ${:.4}\n", cost));
        if let (Some(i), Some(o)) = (report.total_input_tokens(), report.total_output_tokens()) {
            s.push_str(&format!("- 총토큰: 입력 {} / 출력 {}\n", i, o));
        }
    }
    if report.parse_errors > 0 {
        s.push_str(&format!("- 손상된 기록: {}건 건너뜀\n", report.parse_errors));
    }
    s.push('\n');

    s.push_str("| Task | Verdict | 시도 | 재투입 | 폴백 | 검증명령 | 비용 | diff | 마지막 라운드 |\n");
    s.push_str("|------|---------|------|--------|------|----------|------|------|----------------|\n");
    for t in &report.tasks {
        let verdict = if t.final_verdict { "PASS" } else { "FAIL" };
        let fallback = if t.fallback_used { "예" } else { "" };
        let verify = if t.verify_all_passed { "통과" } else { "" };
        let cost = t.cost_usd.map(|c| format!("${:.4}", c)).unwrap_or_else(|| "-".to_string());
        let ts = if t.last_timestamp.is_empty() {
            "-"
        } else {
            t.last_timestamp.as_str()
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            t.task_id, verdict, t.attempts, t.max_redispatch, fallback, verify, cost, t.final_diff_lines, ts
        ));
    }
    s
}

/// Markdown 리포트 기본 출력 경로 (`.porpoise/reports/run-M{N}.md` 또는 `run-all.md`).
fn default_markdown_path(project_path: &Path, report: &MilestoneRunReport) -> PathBuf {
    let name = match report.milestone {
        Some(n) => format!("run-M{}.md", n),
        None => "run-all.md".to_string(),
    };
    project_path
        .join(".porpoise")
        .join("reports")
        .join(name)
}

/// `porpoise report` 서브커맨드 진입점.
pub fn run_report(
    project_path: &Path,
    milestone: Option<u32>,
    markdown: bool,
    out: Option<&str>,
) -> anyhow::Result<()> {
    let report = build_report(project_path, milestone);

    if report.tasks.is_empty() {
        println!();
        if report.parse_errors > 0 {
            println!(
                "{}",
                format!(
                    "감사 기록을 읽을 수 없습니다 (손상 {}건). conductor 실행 후 다시 시도하세요.",
                    report.parse_errors
                )
                .yellow()
            );
        } else {
            println!(
                "{}",
                "집계할 감사 기록이 없습니다. conductor로 마일스톤을 실행한 뒤 다시 시도하세요.".yellow()
            );
        }
        println!();
        return Ok(());
    }

    render_console(&report);

    // --out을 지정하면 사용자 의도가 명확하므로 --markdown 없이도 내보낸다.
    let export = markdown || out.is_some();
    if export {
        let md = render_markdown(&report);
        let path = match out {
            Some(p) => PathBuf::from(p),
            None => default_markdown_path(project_path, &report),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, md)?;
        println!("{} {}", "Markdown 리포트 저장:".green(), path.display());
        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(task: &str, redispatch: u32, ts: &str, verdict: &str, fallback: bool) -> AuditRecord {
        AuditRecord {
            task_id: task.to_string(),
            redispatch,
            timestamp: ts.to_string(),
            verdict: verdict.to_string(),
            fallback_used: fallback,
            ..Default::default()
        }
    }

    /// 비용 포함 레코드.
    fn rec_cost(task: &str, redispatch: u32, ts: &str, verdict: &str, cost: f64) -> AuditRecord {
        AuditRecord {
            cost_usd: Some(cost),
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..rec(task, redispatch, ts, verdict, false)
        }
    }

    #[test]
    fn aggregate_sums_cost_over_latest_run() {
        // 최신 run(R0 FAIL + R1 PASS)의 비용만 합산. 이전 run 비용은 제외.
        let recs = vec![
            rec_cost("M1-T01", 0, "2026-06-09T10:00:00Z", "FAIL", 9.99), // 이전 run (제외)
            rec_cost("M1-T01", 0, "2026-06-09T11:00:00Z", "FAIL", 0.02), // 이번 run R0
            rec_cost("M1-T01", 1, "2026-06-09T11:05:00Z", "PASS", 0.03), // 이번 run R1
        ];
        let out = aggregate(&recs);
        assert_eq!(out.len(), 1);
        assert!(out[0].final_verdict);
        // 부동소수점 — epsilon 비교 (0.02+0.03, 이전 run 9.99 제외)
        assert!(
            (out[0].cost_usd.unwrap() - 0.05).abs() < 1e-9,
            "최신 run 비용만 합산 (0.02+0.03): {:?}",
            out[0].cost_usd
        );
        assert_eq!(out[0].input_tokens, Some(200));
    }

    #[test]
    fn aggregate_cost_none_when_absent() {
        // 구 기록(비용 없음) → None
        let out = aggregate(&[rec("M1-T01", 0, "t", "PASS", false)]);
        assert_eq!(out[0].cost_usd, None);
    }

    #[test]
    fn report_splits_dispatch_and_verifier_cost() {
        // M40: dispatch + verifier 분리 집계, total_cost는 합
        let r1 = AuditRecord {
            cost_usd: Some(0.10),
            verifier_cost_usd: Some(0.02),
            ..rec("M1-T01", 0, "t", "PASS", false)
        };
        let report = MilestoneRunReport {
            milestone: None,
            tasks: aggregate(&[r1]),
            parse_errors: 0,
        };
        assert_eq!(report.total_dispatch_cost(), Some(0.10));
        assert_eq!(report.total_verifier_cost(), Some(0.02));
        assert!((report.total_cost().unwrap() - 0.12).abs() < 1e-9, "총비용 = dispatch+verifier");
    }

    #[test]
    fn report_total_cost_backward_compat_no_verifier() {
        // M40: 구 conductor-4 레코드(verifier_cost 없음) → verifier None, total_cost = dispatch만
        let report = MilestoneRunReport {
            milestone: None,
            tasks: aggregate(&[rec_cost("M1-T01", 0, "t", "PASS", 0.20)]),
            parse_errors: 0,
        };
        assert_eq!(report.total_verifier_cost(), None, "구 레코드는 verifier 비용 None");
        assert!((report.total_cost().unwrap() - 0.20).abs() < 1e-9, "총비용 = dispatch만");
    }

    #[test]
    fn report_total_cost_rollup() {
        let report = MilestoneRunReport {
            milestone: Some(1),
            tasks: aggregate(&[
                rec_cost("M1-T01", 0, "t", "PASS", 0.10),
                rec_cost("M1-T02", 0, "t", "PASS", 0.25),
            ]),
            parse_errors: 0,
        };
        assert!((report.total_cost().unwrap() - 0.35).abs() < 1e-9);
        assert_eq!(report.total_input_tokens(), Some(200));
    }

    #[test]
    fn milestone_of_extracts_number() {
        assert_eq!(milestone_of("M25-T01"), Some(25));
        assert_eq!(milestone_of("M1-T99"), Some(1));
        assert_eq!(milestone_of("nope"), None);
    }

    #[test]
    fn aggregate_single_round_pass() {
        let recs = vec![rec("M1-T01", 0, "2026-01-01T00:00:00Z", "PASS", false)];
        let out = aggregate(&recs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].task_id, "M1-T01");
        assert_eq!(out[0].attempts, 1);
        assert_eq!(out[0].max_redispatch, 0);
        assert!(out[0].final_verdict);
        assert!(!out[0].fallback_used);
    }

    #[test]
    fn aggregate_multi_round_uses_final_verdict() {
        // R0 FAIL → R1 PASS: 최종은 PASS, 재투입 1, 시도 2
        let recs = vec![
            rec("M1-T01", 0, "2026-01-01T00:00:00Z", "FAIL", false),
            rec("M1-T01", 1, "2026-01-01T00:05:00Z", "PASS", false),
        ];
        let out = aggregate(&recs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].attempts, 2);
        assert_eq!(out[0].max_redispatch, 1);
        assert!(out[0].final_verdict, "최종 라운드(R1) PASS여야 함");
    }

    #[test]
    fn aggregate_uses_latest_run_on_rerun() {
        // M27 회귀: 이전 run(R0/R1/R2 FAIL) + 이번 run(R0 PASS)이 섞이면,
        // 최신 run만 반영해 final=PASS, attempts=1, 재투입=0 이어야 한다.
        let recs = vec![
            rec("M1-T02", 0, "2026-06-09T13:19:00Z", "FAIL", false),
            rec("M1-T02", 1, "2026-06-09T13:20:00Z", "FAIL", false),
            rec("M1-T02", 2, "2026-06-09T13:22:00Z", "FAIL", false),
            rec("M1-T02", 0, "2026-06-09T13:40:00Z", "PASS", false), // 이번 run
        ];
        let out = aggregate(&recs);
        assert_eq!(out.len(), 1);
        assert!(out[0].final_verdict, "최신 run의 PASS가 최종이어야 함");
        assert_eq!(out[0].attempts, 1, "최신 run만 카운트");
        assert_eq!(out[0].max_redispatch, 0, "최신 run은 R0뿐");
    }

    #[test]
    fn aggregate_latest_run_can_be_multiround() {
        // 최신 run이 다중 라운드면(R0 FAIL → R1 PASS) 그 run 전체를 집계한다.
        let recs = vec![
            rec("M1-T01", 0, "2026-06-09T10:00:00Z", "PASS", false), // 이전 run
            rec("M1-T01", 0, "2026-06-09T11:00:00Z", "FAIL", false), // 이번 run R0
            rec("M1-T01", 1, "2026-06-09T11:05:00Z", "PASS", false), // 이번 run R1
        ];
        let out = aggregate(&recs);
        assert_eq!(out.len(), 1);
        assert!(out[0].final_verdict, "이번 run 최종(R1) PASS");
        assert_eq!(out[0].attempts, 2, "이번 run의 두 라운드");
        assert_eq!(out[0].max_redispatch, 1);
    }

    #[test]
    fn latest_run_records_returns_only_latest_run() {
        // M36: 상세 보기도 M27의 최신 run 정의를 따른다
        let recs = vec![
            rec("M1-T02", 0, "2026-06-09T13:19:00Z", "FAIL", false), // 이전 run
            rec("M1-T02", 1, "2026-06-09T13:20:00Z", "FAIL", false), // 이전 run
            rec("M1-T02", 0, "2026-06-09T13:40:00Z", "FAIL", false), // 이번 run R0
            rec("M1-T02", 1, "2026-06-09T13:45:00Z", "PASS", false), // 이번 run R1
            rec("M1-T03", 0, "2026-06-09T14:00:00Z", "PASS", false), // 다른 task
        ];
        let run = latest_run_records(&recs, "M1-T02");
        assert_eq!(run.len(), 2, "최신 run의 두 라운드만");
        assert_eq!(run[0].redispatch, 0);
        assert_eq!(run[1].redispatch, 1);
        assert!(run[1].passed());
        assert!(latest_run_records(&recs, "NOPE").is_empty());
    }

    #[test]
    fn aggregate_sorts_unordered_input() {
        // load_records는 read_dir 순서(정렬 미보장)로 읽으므로 aggregate의 timestamp 정렬이
        // load-bearing이다. 입력이 시각 역순이어도 최신 run(이번 R0 PASS)을 골라야 한다.
        let recs = vec![
            rec("M1-T02", 0, "2026-06-09T13:40:00Z", "PASS", false), // 이번 run (가장 최신)
            rec("M1-T02", 2, "2026-06-09T13:22:00Z", "FAIL", false), // 이전 run
            rec("M1-T02", 0, "2026-06-09T13:19:00Z", "FAIL", false), // 이전 run
            rec("M1-T02", 1, "2026-06-09T13:20:00Z", "FAIL", false), // 이전 run
        ];
        let out = aggregate(&recs);
        assert_eq!(out.len(), 1);
        assert!(out[0].final_verdict, "정렬 후 최신 run의 PASS가 최종이어야 함");
        assert_eq!(out[0].attempts, 1);
        assert_eq!(out[0].max_redispatch, 0);
    }

    #[test]
    fn aggregate_fallback_any_round() {
        let recs = vec![
            rec("M1-T01", 0, "2026-01-01T00:00:00Z", "FAIL", true),
            rec("M1-T01", 1, "2026-01-01T00:05:00Z", "PASS", false),
        ];
        let out = aggregate(&recs);
        assert!(out[0].fallback_used, "어느 라운드든 폴백이면 true");
    }

    #[test]
    fn aggregate_verify_all_passed() {
        let mut r = rec("M1-T01", 0, "2026-01-01T00:00:00Z", "PASS", false);
        r.verify_commands = vec![
            VerifyCommandRec { exit_code: 0 },
            VerifyCommandRec { exit_code: 0 },
        ];
        let out = aggregate(&[r]);
        assert!(out[0].verify_all_passed);

        let mut r2 = rec("M1-T02", 0, "2026-01-01T00:00:00Z", "FAIL", false);
        r2.verify_commands = vec![VerifyCommandRec { exit_code: 101 }];
        let out2 = aggregate(&[r2]);
        assert!(!out2[0].verify_all_passed);
    }

    #[test]
    fn report_rollup_metrics() {
        let report = MilestoneRunReport {
            milestone: Some(1),
            tasks: aggregate(&[
                rec("M1-T01", 0, "t", "PASS", false),
                rec("M1-T02", 1, "t", "PASS", true),
                rec("M1-T03", 0, "t", "FAIL", false),
            ]),
            parse_errors: 0,
        };
        assert_eq!(report.total(), 3);
        assert_eq!(report.passed(), 2);
        assert_eq!(report.failed(), 1);
        assert!((report.success_rate() - 66.666).abs() < 0.1);
        assert_eq!(report.total_redispatches(), 1);
        assert_eq!(report.fallback_count(), 1);
    }

    #[test]
    fn build_report_filters_by_milestone_and_skips_corrupt() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();

        let good = serde_json::json!({
            "schema_version": "conductor-3",
            "task_id": "M25-T01",
            "redispatch": 0,
            "timestamp": "2026-06-09T10:00:00Z",
            "verdict": "PASS",
            "fallback_used": false,
            "diff_lines": 12,
            "verify_commands": [{"command": "cargo", "args": ["test"], "exit_code": 0}]
        });
        std::fs::write(
            sessions.join("M25-T01-conductor-20260609-100000-R0.json"),
            serde_json::to_string_pretty(&good).unwrap(),
        )
        .unwrap();
        // 다른 마일스톤
        let other = serde_json::json!({
            "task_id": "M24-T01", "redispatch": 0, "timestamp": "t", "verdict": "PASS"
        });
        std::fs::write(
            sessions.join("M24-T01-conductor-20260601-100000-R0.json"),
            serde_json::to_string(&other).unwrap(),
        )
        .unwrap();
        // 손상 파일
        std::fs::write(
            sessions.join("M25-T02-conductor-20260609-101000-R0.json"),
            "{ this is not valid json",
        )
        .unwrap();
        // 비대상 파일 (conductor 아님)
        std::fs::write(sessions.join("note.txt"), "ignore me").unwrap();

        let report = build_report(tmp.path(), Some(25));
        assert_eq!(report.milestone, Some(25));
        assert_eq!(report.total(), 1, "M25만 집계");
        assert_eq!(report.tasks[0].task_id, "M25-T01");
        assert_eq!(report.parse_errors, 1, "손상 파일 1건");
    }

    #[test]
    fn build_report_defaults_to_latest_milestone() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        for (task, ts) in [("M10-T01", "20260101-100000"), ("M25-T01", "20260609-100000")] {
            let v = serde_json::json!({
                "task_id": task, "redispatch": 0, "timestamp": ts, "verdict": "PASS"
            });
            std::fs::write(
                sessions.join(format!("{}-conductor-{}-R0.json", task, ts)),
                serde_json::to_string(&v).unwrap(),
            )
            .unwrap();
        }
        let report = build_report(tmp.path(), None);
        assert_eq!(report.milestone, Some(25), "미지정 시 최신 마일스톤");
        assert_eq!(report.total(), 1);
    }

    #[test]
    fn render_markdown_contains_key_fields() {
        let report = MilestoneRunReport {
            milestone: Some(25),
            tasks: aggregate(&[
                rec("M25-T01", 0, "t", "PASS", false),
                rec("M25-T02", 2, "t", "FAIL", true),
            ]),
            parse_errors: 0,
        };
        let md = render_markdown(&report);
        assert!(md.contains("마일스톤 M25"));
        assert!(md.contains("M25-T01"));
        assert!(md.contains("M25-T02"));
        assert!(md.contains("PASS"));
        assert!(md.contains("FAIL"));
        assert!(md.contains("성공률"));
        // 폴백 표기
        assert!(md.contains("예"));
    }

    #[test]
    fn load_records_tolerates_utf8_bom() {
        // 일부 도구(PowerShell Set-Content -Encoding UTF8 등)는 BOM을 붙인다.
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let json = serde_json::json!({
            "task_id": "M1-T01", "redispatch": 0, "timestamp": "t", "verdict": "PASS"
        });
        let with_bom = format!("\u{feff}{}", serde_json::to_string(&json).unwrap());
        std::fs::write(
            sessions.join("M1-T01-conductor-20260609-100000-R0.json"),
            with_bom,
        )
        .unwrap();

        let (records, errors) = load_records(tmp.path());
        assert_eq!(errors, 0, "BOM이 있어도 파싱 성공해야 함");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].task_id, "M1-T01");
    }

    #[test]
    fn empty_sessions_no_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let report = build_report(tmp.path(), None);
        assert_eq!(report.total(), 0);
        assert_eq!(report.parse_errors, 0);
    }
}
