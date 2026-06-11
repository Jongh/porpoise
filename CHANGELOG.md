# CHANGELOG (Older Releases)

최신 릴리즈는 [README.md](README.md#changelog) 를 참조하세요.

---

### [v0.35.0]
- **런처 마감 — 포트 설정·런 락 정밀화·재투입 단일 버튼·설정 주석 보존 (M38)**: M37 런처가 review·impl에 남긴 비차단 후속을 한 묶음으로 닫는다
- **대시보드 포트 설정화 `[conductor] dashboard_port`**: 내장 기동(M35)·gate 재실행 경로의 포트가 **설정 가능**(기본 7878, [1024, 65535] 클램프). 설정 편집 폼에도 노출. `porpoise dashboard --port`는 별도(우선) — 독립 대시보드 포트는 CLI가 정함. 공존 주의: 독립 대시보드 `--port X`와 spawn된 자식 `dashboard_port Y(≠X)`면 두 번째 대시보드가 뜬다 → 런처 흐름에선 자식 `serve_dashboard = false` 권장(폼에서 설정)
- **런 락 정밀화 + 강제 실행**: run.lock 차단 판정을 시간 기반 → **자식 PID 생존 기반**으로 정밀화. 실제 락(pid>0)은 자식이 **살아있을 때만** 차단, 죽었으면(런 종료/소멸) 무시하고 죽은 락 정리 → 30초 안에 끝난 런 직후에도 **즉시 재실행** 가능(M37 시간 기반 quirk 해소). 선점 락(pid=0, spawn~`live::start` 공백)만 시간 신선도로 fallback. PID 생존 확인은 플랫폼 격리(Windows `tasklist`, Unix `kill -0`)·conductor 무결합. `POST /api/launch {"force":true}`로 **stale 락 우회**(단 live `run_active==true`는 force여도 **409** — 동시 실행 방지), 409 시 UI [강제 실행] 버튼 노출
- **재투입 + 실행 단일 버튼**: 리포트 FAIL 행 [재투입]이 오버라이드 기록 후 **런 비활성이면 곧바로 [함대 실행]**까지 한 번에 수행(프런트엔드 체이닝, 백엔드 무변경). 런 활성 시엔 끼어들지 않고 "다음 실행에서 적용"으로 안내(M37 범위 유지)
- **설정 편집 주석·서식 보존**: 설정 쓰기를 `toml`(round-trip) → **`toml_edit`**로 교체. workspace.toml의 **주석·키 순서·서식을 보존**하면서 `[conductor]` 화이트리스트 키만 갱신한다(기존 키는 값만 in-place 교체해 그 키의 prefix 주석도 유지 — round-trip이 주석을 날리던 문제 해소). 검증·원자성(실패 시 무쓰기)·화이트리스트는 무변경
- **검증**: 단위(포트 기본·클램프, 선점 락 신선도, **실 `tasklist` PID 생존 판정**(죽은 PID 무차단·죽은 락 정리), `parse_force`, **주석 보존**) + HTTP 하니스 확장(`dashboard-launch-validate.ps1`: 포트 GET/POST·범위 400, 주석 보존, run_active+force→409, 선점 락→409) + **Unix 라이브 하니스 신규**(`dashboard-launch-live.sh`: `process_group(0)` detach 생존 + 종료 후 즉시 재실행). Windows 라이브 detach 무회귀 확인
- **테스트**: 403개 (396 → 403, +7개)

### [v0.34.0]
- **런처 — 대시보드에서 함대 실행·halt 재투입·설정 편집 (M37)**: 대시보드가 관측·게이트 제어를 넘어 **실행을 시작·관리**하는 통제실이 된다. M35까지 conductor가 대시보드를 내장 기동했으나, 런처는 방향을 역전해 **독립 실행 중인 대시보드가 conductor 프로세스를 detached spawn**한다(대시보드가 자식 프로세스의 수명 소유 — M33부터 이월된 후속). 실행 백엔드 외 통신은 여전히 파일 매개·무결합
- **함대 실행 `POST /api/launch`**: 라이브 패널 [▶ 함대 실행] 버튼 → `current_exe`로 `porpoise`를 프로젝트 디렉터리에서 detached spawn(stdin=null, stdout/stderr→`.porpoise/launch.log`; Windows `CREATE_NEW_PROCESS_GROUP|CREATE_NO_WINDOW`, Unix `process_group(0)`). **런 락**(live.json `run_active` OR 30초 신선 `.porpoise/run.lock`)으로 이중 기동을 409 차단 — 락은 spawn 전 선점으로 TOCTOU 창을 닫고 시간 기반 자가 만료. spawn된 gate 모드 conductor는 PortInUse로 기존(런처) 대시보드와 공존(M35)
- **halt task 재투입 `POST /api/control {decision:"redispatch"}`**: max_redispatch 소진으로 halt된 task의 리포트 FAIL 행 [재투입] 버튼 → `.porpoise/control/redispatch-<task_id>.json`(`{extra_budget:1}`) 기록. conductor가 다음 실행에서 해당 task 처리 직전 **소비(삭제)** 하고 유효 재투입 한도를 `base+extra`(상한 20)로 상향, halt 힌트도 정리. 순차·병렬 양 경로 적용. `cleanup_stale_controls`가 redispatch-*.json은 지우지 않아 실행 시작 청소를 넘어 살아남는다. 실행 중 함대로의 핫-재큐는 범위 밖(다음 실행에서 효력)
- **설정 편집 `GET/POST /api/config`**: 설정 패널 [편집] 폼에서 `[conductor]` **화이트리스트 7키**(mode·approval_mode·max_parallel·max_redispatch·serve_dashboard·verifier_model·verdict_fallback) 읽기·쓰기. 필드별 검증(열거·범위·타입·텍스트 안전성), 화이트리스트 외 키·검증 실패는 400·**무쓰기(원자성)**, toml round-trip으로 타 섹션 보존(주석은 미보존 — 알려진 트레이드오프). M33의 "control/ 한정·설정 쓰기 금지" 경계를 [conductor]에 한해 **의도적 확장**(코드·project.md는 여전히 불가), 쓰기는 `utils::fs::write_file`(루트 경계) 경유
- **보안**: 신규 쓰기 엔드포인트 3개(`/api/launch`·`/api/config`·`redispatch` control) 모두 Origin 검증(403)·프로젝트 스코프(404)를 상속. 설정 쓰기는 화이트리스트로 임의 TOML 주입 차단. 하위호환: live.json 스키마 무변경, control `redispatch`·신규 엔드포인트·UI 버튼 모두 가산적, console·`--yes` 의미 보존
- **검증**: ① 단위(런 락 분기·재투입 오버라이드 소비·설정 검증/부분 갱신) ② HTTP 하니스 `scripts/dashboard-launch-validate.ps1`(claude 불필요 — 런 락 409·Origin 403·미등록 404·재투입 작성·설정 GET/POST 왕복·화이트리스트 위반 400 무쓰기) ③ **라이브** `scripts/dashboard-launch-live.ps1` — gate 모드 git 샌드박스에서 [함대 실행]→ 자식이 첫 task 게이트 블록(dispatch 전 → **비용 0**)→ **대시보드 강제 종료 후 자식 생존(detached spawn 증명)**→ stop-next 우아한 정지
- **리뷰가 잡은 수정**: 함대 실행 TOCTOU 경쟁(가드 후 spawn·이후 락 기록 → 동시 요청 이중 spawn 가능) → 락을 spawn **전에** 선점하고 실패 시 롤백
- **테스트**: 396개 (374 → 396, +22개)

### [v0.33.0]
- **태스크 작업 내용 가시화 (M36)**: 사용자 요청 — 모니터링에서 task id·숫자만이 아니라 **실제 작업 내용**이 보이게. ① 라이브 패널에 각 task의 **작업 제목**(`LiveTask.title`, live-1 하위호환 — `save_phase` 4단계 자동 전달, 병렬 배치 포함, 빈 제목은 기존 보존) ② 실행 리포트 **행 클릭 펼침** — 최신 run(M27 규칙 공유 `latest_run_records`)의 라운드별 verdict·diff·비용·**검증 피드백**(FAIL 사유, 빨간 테두리)·**에이전트 작업 보고**(dispatch_output). 재투입 task는 라운드별 구분되어 "왜 재투입됐고 어떻게 반영했는지"가 보인다
- **`GET /api/task?id=M1-T03`**: 감사 기록(conductor-4)에 이미 저장된 본문의 노출 — 새 수집 0·read-only. `AuditRecord`에 본문 필드(feedback·dispatch_output·verifier_raw) 역직렬화 추가, 응답 2KB 트렁케이트(전송 절제), 렌더링 esc() 경유(XSS 방어)
- **잔여 콘솔 게이트화 (gate 모드 무터미널 사이클 완결)**: ① **재실행 마일스톤 세션** — 전부-완료 상태로 재실행 시 무확인 LLM 세션 시작(M35 라이브 발견) 대신 confirm 게이트. 게이트 전 `serve_in_background`로 대시보드를 보장(내장 기동/공존)해 무한 대기 방지. 거부 시 릴리즈 플로우 ② **push 실패 재시도** — 마지막 잔여 콘솔 Confirm을 게이트로(M34 기록 갭). legacy(API 어댑터) 경로는 `conductor_enabled && ClaudeCode` 가드로 게이트 미발동, console·`--yes` 무변경
- **라이브 검증이 잡은 수정 2건**: ① 게이트 카드 표시 조건이 `run_active`에 의존해 — conductor 루프 밖(재실행·릴리즈 게이트)에서 발동한 게이트가 idle 상태라 **카드가 숨겨지던 버그** → pending_gate 단독 조건 + **WAITING 배지** ② legacy+`approval_mode=gate` 설정 조합의 재실행 게이트 오발동 → conductor 활성 가드(리뷰에서 발견)
- **라이브 검증**: ① 라이브 패널 작업 제목 표시 ② 합성 다중 라운드(R0 FAIL→R1 PASS) 행 펼침 — 피드백 빨간 박스·재투입 반영 보고·라운드 구분 시각 확인 ③ 재실행 confirm 게이트 → 릴리즈 text 게이트 → 콘솔 입력 0회 완주. 하니스 +2항목(/api/task 본문·live title)
- **테스트**: 374개 (372 → 374, +2개)

### [v0.32.0]
- **통합 실행 — conductor의 대시보드 내장 기동 (M35, Phase 3c)**: M33 라이브 피드백 3(대시보드·conductor 분리 실행 불편) 해소. **gate 모드 `porpoise` 한 번이면** 대시보드 서버가 같은 프로세스의 백그라운드 스레드로 자동 기동되고 브라우저가 열린다 — **터미널 1개**로 게이트 운영 전체가 시작되고, conductor 종료 시 함께 닫힌다
- **서버 분리 리팩토링**: `run_dashboard`(블로킹 CLI)에서 `try_bind`(바인딩 실패=None)·`serve_loop`·`serve_in_background`(`Started | PortInUse | Failed`)를 분리 — CLI 동작은 분리 조각의 재조립이라 무변경
- **공존 처리**: 포트(7878) 사용 중이면 에러가 아니라 **기존 대시보드와 공존** — 안내 후 브라우저만 연다. `serve_in_background`가 레지스트리 등록을 먼저 수행하므로 현재 프로젝트가 기존 대시보드의 프로젝트 셀렉터(M32)에 즉시 등장. 기동 실패는 경고만 남기고 실행 계속(대시보드는 부가 기능)
- **설정**: `[conductor] serve_dashboard` — 미설정 = gate 모드면 자동, `false` opt-out, `true`면 console 모드에서도 기동. 무결합 불변: 같은 프로세스여도 통신은 파일 매개(live.json·control/)
- **레지스트리 위생**: `/api/projects`가 `.porpoise` **실존 항목만** 반환(`registry::list_existing` — 읽기 필터, 파일 무수정·read-only 유지). 삭제된 프로젝트의 stale 항목이 셀렉터에 노출되던 문제(M34 정리에서 발견) 해소. `serve_in_background` 단위 테스트가 실제 홈 레지스트리를 오염시키던 부작용도 자가 정리로 수정
- **라이브 검증**: ① 통합 실행 — 터미널 1개로 자동 기동·브라우저·게이트 카드·사이클 완주 ② 공존 — 기존 대시보드 선실행 상태에서 "기존 대시보드와 공존" 안내, 게이트 전 사이클(승인→마일스톤 confirm→릴리즈 text) 완주, conductor 종료 후에도 대시보드 생존. 하니스 `scripts/dashboard-embed-validate.ps1`(이중 바인딩 graceful·기존 서버 무영향·stale 숨김) 추가
- **알려진 한계**: 재실행 시 전부-완료 상태로 진입하는 orchestrator 초입의 마일스톤 생성 세션은 아직 **게이트 없이 시작**(라이브에서 발견 — conductor 내 경로는 M34에서 게이트화됨, 일관성 갭은 M36 후보). 내장 기동 포트 7878 고정
- **테스트**: 372개 (369 → 372, +3개)

### [v0.31.0]
- **계획 제어 + 게이트 UX (M34, Phase 3b)**: M33 라이브 검증의 사용자 피드백 2건을 해소 — gate 모드 한 사이클(task 승인→실행→마일스톤 생성 확인→릴리즈 태그 입력)이 **터미널 입력 없이** 돈다
- **정지 예약 가시화 (피드백 1)**: [다음 게이트에서 정지]를 눌러도 화면 변화가 없던 문제 해소. live 페이로드에 `stop_pending`(서버 진실 — `control/stop-next.json` **존재만** 읽음, read-only 유지)을 추가하고 `snapshot`에 포함해 변화가 SSE로 push되게 함 → 버튼이 **"⏹ 정지 예약됨"**(비활성·강조)으로 즉시 전환, **모든 브라우저 창이 일관**, 게이트에서 소비되면 자연 해제
- **텍스트 게이트 프로토콜**: `pending_gate.kind`(`confirm` 기존 | `text` 자유 입력 | `confirm_text` 예약) + 응답 `text` 필드. `gate_exchange` 코어 신설(기존 `gate_decision`은 무변경 위임), `gate_text_decision` 추가. control API는 text를 **4KB 제한·제어문자 거부**로 검증하고 `serde_json` 직렬화로 특수문자를 안전 이스케이프
- **계획·릴리즈 게이트화 (피드백 2)**: gate 모드에서 전 task 완료 후 — "새 마일스톤을 생성하시겠습니까?"(conductor)가 confirm 게이트로, `run_release_flow`의 태그 입력(dialoguer)이 **text 게이트**(입력 폼·**Enter 전송**·빈 값/정지=건너뜀)로. console 모드·`--yes`·레거시(new_format) 경로 무변경
- **라이브 검증**: gate-trace 로그로 입증 — ① dispatch 중 정지 클릭 → 즉시 `STOP-PENDING` 기록·표시, merged 후 게이트 없이 graceful stop ② `confirm:m1-t02 → confirm:new-milestone → text:release-tag` 시퀀스 무터미널 완주, 모든 응답 파일 출현→소비, 잔여 0. 하니스 +3항목(stop_pending 노출/해제·text 왕복 보존)
- **알려진 한계**: git push **실패 시 재시도 confirm**(`run_release_flow` 내)은 아직 콘솔 — 드문 에러 경로로, 후속(M35+)에서 정리 예정
- **테스트**: 369개 (366 → 369, +3개)

### [v0.30.0]
- **게이트 제어 — 대시보드에서 task 승인·정지 (M33, Phase 3a)**: "지휘 통제실" 제어 1단계. `[conductor] approval_mode = "gate"`면 task/배치 승인 게이트("'M1-T01' 작업을 지휘하시겠습니까?")가 콘솔 프롬프트 대신 **대시보드 승인 대기 카드**([승인]/[정지])로 처리된다. 기본 `console` 모드와 `--yes` 자동 승인(자동화·CI)은 무변경
- **게이트 프로토콜 (무결합 유지)**: `src/conductor/gate.rs` 신설 — conductor가 `live.json`에 `pending_gate {id, prompt}`를 기록(SSE로 카드 표시)하고 `.porpoise/control/gate-<id>.json` 응답을 폴링(1초)·**소비(삭제)**. 손상 응답은 제거 후 계속 대기(무한 루프 방지). M31 관측의 역방향이지만 같은 원칙: 파일 매개, conductor는 대시보드의 존재를 모른다
- **graceful stop**: 실행 중(RUNNING) 상시 표시되는 **[다음 게이트에서 정지]** 버튼이 `control/stop-next.json`을 작성 — 진행 중 task를 마치고 다음 게이트에서 자동 정지(사전 정지가 게이트 응답보다 우선). 실행 시작 시 **stale 제어 파일 자동 청소**(`cleanup_stale_controls`) — 직전 실행의 미소비 stop-next가 다음 실행 첫 게이트를 의도치 않게 정지시키는 이월 함정 차단(리뷰에서 발견·수정)
- **`POST /api/control` — 대시보드의 첫 쓰기 기능**: `src/dashboard/control.rs` 신설. `{gate_id?, decision: approve|stop}` (gate_id 생략+stop = 사전 정지). 쓰기 범위를 해당 프로젝트 `.porpoise/control/`로 한정하고 3중 보호 — M32 허용 목록·`?project=` 스코프 상속(미등록 404), gate_id 영숫자·하이픈 검증(경로 주입 400), **Origin 검증**(localhost 외 브라우저 cross-origin POST 403 — CSRF 차단). 거부 요청은 파일을 남기지 않음
- **라이브 검증**: 실제 conductor gate 모드 + 브라우저로 전 시나리오 통과 — [승인]→진행, [정지]→세션 종료, [다음 게이트에서 정지]→T02 완료 후 T03 게이트 없이 자동 정지, 재실행 시 stale 청소로 정상 게이트 표시. 하니스 `scripts/dashboard-gate-validate.ps1`(승인 파일·graceful stop·403/400/404 경계) 추가
- **사용자 피드백 (후속 마일스톤 반영)**: 사전 정지 버튼의 시각 피드백 부재 → M34에서 `stop_pending` 노출·UI 표시 예정. 마일스톤 생성·릴리즈 세션의 콘솔 종속 → M34 계획 제어. 대시보드·conductor 분리 실행 불편 → M35 내장 기동
- **테스트**: 366개 (353 → 366, +13개 — gate 8 · control 5)

### [v0.29.0]
- **멀티 프로젝트 관측 — 대시보드 저장소 선택 (M32)**: 한 대시보드에서 여러 porpoise 프로젝트를 셀렉터로 전환하며 관제한다. 리포트·비용·DAG·라이브 패널이 선택한 프로젝트로 스코프되며, read-only 관측은 유지된다. 제어 UI(M33)가 처음부터 프로젝트-스코프로 설계되도록 기반(레지스트리·스코핑·보안 모델)을 이 단계에서 확립
- **프로젝트 레지스트리(허용 목록)**: `src/dashboard/registry.rs` 신설 — `~/.porpoise/registry.json`(`{id, name, path}`), id는 정규화 경로의 **FNV-1a 안정 해시**(외부 의존 0). `porpoise dashboard` 기동 시 현재 프로젝트 자동 등록(upsert), `--register/--unregister <path>` 명시 관리. 원자적 저장, 손상 파일은 빈 목록으로 우아 처리
- **보안 모델**: 클라이언트는 경로가 아닌 **불투명 id**(`?project=<id>`)로만 프로젝트를 참조하고, 서버는 **레지스트리에 등록된 경로만** 해석(`resolve_project_scope` 단일 관문) — 미등록 id·`.porpoise` 소멸 경로는 404. 자유 경로 입력에 의한 임의 파일시스템 열람 차단. 경계는 단위 테스트(4개 API 거부)와 스모크 양층으로 고정
- **API**: `GET /api/projects`(`{id,name,path,current}`) 신설, 전 데이터 API + **SSE**(`/api/events`)에 `?project=` 스코프. **미지정 시 기동 디렉터리**(기존 동작, 하위호환)
- **UI**: 헤더 프로젝트 셀렉터(등록 2개 이상일 때만 표시 — 1개면 기존 화면 동일), 전환 시 마일스톤→리포트→DAG 갱신 + **라이브 SSE 스트림을 닫고 새 프로젝트로 재구독**
- **라이브 검증**: 두 프로젝트(alpha 1태스크/$0.01 vs beta 5태스크/$0.22·FAIL/폴백/비용없음·DAG 6노드 3단계) 전환과 DevTools Network의 `?project=` 스코프(SSE 포함) 확인. **SSE 스코프 격리** 시각 입증 — beta 라이브 생애주기 재생 중 alpha 선택 시 IDLE 유지, beta 복귀 시 진행 상태 즉시 재수신(연결 직후 현재 상태 push). 멀티 스모크 `scripts/dashboard-multi-validate.ps1`(교차 ground truth·404·하위호환·레지스트리 원복) 추가
- **알려진 한계**: Windows `canonicalize`의 `\\?\` 접두사가 경로 표시에 노출될 수 있음(기능 무영향)
- **테스트**: 353개 (348 → 353, +5개)

### [v0.28.0]
- **대시보드 라이브 스트리밍 (M31, Phase 2)**: "지휘 통제실" 2단계. M30이 끝난 실행의 정적 조회였다면, 이제 **진행 중인 conductor 실행**을 실시간으로 비춘다 — 라이브 패널에 RUNNING/IDLE 배지, task별 현재 단계(brief→dispatch→verify→integrate 진행 배지, MERGED/HALTED 최종), 재투입 횟수, **누적 비용/예산 진행 바**(`budget_usd` 설정 시, 초과면 빨강). 실행 종료(RUNNING→IDLE) 시 리포트·DAG **자동 새로고침**, idle엔 마지막 실행 요약 표시
- **결합 없는 구조**: conductor(CLI)와 dashboard(서버)는 **파일을 매개로만** 통신. `src/conductor/live.rs` 신설 — conductor가 단계 전환·비용 갱신·종료마다 `.porpoise/live.json`(스키마 live-1)을 **원자적**(temp→rename)으로 기록(`save_phase` 끼워넣기로 침습 최소, 순차·병렬 공통). 기록 실패는 실행에 무영향. 에러로 비정상 종료해도 wrapper(`run_conductor`→`run_conductor_inner`)가 `live::finish`를 보장해 대시보드 stale RUNNING 고착 방지
- **SSE push**: `src/dashboard/sse.rs` 신설 — `GET /api/events`(SSE): live.json+sessions 변화를 500ms 폴링으로 감지해 push, 연결 직후 현재 상태 1회 전송, 10초 keep-alive. **요청별 스레드 분리**(M30 단일 루프 → spawn)로 장수명 SSE 연결이 다른 요청을 블록하지 않음. `GET /api/live` 단발 조회 — 프론트는 EventSource 실패 시 2초 폴링 폴백
- **tiny_http 버퍼링 근본 해결**: SSE가 전혀 전송되지 않는 문제를 소스 추적으로 확정 — `chunked_transfer::Encoder`의 8192B 내부 버퍼(flush_after_write=false) + 소켓 1KB `BufWriter`의 이중 버퍼링. SSE 스펙상 무시되는 주석(`:`) 패딩으로 두 버퍼를 강제 통과시켜 즉시 전송 보장(로컬 전용·단계 전환 빈도라 오버헤드 무의미)
- **병렬 모드**: 배치 수준 기록(배치 전체 dispatch → 통합 시 task별 merged) — 스레드 경쟁을 피하는 설계(문서화된 한계)
- **라이브 검증**: 실제 conductor 3 task + 브라우저 동시 관찰 — IDLE→RUNNING·sequential, 단계 배지 실시간 이동(dispatch 하이라이트 등), MERGED 전환, 예산 바 0%→23%→38%($0.3800/$1.00), 종료 시 자동 새로고침까지 시각 입증. live.json 전이 16건 트레이스(`watch-live.ps1`)와 3층 비용 정합(트레이스=패널=리포트 $0.3800) 확인. SSE 생애주기 하니스 `scripts/dashboard-live-validate.ps1` 추가
- **테스트**: 348개 (338 → 348, +10개 — live 5·sse 5)

### [v0.27.0]
- **로컬 웹 대시보드 — `porpoise dashboard` (M30, Phase 1)**: porpoise "지휘 통제실"의 1단계. conductor가 콘솔 텍스트로만 보여주던 데이터(실행 리포트·비용/토큰·의존성 그래프·마일스톤)를 로컬 웹에서 가시화한다. `porpoise dashboard [--port 7878] [--no-open]` — `tiny_http` 서버(127.0.0.1 전용) + `webbrowser` 자동 오픈. **read-only 관측 전용**(파일 쓰기 없음, conductor 로직 무변경)
- **화면**: 롤업 카드(태스크·성공률·PASS/FAIL·재투입·폴백·총비용), 태스크별 비용 막대 차트(PASS 녹색/FAIL 빨강, 비용 없는 conductor-3 태스크는 제외), 실행 리포트 표(verdict·시도·재투입·폴백·비용 — 비용 없음은 "-"), **의존성 DAG**(의존 깊이 열 배치 SVG, done/ready/waiting 색상·엣지). 마일스톤 셀렉터(최신 우선)·수동 새로고침(폴링)
- **JSON API**: `GET /api/milestones`·`/api/report?milestone=N`(롤업+태스크 요약)·`/api/tasks`(현재 태스크+의존성+상태) — `report::build_report`·`schedule::ready_tasks`·`parse_tasks_from_project_md`를 재사용해 직렬화만(새 데이터 0). `TaskRunSummary`에 `Serialize` 파생
- **단일 바이너리·오프라인 원칙 유지**: 프론트엔드(vanilla HTML/JS/CSS + 자체 경량 SVG 차트 lib)를 `include_str!`로 임베드 — CDN 의존 0, node 툴체인·빌드 단계 없음. 신규 의존성은 `tiny_http`·`webbrowser` 2개
- **보안**: 127.0.0.1 전용 바인딩, 고정 경로 정적 서빙(경로순회 불가), 파일 유래 문자열 HTML 이스케이프(self-XSS 방어)
- **라이브 검증**: 합성 데이터(단일 PASS/다중라운드/폴백/FAIL/conductor-3 비용없음/3단계 DAG/복수 마일스톤)로 **브라우저 렌더 ↔ API 캡처 로그 ↔ 원본** 3층 대조 완전 일치(M1·M2 화면 모두). 스모크 하니스 `scripts/dashboard-smoke.ps1`(서버+API 종단)·문서 `docs/dashboard.md` 추가
- **후속 Phase**: M31 라이브 스트리밍(SSE), M32 제어 UI(승인·halt·재투입)
- **테스트**: 338개 (331 → 338, +7개)

### [v0.26.2]
- **런타임 디렉터리 보장 위치 수정 (M29 보완)**: v0.26.1에 추가한 `ensure_runtime_dirs`가 `run_conductor` 내부에 있어, orchestrator의 프로젝트 포맷 판별(`is_new_format` — `.porpoise/sessions` 디렉터리 존재 여부로 판별) **이후**라 실제로 도달하지 못했다. `.porpoise/`가 gitignore된 fresh 체크아웃(sessions 비어 있음)에서 정식 프로젝트가 conductor 분기를 통째로 건너뛰고 "sessions/ 폴더가 없습니다"로 미인식되던 문제를 해소
- **수정**: 보장 로직을 orchestrator 진입부(판별·세션 정리 **이전**)로 이동. `ensure_project_runtime_dirs_if_applicable` 신설 — `.porpoise/project.md` 존재 + 비-legacy(`messages/` 없음)일 때만 `sessions/worktrees/reports` 보장. legacy 프로젝트는 건드리지 않아 마이그레이션 안내 경로 보존
- **라이브 검증(M29 종단)**: 런타임 디렉터리 미생성 + 메인 untracked Cargo.lock 상태의 fresh 프로젝트에서 ① conductor 정상 진입·디렉터리 자동 생성 ② untracked 병합 충돌을 `.porpoise/merge-backup/`로 백업 후 재시도하여 MERGED — 두 robustness 수정 모두 종단 입증
- **테스트**: 331개 (328 → 331, +3개 — new-format 보장→인식·legacy 스킵·비-porpoise 스킵)

### [v0.26.1]
- **conductor robustness 수정 (M29)**: M28 비용 라이브 검증(cost-live)에서 드러난 두 운영 신뢰성 갭 수정
- **병합 untracked 충돌 견고화**: 에이전트가 worktree에서 `cargo test` 등으로 생성한 파일(`Cargo.lock`)이 `git add -A`로 task 커밋에 포함되고, 통합 시 메인의 **untracked 동명 파일**과 충돌해 `git merge`가 하드 실패("untracked working tree files would be overwritten")하던 문제 해소. `integrate.rs`에 `merge_with_untracked_recovery` 추가 — 해당 유형이면 충돌 파일을 `.porpoise/merge-backup/<ts>/`로 **이동(삭제 아님, 데이터 손실 0)** 후 병합을 재시도하고 백업 위치를 콘솔에 안내. 내용 충돌·기타 실패는 기존 처리(abort/Conflicted) 그대로 유지. 순차(`merge_worktree`)·병렬(`try_merge_worktree`) 경로 공통 적용
- **런타임 디렉터리 보장**: `.porpoise/{sessions,worktrees,reports}`는 gitignore 대상이라 새 체크아웃엔 없어 첫 실행이 실패할 수 있었음. `ensure_runtime_dirs`로 conductor 시작 시(순차·병렬 공통 지점) 자동 생성(멱등)
- **한계**: untracked 충돌 감지는 영어 git 메시지에 의존(비영어 로케일은 복구 미발동 — 기존 abort로 안전 폴백)
- **테스트**: 328개 (325 → 328, +3개 — 디렉터리 보장 멱등·에러 파싱·실제 git untracked 복구 회귀)

### [v0.26.0]
- **비용 관측 + 예산 거버넌스 (M28)**: conductor가 dispatch하는 코딩 에이전트의 **비용(USD)·토큰을 캡처·집계**하고, **예산 상한** 도달 시 dispatch를 중단한다. M25 리포트 인프라(관측) 위에 비용 차원을 더하는 운영/거버넌스 단계
- **비용 캡처**: `runner.rs`에 `AgentRun` + `run_agentic_metered` 신설 — Claude Code를 `--output-format stream-json`으로 실행해 최종 `result` 이벤트의 `total_cost_usd`·`usage`(입력/출력 토큰)를 순수 함수 `parse_stream_event`로 파싱. **스트리밍 표시 유지**. CLI 미지원·비-JSON 시 평문 폴백 + 비용 `None`(graceful 저하). `run_agentic`(검증자 경로)·레거시 `execute_claude`는 불변(blast radius 최소화)
- **비용 집계**: 감사 기록 **conductor-4**(`cost_usd`·`input_tokens`·`output_tokens`, 구 기록 `None` 하위호환). `report`가 태스크별 비용 + 마일스톤 총비용·총토큰을 **최신 run 기준**(M27 일관)으로 롤업, 콘솔·Markdown에 비용 컬럼 추가
- **예산 거버넌스**: `[conductor] budget_usd`(선택) 설정. `budget_exceeded` 순수 함수로, 누적 비용이 상한 도달 시 순차·병렬 양쪽에서 다음 dispatch/배치 전 중단(진행 중인 것은 마치고 정지). 미설정·0 이하면 무제한. `status`/`doctor`에 비용·예산 표시
- **라이브 검증**: 실 CLI로 3개 독립 task 실행 → 비용 캡처(세션 `cost_usd` 실측 0.1171/0.1276/0.1159)·누적 추적(콘솔 "누적 $0.3607")·report 롤업(총 $0.3607 · 토큰 5931/2434, ground truth 정확 일치) 확인. 하니스 `scripts/conductor-cost-validate.ps1`·런북 `docs/conductor-cost-runbook.md` 추가
- **테스트**: 325개 (316 → 325, +9개)

### [v0.25.2]
- **`porpoise report` 집계 버그 수정 (M27)**: 같은 task를 **재실행하면** 이전 run의 오래된(stale) 레코드가 `sessions/`에 함께 쌓여, 기존 "최종 라운드 = max redispatch(동률 시 timestamp)" 기준이 **stale FAIL(R2)을 fresh PASS(R0)보다 우선**시해 최종 verdict를 오판하고 `attempts`/`재투입`도 부풀리던 버그를 수정
- **최신 run 기준 집계**: `aggregate`를 timestamp 정렬 후 **마지막 `R0`(redispatch==0 = run 시작)부터 끝까지를 최신 run**으로 보고, 그 run만 집계하도록 변경. `verdict`·`시도`·`재투입`·`fallback`이 가장 최근 실행만 반영. R0가 없으면(세션 정리 등) 전체를 한 run으로 폴백
- **검증**: 회귀 테스트 3개 추가 — 재실행(이전 FAIL×3 + 이번 PASS → final PASS·시도1·재투입0), 최신 run 다중 라운드 보존, `read_dir` 비정렬 입력에서도 정렬로 올바른 run 선택. 라이브 재확인: **동일 sessions**(stale FAIL 포함)에서 report가 M1-T02를 ❌ FAIL→✅ PASS, 롤업 PASS 2/2(100%)로 정정
- **발견 경위**: M26 검증(report-live 재구동)에서 PASS·MERGED된 task가 report엔 FAIL로 표시되며 드러난 순수 집계 버그(conductor 동작은 정상). 런북에 재실행-인지 집계 규칙 명시
- **테스트**: 316개 (313 → 316, +3개)

### [v0.25.1]
- **변경 감지 버그 수정 (M26)**: 에이전트가 격리 worktree 안에서 **자기 작업을 커밋하면** conductor가 빈 diff로 인식해 정상 작업을 "변경 없음"으로 폐기·halt 하던 비결정적 신뢰성 버그를 수정. `Worktree`가 분기 시점 base 커밋 SHA를 기록하고, `capture_diff`를 `git diff --cached <base>`로 계산하여 커밋·미커밋·미추적 변경을 **모두 base 대비로 포착**(커밋 여부 무관)
- **통합 단계 2차 결함 수정**: 위 수정으로 에이전트-커밋 task가 PASS→통합에 진입하면서 드러난, clean 작업트리에서 `git commit`이 "nothing to commit"으로 실패하던 문제도 해결 — `commit()`이 스테이징 변경 없음을 감지하면 커밋을 건너뛰고(에이전트 커밋은 이미 브랜치에 존재) 병합이 가져가도록 함. 빈-diff 가드는 유지(진짜 작업 없음은 여전히 FAIL)
- **검증**: 회귀 테스트 3개 추가(`capture_diff_sees_committed_work`·`capture_diff_empty_when_no_work`·`finalize_succeeds_when_agent_already_committed`), 순수 git 레벨 재현 하니스(`scripts/conductor-commit-detect-validate.ps1`)로 OLD(HEAD 기준)=빈 값 / NEW(base 기준)=포착 대조 증명. 문서 `docs/conductor-change-detection.md` 추가
- **발견 경위**: M25 라이브 검증(report-live)에서 정상 작업 M1-T02가 3회 FAIL→halt 되며 드러난 버그. 순차·병렬 경로 모두 공유 `capture_diff`/`commit`을 쓰므로 함께 수정됨
- **테스트**: 313개 (310 → 313, +3개)

### [v0.25.0]
- **함대 실행 리포트 — `porpoise report` (M25)**: conductor가 매 라운드 `.porpoise/sessions/`에 쓰기만 하고 아무도 읽지 않던 감사 기록(conductor-3 스키마)을 **마일스톤 실행 요약**으로 집계·가시화한다. `src/conductor/report.rs`에 `aggregate`·`build_report`·`load_records`·`render_markdown`(순수 함수 분리, M24 `schedule.rs` 패턴 계승) 신설
- **서브커맨드 옵션**: `porpoise report [--milestone N] [--markdown] [--out 경로]` — 태스크별 verdict·시도(라운드)·재투입·폴백 표 + 롤업(성공률·재투입 합계·폴백 비율). `--milestone` 미지정 시 가장 최근 마일스톤으로 한정. `--out` 지정 시 `--markdown` 없이도 자동 내보내기
- **Markdown 내보내기**: `.porpoise/reports/run-M{N}.md`로 실행 보고서 축적 — 릴리즈 노트·마일스톤 회고 근거로 재사용
- **`porpoise status` 통합**: 최근 실행 1줄 요약(성공률·재투입·폴백)을 표시(기록 없으면 생략, 기존 동작 무변경)
- **견고성**: 손상 JSON은 스킵하고 건수만 표시, serde가 거부하는 **UTF-8 BOM을 제거**해 파싱(외부 도구 호환), 빈 입력 무패닉
- **라이브(합성) 검증**: 알려진 감사 기록을 주입해 `report` 출력이 ground truth와 정확히 일치함을 확인(다중 라운드→최종 verdict, 폴백 OR 집계 포함). 하니스 `scripts/conductor-report-validate.ps1`(합성+`-Live`)·런북 `docs/conductor-report-runbook.md` 추가
- **테스트**: 310개 (299 → 310, +11개)

### [v0.24.0]
- **계획 두뇌 — 의존성 그래프 스케줄링 (M24)**: task가 `(deps: M1-T01, M1-T02)` 형식으로 선행 task를 선언하면, conductor가 **ready(모든 선행 완료) task만** 배치한다. 선행 완료 시 다음 라운드에서 의존 task가 ready로 전이 — DAG 기반 위상(topological) 실행. `src/conductor/schedule.rs`에 `ready_tasks`·`has_cycle`(DFS)·`dangling_deps`·`validate_dependencies` 순수 함수 신설
- **순환·dangling 검증**: 시작 전 의존성 그래프를 검사해 **순환(cycle)이면 거부**(무한 대기 방지), 존재하지 않는 의존성(dangling)은 **경고 후 무시**(오타가 task를 영구 차단하지 않도록 ready 계산에서도 '만족'으로 취급). `porpoise doctor`에 의존성 그래프 검증 항목 추가
- **`porpoise status`**: ready task는 `⏳`, 선행 대기 중인 task는 `🔒 (대기: deps)`로 표시
- **deps 파싱·전파**: `Task`에 `dependencies: Vec<String>` 추가, `parse_task_deps`로 `(deps: ...)` 접미사 파싱(`TaskId::new` 정규화). 마일스톤→project.md 미러링 시 `(deps: ...)`를 **보존**하도록 수정(이전엔 누락되어 스케줄링이 무력화됨). 계획 프롬프트(`05-milestone.tmpl`)에 에이전트 크기 분해·독립 우선·`(deps:)` 작성 가이드 추가
- **라이브 검증(D1)**: max_parallel=3, T03이 T01·T02에 의존하는 시나리오에서 라운드 1에 **T01·T02만**(2개) 병렬 실행, 라운드 2에 T03 실행. project.md `(deps:)` 보존·무충돌(redispatch=0) 완료 확인. PASS
- **테스트**: 299개 (286 → 299, +13개)

### [v0.23.0]
- **병렬 함대 (M23, opt-in)**: `[conductor] max_parallel`(기본 1=순차, [1,8])을 올리면 독립 task N개를 **각자 worktree에서 동시에** dispatch·verify하고, 결과를 **순차·충돌 인지**로 통합. 기본 1이라 기존 동작 무변경
- **낙관적 동시성**: 병합 충돌이 나면 그 task만 abort 후 **갱신된 base에서 재투입**하여 직렬화(수렴). 재투입 시 충돌/실패 피드백을 brief에 주입하여 에이전트가 갱신 코드 위에 재적용하도록 함. 시도 한도(`max_redispatch`) 초과·무진전 시 안전 중단
- **충돌 인지 병합**: `try_merge_worktree`로 non-FF 병합 처리, 충돌 시 abort + `Conflicted` 반환
- **출력 캡처**: 병렬 실행 중 에이전트 출력을 캡처만 하고 완료 후 task별 그룹 표시(인터리브 방지)
- **라이브 검증 완료**: 독립 task 병렬(P1)·충돌→재투입 수렴(P2) 모두 라이브 PASS. 하니스 `scripts/conductor-parallel-validate.ps1`·런북 `docs/conductor-parallel-runbook.md` 추가
- **`porpoise doctor`**: `병렬: N개` 표시
- **테스트**: 286개 (282 → 286, +4개)

### [v0.22.0]
- **⚠ 기본 동작 변경 — conductor 모드 기본 ON (M22)**: claude_code 어댑터에서 `[conductor].mode` 미설정 시 **기본적으로 conductor 루프**(에이전트 통째 위임 + 독립 검증)로 동작합니다. 기존 4단계 phase 방식을 쓰려면 `workspace.toml`에 `[conductor] mode = "legacy"`로 opt-out하세요. API 어댑터는 영향 없음(항상 legacy). 기존 프로젝트 첫 진입 시 1회 전환 안내 출력
- **비-git 자동 폴백**: 기본 ON이지만 git 저장소가 아니면 **자동으로 legacy로 폴백**(하드 실패 방지) — `git init` 또는 명시적 `mode` 설정 안내
- **폴백 정책 (`verdict_fallback`)**: 검증자 verdict 파싱 실패가 재질의 후에도 지속될 때 — `"pass_if_checks_pass"`(기본, 검증 명령 통과면 객관 증거로 PASS) | `"halt"`(보수, 사용자 검토). 폴백 PASS 시 콘솔·감사에 `⚠ 경고` + `fallback_used` 표식(`conductor-3` 스키마)
- **라이브 검증 완료**: 정상 경로(false-negative 0)와 안전망 폴백(파싱 실패 → 재질의 → 객관 증거 PASS) 모두 라이브 3/3 검증. 재검증 하니스(`scripts/conductor-revalidate.ps1`, `-ForceFallback`)·런북(`docs/conductor-revalidation-runbook.md`) 추가
- **`porpoise status`/`doctor`**: conductor 실행 모드(기본 ON/legacy) 표시
- **테스트**: 282개 (270 → 282, +12개)

### [v0.21.0]
- **conductor 검증자 신뢰성 경화 (M21)**: 라이브 스모크 테스트에서 드러난 false-negative FAIL(코드 정상·`cargo test` 통과인데 검증자 LLM 응답을 파싱하지 못해 보수적 FAIL → 정상 작업 폐기)을 해소 — verdict 파싱 실패 시 즉시 FAIL 대신 **재질의 1회 → 객관 증거(`verify_commands` 통과) 기반 폴백**으로 처리. 검증자 출력 형식 강제 강화("도구·탐색·설명 금지, JSON 객체 하나만")
- **감사 기록 관측성 (`conductor-2`)**: `sessions/<task>-conductor-<timestamp>-R<n>.json`에 검증자 원문·dispatch 출력 포함, 타임스탬프 파일명으로 재투입·재실행 이력 보존
- **worktree 누수 방지**: `conduct_task`가 성공·실패·중단 모든 경로에서 worktree·브랜치 정리 보장, conductor 시작 시 `.porpoise/` gitignore 자동 보장
- **라이브 재검증 하니스**: `scripts/conductor-revalidate.ps1`(N회 반복 + 검증자 신뢰성 자동 집계), `scripts/conductor-smoke.ps1`, `docs/conductor.md`(기본 ON 승격 기준)
- conductor 기본 모드는 **legacy 유지**(opt-in) — 라이브 재검증으로 승격 기준 충족 확인
- **테스트**: 270개 (259 → 270, +11개)

### [v0.20.0]
- **`porpoise status` 서브커맨드 신설 (M19)**: 현재 마일스톤·태스크·단계·사이클·세션 파일 수를 한 명령으로 출력 — `checkpoint.json`·`milestones/`·`sessions/`를 통합 요약. 미초기화 디렉토리에서는 초기화 안내 출력
- **`porpoise doctor` 품질 개선 (M19)**: (1) 실패 항목이 있으면 **exit code 1** 반환 — CI 헬스체크(`porpoise doctor || exit 1`) 활용 가능, (2) workspace.toml 체크 메시지에서 어댑터 정보 중복 제거, (3) API 키 미설정 힌트의 macOS/Linux 줄 들여쓰기 정렬
- **`is_likely_api_key()` 정밀도 개선 (M19)**: `claude-` 접두사 제거(모델 이름 `claude-sonnet-4-6` 오진단 해소), Anthropic `sk-ant-`·OpenAI `sk-proj-` 접두사 추가
- **테스트**: 259개 (252 → 259, +7개)

> 참고: M19(doctor·status)는 본래 v0.19.0으로 계획되었으나, 지휘자 피벗(M20)이 먼저 v0.19.0으로 릴리즈되어 v0.20.0으로 배치되었습니다.

### [v0.19.0]
- **지휘자(conductor) 루프 신설 (M20)**: AI worker→manager 전환의 첫 단계 — task 하나를 `Brief → Dispatch → Verify → Integrate` 4단계로 처리. 실제 코딩 에이전트에게 격리 git worktree에서 통째로 위임(Dispatch)하고, 독립 검증자가 실제 테스트 실행 + 적대적 심사로 PASS/FAIL 판정(Verify), PASS 시 worktree 커밋·병합·완료 처리(Integrate), FAIL 시 피드백 재투입(한도 내). 기존 4단계 phase 호출을 단일 에이전틱 위임이 대체
- **`src/conductor/` 모듈 신설**: `brief`(작업 지시서 빌더)·`dispatch`(worktree 격리·diff 캡처)·`verify`(독립 검증 + verdict 파싱)·`integrate`(병합 결정·finalize)·`git`(헬퍼)
- **`[conductor]` workspace.toml 설정**: `mode`(기본 `legacy`, opt-in `conductor`)·`verifier_model`·`max_redispatch`(기본 2) — claude_code 어댑터 전용, API 어댑터는 항상 legacy
- **`ClaudeRunner::run_agentic`**: 작업 디렉토리 지정 풀 에이전틱 실행 모드 추가
- **checkpoint `conductor_phase` 필드**: 지휘 단계(brief/dispatch/verify/integrate) 기록 (레거시 경로는 None)
- **`porpoise doctor` conductor 진단**: 모드·git 저장소·재투입 한도·검증자 점검 항목 추가
- **기본 모드는 legacy 유지**: 종단 간 라이브 검증 완료 전까지 conductor는 명시적 opt-in — 기존 동작 100% 보존, 병렬 함대(M12)·계획 두뇌(M13)는 후속 마일스톤 범위
- **테스트**: 252개 (215 → 252, +37개)

### [v0.18.0]
- **`AnthropicApiAdapter` `api_key_env` 준수**: `workspace.toml`의 `api_key_env` 설정이 `anthropic_api` 어댑터에서 무시되던 버그 수정 — 어댑터 생성 시 설정된 환경변수 이름을 실제로 사용, `ANTHROPIC_API_KEY` 하드코딩 제거
- **`is_likely_api_key()` 정밀도 개선**: 소문자 포함 문자열 전체를 "API 키"로 오진단하던 로직 제거 — `AIzaSy`, `sk-`, `gsk_`, `xai-`, `claude-` 접두사 기반으로 감지 범위 축소, 소문자 env var 이름에 잘못된 경고 미출력
- **`porpoise doctor` 서브커맨드 신설**: 설정 진단 명령 추가 — `workspace.toml` 파싱·어댑터 타입·Claude CLI 설치·API 키 env var·Ollama 서버 연결·`sessions/` 디렉토리·최근 마일스톤 파일 7개 항목 순서대로 점검, 실패 항목에 OS별 해결 안내 출력
- **`cleanup_sessions` 나이 기반 삭제 테스트 보강**: `keep_completed=false + max_age_days=0` 단독 케이스, `max_age_days=1` 조건에서 최근 파일 보존 케이스 추가 — 기존 테스트가 항상 `keep_completed=true`와 조합하던 편향 해소
- **테스트**: 215개 (207 → 215, +8개)

### [v0.17.0]
- **API 키 환경변수명 입력 검증**: `porpoise --new` / `porpoise update config`에서 `api_key_env` 입력 시 대문자·숫자·밑줄 형식 검증(`validate_env_var_name`) + 실제 API 키 값 패턴 감지(`is_likely_api_key`, 3-retry 경고) — 실수로 키 값을 환경변수명 필드에 입력하는 오류 방지
- **초기화 후 OS별 환경변수 설정 안내**: `print_api_key_env_guide()` 추가 — 초기화 완료 및 `update config` 완료 후 PowerShell·Unix 양식의 환경변수 설정 명령 자동 출력
- **어댑터 생성 전 API 키 env var 사전 검증**: `factory.rs` `make_adapter()`에서 `anthropic_api` / `openai_compatible` 어댑터 생성 전 env var 존재 여부 확인 — 설정되지 않은 경우 즉각적인 명확한 에러 메시지 출력 (기존: 실행 중 HTTP 에러)
- **Gemini 기본 모델 변경**: `gemini-2.0-flash` → `gemini-2.5-flash` — `init` 시 기본 선택 모델 업데이트
- **Dead code 경고 16개 → 0개**: 미사용 함수·메서드·필드 제거 및 `#[allow(dead_code)]` 정리 — `Report::stub()`, `report_filename()`, `count_existing_reports()`, `Role::prev()`, `Role::prompt_file()`, `run_with_prompt()` 삭제
- **`cleanup_sessions` 유닛 테스트 3개 추가**: 완료 태스크 세션 삭제 / `keep_completed=true` 보존 / `max_age=0` 비삭제 시나리오 커버
- **`workspace.toml` `[sessions]` 주석 예시 추가**: `default_toml()` 끝에 세션 정책 설정 예시를 주석으로 포함
- **테스트**: 207개 (204 → 207)

### [v0.16.0]
- **`porpoise migrate` 서브커맨드 신설**: 레거시 프로젝트(`.porpoise/messages/`, `.porpoise/reports/`)를 JSON 세션 포맷으로 전환 — `sessions/` 디렉토리 생성 후 다음 실행부터 신규 포맷으로 동작, 기존 레거시 파일 보존
- **`legacy.rs` 삭제 및 진입점 통합**: MD 기반 레거시 오케스트레이터 코드 경로 완전 제거 — `orchestrator::run()` 진입점을 `mod.rs`로 통합, 레거시/신규 분기 로직 단순화
- **Session JSON 자동 정리 (`cleanup_sessions`)**: `workspace.toml [sessions]` 정책에 따라 완료된 마일스톤 세션 파일 및 오래된 세션 파일 자동 삭제 — `keep_completed_milestone_sessions`(기본: false)·`max_session_age_days`(기본: 30) 설정 추가
- **Snapshot git diff 라인 제한**: `GIT_DIFF_MAX_LINES = 200` — 기존 byte 기반 (`16KB`) 제한을 라인 기반으로 변경, 컨텍스트 예측 가능성 개선
- **테스트**: 204개 유지

### [v0.15.2]
- **`orchestrator` 모듈 분리**: `mod.rs` (1634줄) → `legacy.rs`(레거시 MD 기반 라우팅)·`new_format.rs`(JSON 세션 라우팅)로 분리 — 공통 헬퍼 12개는 `mod.rs`에 `pub(super)` 헬퍼로 유지, 단일 파일 복잡도 해소
- **`Milestone` 구조체 dead code 제거**: `metadata: HashMap<String, String>` 필드·`parse_metadata()` 함수·`file_path: PathBuf` 필드 제거 — `parse_milestone_content()` 시그니처에서 `path: &Path` 인자 제거
- **`load_milestone()` 함수 제거**: `milestone/parser.rs`에서 미사용 공개 함수 완전 삭제
- **`TaskId::as_str()` 제거**: `orchestrator/state.rs`에서 `#[allow(dead_code)]` 미사용 메서드 삭제
- **`delete_dir()` 제거**: `utils/fs.rs`에서 미사용 함수 및 관련 테스트 삭제
- **`#[allow(dead_code)]` 애노테이션 정리**: 실제 사용 중인 함수(`delete_file`, `move_file`)와 필드(`raw_sections`)에 붙어 있던 불필요 애노테이션 제거
- **테스트**: 미사용 코드 제거로 총 204개 (v0.15.1 대비 2개 감소)

### [v0.15.1]
- **Gemini `chat_completions_url` 버그 수정**: `openai_compatible` 어댑터의 URL 생성 로직이 `/v1`로 끝나지 않는 엔드포인트(`...v1beta/openai`)에 잘못된 `/v1/chat/completions`를 붙이던 버그 수정 — Gemini API 엔드포인트가 항상 404를 반환하던 문제 해소, 조건에 `ends_with("/openai")` 분기 추가
- **테스트 추가**: 1개 신규 테스트 `chat_completions_url_gemini_openai_suffix` (총 206개)

### [v0.15.0]
- **`porpoise update prompt` 서브커맨드 신설**: `--new` 없이 `.porpoise/prompts/` 6종만 재생성 — `workspace.toml` 어댑터 타입(`claude_code` / `anthropic_api` / `openai_compatible`) 기반으로 CC·API 템플릿 분기 유지, 프로젝트 데이터(milestones, sessions) 무변경
- **`porpoise update config` 서브커맨드 신설**: 언어 및 모델 재선택 대화상자 — `workspace.toml [general].language`와 `[model]` 섹션만 갱신, 기존 `[dod]`·`[conventions]`·`[tech]` 설정 유지
- **최초 마일스톤 M2 오탐 수정**: `05-milestone-api.tmpl` 예시 값(`"M2"`, `M2-T01`)을 `"M1"`, `M1-T01`으로 변경 + `milestone_session.rs`에서 모델 반환 `milestone_id` 무시 및 `next_id` 강제 적용 — json_mode에서 항상 M2가 생성되던 버그 수정
- **task ID 자동 정규화**: `normalize_task_id()` 추가 — 모델이 잘못된 마일스톤 번호를 포함한 task ID(`M2-T01`)를 반환해도 실제 `next_id` 기준으로 재정규화(`M1-T01`) 후 파일에 기록
- **`file_operations` 중복 키 버그 방지**: `02-development-api.tmpl` 다중 파일 "올바른 예"(배열 별도 항목) 및 중복 키 "절대 금지" 예시 추가 — 하나의 JSON 객체에 `op`·`path` 키를 중복 작성하면 앞 파일이 유실됨을 명시
- **테스트 추가**: 4개 신규 테스트 (총 198개)

### [v0.14.2]
- **API 프롬프트 json_mode 폴백 지시 추가**: API 전용 템플릿 5종(`01~05-*-api.tmpl`)의 `submit_report 필드 명세` 섹션 헤더를 "JSON 출력 형식"으로 변경하고 json_mode 폴백 지시 추가 — 도구 호출 불가 환경에서 JSON 객체를 텍스트로 직접 출력하도록 안내, `EOF while parsing` 에러 방지
- **IMP-02 오탐 수정**: API 전용 프롬프트 템플릿 섹션 헤더에 "JSON 출력 형식" 문자열 포함 — API 프롬프트 최신 상태에서도 IMP-02 경고가 발생하던 오탐 수정
- **`testing_schema.json` `regression_check` 추가**: Rust 구조체의 `regression_check: Option<RegressionCheck>` 필드를 JSON 스키마 `properties`에 추가 — 스키마-구조체 정렬
- **HTTP 에러 응답 바디 포함**: `openai_compatible.rs` `post_json()` 및 `anthropic_api.rs` inline ureq 호출에서 HTTP 에러 발생 시 상태 코드와 응답 본문을 에러 메시지에 포함 — API 에러 원인 파악 개선

### [v0.14.1]
- **초기화 시 어댑터 모드 기반 프롬프트 분기 버그 수정**: `porpoise --new`에서 API 어댑터(`anthropic_api`, `openai_compatible`)를 선택해도 CC 전용 프롬프트가 생성되던 버그 수정 — `generator.rs`에 `use_api_templates` 분기 추가, API 어댑터 선택 시 `*-api.tmpl` 5종을 `.porpoise/prompts/`에 배포
- **테스트 추가**: 4개 신규 테스트 (총 194개)

### [v0.14.0]
- **CC / API 전용 프롬프트 분리**: `01-planning.tmpl` 등 기존 CC 전용 템플릿과 별도로 `01-planning-api.tmpl` 등 API 전용 템플릿 5종 신설 — CC 어댑터와 API 어댑터가 각각 최적화된 지시사항·출력 형식을 사용
- **Development `max_tokens` 증가**: API 어댑터에서 Development 단계 `max_tokens`를 4096 → 16384으로 증가 — 파일 작성 중 토큰 한도 도달로 인한 응답 잘림 방지
- **`api_json_format_hint()` 인라인 주입 제거**: 런타임 시스템 프롬프트 주입 방식을 폐기하고 힌트를 API 전용 템플릿 내부로 이전 — 중복 주입 문제 해소
- **`development_schema.json` null 타입 제거**: `file_operations` 배열에서 null 허용 타입을 제거하여 API 모드에서 항상 배열 형식으로 응답하도록 스키마 강제
- **단계 명칭 통일**: 프롬프트 템플릿 10종 및 소스코드 전체에서 '역할' → '단계', 'PM·Developer·Tester·Reviewer' → 'Planning·Development·Testing·Review' 명칭 통일 (소스 25개 위치)

### [v0.13.0]
- **API 모드 Development `file_operations` 필수화**: 프롬프트 힌트 + `development_schema.json required` + 오케스트레이터 3중 강제 — `changes[]` 있는데 `file_operations` 없으면 즉시 PREV 전환 및 안내 메시지 출력 (파일 미생성 → 무한 사이클 버그 수정)
- **AI 응답 원문 출력 `--verbose` 전용 제한**: `[AI 텍스트 응답]`, `[AI submit_report]`, `[AI 응답 (json_mode)]` 등 5개 출력 블록이 `--verbose` 플래그 시에만 표시 — `ModelConfig.verbose` 필드 추가, `execute_role_new()` → 어댑터 전달 경로 완성
- **`&&` 복합 명령 자동 분리 (`parse_command_string_multi`)**: `"ruff check . && mypy ."` 같은 복합 명령을 `&&` 기준으로 자동 분리하여 각각의 `VerifyCommand`로 실행 — `|`, `;`, `` ` ``, `$` 포함 명령은 기존과 동일하게 경고 후 건너뜀
- **커밋 전 `.gitignore` 자동 검증**: `auto_commit()` 진입 시 `.porpoise/` 항목을 `.gitignore`에 자동 추가 (파일 없으면 생성) — 세션·로그 파일 대용량 커밋 방지
- **`auto_commit()` target_paths에서 `.porpoise/` 제거**: porpoise 런타임 데이터(sessions, reports, hints)가 자동 커밋 대상에서 영구 제외
- **테스트 추가**: 1개 신규 테스트 (총 190개)

### [v0.12.0]
- **`issues_found` 역직렬화 버그 수정**: API 모드 testing 역할에서 `issues_found` 필드가 `Vec<String>` → `Vec<IssueFound>` 구조체 배열로 변경, 방어적 deserializer 추가 — 문자열·객체 양쪽 형식 처리 가능
- **`print_error()` 에러 유형별 메시지**: `resolve_hint()` 함수 추가 — 프로그램 미설치·권한 오류·네트워크 오류·API 키 누락 등 6가지 유형별 해결 안내 출력
- **`.context()` 4곳 보완**: 에디터 실행 실패(`input.rs`), 마일스톤 입력 실패(`milestone_session.rs` 2곳), API 응답 JSON 파싱 실패(`anthropic_api.rs`, `openai_compatible.rs`) — 에러 원인 전파 개선
- **lint 명령 단순화**: Python `"ruff check . && mypy ."` → `"ruff check ."`, TypeScript `"npx eslint . && npx tsc --noEmit"` → `"npx eslint ."` — `&&` 메타문자로 인한 `parse_command_string` 건너뜀 버그 수정
- **시작 시 의존성 검사 (`deps.rs` 신설)**: 실행 시 `workspace.toml` 기반 필수 명령어 설치 여부 확인, 미설치 항목은 OS별 설치 방법 안내 후 종료 (`--new`·`clean`·`approve` 시 스킵)
- **`verify_commands` 배열 필드 추가**: `workspace.toml [tech]`에 `verify_commands` 배열 지원 — 복수 검증 명령을 `&&` 없이 개별 지정, 기존 `test_command`·`lint_command` 폴백 유지
- **`workspace.toml` 기본 템플릿 OS별 명령 예시**: `default_toml()`에 `verify_commands` 배열 예시 및 Windows/macOS·Linux OS별 파일 조작 명령 허용 주석 추가
- **언어 템플릿 OS별 파일 명령 자동 삽입**: `LangTemplate`에 `allowed_file_commands_windows/unix` 필드 추가, `porpoise --new` 시 현재 OS에 맞는 파일 명령(`powershell`/`cp mv rm` 등)이 `allowed_command_prefixes`에 자동 포함
- **API 어댑터 AI 응답 콘솔 출력**: `anthropic_api`·`openai_compatible` 어댑터에서 AI 텍스트 응답 및 submit_report 내용을 콘솔에 출력 (디버깅 가시성 향상)
- **API 모드 JSON 출력 형식 안내 강화**: `api_json_format_hint()` 신설 — 역할별 submit_report 필드 안내를 시스템 프롬프트에 자동 주입
- **테스트 추가**: 6개 신규 테스트 (총 189개)

### [v0.11.1]
- **API 역직렬화 방어 처리 전체 적용**: `PlanningOutput`·`DevelopmentOutput`·`TestingOutput`·`ReviewOutput`·`MilestoneOutput`의 모든 비-`Option` 필드에 `#[serde(default)]` 추가 — API 어댑터 응답에서 필드 누락 시 `missing field` 크래시 방지
- **`status` 기본값 수정**: `ExitCode::default()`(`Resp`)가 아닌 `ExitCode::Next`로 기본값 지정 — `status` 필드 누락 시 잘못된 라우팅 방지
- **`review_status` 기본값 추가**: 누락 시 `"CHANGES_REQUESTED"`로 보수적 처리
- **`default_exit_code_next` 공용 함수화**: `session/output.rs`로 이동하여 모든 구조체에서 공유

### [v0.11.0]
- **API 마일스톤 역직렬화 수정**: `MilestoneOutput.role`·`milestone_id`에 `#[serde(default)]` 추가 — `anthropic_api`·`openai_compatible` 어댑터에서 마일스톤 생성 시 `missing field` 오류로 중단되던 버그 수정
- **마일스톤 생성 완료 후 파일 출력**: `claude_code`·API 어댑터 양 경로 모두 마일스톤 생성 완료 시 생성된 `M{n}.md` 파일 전체 내용을 콘솔에 출력
- **역할 완료 후 리포트 파일 출력**: 역할 완료(fresh 실행·캐시 재사용 공통) 시 `output_data.summary()` 요약 대신 `.porpoise/reports/`에 저장된 실제 마크다운 리포트 파일 전체 내용을 출력
- **태스크 전환 시 cycle 리셋**: Reviewer NEXT로 다음 태스크 또는 새 마일스톤으로 전환될 때 `state.cycle`이 1로 리셋되지 않던 버그 수정 — 신규 포맷·레거시 경로 4개소 모두 적용

### [v0.10.0]
- **`messages/` 폴더 제거**: ClaudeCode 어댑터·마일스톤 세션에서 `messages/` 중복 저장 코드 제거 — 신규 프로젝트에서 폴더 미생성
- **`checkpoint.json` 경로 이동**: `messages/checkpoint.json` → `.porpoise/checkpoint.json` 직접 저장, 구 경로 자동 마이그레이션
- **프롬프트 `reports/` 저장 지시 제거**: `00-orche.tmpl`에서 Claude에게 `reports/` 폴더에 직접 저장하라는 지시 삭제 — JSON session mode 기반으로 정리
- **토큰 제한(LIMIT) 감지**: `You've hit your limit` 패턴 감지 시 `ExitCode::Limit` 처리 → "토큰 한도 도달" 메시지 출력 후 세션 종료
- **LIMIT 세션 캐시 무효화**: 토큰 한도 세션이 캐시되어 재실행 시 LIMIT 메시지만 재표시되던 버그 수정 — 재실행 시 역할 새로 실행
- **RESP/LIMIT 세션 재사용 방지**: `find_latest_session`에서 RESP·LIMIT 세션 skip — 해당 세션 이후 재구동 시 항상 역할 재실행
- **역할 완료 후 보고서 요약 출력**: fresh 실행과 캐시 세션 재개 모두에서 역할 완료 시 요약 최대 15줄 콘솔 출력
- **`porpoise approve` 신규 포맷 안내**: sessions/ 폴더가 있는 신규 프로젝트에서 approve 명령 실행 시 레거시 전용 안내 출력
- **테스트 추가**: 7개 신규 테스트 (총 180개)

### [v0.9.0]
- **API 어댑터 마일스톤 생성**: `run_milestone_via_api` 경로 신설 + `write_milestone_file()` — `anthropic_api`·`openai_compatible` 어댑터도 `claude_code`와 동일하게 `.porpoise/milestones/M{n}.md` 생성 및 `project.md` 갱신
- **PREV→non-PM 세션 캐시 무효화**: `invalidate_sessions_from_role()` — PREV로 특정 역할부터 재시작 시 해당 역할 이후 캐시된 세션 파일 자동 무효화(`.json.prev-invalidated` 확장자 변경)
- **`milestone_complete` 불일치 경고**: Reviewer가 `milestone_complete=true`를 반환했지만 `project.md`에 미완료 작업이 남아 있을 때 경고 출력
- **`--yes` 자동 마일스톤 생성**: 모든 작업 완료 후 `--yes` 플래그이면 프롬프트 없이 자동으로 새 마일스톤 생성 세션 진입 (신규 포맷 및 레거시 경로 모두 적용)
- **새 마일스톤 후 루프 재진입**: 마일스톤 생성 완료 후 `break` 대신 state 업데이트 + `continue`로 즉시 PM 역할 재시작
- **초기화 완료 메시지 수정**: `porpoise --new` 완료 후 "Run porpoise again" 대신 "마일스톤 생성 세션을 시작합니다..." 출력
- **테스트 추가**: 6개 신규 테스트 (총 173개)

### [v0.8.0]
- **`model/context.rs` 공유 모듈 신설**: `build_context_text`, `parse_role_output_from_value`, `try_parse_json_output` 를 `anthropic_api` / `openai_compatible` 어댑터가 공유 — 어댑터 간 동작 불일치 해소
- **마일스톤 정보 컨텍스트 주입**: 모든 어댑터에서 `SessionInput.milestone` (ID·제목·버전·목표)이 실제로 컨텍스트에 포함됨 (이전: 필드만 존재, 미사용)
- **`role_extra` API 어댑터 지원**: `workspace.toml [roles].*_extra` 설정이 `anthropic_api`·`openai_compatible` 시스템 프롬프트에 전달됨 (이전: `claude_code` 어댑터만 지원)
- **`prev_reasons` 체크포인트 영속화**: PREV 피드백 이유가 `checkpoint.json`에 저장·복원됨 (이전: 재시작 시 초기화)
- **모델 템플릿 초기화 선택**: `porpoise init` 시 어댑터 템플릿 목록 제시 및 선택 (Claude Code / Anthropic API / OpenAI Compatible)
- **OPENAI_CODEX `api_base_url` 입력**: `porpoise init` 시 OpenAI 호환 API Base URL 직접 입력 가능
- **JSON 세션 디렉터리 자동 생성**: 신규 초기화 프로젝트에 `.porpoise/sessions/` 자동 생성 → 즉시 JSON 모드 진입
- **IMP-02 경고**: JSON 출력 섹션 누락 프롬프트 파일 감지 시 `porpoise --new` 재실행 안내 출력
- **IMP-03 경고 (`--verbose`)**: `prompt_overrides` 경로 파일 존재 여부 검증
- **컨텍스트 순서 정규화**: 모든 어댑터에서 프로젝트 요약 → 마일스톤 → 기술 스택 → 이전 보고서 순서 일관화
- **테스트 추가**: 7개 신규 테스트 (총 167개)

### [v0.7.1]
- executor 타임아웃 종료 후 좀비 프로세스 회수 (Unix: `wait4`, Windows: `WaitForSingleObject`)

### [v0.7.0]
- **파일 미디에이션**: API 어댑터용 파일 읽기·쓰기·이동·삭제 추상화 레이어 (`workspace/apply.rs`, `workspace/executor.rs`)
- **멀티 모델 지원**: `workspace.toml [models]` 섹션으로 역할별 모델 독립 설정 가능
- **언어·프레임워크 템플릿**: `porpoise init` 시 언어/프레임워크별 보일러플레이트 템플릿 자동 적용
- **WorkspaceSnapshot**: API 어댑터용 프로젝트 파일 스냅샷 지원 (`v0_7` 세션 스키마)

### [v0.6.0]
- **JSON 세션 기반 통신 아키텍처**: 역할 간 데이터를 구조화 JSON 세션 파일(`.porpoise/sessions/`)로 교환
- **멀티 어댑터 지원**: `claude_code`, `anthropic_api`, `openai_compatible` 어댑터 선택 가능
- **`SessionInput` / `RoleOutputData`**: 역할 입출력 타입 정의 및 스키마 기반 tool-use 구조화 응답
- **레거시 호환**: `.porpoise/sessions/` 없는 기존 프로젝트는 `reports/`+`messages/` 기반 레거시 모드 유지

### [v0.5.0]
- **BUG-A 수정**: `parse_tasks_from_project_md`가 마크다운 코드 블록(` ``` `) 내부 라인을 건너뛰도록 개선 — `project.md` 예시 항목이 실제 task로 오파싱되어 마일스톤 세션이 스킵되던 문제 해결
- **BUG-B 수정**: `project.tmpl` 예시 task ID를 `M{n}-T{nn}` 형식으로 변경 — 파서가 인식하지 못하도록 방어
- **초기화 자동 연속**: `porpoise --new` 완료 후 별도 재실행 없이 마일스톤 생성 세션 자동 진입
- **PREV 자동 연속**: `execute_role()` 완료 후 RESP break 대신 루프 재진입 — PREV로 인한 재실행 사이클이 단일 세션에서 자동으로 완주
- **테스트 추가**: 코드 블록 파싱 스킵 2개 (총 99개)

### [v0.4.4]
- **마일스톤 생성 세션 명시적 프롬프트**: `05-milestone.tmpl` 신규 생성 — 파일 경로·형식·파서 요건 명시, `{{next_milestone_id}}` 런타임 변수로 구체적 ID 주입
- **`generator.rs`**: `--new` 시 `05-milestone.md` 자동 생성
- **`milestone_session.rs`**: 프롬프트 `00-orche.md` → `05-milestone.md` 교체, `next_id` 세션 전 계산 후 템플릿 치환, `project.md` 컨텍스트 파일 추가
- **`runner.rs`**: `run_with_prompt_str` 메서드 추가 — 런타임 생성 프롬프트 지원
- **`00-orche.tmpl`**: 마일스톤 파일 형식 참조 섹션 추가

### [v0.4.3]
- **프롬프트 마일스톤 내용 보강**: 모든 역할 템플릿에 마일스톤 생성·작업 진행·규칙 관련 내용 추가
- `00-orche.tmpl`: 마일스톤 & 작업 ID 체계, `completed_tasks`, 마일스톤 완료 3단계 흐름 추가
- `project.tmpl`: `{{language}}` 변수, 마일스톤 & 작업 체계 섹션 추가
- 역할 프롬프트 4종: 보고서 헤더에 `{task-id} / 사이클 {cycle}` 형식, `## 대상 작업` 섹션 추가

### [v0.4.2]
- **프롬프트 파일 확장자 변경**: `src/init/prompts/*.md` → `*.tmpl` — `.gitignore`로 `src/init/prompts/claude.md`가 누락되던 문제 해결
- `claude.md`(미추적 파일)를 `claude.tmpl`로 신규 추가

### [v0.4.1]
- **다중 작업 동시 완료**: Reviewer가 `completed_tasks` 필드로 여러 task ID 일괄 완료 처리 및 커밋
- **자동 커밋 메시지 Markdown 형식**: 제목 `[task-id] 작업 완료`, 본문 항목 목록
- **R-01 안전망**: `completed_tasks`에 현재 task_id 자동 추가 + 경고
- **IMP-01**: `workspace.toml`이 프롬프트 파일보다 최신이면 재생성 안내 경고
- **IMP-02**: `[general].language` 값을 응답 언어로 반영 (기본값: `ko`)
- **IMP-03**: `--verbose` 모드에서 `prompt_overrides` 경로 파일 존재 여부 검증
- **BUG-01**: 빈 변수 치환 후 3연속 개행 → 2개로 정규화

### [v0.4.0]
- **프롬프트 리소스화**: `generator.rs` 하드코딩 문자열을 `src/init/prompts/*.md` 7개 파일로 분리 — `include_str!()` 컴파일 타임 임베딩
- **템플릿 변수 치환 시스템**: `src/init/template.rs` — `{{variable}}` 표기법
- **`.porpoise/workspace.toml` 신설**: 프로젝트별 DoD, 컨벤션, 역할 추가 지시사항, 프롬프트 override 지원
- **WorkspaceConfig 구조체**: `[general]`, `[dod]`, `[conventions]`, `[roles]`, `[prompt_overrides]` 5개 섹션
- **`[roles].*_extra`**: 역할별 추가 지시사항 섹션 자동 삽입

### [v0.3.1]
- **폴더 소유권 분리**: `reports/`(Claude 보고서 저장), `messages/`(Porpoise 출력 캡처), `hints/`(사용자 추가 지시) 역할 확정 및 문서화
- **`porpoise approve [NEXT|PREV]`** 서브커맨드 추가: Claude가 보고서를 저장하지 않은 경우 수동 판정 파일 생성
- **ExitCode 폴백 제거**: `reports/` 파일에 종료 코드가 없으면 NEXT로 폴백하지 않고 명시적 경고 후 중단
- **PREV 복귀 대상 역할 지정**: Reviewer가 `PORPOISE_META` 블록의 `prev_target` 필드로 복귀 역할 지정 가능 (`development` / `testing`)
- **Tester 독립 재검증 지시**: Developer 리포트를 신뢰하지 말고 PM 명세 기준으로 독립 재검증하도록 Tester 프롬프트 강화
- **hint 파일 포함 콘솔 출력**: 역할 실행 시 포함된 hint 파일 목록을 콘솔에 표시
- **상태 복원 폴백 개선**: 체크포인트 없을 때 `messages/`와 `reports/` 양쪽을 모두 참조해 역할 상태 추론
- **컨텍스트 파일 수 제한**: PREV 추가 지시사항 파일 최대 5개로 제한
- **마이그레이션 경고**: 구 버전 `report/` 폴더가 감지되면 `reports/`로 이동 안내 출력
- **CLAUDE.md 최소화**: 생성되는 `CLAUDE.md`를 `project.md` 참조 포인터 한 줄로 단순화
- **`project.md` 강화**: 파일 구조(tree), 폴더 소유권 표, 보고서 파일명 규칙을 단일 소스로 통합
- RESP 코드 처리 시 사용자 답변 직접 수집: 각 질문에 터미널 입력 프롬프트 표시 후 Q&A 쌍을 hint 파일에 저장
- 역할 실행 중 스피너 메시지에 Cycle/Task ID 정보 포함 (`[ Cycle N | M7-T01 ] Running PM ...`)
- 토큰 사용량 모니터(`--token-warn`) 제거 — 불필요한 의존성 및 오경고 원인 삭제
- 오케스트레이터 내 중복 리포트 저장 로직 제거 (`save_report()` 삭제, `runner.rs`의 단일 저장 경로로 통합)

### [v0.3.0]
- RESP 코드 처리 방식 변경: 사용자 입력 대기 없이 질문을 hint 파일(`.porpoise/hints/`)에 저장 후 다음 역할로 자동 진행 — 세션 중단 없는 연속 실행 지원
- 프롬프트 파일에서 RESP 관련 섹션 제거 및 hint 파일 참조 방식으로 전환

### [v0.2.4]
- 토큰 사용량 추정 기준을 최근 5시간 이내 수정 파일로 변경 — 누적 리포트 파일에 의한 오경고 방지
- auto commit 시 `git add` 실패 원인(`stderr`) 에러 메시지에 포함 — 디버깅 가능
- auto commit 대상 파일 목록에서 존재하지 않는 경로 사전 제외 처리 추가

### [v0.2.3]
- Planning 프롬프트에 마일스톤 작업 항목 순차 처리 지침 추가 (위에서부터 하나씩)
- 신규 실행 시 description 입력 단계 제거로 초기화 흐름 간소화
- 생성 파일명 `claude.md` → `CLAUDE.md` 대문자 처리
- auto commit 시 `git ls-files` 기반 스테이징으로 `.gitignore` 파일 명시적 제외
- auto commit 대상 경로에 `wix/` 추가 — 버전 파일(`wix/main.wxs`) 자동 커밋 포함

### [v0.1.2]
- Milestone & task ID system (`M{n}-T{nn}` in `project.md`)
- Role exit code protocol (PREV/NEXT/RESP) — replaces keyword-based heuristics
- Deterministic report filenames (`{task-id}-{role}-C{n}-R{n}.md`)
- Auto git commit on Reviewer NEXT: `[{task-id}] {title}`
- Release flow on milestone completion
- BUG-A fix: Critical keyword mis-detection eliminated
- BUG-B fix: RESP code enforces user input before role re-run
- BUG-C fix: Timestamp-based filename collisions eliminated

### [v0.1.1]
- `is_within_project()` symlink escape fix (parent-chain canonicalize)
- `delete_file` / `delete_dir` / `move_file` helpers with boundary check
- `dry_run` guards on all dialoguer prompts
- `with_context()` on all `create_dir_all` calls
