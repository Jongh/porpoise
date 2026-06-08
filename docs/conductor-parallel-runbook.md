# conductor 병렬 함대 라이브 검증 런북 (M23)

v0.23.0(병렬 함대, opt-in)의 병렬 경로를 실제 `claude`로 검증한다. 병렬은 기본 비활성
(`max_parallel=1`)이므로 릴리즈 차단은 아니지만, 사용 전에 두 시나리오(P1 독립·P2 충돌)를
확인하면 신뢰할 수 있다.

라이브 실행은 **토큰을 N배** 쓴다(동시 N개 claude). 사용자가 관찰하며 수행한다.

## 사전 조건
- `claude` CLI·`cargo`·`git` 사용 가능
- 작업 디렉토리: `D:\Code\private\porpoise` (하니스가 현재 소스로 바이너리 재빌드)

## porpoise 프롬프트 응답 (공통)
배치 지휘? → **y** / 새 마일스톤? → **n** / 릴리즈 태그? → **Enter**(빈값)

---

## 시나리오 P1 — 독립 task 병렬 (충돌 없음) 【핵심】

목적: 독립 task 3개(서로 다른 `tests/*.rs` 파일 생성)를 **동시에** dispatch·verify하고 충돌 없이
순차 통합·완료하는지 확인.

```powershell
cd D:\Code\private\porpoise
pwsh scripts\conductor-parallel-validate.ps1 -MaxParallel 3
```

하니스가 자동 스캐폴딩 → porpoise 1회 실행(배치 1개에 3 task) → 상세 측정.

### 확인 항목 (콘솔/로그)
- **병렬 실행 표시**: `[ 배치 3개 ]`, `3개 task 동시 dispatch·verify 중...`
- **그룹 출력**: 각 task 결과가 `▸ [M1-T0x] PASS — diff N줄 ...`로 task별 식별 표시(인터리브 없음)
- **감사 기록**: task당 1개씩 3개, 전부 `verdict=PASS`, `fallback_used=False`
- **커밋**: `[M1-T01]`·`[M1-T02]`·`[M1-T03]` 3개 (init 포함 4커밋)
- **task 완료**: project.md 완료=3, 미완료=0
- **잔여 정리**: worktree=1, porpoise 브랜치=0
- **무결성**: `tests/calc_add.rs`·`calc_sub.rs`·`calc_mul.rs` 모두 존재 + 최종 `cargo test` 통과

### 합격 기준
요약에서 **판정: PASS** (PASS 감사 3/3, task 완료 3/3, 잔여 0, 최종 테스트 통과).

---

## 시나리오 P2 — 충돌 → 재투입 수렴 (선택, 수동)

목적: 같은 파일을 건드리는 task가 병합 충돌할 때, abort 후 **갱신된 base로 재투입**되어 수렴하는지
확인. (낙관적 동시성의 핵심)

자동 하니스가 없으므로 수동 셋업한다. P1 스캐폴드를 재사용하되, 두 task가 **둘 다 `src/lib.rs`**를
수정하도록 project.md/M1.md를 바꾼다:

```
## 작업 목록
- [ ] M1-T01: src/lib.rs에 add(a, b) 함수와 단위 테스트를 추가
- [ ] M1-T02: src/lib.rs에 sub(a, b) 함수와 단위 테스트를 추가
```
그리고 `workspace.toml`의 `max_parallel = 2`, `max_redispatch = 2`로 두고 `porpoise` 실행.

### 확인 항목
- 첫 배치(2 task 병렬) 후, 한 task는 `병합 완료`, 다른 task는 `↻ 병합 충돌 — 갱신된 base에서 재투입 예정 (1회)`
- **다음 배치**에서 충돌했던 task가 재dispatch되어(이제 첫 task의 변경을 본 상태) `병합 완료`
- 최종: 두 task 모두 완료, 잔여 0, `cargo test` 통과
- 무한 루프·시도 한도 초과 없음

### 합격 기준
충돌한 task가 1~2 라운드 내 재투입으로 병합 완료되고, 최종적으로 전 task 완료·테스트 통과.

---

## 로그
각 실행은 `D:\tmp\conductor-parallel-<timestamp>.log`에 transcript 저장(콘솔 마지막 줄에 경로).

## 결과 보고 양식
```
P1 (독립):  PASS 감사 _/3, 완료 _/3, 잔여 _, 최종테스트 _, 판정 _____
P2 (충돌):  충돌 발생 _, 재투입 수렴 _, 전 task 완료 _, 판정 _____
이상 징후: (없음 / 내용)
```

P1(+가능하면 P2) PASS면 병렬 함대를 신뢰하고 사용할 수 있다.
