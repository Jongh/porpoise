# M37 완료보고서 (impl)

## 개요

M37 "런처"를 구현했다 — 대시보드가 관측·게이트 제어를 넘어 **실행을 시작·관리**하는 통제실이
된다. 세 기능을 추가했다: ① **[함대 실행]** 버튼으로 conductor 런을 detached spawn(대시보드가
프로세스 수명 소유), ② max_redispatch 소진으로 **halt된 task의 한 번 클릭 재투입**(다음 실행에서
예산 상향), ③ **`[conductor]` 핵심 설정의 대시보드 편집**. M33부터 이월돼 온 "conductor 프로세스
수명 소유" 후속을 실현했다. 5개 태스크(T01·T03 독립 → T02 → T04 → T05) 전부 완료.

## 태스크별 수행 내용

- **M37-T01 (실행 백엔드)** — `src/dashboard/launch.rs` 신규. `POST /api/launch` →
  `std::env::current_exe()`로 `porpoise`를 프로젝트 디렉터리에서 **detached spawn**
  (stdin=null, stdout/stderr→`.porpoise/launch.log`). 플랫폼 분리: Windows는
  `creation_flags(CREATE_NEW_PROCESS_GROUP|CREATE_NO_WINDOW)`, Unix는 `process_group(0)`
  (libc 의존 없이 새 프로세스 그룹). **런 락**은 live.json `run_active` OR 신선한
  `.porpoise/run.lock`(30초 시간 기반 자가 만료 — spawn~`live::start` 공백 보호) → 둘 중
  하나면 409. 설계 결정: run.lock을 자식이 정리하지 않아도 되도록 **시간 기반 만료**를 택해
  cleanup 책임 문제를 회피했다. 순수 판정 함수(`parse_yes`·`parse_lock_time`·`lock_is_fresh`)
  분리로 spawn 없이 가드 로직을 단위 테스트.

- **M37-T03 (설정 편집 백엔드)** — `src/dashboard/config_edit.rs` 신규. `GET /api/config`는
  `WorkspaceConfig` getter로 effective `[conductor]` 값을 노출. `POST /api/config`는
  **화이트리스트 7키**(mode·approval_mode·max_parallel·max_redispatch·serve_dashboard·
  verifier_model·verdict_fallback)만 필드별 검증(열거·범위·타입·텍스트 안전성). 쓰기는
  기존 workspace.toml을 `toml::Table`로 파싱→`[conductor]` 키만 병합→재직렬화해 **타 섹션
  보존**, `utils::fs::write_file`(루트 경계) 경유. 설계 결정: M33의 "control/ 한정·설정 쓰기
  금지" 경계를 **의도적·제한적으로 확장**(주석·문서에 사유 명시). 검증 실패 시 무쓰기(원자성).
  순수 함수(`validate_config_update`·`apply_updates`)로 round-trip·보존을 테스트.

- **M37-T02 (halt 재투입)** — `src/conductor/redispatch.rs` 신규 + `control.rs` 확장.
  `handle_control`의 decision에 `"redispatch"` 추가 → `gate_id`(task id) 검증 후
  `.porpoise/control/redispatch-<task_id>.json`(`{extra_budget:1}`) 작성. conductor는
  순차 루프(`mod.rs`)·병렬 루프(`parallel.rs`)에서 task 처리 직전 `consume_override`로
  **소비(삭제)** 하고 유효 한도를 `base+extra`(상한 20 클램프)로 상향, halt 힌트도 정리.
  설계 결정: `cleanup_stale_controls`가 `redispatch-*.json`을 지우지 않으므로 실행 시작 청소를
  넘어 해당 task 도달 시점까지 살아남는다 — "다음 실행에서 효력" 의미와 일치. 실행 중 함대로의
  핫-재큐는 스케줄러 복잡도로 범위 밖(문서에 명시).

- **M37-T04 (런처 UI)** — `static/{index.html,app.js,style.css}`. 라이브 패널에 **[▶ 함대 실행]**
  버튼(런 비활성·대기 게이트 없을 때만 노출), 리포트 **FAIL 행에 [재투입]** 버튼(행 펼침과
  이벤트 분리), **설정 편집 폼**(GET 로드→POST 저장, 검증 오류 표시). 모든 표시 텍스트 `esc()`
  경유(M30 XSS 규약). 409/403/404·검증 오류는 토스트로 안내.

- **M37-T05 (문서 + 하니스)** — `docs/dashboard.md`에 런처 3기능·런 락·detached 수명·설정 쓰기
  경계 확장·보안 문서화. `scripts/dashboard-launch-validate.ps1` 신규 — 런 락 409·Origin 403·
  미등록 404, 재투입 오버라이드 작성, 설정 GET/POST 왕복(화이트리스트 위반 거부·타 섹션 보존)을
  HTTP 수준에서 검증(claude 불필요). 실제 detached spawn은 운영자 라이브 검증 항목으로 명시.

## 변경 파일 요약

| 구분 | 파일 |
|---|---|
| 추가 | `src/dashboard/launch.rs`, `src/dashboard/config_edit.rs`, `src/conductor/redispatch.rs`, `scripts/dashboard-launch-validate.ps1` |
| 수정 | `src/dashboard/mod.rs`(모듈 등록·`/api/config` GET·`/api/launch`·`/api/config` POST 라우팅), `src/dashboard/control.rs`(`redispatch` decision), `src/conductor/mod.rs`(모듈 등록·순차 루프 오버라이드 소비), `src/conductor/parallel.rs`(병렬 루프 오버라이드 소비·한도 상향), `src/dashboard/static/{index.html,app.js,style.css}`, `docs/dashboard.md` |
| 삭제 | 없음 |

## 테스트 결과

- `cargo build`: 경고 0개. `cargo clippy`: 신규 경고 0개.
- `cargo test`: **396 passed; 0 failed** (M37 신규 25개 포함 — redispatch 4, launch 8,
  config_edit 7, control redispatch 2, 기타). 베이스라인 대비 신규 테스트가 모두 통과.
- HTTP 하니스 `scripts/dashboard-launch-validate.ps1`: **PASS** — 재투입 오버라이드 작성·
  경로주입 400·Origin 403, 런 락 409·Origin 403·미등록 404, 설정 GET·POST 갱신·타 섹션 보존·
  화이트리스트 위반 400(무쓰기)·범위/열거 위반 400을 실제 서버에서 검증.

구현 중 발견·수정: 처음 CSS에서 텍스트 색 변수를 `--fg`로 썼으나 코드베이스 변수는 `--ink` —
일괄 치환으로 수정.

## 미해결·후속 메모

1. **detached spawn 실 검증 미포함** — 하니스는 spawn 성공 경로를 띄우지 않는다(실제 conductor
   런·claude 필요). 운영자 라이브 검증 항목으로 남김. 리뷰에서 Windows/Unix detach 동작의 실제
   수명 분리(부모 종료 후 자식 생존)를 한 번 확인 권장.
2. **run.lock 정리** — 시간 기반 만료(30초)로 자가 치유하나, 비정상 종료 직후 30초간은 새 실행이
   409로 막힌다(보수적 의도). "강제 실행" 옵션은 후속으로 미룸(마일스톤 명시).
3. **설정 편집 주석 손실** — toml round-trip이 workspace.toml의 주석을 보존하지 않는다(데이터는
   보존). 사용자 작성 주석이 많은 파일에서 체감될 수 있음 — 필요 시 관리 섹션 분리 방식 검토.
4. **재투입은 다음 실행에서만 효력** — 실행 중 함대로의 핫-재큐 미지원. 운영 흐름상 [재투입]→
   [함대 실행] 2스텝. 추후 단일 버튼 통합 여지.
