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

claude_code 어댑터 전용. API 어댑터는 항상 legacy. `[conductor] mode = "conductor"`로 활성화.

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

## 기본 ON 승격 기준 (M21-T06)

현재 기본값은 **legacy** (conductor는 명시적 opt-in). 아래 기준을 **모두** 충족하면
차기 버전에서 conductor 기본 ON으로 전환한다.

- [ ] 스모크 하니스로 **동일 task ≥3회 연속 실행 시 검증자 false-negative 0회**
- [ ] 3회 모두 worktree·브랜치 잔여 0, 병합 커밋에 무관 변경 혼입 0
- [ ] 검증 명령 실패가 정확히 FAIL로 이어지는지(정상 음성) 확인
- [ ] 재투입 → 수렴(또는 명확한 halt)이 의도대로 동작

승격 시 API 어댑터는 영향 없음(항상 legacy 유지).
