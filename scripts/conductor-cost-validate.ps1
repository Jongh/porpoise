<#
.SYNOPSIS
  M28 cost aggregation validation (no Claude needed).

.DESCRIPTION
  Injects known conductor-4 audit records (with cost_usd/tokens) into a temp
  project's sessions/, runs `porpoise report`, and asserts the cost rollup matches
  the injected ground truth. Validates the parse -> aggregate(cost) -> render path
  through the real binary. Uses no-BOM UTF-8 (PS 5.1 Set-Content -Encoding UTF8
  would emit a BOM; the parser tolerates it but ground truth mimics the no-BOM
  writer) and ASCII-only assertions.
#>
[CmdletBinding()]
param(
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-cost-validate")
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; exit 1 }
function Ok($m)   { Write-Host "  OK: $m" -ForegroundColor Green }

Write-Host "=== Build (release) ===" -ForegroundColor Cyan
Push-Location $repoRoot
try {
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { Fail "cargo build failed" }
} finally { Pop-Location }
$exe = Join-Path $repoRoot "target\release\porpoise.exe"
if (-not (Test-Path $exe)) { Fail "binary not found: $exe" }

Write-Host "=== Scaffold + inject conductor-4 records ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
$sessions = Join-Path $WorkDir ".porpoise\sessions"
New-Item -ItemType Directory -Force -Path $sessions | Out-Null

function Write-Audit($task, $redispatch, $ts, $verdict, $cost, $inTok, $outTok) {
    $rec = [ordered]@{
        schema_version = "conductor-4"
        task_id        = $task
        redispatch     = $redispatch
        timestamp      = $ts
        diff_lines     = 5
        verdict        = $verdict
        feedback       = ""
        fallback_used  = $false
        cost_usd       = $cost
        input_tokens   = $inTok
        output_tokens  = $outTok
    }
    $name = "$task-conductor-$($ts -replace '[:\-TZ]','')-R$redispatch.json"
    $json = $rec | ConvertTo-Json -Depth 6
    [System.IO.File]::WriteAllText((Join-Path $sessions $name), $json, (New-Object System.Text.UTF8Encoding($false)))
}

# Ground truth:
#   T01: R0 PASS cost 0.05
#   T02: R0 FAIL cost 0.02, R1 PASS cost 0.03  -> latest run cost 0.05
#   total = 0.10, input tokens 100*3 = 300
Write-Audit "M1-T01" 0 "2026-06-09T10:00:00Z" "PASS" 0.05 100 50
Write-Audit "M1-T02" 0 "2026-06-09T10:01:00Z" "FAIL" 0.02 100 50
Write-Audit "M1-T02" 1 "2026-06-09T10:06:00Z" "PASS" 0.03 100 50

Write-Host "=== Run: porpoise report --milestone 1 --markdown ===" -ForegroundColor Cyan
Push-Location $WorkDir
try {
    $out = & $exe report --milestone 1 --markdown | Out-String
    if ($LASTEXITCODE -ne 0) { Fail "report exited $LASTEXITCODE" }
} finally { Pop-Location }
Write-Host $out

# Console rollup assertions (ASCII-safe substrings)
# NOTE: single-quoted regex — double quotes would let PowerShell expand $0.
if ($out -notmatch "PASS\s+2\b") { Fail "expected 2 PASS tasks" }
Ok "2 PASS tasks"
if ($out -notmatch '\$0\.1000') { Fail "total cost 0.1000 not found in console" }
Ok "total cost 0.1000 in console"

# Markdown assertions
$md = Join-Path $WorkDir ".porpoise\reports\run-M1.md"
if (-not (Test-Path $md)) { Fail "markdown not written" }
$mdText = Get-Content $md -Raw -Encoding UTF8
if ($mdText -notmatch '\$0\.1000') { Fail "markdown rollup missing total cost" }
if ($mdText -notmatch '\$0\.0500') { Fail "markdown missing per-task cost (T01 0.05 / T02 latest 0.05)" }
Ok "markdown contains total + per-task costs"

Write-Host "`nM28 COST VALIDATION: PASS" -ForegroundColor Green
Write-Host "report cost rollup matches injected ground truth (latest-run cost summed)." -ForegroundColor Green
