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

### [v0.5.0]
- **BUG-A 수정**: `parse_tasks_from_project_md`가 마크다운 코드 블록(` ``` `) 내부 라인을 건너뛰도록 개선 — `project.md` 예시 항목이 실제 task로 오파싱되어 마일스톤 세션이 스킵되던 문제 해결
- **BUG-B 수정**: `project.tmpl` 예시 task ID를 `M{n}-T{nn}` 형식으로 변경 — 파서가 인식하지 못하도록 방어 (BUG-A와 중복 방어)
- **초기화 자동 연속**: `porpoise --new` 완료 후 별도 재실행 없이 마일스톤 생성 세션 자동 진입
- **PREV 자동 연속**: `execute_role()` 완료 후 RESP break 대신 루프 재진입 — PREV로 인한 재실행 사이클이 단일 세션에서 자동으로 완주 (Claude가 `reports/`에 보고서를 저장하지 않은 경우에는 기존 RESP break 동작 유지)
- **테스트 추가**: 코드 블록 파싱 스킵 2개 (총 99개)

### [v0.4.4]
- **마일스톤 생성 세션 명시적 프롬프트**: `05-milestone.tmpl` 신규 생성 — 파일 경로·형식·파서 요건 명시, `{{next_milestone_id}}` 런타임 변수로 구체적 ID 주입
- **`generator.rs`**: `--new` 시 `05-milestone.md` 자동 생성
- **`milestone_session.rs`**: 프롬프트 `00-orche.md` → `05-milestone.md` 교체, `next_id` 세션 전 계산 후 템플릿 치환, `project.md` 컨텍스트 파일 추가
- **`runner.rs`**: `run_with_prompt_str` 메서드 추가 — 런타임 생성 프롬프트 지원, `build_prompt_from_content` / `execute_claude` 내부 분리
- **`00-orche.tmpl`**: 마일스톤 파일 형식 참조 섹션 추가

### [v0.4.3]
- **프롬프트 마일스톤 내용 보강**: 모든 역할 템플릿에 마일스톤 생성·작업 진행·규칙 관련 내용 추가
- `00-orche.tmpl`: 마일스톤 & 작업 ID 체계, `completed_tasks`, 마일스톤 완료 3단계 흐름 추가
- `project.tmpl`: `{{language}}` 변수, 마일스톤 & 작업 체계 섹션(ID 형식·진행 규칙·완료 흐름) 추가
- 역할 프롬프트 4종: 보고서 헤더에 `{task-id} / 사이클 {cycle}` 형식, `## 대상 작업` 섹션 추가

### [v0.4.2]
- **프롬프트 파일 확장자 변경**: `src/init/prompts/*.md` → `*.tmpl` — `.gitignore` 규칙 `claude.md`에 의해 `src/init/prompts/claude.md`가 커밋에서 제외되던 문제 해결
- `claude.md`(미추적 파일)를 `claude.tmpl`로 신규 추가

### [v0.4.1]
- **다중 작업 동시 완료**: Reviewer가 `PORPOISE_META` 블록의 `completed_tasks` 필드로 여러 task ID를 쉼표 구분 지정 시 일괄 완료 처리 및 일괄 커밋
- **자동 커밋 메시지 Markdown 형식**: 제목 `[task-id] 작업 완료`, 본문 `- task-id: 제목` 항목 목록
- **R-01 안전망**: `completed_tasks`에 현재 task_id가 없으면 자동 추가 + 경고 출력
- **R-05 경고**: `completed_tasks`의 task ID가 project.md에 없으면 콘솔 경고
- **IMP-01**: 오케스트레이터 시작 시 `workspace.toml`이 프롬프트 파일보다 최신이면 재생성 안내 경고
- **IMP-02**: `workspace.toml`의 `[general].language` 값을 `project.md` 응답 언어로 반영 (기본값: `ko`)
- **IMP-03**: `--verbose` 모드에서 `prompt_overrides` 경로 파일 존재 여부 검증
- **BUG-01**: `apply_template()`에서 빈 변수 치환 후 3연속 개행 → 2개로 정규화

### [v0.4.0]
- **프롬프트 리소스화**: `generator.rs` 하드코딩 문자열을 `src/init/prompts/*.md` 7개 파일로 분리 — `include_str!()` 컴파일 타임 임베딩으로 단일 바이너리 유지
- **템플릿 변수 치환 시스템**: `src/init/template.rs` 추가 — `{{variable}}` 표기법, `str::replace()` 기반, 미치환 변수 경고 출력
- **`.porpoise/workspace.toml` 신설**: 프로젝트별 DoD, 컨벤션, 역할 추가 지시사항, 프롬프트 완전 교체(override) 지원
- **WorkspaceConfig 구조체**: `[general]`, `[dod]`, `[conventions]`, `[roles]`, `[prompt_overrides]` 5개 섹션, TOML 로드 지원
- **`[prompt_overrides]` 하이브리드**: 역할별 커스텀 `.md` 파일 경로 지정 시 기본 템플릿 대신 사용, 파일 없으면 기본값 폴백 + 경고
- **`[roles].*_extra`**: pm/developer/tester/reviewer 각 역할 프롬프트에 추가 지시사항 섹션 자동 삽입

---

이전 버전 릴리즈 내역은 [CHANGELOG.md](CHANGELOG.md)를 참조하세요.

## License

MIT
