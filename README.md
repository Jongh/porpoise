# Porpoise

AI 기반 소프트웨어 개발 오케스트레이션 도구 — Claude Code 또는 OpenAI 호환 API로 **Planning → Development → Testing → Review** 사이클을 자동화합니다.

## Overview

Porpoise는 마일스톤 단위로 개발 워크플로를 오케스트레이션합니다. 각 단계(Planning · Development · Testing · Review)마다 AI가 구조화된 JSON 세션 리포트를 생성하고, 다음 단계는 이를 컨텍스트로 이어받아 실행됩니다. 사용자 개입을 최소화하면서 반복 사이클을 자동으로 완주합니다.

### 실행 모드

Porpoise는 두 가지 실행 모드를 제공합니다.

| 모드 | 동작 | 상태 |
|------|------|------|
| **legacy** (기본) | task당 `Planning → Development → Testing → Review` 4단계를 각각 LLM 호출로 실행하고 JSON 세션으로 이어붙임 | 안정 |
| **conductor** (opt-in, v0.19.0~) | task를 **실제 코딩 에이전트에게 통째로 위임**하고, **독립 검증자**가 실제 테스트 실행 + 적대적 심사로 게이트 | 실험적 |

**conductor 모드**는 "코딩은 에이전트가, 거버넌스(계획·독립 검증·병합·릴리즈)는 Porpoise가" 담당하는 *worker → manager* 방향 전환의 첫걸음입니다. `Brief → Dispatch(격리 git worktree) → Verify(독립 검증) → Integrate(병합·완료)` 4단계로 동작하며, claude_code 어댑터에서 `[conductor] mode = "conductor"`로 활성화합니다. 종단 간 라이브 검증 전까지 기본값은 legacy입니다.

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

# 현재 진행 상황 요약 (마일스톤·태스크·단계·세션 수)
porpoise status

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

# 현재 진행 상황 요약 (마일스톤·태스크·단계·사이클·세션 파일 수)
porpoise status

# 환경 진단 (workspace.toml·어댑터·API 키·CLI 설치 등)
# 실패 항목이 있으면 exit code 1 — CI 헬스체크로 활용 가능
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

### conductor 모드 (opt-in)

`[conductor] mode = "conductor"` (claude_code 어댑터)를 설정하면 위 4단계 phase 호출 대신 task당 다음 흐름으로 동작합니다.

1. **Brief**: project.md · 마일스톤 목표 · DoD · 규약 · 기술 스택을 단일 작업 지시서로 조립
2. **Dispatch**: 격리된 git worktree(`.porpoise/worktrees/<task>`)에서 실제 코딩 에이전트에게 통째로 위임 — 에이전트가 알아서 계획·코딩·테스트
3. **Verify**: **독립 검증자**(설정 시 다른 모델)가 `verify_commands`를 실제 실행하고 diff + DoD를 적대적으로 심사하여 **PASS / FAIL** 판정
4. **Integrate**: PASS면 worktree를 병합하고 task 완료 처리, FAIL이면 검증자 피드백을 덧붙여 **재투입**(`max_redispatch` 한도). 한도 초과 시 사용자 개입 요청

작업을 만든 에이전트가 아닌 *독립 검증자*가 통과를 판정하는 것이 신뢰의 핵심입니다. 병렬 실행·자동 task 분해는 후속 마일스톤에서 다룹니다.

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

[conductor]                    # 지휘자 모드 (claude_code 어댑터 전용)
# mode = "conductor"           # "legacy" (기본) | "conductor" (에이전트 위임 + 독립 검증)
# verifier_model = ""          # 검증자 전용 모델 (생략 시 Dispatch와 동일). 독립성 위해 다른 모델 권장
# max_redispatch = 2           # Verify FAIL 시 재투입 최대 횟수
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

### [v0.27.0]
- **로컬 웹 대시보드 — `porpoise dashboard` (M30, Phase 1)**: conductor가 콘솔로만 보여주던 데이터(실행 리포트·비용/토큰·의존성 그래프)를 **브라우저에서 가시화**. `--port`(기본 7878)·`--no-open` 옵션, `webbrowser`로 자동 오픈. **read-only 관측 전용** — `.porpoise/`에 쓰지 않고 conductor 로직 무변경
- **화면**: 롤업 카드(성공률·재투입·폴백·총비용), 태스크별 비용 막대 차트(PASS 녹색/FAIL 빨강), 실행 리포트 표, **의존성 DAG**(깊이 열 배치, done/ready/waiting 색상). 마일스톤 셀렉터·새로고침
- **JSON API**: `/api/milestones`·`/api/report?milestone=N`·`/api/tasks` — 기존 순수 함수(`build_report`·`ready_tasks`) 재사용, 새 데이터 0
- **단일 바이너리·오프라인**: 프론트엔드(vanilla + 자체 SVG 차트 lib)를 바이너리에 임베드, CDN 의존 0, node 툴체인 없음. 서버는 `tiny_http`(127.0.0.1 전용)
- **라이브 검증**: 합성 데이터(PASS/FAIL/폴백/재투입/conductor-3 비용없음/3단계 DAG)로 브라우저 렌더 ↔ API 응답 ↔ 원본 3층 대조 완전 일치. 스모크 하니스 `scripts/dashboard-smoke.ps1`·문서 `docs/dashboard.md`
- **테스트**: 338개 (331 → 338, +7개)

### [v0.26.2]
- **런타임 디렉터리 보장 위치 수정 (M29 보완)**: v0.26.1의 디렉터리 보장이 `run_conductor` 내부에 있어, 프로젝트 포맷 판별(`is_new_format`, `.porpoise/sessions` 존재로 판별) **이후**라 도달하지 못했다. fresh 체크아웃(`.porpoise/` gitignore로 sessions 비어 있음)에서 정식 프로젝트가 "sessions 폴더가 없습니다"로 미인식되던 문제를 해소 — 보장을 orchestrator 진입부(판별 **이전**)로 이동
- **legacy 안전**: `project.md` 존재 + 비-legacy(`messages/` 없음)일 때만 보장 → 레거시 마이그레이션 안내 경로 보존
- **라이브 검증(M29 종단)**: 런타임 디렉터리 미생성 + untracked Cargo.lock 상태의 fresh 프로젝트에서 conductor가 정상 진입·디렉터리 자동 생성, untracked 병합 충돌을 백업+재시도로 복구하여 MERGED 확인
- **테스트**: 331개 (328 → 331, +3개)

### [v0.26.1]
- **conductor robustness 수정 (M29)**: M28 비용 라이브 검증에서 드러난 두 갭 수정
- **병합 untracked 충돌 견고화**: 에이전트가 worktree에서 생성한 파일(예: `cargo`가 만든 `Cargo.lock`)이 메인의 **untracked 동명 파일**과 충돌해 병합이 하드 실패하던 문제 해소. "untracked overwrite" 유형이면 충돌 파일을 `.porpoise/merge-backup/<ts>/`로 **이동(손실 없음)** 후 재시도하고 위치를 안내. 내용 충돌·기타 실패는 기존 동작(abort) 유지. 순차·병렬 경로 모두 적용
- **런타임 디렉터리 보장**: `.porpoise/{sessions,worktrees,reports}`가 gitignore라 비어 있는 fresh 체크아웃에서 첫 실행이 실패하지 않도록 conductor 시작 시 자동 생성
- **테스트**: 328개 (325 → 328, +3개 — 디렉터리 보장·에러 파싱·실제 git untracked 복구)

### [v0.26.0]
- **비용 관측 + 예산 거버넌스 (M28)**: conductor가 dispatch하는 에이전트의 **비용(USD)·토큰을 캡처**한다. Claude Code를 `--output-format stream-json`으로 실행해 최종 `result` 이벤트의 `total_cost_usd`·`usage`를 파싱(스트리밍 표시 유지). CLI 미지원 시 평문 폴백 + 비용 `None`으로 graceful 저하
- **`porpoise report` 비용 집계**: 태스크별 비용 + 마일스톤 **총비용·총토큰** 롤업(재실행-인지 M27 일관 — 최신 run 비용만). `status`/`doctor`에도 비용·예산 표시
- **예산 상한 (`[conductor] budget_usd`)**: 누적 비용이 상한에 도달하면 다음 dispatch(순차)·배치(병렬) 전에 중단. 미설정·0 이하이면 무제한(기존 동작). 진행 중 task/배치는 마치고 정지
- **감사 기록 conductor-4**: `cost_usd`·`input_tokens`·`output_tokens` 추가(구 기록은 `None`으로 하위호환)
- **라이브 검증**: 실 CLI로 3개 task 실행 → 비용 캡처(세션 cost_usd 실측)·누적 추적·report 롤업(총 $0.3607 등 ground truth 일치) 확인. 하니스 `scripts/conductor-cost-validate.ps1`·런북 `docs/conductor-cost-runbook.md`
- **테스트**: 325개 (316 → 325, +9개)

### [v0.25.2]
- **`porpoise report` 집계 버그 수정 (M27)**: 같은 task를 **재실행하면** 이전 run의 오래된(stale) 레코드가 `sessions/`에 섞여, "최종 라운드 = max redispatch" 기준이 stale FAIL(R2)을 fresh PASS(R0)보다 우선시해 **최종 verdict를 오판**하던 버그를 수정. `aggregate`를 **최신 run 기준**(timestamp 정렬 후 마지막 R0~끝)으로 바꿔 `verdict`·`시도`·`재투입`이 가장 최근 실행만 반영
- **검증**: 회귀 테스트 3개 추가(재실행·다중라운드·정렬 불변식), 동일 sessions에서 report가 M1-T02를 stale FAIL→**정확한 PASS**로 표시함을 라이브 재확인
- M26 검증(report-live 재구동)에서 PASS·MERGED된 task가 report엔 FAIL로 뜨며 드러난 버그
- **테스트**: 316개 (313 → 316, +3개)

### [v0.25.1]
- **변경 감지 버그 수정 (M26)**: 에이전트가 격리 worktree 안에서 **자기 작업을 커밋하면** conductor가 빈 diff로 인식해 정상 작업을 "변경 없음"으로 폐기·halt 하던 비결정적 신뢰성 버그를 수정. `capture_diff`를 현재 HEAD가 아니라 **worktree 분기 base 커밋 기준**(`git diff --cached <base>`)으로 계산하여 커밋 여부와 무관하게 변경을 포착
- **통합 단계 보강**: 위 수정으로 에이전트-커밋 task가 PASS→통합에 진입하게 되면서, clean 작업트리에서 `git commit`이 "nothing to commit"으로 실패하던 2차 결함도 수정 — 스테이징 변경이 없으면 커밋을 건너뛰고 이미 브랜치에 있는 에이전트 커밋을 병합
- **검증**: 커밋 시나리오 회귀 테스트 3개 추가, 순수 git 레벨 재현 하니스(`scripts/conductor-commit-detect-validate.ps1`)로 OLD(HEAD 기준)=빈 값 / NEW(base 기준)=포착 대조 증명. 문서 `docs/conductor-change-detection.md`
- M25 라이브 검증(report-live)에서 M1-T02가 3회 FAIL→halt 되며 드러난 버그
- **테스트**: 313개 (310 → 313, +3개)

### [v0.25.0]
- **함대 실행 리포트 — `porpoise report` (M25)**: conductor가 매 라운드 `sessions/`에 쓰기만 하고 아무도 읽지 않던 감사 기록(conductor-3)을 **마일스톤 실행 요약**으로 집계·가시화. 태스크별 verdict·시도·재투입·폴백 + 마일스톤 롤업(성공률·재투입 합계·폴백 비율)
- **`--milestone N` / `--markdown` / `--out`**: 특정 마일스톤 한정, Markdown 리포트 내보내기(`.porpoise/reports/run-M{N}.md`로 축적 — 릴리즈 노트·회고 근거). `--out` 지정 시 자동 내보내기
- **`porpoise status` 통합**: 최근 실행 1줄 요약(성공률·재투입·폴백) 표시
- **견고성**: 손상·BOM 포함 JSON을 우아하게 스킵(serde가 거부하는 UTF-8 BOM 제거), 빈 입력 무패닉. 파싱/집계는 순수 함수로 분리(M24 패턴 계승)
- **라이브(합성) 검증**: 알려진 감사 기록 주입 → `report` 출력이 ground truth와 정확히 일치(다중 라운드→최종 verdict, 폴백 집계 포함). 하니스 `scripts/conductor-report-validate.ps1`·런북 `docs/conductor-report-runbook.md` 추가
- **테스트**: 310개 (299 → 310, +11개)

### [v0.24.0]
- **계획 두뇌 — 의존성 그래프 스케줄링 (M24)**: task가 `(deps: M1-T01, M1-T02)` 형식으로 선행 task를 선언하면, conductor가 **ready(모든 선행 완료) task만** 배치한다. 선행이 끝나면 다음 라운드에서 의존 task가 ready로 전이 — DAG 기반 위상 실행
- **순환·dangling 검증**: 시작 전 의존성 그래프를 검사해 **순환(cycle)이면 거부**(무한 대기 방지), 존재하지 않는 의존성(dangling)은 **경고 후 무시**(오타가 task를 영구 차단하지 않음). `porpoise doctor`에 의존성 그래프 검증 항목 추가
- **`porpoise status`**: ready task는 `⏳`, 선행 대기 중인 task는 `🔒 (대기: deps)`로 표시
- **deps 전파 수정**: 마일스톤→project.md 미러링 시 `(deps: ...)`를 보존하도록 수정(이전엔 누락되어 스케줄링이 무력화됨). 계획 프롬프트(`05-milestone.tmpl`)에 에이전트 크기 분해·독립 우선·`(deps:)` 작성 가이드 추가
- **라이브 검증(D1)**: max_parallel=3, T03이 T01·T02에 의존 → 라운드 1에 **T01·T02만**(2개) 병렬 실행, 라운드 2에 T03 실행. project.md `(deps:)` 보존·무충돌 완료 확인. PASS
- **테스트**: 299개 (286 → 299, +13개)

### [v0.23.0]
- **병렬 함대 (M23, opt-in)**: `[conductor] max_parallel`(기본 1=순차)을 올리면 독립 task N개를 각자 worktree에서 **동시 dispatch·verify**하고 순차·충돌 인지로 통합. 기본 1이라 무변경
- **낙관적 동시성**: 병합 충돌 시 그 task만 abort 후 **갱신 base에서 재투입**(충돌/실패 피드백을 brief에 주입해 수렴). 시도 한도·무진전 시 안전 중단
- **출력 캡처**: 병렬 실행 출력을 task별 그룹 표시(인터리브 방지). `doctor`에 `병렬: N개` 표시
- **라이브 검증**: 독립(P1)·충돌 수렴(P2) 모두 PASS. 하니스 `scripts/conductor-parallel-validate.ps1`·런북 추가
- **테스트**: 286개 (282 → 286, +4개)

### [v0.22.0]
- **⚠ 기본 동작 변경 — conductor 모드 기본 ON (M22)**: claude_code 어댑터에서 `[conductor].mode` 미설정 시 기본 conductor 루프로 동작. 기존 4단계 방식은 `[conductor] mode = "legacy"`로 opt-out. API 어댑터 무영향. 첫 진입 시 1회 안내
- **비-git 자동 폴백**: 기본 ON이어도 git 저장소가 아니면 자동 legacy 폴백(하드 실패 방지)
- **폴백 정책 `verdict_fallback`**: 검증자 파싱 실패 지속 시 `pass_if_checks_pass`(기본) | `halt`. 폴백 PASS는 `⚠ 경고` + 감사 `fallback_used`(`conductor-3`)
- **라이브 검증 완료**: 정상 경로·안전망 폴백 모두 라이브 3/3. 하니스 `-ForceFallback`·런북 추가. `status`/`doctor`에 실행 모드 표시
- **테스트**: 282개 (270 → 282, +12개)

### [v0.21.0]
- **conductor 검증자 신뢰성 경화 (M21)**: 검증자 LLM 응답 파싱 실패 시 즉시 FAIL 대신 **재질의 1회 → 객관 증거(`verify_commands` 통과) 폴백** — 라이브 테스트에서 관찰된 false-negative FAIL 해소. 출력 형식 강제 강화
- **감사 기록 관측성 (`conductor-2`)**: 검증자 원문·dispatch 출력 포함, 타임스탬프 파일명으로 이력 보존
- **worktree 누수 방지**: 모든 경로(성공·실패·중단)에서 정리 보장 + `.porpoise/` gitignore 자동 보장
- **라이브 재검증 하니스**: `scripts/conductor-revalidate.ps1`·`docs/conductor.md`(승격 기준). conductor 기본 모드는 legacy 유지(opt-in)
- **테스트**: 270개 (259 → 270, +11개)

### [v0.20.0]
- **`porpoise status` 서브커맨드 신설 (M19)**: 현재 마일스톤·태스크·단계·사이클·세션 파일 수를 한 명령으로 출력 — `checkpoint.json`·`milestones/`·`sessions/` 통합 요약
- **`porpoise doctor` 품질 개선 (M19)**: 실패 시 exit code 1(CI 헬스체크 활용), workspace.toml 메시지 어댑터 중복 제거, API 키 힌트 들여쓰기 정렬
- **`is_likely_api_key()` 정밀도 개선 (M19)**: `claude-` 제거(모델명 오진단 해소), `sk-ant-`·`sk-proj-` 추가
- **테스트**: 259개 (252 → 259, +7개)

### [v0.19.0]
- **지휘자(conductor) 루프 신설 (M20)**: AI worker→manager 전환의 첫 단계 — task 하나를 `Brief → Dispatch → Verify → Integrate` 4단계로 처리. 실제 코딩 에이전트에게 격리 git worktree에서 통째로 위임하고, 독립 검증자가 실제 테스트 실행 + 적대적 심사로 PASS/FAIL을 판정, PASS 시 병합·완료, FAIL 시 피드백 재투입(한도 내) — 기존 4단계 phase 호출을 단일 에이전틱 위임이 대체
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
