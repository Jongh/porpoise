# 프로젝트 컨텍스트 (porpoise)

`/tide:milestone`·`/tide:impl` 이 매번 코드베이스를 재조사하지 않도록 구조를 요약한 참조 문서.
권위 있는 개발 가이드는 루트 [CLAUDE.md](../CLAUDE.md), 사용자 문서는 [README.md](../README.md).

## 개요

Claude Code 기반 소프트웨어 개발 오케스트레이션 도구 — "에이전트 함대 지휘자" Rust CLI.
마일스톤 계획·DoD 강제·독립 검증·감사 추적·릴리즈 같은 **거버넌스(외부 루프)**를 담당하고,
실제 코딩(내부 루프)은 코딩 에이전트에게 위임한다.

## 스택 · 언어 · 주요 의존성

- 언어: **Rust** (edition 확인 필요 — `Cargo.toml`), 단일 바이너리 `porpoise`.
- 현재 버전: **0.33.0** (`Cargo.toml`).
- 주요 의존성: `clap`(CLI), `serde`/`serde_json`/`toml`(설정·세션 직렬화), `anyhow`(에러),
  `chrono`(시간), `colored`/`indicatif`/`dialoguer`(터미널 UX), `which`(실행 파일 탐지),
  `ureq`(HTTP 클라이언트, API 어댑터), `tiny_http`(대시보드 웹서버), `webbrowser`(대시보드 기동),
  `walkdir`/`ignore`(파일 순회).

## 최상위 디렉터리 구조 (src/)

| 경로 | 역할 |
|------|------|
| `main.rs` | CLI 진입점(clap). 서브커맨드: `doctor` `status` `migrate` `update` `approve` `clean` 등 |
| `orchestrator/` | legacy 4단계 루프(Planning→Development→Testing→Review), checkpoint, 모드 라우팅 |
| `conductor/` | 지휘자 루프 — `brief`·`dispatch`(격리 git worktree)·`verify`(독립 검증)·`integrate`·`git` |
| `dashboard/` | 웹 대시보드 — 멀티 프로젝트 관측·게이트 제어(task 승인·정지), `tiny_http` 기반 |
| `model/` | 어댑터(`claude_code`·`anthropic_api`·`openai_compatible`) + `factory` |
| `session/` | JSON 세션 스키마(planning/development/testing/review/milestone) + 렌더러 |
| `config/` | `workspace.toml`(WorkspaceConfig) / `porpoise.toml`(Config) 파싱 |
| `milestone/` | 마일스톤 파서·업데이터 |
| `workspace/` | 명령 실행기(verify), 스냅샷, 파일 적용 |
| `init/` | `porpoise --new` 초기화·임베디드 템플릿 |
| `claude/` | claude CLI 스폰 — `run_with_prompt_str`(단발), `run_agentic`(풀 에이전틱) |
| `doctor.rs` / `status.rs` | 환경 진단 / 진행 상황 출력 |
| `logger.rs` | 로깅 |
| `utils/` | `fs`(경계 검사 `write_file`)·`input`·`deps`·`error` |

## 기타 디렉터리

- `docs/` — 설계·런바인 문서(conductor·dashboard 관련) + tide 마일스톤/보고서.
- `scripts/` — 검증/스모크 PowerShell 스크립트(conductor·dashboard).
- `wix/` — Windows MSI 패키징(WiX).
- `.porpoise/` — porpoise 자체 런타임 데이터(마일스톤 포함, gitignore 대상).

## 진입점 · 빌드 · 테스트

- 빌드: `cargo build` (릴리즈: `cargo build --release`).
- 테스트: `cargo test` — CLAUDE.md 기준 **259개** (실제 수는 실행으로 확인 필요).
- 린트: `cargo clippy` — 신규 경고 0개 유지가 규약.
- 진입점: `src/main.rs` (clap CLI).

## 핵심 도메인 개념 · 용어

- **두 실행 모드**:
  - *legacy(기본)* — task당 Planning→Development→Testing→Review 4단계 LLM 호출 + JSON 세션 핸드오프.
  - *conductor(opt-in, v0.19.0~)* — task를 코딩 에이전트에게 위임, 격리 worktree 에서 작업 후 독립 검증으로 게이트.
    `[conductor] mode = "conductor"` 설정 시 활성(claude_code 어댑터 전용).
- **마일스톤** — porpoise 자체 운용 시 `.porpoise/milestones/M*.md`(gitignore). tide 운용 시 `docs/milestones/`.
- **게이트(gate)** — task 승인·정지 제어 지점. v0.33.0 기준 웹 대시보드에서 무터미널로 제어 가능.
- **어댑터(adapter)** — LLM 백엔드 추상화(`claude_code`·`anthropic_api`·`openai_compatible`),
  `model/factory.rs` 가 생성. API 어댑터는 항상 legacy.

## 확인 필요

- Rust edition / 정확한 테스트 개수 — 빌드·테스트 실행으로 확인.
- 서브커맨드 전체 목록 — `main.rs` clap 정의로 확인(위 목록은 CLAUDE.md 기준 일부).
