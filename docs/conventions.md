# tide 개발 사이클 규약 (porpoise)

이 문서는 tide 워크플로우(`/tide:milestone` → `/tide:impl` → `/tide:review` → `/tide:release`)를
이 저장소에서 운용할 때의 규약을 기록한다. 마일스톤·보고서 **형식**의 단일 원본은 tide 플러그인
각 스킬에 동봉된 `template.md` 이며, 이 문서는 형식을 재정의하지 않는다.

## 단계별 금지행위

| 단계 | git 작업 | 비고 |
|------|----------|------|
| `/tide:milestone` | 금지 | 마일스톤 문서만 생성 |
| `/tide:impl` | **금지** | 구현 + 테스트 + 완료보고서. 커밋·태그·푸시 없음 |
| `/tide:review` | **금지** | 비판적 리뷰 + 리뷰보고서 + 릴리즈 판정. 커밋·태그·푸시 없음 |
| `/tide:release` | 허용 | 버전 범프 → CHANGELOG/README → commit → tag → push |

- git 작업 차단은 tide 플러그인이 hook(tide-guard)으로 직접 제공한다. 프로젝트별 hook 설치는 불필요.

## 상태 파일

- 사이클 상태는 `.tide/` 디렉터리에 기록된다 (현재 단계는 `.tide/phase`).
- `.tide/` 는 **커밋 대상이 아니다** (`.gitignore` 에 등재됨).

## 태스크 표기

- 태스크 ID: 마일스톤 문서 내에서 `T1`, `T2` … 형태로 식별.
- 의존성: `(deps: T1, T2)` 표기로 선행 태스크를 명시한다. deps 가 없는 태스크는 병렬 진행 가능.

## 버전 파일

- 정본 버전: **`Cargo.toml`** 의 `[package] version` (현재 `0.33.0`).
- 릴리즈 시 동기화 대상: `Cargo.toml` → `CHANGELOG.md` / `README.md`(`## CHANGELOG` 섹션) → commit → tag → push.

## 기존 `.porpoise/` 워크플로우와의 공존

- porpoise 는 자체 도구로서 런타임 마일스톤을 `.porpoise/milestones/M*.md` 에 둔다(gitignore 대상, 커밋 안 됨).
- tide 워크플로우의 마일스톤·보고서는 `docs/milestones/`·`docs/reports/` 에 둔다(커밋 대상).
- 두 경로는 별개다. 마일스톤 번호를 정할 때 어느 체계를 쓰는지 혼동하지 말 것.
