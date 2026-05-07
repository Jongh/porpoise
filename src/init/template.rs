/// Substitutes `{{variable_name}}` placeholders in a template string.
/// Literal `{` and `}` (single brace) are passed through unchanged.
pub fn apply_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }
    // BUG-01: collapse triple+ newlines caused by empty variable substitution
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }
    if let Some(pos) = result.find("{{") {
        let snippet: String = result[pos..].chars().take(40).collect();
        eprintln!("⚠ 미치환 변수 감지: {}...", snippet.trim());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_single_variable() {
        assert_eq!(
            apply_template("Hello, {{name}}!", &[("name", "World")]),
            "Hello, World!"
        );
    }

    #[test]
    fn substitutes_multiple_variables() {
        let result = apply_template(
            "{{greeting}}, {{name}}!",
            &[("greeting", "안녕"), ("name", "Rust")],
        );
        assert_eq!(result, "안녕, Rust!");
    }

    #[test]
    fn leaves_literal_single_braces_unchanged() {
        let result = apply_template(
            "파일명: {task-id}-{role}.md, 프로젝트: {{project_name}}",
            &[("project_name", "porpoise")],
        );
        assert_eq!(result, "파일명: {task-id}-{role}.md, 프로젝트: porpoise");
    }

    #[test]
    fn empty_value_substitution() {
        assert_eq!(
            apply_template("before{{extra}}after", &[("extra", "")]),
            "beforeafter"
        );
    }

    #[test]
    fn substitution_does_not_affect_other_placeholders() {
        let result = apply_template(
            "{{a}} and {{b}}",
            &[("a", "first")],
        );
        assert!(result.starts_with("first and "));
        assert!(result.contains("{{b}}"));
    }

    #[test]
    fn triple_newlines_collapsed_after_empty_substitution() {
        // Empty role_extra between two sections causes triple newline — should collapse to double
        let template = "## Section A\n\n{{role_extra}}\n\n---\n";
        let result = apply_template(template, &[("role_extra", "")]);
        assert!(!result.contains("\n\n\n"), "result had triple newline: {:?}", result);
        assert!(result.contains("## Section A"));
        assert!(result.contains("---"));
    }
}
