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

## 후속 Phase
- **M32**: 제어 UI — 승인·halt·재투입·마일스톤 편집
