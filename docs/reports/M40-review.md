# M40 리뷰보고서 (review)

## 비판점

### 차단 (0건)

릴리즈를 막는 이슈 없음. DoD 6항목 충족, 하위호환(스키마 가산·라우팅 미설정 시 무변경), 보안
무회귀(설정 화이트리스트·text 검증·Origin·스코프 상속).

### 권장 (2건)

1. **`total_cost` 의미 변경 = dispatch + verifier (비차단)** — 그동안 `total_cost`는 dispatch 비용만
   이었으나 이제 검증자 비용을 포함한다. 구 `conductor-4` 레코드는 verifier None → dispatch만(하위호환,
   단위 테스트로 확인)이라 과거 마일스톤 숫자는 불변이지만, **신규 런은 총비용이 검증자 비용만큼
   커 보인다**. 의도된 정확화(그동안 절반만 보이던 비용)이며 대시보드가 dispatch/verifier 분리도
   노출한다. 릴리즈 노트에 "총비용에 검증자 비용 포함"을 명시할 것.

2. **verifier의 metered-output verdict 파싱 동등성 (라이브 확인 권장)** — 검증자 호출을 비메터드
   `run_agentic`(평문) → `run_agentic_metered`(stream-json)로 전환했다. `.output`은 dispatch가 이미
   같은 경로로 성공적으로 쓰는 최종 narration이고, CLI가 stream-json 미지원이면 **평문 폴백 + 비용
   None**으로 graceful 저하하므로 파싱(`try_parse_verdict`)은 양쪽에서 동작할 것으로 본다. 다만
   실 환경에서 1차 verdict 추출이 평문 때와 동일하게 잘 되는지(검증자 false-negative 0)는 claude 런이
   필요 — 운영자 라이브 1회 권장. 릴리즈를 막지 않음(메커니즘 동일·폴백 안전).

### 사소 (3건)

3. **라우팅이 attempt 기반(난이도 사전 추정 아님)** — "저비용 우선·실패 시 승급". fast 모델 성공률이
   낮으면 재투입이 늘어 **총비용이 오히려 증가**할 수 있다(승급 전 fast 시도 + strong 재시도). fast
   모델 선택은 운영자 판단이며, 미설정이 기본(라우팅 off). 예산/태그 기반 라우팅은 후속.
4. **`run_agentic` 제거** — verifier 전환으로 무용이 된 public 메서드를 제거(dead-code 경고 0 유지).
   바이너리 크레이트라 외부 소비자 없음 — 무해. 향후 평문 에이전트 실행이 필요하면 metered로 대체.
5. **실 verifier 비용·실 모델 승급 미실측** — 계측 합산·route_model·report 집계·UI 노출은 단위/합성으로
   덮였으나, 실제 검증자 비용이 0이 아니게 잡히는지·fast→strong 승급이 감사/로그에 보이는지는 운영자
   라이브(실 conductor 런).

## 수정 내용

- 리뷰 중 코드 수정 없음(차단·권장 모두 비-수정 — 동작 정확·하위호환·문서화로 충분).

## 검증

- `cargo build` 경고 0, `cargo clippy` 경고 **0**, `cargo test` **414 passed / 0 failed**(M40 신규 3개).
- 단위: `route_model`(승급 경계·fast 미설정 폴백·strong None), `report_splits_dispatch_and_verifier_cost`
  (분리 집계·total=합), `report_total_cost_backward_compat_no_verifier`(구 레코드 verifier None →
  dispatch만), 감사 conductor-5/verifier_cost 단언.
- HTTP 하니스 `dashboard-launch-validate.ps1`: **PASS** — 합성 conductor-5 → `/api/report`
  total_verifier_cost=0.02·total_cost=0.12·`/api/task` verifier_cost_usd, 라우팅 설정 키 GET/POST.
- **잔여 리스크**: 권장 2(verdict 파싱 동등성)·사소 5(실 비용/승급)는 운영자 라이브 — 실 conductor 런 필요.

## 릴리즈 판정

**가능** — 추천 버전: **v0.37.0 (minor)**

- DoD 6항목 충족: 검증자 비용 계측(conductor-5)·dispatch/verifier 분리·저비용 우선 승급 라우팅·대시보드
  분리 노출·보안 무회귀·단위/합성 하니스.
- **하위호환**: 감사 스키마 가산(구 레코드 verifier None), 라우팅 미설정 시 기존(항상 strong) 동작,
  live.json·control·API는 가산적. `total_cost` 의미만 합으로 정밀화(과거 수치 불변) → **minor**.
- 차단 0, 권장 2건 비차단(의미 변경 문서화·파싱 동등성 라이브), 사소 3건 문서화.

## 다음 단계

- **`/tide:release v0.37.0`** (버전 범프 → CHANGELOG/README → commit → tag → push). 릴리즈 노트에
  **total_cost가 검증자 비용 포함으로 정밀화**됨을 명시.
- 릴리즈 후 권장 후속:
  - 운영자 라이브: 검증자 비용이 0이 아니게 잡히는지, fast→strong 승급(↑모델 승급 로그)·verdict 파싱 정상.
  - retro 두 뿌리(A=M39 halt 회복, B=M40 비용) 모두 해소 — 다음 retro로 누적 후속 재집계 권장.
  - 예산/난이도 기반 라우팅, auto_replan LLM 비용 계측(M39 후속), replan diff 첨부.
