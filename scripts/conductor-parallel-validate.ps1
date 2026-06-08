<#
.SYNOPSIS
  conductor 병렬 함대(M23) 라이브 검증 하니스 — 독립 task N개를 한 번에 병렬 실행하고 결과를 집계한다.

.DESCRIPTION
  서로 다른 파일(tests/calc_*.rs)을 만드는 **독립 task 3개**와 max_parallel>1로 임시 프로젝트를
  스캐폴딩한다. porpoise를 한 번 실행하면 conductor가 세 task를 동시에 dispatch·verify하고 순차
  통합한다(충돌 없음 — 파일이 겹치지 않음). 종료 후 감사 기록·커밋·잔여·무결성을 상세 덤프한다.

  porpoise 실행은 실제 claude를 호출하므로(토큰 N배) 사용자가 관찰하며 수행한다.
  프롬프트 응답: (1) N개 병렬 지휘? → y  (2) 새 마일스톤? → n  (3) 릴리즈 태그? → Enter(빈값)

.PARAMETER Path
  임시 프로젝트 경로 (기본 D:\tmp\porpoise-parallel). 삭제·재생성된다.

.PARAMETER MaxParallel
  동시 task 수 (기본 3, [1,8]).

.PARAMETER Binary
  porpoise 실행 파일. 비우면 리포 target\release\porpoise.exe (항상 재빌드).

.PARAMETER LogFile
  transcript 로그 경로. 비우면 자동 생성.

.EXAMPLE
  pwsh scripts\conductor-parallel-validate.ps1 -MaxParallel 3
#>
param(
    [string]$Path = "D:\tmp\porpoise-parallel",
    [int]$MaxParallel = 3,
    [string]$Binary = "",
    [string]$LogFile = ""
)

$ErrorActionPreference = "Stop"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { $env:Path += ";$env:USERPROFILE\.cargo\bin" }

# ── 바이너리 (항상 재빌드 — stale 방지) ─────────────────────────────────────
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if ([string]::IsNullOrWhiteSpace($Binary)) {
    $Binary = Join-Path $repoRoot "target\release\porpoise.exe"
    Write-Host "릴리즈 재빌드 중 (현재 소스 반영)..." -ForegroundColor Yellow
    Push-Location $repoRoot; cargo build --release | Out-Null; Pop-Location
}
if (-not (Test-Path $Binary)) { throw "porpoise 바이너리를 찾을 수 없습니다: $Binary" }
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) { throw "claude CLI가 PATH에 없습니다." }

if ([string]::IsNullOrWhiteSpace($LogFile)) {
    $LogFile = Join-Path (Split-Path $Path) ("conductor-parallel-" + (Get-Date -Format "yyyyMMdd-HHmmss") + ".log")
}

# 독립 task 정의 (서로 다른 tests/ 파일 → 병합 충돌 없음)
$TASKS = @(
    @{ id = "M1-T01"; file = "tests/calc_add.rs"; desc = "두 i64를 더하는 add 함수" },
    @{ id = "M1-T02"; file = "tests/calc_sub.rs"; desc = "두 i64를 빼는 sub 함수" },
    @{ id = "M1-T03"; file = "tests/calc_mul.rs"; desc = "두 i64를 곱하는 mul 함수" }
)

# ── 스캐폴딩 ─────────────────────────────────────────────────────────────────
function New-ParallelProject {
    param([string]$P)

    Remove-Item -Recurse -Force $P -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force (Split-Path $P) | Out-Null
    cargo new --lib $P | Out-Null
    New-Item -ItemType Directory -Force "$P\.porpoise\milestones", "$P\.porpoise\sessions", "$P\tests" | Out-Null

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
max_redispatch = 1
max_parallel = $MaxParallel
"@ | Out-File "$P\.porpoise\workspace.toml" -Encoding utf8

    $taskLines = ($TASKS | ForEach-Object { "- [ ] $($_.id): $($_.file) 파일을 새로 만들어 $($_.desc)와 그 단위 테스트를 작성 (다른 파일은 건드리지 말 것)" }) -join "`n"
    @"
# parallel-test 프로젝트

conductor 병렬 함대 검증용. 각 task는 서로 다른 tests/ 파일만 만든다(독립).

## 작업 목록
$taskLines
"@ | Out-File "$P\.porpoise\project.md" -Encoding utf8

    @"
# M1: 병렬 함대 검증 (v0.1.0)

## 목표
독립 task 3개를 동시에 dispatch·verify하고 충돌 없이 순차 통합하는지 검증한다.

## 작업 목록
$taskLines

## 메타데이터
- created: parallel-smoke
- status: active
"@ | Out-File "$P\.porpoise\milestones\M1.md" -Encoding utf8

    # Cargo.lock도 ignore — 공유 파일 충돌 가능성 제거(완전 독립)
    Add-Content -Path "$P\.gitignore" -Value ".porpoise/`nCargo.lock" -Encoding utf8
    git -C $P add -A | Out-Null
    git -C $P commit -m "init: cargo 스켈레톤" | Out-Null
}

# ── 측정·상세 덤프 ──────────────────────────────────────────────────────────
function Show-And-Measure {
    param([string]$P)

    Write-Host ""
    Write-Host "===== 병렬 검증 상세 =====" -ForegroundColor Cyan

    # 1. 감사 기록 (task당 1개씩, 전부 PASS·fallback_used=False 기대)
    Write-Host "── 감사 기록 (sessions/) ──" -ForegroundColor DarkCyan
    $records = @(Get-ChildItem "$P\.porpoise\sessions" -Filter "M1-T*-conductor-*.json" -EA SilentlyContinue |
        ForEach-Object { Get-Content $_.FullName -Raw | ConvertFrom-Json })
    foreach ($r in ($records | Sort-Object task_id)) {
        $cmd = (($r.verify_commands | ForEach-Object { "$($_.command)=exit$($_.exit_code)" }) -join " ")
        Write-Host ("    [{0}] verdict={1} fallback_used={2} 검증명령=[{3}]" -f $r.task_id, $r.verdict, $r.fallback_used, $cmd)
    }

    # 2. 커밋 (task당 1개)
    Write-Host "── 커밋 로그 ──" -ForegroundColor DarkCyan
    (git -C $P log --oneline 2>$null) | ForEach-Object { Write-Host "    $_" }

    # 3. 모든 task 완료 여부 (project.md)
    Write-Host "── task 완료 (project.md) ──" -ForegroundColor DarkCyan
    $proj = Get-Content "$P\.porpoise\project.md" -Raw -EA SilentlyContinue
    $doneCount = ([regex]::Matches($proj, "- \[x\] M1-T")).Count
    $openCount = ([regex]::Matches($proj, "- \[ \] M1-T")).Count
    Write-Host ("    완료=$doneCount  미완료=$openCount  (기대: 완료=$($TASKS.Count), 미완료=0)")

    # 4. 잔여 정리
    Write-Host "── 잔여 정리 ──" -ForegroundColor DarkCyan
    $wt = @(git -C $P worktree list 2>$null).Count
    $br = @(git -C $P branch --list "porpoise/*" 2>$null)
    Write-Host ("    worktree 수=$wt (1=정상)   porpoise 브랜치 잔여=$($br.Count) (0=정상)")

    # 5. 무결성 — 각 task의 파일 존재 + 최종 cargo test
    Write-Host "── 산출물 무결성 ──" -ForegroundColor DarkCyan
    $allFiles = $true
    foreach ($t in $TASKS) {
        $exists = Test-Path (Join-Path $P $t.file)
        if (-not $exists) { $allFiles = $false }
        Write-Host ("    {0} {1} 존재={2}" -f $(if ($exists){"✓"}else{"✗"}), $t.file, $exists)
    }
    Push-Location $P
    # cargo의 stderr(Compiling...)가 PS 5.1에서 NativeCommandError로 종료 에러가 되지 않도록
    # EAP를 잠시 완화하고, 결과는 종료 코드로 판정한다(출력 파싱 대신).
    $prevEAP = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    cargo test 2>&1 | Out-Null
    $testPass = ($LASTEXITCODE -eq 0)
    $ErrorActionPreference = $prevEAP
    Pop-Location
    Write-Host ("    {0} 최종 cargo test 통과={1}" -f $(if($testPass){"✓"}else{"✗"}), $testPass)

    # ── 판정 ──
    $merged = ($records | Where-Object { $_.verdict -eq "PASS" }).Count
    $pass = ($doneCount -eq $TASKS.Count) -and ($openCount -eq 0) -and ($wt -eq 1) -and ($br.Count -eq 0) -and $allFiles -and $testPass
    Write-Host ""
    Write-Host "================= 병렬 검증 요약 =================" -ForegroundColor Cyan
    Write-Host ("동시 task 수 : {0} (max_parallel={1})" -f $TASKS.Count, $MaxParallel)
    Write-Host ("PASS 감사    : {0}/{1}" -f $merged, $TASKS.Count)
    Write-Host ("task 완료    : {0}/{1}" -f $doneCount, $TASKS.Count)
    Write-Host ("잔여 정리    : worktree=$wt, 브랜치=$($br.Count)")
    Write-Host "================================================="
    if ($pass) {
        Write-Host "판정: PASS — 독립 task 전부 병렬 처리·통합·완료, 잔여 0, 최종 테스트 통과." -ForegroundColor Green
    } else {
        Write-Host "판정: 미충족 — 위 상세를 확인하세요." -ForegroundColor Red
    }
    Write-Host ("상세 로그: {0}" -f $LogFile) -ForegroundColor Cyan
}

# ── 메인 ────────────────────────────────────────────────────────────────────
Start-Transcript -Path $LogFile -Force | Out-Null
try {
    Write-Host ""
    Write-Host "=== conductor 병렬 함대 검증 (max_parallel=$MaxParallel) ===" -ForegroundColor Cyan
    Write-Host "바이너리: $Binary"
    Write-Host "경로    : $Path"
    Write-Host "로그    : $LogFile"
    Write-Host ""
    Write-Host "스캐폴딩 중 (독립 task $($TASKS.Count)개)..." -ForegroundColor Yellow
    New-ParallelProject -P $Path

    Write-Host "porpoise 실행 — 응답: $($TASKS.Count)개 병렬 지휘? y / 새 마일스톤? n / 릴리즈 태그? Enter" -ForegroundColor Yellow
    Write-Host "(병렬 실행 중 각 task 출력은 완료 후 그룹으로 표시됩니다)" -ForegroundColor DarkGray
    Push-Location $Path
    & $Binary
    Pop-Location

    Show-And-Measure -P $Path
}
finally {
    Stop-Transcript | Out-Null
}
