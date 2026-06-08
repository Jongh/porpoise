<#
.SYNOPSIS
  conductor 라이브 재검증 하니스 — D: 경로에서 N회 반복 실행 후 검증자 신뢰성을 집계한다.

.DESCRIPTION
  M21 DoD: "스모크 하니스로 동일 task를 N(>=3)회 연속 실행 시 검증자 false-negative 0회".
  각 회차마다 임시 프로젝트를 새로 스캐폴딩하고(claude 미호출), 사용자가 porpoise를 직접 실행하면
  종료 후 .porpoise/sessions/ 감사 기록(conductor-2)을 파싱해 결과를 집계한다.

  porpoise 실행은 실제 claude를 호출하므로(토큰 소모) 자동화하지 않고 사용자가 관찰하며 수행한다.
  각 회차 porpoise 프롬프트 응답: (1) 지휘? → y  (2) 새 마일스톤? → n  (3) 릴리즈 태그? → Enter(빈값)

.PARAMETER Path
  임시 프로젝트 경로 (기본 D:\tmp\porpoise-smoke). 매 회차 삭제·재생성된다.

.PARAMETER Runs
  반복 횟수 (기본 3).

.PARAMETER Binary
  porpoise 실행 파일. 비우면 리포의 target\release\porpoise.exe 사용(없으면 빌드).

.EXAMPLE
  pwsh scripts\conductor-revalidate.ps1 -Runs 3
#>
param(
    [string]$Path = "D:\tmp\porpoise-smoke",
    [int]$Runs = 3,
    [string]$Binary = ""
)

$ErrorActionPreference = "Stop"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { $env:Path += ";$env:USERPROFILE\.cargo\bin" }

# ── 바이너리 결정 ───────────────────────────────────────────────────────────
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$defaultBinary = [string]::IsNullOrWhiteSpace($Binary)
if ($defaultBinary) {
    $Binary = Join-Path $repoRoot "target\release\porpoise.exe"
    # 기본 바이너리는 항상 재빌드 — 현재 소스(미커밋 변경 포함)를 반드시 반영.
    # (stale 바이너리로 잘못된 버전을 검증하는 사고 방지)
    Write-Host "릴리즈 재빌드 중 (현재 소스 반영, stale 방지)..." -ForegroundColor Yellow
    Push-Location $repoRoot
    cargo build --release | Out-Null
    Pop-Location
}
if (-not (Test-Path $Binary)) { throw "porpoise 바이너리를 찾을 수 없습니다: $Binary" }

# claude CLI 사전 확인
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
    throw "claude CLI가 PATH에 없습니다. conductor 라이브 실행이 불가합니다."
}

# ── 스캐폴딩 함수 (claude 미호출) ───────────────────────────────────────────
function New-SmokeProject {
    param([string]$P, [int]$MaxRedispatch = 1)

    Remove-Item -Recurse -Force $P -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force (Split-Path $P) | Out-Null
    cargo new --lib $P | Out-Null
    New-Item -ItemType Directory -Force "$P\.porpoise\milestones", "$P\.porpoise\sessions" | Out-Null

    @"
[general]
language = "ko"

[model]
adapter = "claude_code"

[dod]
items = ["코드가 컴파일된다", "cargo test가 통과한다"]

[tech]
stack = "Rust"
test_command = "cargo test"

[security]
allowed_command_prefixes = ["cargo"]
verify_timeout_secs = 180

[conductor]
mode = "conductor"
max_redispatch = $MaxRedispatch
"@ | Out-File "$P\.porpoise\workspace.toml" -Encoding utf8

    @"
# smoke-test 프로젝트

conductor 루프 종단 간 검증용 임시 프로젝트.

## 작업 목록
- [ ] M1-T01: src/lib.rs에 두 정수를 더하는 add(a, b) 함수와 단위 테스트를 추가
"@ | Out-File "$P\.porpoise\project.md" -Encoding utf8

    @"
# M1: conductor 스모크 테스트 (v0.1.0)

## 목표
conductor 루프가 단일 task를 Brief→Dispatch→Verify→Integrate로 끝까지 처리하는지 검증한다.

## 작업 목록
- [ ] M1-T01: src/lib.rs에 두 정수를 더하는 add(a, b) 함수와 단위 테스트를 추가

## 메타데이터
- created: smoke
- status: active
"@ | Out-File "$P\.porpoise\milestones\M1.md" -Encoding utf8

    Add-Content -Path "$P\.gitignore" -Value ".porpoise/" -Encoding utf8
    git -C $P add -A | Out-Null
    git -C $P commit -m "init: cargo 스켈레톤" | Out-Null
}

# ── 회차 결과 집계 함수 (감사 기록 파싱) ────────────────────────────────────
function Measure-Run {
    param([string]$P)

    $sessionDir = "$P\.porpoise\sessions"
    # @()로 강제 배열화 — 기록이 1개여도 .Count가 올바르게 동작하도록
    # (단일 PSCustomObject는 .Count가 멤버 조회로 $null 반환되는 PS 함정 회피)
    $records = @()
    if (Test-Path $sessionDir) {
        $records = @(Get-ChildItem $sessionDir -Filter "M1-T01-conductor-*.json" -ErrorAction SilentlyContinue |
            ForEach-Object { Get-Content $_.FullName -Raw | ConvertFrom-Json })
    }

    # 최종 task 병합 여부 (git 커밋)
    $merged = $false
    try {
        $log = git -C $P log --oneline 2>$null
        if ($log -match "M1-T01") { $merged = $true }
    } catch {}

    $falseNeg = $false       # 테스트 통과인데 최종 FAIL (M21 목표: 0)
    $reliabilityEvent = $false  # 재질의 또는 객관 증거 폴백 발생 (검증자 비신뢰 신호)

    foreach ($r in $records) {
        $cmds = @($r.verify_commands)
        $allCmdsPass = ($cmds.Count -gt 0) -and (-not ($cmds | Where-Object { $_.exit_code -ne 0 }))
        if ($r.verdict -eq "FAIL" -and $allCmdsPass) { $falseNeg = $true }
        if ($r.feedback -and $r.feedback -match "객관 증거") { $reliabilityEvent = $true }
        if ($r.verifier_raw -and $r.verifier_raw -match "재질의") { $reliabilityEvent = $true }
    }

    [pscustomobject]@{
        Records          = $records.Count
        Merged           = $merged
        FalseNegative    = $falseNeg
        ReliabilityEvent = $reliabilityEvent
    }
}

# ── 메인 루프 ───────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "=== conductor 라이브 재검증 ($Runs 회) ===" -ForegroundColor Cyan
Write-Host "바이너리: $Binary"
Write-Host "경로    : $Path"
Write-Host ""

$results = @()
for ($i = 1; $i -le $Runs; $i++) {
    Write-Host "──────────────────────────────────────────────" -ForegroundColor DarkGray
    Write-Host "[$i/$Runs] 스캐폴딩 중..." -ForegroundColor Yellow
    New-SmokeProject -P $Path

    Write-Host "[$i/$Runs] porpoise 실행 — 프롬프트 응답: 지휘? y / 새 마일스톤? n / 릴리즈 태그? Enter" -ForegroundColor Yellow
    Push-Location $Path
    & $Binary
    Pop-Location

    $m = Measure-Run -P $Path
    $results += $m
    $tag = if ($m.FalseNegative) { "FALSE-NEG" } elseif ($m.ReliabilityEvent) { "회복(폴백/재질의)" } else { "정상" }
    Write-Host ("[$i/$Runs] 결과: 병합={0} 감사기록={1} → {2}" -f $m.Merged, $m.Records, $tag) -ForegroundColor Green
}

# ── 요약 ────────────────────────────────────────────────────────────────────
$mergedCount = ($results | Where-Object Merged).Count
$falseNegCount = ($results | Where-Object FalseNegative).Count
$reliabilityCount = ($results | Where-Object ReliabilityEvent).Count

Write-Host ""
Write-Host "================= 재검증 요약 =================" -ForegroundColor Cyan
Write-Host ("총 실행      : {0}" -f $Runs)
Write-Host ("task 병합    : {0}/{1}" -f $mergedCount, $Runs)
Write-Host ("검증자 회복  : {0} (재질의/객관 증거 폴백 — 검증자 비신뢰 신호, 0이 이상적)" -f $reliabilityCount)
Write-Host ("false-neg    : {0} (테스트 통과인데 최종 FAIL — M21 목표: 0)" -f $falseNegCount) -ForegroundColor $(if ($falseNegCount -eq 0) { "Green" } else { "Red" })
Write-Host "=============================================="
if ($falseNegCount -eq 0 -and $mergedCount -eq $Runs) {
    Write-Host "판정: PASS — false-negative 0회, 전 회차 병합 성공. conductor 기본 ON 승격 기준 충족." -ForegroundColor Green
} else {
    Write-Host "판정: 미충족 — 위 수치를 확인하세요. 감사 기록: $Path\.porpoise\sessions" -ForegroundColor Red
}
Write-Host ""
