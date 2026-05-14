use colored::Colorize;
use std::path::Path;

use crate::config::workspace::WorkspaceConfig;

pub struct MissingCommand {
    pub name: String,
    pub install_hint_windows: String,
    pub install_hint_unix: String,
}

pub fn check_and_report(project_path: &Path) -> bool {
    let workspace = match WorkspaceConfig::load(project_path) {
        Ok(ws) => ws,
        Err(_) => return false,
    };

    let missing = collect_missing(&workspace);
    if missing.is_empty() {
        return false;
    }

    eprintln!();
    eprintln!("  {} 필수 명령어가 설치되어 있지 않습니다:", "[Porpoise]".yellow().bold());
    eprintln!();
    for m in &missing {
        let hint = if cfg!(target_os = "windows") {
            &m.install_hint_windows
        } else {
            &m.install_hint_unix
        };
        eprintln!("    {} {}   —   {}", "✗".red(), m.name.yellow(), hint);
    }
    eprintln!();
    eprintln!("  설치 후 다시 실행하세요.");
    eprintln!();

    true
}

fn collect_missing(workspace: &WorkspaceConfig) -> Vec<MissingCommand> {
    let mut missing = Vec::new();
    let mut checked: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut check = |name: &str| {
        if !checked.insert(name.to_string()) {
            return;
        }
        if which::which(name).is_err() {
            if let Some(hint) = install_hint(name) {
                missing.push(MissingCommand {
                    name: name.to_string(),
                    install_hint_windows: hint.0.to_string(),
                    install_hint_unix: hint.1.to_string(),
                });
            }
        }
    };

    use crate::model::adapter::AdapterType;
    if workspace.model_adapter_type() == AdapterType::ClaudeCode {
        check("claude");
    }

    if let Some(tech) = &workspace.tech {
        for cmd_str in [&tech.build_command, &tech.test_command, &tech.lint_command]
            .iter()
            .filter_map(|o| o.as_deref())
        {
            if let Some(binary) = cmd_str.split_whitespace().next() {
                check(binary);
            }
        }

        if let Some(cmds) = &tech.verify_commands {
            for cmd in cmds {
                check(&cmd.command);
            }
        }
    }

    missing
}

fn install_hint(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "claude" => Some((
            "npm install -g @anthropic-ai/claude-code",
            "npm install -g @anthropic-ai/claude-code",
        )),
        "cargo" | "rustup" => Some((
            "winget install Rustlang.Rustup",
            "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
        )),
        "rustfmt" | "clippy" => Some((
            "rustup component add rustfmt clippy",
            "rustup component add rustfmt clippy",
        )),
        "node" | "npm" | "npx" => Some((
            "winget install OpenJS.NodeJS.LTS",
            "brew install node  /  apt install nodejs npm",
        )),
        "python" | "python3" => Some((
            "winget install Python.Python.3",
            "brew install python3  /  apt install python3",
        )),
        "pip" | "pip3" => Some((
            "Python 설치 시 포함 (winget install Python.Python.3)",
            "python3 -m ensurepip --upgrade",
        )),
        "pytest" => Some(("pip install pytest", "pip install pytest")),
        "mypy" => Some(("pip install mypy", "pip install mypy")),
        "ruff" => Some(("pip install ruff", "pip install ruff")),
        "go" => Some((
            "winget install GoLang.Go",
            "brew install go  /  공식 go.dev/dl 참조",
        )),
        "mvn" => Some((
            "winget install Apache.Maven",
            "brew install maven  /  apt install maven",
        )),
        "gradle" => Some((
            "winget install Gradle.Gradle",
            "brew install gradle  /  apt install gradle",
        )),
        "java" | "javac" => Some((
            "winget install Microsoft.OpenJDK.21",
            "brew install java  /  apt install default-jdk",
        )),
        "golangci-lint" => Some((
            "go install github.com/golangci/golangci-lint/cmd/golangci-lint@latest",
            "brew install golangci-lint",
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_hint_known_tools() {
        assert!(install_hint("claude").is_some());
        assert!(install_hint("cargo").is_some());
        assert!(install_hint("npm").is_some());
        assert!(install_hint("pytest").is_some());
    }

    #[test]
    fn install_hint_unknown_returns_none() {
        assert!(install_hint("some_custom_tool_xyz").is_none());
    }
}
