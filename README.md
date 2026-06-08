# Porpoise

AI 기반 소프트웨어 개발 오케스트레이션 도구 — Claude Code 또는 OpenAI 호환 API로 **Planning → Development → Testing → Review** 사이클을 자동화합니다.

## Overview

Porpoise는 마일스톤 단위로 개발 워크플로를 오케스트레이션합니다. 각 단계(Planning · Development · Testing · Review)마다 AI가 구조화된 JSON 세션 리포트를 생성하고, 다음 단계는 이를 컨텍스트로 이어받아 실행됩니다. 사용자 개입을 최소화하면서 반복 사이클을 자동으로 완주합니다.

**지원 어댑터**

| 어댑터 | 설명 | 대표 모델 |
|--------|------|-----------|
| `claude_code` | Claude Code CLI 직접 실행 (권장) | Claude Sonnet 4.x |
| `anthropic_api` | Anthropic Messages API 직접 호출 | claude-sonnet-4-6 |
| `openai_compatible` | OpenAI 호환 API | Groq · Gemini · OpenAI · Ollama |

## Installation

### Windows

[Releases](https://github.com/Jongh/porpoise/releases)에서 `porpoise-*.msi`를 다운로드 후 실행하면 PATH가 자동으로 등록됩니다.

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

## Quick Start

```bash
# 새 프로젝트 초기화 (어댑터·모델 선택 포함)
porpoise --new

# 기존 프로젝트 재개
porpoise

# 환경 설정 진단 (API 키·CLI·sessions/ 등 한 번에 확인)
porpoise doctor
```

## Usage

```bash
# 자동 감지: 기존 프로젝트 재개 또는 신규 초기화
porpoise

# 새 프로젝트 강제 초기화
porpoise --new

# 특정 단계부터 시작
porpoise --from development   # planning | development | testing | review

# 드라이런 (실행 계획만 출력)
porpoise --dry-run

# 상세 출력 (AI 원문·리포트 포함)
porpoise --verbose

# 환경 진단 (workspace.toml·어댑터·API 키·CLI 설치 등)
porpoise doctor

# 수동 판정 파일 생성 (레거시 프로젝트용)
porpoise approve NEXT
porpoise approve PREV

# 프롬프트 파일만 재생성 (어댑터 변경 후)
porpoise update prompt

# 모델·언어 설정 재선택
porpoise update config

# 레거시 프로젝트를 JSON 세션 포맷으로 전환
porpoise migrate

# 오래된 메시지 파일 아카이브 (레거시)
porpoise clean [--days N] [--dry-run]
```

## How it works

1. **초기화** (`porpoise --new`): 프로젝트 디렉토리를 스캔하고 `CLAUDE.md`와 `.porpoise/` 구조를 생성합니다. 어댑터·모델·언어를 대화식으로 선택하고, API 어댑터 선택 시 환경변수 설정 안내를 출력합니다.
2. **마일스톤 세션**: 사용자가 목표를 설명하면 AI가 `.porpoise/milestones/M{n}.md` 태스크 목록을 작성합니다.
3. **Planning 세션**: AI가 구현 계획·DoD·리스크를 구조화 JSON으로 `.porpoise/sessions/`에 저장합니다.
4. **Development 세션**: AI가 코드를 구현하고 파일 변경 내역을 세션에 기록합니다.
5. **Testing 세션**: AI가 테스트를 실행하고 결과를 세션에 기록합니다.
6. **Review 세션**: AI가 코드 리뷰 후 NEXT / PREV / RESP를 반환합니다. NEXT 시 자동 커밋, PREV 시 지정 단계부터 재시작합니다.
7. **라우팅**: `.porpoise/sessions/`가 존재하면 JSON 세션 모드로 동작합니다. `sessions/`가 없는 레거시 프로젝트는 `reports/`+`messages/` 기반 모드를 유지합니다 (`porpoise migrate`로 전환 가능).

중단 후 재실행 시 체크포인트에서 자동 재개됩니다.

## Configuration (`workspace.toml`)

초기화 시 `.porpoise/workspace.toml`이 생성됩니다. 주요 설정:

```toml
[general]
language = "ko"                # 응답 언어

[model]
adapter = "claude_code"        # claude_code | anthropic_api | openai_compatible
model_id = "claude-sonnet-4-6" # API 어댑터 사용 시
api_key_env = "ANTHROPIC_API_KEY"  # 환경변수 이름 (키 값이 아님)
api_base_url = "https://api.openai.com/v1"  # openai_compatible 전용

[tech]
test_command = "cargo test"
verify_commands = [
    { command = "cargo", args = ["clippy"] }
]

[sessions]
# keep_completed_milestone_sessions = false  # true: 완료 세션 파일 보존
# max_session_age_days = 30                  # 0 = 무제한
```

## File structure (generated)

```
{project}/
├── CLAUDE.md                      # .porpoise/project.md 포인터
└── .porpoise/
    ├── project.md                 # 프로젝트 컨텍스트 (파일 트리·컨벤션·폴더 소유권)
    ├── workspace.toml             # 어댑터·모델·DoD·컨벤션·기술 스택 설정
    ├── checkpoint.json            # 현재 태스크·사이클 상태 (오케스트레이터 기록)
    ├── prompts/
    │   ├── 00-orche.md
    │   ├── 01-planning.md
    │   ├── 02-development.md
    │   ├── 03-testing.md
    │   ├── 04-review.md
    │   └── 05-milestone.md
    ├── milestones/
    │   └── M{n}.md                # AI가 작성하는 마일스톤 정의
    ├── sessions/                  # JSON 세션 파일 (신규 포맷)
    │   └── {task-id}-{role}-C{n}-R{n}.json
    ├── reports/                   # 레거시 마크다운 리포트
    ├── messages/                  # 레거시 원문 출력 및 체크포인트
    └── hints/                     # RESP 시 사용자 추가 지시사항
        └── {task-id}-{role}-C{n}-R{n}-hints.md
```

### Folder ownership

| 폴더 | 작성자 | 목적 |
|------|--------|------|
| `sessions/` | Porpoise/어댑터 | JSON 세션 엔벨로프 (신규 프로젝트) |
| `reports/` | Claude (레거시) | 역할별 NEXT/PREV 판정 포함 마크다운 리포트 |
| `messages/` | Porpoise (레거시) | 원문 Claude 출력 캡처 |
| `hints/` | Porpoise (RESP 흐름) | 사용자 추가 지시사항 |

## Exit codes (role protocol)

| 코드 | 의미 | 오케스트레이터 동작 |
|------|------|---------------------|
| `NEXT` | 단계 완료, 진행 | 다음 단계로 이동 (Reviewer NEXT → 자동 커밋) |
| `PREV` | 이전 단계 재작업 필요 | `prev_target` 단계부터 재시작 |
| `RESP` | 사용자 답변 요청 | 질문을 hints 파일에 저장 후 재실행 |

### PREV target routing

```markdown
<!-- PORPOISE_META
status: CHANGES_REQUESTED
prev_target: development
-->
```

`prev_target` 허용 값: `development`, `testing`. 생략 시 Planning부터 재시작.

## CHANGELOG

### [v0.19.0]
- **지휘자(conductor) 루프 신설 (M10)**: AI worker→manager 전환의 첫 단계 — task 하나를 `Brief → Dispatch → Verify → Integrate` 4단계로 처리. 실제 코딩 에이전트에게 격리 git worktree에서 통째로 위임하고, 독립 검증자가 실제 테스트 실행 + 적대적 심사로 PASS/FAIL을 판정, PASS 시 병합·완료, FAIL 시 피드백 재투입(한도 내) — 기존 4단계 phase 호출을 단일 에이전틱 위임이 대체
- **`[conductor]` 설정**: `mode`(기본 `legacy`, opt-in `conductor`)·`verifier_model`·`max_redispatch`(기본 2) — claude_code 어댑터 전용. 기본은 legacy로 기존 동작 100% 보존, conductor는 명시적 opt-in
- **`src/conductor/` 모듈 신설**: brief·dispatch·verify·integrate·git 헬퍼. `ClaudeRunner::run_agentic`(작업 디렉토리 지정 풀 에이전틱 실행), checkpoint `conductor_phase` 필드, `porpoise doctor` conductor 진단 추가
- **테스트**: 252개 (215 → 252, +37개)

### [v0.18.0]
- **`AnthropicApiAdapter` `api_key_env` 준수**: `workspace.toml`의 `api_key_env` 설정이 `anthropic_api` 어댑터에서 무시되던 버그 수정 — 어댑터 생성 시 설정된 환경변수 이름을 실제로 사용, `ANTHROPIC_API_KEY` 하드코딩 제거
- **`is_likely_api_key()` 정밀도 개선**: 소문자 포함 문자열 전체를 "API 키"로 오진단하던 로직 제거 — `AIzaSy`, `sk-`, `gsk_`, `xai-`, `claude-` 접두사 기반으로 감지 범위 축소
- **`porpoise doctor` 서브커맨드 신설**: 설정 진단 명령 추가 — workspace.toml·어댑터·Claude CLI·API 키·Ollama·sessions/·마일스톤 7개 항목 점검, 실패 항목에 OS별 해결 안내
- **테스트**: 215개

### [v0.17.0]
- **API 키 환경변수명 입력 검증**: `api_key_env` 입력 시 형식 검증 + 실제 키 값 패턴 감지(3-retry 경고) — 키 값을 환경변수명 필드에 입력하는 오류 방지
- **초기화 후 OS별 환경변수 설정 안내**: `print_api_key_env_guide()` — 초기화·`update config` 완료 후 PowerShell·Unix 설정 명령 자동 출력
- **어댑터 생성 전 API 키 사전 검증**: `factory.rs`에서 어댑터 생성 전 env var 존재 확인 — 즉각적인 명확한 에러 출력
- **Gemini 기본 모델**: `gemini-2.0-flash` → `gemini-2.5-flash`
- **Dead code 경고 0개**: 미사용 함수·메서드·필드 제거
- **테스트**: 207개

### [v0.16.0]
- **`porpoise migrate` 서브커맨드 신설**: 레거시 프로젝트를 JSON 세션 포맷으로 전환
- **`legacy.rs` 삭제**: MD 기반 레거시 오케스트레이터 코드 경로 완전 제거
- **`cleanup_sessions`**: `workspace.toml [sessions]` 정책 기반 세션 파일 자동 정리
- **Snapshot git diff 라인 제한**: `GIT_DIFF_MAX_LINES = 200`
- **테스트**: 204개

전체 릴리즈 내역은 [CHANGELOG.md](CHANGELOG.md)를 참조하세요.

## License

MIT
