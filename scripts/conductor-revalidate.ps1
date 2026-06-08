<#
.SYNOPSIS
  conductor 라이브 재검증 하니스 — D: 경로에서 N회 반복 실행 후 검증자 신뢰성을 집계하고
  회차별 상세 로그(감사 기록·병합 커밋·잔여 정리·무결성)를 파일로 남긴다.

.DESCRIPTION
  매 회차마다 임시 프로젝트를 새로 스캐폴딩하고(claude 미호출), 사용자가 porpoise를 직접 실행하면
  종료 후 .porpoise/sessions/ 감사 기록(conductor-3)을 파싱해 결과를 집계한다.
  전체 콘솔 출력은 -LogFile로 transcript 저장된다.

  porpoise 실행은 실제 claude를 호출하므로(토큰 소모) 자동화하지 않고 사용자가 관찰하며 수행한다.
  각 회차 porpoise 프롬프트 응답: (1) 지휘? → y  (2) 새 마일스톤? → n  (3) 릴리즈 태그? → Enter(빈값)

.PARAMETER Path
  임시 프로젝트 경로 (기본 D:\tmp\porpoise-smoke). 매 회차 삭제·재생성된다.

.PARAMETER Runs
  반복 횟수 (기본 3).

.PARAMETER Binary
  porpoise 실행 파일. 비우면 리포의 target\release\porpoise.exe 사용(항상 재빌드).

.PARAMETER ForceFallback
  검증자 파싱 실패를 강제(PORPOISE_VERIFY_CHAOS=1)하여 안전망(재질의·폴백)을 라이브 발동시킨다.

.PARAMETER LogFile
  상세 transcript 로그 경로. 비우면 임시 경로 옆에 자동 생성.

.EXAMPLE
  pwsh scripts\conductor-revalidate.ps1 -Runs 3
.EXAMPLE
  pwsh scripts\conductor-revalidate.ps1 -Runs 3 -ForceFallback
#>
param(
    [string]$Path = "D:\tmp\porpoise-smoke",
    [int]$Runs = 3,
    [string]$Binary = "",
    [switch]$ForceFallback,
    [string]$LogFile = ""
)

$ErrorActionPreference = "Stop"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { $env:Path += ";$env:USERPROFILE\.cargo\bin" }

# M22-T04: 안전망(재질의·폴백) 라이브 발동 강제 — 검증자가 파싱 불가 응답을 내도록 유도
if ($ForceFallback) {
    $env:PORPOISE_VERIFY_CHAOS = "1"
} else {
    Remove-Item Env:\PORPOISE_VERIFY_CHAOS -ErrorAction SilentlyContinue
}

# ── 바이너리 결정 ───────────────────────────────────────────────────────────
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$defaultBinary = [string]::IsNullOrWhiteSpace($Binary)
if ($defaultBinary) {
    $Binary = Join-Path $repoRoot "target\release\porpoise.exe"
    # 기본 바이너리는 항상 재빌드 — 현재 소스(미커밋 변경 포함)를 반드시 반영 (stale 방지)
    Write-Host "릴리즈 재빌드 중 (현재 소스 반영, stale 방지)..." -ForegroundColor Yellow
    Push-Location $repoRoot
    cargo build --release | Out-Null
    Pop-Location
}
if (-not (Test-Path $Binary)) { throw "porpoise 바이너리를 찾을 수 없습니다: $Binary" }
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
    throw "claude CLI가 PATH에 없습니다. conductor 라이브 실행이 불가합니다."
}

# ── 로그 파일 ───────────────────────────────────────────────────────────────
if ([string]::IsNullOrWhiteSpace($LogFile)) {
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $LogFile = Join-Path (Split-Path $Path) "conductor-revalidate-$stamp.log"
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
    $records = @()
    if (Test-Path $sessionDir) {
        $records = @(Get-ChildItem $sessionDir -Filter "M1-T01-conductor-*.json" -ErrorAction SilentlyContinue |
            ForEach-Object { Get-Content $_.FullName -Raw | ConvertFrom-Json })
    }

    $merged = $false
    try { if ((git -C $P log --oneline 2>$null) -match "M1-T01") { $merged = $true } } catch {}

    $falseNeg = $false          # 테스트 통과인데 최종 FAIL
    $fallbackFired = $false     # 폴백/재질의 발동 (검증자 비신뢰 신호)

    foreach ($r in $records) {
        $cmds = @($r.verify_commands)
        $allCmdsPass = ($cmds.Count -gt 0) -and (-not ($cmds | Where-Object { $_.exit_code -ne 0 }))
        if ($r.verdict -eq "FAIL" -and $allCmdsPass) { $falseNeg = $true }
        if ($r.fallback_used -eq $true) { $fallbackFired = $true }
        if ($r.verifier_raw -and $r.verifier_raw -match "재질의") { $fallbackFired = $true }
    }

    [pscustomobject]@{
        Records = $records.Count; Merged = $merged
        FalseNegative = $falseNeg; FallbackFired = $fallbackFired
    }
}

# ── 회차 상세 덤프 (확인해야 하는 내용을 로그로) ────────────────────────────
function Show-RunDetail {
    param([string]$P)

    Write-Host "  ── 감사 기록 (sessions/) ──" -ForegroundColor DarkCyan
    $sessionDir = "$P\.porpoise\sessions"
    $files = @(Get-ChildItem $sessionDir -Filter "M1-T01-conductor-*.json" -ErrorAction SilentlyContinue | Sort-Object Name)
    if ($files.Count -eq 0) { Write-Host "    (감사 기록 없음 — 비정상!)" -ForegroundColor Red }
    foreach ($file in $files) {
        $r = Get-Content $file.FullName -Raw | ConvertFrom-Json
        $cmdStr = (($r.verify_commands | ForEach-Object { "$($_.command) $($_.args -join ' ')=exit$($_.exit_code)" }) -join " | ")
        Write-Host ("    [{0}] schema={1} verdict={2} fallback_used={3}" -f $file.Name, $r.schema_version, $r.verdict, $r.fallback_used)
        Write-Host ("      검증명령: {0}" -f $cmdStr) -ForegroundColor DarkGray
        if ($r.feedback) { Write-Host ("      feedback: {0}" -f ($r.feedback -replace "`r?`n", " ")) -ForegroundColor DarkGray }
        if ($r.verifier_raw) {
            $rawHead = $r.verifier_raw.Substring(0, [Math]::Min(240, $r.verifier_raw.Length)) -replace "`r?`n", " "
            Write-Host ("      verifier_raw(앞 240자): {0}" -f $rawHead) -ForegroundColor DarkGray
        }
    }

    Write-Host "  ── 병합 커밋 ──" -ForegroundColor DarkCyan
    Write-Host ("    HEAD: " + (git -C $P log -1 --pretty="%h %s" 2>$null))
    Write-Host "    변경 파일(--stat):"
    (git -C $P show HEAD --stat --pretty="format:" 2>$null) | Where-Object { $_ -ne "" } | ForEach-Object { Write-Host "      $_" }

    Write-Host "  ── 잔여 정리 확인 ──" -ForegroundColor DarkCyan
    $wtCount = @(git -C $P worktree list 2>$null).Count
    $stray = @(git -C $P branch --list "porpoise/*" 2>$null)
    $wtMark = if ($wtCount -eq 1) { "✓" } else { "✗" }
    $brMark = if ($stray.Count -eq 0) { "✓" } else { "✗" }
    Write-Host ("    {0} worktree 수={1} (1=정상)   {2} porpoise 브랜치 잔여={3} (0=정상)" -f $wtMark, $wtCount, $brMark, $stray.Count)

    Write-Host "  ── 산출물 무결성 ──" -ForegroundColor DarkCyan
    $lib = Get-Content "$P\src\lib.rs" -Raw -ErrorAction SilentlyContinue
    $hasAdd = $lib -match "fn add"
    $libMark = if ($hasAdd) { "✓" } else { "✗" }
    Write-Host ("    {0} src/lib.rs에 'fn add' 포함={1}" -f $libMark, $hasAdd)
}

# ── 메인 ────────────────────────────────────────────────────────────────────
Start-Transcript -Path $LogFile -Force | Out-Null
try {
    Write-Host ""
    Write-Host "=== conductor 라이브 재검증 ($Runs 회) ===" -ForegroundColor Cyan
    Write-Host "바이너리     : $Binary"
    Write-Host "경로         : $Path"
    Write-Host "ForceFallback: $($ForceFallback.IsPresent)  (안전망 강제 발동)"
    Write-Host "로그         : $LogFile"
    Write-Host ""

    $results = @()
    for ($i = 1; $i -le $Runs; $i++) {
        Write-Host "──────────────────────────────────────────────" -ForegroundColor DarkGray
        Write-Host "[$i/$Runs] 스캐폴딩 중..." -ForegroundColor Yellow
        New-SmokeProject -P $Path

        Write-Host "[$i/$Runs] porpoise 실행 — 응답: 지휘? y / 새 마일스톤? n / 릴리즈 태그? Enter" -ForegroundColor Yellow
        Push-Location $Path
        & $Binary
        Pop-Location

        Write-Host ""
        Write-Host "[$i/$Runs] ===== 검증 상세 =====" -ForegroundColor Cyan
        Show-RunDetail -P $Path

        $m = Measure-Run -P $Path
        $results += $m
        $tag = if ($m.FalseNegative) { "FALSE-NEG" } elseif ($m.FallbackFired) { "폴백 발동" } else { "정상" }
        Write-Host ("[$i/$Runs] 요약: 병합={0} 감사기록={1} 폴백={2} → {3}" -f $m.Merged, $m.Records, $m.FallbackFired, $tag) -ForegroundColor Green
        Write-Host ""
    }

    # ── 최종 요약 ────────────────────────────────────────────────────────────
    $mergedCount = @($results | Where-Object Merged).Count
    $falseNegCount = @($results | Where-Object FalseNegative).Count
    $fallbackCount = @($results | Where-Object FallbackFired).Count

    Write-Host "================= 재검증 요약 =================" -ForegroundColor Cyan
    Write-Host ("총 실행      : {0}" -f $Runs)
    Write-Host ("task 병합    : {0}/{1}" -f $mergedCount, $Runs)
    Write-Host ("폴백 발동    : {0}/{1} (ForceFallback면 전부 발동이 정상)" -f $fallbackCount, $Runs)
    Write-Host ("false-neg    : {0} (테스트 통과인데 최종 FAIL — 목표 0)" -f $falseNegCount) -ForegroundColor $(if ($falseNegCount -eq 0) { "Green" } else { "Red" })
    Write-Host "=============================================="

    $pass = ($falseNegCount -eq 0) -and ($mergedCount -eq $Runs)
    if ($ForceFallback) {
        # 안전망 검증: 폴백이 전부 발동했고, 그럼에도 false-neg 0 + 전부 병합이어야 PASS
        $pass = $pass -and ($fallbackCount -eq $Runs)
        if ($pass) {
            Write-Host "판정: PASS — 폴백 안전망이 전 회차 발동했고 정상 코드가 false-negative 없이 통과·병합됨." -ForegroundColor Green
        } else {
            Write-Host "판정: 미충족 — 폴백 발동/병합/false-neg 수치를 확인하세요." -ForegroundColor Red
        }
    } else {
        if ($pass) {
            Write-Host "판정: PASS — false-negative 0회, 전 회차 병합 성공." -ForegroundColor Green
        } else {
            Write-Host "판정: 미충족 — 위 수치와 상세 로그를 확인하세요." -ForegroundColor Red
        }
    }
    Write-Host ("상세 로그: {0}" -f $LogFile) -ForegroundColor Cyan
    Write-Host ""
}
finally {
    Stop-Transcript | Out-Null
}
