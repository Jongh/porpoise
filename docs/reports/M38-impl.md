# M38 완료보고서 (impl)

## 개요

M38 "런처 마감"을 구현했다 — M37 런처가 release·impl 보고서에 남긴 비차단 후속을 한 묶음으로
닫았다: ① 대시보드 포트 설정화(`[conductor] dashboard_port`, 7878 하드코딩 제거), ② 런 락을
**자식 PID 생존 기반**으로 정밀화 + **강제 실행(force)** 옵션, ③ halt task **재투입+실행 단일
버튼**, ④ 설정 편집의 **주석·서식 보존**(`toml_edit`). Unix `process_group(0)` detach 실측용 이식
하니스도 추가했다. 5개 태스크(T01~T04 독립 → T05) 전부 완료.

## 태스크별 수행 내용

- **M38-T01 (포트 설정화)** — `WorkspaceConductor.dashboard_port: Option<u16>` + getter
  `conductor_dashboard_port()`(기본 7878, [1024,65535] 클램프). 내장 기동 2개 호출부
  (`conductor/mod.rs`·`orchestrator/mod.rs`)의 `7878` 하드코딩을 getter로 교체. `config_edit`
  화이트리스트에 `dashboard_port` 추가(검증 1024..=65535)·GET 노출. 설정 폼에 number 입력.
  `porpoise dashboard --port`는 그대로(우선) — 설정은 내장 기동 기본값.

- **M38-T02 (런 락 정밀화 + force)** — `launch.rs`. `launch_blocked`를 `live_run_active` +
  `lock_blocks`로 분리. `lock_blocks`: 실제 락(pid>0)은 `pid_alive`로 **살아있을 때만 차단**,
  죽었으면 무시+죽은 락 정리(M37 시간 quirk 해소); 선점 락(pid=0)만 시간 신선도 fallback.
  `pid_alive`는 플랫폼 격리(Windows `tasklist`, Unix `kill -0`)·의존성 없이 shell-out·conductor
  무결합. `handle_launch`: `run_active`는 spawn 전 무조건 409(force 무관), 락 단독 차단만
  `force`(body `{"force":true}`)로 우회. 409 시 UI [강제 실행] 버튼 노출. **설계 결정**:
  run_active와 lock 차단을 분리해 "진짜 동시 실행은 force로도 못 뚫게" 했다.

- **M38-T03 (재투입+실행 단일 버튼)** — `app.js`(프런트엔드만, 백엔드 0). `lastRunActive`를
  `renderLive`에서 추적. `redispatchTask`: 오버라이드 기록 성공 후 **런 비활성이면 곧바로
  `launchFleet()`** 체이닝("재투입 후 함대 실행"), 활성이면 끼어들지 않고 "다음 실행 적용"(M37 범위
  유지). `launchFleet(force)`로 일반화 — 409 시 [강제 실행] 노출, 반환값으로 체이닝 성공 판단.

- **M38-T04 (설정 주석 보존)** — `Cargo.toml`에 `toml_edit = "0.22"`. `apply_updates`를
  `toml::Table` round-trip → `toml_edit::DocumentMut`로 교체. **설계 결정/발견**: 처음엔
  `table.insert`로 키를 갱신했으나 기존 키의 **prefix 주석이 소실**(insert가 키 decor를 리셋)
  — `get_mut`으로 **값만 in-place 교체**해 키 주석까지 보존하도록 수정(테스트로 포착). 검증·
  화이트리스트·원자성은 무변경.

- **M38-T05 (문서 + 하니스)** — `docs/dashboard.md` "런처 마감(M38)" 절 추가. HTTP 하니스
  `dashboard-launch-validate.ps1` 확장(+5항목: port GET/POST·범위 400, 주석 보존, run_active+force
  →409, 선점 락→409 — 모두 비-spawn). Unix 라이브 하니스 `dashboard-launch-live.sh` 신규
  (`process_group(0)` detach 생존 + 종료 후 즉시 재실행 200 검증).

## 변경 파일 요약

| 구분 | 파일 |
|---|---|
| 추가 | `scripts/dashboard-launch-live.sh` |
| 수정 | `src/config/workspace.rs`(dashboard_port), `src/conductor/mod.rs`·`src/orchestrator/mod.rs`(포트 사용), `src/dashboard/launch.rs`(PID 락·force), `src/dashboard/config_edit.rs`(포트 화이트리스트·toml_edit apply), `src/dashboard/static/{index.html,app.js}`(포트 입력·강제 실행·재투입+실행), `Cargo.toml`/`Cargo.lock`(toml_edit), `docs/dashboard.md`, `scripts/dashboard-launch-validate.ps1` |
| 삭제 | 없음 |

## 테스트 결과

- `cargo build` 경고 0개. `cargo clippy` 신규 경고 **0개**.
- `cargo test`: **403 passed / 0 failed** (M38 신규 ~9개: dashboard_port·preempt/dead-pid 락·
  parse_force·주석 보존·force 분기 등). `live_pid_lock_blocks_dead_pid_does_not`는 실제 `tasklist`
  `pid_alive` 경로를 Windows에서 실행(자기 PID 생존·죽은 PID 999999) — **런 락 핵심 로직 실측**.
- HTTP 하니스 `dashboard-launch-validate.ps1`: **PASS** (M37 전체 + M38 5항목).
- 라이브 하니스 `dashboard-launch-live.ps1`(Windows, 회귀): **PASS** — launch.rs 재작성 후에도
  detached spawn 생존·우아한 정지 무회귀.

구현 중 발견·수정: `toml_edit` 키 갱신 시 `insert`가 prefix 주석을 날리던 문제 → `get_mut` in-place
교체로 수정(테스트 `apply_updates_preserves_comments_and_order`가 포착).

## 미해결·후속 메모

1. **Unix detach 실측 미수행** — `dashboard-launch-live.sh`를 작성했으나 개발 환경이 Windows라
   실행은 운영자(Unix) 몫. Windows detach·dead-PID 즉시 재실행은 라이브/단위로 검증됨.
2. **포트 공존 안내만** — 독립 대시보드 `--port X`와 자식 `dashboard_port Y`가 다르면 두 번째
   대시보드가 뜬다. 코드로 강제하지 않고 문서·`serve_dashboard=false` 권장으로 처리(런처 흐름).
3. **`pid_alive` shell-out 비용** — 매 launch 시도마다 `tasklist`/`kill` 1회. 빈도가 낮아(사용자
   클릭) 무시 가능하나, 고빈도 환경이면 캐시 검토 여지.
4. **force-success·즉시 재실행은 HTTP 하니스 밖** — 둘 다 실제 spawn을 유발해 no-claude 하니스에선
   비-spawn 분기만 검증. spawn 경로는 단위(dead-pid)·`.sh` 라이브로 커버.
