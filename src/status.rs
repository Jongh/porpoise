use colored::Colorize;
use std::path::Path;

use crate::config::workspace::WorkspaceConfig;
use crate::model::adapter::AdapterType;
use crate::orchestrator::checkpoint::load_checkpoint;

struct TaskStatus {
    id: String,
    title: String,
    completed: bool,
    is_current: bool,
    dependencies: Vec<String>,
}

struct StatusInfo {
    adapter: String,
    /// 실행 모드 표시 (claude_code: "conductor (기본 ON)"/"legacy", API 어댑터: None)
    run_mode: Option<String>,
    model_id: Option<String>,
    task_id: Option<String>,
    role: Option<String>,
    cycle: u32,
    milestone_number: Option<u32>,
    milestone_title: Option<String>,
    tasks: Vec<TaskStatus>,
    session_count: usize,
}

pub fn run_status(project_path: &Path) {
    println!();
    println!("{}", "Porpoise Status".cyan().bold());
    println!("{}", "─────────────────────────────────────".dimmed());

    let porpoise_dir = project_path.join(".porpoise");
    if !porpoise_dir.exists() {
        println!("{}", "⚠ .porpoise/ 없음 — 'porpoise --new'로 초기화하세요.".yellow());
        println!();
        return;
    }

    let info = collect_status(project_path);

    println!("프로젝트: {}", project_path.display().to_string().cyan());
    println!("어댑터: {}", info.adapter.cyan());
    if let Some(ref mode) = info.run_mode {
        println!("실행 모드: {}", mode.cyan());
    }
    if let Some(ref model) = info.model_id {
        println!("모델: {}", model.dimmed());
    }
    println!("{}", "─────────────────────────────────────".dimmed());

    if let (Some(ref task), Some(ref role)) = (&info.task_id, &info.role) {
        println!(
            "현재 태스크: {}  |  단계: {}  |  사이클: {}",
            task.cyan().bold(),
            role.yellow(),
            info.cycle
        );
    } else {
        println!("{}", "진행 중인 태스크 없음 (checkpoint 없음)".dimmed());
    }

    println!("{}", "─────────────────────────────────────".dimmed());

    if let (Some(num), Some(ref title)) = (info.milestone_number, &info.milestone_title) {
        let total = info.tasks.len();
        let done = info.tasks.iter().filter(|t| t.completed).count();
        println!(
            "마일스톤 M{}: {}  ({}/{} 완료)",
            num,
            title.bold(),
            done,
            total
        );
        // M24: 의존성 충족 여부로 ready/대기 구분
        let completed_ids: std::collections::HashSet<&str> =
            info.tasks.iter().filter(|t| t.completed).map(|t| t.id.as_str()).collect();
        for t in &info.tasks {
            let waiting = !t.completed
                && !t.dependencies.is_empty()
                && !t.dependencies.iter().all(|d| completed_ids.contains(d.as_str()));
            let marker = if t.completed {
                "✅"
            } else if t.is_current {
                "🔄"
            } else if waiting {
                "🔒"
            } else {
                "⏳"
            };
            let mut label = if t.is_current {
                format!("{}   ← 현재", t.title).yellow().to_string()
            } else {
                t.title.clone()
            };
            if waiting {
                label = format!("{}  (대기: {})", label, t.dependencies.join(", ")).dimmed().to_string();
            }
            println!("  {} {}: {}", marker, t.id.dimmed(), label);
        }
    } else {
        println!("{}", "마일스톤 정보 없음".dimmed());
    }

    println!("{}", "─────────────────────────────────────".dimmed());
    println!("sessions/: {}개 파일", info.session_count);

    // M25: 최근 실행 요약 (감사 기록 집계 — 기록 없으면 생략)
    let report = crate::conductor::report::build_report(project_path, None);
    if !report.tasks.is_empty() {
        let ms = report
            .milestone
            .map(|m| format!("M{}", m))
            .unwrap_or_else(|| "전체".to_string());
        println!("{}", "─────────────────────────────────────".dimmed());
        println!(
            "최근 실행 ({}): PASS {}/{} · 성공률 {} · 재투입 {} · 폴백 {}",
            ms.cyan(),
            report.passed(),
            report.total(),
            format!("{:.0}%", report.success_rate()).bold(),
            report.total_redispatches(),
            report.fallback_count()
        );
        println!("{}", "  ('porpoise report'로 상세 보기)".dimmed());
    }
    println!();
}

fn collect_status(project_path: &Path) -> StatusInfo {
    // 1. workspace.toml
    let workspace = WorkspaceConfig::load(project_path).unwrap_or_default();

    let adapter = match workspace.model_adapter_type() {
        AdapterType::ClaudeCode => "claude_code".to_string(),
        AdapterType::AnthropicApi => "anthropic_api".to_string(),
        AdapterType::OpenAiCompatible => "openai_compatible".to_string(),
    };

    // 실행 모드: claude_code만 conductor 적용 (API 어댑터는 항상 legacy → None)
    let run_mode = if matches!(workspace.model_adapter_type(), AdapterType::ClaudeCode) {
        if workspace.conductor_enabled() {
            let basis = if workspace.conductor_mode_unset() { "기본 ON" } else { "명시" };
            Some(format!("conductor ({})", basis))
        } else {
            Some("legacy (opt-out)".to_string())
        }
    } else {
        None
    };

    let model_id = workspace
        .model
        .as_ref()
        .and_then(|m| m.model_id.as_deref())
        .map(str::to_string);

    // 2. checkpoint.json
    let (task_id, role, cycle) = match load_checkpoint(project_path) {
        Ok(cp) => {
            let tid = if cp.current_task_id.is_empty() {
                None
            } else {
                Some(cp.current_task_id)
            };
            let r = if cp.current_role.is_empty() {
                None
            } else {
                Some(cp.current_role)
            };
            (tid, r, cp.cycle)
        }
        Err(_) => (None, None, 1),
    };

    // 3. 현재 마일스톤 번호 파악 (task_id에서 추출 또는 최신 milestone)
    let milestone_number = task_id
        .as_deref()
        .and_then(extract_milestone_number)
        .or_else(|| find_latest_milestone_number(project_path));

    // 4. 마일스톤 태스크 목록 파싱
    let (milestone_title, tasks) = if let Some(num) = milestone_number {
        load_milestone_tasks(project_path, num, task_id.as_deref())
    } else {
        (None, vec![])
    };

    // 5. sessions/ 파일 수
    let session_count = count_session_files(project_path);

    StatusInfo {
        adapter,
        run_mode,
        model_id,
        task_id,
        role,
        cycle,
        milestone_number,
        milestone_title,
        tasks,
        session_count,
    }
}

fn extract_milestone_number(task_id: &str) -> Option<u32> {
    // "M19-T03" → 19
    let rest = task_id.strip_prefix('M')?;
    let dash_pos = rest.find('-')?;
    rest[..dash_pos].parse().ok()
}

fn find_latest_milestone_number(project_path: &Path) -> Option<u32> {
    let milestones_dir = project_path.join(".porpoise").join("milestones");
    let entries = std::fs::read_dir(&milestones_dir).ok()?;
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('M') && name.ends_with(".md") && !name.contains('-') {
                name[1..name.len() - 3].parse::<u32>().ok()
            } else {
                None
            }
        })
        .max()
}

fn load_milestone_tasks(
    project_path: &Path,
    milestone_num: u32,
    current_task_id: Option<&str>,
) -> (Option<String>, Vec<TaskStatus>) {
    let path = project_path
        .join(".porpoise")
        .join("milestones")
        .join(format!("M{}.md", milestone_num));

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (None, vec![]),
    };

    // 제목 파싱: "# M19: 제목 (v0.19.0)" 형식
    let title = content
        .lines()
        .next()
        .and_then(|line| {
            let rest = line.strip_prefix("# M")?;
            let colon = rest.find(": ")?;
            let title_part = rest[colon + 2..].trim();
            // "(vX.Y.Z)" 접미사 제거
            if let Some(paren) = title_part.rfind(" (") {
                let after = &title_part[paren + 2..];
                if after.ends_with(')') && after.starts_with('v') {
                    return Some(title_part[..paren].to_string());
                }
            }
            Some(title_part.to_string())
        });

    // 태스크 목록 파싱
    let mut tasks = vec![];
    let mut in_task_section = false;

    for line in content.lines() {
        let header = line.trim_start();
        // 마일스톤 문서 규약 불일치 대응: 파서(parser.rs)·M1~M18은 "작업 목록",
        // M19/M20은 "태스크 목록"을 쓴다. 두 헤더를 모두 인식한다.
        if header.starts_with("## 작업 목록") || header.starts_with("## 태스크 목록") {
            in_task_section = true;
            continue;
        }
        if in_task_section && header.starts_with("## ") {
            break;
        }
        if !in_task_section {
            continue;
        }
        let trimmed = line.trim();
        let completed = if trimmed.starts_with("- [x] ") {
            true
        } else if trimmed.starts_with("- [ ] ") {
            false
        } else {
            continue;
        };

        let rest = &trimmed[6..];
        if let Some(colon) = rest.find(": ") {
            let id = rest[..colon].trim().to_string();
            let raw_title = rest[colon + 2..].trim();
            let (task_title, dependencies) =
                crate::orchestrator::state::parse_task_deps(raw_title);
            if id.starts_with('M') && id.contains("-T") {
                let is_current = current_task_id.map(|c| c == id).unwrap_or(false);
                tasks.push(TaskStatus {
                    id,
                    title: task_title,
                    completed,
                    is_current,
                    dependencies,
                });
            }
        }
    }

    (title, tasks)
}

fn count_session_files(project_path: &Path) -> usize {
    let sessions_dir = project_path.join(".porpoise").join("sessions");
    match std::fs::read_dir(&sessions_dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.path().is_file())
            .count(),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_milestone_number_parses_task_id() {
        assert_eq!(extract_milestone_number("M19-T03"), Some(19));
        assert_eq!(extract_milestone_number("M1-T01"), Some(1));
        assert_eq!(extract_milestone_number("M100-T99"), Some(100));
        assert_eq!(extract_milestone_number("invalid"), None);
    }

    #[test]
    fn run_status_no_porpoise_dir_prints_warning() {
        let tmp = tempfile::tempdir().unwrap();
        // .porpoise/ 없음 — 경고 출력 후 반환 (패닉 없음 확인)
        run_status(tmp.path());
    }

    #[test]
    fn run_status_with_minimal_setup() {
        let tmp = tempfile::tempdir().unwrap();
        let porpoise_dir = tmp.path().join(".porpoise");
        std::fs::create_dir_all(porpoise_dir.join("sessions")).unwrap();
        std::fs::write(
            porpoise_dir.join("workspace.toml"),
            "[general]\nlanguage = \"ko\"\n",
        )
        .unwrap();
        // 마일스톤 파일 생성
        std::fs::create_dir_all(porpoise_dir.join("milestones")).unwrap();
        std::fs::write(
            porpoise_dir.join("milestones").join("M1.md"),
            "# M1: 테스트 마일스톤 (v0.1.0)\n\n## 태스크 목록\n- [ ] M1-T01: 첫 번째 태스크\n- [x] M1-T02: 완료된 태스크\n\n## 메타데이터\n- status: active\n",
        )
        .unwrap();
        // 패닉 없이 실행 확인
        run_status(tmp.path());
    }

    #[test]
    fn load_milestone_tasks_parses_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let milestones_dir = tmp.path().join(".porpoise").join("milestones");
        std::fs::create_dir_all(&milestones_dir).unwrap();
        std::fs::write(
            milestones_dir.join("M5.md"),
            "# M5: 상태 명령 (v0.5.0)\n\n## 태스크 목록\n- [x] M5-T01: 완료\n- [ ] M5-T02: 진행 중\n- [ ] M5-T03: 대기\n\n## 메타데이터\n",
        )
        .unwrap();

        let (title, tasks) = load_milestone_tasks(tmp.path(), 5, Some("M5-T02"));
        assert_eq!(title.as_deref(), Some("상태 명령"));
        assert_eq!(tasks.len(), 3);
        assert!(tasks[0].completed);
        assert!(!tasks[1].completed);
        assert!(tasks[1].is_current);
        assert!(!tasks[2].is_current);
    }

    #[test]
    fn load_milestone_tasks_accepts_jakeop_header() {
        // "작업 목록" 헤더(파서·M1~M18 규약)도 인식해야 한다 (헤더 불일치 버그 수정)
        let tmp = tempfile::tempdir().unwrap();
        let milestones_dir = tmp.path().join(".porpoise").join("milestones");
        std::fs::create_dir_all(&milestones_dir).unwrap();
        std::fs::write(
            milestones_dir.join("M6.md"),
            "# M6: 작업 목록 헤더 (v0.6.0)\n\n## 작업 목록\n- [x] M6-T01: 완료\n- [ ] M6-T02: 진행\n\n## 메타데이터\n",
        )
        .unwrap();

        let (title, tasks) = load_milestone_tasks(tmp.path(), 6, Some("M6-T02"));
        assert_eq!(title.as_deref(), Some("작업 목록 헤더"));
        assert_eq!(tasks.len(), 2, "'작업 목록' 헤더도 파싱되어야 함");
        assert!(tasks[0].completed);
        assert!(tasks[1].is_current);
    }

    #[test]
    fn count_session_files_returns_correct_count() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join(".porpoise").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(sessions_dir.join("a.json"), "{}").unwrap();
        std::fs::write(sessions_dir.join("b.json"), "{}").unwrap();

        assert_eq!(count_session_files(tmp.path()), 2);
    }
}
