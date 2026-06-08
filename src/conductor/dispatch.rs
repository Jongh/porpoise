//! Dispatch — 격리 worktree에서 실제 코딩 에이전트에게 task를 통째로 위임.
//!
//! 기존 Planning + Development + Testing 3개 호출을 단일 에이전틱 실행 1회가 대체한다.
//! 에이전트는 격리된 git worktree 안에서 자유롭게 계획·코딩·테스트하고,
//! Porpoise는 그 결과 diff를 캡처해 Verify 단계로 넘긴다.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::claude::runner::ClaudeRunner;
use crate::conductor::brief::Brief;
use crate::conductor::git::run_git;

/// task별 격리 worktree. Drop이 아니라 명시적 `remove()`로 정리한다
/// (정리 실패를 로깅·무시하기 위함).
pub struct Worktree {
    pub repo_root: PathBuf,
    pub path: PathBuf,
    pub branch: String,
}

impl Worktree {
    /// 현재 HEAD에서 분기한 task 전용 worktree를 생성한다.
    /// 동일 task의 이전 worktree·브랜치가 남아 있으면 먼저 정리한다.
    pub fn create(repo_root: &Path, task_id: &str) -> Result<Worktree> {
        let branch = dispatch_branch_name(task_id);
        let path = worktree_path(repo_root, task_id);

        // 이전 잔여 정리 (실패 무시 — 존재하지 않을 수 있음)
        let path_str = path.to_string_lossy().to_string();
        run_git(repo_root, &["worktree", "remove", "--force", &path_str]);
        run_git(repo_root, &["worktree", "prune"]);
        run_git(repo_root, &["branch", "-D", &branch]);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("worktree 부모 디렉토리 생성 실패: {}", parent.display()))?;
        }

        let out = run_git(
            repo_root,
            &["worktree", "add", "--force", "-b", &branch, &path_str, "HEAD"],
        );
        if !out.success {
            anyhow::bail!("git worktree 생성 실패: {}", out.stderr.trim());
        }

        Ok(Worktree {
            repo_root: repo_root.to_path_buf(),
            path,
            branch,
        })
    }

    /// worktree 안의 모든 변경(미추적 포함)을 스테이징한 뒤 diff를 반환한다.
    /// 변경이 없으면 빈 문자열.
    pub fn capture_diff(&self) -> String {
        run_git(&self.path, &["add", "-A"]);
        run_git(&self.path, &["diff", "--cached"]).stdout
    }

    /// worktree의 변경을 task 브랜치에 커밋한다 (스테이징 후).
    pub fn commit(&self, message: &str) -> Result<()> {
        run_git(&self.path, &["add", "-A"]);
        let out = run_git(&self.path, &["commit", "-m", message]);
        if !out.success {
            anyhow::bail!("worktree 커밋 실패: {}", out.stderr.trim());
        }
        Ok(())
    }

    /// worktree와 task 브랜치를 정리한다. 정리 실패는 무시한다.
    pub fn remove(self) {
        let path_str = self.path.to_string_lossy().to_string();
        run_git(&self.repo_root, &["worktree", "remove", "--force", &path_str]);
        run_git(&self.repo_root, &["worktree", "prune"]);
        run_git(&self.repo_root, &["branch", "-D", &self.branch]);
    }

    /// 격리 worktree 안에서 에이전트를 풀 에이전틱 모드로 실행하고 출력을 반환한다.
    /// `stream=false`면 출력을 캡처만 한다(병렬 실행 인터리브 방지, M23).
    pub fn run_agent(
        &self,
        runner: &ClaudeRunner,
        brief: &Brief,
        model: Option<&str>,
        stream: bool,
    ) -> Result<String> {
        runner
            .run_agentic(&brief.render(), &self.path, model, stream)
            .context("에이전트 dispatch 실행 실패")
    }
}

/// task ID에서 dispatch 브랜치 이름을 만든다. 예: "M10-T01" → "porpoise/m10-t01".
pub fn dispatch_branch_name(task_id: &str) -> String {
    format!("porpoise/{}", sanitize(task_id))
}

/// task별 worktree 절대 경로. `.porpoise/worktrees/<task>` (gitignore 대상).
pub fn worktree_path(repo_root: &Path, task_id: &str) -> PathBuf {
    repo_root
        .join(".porpoise")
        .join("worktrees")
        .join(sanitize(task_id))
}

/// 브랜치·디렉토리 이름에 안전하도록 task ID를 정규화한다.
fn sanitize(task_id: &str) -> String {
    task_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conductor::git::run_git;

    #[test]
    fn branch_name_is_lowercased_and_namespaced() {
        assert_eq!(dispatch_branch_name("M10-T01"), "porpoise/m10-t01");
        assert_eq!(dispatch_branch_name("M2-T07"), "porpoise/m2-t07");
    }

    #[test]
    fn sanitize_replaces_unsafe_chars() {
        assert_eq!(sanitize("M10-T01"), "m10-t01");
        assert_eq!(sanitize("M1 T1/x"), "m1-t1-x");
    }

    #[test]
    fn worktree_path_under_porpoise() {
        let p = worktree_path(Path::new("/repo"), "M10-T01");
        assert!(p.ends_with("worktrees/m10-t01") || p.ends_with("worktrees\\m10-t01"));
        assert!(p.to_string_lossy().contains(".porpoise"));
    }

    /// 실제 임시 git 저장소를 만들어 init·커밋 후 헬퍼 동작을 검증한다.
    fn init_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "test@example.com"]);
        run_git(root, &["config", "user.name", "Test"]);
        std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-m", "init"]);
        tmp
    }

    #[test]
    fn create_capture_and_remove_worktree() {
        let tmp = init_repo();
        let root = tmp.path();

        let wt = Worktree::create(root, "M10-T01").expect("worktree 생성");
        assert!(wt.path.exists(), "worktree 디렉토리가 생성되어야 함");
        assert_eq!(wt.branch, "porpoise/m10-t01");

        // 변경 없음 → diff 비어 있음
        assert!(wt.capture_diff().trim().is_empty());

        // 파일 추가 → diff에 내용 포함
        std::fs::write(wt.path.join("new.txt"), "hello conductor\n").unwrap();
        let diff = wt.capture_diff();
        assert!(diff.contains("new.txt"), "diff에 새 파일이 포함되어야 함: {}", diff);
        assert!(diff.contains("hello conductor"));

        let branch = wt.branch.clone();
        wt.remove();

        // 브랜치가 제거되었는지 확인
        let branches = run_git(root, &["branch", "--list", &branch]).stdout;
        assert!(branches.trim().is_empty(), "브랜치가 정리되어야 함: {:?}", branches);
    }

    #[test]
    fn create_cleans_up_stale_worktree() {
        let tmp = init_repo();
        let root = tmp.path();

        let wt1 = Worktree::create(root, "M10-T02").expect("첫 생성");
        std::fs::write(wt1.path.join("a.txt"), "1\n").unwrap();
        // remove 없이 동일 task 재생성 — 잔여 정리 후 성공해야 함
        let wt2 = Worktree::create(root, "M10-T02").expect("재생성");
        assert!(wt2.path.exists());
        wt2.remove();
    }

    #[test]
    fn commit_in_worktree_creates_commit_on_branch() {
        let tmp = init_repo();
        let root = tmp.path();

        let wt = Worktree::create(root, "M10-T03").expect("worktree 생성");
        std::fs::write(wt.path.join("feature.txt"), "impl\n").unwrap();
        wt.commit("[M10-T03] 구현").expect("커밋");

        // 브랜치 최신 커밋 메시지 확인
        let log = run_git(&wt.path, &["log", "-1", "--pretty=%s"]).stdout;
        assert!(log.contains("M10-T03"), "커밋 메시지 확인: {}", log);
        wt.remove();
    }

    #[test]
    fn remove_cleans_up_dirty_worktree() {
        // M21 에러 경로 모사: 에이전트가 worktree를 변경했으나 병합 전 정리.
        // 미커밋 변경이 있어도 worktree·브랜치가 잔여 없이 정리되어야 한다.
        let tmp = init_repo();
        let root = tmp.path();

        let wt = Worktree::create(root, "M21-T04").expect("worktree 생성");
        std::fs::write(wt.path.join("dirty.txt"), "uncommitted\n").unwrap();
        let branch = wt.branch.clone();
        let wt_path = wt.path.clone();

        wt.remove();

        assert!(!wt_path.exists(), "미커밋 변경이 있는 worktree도 정리되어야 함");
        let branches = run_git(root, &["branch", "--list", &branch]).stdout;
        assert!(branches.trim().is_empty(), "브랜치가 정리되어야 함: {:?}", branches);
    }
}
