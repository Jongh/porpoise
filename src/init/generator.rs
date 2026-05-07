use anyhow::{Context, Result};
use chrono::Local;
use colored::Colorize;
use std::path::Path;

use super::context::ProjectContext;
use super::template::apply_template;
use crate::config::workspace::WorkspaceConfig;
use crate::utils::fs::write_file;

const CLAUDE_MD_TEMPLATE: &str = include_str!("prompts/claude.md");
const PROJECT_MD_TEMPLATE: &str = include_str!("prompts/project.md");
const ORCHE_TEMPLATE: &str = include_str!("prompts/00-orche.md");
const PM_TEMPLATE: &str = include_str!("prompts/01-planning.md");
const DEVELOPER_TEMPLATE: &str = include_str!("prompts/02-development.md");
const TESTER_TEMPLATE: &str = include_str!("prompts/03-testing.md");
const REVIEWER_TEMPLATE: &str = include_str!("prompts/04-review.md");

pub fn generate_docs(ctx: &ProjectContext, path: &Path, workspace: &WorkspaceConfig) -> Result<()> {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // CLAUDE.md — minimal reference pointer
    let claude_md_path = path.join("CLAUDE.md");
    let claude_content = apply_template(
        CLAUDE_MD_TEMPLATE,
        &[("project_name", &ctx.project_name)],
    );
    write_file(&claude_md_path, &claude_content, path)?;
    println!("  {} {}", "Created:".green(), claude_md_path.display());

    // .porpoise/project.md — single source of truth
    let docs_dir = path.join(".porpoise");
    let dod_items = format_list_items(&workspace.dod_items());
    let conventions = format_list_items(&workspace.convention_lines());
    let language = workspace.language().to_string();
    let project_content = apply_template(
        PROJECT_MD_TEMPLATE,
        &[
            ("project_name", &ctx.project_name),
            ("timestamp", &timestamp),
            ("tree", &ctx.tree_output),
            ("dod_items", &dod_items),
            ("conventions", &conventions),
            ("language", &language),
        ],
    );
    let project_md_path = docs_dir.join("project.md");
    write_file(&project_md_path, &project_content, path)?;
    println!("  {} {}", "Created:".green(), project_md_path.display());

    // Directories
    for dir_name in &["hints", "reports"] {
        let dir = docs_dir.join(dir_name);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("{} 디렉토리 생성 실패: {}", dir_name, dir.display()))?;
    }

    // .porpoise/workspace.toml — skip if exists (preserve user customizations)
    let workspace_toml_path = docs_dir.join("workspace.toml");
    if !workspace_toml_path.exists() {
        write_file(&workspace_toml_path, WorkspaceConfig::default_toml(), path)?;
        println!("  {} {}", "Created:".green(), workspace_toml_path.display());
    } else {
        println!(
            "  {} {} (기존 설정 유지)",
            "Skipped:".dimmed(),
            workspace_toml_path.display()
        );
    }

    // .porpoise/prompts/ — orchestration prompt
    let prompts_dir = docs_dir.join("prompts");
    let orche_content = apply_template(ORCHE_TEMPLATE, &[("project_name", &ctx.project_name)]);
    let orche_path = prompts_dir.join("00-orche.md");
    write_file(&orche_path, &orche_content, path)?;
    println!("  {} {}", "Created:".green(), orche_path.display());

    // Role prompts — workspace overrides and extras applied
    let role_prompts: &[(&str, &str, &str)] = &[
        ("01-planning.md", PM_TEMPLATE, "pm"),
        ("02-development.md", DEVELOPER_TEMPLATE, "developer"),
        ("03-testing.md", TESTER_TEMPLATE, "tester"),
        ("04-review.md", REVIEWER_TEMPLATE, "reviewer"),
    ];

    for (filename, template, role_key) in role_prompts {
        let content = match workspace.prompt_override_content(role_key, path) {
            Some(override_content) => {
                println!(
                    "  {} {} (오버라이드)",
                    "→".yellow(),
                    filename
                );
                override_content
            }
            None => {
                let extra = workspace.role_extra_formatted(role_key);
                apply_template(template, &[("role_extra", &extra)])
            }
        };
        let prompt_path = prompts_dir.join(filename);
        write_file(&prompt_path, &content, path)?;
        println!("  {} {}", "Created:".green(), prompt_path.display());
    }

    Ok(())
}

fn format_list_items(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- {}", item))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::context::ProjectContext;

    fn make_ctx() -> ProjectContext {
        ProjectContext {
            project_name: "test-project".to_string(),
            tree_output: "src/\n  main.rs".to_string(),
            detected_files: vec![],
        }
    }

    #[test]
    fn generate_docs_creates_expected_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(&ctx, path, &workspace).unwrap();

        assert!(path.join("CLAUDE.md").exists());
        assert!(path.join(".porpoise").join("project.md").exists());
        assert!(path.join(".porpoise").join("workspace.toml").exists());
        assert!(path.join(".porpoise").join("prompts").join("00-orche.md").exists());
        assert!(path.join(".porpoise").join("prompts").join("01-planning.md").exists());
        assert!(path.join(".porpoise").join("prompts").join("02-development.md").exists());
        assert!(path.join(".porpoise").join("prompts").join("03-testing.md").exists());
        assert!(path.join(".porpoise").join("prompts").join("04-review.md").exists());
        assert!(path.join(".porpoise").join("hints").exists());
        assert!(path.join(".porpoise").join("reports").exists());
    }

    #[test]
    fn generate_docs_substitutes_project_name() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(&ctx, path, &workspace).unwrap();

        let claude_md = std::fs::read_to_string(path.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("test-project"));
        assert!(!claude_md.contains("{{project_name}}"));

        let orche = std::fs::read_to_string(
            path.join(".porpoise").join("prompts").join("00-orche.md"),
        ).unwrap();
        assert!(orche.contains("test-project"));
        assert!(!orche.contains("{{project_name}}"));

        let project_md = std::fs::read_to_string(
            path.join(".porpoise").join("project.md"),
        ).unwrap();
        assert!(!project_md.contains("{{language}}"), "project.md에 {{language}} 미치환 변수가 남아있음");
        assert!(project_md.contains("ko"), "project.md에 기본 언어값 'ko'가 없음");
    }

    #[test]
    fn generate_docs_preserves_workspace_toml_on_rerun() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(&ctx, path, &workspace).unwrap();

        // Modify workspace.toml
        let ws_path = path.join(".porpoise").join("workspace.toml");
        std::fs::write(&ws_path, "# custom content").unwrap();

        // Re-run init
        generate_docs(&ctx, path, &workspace).unwrap();

        let content = std::fs::read_to_string(&ws_path).unwrap();
        assert_eq!(content, "# custom content");
    }

    #[test]
    fn generate_docs_workspace_dod_reflected_in_project_md() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig {
            dod: Some(crate::config::workspace::WorkspaceDod {
                items: Some(vec!["성능 테스트 통과".to_string()]),
            }),
            ..Default::default()
        };

        generate_docs(&ctx, path, &workspace).unwrap();

        let project_md = std::fs::read_to_string(
            path.join(".porpoise").join("project.md"),
        ).unwrap();
        assert!(project_md.contains("성능 테스트 통과"));
    }

    #[test]
    fn generate_docs_role_extra_in_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig {
            roles: Some(crate::config::workspace::WorkspaceRoles {
                tester_extra: Some("부하 테스트 필수".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        generate_docs(&ctx, path, &workspace).unwrap();

        let tester_prompt = std::fs::read_to_string(
            path.join(".porpoise").join("prompts").join("03-testing.md"),
        ).unwrap();
        assert!(tester_prompt.contains("부하 테스트 필수"));
    }

    #[test]
    fn generate_docs_no_unresolved_placeholders_in_role_prompts() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(&ctx, path, &workspace).unwrap();

        for filename in &["01-planning.md", "02-development.md", "03-testing.md", "04-review.md"] {
            let content = std::fs::read_to_string(
                path.join(".porpoise").join("prompts").join(filename),
            ).unwrap();
            assert!(
                !content.contains("{{role_extra}}"),
                "{} still contains {{{{role_extra}}}}",
                filename
            );
        }
    }

    #[test]
    fn format_list_items_formats_correctly() {
        let items = vec!["a".to_string(), "b".to_string()];
        assert_eq!(format_list_items(&items), "- a\n- b");
    }

    #[test]
    fn format_list_items_empty() {
        assert_eq!(format_list_items(&[]), "");
    }
}
