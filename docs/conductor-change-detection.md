# conductor 변경 감지 (M26)

conductor는 에이전트가 격리 worktree에서 한 작업을 **diff로 포착**해 검증자에게 넘긴다.
diff가 비어 있으면 "작업 미수행"으로 간주해 FAIL 처리한다(빈-diff 가드).

## 버그 (M25 라이브 검증에서 발견)

worktree는 생성 시 `HEAD`에서 분기한다. 과거 `Worktree::capture_diff`는
`git add -A` 후 `git diff --cached`(= index vs **현재 HEAD**)를 반환했다.

에이전트가 worktree 안에서 **자기 작업을 커밋**하면:
- HEAD가 그 커밋으로 이동하고 작업트리가 clean이 된다.
- `git add -A`는 스테이징할 것이 없고, `git diff --cached`는 **빈 값**이 된다.
- 빈 diff → 가드가 "에이전트가 어떤 파일도 변경하지 않았습니다"로 FAIL.

→ **에이전트의 커밋 여부에 따라 정상 작업이 폐기되는 비결정적 신뢰성 버그.**
M25 report-live 검증에서 M1-T02(올바른 `tests/b.rs`)가 3회 FAIL 후 halt 되어 드러났다.

## 수정 (M26)

diff를 현재 HEAD가 아니라 **분기 base 커밋 기준**으로 계산한다.

- `Worktree::create`가 분기 직후 `git rev-parse HEAD`로 **base 커밋 SHA를 기록**한다(`base_commit`).
- `capture_diff`는 `git add -A` 후 `git diff --cached <base_commit>`를 반환한다.
  - 커밋·미커밋·미추적 변경이 **모두 base 대비로 포착**된다(에이전트 커밋 여부 무관).
  - base가 비어 있는 예외 상황에서는 기존 동작(index vs HEAD)으로 폴백한다.
- 빈-diff 가드 자체는 유지된다(진짜로 아무 작업이 없으면 여전히 빈 diff → FAIL).
- 순차·병렬 경로 모두 동일한 `capture_diff`를 쓰므로 함께 고쳐진다.

## 검증

- 단위 회귀 테스트(`src/conductor/dispatch.rs`):
  - `capture_diff_sees_committed_work` — 커밋 후에도 변경 포착(핵심 회귀)
  - `capture_diff_empty_when_no_work` — 변경 없으면 빈 diff(가드 유지)
- 재현 하니스(`scripts/conductor-commit-detect-validate.ps1`): 순수 git 레벨에서
  OLD(HEAD 기준) = 빈 값 / NEW(base 기준) = 변경 포착 을 대조 증명. claude 불필요.

```powershell
pwsh scripts/conductor-commit-detect-validate.ps1
```
