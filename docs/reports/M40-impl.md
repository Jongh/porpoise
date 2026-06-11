# M40 완료보고서 (impl)

## 개요

M40 "비용 관측·라우팅"을 구현했다 — retro 뿌리 B("비용이 절반만 보인다")를 닫는다. 그동안
누락됐던 **검증자(verifier) LLM 비용**을 계측해 dispatch 비용과 분리 집계·노출하고, task를
**저비용 모델로 먼저 시도하되 재투입 시 강한 모델로 승급**하는 라우팅을 추가했다. 4개 태스크
(T01·T02 독립 → T03 → T04) 전부 완료. retro 두 뿌리(A=M39, B=M40)가 모두 해소된다.

## 태스크별 수행 내용

- **M40-T01 (검증자 비용 계측)** — `verify.rs`·`mod.rs`·`parallel.rs`·`report.rs`. `run_verification`의
  비메터드 `run_agentic`(2곳: 1차·재질의)를 **`run_agentic_metered`**로 교체, 비용을 누적
  (`add_cost` 헬퍼)해 `VerifyOutcome.verifier_cost_usd`로 반환. `conduct_in_worktree`·`parallel`이
  task 비용·live 누적에 **dispatch + verifier 합산**. `write_audit_record`에 `verifier_cost_usd`
  추가, 스키마 `conductor-4` → **`conductor-5`**. `report.rs`: `AuditRecord`·`TaskRunSummary`에
  verifier 비용(serde default None), `total_dispatch_cost`·`total_verifier_cost` 추가, **`total_cost`를
  dispatch+verifier 합**으로 재정의. **설계 결정**: `total_cost` 의미를 합으로 바꾸되 구 `conductor-4`
  레코드(verifier None)는 dispatch만 → 하위호환(단위 테스트). 부수: 비메터드 `run_agentic`이 무용
  → 제거(신규 dead-code 경고 0 유지).

- **M40-T02 (비용 기반 모델 라우팅)** — `workspace.rs`·`mod.rs`·`parallel.rs`. `[conductor]
  dispatch_model_fast`·`verifier_model_fast`(빈 값/미설정 None) + getter. 순수 헬퍼
  `route_model(strong, fast, attempt)`: `attempt==0 && fast` → fast, 아니면 strong (단위 테스트:
  승급 경계·fast 미설정 폴백). 순차(`conduct_in_worktree`)는 redispatch별, 병렬
  (`dispatch_batch_parallel`)은 task별 attempts로 dispatch·verifier 모델 라우팅. 미설정이면 항상
  strong(기존 동작). 재투입 1회차에 "↑모델 승급" 안내.

- **M40-T03 (대시보드)** — `api.rs`·`config_edit.rs`·`static/{app.js,index.html}`. `report_json`에
  `total_dispatch_cost`·`total_verifier_cost`, `task_detail_json` 라운드에 `verifier_cost_usd` 추가.
  롤업에 "검증자 비용" 카드, task 상세에 "작업/검증" 비용 분리. 설정 폼·`config_edit` 화이트리스트·
  text 검증·GET에 `dispatch_model_fast`·`verifier_model_fast` 추가.

- **M40-T04 (문서 + 하니스)** — `docs/conductor.md` "비용 관측·라우팅(M40)" 절. `dashboard-launch-validate.ps1`에
  합성 `conductor-5` 감사 레코드 → `/api/report` 분리 집계·`/api/task` verifier 노출, 라우팅 설정 키
  GET/POST 검증 추가. 실 verifier 비용·실 승급은 conductor 런(claude) 필요 → 운영자 라이브 항목.

## 변경 파일 요약

| 구분 | 파일 |
|---|---|
| 추가 | 없음(헬퍼는 기존 모듈에) |
| 수정 | `src/conductor/verify.rs`(메터드·`add_cost`·verifier_cost_usd), `src/conductor/mod.rs`(비용 합산·conductor-5·`route_model`·라우팅 적용), `src/conductor/parallel.rs`(병렬 비용·라우팅), `src/conductor/report.rs`(verifier 비용 집계·총비용 재정의), `src/claude/runner.rs`(dead `run_agentic` 제거), `src/config/workspace.rs`(`*_model_fast`), `src/dashboard/{api.rs,config_edit.rs,static/{app.js,index.html}}`, `docs/conductor.md`, `scripts/dashboard-launch-validate.ps1` |
| 삭제 | 없음 |

## 테스트 결과

- `cargo build` 경고 0개, `cargo clippy` 신규 경고 **0개**, `cargo test` **414 passed / 0 failed**
  (411 → 414, M40 신규 3개: `route_model_fast_first_then_strong`, `report_splits_dispatch_and_verifier_cost`,
  `report_total_cost_backward_compat_no_verifier` + 감사 conductor-5/verifier_cost 단언 보강).
- HTTP 하니스 `dashboard-launch-validate.ps1`: **PASS** — M37~M39 전체 + M40(합성 conductor-5 →
  `/api/report` total_verifier_cost=0.02·total_cost=0.12·`/api/task` verifier_cost_usd, 라우팅 설정 키).

## 미해결·후속 메모

1. **실 verifier 비용·실 모델 승급 미실측** — `run_agentic_metered`로의 전환·라우팅은 실제 conductor
   런(claude)에서만 검증된다. 계측 합산·route_model·report 집계·UI 노출은 단위/합성으로 덮음. 운영자
   라이브 1회 권장(검증자 비용이 0이 아니게 잡히는지, fast→strong 승급이 로그/감사에 보이는지).
2. **`total_cost` 의미 변경** — 이제 dispatch+verifier. 구 레코드는 dispatch만(하위호환)이라 과거
   마일스톤 숫자는 불변이나, 신규 런은 총비용이 (검증자 비용만큼) 커 보인다. 리뷰에서 명시 권장.
3. **라우팅이 attempt 기반(난이도 추정 아님)** — "저비용 우선·실패 시 승급". task 자체 난이도를 사전
   추정하진 않는다(예산/태그 기반 라우팅은 후속). 첫 시도 성공률이 낮은 모델이면 재투입↑로 비용이
   오히려 늘 수 있음 — fast 모델 선택은 운영자 판단.
4. **`run_agentic_metered` 스트리밍 동등성** — verifier가 stream-json 경로로 바뀜. dispatch가 이미
   같은 경로라 동등하나, verifier 출력 파싱(verdict 추출)이 실 환경에서 1차로 잘 되는지 라이브 확인 권장.
