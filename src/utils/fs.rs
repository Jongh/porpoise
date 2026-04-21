use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// 부모 디렉토리를 자동 생성한 뒤 파일을 씁니다.
/// 실패 시 경로 정보가 포함된 에러 메시지를 반환합니다.
pub fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("디렉토리 생성 실패: {}", parent.display()))?;
    }
    fs::write(path, content)
        .with_context(|| format!("파일 쓰기 실패: {}", path.display()))?;
    Ok(())
}
