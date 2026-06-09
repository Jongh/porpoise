<#
.SYNOPSIS
  M25 fleet execution report (`porpoise report`) validation harness.

.DESCRIPTION
  Synthetic mode (default, no Claude needed): injects known conductor-3 audit
  records into a temp project's .porpoise/sessions/, runs `porpoise report`, and
  asserts the aggregated numbers match the injected ground truth. Validates the
  parse -> aggregate -> render pipeline end-to-end through the real binary.

  Live mode (-Live): scaffolds an independent-task milestone, expects the operator
  to run conductor to produce real sessions, then cross-checks report numbers
  against the raw session files.

.EXAMPLE
  pwsh scripts/conductor-report-validate.ps1
  pwsh scripts/conductor-report-validate.ps1 -Live -WorkDir D:\tmp\porpoise-report
#>
[CmdletBinding()]
param(
    [switch]$Live,
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-report-validate")
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Write-Section($t) { Write-Host "`n=== $t ===" -ForegroundColor Cyan }
function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; exit 1 }
function Pass($m) { Write-Host "PASS: $m" -ForegroundColor Green }

# 1. Always rebuild release (avoid stale-binary trap)
Write-Section "Build (release)"
Push-Location $repoRoot
try {
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { Fail "cargo build failed" }
} finally { Pop-Location }
$exe = Join-Path $repoRoot "target\release\porpoise.exe"
if (-not (Test-Path $exe)) { Fail "binary not found: $exe" }

# 2. Fresh temp project
Write-Section "Scaffold temp project: $WorkDir"
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
$sessions = Join-Path $WorkDir ".porpoise\sessions"
New-Item -ItemType Directory -Force -Path $sessions | Out-Null

function Write-Audit($task, $redispatch, $ts, $verdict, $fallback) {
    # verify_commands omitted on purpose: it is optional (#[serde(default)]) and PS 5.1
    # unwraps single-element arrays, which would corrupt the JSON shape.
    $rec = [ordered]@{
        schema_version = "conductor-3"
        task_id        = $task
        redispatch     = $redispatch
        timestamp      = $ts
        diff_lines     = 5
        verdict        = $verdict
        feedback       = ""
        fallback_used  = $fallback
        verifier_raw   = "raw"
        dispatch_output = "out"
    }
    $name = "$task-conductor-$($ts -replace '[:\-TZ]','')-R$redispatch.json"
    # Write UTF-8 WITHOUT BOM (PS 5.1 Set-Content -Encoding UTF8 emits a BOM, which
    # is exactly the kind of input the parser must tolerate but the ground truth
    # here mimics conductor's own no-BOM writer).
    $json = $rec | ConvertTo-Json -Depth 6
    [System.IO.File]::WriteAllText((Join-Path $sessions $name), $json, (New-Object System.Text.UTF8Encoding($false)))
}

if (-not $Live) {
    Write-Section "Synthetic mode: inject known audit records"
    # Ground truth: 4 tasks, 5 files
    #   T01: R0 PASS            -> PASS, redispatch 0
    #   T02: R0 FAIL, R1 PASS   -> PASS, redispatch 1, attempts 2
    #   T03: R0 PASS fallback   -> PASS (warn), fallback
    #   T04: R0 FAIL            -> FAIL
    Write-Audit "M1-T01" 0 "2026-06-09T10:00:00Z" "PASS" $false
    Write-Audit "M1-T02" 0 "2026-06-09T10:01:00Z" "FAIL" $false
    Write-Audit "M1-T02" 1 "2026-06-09T10:06:00Z" "PASS" $false
    Write-Audit "M1-T03" 0 "2026-06-09T10:02:00Z" "PASS" $true
    Write-Audit "M1-T04" 0 "2026-06-09T10:03:00Z" "FAIL" $false

    $expected = @{ Total = 4; Pass = 3; Fail = 1; Redispatch = 1; Fallback = 1; Files = 5 }

    # Independent ground-truth count from filesystem
    $fileCount = (Get-ChildItem $sessions -Filter "*-conductor-*.json").Count
    if ($fileCount -ne $expected.Files) { Fail "session file count $fileCount != $($expected.Files)" }
    Pass "session files on disk: $fileCount"

    Write-Section "Run: porpoise report --milestone 1 --markdown"
    Push-Location $WorkDir
    try {
        $out = & $exe report --milestone 1 --markdown | Out-String
        if ($LASTEXITCODE -ne 0) { Fail "report exited $LASTEXITCODE" }
    } finally { Pop-Location }
    Write-Host $out

    # Assert console rollup (ASCII-only substrings; Korean labels are encoding-fragile in PS 5.1)
    if ($out -notmatch "PASS\s+$($expected.Pass)\b") { Fail "console PASS count mismatch" }
    if ($out -notmatch "FAIL\s+$($expected.Fail)\b") { Fail "console FAIL count mismatch" }
    if ($out -notmatch "75\.0%") { Fail "success rate 75.0% not found" }
    Pass "console rollup matches ground truth (PASS 3 / FAIL 1 / 75.0%)"

    # Assert markdown export
    $md = Join-Path $WorkDir ".porpoise\reports\run-M1.md"
    if (-not (Test-Path $md)) { Fail "markdown not written: $md" }
    $mdText = Get-Content $md -Raw
    foreach ($t in "M1-T01","M1-T02","M1-T03","M1-T04") {
        if ($mdText -notmatch [regex]::Escape($t)) { Fail "markdown missing row $t" }
    }
    if ($mdText -notmatch "PASS 3 / FAIL 1") { Fail "markdown rollup mismatch" }
    Pass "markdown export contains all rows + rollup"

    Write-Host "`nSYNTHETIC VALIDATION: PASS" -ForegroundColor Green
    Write-Host "Report numbers match injected ground truth and filesystem." -ForegroundColor Green
    exit 0
}

# ---- Live mode ----
Write-Section "Live mode setup"
$ms = Join-Path $WorkDir ".porpoise\milestones"
New-Item -ItemType Directory -Force -Path $ms | Out-Null

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
[conductor]
mode = "conductor"
max_parallel = 3
"@ | Set-Content -Path (Join-Path $WorkDir ".porpoise\workspace.toml") -Encoding UTF8

@"
# report-live 프로젝트

3개 독립 task로 conductor를 1회 실행해 sessions/ 감사 기록을 생성한 뒤,
porpoise report 수치가 원본과 일치하는지 대조한다.

## 작업 목록
- [ ] M1-T01: tests/a.rs 새 파일에 add 함수+테스트 작성 (다른 파일 금지)
- [ ] M1-T02: tests/b.rs 새 파일에 sub 함수+테스트 작성 (다른 파일 금지)
- [ ] M1-T03: tests/c.rs 새 파일에 mul 함수+테스트 작성 (다른 파일 금지)
"@ | Set-Content -Path (Join-Path $WorkDir ".porpoise\project.md") -Encoding UTF8

Copy-Item (Join-Path $WorkDir ".porpoise\project.md") (Join-Path $ms "M1.md")

Push-Location $WorkDir
try { & git init -q; & git add -A; & git commit -q -m "init" | Out-Null } finally { Pop-Location }

Write-Host @"

Live scaffold ready at: $WorkDir

Next steps (run manually):
  1. cd "$WorkDir"
  2. "$exe"                       # run conductor to produce real sessions/*.json
  3. "$exe" report --milestone 1 --markdown

Then cross-check:
  - (Get-ChildItem .porpoise\sessions -Filter '*-conductor-*.json').Count
    should equal the sum of 'attempts' in the report table.
  - distinct task ids in sessions/ should equal report task count.
  - To exercise fallback aggregation, set `$env:PORPOISE_VERIFY_CHAOS=1` before step 2.
"@ -ForegroundColor Yellow
