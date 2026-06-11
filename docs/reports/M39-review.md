# M39 리뷰보고서 (review)

## 비판점

### 차단 (0건)

릴리즈를 막는 이슈 없음. DoD 7항목 충족, 보안 무회귀(설정 화이트리스트·Origin·스코프 상속),
무한 루프 방지(파킹 + dispatch-err 카운트), `park_on_halt=false`로 구 동작 보존.

### 권장 (2건)

1. **`park_on_halt` 기본 true = 런 종료 동작 변경 (비차단)** — 정지(halt) 시 이전엔 런이 끝났으나
   이제 함대가 계속 진행하고 정지 task를 파킹한다. gate 모드에서 "정지하면 멈춰서 개입"을 기대하던
   운영자에겐 흐름이 달라진다. 완화: ① 대시보드가 파킹을 "정지·재투입 대기"로 노출 + [재투입]·
   [정지] 가능, ② `park_on_halt=false`로 구 동작 즉시 복원, ③ 하위호환(설정 기본만 바뀜, 데이터·API
   무변경). 의도된 개선(더 자율적)이며 문서화됨 — 릴리즈 노트에 동작 변경을 분명히 적을 것.

2. **end-to-end 파킹·핫-재큐·LLM 분할 미실측** — 파킹 상태기계·`revive_parked`·`insert_subtasks`·
   `lock_verdict`는 단위/합성으로 덮였으나, "정지→파킹→다른 task 계속→재투입→un-park→재시도"의
   라이브 1회는 미수행. 런처 detach(M37)와 달리 파킹은 **실제 dispatch 실패**가 있어야 발동해
   비용·비결정성이 커서 자동 review-게이트에 부적합 — 운영자 라이브(실 conductor 런)로 권장.
   릴리즈를 막지 않음(로직이 순수 분리·테스트됨).

### 사소 (4건)

3. **replan이 diff 미사용** — 피드백 + project.md 컨텍스트만으로 분할 제안(impl 스코프 컷). 분할
   품질이 낮으면 diff 첨부가 후속(Halted에 diff 전달 plumbing 필요).
4. **`auto_replan` LLM 호출이 무게이트·무계측** — opt-in이지만 정지 시 자동 LLM 호출이 게이트 없이
   발동하고, `run_with_prompt_str` 경로라 비용이 `total_cost`/budget에 계측되지 않는다. opt-in이라
   허용 가능하나, 비용 관측은 B 묶음(검증자 비용 계측)과 함께 후속.
5. **`insert_subtasks`가 하위 task를 파일 끝에 추가** — 부모 근처가 아닌 project.md 끝(다중 마일스톤
   섹션이면 시각적으로 다른 섹션 아래). 파서는 평면이라 스케줄링·기능 무영향, 가독성 차원의 사소점.
6. **`parked`는 in-memory(런 한정)** — 런 재시작 시 파킹 상태 소실 → 파킹 task는 다시 pending으로
   기본 예산 재시도(halt 힌트 파일은 보존). 재시작 = 새 시도로 합당. per-run 의미 명시.

## 수정 내용

- 리뷰 중 코드 수정 없음(차단·권장 모두 비-수정 — 동작 정확·문서화로 충분).
- 참고: impl 중 **병렬 종료 안전성** 결함을 선제 수정 — dispatch 오류 task가 attempts에 누적되지
  않아 파킹 무한 재시도 가능 → 루프 헤더를 `batch.iter().zip(...)`로 바꿔 오류도 카운트.

## 검증

- `cargo build` 경고 0, `cargo clippy` 경고 **0**, `cargo test` **411 passed / 0 failed**(M39 신규 8개).
- 핵심 로직 단위/합성: `revive_parked`(오버라이드 있는 파킹만 un-park·비소비), `replan::insert_subtasks`
  (부모 `[분할→]` 치환·하위 `-S` 순차 deps 체인·파서 왕복·부모 부재 폴백), `parse_subtasks`(JSON 추출·
  4개 클램프·2개 미만/빈 항목 폐기), `is_replannable`(깊이 1), `lock_verdict`(dead-PID/선점/신선도
  3분기 **shell-out 없이**), 설정 기본/오버라이드.
- HTTP 하니스 `dashboard-launch-validate.ps1`: **PASS** — M37~M38 전체 + M39 설정 키
  (park_on_halt 기본 true·auto_replan 기본 false·POST 갱신·비-bool 400).
- **잔여 리스크**: end-to-end 파킹/핫-재큐/LLM 분할(권장2)은 운영자 라이브 — 실 conductor 런 필요.

## 릴리즈 판정

**가능** — 추천 버전: **v0.36.0 (minor)**

- DoD 7항목 충족: 정지 파킹·함대 계속·핫-재큐 revive·옵트인 적응 재계획·lock 순수 분리·대시보드
  노출·단위/합성 하니스.
- **하위호환**: 신규 설정(`park_on_halt`·`auto_replan`)·`-S` 하위 task·UI는 가산적. `park_on_halt=false`로
  구 "정지 시 종료" 복원, `auto_replan` 기본 off. live.json·control·API 스키마 무변경 → **minor**.
- 차단 0, 권장 2건 비차단(동작 변경은 문서/옵트아웃으로 완화, 라이브는 운영자), 사소 4건 문서화.

## 다음 단계

- **`/tide:release v0.36.0`** (버전 범프 → CHANGELOG/README → commit → tag → push). 릴리즈 노트에
  **park_on_halt 기본 동작 변경**을 분명히 명시할 것.
- 릴리즈 후 권장 후속:
  - 운영자 라이브: 정지 task 파킹→다른 task 계속→[재투입] 핫-재큐→재시도, `auto_replan` 분할 1회.
  - 차기(M40, B 묶음): 검증자 비용 계측(replan/verifier 비용 가시화) + 비용 기반 모델 라우팅.
  - replan diff 첨부, `insert_subtasks` 부모 인접 삽입(가독성).
