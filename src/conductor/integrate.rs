//! Integrate — PASS 시 worktree 병합·task 완료 처리, FAIL 시 재투입/중단 결정.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::conductor::dispatch::Worktree;
use crate::conductor::git::{run_git, GitOutput};
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

/// task 브랜치를 현재 브랜치로 병합한다 (단일 task 경로 — fast-forward 가정).
///
/// 충돌이 발생하면 병합을 중단(abort)하고 에러를 반환한다. 병렬 함대의 충돌 인지 병합은
/// `try_merge_worktree`를 사용한다.
pub fn merge_worktree(repo_root: &Path, branch: &str) -> Result<()> {
    let out = merge_with_untracked_recovery(repo_root, branch);
    if !out.success {
        run_git(repo_root, &["merge", "--abort"]);
        anyhow::bail!("worktree 병합 실패 ({}): {}", branch, out.stderr.trim());
    }
    Ok(())
}

/// 병합을 시도하되, 메인의 **untracked 파일이 덮어쓰기로 충돌**하면(에이전트가 worktree에서
/// 생성·커밋한 산출물과 동명) 그 파일을 백업으로 옮기고 **한 번 재시도**한다 (M29).
///
/// untracked 덮어쓰기 유형이 아니면(내용 충돌 등) 원래 실패를 그대로 반환해 기존 처리(abort)에
/// 맡긴다. 데이터 손실이 없도록 파일은 삭제가 아니라 `.porpoise/merge-backup/<ts>/`로 이동한다.
fn merge_with_untracked_recovery(repo_root: &Path, branch: &str) -> GitOutput {
    let out = run_git(repo_root, &["merge", "--no-edit", branch]);
    if out.success {
        return out;
    }
    let files = match parse_untracked_overwrite(&out.stderr) {
        Some(f) => f,
        None => return out, // untracked 덮어쓰기 케이스 아님 → 기존 처리
    };
    match backup_untracked(repo_root, &files) {
        Ok(backup) => {
            println!(
                "  ⚠ 병합 충돌(untracked) — {}개 파일을 {}로 백업 후 재시도",
                files.len(),
                backup.display()
            );
            run_git(repo_root, &["merge", "--no-edit", branch])
        }
        Err(_) => out, // 백업 실패 → 원래 실패 반환(안전)
    }
}

/// 병합 실패 stderr에서 "untracked working tree files would be overwritten" 충돌 파일 목록을
/// 추출한다. 해당 유형이 아니면 None.
fn parse_untracked_overwrite(stderr: &str) -> Option<Vec<String>> {
    if !stderr.contains("untracked working tree files would be overwritten") {
        return None;
    }
    let mut files = Vec::new();
    let mut in_list = false;
    for line in stderr.lines() {
        if line.contains("would be overwritten by merge") {
            in_list = true;
            continue;
        }
        if line.contains("Please move or remove") || line.trim() == "Aborting" {
            break;
        }
        if in_list {
            let f = line.trim();
            if !f.is_empty() {
                files.push(f.to_string());
            }
        }
    }
    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

/// 충돌하는 untracked 파일을 `.porpoise/merge-backup/<ts>/`로 (상대경로 보존하여) 옮긴다.
/// 반환: 백업 디렉터리 경로.
fn backup_untracked(repo_root: &Path, files: &[String]) -> Result<PathBuf> {
    use chrono::Local;
    let ts = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let backup = repo_root.join(".porpoise").join("merge-backup").join(&ts);
    for f in files {
        let src = repo_root.join(f);
        if !src.exists() {
            continue;
        }
        let dst = backup.join(f);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("백업 디렉터리 생성 실패: {}", parent.display()))?;
        }
        // rename이 교차 디바이스 등으로 실패하면 copy+remove로 폴백
        if std::fs::rename(&src, &dst).is_err() {
            std::fs::copy(&src, &dst)
                .with_context(|| format!("백업 복사 실패: {}", src.display()))?;
            std::fs::remove_file(&src).ok();
        }
    }
    Ok(backup)
}

/// 병렬 함대에서 PASS된 task 하나를 통합한다: 커밋 → 충돌 인지 병합 → worktree 정리.
///
/// 순서가 중요하다 — `Worktree::remove()`가 브랜치를 삭제하므로 반드시 **병합을 먼저** 한다.
/// 병합 시도 후에는 결과(Merged/Conflicted/Err)와 무관하게 worktree를 정리한다(누수 방지).
pub fn integrate_parallel(wt: Worktree, repo_root: &Path, commit_msg: &str) -> Result<MergeOutcome> {
    let outcome = wt
        .commit(commit_msg)
        .context("worktree 커밋 실패")
        .and_then(|()| try_merge_worktree(repo_root, &wt.branch));
    wt.remove();
    outcome
}

/// 병렬 함대 통합의 병합 결과 (M23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// 깨끗이 병합됨 (FF 또는 3-way 자동 병합).
    Merged,
    /// 충돌 — 병합을 abort했으며, 해당 task는 갱신된 base에서 재투입해야 한다.
    Conflicted,
}

/// 충돌 인지 병합 (M23 병렬 함대). 병렬로 만든 여러 task 브랜치를 **순서대로** 통합할 때 쓴다.
///
/// 첫 병합 후 HEAD가 이동하므로 후속 병합은 non-FF가 될 수 있고, 겹치는 파일을 건드린 task는
/// 충돌한다. 충돌이면 `git merge --abort`로 되돌리고 `Conflicted`를 반환한다(에러 아님) —
/// 호출자가 해당 task를 재투입한다. 병합 외 사유로 실패하면(브랜치 부재 등) 에러를 전파한다.
pub fn try_merge_worktree(repo_root: &Path, branch: &str) -> Result<MergeOutcome> {
    let out = merge_with_untracked_recovery(repo_root, branch);
    if out.success {
        return Ok(MergeOutcome::Merged);
    }
    // 실패 — 진행 중인 병합을 abort. abort가 성공하면 충돌이었던 것(병합이 진행 중이었음).
    let abort = run_git(repo_root, &["merge", "--abort"]);
    if abort.success {
        Ok(MergeOutcome::Conflicted)
    } else {
        anyhow::bail!("병합 실패 (충돌 아님, {}): {}", branch, out.stderr.trim());
    }
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

    #[test]
    fn finalize_succeeds_when_agent_already_committed() {
        // M26 회귀: 에이전트가 worktree 안에서 이미 커밋한 경우에도 finalize가 성공하고
        // 병합되어야 한다. (commit()이 clean 트리에서 "nothing to commit"으로 bail하면 이 테스트가 잡음)
        let tmp = init_repo();
        let root = tmp.path();

        let wt = Worktree::create(root, "M26-T09").expect("worktree 생성");
        let branch = wt.branch.clone();
        std::fs::write(wt.path.join("agent.txt"), "agent committed\n").unwrap();
        // 에이전트가 직접 커밋 → 작업트리 clean
        wt.commit("[M26-T09] 에이전트 커밋").expect("에이전트 커밋");

        // finalize의 commit 단계는 새로 커밋할 게 없지만 에러 없이 진행되고 병합돼야 함
        finalize(&wt, root, "[M26-T09] 통합 커밋").expect("finalize는 성공해야 함");

        assert!(root.join("agent.txt").exists(), "에이전트 커밋 내용이 main에 병합되어야 함");
        let log = run_git(root, &["log", "-1", "--pretty=%s"]).stdout;
        assert!(log.contains("M26-T09"), "main에 task 커밋이 반영되어야 함: {}", log);

        wt.remove();
        assert!(run_git(root, &["branch", "--list", &branch]).stdout.trim().is_empty());
    }

    #[test]
    fn parse_untracked_overwrite_extracts_files() {
        let stderr = "error: The following untracked working tree files would be overwritten by merge:\n\tCargo.lock\n\tsrc/gen.rs\nPlease move or remove them before you merge.\nAborting\n";
        let files = parse_untracked_overwrite(stderr).unwrap();
        assert_eq!(files, vec!["Cargo.lock".to_string(), "src/gen.rs".to_string()]);
        // 내용 충돌 등 다른 유형은 None
        assert!(parse_untracked_overwrite("CONFLICT (content): Merge conflict in x").is_none());
    }

    #[test]
    fn merge_recovers_from_untracked_overwrite() {
        // M29 회귀: 메인에 untracked 동명 파일이 있고 task 브랜치가 같은 파일을 추가하면,
        // 과거엔 병합이 하드 실패했다. 이제 백업 후 재시도로 성공해야 한다.
        let tmp = init_repo();
        let root = tmp.path();

        let wt = Worktree::create(root, "M29-T02").expect("worktree");
        std::fs::write(wt.path.join("gen.txt"), "from agent\n").unwrap();
        wt.commit("[M29-T02] gen.txt").expect("commit");
        let branch = wt.branch.clone();

        // 메인에 추적되지 않는 동명 파일 (병합이 덮어쓰려 함)
        std::fs::write(root.join("gen.txt"), "stale untracked\n").unwrap();

        // 하드 실패가 아니라 백업+재시도로 성공해야 함
        merge_worktree(root, &branch).expect("untracked 백업 후 병합 성공해야 함");

        // 메인은 에이전트 내용으로 병합됨
        let content = std::fs::read_to_string(root.join("gen.txt")).unwrap();
        assert!(content.contains("from agent"), "에이전트 내용 병합: {:?}", content);

        // 기존 untracked 내용은 백업으로 보존(데이터 손실 없음)
        let backup_root = root.join(".porpoise").join("merge-backup");
        assert!(backup_root.exists(), "백업 디렉터리 생성됨");
        let mut found = false;
        for ts_dir in std::fs::read_dir(&backup_root).unwrap().flatten() {
            let bf = ts_dir.path().join("gen.txt");
            if bf.exists() {
                assert!(std::fs::read_to_string(&bf).unwrap().contains("stale untracked"));
                found = true;
            }
        }
        assert!(found, "백업된 untracked 파일이 있어야 함");
        wt.remove();
    }

    #[test]
    fn try_merge_clean_then_conflict() {
        // 병렬 함대 충돌 인지: 같은 파일을 다르게 건드린 두 브랜치를 순차 병합 →
        // 첫 번째 Merged, 두 번째 Conflicted(abort됨).
        let tmp = init_repo();
        let root = tmp.path();

        // 두 worktree 모두 동일 base(seed만 있는 HEAD)에서 분기
        let wt_a = Worktree::create(root, "M23-T01").expect("wtA");
        std::fs::write(wt_a.path.join("shared.txt"), "from A\n").unwrap();
        wt_a.commit("[M23-T01] A").expect("commit A");

        let wt_b = Worktree::create(root, "M23-T02").expect("wtB");
        std::fs::write(wt_b.path.join("shared.txt"), "from B\n").unwrap();
        wt_b.commit("[M23-T02] B").expect("commit B");

        // A 병합 → 깨끗 (main이 base에서 안 움직였으므로 FF)
        assert_eq!(
            try_merge_worktree(root, &wt_a.branch).unwrap(),
            MergeOutcome::Merged
        );
        // B 병합 → shared.txt add/add 충돌 → Conflicted (abort됨)
        assert_eq!(
            try_merge_worktree(root, &wt_b.branch).unwrap(),
            MergeOutcome::Conflicted
        );

        // 충돌 abort 후 작업 트리가 깨끗해야 함 (병합 진행 중 아님)
        assert!(!root.join(".git").join("MERGE_HEAD").exists(), "병합이 abort되어야 함");
        // A의 내용은 그대로 병합되어 있어야 함
        assert!(std::fs::read_to_string(root.join("shared.txt")).unwrap().contains("from A"));

        wt_a.remove();
        wt_b.remove();
    }

    #[test]
    fn integrate_parallel_merges_then_conflict_and_cleans_up() {
        // 병렬 통합 헬퍼: 커밋→병합→정리 순서. (구버전처럼 remove가 먼저면 병합이 실패해 이 테스트가 잡음)
        let tmp = init_repo();
        let root = tmp.path();

        let wt_a = Worktree::create(root, "M23-T10").expect("wtA");
        let branch_a = wt_a.branch.clone();
        std::fs::write(wt_a.path.join("shared.txt"), "from A\n").unwrap();

        let wt_b = Worktree::create(root, "M23-T11").expect("wtB");
        let branch_b = wt_b.branch.clone();
        std::fs::write(wt_b.path.join("shared.txt"), "from B\n").unwrap();

        // A 통합 → Merged, worktree·브랜치 정리됨
        assert_eq!(
            integrate_parallel(wt_a, root, "[M23-T10] A").unwrap(),
            MergeOutcome::Merged
        );
        assert!(root.join("shared.txt").exists(), "A 병합 결과가 main에 있어야 함");
        assert!(run_git(root, &["branch", "--list", &branch_a]).stdout.trim().is_empty(), "A 브랜치 정리");

        // B 통합 → Conflicted(같은 파일 add/add), 정리됨
        assert_eq!(
            integrate_parallel(wt_b, root, "[M23-T11] B").unwrap(),
            MergeOutcome::Conflicted
        );
        assert!(run_git(root, &["branch", "--list", &branch_b]).stdout.trim().is_empty(), "B 브랜치 정리");
        assert!(!root.join(".git").join("MERGE_HEAD").exists(), "충돌 abort됨");
        assert!(std::fs::read_to_string(root.join("shared.txt")).unwrap().contains("from A"), "A 내용 유지");
    }
}
