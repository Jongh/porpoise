use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

/// Windows 장경로 접두어(`\\?\`)를 제거합니다.
fn strip_unc_prefix(path: std::path::PathBuf) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix("\\\\?\\") {
        std::path::PathBuf::from(stripped)
    } else {
        path
    }
}

/// `..`와 `.`을 해소하는 lexical 경로 정규화입니다 (파일시스템 접근 없음).
fn lexical_normalize(path: &Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut parts: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let last_is_normal = parts.last().map(|c| matches!(c, Component::Normal(_))).unwrap_or(false);
                if last_is_normal {
                    parts.pop();
                }
            }
            c => parts.push(c),
        }
    }
    parts.iter().collect()
}

/// 경로가 프로젝트 루트 하위에 있는지 확인합니다.
/// 존재하는 경로는 canonicalize로 심볼릭 링크를 해소합니다.
/// 미존재 경로는 부모 체인을 상향 탐색하여 canonicalize 가능한 가장 가까운 조상을 찾아
/// 심볼릭 링크를 해소한 뒤 나머지 경로를 lexical로 붙입니다.
/// 이로써 미존재 경로에서도 심볼릭 링크를 통한 경계 탈출을 차단합니다.
pub fn is_within_project(path: &Path, project_root: &Path) -> bool {
    let norm_root = fs::canonicalize(project_root)
        .map(strip_unc_prefix)
        .unwrap_or_else(|_| lexical_normalize(project_root));

    let norm_path = fs::canonicalize(path)
        .map(strip_unc_prefix)
        .unwrap_or_else(|_| canonicalize_via_parent(path, &norm_root));

    norm_path.starts_with(&norm_root)
}

/// 미존재 경로에 대해 부모 체인을 올라가며 canonicalize를 시도합니다.
/// 심볼릭 링크를 포함한 부모 경로를 정확히 해소한 뒤 나머지 컴포넌트를 붙입니다.
fn canonicalize_via_parent(path: &Path, norm_root: &Path) -> std::path::PathBuf {
    let mut current = if path.is_absolute() {
        path.to_path_buf()
    } else {
        norm_root.join(path)
    };
    let mut tail: Vec<std::ffi::OsString> = Vec::new();

    loop {
        let parent = match current.parent() {
            Some(p) if p != current => p.to_path_buf(),
            _ => break,
        };
        if let Some(name) = current.file_name() {
            tail.push(name.to_os_string());
        }
        current = parent;
        if let Ok(canon) = fs::canonicalize(&current) {
            let mut result = strip_unc_prefix(canon);
            for comp in tail.iter().rev() {
                result = result.join(comp);
            }
            return result;
        }
    }
    // 루트까지 올라가도 canonicalize 불가한 극단 케이스 — lexical fallback
    lexical_normalize(path)
}

/// 부모 디렉토리를 자동 생성한 뒤 파일을 씁니다.
/// 실패 시 경로 정보가 포함된 에러 메시지를 반환합니다.
pub fn write_file(path: &Path, content: &str, project_root: &Path) -> Result<()> {
    if !is_within_project(path, project_root) {
        bail!("파일 쓰기 거부: 프로젝트 루트 외부 경로 — {}", path.display());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("디렉토리 생성 실패: {}", parent.display()))?;
    }
    fs::write(path, content)
        .with_context(|| format!("파일 쓰기 실패: {}", path.display()))?;
    Ok(())
}

/// 파일을 삭제합니다. 프로젝트 루트 외부 경로는 거부합니다. 멱등성 보장 (미존재 시 Ok).
#[allow(dead_code)]
pub fn delete_file(path: &Path, project_root: &Path) -> Result<()> {
    if !is_within_project(path, project_root) {
        bail!("파일 삭제 거부: 프로젝트 루트 외부 경로 — {}", path.display());
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("파일 삭제 실패: {}", path.display())),
    }
}

/// 디렉토리를 재귀 삭제합니다. 프로젝트 루트 외부 경로는 거부합니다. 멱등성 보장 (미존재 시 Ok).
#[allow(dead_code)]
pub fn delete_dir(path: &Path, project_root: &Path) -> Result<()> {
    if !is_within_project(path, project_root) {
        bail!("디렉토리 삭제 거부: 프로젝트 루트 외부 경로 — {}", path.display());
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("디렉토리 삭제 실패: {}", path.display())),
    }
}

/// 파일을 이동합니다. src·dst 양쪽 모두 프로젝트 루트 하위여야 합니다.
/// dst의 부모 디렉토리가 없으면 자동 생성합니다.
#[allow(dead_code)]
pub fn move_file(src: &Path, dst: &Path, project_root: &Path) -> Result<()> {
    if !is_within_project(src, project_root) {
        bail!("파일 이동 거부: 원본이 프로젝트 루트 외부 — {}", src.display());
    }
    if !is_within_project(dst, project_root) {
        bail!("파일 이동 거부: 대상이 프로젝트 루트 외부 — {}", dst.display());
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("디렉토리 생성 실패: {}", parent.display()))?;
    }
    fs::rename(src, dst)
        .with_context(|| format!("파일 이동 실패: {} → {}", src.display(), dst.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_project() -> (std::path::PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let root = dir.path().to_path_buf();
        (root, dir)
    }

    #[test]
    fn write_file_creates_nested_dirs() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        let target = root.join("a").join("b").join("file.txt");
        write_file(&target, "hello", root).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
    }

    #[test]
    fn write_file_rejects_outside_project() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        let outside = env::temp_dir().join("outside_test_file.txt");
        let result = write_file(&outside, "bad", root);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("프로젝트 루트 외부"));
    }

    #[test]
    fn is_within_project_inside() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        let inner = root.join("sub").join("file.txt");
        assert!(is_within_project(&inner, root));
    }

    #[test]
    fn is_within_project_outside() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        let outside = env::temp_dir();
        assert!(!is_within_project(&outside, root));
    }

    #[test]
    fn is_within_project_escape_attempt() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        // lexical escape attempt via parent traversal
        let escape = root.join("..").join("other");
        assert!(!is_within_project(&escape, root));
    }

    #[test]
    fn delete_file_idempotent() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        let file = root.join("to_delete.txt");
        // 미존재 파일 삭제 → Ok (멱등성)
        assert!(delete_file(&file, root).is_ok());
        // 존재하는 파일 삭제 → Ok
        fs::write(&file, "x").unwrap();
        assert!(delete_file(&file, root).is_ok());
        assert!(!file.exists());
    }

    #[test]
    fn delete_file_blocks_outside() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        let outside = env::temp_dir().join("outside_delete_test.txt");
        let result = delete_file(&outside, root);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("프로젝트 루트 외부"));
    }

    #[test]
    fn move_file_within_project() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        let src = root.join("src.txt");
        let dst = root.join("sub").join("dst.txt");
        fs::write(&src, "content").unwrap();
        move_file(&src, &dst, root).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "content");
    }

    #[test]
    fn move_file_blocks_external_dst() {
        let (_root, dir) = temp_project();
        let root = dir.path();
        let src = root.join("src.txt");
        fs::write(&src, "x").unwrap();
        let outside_dst = env::temp_dir().join("outside_move_dst.txt");
        let result = move_file(&src, &outside_dst, root);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("프로젝트 루트 외부"));
    }
}
