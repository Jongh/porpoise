# Conductor 모드 (에이전트 함대 지휘)

Porpoise의 *worker → manager* 피벗 실행 경로. task당 4단계 LLM 호출(legacy) 대신,
task를 실제 코딩 에이전트에게 통째로 위임하고 **독립 검증자**로 게이트한다.

## 흐름

```
Brief → Dispatch → Verify → Integrate
```

- **Brief**: project.md·DoD·규약·마일스톤 목표를 단일 작업 지시서로 조립 (LLM 없음)
- **Dispatch**: 격리 git worktree(`.porpoise/worktrees/<task>`)에서 에이전트에게 통째 위임
- **Verify**: `verify_commands`를 worktree에서 실제 실행(객관 증거) + 독립 검증자 LLM 심사(적대적)
  → PASS/FAIL
- **Integrate**: PASS면 커밋·병합·완료 처리, FAIL이면 피드백 재투입(`max_redispatch` 한도) 또는 중단

claude_code 어댑터 전용. API 어댑터는 항상 legacy.
**v0.22.0부터 claude_code에서 기본 활성화**된다(미설정 = conductor). 기존 4단계 방식을 쓰려면 opt-out:

```toml
[conductor]
mode = "legacy"
```

기존 프로젝트가 처음 conductor로 진입하면 1회 전환 안내가 출력된다.

## 검증자 신뢰성 (M21)

라이브 스모크 테스트에서 **검증자 LLM이 파싱 불가능한 응답을 반환해 false-negative FAIL**이
발생했다(코드 정상·`cargo test` 통과인데도). M21에서 다음으로 경화했다:

1. **재질의 1회**: verdict 파싱 실패 시 "JSON 한 줄만" 재요청.
2. **객관 증거 폴백**: 재질의 후에도 파싱 불가 + 검증 명령이 전부 통과면, 즉시 FAIL 대신
   객관 증거 기반으로 **PASS** 처리. 검증 명령이 전혀 없으면 보수적 FAIL.
3. **출력 형식 강제**: 검증자 프롬프트가 "도구 사용·설명 금지, JSON 객체 하나만" 출력을 요구.
4. **감사 관측성**: `sessions/<task>-conductor-<timestamp>-R<n>.json`에 검증자 원문·dispatch 출력
   포함, 타임스탬프 파일명으로 재투입·재실행 이력 보존.
5. **worktree 누수 방지**: 성공·실패·중단 모든 경로에서 worktree·브랜치 정리 보장.

## 폴백 정책 (M22)

검증자 verdict 파싱 실패가 재질의 후에도 지속될 때의 처리를 `[conductor] verdict_fallback`으로 제어한다.

| 값 | 동작 |
|----|------|
| `pass_if_checks_pass` (기본) | 검증 명령이 전부 통과면 객관 증거 기반 **PASS** (false-negative 방지). 폴백 발동 시 콘솔·감사에 경고 표시 |
| `halt` | 폴백 PASS 대신 **FAIL** 처리하여 사용자 검토 유도 (false-positive 보수 차단) |

- 폴백 PASS로 병합된 task는 감사 기록에 `fallback_used: true`로 표식되어 추적 가능하다.
- `verify_commands`가 전혀 없으면 정책과 무관하게 보수적 FAIL(객관 증거 부재).

## 라이브 스모크 테스트

```powershell
# 1. 임시 프로젝트 스캐폴딩 (claude 호출 없음)
pwsh scripts/conductor-smoke.ps1

# 2. 빌드된 바이너리로 직접 실행 (토큰 소모·실제 변경)
$P = "target\release\porpoise.exe"
Push-Location D:\tmp\porpoise-smoke
& $P doctor      # conductor 활성 확인
& $P             # 프롬프트에 y
Pop-Location
```

판정: 병합 커밋에 의도한 변경만 / worktree·브랜치 잔여 0 / `sessions/` 감사 기록 / 무결성.

## 병렬 함대 (M23, opt-in)

독립적인 task가 여럿일 때 `[conductor] max_parallel`(기본 1=순차, [1,8])을 올리면 task들을
**각자 worktree에서 동시에** dispatch·verify하고, 결과를 **순차·충돌 인지**로 통합한다.

```toml
[conductor]
max_parallel = 3
```

- **낙관적 동시성**: 일단 병렬 실행하고, 통합 시 병합 충돌이 나면 그 task만 **갱신된 base에서 재투입**하여 사실상 직렬화한다(수렴). 시도 한도(`max_redispatch`) 초과 시 중단.
- **독립 task 전제**: 병렬 task는 같은 base에서 분기하므로 서로의 변경을 못 본다. 의존 task는 `max_parallel = 1` 권장.
- **출력**: 병렬 실행 중에는 에이전트 출력을 캡처만 하고, 완료 후 task별로 그룹 표시(인터리브 방지).
- **비용 주의**: 동시 N개 claude 프로세스 → 토큰·레이트리밋 N배. 감사 기록은 task별 유지.

## 라이브 재검증 하니스

```powershell
# N회 반복 + 검증자 신뢰성 자동 집계 (각 회차 직접 실행: 지휘? y / 새 마일스톤? n / 릴리즈 태그? Enter)
pwsh scripts/conductor-revalidate.ps1 -Runs 3

# 안전망(재질의·폴백) 라이브 발동 강제 — 검증자 파싱 실패를 유도해 폴백 경로 검증
pwsh scripts/conductor-revalidate.ps1 -Runs 3 -ForceFallback
```

`-ForceFallback`은 `PORPOISE_VERIFY_CHAOS=1`로 검증자가 파싱 불가 응답을 내도록 유도한다.
이때 정상 코드라면 재질의→객관 증거 폴백으로 **PASS 회복**(false-negative 0)되어야 한다.

## 기본 ON 승격 (완료 — M22)

M21 라이브 재검증(3/3 PASS, false-negative 0)으로 아래 기준을 충족하여 **v0.22.0에서 기본 ON으로 승격**했다.

- [x] 스모크 하니스로 동일 task ≥3회 연속 실행 시 검증자 false-negative 0회
- [x] 전 회차 worktree·브랜치 잔여 0, 병합 커밋에 무관 변경 혼입 0
- [x] 감사 관측성(verifier_raw·dispatch·타임스탬프 이력) 라이브 확인
- [ ] (M22 검증 항목) `-ForceFallback`으로 폴백 경로를 라이브 1회 이상 발동 + false-positive 0

승격 후에도 API 어댑터는 영향 없음(항상 legacy). 기존 사용자는 `mode = "legacy"`로 즉시 opt-out 가능.
