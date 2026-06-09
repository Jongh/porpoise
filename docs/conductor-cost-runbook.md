# 비용 관측 + 예산 거버넌스 런북 (M28)

conductor가 dispatch하는 코딩 에이전트의 **비용(USD)·토큰**을 캡처·집계하고, **예산 상한**에
도달하면 다음 dispatch 전에 중단한다.

## 비용 캡처 동작

- 에이전트는 Claude Code를 `--output-format stream-json`으로 실행해, 최종 `result` 이벤트의
  `total_cost_usd`·`usage`(입력/출력 토큰)를 캡처한다(`runner.rs`).
- 스트리밍 표시는 유지된다(assistant 텍스트 증분 출력).
- CLI가 stream-json/비용을 지원하지 않으면 **평문 폴백 + 비용 `None`**으로 graceful 저하한다
  (하드 실패 없음). 이 경우 report는 비용을 `-`로 표시한다.

## 감사 기록 (conductor-4)

`sessions/<task>-conductor-<ts>-R<n>.json`에 `cost_usd`·`input_tokens`·`output_tokens`가
추가된다. 구 기록(conductor-3)은 해당 필드가 없으며 report 파서가 `None`으로 처리한다(하위호환).

## 비용 보기 — `porpoise report`

```bash
porpoise report --milestone 28          # 태스크별 비용 + 마일스톤 총비용·총토큰
porpoise report --milestone 28 --markdown
```

- 롤업에 **총비용·총토큰**, 태스크 표에 **비용 컬럼**이 추가된다.
- 재실행-인지(M27)와 일관: **최신 run의 비용만** 합산한다.
- `porpoise status`에도 최근 실행 비용·예산 한도가 표시된다.

## 예산 상한 — `[conductor] budget_usd`

`workspace.toml`:
```toml
[conductor]
mode = "conductor"
budget_usd = 5.00     # 누적 비용이 $5에 도달하면 다음 dispatch 전 중단
```

- 누적 비용이 상한에 **도달하면** 다음 task(또는 배치)를 시작하지 않고 중단한다.
  진행 중인 task/배치는 마치고 정지한다.
- 미설정·0 이하이면 **무제한**(기존 동작).
- 순차·병렬 경로 모두 적용된다.
- `porpoise doctor`/`status`가 설정된 예산을 표시한다.

## 검증

`scripts/conductor-cost-validate.ps1`:
- 합성 conductor-4 감사 기록(비용 포함)을 주입 → `porpoise report`의 비용 롤업이 ground truth와
  일치하는지 대조(claude 불필요).
- 예산 상한 판정(`budget_exceeded`)·집계(최신 run 비용 합산)는 단위 테스트로 고정.

```powershell
pwsh scripts/conductor-cost-validate.ps1
```
