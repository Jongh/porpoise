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

## 후속 Phase
- **M33**: 제어 UI — 승인·halt·재투입·마일스톤 편집 (프로젝트-스코프로 설계)
