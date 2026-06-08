# conductor 라이브 재검증 런북 (M22 기본 ON 승격 전)

이 문서는 v0.22.0(conductor 기본 ON) 릴리즈 **전에** 사용자가 직접 수행해야 하는 라이브 검증
작업을 미리 정의한다. 두 작업(V1·V2) 모두 통과해야 릴리즈한다.

라이브 실행은 실제 `claude`를 호출하므로(토큰 소모) 자동화하지 않는다. 하니스가 스캐폴딩·집계·
상세 로그를 자동 처리하고, 사용자는 porpoise 프롬프트만 응답한다.

## 사전 조건
- `claude` CLI가 PATH에 존재
- `cargo`, `git` 사용 가능
- 작업 디렉토리: `D:\Code\private\porpoise` (하니스가 현재 소스로 릴리즈 바이너리를 항상 재빌드)

## 공통 — porpoise 프롬프트 응답
각 회차 porpoise 실행 시: **지휘? → `y`** / **새 마일스톤? → `n`** / **릴리즈 태그? → `Enter`(빈값)**

---

## 작업 V1 — 정상 경로 재검증 (false-negative 0)

목적: 검증자가 정상 동작하는 경로에서 false-negative 없이 전 회차 병합되는지 확인.

```powershell
cd D:\Code\private\porpoise
pwsh scripts\conductor-revalidate.ps1 -Runs 3
```

### 확인 항목 (상세 로그/콘솔에서)
각 회차 "검증 상세" 블록에서 다음을 확인한다:
- **감사 기록**: `schema=conductor-3`, `verdict=PASS`, `verify_commands`의 `cargo test=exit0`
- **병합 커밋**: `HEAD`에 `[M1-T01] ...` 커밋, 변경 파일이 `src/lib.rs`(+`Cargo.lock`)뿐 — 무관 파일 없음
- **잔여 정리**: `✓ worktree 수=1`, `✓ porpoise 브랜치 잔여=0`
- **무결성**: `✓ src/lib.rs에 'fn add' 포함=True`

### 합격 기준
- 최종 요약: `task 병합 3/3`, `false-neg 0`
- 판정: **PASS**

---

## 작업 V2 — 안전망(폴백) 라이브 검증 (false-positive 경계)

목적: 검증자 파싱 실패를 **강제**해 재질의→객관 증거 폴백이 실제로 동작하고, 정상 코드가
false-negative 없이 통과되는지(=안전망이 회복으로 작동) 확인.

```powershell
cd D:\Code\private\porpoise
pwsh scripts\conductor-revalidate.ps1 -Runs 3 -ForceFallback
```

`-ForceFallback`은 `PORPOISE_VERIFY_CHAOS=1`로 검증자가 JSON 없이 산문만 내도록 유도한다.

### 확인 항목 (상세 로그/콘솔에서)
- **폴백 발동**: 감사 기록에 `fallback_used=True` (전 회차) — 안전망이 실제로 발동했다는 증거
- **콘솔에 ⚠ 경고**: `Verify PASS (폴백) — 검증자 판정 파싱 실패, 객관 증거 기반 통과. 검토 권장`
- **false-negative 없음**: 정상 코드(`cargo test` 통과)가 폴백으로 **PASS**되어 병합됨 (FAIL로 폐기되지 않음)
- **병합·잔여·무결성**: V1과 동일하게 정상

### 합격 기준
- 최종 요약: `폴백 발동 3/3`, `task 병합 3/3`, `false-neg 0`
- 판정: **PASS** (ForceFallback 전용 판정 — 폴백 전부 발동 + false-neg 0 + 전부 병합)

> 참고 — false-positive(나쁜 코드가 통과): 이 하니스의 task는 정상 코드라 false-positive를 직접
> 재현하지 않는다. false-positive는 `verify_commands`가 약할 때의 리스크이며, T03의 완화책
> (폴백 PASS 경고·`fallback_used` 표식·`verdict_fallback="halt"` 옵션·verify_commands 부재 시 보수 FAIL)으로
> 다룬다. 의심 시 폴백 PASS 커밋을 사람이 검토한다.

---

## 로그
- 각 실행은 `D:\tmp\conductor-revalidate-<timestamp>.log`에 전체 transcript를 남긴다(경로는 콘솔 마지막 줄에 출력).
- 결과 공유 시 이 로그 파일 또는 콘솔의 "검증 상세" + "재검증 요약"을 첨부한다.

## 결과 보고 양식 (공유용)
```
V1 (정상):     병합 _/3, false-neg _, 판정 _____
V2 (ForceFallback): 폴백 _/3, 병합 _/3, false-neg _, 판정 _____
이상 징후: (없음 / 내용)
```

V1·V2 모두 PASS면 → `/release v0.22.0` 진행.
