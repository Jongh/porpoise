# M37 리뷰보고서 (review)

## 비판점

### 차단 (0건)

릴리즈를 막는 이슈 없음. 완료 기준(DoD) 8항목 전부 충족, 보안 경계(Origin·스코프·화이트리스트)
일관 적용, 하위호환 유지.

### 권장 (3건 — 1건 리뷰 중 수정)

1. **함대 실행 TOCTOU 경쟁 (수정 완료)** — `handle_launch`가 `launch_blocked` 확인 후 spawn하고
   **그 뒤** run.lock을 썼다. 두 요청이 거의 동시에 도착하면 둘 다 가드를 통과해 conductor를
   **이중 spawn**할 수 있었다(로컬 단일 사용자라 가능성은 낮지만 명백한 결함). → 리뷰 중 락을
   **spawn 전에 선점**(pid=0 기록)하고 spawn 실패 시 롤백하도록 수정. 두 번째 동시 요청은
   신선한 선점 락을 보고 409.

2. **detached spawn 실 수명 분리 (검증 완료)** — 단위 테스트·HTTP 하니스는 가드(403/404/409)만
   덮고 spawn 성공 경로를 띄우지 않았다. 이를 **라이브 검증 스크립트**(`scripts/dashboard-launch-live.ps1`)로
   실측: gate 모드 샌드박스(approval_mode="gate", serve_dashboard=false)에서 [함대 실행]→ 자식이
   첫 task 게이트에서 블록(dispatch 전 → **비용 0**) → **대시보드 강제 종료 후 자식 생존 확인**
   (detach 증명) → stop-next로 우아한 정지. Windows `CREATE_NEW_PROCESS_GROUP`+`CREATE_NO_WINDOW`
   경로에서 PASS. (Unix `process_group(0)`는 동일 std 메커니즘 — 별도 실측은 후속.)

3. **sub-30초 런 직후 재실행 일시 차단** — run.lock은 30초 시간 기반 자가 만료다. 런이 30초 안에
   끝나면 종료 후 남은 시간 동안 새 [함대 실행]이 409로 막힌다(live run_active는 false이나 락이
   아직 신선). conductor가 run.lock을 모르는 무결합 설계의 trade-off. 보수적 동작이라 데이터
   안전엔 무해. 필요 시 `live::finish`에서 락 제거를 후속 검토(단, 결합 도입 주의).

### 사소 (3건)

4. **설정 편집 시 주석 손실** — workspace.toml을 toml round-trip(파싱→재직렬화)으로 갱신하므로
   데이터는 보존되나 사용자 주석은 사라진다. impl 보고서·문서·코드 주석에 명시된 알려진
   트레이드오프. 기본 템플릿이 주석 다수라 체감 가능 — 관리 섹션 분리는 후속.

5. **병렬 경로의 무해한 오버라이드 소비** — `run_parallel`이 매 루프 ready task 전체에 대해
   `consume_override`를 호출한다. halt가 아닌 task엔 오버라이드 파일이 없어 None을 반환하므로
   기능상 무해하나, 의미상 "halt 재투입"이 아닌 task까지 훑는다. 가독성 차원의 사소점.

6. **재투입→실행 2스텝** — 실행 중 함대로의 핫-재큐 미지원으로 [재투입] 후 [함대 실행]을 따로
   눌러야 한다. 마일스톤이 명시한 범위(다음 실행에서 효력)와 일치. 단일 버튼 통합은 후속 여지.

## 수정 내용

- **이슈 1**: `src/dashboard/launch.rs` `handle_launch` — run.lock을 spawn 전에 선점(pid=0)하고
  spawn 실패 시 `run_lock_path` 제거로 롤백. 동시 요청의 이중 기동 창을 제거.

## 검증

- 수정 후 `cargo build` 경고 0, `cargo clippy` 경고 **0**, `cargo test` **396 passed / 0 failed**
  (M37 신규 25개 포함).
- HTTP 하니스 `scripts/dashboard-launch-validate.ps1`: **PASS** (재투입 채널·런 락 409·Origin 403·
  미등록 404·설정 GET/POST 왕복·화이트리스트 위반 400 무쓰기·범위/열거 위반 400). TOCTOU 수정은
  하니스가 건드리지 않는 spawn 성공 경로에만 영향 → 하니스 결과 불변(별도 재실행 불요).
- **라이브 검증** `scripts/dashboard-launch-live.ps1`: **PASS** — 실제 git 샌드박스에서 [함대 실행]→
  자식 게이트 블록(비용 0)→ **대시보드 종료 후 자식 생존(detach 증명)**→ 우아한 정지. 권장 2 해소.
- **잔여 리스크**: Windows 경로는 실측 완료. Unix(`process_group(0)`)는 동일 std 메커니즘이나 별도
  실측 미수행 — 후속. UI 3버튼의 브라우저 시각 확인은 라이브 스크립트가 체크리스트로 안내(선택).

## 릴리즈 판정

**가능** — 추천 버전: **v0.34.0 (minor)**

- DoD 8항목 충족: detached 실행·런 락 409·M35 공존 회귀 없음·halt 재투입·설정 편집 검증·신규
  엔드포인트 보안 상속·시각 회귀 없음(esc)·단위+하니스 테스트.
- **하위호환**: 신규 엔드포인트(`/api/launch`·`/api/config`)·control `redispatch` decision·UI 버튼은
  모두 가산적이다. live.json 스키마 무변경, 설정 쓰기는 기존 키만 갱신, console·`--yes` 의미 보존.
  기존 동작 변경 없음 → **minor**가 정확(major 아님).
- 차단 이슈 0, 권장 1건은 리뷰 중 수정 완료, 나머지는 비차단·문서화됨.

## 다음 단계

- **`/tide:release v0.34.0`** 로 릴리즈 진행(버전 범프 → CHANGELOG/README → commit → tag → push).
  git 작업은 release 단계 전용이므로 이 사이클에서는 수행하지 않음.
- 릴리즈 후 권장 후속:
  - 운영자 라이브 검증: 대시보드 [함대 실행] → 실제 런 기동 → 대시보드 종료 후 자식 생존 확인.
  - 차기 마일스톤 후보: 재투입→실행 단일 버튼 통합, run.lock의 `live::finish` 연동(무결합 유지선),
    설정 편집 주석 보존(관리 섹션 분리).
