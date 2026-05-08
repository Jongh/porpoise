use anyhow::{Context, Result};
use chrono::Local;
use colored::Colorize;
use std::path::Path;

use super::context::ProjectContext;
use super::lang_template::LangTemplate;
use super::model_template::ModelTemplate;
use super::template::apply_template;
use crate::config::workspace::WorkspaceConfig;
use crate::utils::fs::write_file;

const CLAUDE_MD_TEMPLATE: &str = include_str!("prompts/claude.tmpl");
const PROJECT_MD_TEMPLATE: &str = include_str!("prompts/project.tmpl");
const ORCHE_TEMPLATE: &str = include_str!("prompts/00-orche.tmpl");
const PM_TEMPLATE: &str = include_str!("prompts/01-planning.tmpl");
const DEVELOPER_TEMPLATE: &str = include_str!("prompts/02-development.tmpl");
const TESTER_TEMPLATE: &str = include_str!("prompts/03-testing.tmpl");
const REVIEWER_TEMPLATE: &str = include_str!("prompts/04-review.tmpl");
// {{next_milestone_id}} in this template is a runtime variable — not substituted at init time.
const MILESTONE_TEMPLATE: &str = include_str!("prompts/05-milestone.tmpl");

pub fn generate_docs(
    ctx: &ProjectContext,
    path: &Path,
    workspace: &WorkspaceConfig,
    lang_template: Option<&'static LangTemplate>,
    model_template: Option<&'static ModelTemplate>,
) -> Result<()> {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // CLAUDE.md — minimal reference pointer
    let claude_md_path = path.join("CLAUDE.md");
    let claude_content = apply_template(CLAUDE_MD_TEMPLATE, &[("project_name", &ctx.project_name)]);
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
        let toml_content = match (lang_template, model_template) {
            (None, None) => WorkspaceConfig::default_toml().to_string(),
            _ => workspace_toml_from_template(lang_template, model_template),
        };
        write_file(&workspace_toml_path, &toml_content, path)?;
        let template_names = [
            lang_template.map(|t| t.display_name),
            model_template.map(|t| t.display_name),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        if template_names.is_empty() {
            println!("  {} {}", "Created:".green(), workspace_toml_path.display());
        } else {
            println!(
                "  {} {} ({})",
                "Created:".green(),
                workspace_toml_path.display(),
                template_names
            );
        }
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
                println!("  {} {} (오버라이드)", "→".yellow(), filename);
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

    // 05-milestone.md — {{next_milestone_id}} is a runtime variable substituted by milestone_session.
    // Write the template as-is so the placeholder survives until runtime.
    let milestone_path = prompts_dir.join("05-milestone.md");
    write_file(&milestone_path, MILESTONE_TEMPLATE, path)?;
    println!("  {} {}", "Created:".green(), milestone_path.display());

    Ok(())
}

fn workspace_toml_from_template(
    lang_template: Option<&LangTemplate>,
    model_template: Option<&ModelTemplate>,
) -> String {
    let title = [
        lang_template.map(|t| t.display_name),
        model_template.map(|t| t.display_name),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" + ");

    let (dod_items, conventions, tech_section, security_section) = if let Some(t) = lang_template {
        let dod_items: String = t
            .dod_items
            .iter()
            .map(|d| format!("    {},\n", toml_string(d)))
            .collect();
        let conventions: String = t
            .conventions
            .iter()
            .map(|c| format!("    {},\n", toml_string(c)))
            .collect();
        let prefixes = t
            .default_allowed_command_prefixes
            .iter()
            .map(|p| toml_string(p))
            .collect::<Vec<_>>()
            .join(", ");
        let tech_section = format!(
            r#"[tech]
stack = {stack}
build_command = {build}
test_command = {test}
lint_command = {lint}
"#,
            stack = toml_string(t.tech_stack),
            build = toml_string(t.build_command),
            test = toml_string(t.test_command),
            lint = toml_string(t.lint_command),
        );
        let security_section = format!(
            r#"[security]
allowed_command_prefixes = [{prefixes}]
verify_timeout_secs = 60
"#
        );
        (dod_items, conventions, tech_section, security_section)
    } else {
        let dod_items = ["코드 리뷰 통과", "테스트 통과", "문서화 완료"]
            .iter()
            .map(|d| format!("    {},\n", toml_string(d)))
            .collect();
        (dod_items, String::new(), String::new(), String::new())
    };

    let model_section = model_template.map(render_model_section).unwrap_or_default();

    let mut content = format!(
        r#"# Porpoise 작업 환경 설정 — {title} 템플릿

[general]
language = "ko"

[dod]
items = [
{dod_items}]

[conventions]
custom_rules = [
{conventions}]

[roles]
pm_extra = ""
developer_extra = ""
tester_extra = ""
reviewer_extra = ""
"#,
        title = title,
        dod_items = dod_items,
        conventions = conventions,
    );

    if !model_section.is_empty() {
        content.push('\n');
        content.push_str(&model_section);
    }
    if !tech_section.is_empty() {
        content.push('\n');
        content.push_str(&tech_section);
    }
    if !security_section.is_empty() {
        content.push('\n');
        content.push_str(&security_section);
    }
    content
}

fn render_model_section(t: &ModelTemplate) -> String {
    let mut lines = vec![
        "[model]".to_string(),
        format!("adapter = {}", toml_string(t.adapter)),
        format!("model_id = {}", toml_string(t.model_id)),
    ];

    if let Some(api_base_url) = t.api_base_url {
        lines.push(format!("api_base_url = {}", toml_string(api_base_url)));
    }
    if let Some(api_key_env) = t.api_key_env {
        lines.push(format!("api_key_env = {}", toml_string(api_key_env)));
    }
    if let Some(mode) = t.structured_output_mode {
        lines.push(format!("structured_output_mode = {}", toml_string(mode)));
    }
    if let Some(budget) = t.snapshot_token_budget {
        lines.push(format!("snapshot_token_budget = {}", budget));
    }

    let mut section = lines.join("\n");
    section.push('\n');
    section
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
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

        generate_docs(&ctx, path, &workspace, None, None).unwrap();

        assert!(path.join("CLAUDE.md").exists());
        assert!(path.join(".porpoise").join("project.md").exists());
        assert!(path.join(".porpoise").join("workspace.toml").exists());
        assert!(path
            .join(".porpoise")
            .join("prompts")
            .join("00-orche.md")
            .exists());
        assert!(path
            .join(".porpoise")
            .join("prompts")
            .join("01-planning.md")
            .exists());
        assert!(path
            .join(".porpoise")
            .join("prompts")
            .join("02-development.md")
            .exists());
        assert!(path
            .join(".porpoise")
            .join("prompts")
            .join("03-testing.md")
            .exists());
        assert!(path
            .join(".porpoise")
            .join("prompts")
            .join("04-review.md")
            .exists());
        assert!(path
            .join(".porpoise")
            .join("prompts")
            .join("05-milestone.md")
            .exists());
        assert!(path.join(".porpoise").join("hints").exists());
        assert!(path.join(".porpoise").join("reports").exists());
    }

    #[test]
    fn generate_docs_substitutes_project_name() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(&ctx, path, &workspace, None, None).unwrap();

        let claude_md = std::fs::read_to_string(path.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("test-project"));
        assert!(!claude_md.contains("{{project_name}}"));

        let orche =
            std::fs::read_to_string(path.join(".porpoise").join("prompts").join("00-orche.md"))
                .unwrap();
        assert!(orche.contains("test-project"));
        assert!(!orche.contains("{{project_name}}"));

        let project_md =
            std::fs::read_to_string(path.join(".porpoise").join("project.md")).unwrap();
        assert!(
            !project_md.contains("{{language}}"),
            "project.md에 {{language}} 미치환 변수가 남아있음"
        );
        assert!(
            project_md.contains("ko"),
            "project.md에 기본 언어값 'ko'가 없음"
        );
    }

    #[test]
    fn generate_docs_preserves_workspace_toml_on_rerun() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(&ctx, path, &workspace, None, None).unwrap();

        // Modify workspace.toml
        let ws_path = path.join(".porpoise").join("workspace.toml");
        std::fs::write(&ws_path, "# custom content").unwrap();

        // Re-run init
        generate_docs(&ctx, path, &workspace, None, None).unwrap();

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

        generate_docs(&ctx, path, &workspace, None, None).unwrap();

        let project_md =
            std::fs::read_to_string(path.join(".porpoise").join("project.md")).unwrap();
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

        generate_docs(&ctx, path, &workspace, None, None).unwrap();

        let tester_prompt =
            std::fs::read_to_string(path.join(".porpoise").join("prompts").join("03-testing.md"))
                .unwrap();
        assert!(tester_prompt.contains("부하 테스트 필수"));
    }

    #[test]
    fn generate_docs_no_unresolved_placeholders_in_role_prompts() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(&ctx, path, &workspace, None, None).unwrap();

        for filename in &[
            "01-planning.md",
            "02-development.md",
            "03-testing.md",
            "04-review.md",
        ] {
            let content =
                std::fs::read_to_string(path.join(".porpoise").join("prompts").join(filename))
                    .unwrap();
            assert!(
                !content.contains("{{role_extra}}"),
                "{} still contains {{{{role_extra}}}}",
                filename
            );
        }
    }

    #[test]
    fn generate_docs_milestone_prompt_has_runtime_placeholder() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(&ctx, path, &workspace, None, None).unwrap();

        let content = std::fs::read_to_string(
            path.join(".porpoise")
                .join("prompts")
                .join("05-milestone.md"),
        )
        .unwrap();
        // {{next_milestone_id}} is intentionally left for runtime substitution
        assert!(
            content.contains("{{next_milestone_id}}"),
            "05-milestone.md should retain the runtime placeholder {{next_milestone_id}}"
        );
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

    #[test]
    fn from_template_populates_all_sections() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(
            &ctx,
            path,
            &workspace,
            Some(&crate::init::lang_template::RUST),
            None,
        )
        .unwrap();

        let ws_content =
            std::fs::read_to_string(path.join(".porpoise").join("workspace.toml")).unwrap();
        assert!(ws_content.contains("cargo test"), "test_command 없음");
        assert!(ws_content.contains("cargo clippy"), "lint_command 없음");
        assert!(ws_content.contains("[tech]"), "[tech] 섹션 없음");
        assert!(ws_content.contains("[security]"), "[security] 섹션 없음");
    }

    #[test]
    fn from_model_template_writes_active_claude_model_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(
            &ctx,
            path,
            &workspace,
            None,
            Some(&crate::init::model_template::CLAUDE_CODE),
        )
        .unwrap();

        let ws_content =
            std::fs::read_to_string(path.join(".porpoise").join("workspace.toml")).unwrap();
        assert!(ws_content.contains("\n[model]\n"), "활성 [model] 섹션 없음");
        assert!(ws_content.contains("adapter = \"claude_code\""));
        assert!(ws_content.contains("model_id = \"claude-sonnet-4-6\""));
        assert!(
            !ws_content.contains("# [model]"),
            "주석 처리된 모델 블록이 생성됨"
        );

        let parsed: WorkspaceConfig = toml::from_str(&ws_content).unwrap();
        let model = parsed.model.expect("model config");
        assert_eq!(model.adapter.as_deref(), Some("claude_code"));
        assert_eq!(model.model_id.as_deref(), Some("claude-sonnet-4-6"));
    }

    #[test]
    fn from_model_template_writes_openai_compatible_fields_with_lang_template() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(
            &ctx,
            path,
            &workspace,
            Some(&crate::init::lang_template::RUST),
            Some(&crate::init::model_template::OPENAI_CODEX),
        )
        .unwrap();

        let ws_content =
            std::fs::read_to_string(path.join(".porpoise").join("workspace.toml")).unwrap();
        assert!(ws_content.contains("[model]"), "[model] 섹션 없음");
        assert!(ws_content.contains("adapter = \"openai_compatible\""));
        assert!(ws_content.contains("model_id = \"codex-mini-latest\""));
        assert!(ws_content.contains("api_base_url = \"https://api.openai.com/v1\""));
        assert!(ws_content.contains("api_key_env = \"OPENAI_API_KEY\""));
        assert!(ws_content.contains("structured_output_mode = \"function_calling\""));
        assert!(ws_content.contains("snapshot_token_budget = 80000"));
        assert!(
            ws_content.contains("[tech]"),
            "언어 템플릿 [tech] 섹션 없음"
        );
        assert!(
            ws_content.contains("cargo test"),
            "언어 템플릿 test_command 없음"
        );

        let parsed: WorkspaceConfig = toml::from_str(&ws_content).unwrap();
        let model = parsed.model.expect("model config");
        assert_eq!(model.adapter.as_deref(), Some("openai_compatible"));
        assert_eq!(
            model.api_base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(model.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
        assert_eq!(
            model.structured_output_mode.as_deref(),
            Some("function_calling")
        );
    }

    #[test]
    fn from_template_sets_allowed_prefixes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path();
        let ctx = make_ctx();
        let workspace = WorkspaceConfig::default();

        generate_docs(
            &ctx,
            path,
            &workspace,
            Some(&crate::init::lang_template::RUST),
            None,
        )
        .unwrap();

        let ws_content =
            std::fs::read_to_string(path.join(".porpoise").join("workspace.toml")).unwrap();
        assert!(
            ws_content.contains("\"cargo\""),
            "allowed_prefixes에 cargo 없음"
        );
    }
}
