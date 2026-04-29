use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub github_repo: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportsConfig {
    pub archive_after_days: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub project: Option<ProjectConfig>,
    pub claude: Option<ClaudeConfig>,
    pub reports: Option<ReportsConfig>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let config_path = path.join("porpoise.toml");
        if !config_path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn github_repo(&self) -> Option<&str> {
        self.project.as_ref()?.github_repo.as_deref()
    }

    pub fn model(&self) -> Option<&str> {
        self.claude.as_ref()?.model.as_deref()
    }

    pub fn archive_after_days(&self) -> u32 {
        self.reports
            .as_ref()
            .and_then(|r| r.archive_after_days)
            .unwrap_or(30)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_values() {
        let cfg = Config::default();
        assert!(cfg.github_repo().is_none());
        assert!(cfg.model().is_none());
        assert_eq!(cfg.archive_after_days(), 30);
    }

    #[test]
    fn load_returns_default_when_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = Config::load(tmp.path()).unwrap();
        assert!(cfg.github_repo().is_none());
        assert_eq!(cfg.archive_after_days(), 30);
    }

    #[test]
    fn load_parses_toml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("porpoise.toml"),
            "[project]\ngithub_repo = \"owner/repo\"\n\n[claude]\nmodel = \"claude-sonnet-4-6\"\n\n[reports]\narchive_after_days = 14\n",
        )
        .unwrap();
        let cfg = Config::load(tmp.path()).unwrap();
        assert_eq!(cfg.github_repo(), Some("owner/repo"));
        assert_eq!(cfg.model(), Some("claude-sonnet-4-6"));
        assert_eq!(cfg.archive_after_days(), 14);
    }
}
