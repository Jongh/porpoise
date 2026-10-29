# M41 리뷰보고서 (review)

## 비판점

### 차단 (0건)

없음.

### 권장 (1건)

1. **비-git 자동 폴백 조건 누락** — `README.md` 실행 모드 표와 Overview 문단이 "비-git 저장소는 자동으로 legacy"라고만 적었다.
   실제 `route_decision`(`src/conductor/mod.rs:58-69`)은 **mode 미설정일 때만** `LegacyNonGit`으로 폴백하고, `mode = "conductor"`를
   명시하면 비-git이어도 conductor로 진입해 git 필요 안내로 중단한다. 사용자가 명시 설정 후 비-git에서 실패할 때 문서가 오도한다.

### 사소 (5건)

2. **`doctor` → `/tide:status` 매핑 과장** — `docs/retrospective.md` 이전 표. `/tide:status`는 사이클 상태 보고이며 환경 진단 기능이 없다.
3. **"M25~M29의 버그 대부분" 귀속 부정확** — `docs/retrospective.md` §3. M25(report)·M28(비용 관측)은 기능 추가이고, 라이브에서 드러난 버그 수정은 M26·M27·M29다.
4. **"v0.38.0 최종 릴리즈" 문구가 릴리즈 전에는 미성립** — `Cargo.toml`은 아직 `0.37.0`, 태그 없음. release가 v0.38.0으로 범프해야 사실이 된다(다음 단계에 명시).
5. **README 본문의 현재형 서술** — "자동화합니다" 등. 종료 배너가 최상단에 있어 오해 소지가 낮고, 기록 보존 목적상 본문을 과거형으로 다시 쓰지 않는다(수용).
6. **v0.19.0 릴리즈 커밋 메시지의 "(M10)" 오기** — git 이력의 문제이고 문서(M20)는 CHANGELOG와 일치. 이력 재작성은 하지 않는다(기록).

## 수정 내용

- **이슈 1**: `README.md` 표 legacy 행을 "API 어댑터(항상), mode 미설정 + 비-git 저장소(자동 폴백), 또는 `mode = "legacy"`"로, Overview 괄호를
  "API 어댑터는 항상, mode 미설정 상태의 비-git 저장소는 자동으로 legacy"로 고쳤다.
- **이슈 2**: `docs/retrospective.md` 이전 표 행을 `porpoise status` → `/tide:status`로 좁혔다(doctor 제거 — README 표와 일치).
- **이슈 3**: "M25~M29의 버그 대부분"을 "M26·M27·M29의 버그 수정"으로 고쳤다(fresh 체크아웃 = v0.26.2 M29 보완, CHANGELOG 확인).

## 검증

- 검증 범위: 반증 서브에이전트가 ⑴ `docs/retrospective.md`의 수치·마일스톤 귀속을 `git log`·`git tag`·`CHANGELOG.md`·README CHANGELOG 절과 대조
  ⑵ README·회고의 tide 매핑을 `../tide/README.md`·`../tide/skills/*/SKILL.md`·`../tide/hooks/`와 대조 ⑶ 실행 모드 서술을 `src/config/workspace.rs`·
  `src/conductor/mod.rs`와 대조 ⑷ 설치 명령을 tide README와 대조 ⑸ README의 CHANGELOG 절 **밖** 서술을 훑었다(CHANGELOG 절은 역사 기록이라 대상 밖).
  루트는 porpoise 저장소와 형제 `../tide` 체크아웃. 지적 1은 리뷰어가 `src/conductor/mod.rs:58-69`를 직접 읽어 재확인했다.
- 재검증 결과 (re-verify): 수정 3건 후 상대 링크 존재 검사(`README.md`·`docs/retrospective.md`) 전부 통과, `cargo test` 414 passed / 0 failed.

## 릴리즈 판정

**가능** — 추천 버전: **v0.38.0 (minor)**

계측: in-review 수정 차단 0/권장 1/사소 2 · 반증 시도 수행 성립 6건 · 연속 폴백(fallback-streak) 0 · 부인 기록(precedent-waiver) 0건 · 연속 부인(waiver-streak) 0 · 재작업 라운드(rework) 0

- 완료 기준 7개 모두 충족(impl 보고서 대조표 + 리뷰 수정 후 재검증). 차단 0.
- 코드 무변경이나 프로젝트 상태 전환(종료)의 최종 릴리즈 표지라 minor로 둔다(M41 목표 버전과 동일).
- 반증 시도: 수행 — 성립 6건, 성립한 지적은 위 비판점 섹션에 편입했다.

## 다음 단계

- **`/tide:release v0.38.0`** — `Cargo.toml` 0.37.0 → 0.38.0 범프로 README·회고의 "v0.38.0 최종" 문구를 성립시킨다(이슈 4).
  릴리즈 노트에 "기능 변경 없음 · 프로젝트 종료 · tide 이전"을 명시한다.
- 태그 push로 `release.yml` 바이너리 빌드가 돈다(코드 무변경 재게시 — 최종판으로 수용, impl 후속 메모 1).
- 릴리즈 후: GitHub 저장소 Archive 설정(사용자 몫). 자동 회고가 `docs/reports/retro.md`를 다시 갱신하면 `/tide:release docs`로 커밋.
