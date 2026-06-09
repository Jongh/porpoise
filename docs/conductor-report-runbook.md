# 함대 실행 리포트 런북 (M25)

`porpoise report` — conductor가 `.porpoise/sessions/`에 남긴 감사 기록(conductor-3 스키마)을
태스크별·마일스톤별 **실행 요약**으로 집계·가시화한다.

## 명령

```bash
# 가장 최근 마일스톤 요약 (콘솔)
porpoise report

# 특정 마일스톤 한정
porpoise report --milestone 25

# Markdown 리포트도 함께 내보내기 (.porpoise/reports/run-M25.md)
porpoise report --milestone 25 --markdown

# 출력 경로 지정
porpoise report --milestone 25 --markdown --out docs/run-M25.md
```

`porpoise status` 하단에도 최근 실행 1줄 요약(성공률·재투입·폴백)이 표시된다.

## 출력 항목 해석

| 항목 | 의미 |
|------|------|
| **시도(attempts)** | 해당 태스크의 기록된 라운드 수(= sessions JSON 개수) |
| **재투입(redispatch)** | 최대 `redispatch` 값. 0이면 한 번에 통과, N이면 N회 재투입 |
| **Verdict** | 최종 라운드(가장 큰 redispatch, 동률이면 최신 timestamp)의 PASS/FAIL |
| **폴백(fallback)** | 어느 라운드든 검증자 파싱 실패 → 객관 증거 폴백이 발동했으면 표시. **false-positive 추적 신호** |
| **검증명령** | 최종 라운드의 `verify_commands`가 모두 exit 0이면 "통과" |
| **성공률** | 최종 PASS 태스크 / 전체 태스크 |

### 건강 신호 읽는 법
- **재투입 합계가 크다** → task 분해가 굵거나 의존성 누락(병합 충돌). 계획 두뇌(M24) 분해 재검토.
- **폴백 건수가 많다** → 검증자 출력 형식 불안정. `verifier_model` 상향 또는 프롬프트 점검.
- **성공률 저하 추세** → 마일스톤 난이도·DoD 모호성 점검.

## 집계 규칙 (요약)

- `sessions/<task>-conductor-<ts>-R<n>.json`만 대상. 비대상 파일은 조용히 무시하고, **손상된 대상 파일만** 스킵 후 건수로 표시.
- **재실행-인지 집계(M27)**: 같은 task를 다시 실행하면 이전 run의 레코드가 섞인다. timestamp 순으로
  정렬한 뒤 **마지막 `R0`(redispatch==0 = run 시작)부터 끝까지를 "최신 run"**으로 보고, 그 run만
  집계한다. 따라서 `verdict`·`시도`·`재투입`은 **가장 최근 실행 결과**만 반영한다(이전 run의 stale
  FAIL이 최종을 오판하지 않음).
- 최신 run 안에서 **최종 라운드(최신 timestamp)의 verdict**를 결과로, **그 run의 어느 라운드든 폴백**이면 폴백으로 집계.
- `--milestone` 미지정 시 기록 중 **가장 최근 마일스톤**으로 한정.

## 라이브 검증

`scripts/conductor-report-validate.ps1` 참조.

- **합성 모드(기본, claude 불필요)**: 알려진 감사 JSON을 주입 → `porpoise report` 출력 수치가
  주입값과 일치하는지 대조. 집계·렌더링 파이프라인을 결정적으로 검증.
- **라이브 모드(`-Live`)**: 독립 task 마일스톤을 실제 conductor로 1회 실행 → 생성된
  `sessions/*.json` 원본 개수·verdict 분포와 `porpoise report` 수치가 일치하는지 대조.
  폴백 케이스는 `PORPOISE_VERIFY_CHAOS`로 유도해 `fallback` 집계까지 확인.

```powershell
# 합성(빠름)
pwsh scripts/conductor-report-validate.ps1

# 라이브
pwsh scripts/conductor-report-validate.ps1 -Live -WorkDir D:\tmp\porpoise-report
```
