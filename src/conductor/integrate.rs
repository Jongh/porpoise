//! Integrate — PASS 시 worktree 병합·task 완료 처리, FAIL 시 재투입/중단 결정.

use anyhow::{Context, Result};
use std::path::Path;

use crate::conductor::dispatch::Worktree;
use crate::conductor::git::run_git;
use crate::conductor::verify::Verdict;

/// Verify 결과와 재투입 횟수로 결정되는 다음 동작.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrateDecision {
    /// PASS — worktree를 병합하고 task를 완료 처리한다.
    Merge,
    /// FAIL이지만 재투입 여력이 남음 — 피드백과 함께 다시 Dispatch.
    Redispatch { feedback: String },
    /// FAIL이고 재투입 한도 소진 — 사용자 개입 필요, 중단.
    Halt { feedback: String },
}

/// 판정과 현재까지의 재투입 횟수로 다음 동작을 결정한다 (순수 함수).
///
/// `redispatch_count`는 지금까지 수행한 재투입 횟수(최초 투입 제외).
pub fn decide(verdict: &Verdict, redispatch_count: u32, max_redispatch: u32) -> IntegrateDecision {
    if verdict.pass {
        return IntegrateDecision::Merge;
    }
    if redispatch_count < max_redispatch {
        IntegrateDecision::Redispatch { feedback: verdict.feedback.clone() }
    } else {
        IntegrateDecision::Halt { feedback: verdict.feedback.clone() }
    }
}

/// PASS된 task를 통합한다: worktree 커밋 → 브랜치 병합.
///
/// worktree 정리는 호출자가 담당한다(에러·halt 경로 포함 항상 정리 보장을 위해).
/// `&Worktree`만 빌려 커밋·병합만 수행하며, 병합 실패 시 에러를 전파한다.
pub fn finalize(wt: &Worktree, repo_root: &Path, commit_msg: &str) -> Result<()> {
    wt.commit(commit_msg).context("worktree 커밋 실패")?;
    merge_worktree(repo_root, &wt.branch).context("worktree 병합 실패")?;
    Ok(())
}

/// task 브랜치를 현재 브랜치로 병합한다.
///
/// 순차 단일 task 시나리오에서는 main HEAD가 분기 지점에서 이동하지 않았으므로
/// fast-forward로 병합된다. 충돌이 발생하면(병렬 함대 — M12 범위) 병합을 중단(abort)하고
/// 에러를 반환한다.
pub fn merge_worktree(repo_root: &Path, branch: &str) -> Result<()> {
    let out = run_git(repo_root, &["merge", "--no-edit", branch]);
    if !out.success {
        // 충돌 등으로 실패 → 병합 상태를 깨끗이 되돌림
        run_git(repo_root, &["merge", "--abort"]);
        anyhow::bail!(
            "worktree 병합 실패 ({}): {}\n순차 단일 task에서는 충돌이 없어야 합니다. \
             병렬 실행 충돌 해소는 후속 마일스톤(M12) 범위입니다.",
            branch,
            out.stderr.trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conductor::git::run_git;

    #[test]
    fn decide_pass_merges() {
        assert_eq!(decide(&Verdict::pass(), 0, 2), IntegrateDecision::Merge);
        assert_eq!(decide(&Verdict::pass(), 5, 2), IntegrateDecision::Merge);
    }

    #[test]
    fn decide_fail_with_budget_redispatches() {
        let d = decide(&Verdict::fail("고쳐"), 0, 2);
        assert_eq!(d, IntegrateDecision::Redispatch { feedback: "고쳐".to_string() });
        let d2 = decide(&Verdict::fail("고쳐"), 1, 2);
        assert_eq!(d2, IntegrateDecision::Redispatch { feedback: "고쳐".to_string() });
    }

    #[test]
    fn decide_fail_exhausted_halts() {
        let d = decide(&Verdict::fail("끝"), 2, 2);
        assert_eq!(d, IntegrateDecision::Halt { feedback: "끝".to_string() });
        let d2 = decide(&Verdict::fail("끝"), 3, 2);
        assert_eq!(d2, IntegrateDecision::Halt { feedback: "끝".to_string() });
    }

    #[test]
    fn decide_zero_budget_halts_immediately() {
        let d = decide(&Verdict::fail("즉시 중단"), 0, 0);
        assert_eq!(d, IntegrateDecision::Halt { feedback: "즉시 중단".to_string() });
    }

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
    fn merge_brings_worktree_changes_into_main() {
        let tmp = init_repo();
        let root = tmp.path();

        let wt = Worktree::create(root, "M10-T05").expect("worktree 생성");
        std::fs::write(wt.path.join("feature.txt"), "merged content\n").unwrap();
        wt.commit("[M10-T05] 기능 추가").expect("커밋");
        let branch = wt.branch.clone();

        merge_worktree(root, &branch).expect("병합 성공");

        // 병합 후 main 작업 트리에 파일이 나타나야 함
        let merged = root.join("feature.txt");
        assert!(merged.exists(), "병합된 파일이 main에 존재해야 함");
        // Windows의 autocrlf로 줄바꿈이 변환될 수 있어 내용만 비교
        let content = std::fs::read_to_string(&merged).unwrap();
        assert!(content.contains("merged content"), "병합 내용 확인: {:?}", content);

        wt.remove();
    }

    #[test]
    fn finalize_commits_and_merges_caller_cleans_up() {
        let tmp = init_repo();
        let root = tmp.path();

        let wt = Worktree::create(root, "M10-T06").expect("worktree 생성");
        let branch = wt.branch.clone();
        let wt_path = wt.path.clone();
        std::fs::write(wt.path.join("done.txt"), "finalized\n").unwrap();

        // finalize는 커밋·병합만 — 정리는 호출자(여기선 테스트)가 수행
        finalize(&wt, root, "[M10-T06] 완료").expect("finalize 성공");

        // 병합 결과가 main에 반영
        assert!(root.join("done.txt").exists(), "병합된 파일이 main에 있어야 함");
        let log = run_git(root, &["log", "-1", "--pretty=%s"]).stdout;
        assert!(log.contains("M10-T06"), "main HEAD에 task 커밋이 있어야 함: {}", log);

        // 호출자가 정리
        wt.remove();
        assert!(!wt_path.exists(), "worktree 디렉토리가 제거되어야 함");
        let branches = run_git(root, &["branch", "--list", &branch]).stdout;
        assert!(branches.trim().is_empty(), "브랜치가 정리되어야 함: {:?}", branches);
    }
}
