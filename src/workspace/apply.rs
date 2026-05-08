use anyhow::{Result, bail};
use std::path::Path;
use crate::session::v0_7::{AppliedOperationsSummary, FileOperation};
use crate::utils::fs::{delete_file, move_file, write_file};

pub fn apply_file_operations(
    project_root: &Path,
    operations: &[FileOperation],
) -> Result<AppliedOperationsSummary> {
    let mut summary = AppliedOperationsSummary::default();

    for op in operations {
        let result = apply_single_op(project_root, op);
        match result {
            Ok(_) => match op.op.as_str() {
                "write" => summary.files_written += 1,
                "delete" => summary.files_deleted += 1,
                "rename" => summary.files_renamed += 1,
                _ => {}
            },
            Err(e) => {
                eprintln!("⚠ FileOperation 실패 [{}] {}: {}", op.op, op.path, e);
            }
        }
    }

    Ok(summary)
}

fn apply_single_op(project_root: &Path, op: &FileOperation) -> Result<()> {
    match op.op.as_str() {
        "write" => {
            let abs_path = resolve_op_path(project_root, &op.path)?;
            let content = op.content.as_deref().unwrap_or("");
            write_file(&abs_path, content, project_root)
        }
        "delete" => {
            let abs_path = resolve_op_path(project_root, &op.path)?;
            delete_file(&abs_path, project_root)
        }
        "rename" => {
            let src = resolve_op_path(project_root, &op.path)?;
            let new_path = op.new_path.as_deref()
                .ok_or_else(|| anyhow::anyhow!("rename op에 new_path가 없습니다"))?;
            let dst = resolve_op_path(project_root, new_path)?;
            move_file(&src, &dst, project_root)
        }
        "write_patch" => {
            bail!("write_patch는 v0.8.0에서 구현 예정입니다. write op을 사용하세요.")
        }
        other => {
            bail!("알 수 없는 FileOperation.op: {}", other)
        }
    }
}

fn resolve_op_path(project_root: &Path, op_path: &str) -> Result<std::path::PathBuf> {
    // 상대/절대 경로 모두 project_root 기준으로 정규화
    let p = std::path::Path::new(op_path);
    let abs = if p.is_absolute() { p.to_path_buf() } else { project_root.join(p) };

    if !crate::utils::fs::is_within_project(&abs, project_root) {
        bail!("FileOperation 경로가 프로젝트 루트 외부입니다: {}", op_path);
    }
    Ok(abs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_op(op: &str, path: &str, content: Option<&str>, new_path: Option<&str>) -> FileOperation {
        FileOperation {
            op: op.to_string(),
            path: path.to_string(),
            content: content.map(str::to_string),
            patch: None,
            patch_format: None,
            new_path: new_path.map(str::to_string),
        }
    }

    #[test]
    fn write_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ops = vec![make_op("write", "src/new.rs", Some("fn f() {}"), None)];
        apply_file_operations(tmp.path(), &ops).unwrap();
        assert_eq!(fs::read_to_string(tmp.path().join("src/new.rs")).unwrap(), "fn f() {}");
    }

    #[test]
    fn write_overwrites_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("a.rs");
        fs::write(&file, "old").unwrap();
        let ops = vec![make_op("write", "a.rs", Some("new"), None)];
        apply_file_operations(tmp.path(), &ops).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
    }

    #[test]
    fn delete_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("del.rs");
        fs::write(&file, "x").unwrap();
        let ops = vec![make_op("delete", "del.rs", None, None)];
        apply_file_operations(tmp.path(), &ops).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn rename_moves_file() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("old.rs"), "x").unwrap();
        let ops = vec![make_op("rename", "old.rs", None, Some("new.rs"))];
        apply_file_operations(tmp.path(), &ops).unwrap();
        assert!(!tmp.path().join("old.rs").exists());
        assert!(tmp.path().join("new.rs").exists());
    }

    #[test]
    fn path_outside_project_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let ops = vec![make_op("write", "../escape.rs", Some("x"), None)];
        // 에러가 반환되지 않고 내부적으로 경고를 출력하며 계속 진행
        // summary.files_written = 0이면 실패 처리됨
        let summary = apply_file_operations(tmp.path(), &ops).unwrap();
        assert_eq!(summary.files_written, 0);
    }

    #[test]
    fn write_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let ops = vec![make_op("write", "a/b/c/deep.rs", Some("fn d() {}"), None)];
        apply_file_operations(tmp.path(), &ops).unwrap();
        assert!(tmp.path().join("a/b/c/deep.rs").exists());
    }

    #[test]
    fn unknown_op_is_skipped_gracefully() {
        let tmp = tempfile::tempdir().unwrap();
        let ops = vec![make_op("write_patch", "a.rs", Some("patch"), None)];
        let summary = apply_file_operations(tmp.path(), &ops).unwrap();
        assert_eq!(summary.files_written, 0);
    }
}
