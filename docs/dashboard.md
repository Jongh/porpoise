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

## 후속 Phase
- **M31**: 라이브 스트리밍(SSE) — 진행 중 실행·비용 번다운 실시간
- **M32**: 제어 UI — 승인·halt·재투입·마일스톤 편집
