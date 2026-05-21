pub struct LangTemplate {
    #[allow(dead_code)]
    pub id: &'static str,
    pub display_name: &'static str,
    pub tech_stack: &'static str,
    pub dod_items: &'static [&'static str],
    pub conventions: &'static [&'static str],
    pub build_command: &'static str,
    pub test_command: &'static str,
    pub lint_command: &'static str,
    pub default_allowed_command_prefixes: &'static [&'static str],
    pub allowed_file_commands_windows: &'static [&'static str],
    pub allowed_file_commands_unix: &'static [&'static str],
}

pub static RUST: LangTemplate = LangTemplate {
    id: "rust",
    display_name: "Rust (Cargo)",
    tech_stack: "Rust (Cargo, Clippy, Rustfmt)",
    dod_items: &[
        "cargo test 통과",
        "cargo clippy -- -D warnings 통과",
        "cargo fmt 적용",
    ],
    conventions: &[
        "pub API에 doc comment 필수",
        "unwrap()/expect() 시스템 경계 이외 금지",
        "오류 처리: anyhow 또는 thiserror 사용",
        "unsafe 블록 사용 금지 (예외 시 주석 필수)",
    ],
    build_command: "cargo build --release",
    test_command: "cargo test",
    lint_command: "cargo clippy -- -D warnings",
    default_allowed_command_prefixes: &["cargo", "rustfmt"],
    allowed_file_commands_windows: &["powershell", "xcopy", "robocopy"],
    allowed_file_commands_unix: &["cp", "mv", "rm", "mkdir", "touch", "cat", "grep", "sed", "find", "chmod", "diff", "tar"],
};

pub static PYTHON: LangTemplate = LangTemplate {
    id: "python",
    display_name: "Python 3.11+",
    tech_stack: "Python 3.11+ (pytest, mypy, ruff)",
    dod_items: &[
        "pytest 통과",
        "mypy --strict 통과",
        "ruff check 통과",
        "coverage 80% 이상",
    ],
    conventions: &[
        "타입 힌트 필수 (PEP 484)",
        "docstring 필수 (Google style)",
        "f-string 사용",
        "가변 전역 상태 금지",
    ],
    build_command: "pip install -e .",
    test_command: "pytest",
    lint_command: "ruff check .",
    default_allowed_command_prefixes: &["pytest", "python", "mypy", "ruff", "pip"],
    allowed_file_commands_windows: &["powershell", "xcopy", "robocopy"],
    allowed_file_commands_unix: &["cp", "mv", "rm", "mkdir", "touch", "cat", "grep", "sed", "find", "chmod", "diff", "tar"],
};

pub static TYPESCRIPT: LangTemplate = LangTemplate {
    id: "typescript",
    display_name: "TypeScript (Node.js)",
    tech_stack: "TypeScript (strict, ESLint, Prettier)",
    dod_items: &[
        "npm test 통과",
        "tsc --noEmit 오류 없음",
        "eslint 경고 없음",
        "prettier 적용",
    ],
    conventions: &[
        "strict 모드 활성화",
        "any 타입 사용 금지",
        "명시적 반환 타입 선언",
        "null 대신 undefined 통일",
    ],
    build_command: "npm run build",
    test_command: "npm test",
    lint_command: "npx eslint .",
    default_allowed_command_prefixes: &["npm", "npx", "node"],
    allowed_file_commands_windows: &["powershell", "xcopy", "robocopy"],
    allowed_file_commands_unix: &["cp", "mv", "rm", "mkdir", "touch", "cat", "grep", "sed", "find", "chmod", "diff", "tar"],
};

pub static GO: LangTemplate = LangTemplate {
    id: "go",
    display_name: "Go 1.22+",
    tech_stack: "Go 1.22+ (golangci-lint)",
    dod_items: &[
        "go test ./... 통과",
        "go vet ./... 통과",
        "golangci-lint run 통과",
    ],
    conventions: &[
        "error wrapping (fmt.Errorf %w)",
        "context.Context 첫 번째 인자 전파",
        "goroutine leak 없음 (defer cancel)",
        "명시적 interface 선언",
    ],
    build_command: "go build ./...",
    test_command: "go test ./...",
    lint_command: "go vet ./...",
    default_allowed_command_prefixes: &["go", "golangci-lint"],
    allowed_file_commands_windows: &["powershell", "xcopy", "robocopy"],
    allowed_file_commands_unix: &["cp", "mv", "rm", "mkdir", "touch", "cat", "grep", "sed", "find", "chmod", "diff", "tar"],
};

pub static JAVA: LangTemplate = LangTemplate {
    id: "java",
    display_name: "Java 21+ (Maven)",
    tech_stack: "Java 21+ (Maven/Gradle, SpotBugs, Checkstyle)",
    dod_items: &[
        "mvn test 통과",
        "SpotBugs 경고 없음",
        "Checkstyle 통과",
        "Javadoc public API 완성",
    ],
    conventions: &[
        "Effective Java 3판 패턴 준수",
        "불변 객체 우선 설계",
        "raw type 사용 금지",
        "checked exception 최소화",
    ],
    build_command: "mvn package -DskipTests",
    test_command: "mvn test",
    lint_command: "mvn checkstyle:check spotbugs:check",
    default_allowed_command_prefixes: &["mvn", "gradle", "java"],
    allowed_file_commands_windows: &["powershell", "xcopy", "robocopy"],
    allowed_file_commands_unix: &["cp", "mv", "rm", "mkdir", "touch", "cat", "grep", "sed", "find", "chmod", "diff", "tar"],
};

pub static SPRING_BOOT: LangTemplate = LangTemplate {
    id: "spring_boot",
    display_name: "Spring Boot 3.x",
    tech_stack: "Spring Boot 3.x (JPA, REST API, Actuator, Springdoc OpenAPI)",
    dod_items: &[
        "mvn test 통과 (단위 + @SpringBootTest 통합)",
        "Actuator /health 200 반환",
        "OpenAPI 문서 자동 생성 확인",
        "운영 환경 로그 레벨 INFO 이상",
    ],
    conventions: &[
        "REST API: 리소스 명사 복수형·HTTP 동사 준수",
        "@Transactional 메서드 단위 명시적 적용",
        "DTO/Entity 계층 분리 필수 (Entity 직접 노출 금지)",
        "의존성 주입: 생성자 주입 사용 (@Autowired 금지)",
        "@ControllerAdvice 전역 예외 핸들러 사용",
    ],
    build_command: "mvn package -DskipTests",
    test_command: "mvn test",
    lint_command: "mvn checkstyle:check",
    default_allowed_command_prefixes: &["mvn", "gradle", "java"],
    allowed_file_commands_windows: &["powershell", "xcopy", "robocopy"],
    allowed_file_commands_unix: &["cp", "mv", "rm", "mkdir", "touch", "cat", "grep", "sed", "find", "chmod", "diff", "tar"],
};

pub static ALL_TEMPLATES: &[&LangTemplate] = &[
    &RUST, &PYTHON, &TYPESCRIPT, &GO, &JAVA, &SPRING_BOOT,
];

#[allow(dead_code)]
pub fn find_template(id: &str) -> Option<&'static LangTemplate> {
    ALL_TEMPLATES.iter().copied().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_templates_non_empty() {
        for t in ALL_TEMPLATES {
            assert!(!t.id.is_empty(), "id 비어있음: {}", t.display_name);
            assert!(!t.tech_stack.is_empty());
            assert!(!t.dod_items.is_empty());
            assert!(!t.conventions.is_empty());
            assert!(!t.test_command.is_empty());
        }
    }

    #[test]
    fn find_template_returns_correct() {
        assert_eq!(find_template("rust").unwrap().id, "rust");
        assert_eq!(find_template("spring_boot").unwrap().id, "spring_boot");
        assert!(find_template("unknown").is_none());
    }

    #[test]
    fn template_count_is_six() {
        assert_eq!(ALL_TEMPLATES.len(), 6);
    }
}
