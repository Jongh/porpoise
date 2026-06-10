<#
.SYNOPSIS
  M32 multi-project dashboard smoke (no browser, no Claude).
.DESCRIPTION
  Creates two temp projects with DIFFERENT synthetic data, registers them,
  starts one dashboard, then asserts: /api/projects lists both; the same
  endpoint returns each project's own ground truth when switched by ?project=;
  unknown id -> 404; no ?project= -> startup dir (backward compat).
  Cleans up registry entries at the end.
#>
[CmdletBinding()]
param(
    [int]$Port = 7885,
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-multi-smoke")
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

$script:proc = $null
function Cleanup {
    if ($script:proc -and -not $script:proc.HasExited) { Stop-Process -Id $script:proc.Id -Force }
    foreach ($p in "$WorkDir\alpha", "$WorkDir\beta") {
        & $exe dashboard --unregister $p 2>$null | Out-Null
    }
}
function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; Cleanup; exit 1 }
function Ok($m)   { Write-Host "  OK: $m" -ForegroundColor Green }

Write-Host "=== Build (release) ===" -ForegroundColor Cyan
Push-Location $repoRoot
try { & cargo build --release; if ($LASTEXITCODE -ne 0) { exit 1 } } finally { Pop-Location }
$exe = Join-Path $repoRoot "target\release\porpoise.exe"

Write-Host "=== Scaffold two projects (alpha / beta) ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
function WriteUtf8($p, $s) { [System.IO.File]::WriteAllText($p, $s, (New-Object System.Text.UTF8Encoding($false))) }
function Scaffold($name, $taskCount, $cost) {
    $proj = Join-Path $WorkDir $name
    $porpoise = Join-Path $proj ".porpoise"
    New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "milestones") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "sessions") | Out-Null
    WriteUtf8 (Join-Path $porpoise "milestones\M1.md") "# M1: $name (v0.1.0)`n"
    $tasks = (1..$taskCount | ForEach-Object { "- [x] M1-T0${_}: task $_" }) -join "`n"
    WriteUtf8 (Join-Path $porpoise "project.md") "# $name`n`n## 작업 목록`n$tasks`n"
    for ($i = 1; $i -le $taskCount; $i++) {
        $rec = @{ schema_version="conductor-4"; task_id="M1-T0$i"; redispatch=0; timestamp="2026-06-10T10:0${i}:00Z"; verdict="PASS"; cost_usd=$cost; input_tokens=100; output_tokens=50 } | ConvertTo-Json
        WriteUtf8 (Join-Path $porpoise "sessions\M1-T0$i-conductor-2026061010000$i-R0.json") $rec
    }
    return $proj
}
# alpha: 1 task, $0.01 each | beta: 3 tasks, $0.10 each (구분되는 ground truth)
$alpha = Scaffold "alpha" 1 0.01
$beta  = Scaffold "beta"  3 0.10

Write-Host "=== Register both, start dashboard from alpha ===" -ForegroundColor Cyan
& $exe dashboard --register $alpha | Out-Null
& $exe dashboard --register $beta | Out-Null
$script:proc = Start-Process -FilePath $exe -ArgumentList "dashboard","--no-open","--port","$Port" -WorkingDirectory $alpha -PassThru -WindowStyle Hidden

$base = "http://127.0.0.1:$Port"
$up = $false
for ($i = 0; $i -lt 30; $i++) {
    try { Invoke-RestMethod "$base/api/projects" -TimeoutSec 2 | Out-Null; $up = $true; break }
    catch { Start-Sleep -Milliseconds 300 }
}
if (-not $up) { Fail "server did not come up" }
Ok "server up"

# 1) /api/projects lists both, current = alpha
$projects = (Invoke-RestMethod "$base/api/projects").projects
$pa = $projects | Where-Object { $_.name -eq "alpha" }
$pb = $projects | Where-Object { $_.name -eq "beta" }
if (-not $pa -or -not $pb) { Fail "/api/projects missing alpha/beta (got $($projects.Count))" }
if (-not $pa.current) { Fail "alpha should be current (startup dir)" }
Ok "/api/projects: alpha(current) + beta listed"

# 2) default scope (no ?project=) = startup dir = alpha
$repDefault = Invoke-RestMethod "$base/api/report?milestone=1"
if ($repDefault.total -ne 1) { Fail "default scope should be alpha (1 task), got $($repDefault.total)" }
Ok "no ?project= -> startup dir (alpha, 1 task) [backward compat]"

# 3) ?project=<beta id> returns beta's ground truth
$repBeta = Invoke-RestMethod "$base/api/report?milestone=1&project=$($pb.id)"
if ($repBeta.total -ne 3) { Fail "beta report should have 3 tasks, got $($repBeta.total)" }
if ([math]::Abs([double]$repBeta.total_cost - 0.30) -gt 0.0001) { Fail "beta total cost != 0.30" }
Ok "?project=beta -> beta data (3 tasks, cost 0.30)"

# 4) switching back to alpha id returns alpha's
$repAlpha = Invoke-RestMethod "$base/api/report?milestone=1&project=$($pa.id)"
if ($repAlpha.total -ne 1 -or [math]::Abs([double]$repAlpha.total_cost - 0.01) -gt 0.0001) { Fail "alpha scoped report wrong" }
Ok "?project=alpha -> alpha data (1 task, cost 0.01)"

# 5) /api/tasks scoped
$tkBeta = Invoke-RestMethod "$base/api/tasks?project=$($pb.id)"
if ($tkBeta.tasks.Count -ne 3) { Fail "beta tasks count != 3" }
Ok "/api/tasks scoped to beta (3 tasks)"

# 6) unknown id -> 404
$code = 0
try { Invoke-RestMethod "$base/api/report?project=deadbeefdeadbeef" | Out-Null }
catch { $code = [int]$_.Exception.Response.StatusCode }
if ($code -ne 404) { Fail "unknown project id should be 404, got $code" }
Ok "unknown project id -> 404"

# 7) /api/live scoped (beta idle)
$liveBeta = Invoke-RestMethod "$base/api/live?project=$($pb.id)"
if ($liveBeta.live.run_active -ne $false) { Fail "beta live should be idle" }
if ($liveBeta.sessions_count -ne 3) { Fail "beta sessions_count != 3" }
Ok "/api/live scoped (beta: idle, 3 sessions)"

Cleanup
Write-Host "`nM32 MULTI-PROJECT SMOKE: PASS" -ForegroundColor Green
Write-Host "Registry allow-list + ?project= scoping verified end-to-end." -ForegroundColor Green
