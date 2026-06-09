use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::session::v0_7::VerifyCommand;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceGeneral {
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceDod {
    pub items: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceConventions {
    pub custom_rules: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceRoles {
    pub pm_extra: Option<String>,
    pub developer_extra: Option<String>,
    pub tester_extra: Option<String>,
    pub reviewer_extra: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspacePromptOverrides {
    pub pm: Option<String>,
    pub developer: Option<String>,
    pub tester: Option<String>,
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceModel {
    pub adapter: Option<String>,
    pub model_id: Option<String>,
    pub api_base_url: Option<String>,
    pub per_role: Option<WorkspaceModelPerRole>,
    pub api_key_env: Option<String>,
    pub structured_output_mode: Option<String>,
    pub snapshot_token_budget: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceTech {
    pub stack: Option<String>,
    pub build_command: Option<String>,
    pub test_command: Option<String>,
    pub lint_command: Option<String>,
    pub verify_commands: Option<Vec<crate::session::v0_7::VerifyCommand>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceSecurity {
    pub allowed_command_prefixes: Option<Vec<String>>,
    pub verify_timeout_secs: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceModelPerRole {
    pub planning: Option<String>,
    pub development: Option<String>,
    pub testing: Option<String>,
    pub review: Option<String>,
    pub milestone: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceSessions {
    /// 완료된 마일스톤의 세션 파일 보존 여부 (기본값: false — 정리 대상)
    pub keep_completed_milestone_sessions: Option<bool>,
    /// 이 일수를 초과한 세션 파일 자동 삭제 (기본값: 30, 0 = 무제한)
    pub max_session_age_days: Option<u32>,
}

/// `[conductor]` — 에이전트 함대 지휘자 루프 설정 (M10+).
/// task 하나를 실제 코딩 에이전트에게 통째로 위임(Dispatch)하고
/// 독립 검증자(Verify)가 PASS/FAIL을 판정하는 신규 실행 경로를 제어한다.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceConductor {
    /// "conductor" (기본, v0.22.0~) | "legacy" — legacy는 기존 4단계 phase 오케스트레이션 사용
    pub mode: Option<String>,
    /// 검증자 모델 ID (생략 시 Dispatch와 동일 모델). 독립 검증을 위해 다른 모델 권장.
    pub verifier_model: Option<String>,
    /// Verify FAIL 시 재투입(re-dispatch) 최대 횟수 (기본값: 2)
    pub max_redispatch: Option<u32>,
    /// 검증자 verdict 파싱 실패 시 폴백 정책:
    /// "pass_if_checks_pass" (기본 — 검증 명령 전부 통과면 객관 증거로 PASS) | "halt" (보수 — 사용자 검토)
    pub verdict_fallback: Option<String>,
    /// 동시에 dispatch·verify할 task 수 (기본 1 = 순차). >1이면 병렬 함대(독립 task 전제, M23).
    pub max_parallel: Option<u32>,
    /// 누적 비용(USD) 상한 (M28). 설정 시 누적 비용이 도달하면 다음 dispatch 전 중단.
    /// 생략·0 이하이면 무제한(기존 동작).
    pub budget_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub general: Option<WorkspaceGeneral>,
    pub dod: Option<WorkspaceDod>,
    pub conventions: Option<WorkspaceConventions>,
    pub roles: Option<WorkspaceRoles>,
    pub prompt_overrides: Option<WorkspacePromptOverrides>,
    pub model: Option<WorkspaceModel>,
    pub tech: Option<WorkspaceTech>,
    pub security: Option<WorkspaceSecurity>,
    pub sessions: Option<WorkspaceSessions>,
    pub conductor: Option<WorkspaceConductor>,
}

impl WorkspaceConfig {
    pub fn load(project_path: &Path) -> Result<Self> {
        let ws_path = project_path.join(".porpoise").join("workspace.toml");
        if !ws_path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&ws_path)?;
        let config: WorkspaceConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn dod_items(&self) -> Vec<String> {
        self.dod
            .as_ref()
            .and_then(|d| d.items.as_ref())
            .cloned()
            .unwrap_or_else(|| {
                vec![
                    "코드 리뷰 통과".to_string(),
                    "테스트 통과".to_string(),
                    "문서화 완료".to_string(),
                ]
            })
    }

    pub fn convention_lines(&self) -> Vec<String> {
        let mut lines = vec![
            "커밋 메시지: 한국어 허용".to_string(),
            "브랜치 전략: main 브랜치 직접 커밋".to_string(),
        ];
        if let Some(rules) = self.conventions.as_ref().and_then(|c| c.custom_rules.as_ref()) {
            for rule in rules {
                if !rule.trim().is_empty() {
                    lines.push(rule.clone());
                }
            }
        }
        lines
    }

    /// Returns formatted extra instructions for the given role key, or empty string.
    /// Role keys: "pm", "developer", "tester", "reviewer"
    pub fn role_extra_formatted(&self, role_key: &str) -> String {
        let raw = self
            .roles
            .as_ref()
            .map(|r| {
                match role_key {
                    "pm" => r.pm_extra.as_deref().unwrap_or(""),
                    "developer" => r.developer_extra.as_deref().unwrap_or(""),
                    "tester" => r.tester_extra.as_deref().unwrap_or(""),
                    "reviewer" => r.reviewer_extra.as_deref().unwrap_or(""),
                    _ => "",
                }
                .to_string()
            })
            .unwrap_or_default();

        if raw.trim().is_empty() {
            String::new()
        } else {
            format!("\n## 추가 지시사항 (workspace.toml)\n\n{}\n", raw.trim())
        }
    }

    /// Returns the language string from [general].language, defaulting to "ko".
    pub fn language(&self) -> &str {
        self.general
            .as_ref()
            .and_then(|g| g.language.as_deref())
            .unwrap_or("ko")
    }

    /// Returns the raw override path string for the given role key, if configured.
    pub fn prompt_override_path(&self, role_key: &str) -> Option<&str> {
        self.prompt_overrides.as_ref().and_then(|po| match role_key {
            "pm" => po.pm.as_deref(),
            "developer" => po.developer.as_deref(),
            "tester" => po.tester.as_deref(),
            "reviewer" => po.reviewer.as_deref(),
            _ => None,
        })
    }

    /// Resolves the override path for the given role key against the project root.
    pub fn resolved_override_path(&self, role_key: &str, project_path: &Path) -> Option<std::path::PathBuf> {
        self.prompt_override_path(role_key).map(|p| resolve_path(p, project_path))
    }

    /// Reads prompt override file for the given role key, if configured in [prompt_overrides].
    /// Returns None if not configured or file cannot be read (falls back to default template).
    pub fn prompt_override_content(&self, role_key: &str, project_path: &Path) -> Option<String> {
        let full_path = self.resolved_override_path(role_key, project_path)?;
        match std::fs::read_to_string(&full_path) {
            Ok(content) => Some(content),
            Err(_) => {
                let raw = self.prompt_override_path(role_key).unwrap_or("?");
                eprintln!(
                    "⚠ 프롬프트 오버라이드 파일을 읽을 수 없습니다: {} (기본 템플릿 사용)",
                    raw
                );
                None
            }
        }
    }

    pub fn model_adapter_type(&self) -> crate::model::adapter::AdapterType {
        use crate::model::adapter::AdapterType;
        self.model.as_ref()
            .and_then(|m| m.adapter.as_deref())
            .map(|a| match a {
                "anthropic_api" => AdapterType::AnthropicApi,
                "openai_compatible" => AdapterType::OpenAiCompatible,
                _ => AdapterType::ClaudeCode,
            })
            .unwrap_or(AdapterType::ClaudeCode)
    }

    pub fn allowed_command_prefixes(&self) -> Vec<String> {
        self.security
            .as_ref()
            .and_then(|s| s.allowed_command_prefixes.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    pub fn verify_timeout_secs(&self) -> u32 {
        self.security
            .as_ref()
            .and_then(|s| s.verify_timeout_secs)
            .unwrap_or(60)
    }

    pub fn snapshot_token_budget(&self) -> usize {
        self.model
            .as_ref()
            .and_then(|m| m.snapshot_token_budget)
            .unwrap_or(32_000) as usize
    }

    pub fn default_verify_commands(&self) -> Vec<VerifyCommand> {
        let tech = match self.tech.as_ref() {
            Some(t) => t,
            None => return vec![],
        };
        // verify_commands 배열이 있으면 우선 사용 (test_command/lint_command 무시)
        if let Some(cmds) = &tech.verify_commands {
            if !cmds.is_empty() {
                return cmds.clone();
            }
        }
        // 폴백: 기존 단일 명령 파싱
        let mut cmds = Vec::new();
        if let Some(test_cmd) = &tech.test_command {
            cmds.extend(parse_command_string_multi(test_cmd, "테스트 실행"));
        }
        if let Some(lint_cmd) = &tech.lint_command {
            cmds.extend(parse_command_string_multi(lint_cmd, "린트 검사"));
        }
        cmds
    }

    pub fn tech_context(&self) -> Option<String> {
        let tech = self.tech.as_ref()?;
        let mut parts = Vec::new();
        if let Some(stack) = &tech.stack {
            parts.push(format!("Tech Stack: {}", stack));
        }
        if let Some(build) = &tech.build_command {
            parts.push(format!("Build: `{}`", build));
        }
        if let Some(test) = &tech.test_command {
            parts.push(format!("Test: `{}`", test));
        }
        if let Some(lint) = &tech.lint_command {
            parts.push(format!("Lint: `{}`", lint));
        }
        if parts.is_empty() { None } else { Some(parts.join("\n")) }
    }

    pub fn openai_api_key_env(&self) -> Option<&str> {
        self.model.as_ref()?.api_key_env.as_deref()
    }

    pub fn structured_output_mode(&self) -> &str {
        self.model
            .as_ref()
            .and_then(|m| m.structured_output_mode.as_deref())
            .unwrap_or("auto")
    }

    pub fn openai_api_base_url(&self) -> Option<&str> {
        self.model.as_ref()?.api_base_url.as_deref()
    }

    pub fn model_id_for_role(&self, role: &crate::orchestrator::state::Role) -> Option<&str> {
        let m = self.model.as_ref()?;
        let per_role_id = m.per_role.as_ref().and_then(|pr| {
            use crate::orchestrator::state::Role;
            match role {
                Role::PM => pr.planning.as_deref(),
                Role::Developer => pr.development.as_deref(),
                Role::Tester => pr.testing.as_deref(),
                Role::Reviewer => pr.review.as_deref(),
            }
        });
        per_role_id.or(m.model_id.as_deref())
    }

    pub fn default_toml() -> &'static str {
        r#"# Porpoise 작업 환경 설정
# 이 파일은 .porpoise/ 폴더에 저장됩니다.
# git 추적 여부는 .gitignore로 독립적으로 제어할 수 있습니다.
# porpoise.toml (프로젝트 루트)의 시스템 설정과 별도로 관리됩니다.

[general]
# 작업 언어 (ko = 한국어, en = 영어)
language = "ko"

[dod]
# 완료 기준 (Definition of Done) — .porpoise/project.md에 포함됩니다
# porpoise --new 실행 시 이 목록이 project.md에 반영됩니다
items = [
    "코드 리뷰 통과",
    "테스트 통과",
    "문서화 완료",
]

[conventions]
# 프로젝트 코딩 규칙 (project.md의 컨벤션 섹션에 추가됩니다)
custom_rules = []

[roles]
# 각 단계 프롬프트에 추가될 지시사항 (비어 있으면 생략됩니다)
pm_extra = ""
developer_extra = ""
tester_extra = ""
reviewer_extra = ""

[prompt_overrides]
# 단계 프롬프트를 완전히 교체할 파일 경로 (주석 처리하면 기본 템플릿 사용)
# pm = ".porpoise/custom-prompts/01-planning.md"
# developer = ".porpoise/custom-prompts/02-development.md"
# tester = ".porpoise/custom-prompts/03-testing.md"
# reviewer = ".porpoise/custom-prompts/04-review.md"

# === 기술 스택 설정 (언어 템플릿 초기화 시 자동 입력) ===
# [tech]
# stack = "Rust (Cargo, Clippy)"
# build_command = "cargo build --release"
# test_command = "cargo test"
# lint_command = "cargo clippy -- -D warnings"

# === 보안 설정 (VerifyCommand 허용 명령 — 기본값 [] = 모든 실행 차단) ===
# [security]
# allowed_command_prefixes = ["cargo", "rustfmt"]
# verify_timeout_secs = 60

# === verify_commands 배열 (test_command/lint_command 대신 사용 시) ===
# [tech]
# verify_commands = [
#   { command = "pytest", args = [], purpose = "단위 테스트", expected_exit_code = 0 },
#   { command = "mypy",   args = ["."], purpose = "타입 검사", expected_exit_code = 0 },
#   { command = "ruff",   args = ["check", "."], purpose = "린트", expected_exit_code = 0 },
# ]

# === OS별 파일 조작 명령 허용 (API 모드에서 파일 편집·이동·삭제 시) ===
#
# Windows (PowerShell 기반):
# [security]
# allowed_command_prefixes = ["powershell", "xcopy", "robocopy"]
#
# macOS / Linux:
# [security]
# allowed_command_prefixes = ["cp", "mv", "rm", "mkdir", "touch", "cat", "grep", "sed", "find", "chmod", "diff", "tar"]

# === 세션 JSON 자동 정리 정책 ===
# [sessions]
# keep_completed_milestone_sessions = false  # true: 완료된 마일스톤 세션 파일 보존
# max_session_age_days = 30                  # 0 = 무제한 보존

# === 지휘자(conductor) 루프 설정 (claude_code 어댑터 전용) ===
# task 하나를 실제 코딩 에이전트에게 통째로 위임하고 독립 검증자가 PASS/FAIL을 판정합니다.
# v0.22.0부터 claude_code 어댑터에서 기본 활성화됩니다. 기존 4단계 방식을 쓰려면 mode = "legacy".
# [conductor]
# mode = "conductor"          # "conductor" (기본) | "legacy" (기존 4단계 phase 방식)
# verifier_model = ""         # 검증자 전용 모델 (생략 시 Dispatch와 동일). 독립성을 위해 다른 모델 권장
# max_redispatch = 2          # Verify FAIL 시 재투입 최대 횟수
# verdict_fallback = "pass_if_checks_pass"  # 검증자 verdict 파싱 실패 시: 검증 명령 통과면 PASS | "halt"(보수)
# max_parallel = 1            # 동시 처리 task 수 (1=순차, >1=병렬 함대 — 독립 task 전제, [1,8] 클램프)
"#
    }

    /// conductor 모드 활성화 여부. v0.22.0부터 **기본 활성화** — `[conductor].mode`가
    /// "legacy"가 아니면 true(미설정 포함). M21 라이브 검증으로 승격 기준 충족됨.
    /// 실제 진입은 claude_code 어댑터일 때만 (API 어댑터는 항상 legacy).
    pub fn conductor_enabled(&self) -> bool {
        let mode = self
            .conductor
            .as_ref()
            .and_then(|c| c.mode.as_deref())
            .unwrap_or("conductor");
        !mode.eq_ignore_ascii_case("legacy")
    }

    /// `[conductor].mode`가 명시되지 않았는지 여부. true면 기본값(conductor)으로 동작 중 —
    /// 기존 사용자에게 1회 전환 안내를 띄울지 판단하는 데 쓴다.
    pub fn conductor_mode_unset(&self) -> bool {
        self.conductor
            .as_ref()
            .and_then(|c| c.mode.as_deref())
            .is_none()
    }

    /// 동시 처리 task 수 (기본 1 = 순차). [1, 8] 범위로 클램프한다 (M23).
    /// >1이면 독립 task를 병렬로 dispatch·verify하고 순차·충돌 인지로 통합한다.
    pub fn conductor_max_parallel(&self) -> u32 {
        self.conductor
            .as_ref()
            .and_then(|c| c.max_parallel)
            .unwrap_or(1)
            .clamp(1, 8)
    }

    /// 폴백 정책이 "halt"인지 여부. true면 검증자 verdict 파싱 실패 시 객관 증거 PASS 대신
    /// 사용자 검토를 위해 FAIL 처리한다 (기본: false = pass_if_checks_pass).
    pub fn conductor_verdict_fallback_halt(&self) -> bool {
        self.conductor
            .as_ref()
            .and_then(|c| c.verdict_fallback.as_deref())
            .map(|v| v.eq_ignore_ascii_case("halt"))
            .unwrap_or(false)
    }

    /// Verify FAIL 시 재투입 최대 횟수 (기본값: 2).
    pub fn conductor_max_redispatch(&self) -> u32 {
        self.conductor
            .as_ref()
            .and_then(|c| c.max_redispatch)
            .unwrap_or(2)
    }

    /// 검증자 전용 모델 ID. 생략 시 None (Dispatch와 동일 모델 사용).
    pub fn conductor_verifier_model(&self) -> Option<&str> {
        self.conductor
            .as_ref()
            .and_then(|c| c.verifier_model.as_deref())
            .filter(|s| !s.is_empty())
    }

    /// 누적 비용(USD) 상한 (M28). 미설정·0 이하이면 None(무제한).
    pub fn conductor_budget_usd(&self) -> Option<f64> {
        self.conductor
            .as_ref()
            .and_then(|c| c.budget_usd)
            .filter(|b| *b > 0.0)
    }

    pub fn session_keep_completed(&self) -> bool {
        self.sessions
            .as_ref()
            .and_then(|s| s.keep_completed_milestone_sessions)
            .unwrap_or(false)
    }

    pub fn session_max_age_days(&self) -> u32 {
        self.sessions
            .as_ref()
            .and_then(|s| s.max_session_age_days)
            .unwrap_or(30)
    }
}

/// && 기준 자동 분리: "ruff check . && mypy ." → 2개의 VerifyCommand.
/// | ; ` $ 등 나머지 메타문자는 개별 경고 후 스킵한다.
fn parse_command_string_multi(s: &str, purpose: &str) -> Vec<VerifyCommand> {
    const REMAINING_METACHARS: &[char] = &['|', ';', '`', '$'];
    let parts: Vec<&str> = s.split("&&").map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.len() > 1 {
        eprintln!("  정보: '{}' 를 {} 개의 명령으로 분리합니다.", s, parts.len());
    }
    parts.iter().enumerate().filter_map(|(i, part)| {
        if part.chars().any(|c| REMAINING_METACHARS.contains(&c)) {
            eprintln!(
                "경고: parse_command_string: '{}' 에 지원되지 않는 shell 메타문자가 포함되어 있어 건너뜁니다. \
                 workspace.toml에서 verify_commands의 command와 args를 분리하여 지정하세요.",
                part
            );
            return None;
        }
        let mut tokens = part.split_whitespace();
        let command = tokens.next()?.to_string();
        let args: Vec<String> = tokens.map(str::to_string).collect();
        let purpose_i = if i == 0 { purpose.to_string() } else { format!("{} ({})", purpose, i + 1) };
        Some(VerifyCommand { command, args, purpose: purpose_i, expected_exit_code: 0 })
    }).collect()
}

fn resolve_path(override_path: &str, project_path: &Path) -> std::path::PathBuf {
    if Path::new(override_path).is_absolute() {
        std::path::PathBuf::from(override_path)
    } else {
        project_path.join(override_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_returns_default_when_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = WorkspaceConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.dod_items().len(), 3);
        assert!(cfg.convention_lines().iter().any(|l| l.contains("한국어")));
    }

    #[test]
    fn load_parses_dod_and_roles() {
        let tmp = tempfile::tempdir().unwrap();
        let porpoise_dir = tmp.path().join(".porpoise");
        std::fs::create_dir_all(&porpoise_dir).unwrap();
        std::fs::write(
            porpoise_dir.join("workspace.toml"),
            "[dod]\nitems = [\"테스트 통과\"]\n\n[roles]\ntester_extra = \"성능 테스트 필수\"\n",
        )
        .unwrap();
        let cfg = WorkspaceConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.dod_items(), vec!["테스트 통과"]);
        let extra = cfg.role_extra_formatted("tester");
        assert!(extra.contains("성능 테스트 필수"));
        assert!(extra.contains("## 추가 지시사항"));
    }

    #[test]
    fn convention_lines_includes_defaults_and_custom() {
        let cfg = WorkspaceConfig {
            conventions: Some(WorkspaceConventions {
                custom_rules: Some(vec!["unwrap() 금지".to_string()]),
            }),
            ..Default::default()
        };
        let lines = cfg.convention_lines();
        assert!(lines.iter().any(|l| l.contains("한국어")));
        assert!(lines.iter().any(|l| l.contains("unwrap()")));
    }

    #[test]
    fn role_extra_empty_when_not_configured() {
        let cfg = WorkspaceConfig::default();
        assert_eq!(cfg.role_extra_formatted("pm"), "");
        assert_eq!(cfg.role_extra_formatted("developer"), "");
        assert_eq!(cfg.role_extra_formatted("tester"), "");
        assert_eq!(cfg.role_extra_formatted("reviewer"), "");
    }

    #[test]
    fn role_extra_all_roles() {
        let cfg = WorkspaceConfig {
            roles: Some(WorkspaceRoles {
                pm_extra: Some("PM 추가".to_string()),
                developer_extra: Some("Dev 추가".to_string()),
                tester_extra: Some("Test 추가".to_string()),
                reviewer_extra: Some("Review 추가".to_string()),
            }),
            ..Default::default()
        };
        assert!(cfg.role_extra_formatted("pm").contains("PM 추가"));
        assert!(cfg.role_extra_formatted("developer").contains("Dev 추가"));
        assert!(cfg.role_extra_formatted("tester").contains("Test 추가"));
        assert!(cfg.role_extra_formatted("reviewer").contains("Review 추가"));
    }

    #[test]
    fn default_toml_parses_without_error() {
        let cfg: WorkspaceConfig = toml::from_str(WorkspaceConfig::default_toml()).unwrap();
        assert_eq!(cfg.dod_items().len(), 3);
        assert_eq!(cfg.role_extra_formatted("pm"), "");
    }

    #[test]
    fn prompt_override_returns_none_when_not_configured() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = WorkspaceConfig::default();
        assert!(cfg.prompt_override_content("pm", tmp.path()).is_none());
    }

    #[test]
    fn language_defaults_to_ko() {
        let cfg = WorkspaceConfig::default();
        assert_eq!(cfg.language(), "ko");
    }

    #[test]
    fn language_reads_from_general() {
        let cfg = WorkspaceConfig {
            general: Some(WorkspaceGeneral { language: Some("en".to_string()) }),
            ..Default::default()
        };
        assert_eq!(cfg.language(), "en");
    }

    #[test]
    fn custom_rules_empty_array_parses() {
        let toml = "[conventions]\ncustom_rules = []\n";
        let cfg: WorkspaceConfig = toml::from_str(toml).unwrap();
        let lines = cfg.convention_lines();
        assert!(lines.iter().any(|l| l.contains("한국어")));
        assert_eq!(lines.len(), 2); // only defaults
    }

    #[test]
    fn prompt_override_reads_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("my-pm.md"), "custom pm prompt").unwrap();
        let cfg = WorkspaceConfig {
            prompt_overrides: Some(WorkspacePromptOverrides {
                pm: Some("my-pm.md".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let content = cfg.prompt_override_content("pm", tmp.path());
        assert_eq!(content, Some("custom pm prompt".to_string()));
    }

    #[test]
    fn tech_and_security_sections_parse() {
        let toml = r#"
[tech]
stack = "Rust"
build_command = "cargo build"
test_command = "cargo test"
lint_command = "cargo clippy"

[security]
allowed_command_prefixes = ["cargo"]
verify_timeout_secs = 30
"#;
        let cfg: WorkspaceConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.tech.as_ref().unwrap().stack.as_deref(), Some("Rust"));
        assert_eq!(cfg.tech.as_ref().unwrap().test_command.as_deref(), Some("cargo test"));
        assert_eq!(cfg.allowed_command_prefixes(), vec!["cargo".to_string()]);
        assert_eq!(cfg.verify_timeout_secs(), 30);
    }

    #[test]
    fn allowed_command_prefixes_defaults_empty() {
        let cfg = WorkspaceConfig::default();
        assert!(cfg.allowed_command_prefixes().is_empty());
    }

    #[test]
    fn default_verify_commands_from_tech() {
        let cfg = WorkspaceConfig {
            tech: Some(WorkspaceTech {
                stack: None,
                build_command: None,
                test_command: Some("cargo test".to_string()),
                lint_command: Some("cargo clippy -- -D warnings".to_string()),
                verify_commands: None,
            }),
            ..Default::default()
        };
        let cmds = cfg.default_verify_commands();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].command, "cargo");
        assert_eq!(cmds[0].args, vec!["test"]);
        assert_eq!(cmds[1].command, "cargo");
        assert_eq!(cmds[1].args, vec!["clippy", "--", "-D", "warnings"]);
    }

    #[test]
    fn snapshot_token_budget_default() {
        let cfg = WorkspaceConfig::default();
        assert_eq!(cfg.snapshot_token_budget(), 32_000);
    }

    #[test]
    fn tech_context_with_stack() {
        let cfg = WorkspaceConfig {
            tech: Some(WorkspaceTech {
                stack: Some("Rust".to_string()),
                test_command: Some("cargo test".to_string()),
                build_command: None,
                lint_command: None,
                verify_commands: None,
            }),
            ..Default::default()
        };
        let ctx = cfg.tech_context().unwrap();
        assert!(ctx.contains("Rust"));
        assert!(ctx.contains("cargo test"));
    }

    #[test]
    fn openai_adapter_type_parses() {
        let toml = "[model]\nadapter = \"openai_compatible\"\n";
        let cfg: WorkspaceConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.model_adapter_type(), crate::model::adapter::AdapterType::OpenAiCompatible);
    }

    #[test]
    fn parse_command_string_accepts_simple() {
        let cmds = parse_command_string_multi("cargo test", "테스트");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command, "cargo");
        assert_eq!(cmds[0].args, vec!["test"]);
        assert_eq!(cmds[0].purpose, "테스트");

        let cmds2 = parse_command_string_multi("ruff check .", "린트");
        assert_eq!(cmds2.len(), 1);
        assert_eq!(cmds2[0].command, "ruff");
        assert_eq!(cmds2[0].args, vec!["check", "."]);
    }

    #[test]
    fn parse_command_string_rejects_remaining_metachars() {
        // && 포함 → 분리 성공 (multi)
        let cmds = parse_command_string_multi("ruff check . && mypy .", "린트");
        assert_eq!(cmds.len(), 2);
        // || 포함 → | 메타문자로 거부
        assert!(parse_command_string_multi("cargo test || true", "테스트").is_empty());
        // ; 포함 → 거부
        assert!(parse_command_string_multi("echo a; echo b", "출력").is_empty());
        // | 포함 → 거부
        assert!(parse_command_string_multi("cat file | grep x", "검색").is_empty());
        // $ 포함 → 거부
        assert!(parse_command_string_multi("echo $HOME", "출력").is_empty());
    }

    #[test]
    fn parse_command_string_compound_split_in_default_verify() {
        let cfg = WorkspaceConfig {
            tech: Some(WorkspaceTech {
                stack: None,
                build_command: None,
                test_command: Some("cargo test && echo done".to_string()),
                lint_command: Some("cargo clippy".to_string()),
                verify_commands: None,
            }),
            ..Default::default()
        };
        // && 복합 명령은 분리 — test 2개 + lint 1개 = 3개
        let cmds = cfg.default_verify_commands();
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].command, "cargo");
        assert_eq!(cmds[0].args, vec!["test"]);
        assert_eq!(cmds[1].command, "echo");
        assert_eq!(cmds[1].args, vec!["done"]);
        assert_eq!(cmds[2].command, "cargo");
        assert_eq!(cmds[2].args, vec!["clippy"]);
    }

    #[test]
    fn parse_command_string_multi_splits_and_and() {
        // && 분리 — 2개 생성
        let cmds = parse_command_string_multi("ruff check . && mypy .", "린트");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].command, "ruff");
        assert_eq!(cmds[0].args, vec!["check", "."]);
        assert_eq!(cmds[0].purpose, "린트");
        assert_eq!(cmds[1].command, "mypy");
        assert_eq!(cmds[1].args, vec!["."]);
        assert_eq!(cmds[1].purpose, "린트 (2)");

        // 단순 명령 — 1개
        let cmds2 = parse_command_string_multi("cargo test", "테스트");
        assert_eq!(cmds2.len(), 1);
        assert_eq!(cmds2[0].command, "cargo");

        // | 포함 — 경고 후 빈 목록
        let cmds3 = parse_command_string_multi("cat file | grep x", "검색");
        assert!(cmds3.is_empty());

        // && 분리 후 | 포함 부분만 스킵
        let cmds4 = parse_command_string_multi("ruff check . && cat f | grep x", "혼용");
        assert_eq!(cmds4.len(), 1);
        assert_eq!(cmds4[0].command, "ruff");
    }

    #[test]
    fn verify_commands_array_takes_priority() {
        use crate::session::v0_7::VerifyCommand;
        let cfg = WorkspaceConfig {
            tech: Some(WorkspaceTech {
                test_command: Some("cargo test".to_string()),
                lint_command: Some("cargo clippy".to_string()),
                verify_commands: Some(vec![
                    VerifyCommand { command: "pytest".to_string(), args: vec![], purpose: "테스트".to_string(), expected_exit_code: 0 },
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cmds = cfg.default_verify_commands();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command, "pytest");
    }

    #[test]
    fn empty_verify_commands_falls_back_to_single_commands() {
        let cfg = WorkspaceConfig {
            tech: Some(WorkspaceTech {
                test_command: Some("cargo test".to_string()),
                verify_commands: Some(vec![]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cmds = cfg.default_verify_commands();
        assert_eq!(cmds[0].command, "cargo");
    }

    #[test]
    fn conductor_enabled_by_default() {
        // v0.22.0: 기본 활성화 (미설정 = conductor)
        let cfg = WorkspaceConfig::default();
        assert!(cfg.conductor_enabled());
        assert!(cfg.conductor_mode_unset());
        assert_eq!(cfg.conductor_max_redispatch(), 2);
        assert!(cfg.conductor_verifier_model().is_none());
        assert!(!cfg.conductor_verdict_fallback_halt());
    }

    #[test]
    fn conductor_mode_unset_false_when_set() {
        let cfg: WorkspaceConfig = toml::from_str("[conductor]\nmode = \"conductor\"\n").unwrap();
        assert!(!cfg.conductor_mode_unset());
    }

    #[test]
    fn conductor_verdict_fallback_halt_parses() {
        let cfg: WorkspaceConfig = toml::from_str("[conductor]\nverdict_fallback = \"halt\"\n").unwrap();
        assert!(cfg.conductor_verdict_fallback_halt());
        let cfg2: WorkspaceConfig = toml::from_str("[conductor]\nverdict_fallback = \"pass_if_checks_pass\"\n").unwrap();
        assert!(!cfg2.conductor_verdict_fallback_halt());
    }

    #[test]
    fn conductor_max_parallel_default_and_clamp() {
        // 기본 1 (순차)
        assert_eq!(WorkspaceConfig::default().conductor_max_parallel(), 1);
        // 설정값 사용
        let cfg: WorkspaceConfig = toml::from_str("[conductor]\nmax_parallel = 4\n").unwrap();
        assert_eq!(cfg.conductor_max_parallel(), 4);
        // 상한 8로 클램프
        let hi: WorkspaceConfig = toml::from_str("[conductor]\nmax_parallel = 99\n").unwrap();
        assert_eq!(hi.conductor_max_parallel(), 8);
        // 하한 1로 클램프 (0 → 1)
        let lo: WorkspaceConfig = toml::from_str("[conductor]\nmax_parallel = 0\n").unwrap();
        assert_eq!(lo.conductor_max_parallel(), 1);
    }

    #[test]
    fn conductor_legacy_mode_disables() {
        let toml = "[conductor]\nmode = \"legacy\"\n";
        let cfg: WorkspaceConfig = toml::from_str(toml).unwrap();
        assert!(!cfg.conductor_enabled());
    }

    #[test]
    fn conductor_mode_case_insensitive() {
        // "CONDUCTOR" (대문자)도 활성, "LEGACY"는 비활성
        let cfg_on: WorkspaceConfig = toml::from_str("[conductor]\nmode = \"CONDUCTOR\"\n").unwrap();
        assert!(cfg_on.conductor_enabled());
        let cfg_off: WorkspaceConfig = toml::from_str("[conductor]\nmode = \"LEGACY\"\n").unwrap();
        assert!(!cfg_off.conductor_enabled());
    }

    #[test]
    fn conductor_explicit_conductor_mode_enabled() {
        let toml = "[conductor]\nmode = \"conductor\"\nmax_redispatch = 5\nverifier_model = \"claude-opus-4-8\"\n";
        let cfg: WorkspaceConfig = toml::from_str(toml).unwrap();
        assert!(cfg.conductor_enabled());
        assert_eq!(cfg.conductor_max_redispatch(), 5);
        assert_eq!(cfg.conductor_verifier_model(), Some("claude-opus-4-8"));
    }

    #[test]
    fn conductor_empty_verifier_model_is_none() {
        let toml = "[conductor]\nverifier_model = \"\"\n";
        let cfg: WorkspaceConfig = toml::from_str(toml).unwrap();
        assert!(cfg.conductor_verifier_model().is_none());
    }

    #[test]
    fn default_toml_includes_conductor_docs() {
        let cfg: WorkspaceConfig = toml::from_str(WorkspaceConfig::default_toml()).unwrap();
        // 주석 처리된 [conductor]이므로 파싱 후 기본값(conductor = 활성, v0.22.0~)이어야 함
        assert!(cfg.conductor_enabled());
        assert!(WorkspaceConfig::default_toml().contains("[conductor]"));
    }

    #[test]
    fn verify_commands_array_parses_from_toml() {
        let toml_str = r#"
[tech]
verify_commands = [
  { command = "pytest", args = [], purpose = "테스트", expected_exit_code = 0 },
]
"#;
        let cfg: WorkspaceConfig = toml::from_str(toml_str).unwrap();
        let cmds = cfg.default_verify_commands();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command, "pytest");
    }
}
