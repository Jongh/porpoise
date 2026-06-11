# M39 완료보고서 (impl)

## 개요

M39 "halt 회복 지능"을 구현했다 — task 하나의 정지(halt)가 함대 전체를 멈추지 않는다. 정지 task는
**파킹**되고 나머지 ready task는 계속 진행되며, 대시보드 [재투입]이 도착하면 **같은 런에서
un-park·재시도(핫-재큐)**된다. 옵트인 `auto_replan`이 켜지면 정지 task를 **2~4개 하위 task로 자동
분할**한다. 함께 M38-review 권장1(lock_blocks 부수효과 분리)을 위생 정리했다. 5개 태스크
(T01·T03 독립 → T02 → T04 → T05) 전부 완료.

## 태스크별 수행 내용

- **M39-T01 (핫-재큐 — 파킹 + 함대 계속)** — `conductor/mod.rs`(순차)·`parallel.rs`(병렬)·
  `workspace.rs`. `[conductor] park_on_halt`(기본 true) 추가. 순차: `TaskOutcome::Halted`에서
  `break` 대신 `parked` 집합에 넣고 **계속**; ready 선택에서 parked 제외; 루프 상단에서
  `revive_parked`(오버라이드 있는 파킹 task un-park). 병렬: stuck(attempts>cap) → break를 **park**로
  변경, ready에서 parked 제외, override 도착 시 un-park + attempts 리셋. **설계 결정**: 파킹/revive
  로직을 순수 헬퍼 `conductor::revive_parked`로 분리해 단위 테스트(오버라이드 비소비 — conduct
  직전 consume이 소비). **종료 안전성**: 병렬에서 dispatch 오류도 attempts에 누적하도록 고쳐(루프
  헤더를 `batch.iter().zip(worktrees).zip(runs)`로 변경) 파킹 무한 재시도를 차단.

- **M39-T03 (위생 — lock_blocks 분리)** — `dashboard/launch.rs`. `lock_blocks`를 **순수**
  `lock_verdict(content, now, pid_alive: Option<bool>) -> LockVerdict{Blocked|Stale|DeadPrune}` +
  IO 래퍼로 분리(M38-review 권장1). dead-PID/선점/신선도 3분기를 **shell-out 없이** 단위 테스트
  (`lock_verdict_pure_branches`) — M38은 실 `tasklist`에 의존했음. launch 동작 무변경.

- **M39-T02 (적응형 재계획)** — `conductor/replan.rs`(신규)·`conductor/mod.rs`·`parallel.rs`·
  `workspace.rs`. `[conductor] auto_replan`(기본 false, 옵트인). `propose_subtasks`(LLM —
  project.md를 컨텍스트로 첨부, 검증 피드백 기반 2~4개 제안, `parse_subtasks`로 JSON 배열 추출·
  클램프·2개 미만 폐기), `insert_subtasks`(부모 `- [x] {id}: [분할→S1,S2] {title}` 치환 + 하위
  `{id}-S1`… **순차 deps 체인** 추가, `write_file` 경계), `try_replan`(깊이 1 가드 — `-S` 미재분할).
  파킹 경로에서 replan 성공 시 파킹 생략. **설계 결정(스코프 컷)**: replan은 피드백 + project.md
  컨텍스트만 사용하고 **diff는 쓰지 않음**(conduct_task에서 diff를 빼내는 plumbing 회피 — 피드백이
  실패 신호를 충분히 담음). 마일스톤의 "피드백·diff" 중 diff 생략을 명시.

- **M39-T04 (대시보드)** — `config_edit.rs`·`static/{index.html,app.js}`. `park_on_halt`·`auto_replan`을
  화이트리스트·검증(bool)·GET에 추가하고 설정 폼에 체크박스 2개. 라이브 패널의 "halted" 라벨을
  **"정지·재투입 대기"**로(회복 가능 상태 명시), 기존 [재투입]이 그대로 핫-재큐 트리거.

- **M39-T05 (문서 + 하니스)** — `docs/conductor.md`에 "halt 회복 지능(M39)" 절(파킹·핫-재큐·
  park_on_halt·auto_replan·`-S` 규약·[분할됨]). `dashboard-launch-validate.ps1`에 M39 설정 키
  GET/POST·타입 검사 추가. end-to-end 파킹/분할은 실 conductor 런(claude) 필요 → 운영자 라이브 항목.

## 변경 파일 요약

| 구분 | 파일 |
|---|---|
| 추가 | `src/conductor/replan.rs` |
| 수정 | `src/conductor/mod.rs`(파킹·revive·replan 연동·헬퍼), `src/conductor/parallel.rs`(stuck→park·dispatch-err 카운트), `src/conductor/redispatch.rs`(`has_override`), `src/config/workspace.rs`(`park_on_halt`·`auto_replan`), `src/dashboard/launch.rs`(lock_verdict 분리), `src/dashboard/config_edit.rs`(키 2개), `src/dashboard/static/{index.html,app.js}`, `docs/conductor.md`, `scripts/dashboard-launch-validate.ps1` |
| 삭제 | 없음 |

## 테스트 결과

- `cargo build` 경고 0개, `cargo clippy` 신규 경고 **0개**, `cargo test` **411 passed / 0 failed**
  (403 → 411, M39 신규 8개: `revive_parked`, `replan`(5: is_replannable·parse_subtasks·subtask_ids·
  insert 왕복·insert 폴백), `lock_verdict_pure_branches`, halt-recovery 설정).
- HTTP 하니스 `dashboard-launch-validate.ps1`: **PASS** — M37~M38 전체 + M39 설정 키
  (park_on_halt 기본 true·auto_replan 기본 false·POST 갱신·비-bool 400).

## 미해결·후속 메모

1. **end-to-end 파킹·핫-재큐·LLM 분할 미실측** — 실제 conductor 런(claude·git worktree) 필요. 파킹
   상태기계·revive·replan 삽입·lock 판정은 단위/합성으로 덮었으나, "정지→파킹→다른 task 계속→
   재투입→un-park→재시도"의 라이브 1회 검증은 운영자 몫(리뷰에서 가능하면 1회 권장).
2. **replan이 diff 미사용** — 피드백 + project.md 컨텍스트만 사용. 분할 품질이 낮으면 diff 첨부를
   후속으로(Halted 변형에 diff 추가하는 plumbing 필요).
3. **park_on_halt 기본 true = 동작 변경** — 정지 시 런이 더 진행된다(이전엔 종료). `park_on_halt=false`로
   구 동작 보존. 리뷰에서 하위호환·기대 부합 여부 점검 권장.
4. **병렬 파킹 비용** — park_on_halt에서 task가 cap까지 누적되며 매 배치 재dispatch(LLM 비용). attempts
   cap이 상한이나, 큰 함대에서 비용 관측 권장(B 묶음 검증자 비용 계측과 연결).
