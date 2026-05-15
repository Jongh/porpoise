# CHANGELOG (Older Releases)

최신 릴리즈는 [README.md](README.md#changelog) 를 참조하세요.

---

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
