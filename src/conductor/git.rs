//! 지휘자 루프 전용 git 헬퍼.
//!
//! worktree 생성·diff 캡처·병합에 쓰이는 얇은 `git` 명령 래퍼.

use std::path::Path;
use std::process::Command;

/// `git` 명령 실행 결과.
pub struct GitOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// 지정한 디렉토리에서 `git <args>`를 실행한다.
pub fn run_git(cwd: &Path, args: &[&str]) -> GitOutput {
    match Command::new("git").current_dir(cwd).args(args).output() {
        Ok(out) => GitOutput {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        },
        Err(e) => GitOutput {
            success: false,
            stdout: String::new(),
            stderr: format!("git 실행 실패: {}", e),
        },
    }
}

/// git 저장소 여부 확인 (`git rev-parse --is-inside-work-tree`).
pub fn is_git_repo(cwd: &Path) -> bool {
    run_git(cwd, &["rev-parse", "--is-inside-work-tree"])
        .stdout
        .trim()
        == "true"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_git_reports_failure_in_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        // git status는 비-저장소에서 실패해야 함
        let out = run_git(tmp.path(), &["status"]);
        assert!(!out.success);
    }

    #[test]
    fn is_git_repo_false_for_plain_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_git_repo(tmp.path()));
    }
}
