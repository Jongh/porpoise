# Porpoise

Software development orchestration tool powered by Claude Code.

## Overview

Porpoise automates the full software development workflow by orchestrating **Planning → Development → Testing → Review** session cycles using Claude Code. It generates structured reports between sessions to maintain context continuity and minimizes user interruptions.

## Installation

### Windows

Download `porpoise-*.msi` from [Releases](https://github.com/Jongh/porpoise/releases) and run the installer. `porpoise` will be added to your PATH automatically.

### Ubuntu/Debian

```bash
sudo dpkg -i porpoise_*.deb
```

### RHEL/Fedora

```bash
sudo rpm -i porpoise-*.rpm
```

### macOS / Linux

```bash
tar xzf porpoise-*.tar.gz
sudo mv porpoise /usr/local/bin/
```

### Build from source

```bash
cargo build --release
```

## Usage

```bash
# Auto-detect mode: resume existing project or initialize new one
porpoise

# Force new initialization
porpoise --new

# Start from a specific session
porpoise --from development   # planning | development | testing | review

# Dry run (show plan without executing)
porpoise --dry-run

# Verbose output
porpoise --verbose

# Create a manual verdict for the current role (when Claude did not save to reports/)
porpoise approve NEXT
porpoise approve PREV

# Archive old messages
porpoise clean [--days N] [--dry-run]
```

## How it works

1. **Initialization** (`porpoise --new`): Scans project directory, generates `CLAUDE.md` and `.porpoise/` structure, and creates `.porpoise/sessions/` so new projects run in JSON session mode.
2. **Milestone session**: User describes the next milestone; Claude creates `.porpoise/milestones/M{n}.md` with task list.
3. **Planning session**: Claude/adapter writes structured Planning output to `.porpoise/sessions/`.
4. **Development session**: Claude/adapter implements code and writes structured Development output to `.porpoise/sessions/`.
5. **Testing session**: Claude/adapter tests and writes structured Testing output to `.porpoise/sessions/`.
6. **Review session**: Code review → NEXT / PREV / RESP; structured Review output is saved to `.porpoise/sessions/`.
7. **Routing**: For new projects, Porpoise routes from JSON session status. The legacy `reports/`/`messages/` router remains only for older projects that do not have `.porpoise/sessions/`.

Checkpoints enable resuming after interruption. Use `porpoise approve NEXT|PREV` to create a manual verdict when Claude did not save a report.

## File structure (generated)

```
{project}/
├── CLAUDE.md                      # Pointer to .porpoise/project.md
└── .porpoise/
    ├── project.md                 # Full project context (file tree, conventions, folder ownership)
    ├── prompts/
    │   ├── 00-orche.md              # Master orchestrator prompt
    │   ├── 01-planning.md           # Planning session prompt
    │   ├── 02-development.md        # Development session prompt
    │   ├── 03-testing.md            # Testing session prompt
    │   ├── 04-review.md             # Review session prompt
    │   └── 05-milestone.md          # Milestone creation session prompt
    ├── milestones/                # Milestone definition files (written by Claude)
    │   └── M{n}.md
    ├── sessions/                  # JSON session mode outputs (new projects)
    │   ├── {task-id}-planning-C{n}-R{n}.json
    │   ├── {task-id}-development-C{n}-R{n}.json
    │   ├── {task-id}-testing-C{n}-R{n}.json
    │   └── {task-id}-review-C{n}-R{n}.json
    ├── reports/                   # Legacy formatted role reports
    ├── messages/                  # Legacy captured output and checkpoint data
    │   └── checkpoint.json
    └── hints/                     # User additional instructions (written by Porpoise on RESP)
        └── {task-id}-{role}-C{n}-R{n}-hints.md
```

### Folder ownership

| Folder | Writer | Purpose |
|--------|--------|---------|
| `sessions/` | Porpoise/model adapter | JSON session envelopes used by new projects |
| `reports/` | Claude (exclusive) | Legacy formatted role reports with NEXT/PREV exit code |
| `messages/` | Porpoise (exclusive) | Legacy raw Claude output (questions, summaries, token warnings) |
| `hints/` | Porpoise (RESP flow) | User-provided additional instructions |

## Exit codes (role protocol)

Each role appends one of these codes as the **last line** of its `reports/` file:

| Code | Meaning | Orchestrator action |
|------|---------|---------------------|
| `NEXT` | Role complete, proceed | Advance to next role (Reviewer NEXT → auto-commit) |
| `PREV` | Previous role needs rework | Restart from target role (`prev_target` in META block) or Planning |

### PREV target routing

Reviewers can specify which role to restart from using the `PORPOISE_META` block:

```markdown
<!-- PORPOISE_META
status: CHANGES_REQUESTED
prev_target: development
-->
```

Allowed values for `prev_target`: `development`, `testing`. Omit to restart from Planning (default).

## CHANGELOG

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
- **API 마일스톤 역직렬화 수정**: `MilestoneOutput.role`에 `#[serde(default)]` 추가 — `anthropic_api`·`openai_compatible` 어댑터에서 마일스톤 생성 시 `missing field 'role'` 오류로 중단되던 버그 수정, 빈 문자열 역직렬화 시 `"milestone"`으로 자동 보정
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

---

이전 버전 릴리즈 내역은 [CHANGELOG.md](CHANGELOG.md)를 참조하세요.

## License

MIT
