use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
pub struct WorkspaceConfig {
    pub general: Option<WorkspaceGeneral>,
    pub dod: Option<WorkspaceDod>,
    pub conventions: Option<WorkspaceConventions>,
    pub roles: Option<WorkspaceRoles>,
    pub prompt_overrides: Option<WorkspacePromptOverrides>,
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

    /// Reads prompt override file for the given role key, if configured in [prompt_overrides].
    /// Returns None if not configured or file cannot be read (falls back to default template).
    pub fn prompt_override_content(&self, role_key: &str, project_path: &Path) -> Option<String> {
        let override_path_str = self.prompt_overrides.as_ref().and_then(|po| {
            match role_key {
                "pm" => po.pm.as_deref(),
                "developer" => po.developer.as_deref(),
                "tester" => po.tester.as_deref(),
                "reviewer" => po.reviewer.as_deref(),
                _ => None,
            }
        })?;

        let full_path = if Path::new(override_path_str).is_absolute() {
            std::path::PathBuf::from(override_path_str)
        } else {
            project_path.join(override_path_str)
        };

        match std::fs::read_to_string(&full_path) {
            Ok(content) => Some(content),
            Err(_) => {
                eprintln!(
                    "⚠ 프롬프트 오버라이드 파일을 읽을 수 없습니다: {} (기본 템플릿 사용)",
                    override_path_str
                );
                None
            }
        }
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
# 각 역할 프롬프트에 추가될 지시사항 (비어 있으면 생략됩니다)
pm_extra = ""
developer_extra = ""
tester_extra = ""
reviewer_extra = ""

[prompt_overrides]
# 역할 프롬프트를 완전히 교체할 파일 경로 (주석 처리하면 기본 템플릿 사용)
# pm = ".porpoise/custom-prompts/01-planning.md"
# developer = ".porpoise/custom-prompts/02-development.md"
# tester = ".porpoise/custom-prompts/03-testing.md"
# reviewer = ".porpoise/custom-prompts/04-review.md"
"#
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
}
