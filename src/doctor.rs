use colored::Colorize;
use std::path::Path;

use crate::config::workspace::WorkspaceConfig;
use crate::model::adapter::AdapterType;
use crate::model::openai_compatible::{check_ollama_availability, is_ollama_endpoint};

pub struct CheckResult {
    pub label: String,
    pub ok: bool,
    pub message: String,
    pub hint: Option<String>,
}

pub fn run_doctor(project_path: &Path) -> usize {
    println!();
    println!("{}", "Porpoise Doctor — 설정 진단".cyan().bold());
    println!("{}", "─────────────────────────────────────".dimmed());

    let results = collect_checks(project_path);
    let total = results.len();
    let passed = results.iter().filter(|r| r.ok).count();

    for r in &results {
        if r.ok {
            println!("✅ {}: {}", r.label.cyan(), r.message);
        } else {
            println!("❌ {}: {}", r.label.red(), r.message);
            if let Some(hint) = &r.hint {
                for line in hint.lines() {
                    println!("   → {}", line.dimmed());
                }
            }
        }
    }

    println!("{}", "─────────────────────────────────────".dimmed());
    let failures = total - passed;
    if failures == 0 {
        println!(
            "{}",
            format!("진단 완료: {}/{} 항목 통과", passed, total).green().bold()
        );
    } else {
        println!(
            "{}",
            format!("진단 완료: {}/{} 항목 통과 ({} 실패)", passed, total, failures)
                .yellow()
                .bold()
        );
    }
    println!();
    failures
}

fn collect_checks(project_path: &Path) -> Vec<CheckResult> {
    let mut results = Vec::new();

    // 1. workspace.toml 존재 및 파싱
    let workspace_path = project_path.join(".porpoise").join("workspace.toml");
    if !workspace_path.exists() {
        results.push(CheckResult {
            label: "workspace.toml".to_string(),
            ok: false,
            message: "파일 없음".to_string(),
            hint: Some("먼저 'porpoise --new'로 프로젝트를 초기화하세요.".to_string()),
        });
        return results;
    }

    let workspace = match WorkspaceConfig::load(project_path) {
        Ok(ws) => {
            results.push(CheckResult {
                label: "workspace.toml".to_string(),
                ok: true,
                message: "파싱 성공".to_string(),
                hint: None,
            });
            ws
        }
        Err(e) => {
            results.push(CheckResult {
                label: "workspace.toml".to_string(),
                ok: false,
                message: format!("파싱 오류: {}", e),
                hint: Some("workspace.toml 문법을 확인하세요.".to_string()),
            });
            return results;
        }
    };

    let adapter_type = workspace.model_adapter_type();

    // 2. 어댑터 타입 표시
    let adapter_label = match adapter_type {
        AdapterType::ClaudeCode => "claude_code",
        AdapterType::AnthropicApi => "anthropic_api",
        AdapterType::OpenAiCompatible => "openai_compatible",
    };
    results.push(CheckResult {
        label: "어댑터".to_string(),
        ok: true,
        message: adapter_label.to_string(),
        hint: None,
    });

    // 3. Claude CLI 설치 여부 (claude_code 어댑터만)
    if adapter_type == AdapterType::ClaudeCode {
        match which::which("claude") {
            Ok(path) => results.push(CheckResult {
                label: "claude CLI".to_string(),
                ok: true,
                message: path.display().to_string(),
                hint: None,
            }),
            Err(_) => results.push(CheckResult {
                label: "claude CLI".to_string(),
                ok: false,
                message: "미설치".to_string(),
                hint: Some("npm install -g @anthropic-ai/claude-code".to_string()),
            }),
        }
    }

    // 4. API 키 env var 설정 여부 (API 어댑터만)
    if adapter_type != AdapterType::ClaudeCode {
        let env_name = match adapter_type {
            AdapterType::AnthropicApi => workspace
                .openai_api_key_env()
                .filter(|s| !s.is_empty())
                .unwrap_or("ANTHROPIC_API_KEY")
                .to_string(),
            AdapterType::OpenAiCompatible => workspace
                .openai_api_key_env()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            AdapterType::ClaudeCode => unreachable!(),
        };

        if env_name.is_empty() {
            results.push(CheckResult {
                label: "API 키".to_string(),
                ok: true,
                message: "인증 불필요 (무인증 엔드포인트)".to_string(),
                hint: None,
            });
        } else {
            match std::env::var(&env_name) {
                Ok(_) => results.push(CheckResult {
                    label: "API 키".to_string(),
                    ok: true,
                    message: format!("{}: 설정됨", env_name),
                    hint: None,
                }),
                Err(_) => results.push(CheckResult {
                    label: "API 키".to_string(),
                    ok: false,
                    message: format!("{}: 미설정", env_name),
                    hint: Some(format!(
                        "Windows (PowerShell): $env:{} = \"실제키값\"\nmacOS / Linux:        export {}=\"실제키값\"",
                        env_name, env_name
                    )),
                }),
            }
        }
    }

    // 5. Ollama 서버 응답 확인 (openai_compatible + ollama 엔드포인트만)
    if adapter_type == AdapterType::OpenAiCompatible {
        if let Some(base_url) = workspace.openai_api_base_url() {
            if is_ollama_endpoint(base_url) {
                let model_id = workspace
                    .model
                    .as_ref()
                    .and_then(|m| m.model_id.as_deref())
                    .unwrap_or("unknown");
                match check_ollama_availability(base_url, model_id) {
                    Ok(()) => results.push(CheckResult {
                        label: "Ollama 서버".to_string(),
                        ok: true,
                        message: format!("{}: 응답 정상, 모델 '{}' 확인됨", base_url, model_id),
                        hint: None,
                    }),
                    Err(e) => results.push(CheckResult {
                        label: "Ollama 서버".to_string(),
                        ok: false,
                        message: e.to_string(),
                        hint: Some("'ollama serve'로 서버를 시작하거나 'ollama pull <모델>'로 모델을 설치하세요.".to_string()),
                    }),
                }
            }
        }
    }

    // 6. sessions/ 디렉토리 존재 (신규 포맷 여부)
    let sessions_dir = project_path.join(".porpoise").join("sessions");
    if sessions_dir.is_dir() {
        results.push(CheckResult {
            label: "sessions/".to_string(),
            ok: true,
            message: "존재함 (신규 포맷)".to_string(),
            hint: None,
        });
    } else {
        results.push(CheckResult {
            label: "sessions/".to_string(),
            ok: false,
            message: "없음 (레거시 모드 또는 미초기화)".to_string(),
            hint: Some(
                "'porpoise migrate'로 신규 포맷으로 전환하거나 'porpoise --new'로 재초기화하세요."
                    .to_string(),
            ),
        });
    }

    // 6.5 지휘자(conductor) 모드 진단 (claude_code 어댑터 한정)
    if adapter_type == AdapterType::ClaudeCode {
        let enabled = workspace.conductor_enabled();
        if enabled {
            // conductor는 git worktree가 필요
            if crate::conductor::git::is_git_repo(project_path) {
                let verifier = workspace
                    .conductor_verifier_model()
                    .unwrap_or("(Dispatch와 동일)");
                let basis = if workspace.conductor_mode_unset() { "기본 ON" } else { "명시적" };
                let parallel = workspace.conductor_max_parallel();
                let par_str = if parallel > 1 {
                    format!(", 병렬: {}개", parallel)
                } else {
                    String::new()
                };
                // M28: 예산 상한 설정 시 표시
                let budget_str = workspace
                    .conductor_budget_usd()
                    .map(|b| format!(", 예산: ${:.4}", b))
                    .unwrap_or_default();
                results.push(CheckResult {
                    label: "conductor".to_string(),
                    ok: true,
                    message: format!(
                        "활성 ({}, 재투입 한도: {}, 검증자: {}{}{})",
                        basis,
                        workspace.conductor_max_redispatch(),
                        verifier,
                        par_str,
                        budget_str
                    ),
                    hint: None,
                });
            } else {
                results.push(CheckResult {
                    label: "conductor".to_string(),
                    ok: false,
                    message: "활성이지만 git 저장소가 아님".to_string(),
                    hint: Some(
                        "'git init'으로 저장소를 초기화하거나 workspace.toml에 \
                         [conductor] mode = \"legacy\"를 설정하세요."
                            .to_string(),
                    ),
                });
            }
        } else {
            results.push(CheckResult {
                label: "conductor".to_string(),
                ok: true,
                message: "legacy 모드 (명시적 opt-out) — 기본 conductor 대신 4단계 phase 방식 사용".to_string(),
                hint: None,
            });
        }
    }

    // 7. 최근 마일스톤 파일
    let milestones_dir = project_path.join(".porpoise").join("milestones");
    match std::fs::read_dir(&milestones_dir) {
        Ok(entries) => {
            let mut milestone_files: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    // M{N}.md 형식만 (M1.md, M18.md 등 — 하이픈 없는 것)
                    if name.starts_with('M')
                        && name.ends_with(".md")
                        && !name.contains('-')
                        && name[1..name.len() - 3].chars().all(|c| c.is_ascii_digit())
                    {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect();
            milestone_files.sort_by_key(|s| {
                s.trim_start_matches('M')
                    .trim_end_matches(".md")
                    .parse::<u32>()
                    .unwrap_or(0)
            });

            if let Some(latest) = milestone_files.last() {
                results.push(CheckResult {
                    label: "최근 마일스톤".to_string(),
                    ok: true,
                    message: latest.clone(),
                    hint: None,
                });
            } else {
                results.push(CheckResult {
                    label: "최근 마일스톤".to_string(),
                    ok: false,
                    message: "마일스톤 없음".to_string(),
                    hint: Some("'porpoise'를 실행하여 첫 마일스톤을 생성하세요.".to_string()),
                });
            }
        }
        Err(_) => {
            results.push(CheckResult {
                label: "최근 마일스톤".to_string(),
                ok: false,
                message: "milestones/ 디렉토리 없음".to_string(),
                hint: Some("'porpoise --new'로 초기화하세요.".to_string()),
            });
        }
    }

    // 8. 의존성 그래프 검증 (M24) — 순환·dangling
    {
        let tasks = crate::orchestrator::state::parse_tasks_from_project_md(project_path);
        let has_deps = tasks.iter().any(|t| !t.dependencies.is_empty());
        if has_deps {
            match crate::conductor::schedule::validate_dependencies(&tasks) {
                Ok(warnings) if warnings.is_empty() => results.push(CheckResult {
                    label: "의존성 그래프".to_string(),
                    ok: true,
                    message: "유효 (순환·dangling 없음)".to_string(),
                    hint: None,
                }),
                Ok(warnings) => results.push(CheckResult {
                    label: "의존성 그래프".to_string(),
                    ok: false,
                    message: format!("dangling 의존성 {}건", warnings.len()),
                    hint: Some(warnings.join("\n")),
                }),
                Err(e) => results.push(CheckResult {
                    label: "의존성 그래프".to_string(),
                    ok: false,
                    message: e,
                    hint: Some("project.md의 (deps: ...) 표기에서 순환을 제거하세요.".to_string()),
                }),
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_checks_returns_early_when_no_workspace_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let results = collect_checks(tmp.path());
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert!(results[0].message.contains("파일 없음"));
    }

    #[test]
    fn collect_checks_shows_sessions_missing_for_fresh_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let porpoise_dir = tmp.path().join(".porpoise");
        std::fs::create_dir_all(&porpoise_dir).unwrap();
        // Create minimal workspace.toml
        std::fs::write(
            porpoise_dir.join("workspace.toml"),
            "[general]\nlanguage = \"ko\"\n",
        )
        .unwrap();

        let results = collect_checks(tmp.path());
        let ws_check = results.iter().find(|r| r.label == "workspace.toml");
        assert!(ws_check.is_some());
        assert_eq!(ws_check.unwrap().message, "파싱 성공");
        let sessions_check = results.iter().find(|r| r.label == "sessions/");
        assert!(sessions_check.is_some());
        assert!(!sessions_check.unwrap().ok);
    }

    #[test]
    fn collect_checks_shows_sessions_ok_when_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let porpoise_dir = tmp.path().join(".porpoise");
        std::fs::create_dir_all(porpoise_dir.join("sessions")).unwrap();
        std::fs::write(
            porpoise_dir.join("workspace.toml"),
            "[general]\nlanguage = \"ko\"\n",
        )
        .unwrap();

        let results = collect_checks(tmp.path());
        let ws_check = results.iter().find(|r| r.label == "workspace.toml");
        assert!(ws_check.is_some());
        assert_eq!(ws_check.unwrap().message, "파싱 성공");
        let sessions_check = results.iter().find(|r| r.label == "sessions/");
        assert!(sessions_check.is_some());
        assert!(sessions_check.unwrap().ok);
    }
}
