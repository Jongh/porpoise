# CHANGELOG (Older Releases)

최신 릴리즈는 [README.md](README.md#changelog) 를 참조하세요.

---

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
