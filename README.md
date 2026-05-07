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

1. **Initialization** (first run): Scans project directory, generates `CLAUDE.md` and `.porpoise/` structure
2. **Planning session**: Claude saves a formatted PM report to `.porpoise/reports/`
3. **Development session**: Claude implements code and saves the developer report
4. **Testing session**: Claude tests and saves the tester report
5. **Review session**: Code review → APPROVED / CHANGES_REQUESTED / REJECTED; report saved to `.porpoise/reports/`
6. **Routing**: Porpoise reads the last line of the latest `reports/` file to determine NEXT or PREV

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
    │   └── 04-review.md             # Review session prompt
    ├── reports/                   # Claude's formatted role reports (written by Claude)
    │   ├── {task-id}-planning-C{n}-R{n}.md
    │   ├── {task-id}-development-C{n}-R{n}.md
    │   ├── {task-id}-testing-C{n}-R{n}.md
    │   └── {task-id}-review-C{n}-R{n}.md
    ├── messages/                  # Porpoise's captured output (written by Porpoise)
    │   ├── checkpoint.json
    │   └── {task-id}-{role}-C{n}-R{n}.md
    └── hints/                     # User additional instructions (written by Porpoise on RESP)
        └── {task-id}-{role}-C{n}-R{n}-hints.md
```

### Folder ownership

| Folder | Writer | Purpose |
|--------|--------|---------|
| `reports/` | Claude (exclusive) | Formatted role reports with NEXT/PREV exit code |
| `messages/` | Porpoise (exclusive) | Raw Claude output (questions, summaries, token warnings) |
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

## License

MIT
