pub fn print_error(err: &anyhow::Error) {
    eprintln!();
    eprintln!("  [Porpoise Error] {}", err);
    let mut source = err.source();
    while let Some(cause) = source {
        eprintln!("  원인: {}", cause);
        source = cause.source();
    }
    eprintln!("  해결: {}", resolve_hint(err));
    eprintln!();
}

fn resolve_hint(err: &anyhow::Error) -> &'static str {
    let full = format!("{:?}", err).to_lowercase();

    if full.contains("program not found")
        || full.contains("no such file or directory")
        || (full.contains("os error 2") && !full.contains("permission"))
    {
        "EDITOR/VISUAL 환경변수 또는 PATH를 확인하세요. 해당 프로그램이 설치되어 있지 않을 수 있습니다."
    } else if full.contains("permission denied")
        || full.contains("access is denied")
        || full.contains("os error 13")
    {
        "해당 경로에 쓰기 권한이 있는지 확인하세요."
    } else if full.contains("connection refused")
        || full.contains("connection reset")
        || full.contains("timed out")
        || full.contains("network")
        || full.contains("dns")
        || full.contains("connect error")
    {
        "네트워크 연결 또는 API 엔드포인트(api_base_url)를 확인하세요."
    } else if full.contains("anthropic_api_key") || full.contains("x-api-key") {
        "환경변수 ANTHROPIC_API_KEY가 설정되어 있는지 확인하세요."
    } else if full.contains("openai_api_key") || full.contains("bearer") {
        "환경변수 OPENAI_API_KEY 또는 workspace.toml의 api_key_env를 확인하세요."
    } else if full.contains("타임아웃") {
        "명령 실행 시간 초과입니다. workspace.toml의 verify_timeout_secs를 늘려보세요."
    } else {
        ".porpoise/logs/ 로그 파일에서 상세 원인을 확인하세요."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_hint_not_found() {
        let err = anyhow::anyhow!("program not found");
        assert!(resolve_hint(&err).contains("PATH"));
    }

    #[test]
    fn resolve_hint_permission() {
        let err = anyhow::anyhow!("permission denied: /etc/hosts");
        assert!(resolve_hint(&err).contains("쓰기 권한"));
    }

    #[test]
    fn resolve_hint_network() {
        let err = anyhow::anyhow!("connection refused");
        assert!(resolve_hint(&err).contains("API 엔드포인트"));
    }

    #[test]
    fn resolve_hint_fallback() {
        let err = anyhow::anyhow!("unknown situation xyz");
        assert!(resolve_hint(&err).contains("로그 파일"));
    }
}
