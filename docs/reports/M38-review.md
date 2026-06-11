# M38 리뷰보고서 (review)

## 비판점

### 차단 (0건)

릴리즈를 막는 이슈 없음. DoD 7항목 충족, 보안 무회귀(Origin·스코프·화이트리스트), 하위호환 유지,
M37 라이브 detach 무회귀 확인.

### 권장 (2건)

1. **`lock_blocks`의 부수효과 (비차단)** — 쿼리형 이름인데 죽은 락 파일을 삭제(부수효과)한다. 동작은
   의도대로(죽은 락 정리)이고 doc 주석에 명시돼 있으나, "판정 함수가 파일을 지운다"는 점은
   유지보수자에게 놀라울 수 있다. 동작·정확성 문제는 아님 — 명명/주석으로 충분. 분리(`is_blocked`
   순수 + 별도 `prune_dead_lock`)는 후속 선택지.

2. **PID 재사용 시 과차단 가능 (희박·보수적)** — 죽은 자식의 PID를 무관한 프로세스가 재사용하면
   `pid_alive`가 true가 되어 새 실행이 막힐 수 있다. 발생 확률이 낮고, 막혀도 **[강제 실행](force)**로
   즉시 우회 가능하며 방향이 보수적(과차단 > 동시 실행)이라 안전. 운영 영향 미미.

### 사소 (3건)

3. **`pid_alive` tasklist 휴리스틱** — Windows에서 `tasklist /FI "PID eq N"` 출력에 PID 문자열
   포함 여부로 판정한다. `/FI`로 해당 PID만 필터되므로 매칭 행이 있을 때만 N이 등장 → 오탐 위험은
   필터 스코프로 차단된다(메모리 열의 우연한 숫자도 매칭 행 내부에만 존재). 안전하나 휴리스틱임은
   명시. (단위 테스트가 실제 `tasklist`로 자기 PID 생존·죽은 PID를 검증.)

4. **force-success·즉시 재실행은 no-claude HTTP 하니스 밖** — 둘 다 실제 spawn을 유발해 무-claude
   하니스에선 비-spawn 분기(409)만 검증. spawn 경로는 단위(`live_pid_lock_blocks_dead_pid_does_not`,
   실 `tasklist`)·`dashboard-launch-live.sh`(Unix) 라이브로 커버됨.

5. **dashboard_port 변경은 실행 중 standalone 대시보드 미반영** — 설정은 다음 내장 기동 기본값에만
   적용(현재 `--port`로 떠 있는 서버는 그대로). 문서에 명시. 의도된 동작.

## 수정 내용

- 리뷰 중 코드 수정 없음(차단·권장 모두 비-수정 처리 가능 — 명명/문서로 충분, 동작 정확).
- 참고: impl 중 `toml_edit` 키 갱신의 주석 소실을 `get_mut` in-place 교체로 이미 수정(테스트 포착).

## 검증

- `cargo build` 경고 0, `cargo clippy` 경고 **0**, `cargo test` **403 passed / 0 failed**.
- M38 핵심 로직 단위 검증: `dashboard_port` 기본·클램프·검증, 선점 락(pid=0) 신선도, **실 PID 생존
  기반 락 판정**(`live_pid_lock_blocks_dead_pid_does_not` — Windows `tasklist`로 자기 PID 생존 +
  죽은 PID 999999 무차단·죽은 락 정리), `parse_force`, **주석 보존**(`apply_updates_preserves_comments_and_order`).
- HTTP 하니스 `dashboard-launch-validate.ps1`(확장): **PASS** — port GET/POST·범위 400, 주석 보존,
  run_active+force→409, 신선한 선점 락→409.
- 라이브 하니스 `dashboard-launch-live.ps1`(Windows 회귀): **PASS** — launch.rs 재작성 후에도
  detached spawn 생존·우아한 정지 무회귀.
- **잔여 리스크**: Unix `process_group(0)` detach·즉시 재실행은 `dashboard-launch-live.sh`로
  운영자 Unix 환경에서 실측(Windows는 단위+라이브로 검증 완료). UI 3요소(포트 입력·강제 실행·재투입
  +실행)의 브라우저 시각 확인은 선택.

## 릴리즈 판정

**가능** — 추천 버전: **v0.35.0 (minor)**

- DoD 7항목 충족: 포트 설정·런 락 PID 정밀화+force·재투입+실행 단일 버튼·설정 주석 보존·보안 무회귀·
  하니스(HTTP+Unix 라이브).
- **하위호환**: `dashboard_port`는 미설정 시 기존 7878, control/launch/config는 기존 동작 보존, UI는
  가산적. 기존 동작 변경 없음 → **minor**.
- 차단 0, 권장 2건은 비차단(명명/희박 케이스), 사소 3건 문서화됨.

## 다음 단계

- **`/tide:release v0.35.0`** 로 릴리즈 진행(버전 범프 → CHANGELOG/README → commit → tag → push).
- 릴리즈 후 권장 후속:
  - Unix `dashboard-launch-live.sh` 실측(운영자 Unix 환경).
  - 차기 마일스톤 후보(M39+): `lock_blocks` 순수/부수효과 분리, `pid_alive` 캐시(고빈도 시),
    적응형 재계획(반복 halt task 분할 — B1), 비용 기반 모델 라우팅(B2), 검증자 비용 계측(B3).
