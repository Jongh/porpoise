<#
.SYNOPSIS
  M30 dashboard smoke test (no browser).
.DESCRIPTION
  Scaffolds a temp project with synthetic data, starts `porpoise dashboard --no-open`
  in the background, hits the JSON API over HTTP, asserts the responses, then stops
  the server. Validates server + API end-to-end through the real binary.
#>
[CmdletBinding()]
param(
    [int]$Port = 7879,
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-dashboard-smoke")
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force } ; exit 1 }
function Ok($m)   { Write-Host "  OK: $m" -ForegroundColor Green }

Write-Host "=== Build (release) ===" -ForegroundColor Cyan
Push-Location $repoRoot
try { & cargo build --release; if ($LASTEXITCODE -ne 0) { exit 1 } } finally { Pop-Location }
$exe = Join-Path $repoRoot "target\release\porpoise.exe"

Write-Host "=== Scaffold temp project ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
$porpoise = Join-Path $WorkDir ".porpoise"
New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "milestones") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "sessions") | Out-Null
function WriteUtf8($p, $s) { [System.IO.File]::WriteAllText($p, $s, (New-Object System.Text.UTF8Encoding($false))) }
WriteUtf8 (Join-Path $porpoise "milestones\M1.md") "# M1: 스모크 (v0.1.0)`n"
WriteUtf8 (Join-Path $porpoise "project.md") "# proj`n`n## 작업 목록`n- [x] M1-T01: a`n- [ ] M1-T02: b (deps: M1-T01)`n"
$rec = @{ schema_version="conductor-4"; task_id="M1-T01"; redispatch=0; timestamp="2026-06-09T10:00:00Z"; verdict="PASS"; cost_usd=0.05; input_tokens=100; output_tokens=50 } | ConvertTo-Json
WriteUtf8 (Join-Path $porpoise "sessions\M1-T01-conductor-20260609-100000-R0.json") $rec

Write-Host "=== Start dashboard (--no-open, port $Port) ===" -ForegroundColor Cyan
$proc = Start-Process -FilePath $exe -ArgumentList "dashboard","--no-open","--port","$Port" -WorkingDirectory $WorkDir -PassThru -WindowStyle Hidden

$base = "http://127.0.0.1:$Port"
$up = $false
for ($i = 0; $i -lt 30; $i++) {
    try { Invoke-RestMethod "$base/api/milestones" -TimeoutSec 2 | Out-Null; $up = $true; break }
    catch { Start-Sleep -Milliseconds 300 }
}
if (-not $up) { Fail "server did not come up on $base" }
Ok "server is up"

# /api/milestones
$ms = Invoke-RestMethod "$base/api/milestones"
if ($ms.milestones.Count -lt 1) { Fail "no milestones" }
if ($ms.milestones[0].number -ne 1) { Fail "milestone number mismatch" }
Ok "GET /api/milestones (M1)"

# /api/report?milestone=1
$rep = Invoke-RestMethod "$base/api/report?milestone=1"
if ($rep.total -ne 1) { Fail "report.total != 1" }
if ($rep.passed -ne 1) { Fail "report.passed != 1" }
if ([math]::Abs([double]$rep.total_cost - 0.05) -gt 0.0001) { Fail "report.total_cost != 0.05" }
if ($rep.tasks[0].task_id -ne "M1-T01") { Fail "report task_id mismatch" }
Ok "GET /api/report (total=1, cost=0.05, task M1-T01)"

# /api/tasks
$tk = Invoke-RestMethod "$base/api/tasks"
if ($tk.tasks.Count -ne 2) { Fail "tasks count != 2" }
$t1 = $tk.tasks | Where-Object { $_.id -eq "M1-T01" }
$t2 = $tk.tasks | Where-Object { $_.id -eq "M1-T02" }
if ($t1.status -ne "done") { Fail "M1-T01 status != done (got $($t1.status))" }
if ($t2.status -ne "ready") { Fail "M1-T02 status != ready (got $($t2.status))" }
Ok "GET /api/tasks (T01 done, T02 ready)"

# index served
$idx = Invoke-WebRequest "$base/" -UseBasicParsing
if ($idx.StatusCode -ne 200) { Fail "index not served" }
Ok "GET / (index.html)"

Stop-Process -Id $proc.Id -Force
Write-Host "`nM30 DASHBOARD SMOKE: PASS" -ForegroundColor Green
Write-Host "Server + JSON API validated end-to-end." -ForegroundColor Green
