/// 실행 에러를 표준화된 형식으로 출력합니다.
/// 파일 쓰기 실패의 경우 write_file()이 경로 정보를 에러 체인에 포함하므로
/// 이 함수 하나로 모든 케이스를 커버합니다.
pub fn print_error(err: &anyhow::Error) {
    eprintln!();
    eprintln!("  [Porpoise Error] {}", err);
    let mut source = err.source();
    while let Some(cause) = source {
        eprintln!("  원인: {}", cause);
        source = cause.source();
    }
    eprintln!("  해결: 해당 디렉토리에 쓰기 권한이 있는지 확인하세요.");
    eprintln!();
}
