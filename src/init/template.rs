/// Substitutes `{{variable_name}}` placeholders in a template string.
/// Literal `{` and `}` (single brace) are passed through unchanged.
pub fn apply_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
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
}
