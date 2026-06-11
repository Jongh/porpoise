# 로컬 웹 대시보드 (M30)

`porpoise dashboard` — conductor 데이터(실행 리포트·비용·의존성 그래프)를 로컬 웹에서 본다.
**read-only 관측 전용**: `.porpoise/`에 쓰지 않고 conductor 실행 로직도 건드리지 않는다.

## 실행

```bash
porpoise dashboard                 # localhost:7878, 브라우저 자동 오픈
porpoise dashboard --port 9000     # 포트 지정
porpoise dashboard --no-open       # 브라우저 안 열고 URL만 출력 (헤드리스/원격)
```
Ctrl-C로 종료.

## 화면

- **롤업 카드**: 태스크 수·성공률·PASS/FAIL·재투입·폴백·총비용
- **비용 차트**: 태스크별 비용(USD) 막대 (PASS 녹색 / FAIL 빨강)
- **실행 리포트 표**: 태스크별 verdict·시도·재투입·폴백·비용
- **의존성 그래프**: 의존 깊이로 열 배치한 DAG. 노드 색 = 상태(완료/실행가능/대기)

마일스톤 셀렉터로 대상을 바꾸고 `↻`로 새로고침한다(폴링; 라이브 SSE는 후속 Phase).

## 기술

- 서버: `tiny_http` (로컬 바인딩 `127.0.0.1`)
- 프론트엔드: 바이너리에 임베드된 vanilla HTML/JS/CSS + 자체 경량 SVG 차트 lib (CDN 의존 0, 오프라인)
- 브라우저 오픈: `webbrowser` 크레이트

## JSON API (read-only)

| 엔드포인트 | 설명 |
|---|---|
| `GET /api/milestones` | `{milestones:[{number,title}]}` (최신 우선) |
| `GET /api/report?milestone=N` | 롤업(성공률·총비용·총토큰 등) + 태스크별 요약. 미지정 시 최신 |
| `GET /api/tasks` | 현재(project.md) 태스크 + 의존성 + 상태(ready/waiting/done) |

`report::build_report`·`schedule::ready_tasks`·`parse_tasks_from_project_md`를 재사용해 직렬화만 한다.

## 검증

`scripts/dashboard-smoke.ps1`: 합성 데이터 주입 → `--no-open`으로 서버 기동 → `/api/*`를
HTTP로 호출해 응답 JSON을 대조(브라우저 불필요).

## 라이브 패널 (M31)

대시보드 상단의 **라이브 패널**이 진행 중인 conductor 실행을 실시간으로 보여준다:
- **RUNNING/IDLE 배지** + 모드(sequential/parallel)
- task별 현재 단계(brief→dispatch→verify→integrate 진행 배지, MERGED/HALTED 최종 표시)·재투입 횟수
- **누적 비용 / 예산 진행 바**(`budget_usd` 설정 시 — 초과면 빨강)
- 실행 종료(RUNNING→IDLE) 시 리포트·DAG 자동 새로고침. idle엔 마지막 실행 요약 표시

### 동작 구조 (프로세스 결합 없음)
```
conductor ─쓰기→ .porpoise/live.json ←변화 감지(500ms 폴링)─ dashboard ─SSE push→ 브라우저
```
- conductor가 단계 전환마다 `live.json`(스키마 live-1)을 **원자적**(temp→rename)으로 갱신.
  기록 실패는 실행에 영향 없음. conductor는 대시보드의 존재를 모른다.
- `GET /api/events` (SSE): 연결 직후 현재 상태 1회 push, 이후 변화 시마다 push, 10초 keep-alive.
  요청별 스레드로 처리되어 장수명 연결이 다른 요청을 블록하지 않는다.
- `GET /api/live` (단발): SSE 실패 시 프론트가 2초 폴링으로 폴백.
- 병렬 모드는 **배치 수준** 기록(배치 전체 dispatch → 통합 시 task별 merged) — 배치 내 개별
  단계 전환은 스레드 경쟁을 피하기 위해 기록하지 않는다(Phase 2 한계).

### live.json (live-1)
```json
{
  "schema_version": "live-1", "run_active": true,
  "started_at": "...", "updated_at": "...", "mode": "sequential",
  "total_cost_usd": 0.12, "budget_usd": 1.0,
  "tasks": [{ "task_id": "M1-T01", "phase": "verify", "redispatch": 0 }]
}
```

### 검증
`scripts/dashboard-live-validate.ps1`: 대시보드 기동 → live.json을 시나리오로 재생
(시작→dispatch→verify→merged→종료) → SSE 스트림을 구독해 이벤트 수신·내용을 대조
(claude 불필요).

## 멀티 프로젝트 (M32)

대시보드에서 **여러 porpoise 프로젝트를 셀렉터로 전환**하며 관제한다. read-only 유지.

### 레지스트리 (허용 목록)
- 위치: `~/.porpoise/registry.json` — 항목 `{id, name, path}` (id = 경로의 안정 해시)
- `porpoise dashboard` 실행 시 현재 프로젝트가 **자동 등록**된다.
- 명시 관리: `porpoise dashboard --register <path>` / `--unregister <path>` (실행 없이 등록만)

### 보안 모델
- 클라이언트는 경로가 아니라 **불투명 id**(`?project=<id>`)로만 프로젝트를 참조한다.
- 서버는 **레지스트리에 등록된 경로만** 해석한다 — 미등록 id·`.porpoise` 소멸 경로는 404.
  (자유 경로 입력에 의한 임의 파일시스템 열람 차단)
- 후속 제어 UI(M33)는 이 스코프·허용 목록 모델을 그대로 상속한다.

### API
- `GET /api/projects` → `{projects:[{id,name,path,current}]}` (current = 기동 디렉터리)
- 모든 데이터 API(+SSE `/api/events`)가 `?project=<id>`를 받는다.
  **미지정 시 기동 디렉터리**(기존 동작, 하위호환).

### UI
- 헤더의 프로젝트 셀렉터(등록 2개 이상일 때만 표시 — 1개면 기존 화면과 동일).
- 전환 시 마일스톤 목록·리포트·DAG가 새 스코프로 갱신되고, **라이브 SSE 스트림을 닫고
  새 프로젝트로 재구독**한다.

### 검증
`scripts/dashboard-multi-validate.ps1`: 임시 프로젝트 2개(서로 다른 합성 데이터)를 등록 →
같은 엔드포인트를 project id만 바꿔 호출해 서로 다른 ground truth 반환을 대조, 미등록 id
404, 미지정 하위호환 확인. 종료 시 임시 항목을 레지스트리에서 해제(원복).

## 게이트 제어 (M33)

콘솔 승인 게이트를 대시보드 버튼으로 처리한다. `workspace.toml`:
```toml
[conductor]
approval_mode = "gate"   # 기본 "console" — 미설정 시 기존 터미널 프롬프트
```
- gate 모드에서 conductor는 task/배치 승인 시점에 `live.json`에 `pending_gate {id, prompt}`를
  올리고 `.porpoise/control/gate-<id>.json` 응답을 폴링한다(1초). 라이브 패널에 **승인 대기
  카드**([승인]/[정지])가 떠오르고, 클릭하면 응답 파일이 작성되어 conductor가 소비(삭제)한다.
- **graceful stop**: 실행 중(RUNNING) 상시 표시되는 **[다음 게이트에서 정지]** 버튼이
  `control/stop-next.json`을 만든다 — 진행 중 task를 마치고 다음 게이트에서 자동 정지.
- `--yes`는 gate 모드에서도 자동 승인(자동화·CI 우선). 콘솔 모드 동작은 무변경.

### 제어 채널 보안 (read-only의 의도된 첫 이탈)
- 쓰기 범위는 해당 프로젝트의 **`.porpoise/control/`** 게이트 응답 파일로 한정 — 코드·설정·
  project.md는 일절 쓰지 않는다.
- `POST /api/control` `{gate_id?, decision: approve|stop}` — M32 허용 목록·`?project=` 스코프
  상속(미등록 404), gate_id는 영숫자·하이픈만(경로 주입 차단), **Origin 검증**(localhost 외
  브라우저 cross-origin POST 403 — CSRF 차단).

### 검증
`scripts/dashboard-gate-validate.ps1`: 제어 API 검증(승인 파일 작성·Origin 403·주입 400·
미등록 404) + 게이트 왕복(가짜 conductor 게이트 → API 응답 → 소비 확인). claude 불필요.

## 계획 제어 + 게이트 UX (M34)

- **정지 예약 가시화**: [다음 게이트에서 정지]를 누르면 live 페이로드의 `stop_pending`이
  true가 되어(SSE push) 버튼이 **"⏹ 정지 예약됨"**으로 전환된다 — 서버 진실(`control/
  stop-next.json` 존재)이라 모든 브라우저 창이 일관. 게이트에서 소비되면 자연 해제.
- **게이트 종류(kind)**: `confirm`(승인/정지 — 기존) | `text`(자유 텍스트 + [전송]) |
  `confirm_text`(승인 + 선택 텍스트). 응답에 `text` 필드(4KB 제한·제어문자 거부).
- **계획·릴리즈 게이트화**: gate 모드에서 모든 task 완료 후 —
  - "새 마일스톤을 생성하시겠습니까?" → confirm 게이트
  - "신규 릴리즈 태그 (비워두면 건너뜀)" → **text 게이트** (빈 텍스트·정지 = 건너뜀)
  - 즉 gate 모드 한 사이클(승인→실행→마일스톤→릴리즈)이 **터미널 입력 없이** 돈다.
  console 모드·`--yes`·레거시 경로는 무변경.

## 통합 실행 — 대시보드 내장 기동 (M35)

**gate 모드로 `porpoise`를 실행하면 대시보드가 자동으로 함께 기동**된다(같은 프로세스의
백그라운드 스레드, 포트 7878) — 터미널 하나로 게이트 운영 전체가 시작된다. 브라우저도
자동으로 열린다. conductor 종료 시 함께 닫힌다.

```toml
[conductor]
approval_mode = "gate"     # gate 모드면 내장 기동이 기본
serve_dashboard = false    # 끄기 (또는 true로 console 모드에서도 기동)
```
- 이미 `porpoise dashboard`가 떠 있으면(포트 사용 중) 에러 없이 **기존 대시보드와 공존** —
  안내 후 브라우저만 연다.
- 기동 실패는 경고만 남기고 실행은 계속된다(대시보드는 부가 기능).
- 같은 프로세스여도 통신은 파일 매개(live.json·control/) 그대로 — 무결합 설계 불변.

### 레지스트리 위생
`/api/projects`(셀렉터)는 **실존하는 프로젝트만** 노출한다 — 삭제된 프로젝트의 stale 항목은
숨겨진다(레지스트리 파일은 수정하지 않는 읽기 필터). 영구 제거는 `--unregister <path>`.

### 검증
`scripts/dashboard-embed-validate.ps1`: 내장 기동 HTTP 응답·이중 기동 공존(기존 서버 유지)·
stale 필터를 검증. claude 불필요.

## 태스크 작업 내용 가시화 (M36)

- **라이브 패널**: 각 task에 **작업 제목**이 표시된다 (live.json `LiveTask.title` — 무슨
  작업인지 project.md를 안 봐도 됨).
- **리포트 행 펼침**: 실행 리포트의 행을 클릭하면 상세 패널이 펼쳐진다 — 최신 run의
  **라운드별** verdict·diff 규모·비용, **검증 피드백**(FAIL 사유, 빨간 테두리),
  **에이전트 작업 보고**(dispatch_output). 재투입 task는 라운드별로 구분되어 "왜
  재투입됐는지"가 보인다.
- `GET /api/task?id=M1-T03` — 감사 기록에 이미 저장된 본문의 노출(새 수집 0, read-only),
  본문은 2KB 트렁케이트(전송 절제), 렌더링은 esc() 경유(XSS 방어).

## 잔여 콘솔 게이트화 (M36)

gate 모드에서 마지막 남은 터미널 입력 2곳이 게이트로 처리된다:
- **재실행 마일스톤 세션**: 전부-완료 상태로 `porpoise`를 재실행하면 — 무확인 시작 대신
  confirm 게이트("새 마일스톤을 생성하시겠습니까?"). 게이트 전에 대시보드를 보장(내장 기동
  또는 공존)하므로 무한 대기가 없다. 거부(정지) 시 릴리즈 플로우로.
- **push 재시도**: 릴리즈의 git push 실패 시 재시도 여부가 confirm 게이트로.
console 모드·`--yes`는 기존 동작 유지.

## 런처 — 함대 실행·재투입·설정 편집 (M37)

대시보드가 관측·게이트 제어를 넘어 **실행을 시작·관리**한다. M35까지는 conductor가 대시보드를
내장 기동했으나, 런처는 그 방향을 역전해 **독립 실행 중인 대시보드가 conductor 프로세스를 spawn**한다
(대시보드가 자식 프로세스의 수명을 소유). 실행 백엔드를 제외한 통신은 여전히 파일 매개를 유지한다.

### 함대 실행 — `POST /api/launch`
- 라이브 패널의 **[▶ 함대 실행]** 버튼(런 비활성·대기 게이트 없을 때만 노출) → `porpoise`를
  프로젝트 디렉터리에서 **detached spawn**(stdin=null, stdout/stderr → `.porpoise/launch.log`,
  새 프로세스 그룹). 대시보드를 닫거나 Ctrl-C해도 런은 계속된다.
- **런 락**: live.json `run_active`가 true이거나 신선한(30초 이내) `.porpoise/run.lock`이
  있으면 **409**(이중 기동 차단). run.lock은 spawn~`live::start` 공백을 덮고 시간 기반으로 자가
  만료(stale 락은 무시)한다.
- spawn된 gate 모드 conductor는 자기 대시보드를 `serve_in_background`로 띄우려다 `PortInUse`로
  기존(런처) 대시보드와 **공존**(M35 경로). body `{"yes":true}`면 `--yes`(자동 승인) 전달.

### halt task 재투입 — `POST /api/control {decision:"redispatch", gate_id:<task_id>}`
- `max_redispatch` 소진으로 halt된 task는 incomplete로 남아 다음 실행에서 어차피 재시도되지만,
  같은 한도에서 또 즉시 halt된다. 재투입은 **재투입 예산을 +1 상향**해 이를 막는다.
- 리포트의 **FAIL task 행 [재투입] 버튼** → `.porpoise/control/redispatch-<task_id>.json`(`{extra_budget:1}`)
  기록. conductor가 다음 실행에서 해당 task 처리 직전 **소비(삭제)** 하고 유효 한도를 `base+extra`로
  올리며 halt 힌트도 정리한다. (`cleanup_stale_controls`는 redispatch-*.json을 지우지 않아 살아남는다.)
- 실행 중 함대로의 핫-재큐는 범위 밖 — 재투입은 **다음 [함대 실행]에서 효력**.

### 설정 편집 — `GET/POST /api/config`
- `GET` → 현재 `[conductor]` 편집 가능 값(effective). 설정 패널의 **[편집]** 폼에 채워진다.
- `POST` → **화이트리스트 키만** 검증·저장: `mode`·`approval_mode`·`max_parallel`·`max_redispatch`·
  `serve_dashboard`·`verifier_model`·`verdict_fallback`. 화이트리스트 외 키·범위 위반·잘못된
  열거값은 **400**이며 **아무것도 쓰지 않는다**(원자성). workspace.toml의 다른 섹션·값은 보존된다
  (주석은 toml round-trip 특성상 보존되지 않음 — 알려진 트레이드오프).
- **경계 확장**: M33은 제어 쓰기를 `control/`로 한정했으나(설정·코드 쓰기 금지), 이 엔드포인트만
  `[conductor]` 설정 쓰기를 **의도적으로** 허용한다(코드·project.md는 여전히 불가). 쓰기는
  `utils::fs::write_file`(루트 경계) 경유.

### 보안
신규 쓰기 엔드포인트(`/api/launch`·`/api/config`·`redispatch` control) 모두 **Origin 검증(403)**·
**프로젝트 스코프(404)** 를 상속한다. 설정 쓰기는 `[conductor]` 화이트리스트로 한정(임의 TOML 주입 불가).

### 검증
`scripts/dashboard-launch-validate.ps1`: 런 락 409·Origin 403·미등록 404, 재투입 오버라이드 작성,
설정 GET/POST 왕복(화이트리스트 위반 거부·타 섹션 보존)을 HTTP 수준에서 검증. claude 불필요.
실제 detached spawn([함대 실행] 성공 경로)은 실제 conductor 런을 띄우므로 **운영자 라이브 검증** 항목.
