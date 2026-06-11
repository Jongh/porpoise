<#
.SYNOPSIS
  M37 launcher validation (no browser, no Claude, no real fleet spawn).
.DESCRIPTION
  Starts the dashboard on a temp project, then validates the launcher HTTP surface:
    1. POST /api/control {decision:redispatch, gate_id} -> control/redispatch-<id>.json written
       + path-injection task id -> 400, evil Origin -> 403
    2. POST /api/launch guards: run_active live.json -> 409, evil Origin -> 403,
       unknown project -> 404
    3. GET /api/config -> effective [conductor] defaults
    4. POST /api/config valid -> workspace.toml [conductor] updated, GET reflects it,
       other sections preserved; whitelist-violation -> 400 + file unchanged; bad value -> 400
  Real detached spawn of a fleet ([함대 실행] success path) is an OPERATOR live check —
  it starts a real conductor process and is out of scope for this no-Claude harness.
  Cleans up registry entries at the end.
#>
[CmdletBinding()]
param(
    [int]$Port = 7889,
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-launch-smoke")
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

$script:proc = $null
function Cleanup {
    if ($script:proc -and -not $script:proc.HasExited) { Stop-Process -Id $script:proc.Id -Force }
    & $exe dashboard --unregister $WorkDir 2>$null | Out-Null
}
function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; Cleanup; exit 1 }
function Ok($m)   { Write-Host "  OK: $m" -ForegroundColor Green }
function PostCode($uri, $body, $headers) {
    try { Invoke-RestMethod -Method Post -Uri $uri -ContentType "application/json" -Headers $headers -Body $body | Out-Null; return 200 }
    catch { return [int]$_.Exception.Response.StatusCode }
}

Write-Host "=== Build (release) ===" -ForegroundColor Cyan
Push-Location $repoRoot
try { & cargo build --release; if ($LASTEXITCODE -ne 0) { exit 1 } } finally { Pop-Location }
$exe = Join-Path $repoRoot "target\release\porpoise.exe"

Write-Host "=== Scaffold + start dashboard ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
$porpoise = Join-Path $WorkDir ".porpoise"
New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "sessions") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "control") | Out-Null
function WriteUtf8($p, $s) { [System.IO.File]::WriteAllText($p, $s, (New-Object System.Text.UTF8Encoding($false))) }
WriteUtf8 (Join-Path $porpoise "project.md") "# launch-smoke`n`n## 작업 목록`n- [ ] M1-T01: t`n"
WriteUtf8 (Join-Path $porpoise "workspace.toml") "[general]`nlanguage = `"en`"`n`n[conductor]`nmode = `"conductor`"`nmax_parallel = 1`n"

$script:proc = Start-Process -FilePath $exe -ArgumentList "dashboard","--no-open","--port","$Port" -WorkingDirectory $WorkDir -PassThru -WindowStyle Hidden
$base = "http://127.0.0.1:$Port"
$up = $false
for ($i = 0; $i -lt 30; $i++) {
    try { Invoke-RestMethod "$base/api/live" -TimeoutSec 2 | Out-Null; $up = $true; break }
    catch { Start-Sleep -Milliseconds 300 }
}
if (-not $up) { Fail "server did not come up" }
Ok "server up"

# 1. redispatch override roundtrip
$r = Invoke-RestMethod -Method Post -Uri "$base/api/control" -ContentType "application/json" -Body '{"gate_id":"M1-T01","decision":"redispatch"}'
if (-not $r.ok) { Fail "redispatch POST not ok" }
$rdFile = Join-Path $porpoise "control\redispatch-M1-T01.json"
if (-not (Test-Path $rdFile)) { Fail "redispatch override file not written" }
$rdJson = Get-Content $rdFile -Raw | ConvertFrom-Json
if ($rdJson.extra_budget -ne 1) { Fail "redispatch extra_budget wrong" }
Ok "POST redispatch -> control/redispatch-<id>.json written"

# 1b. path-injection task id -> 400
if ((PostCode "$base/api/control" '{"gate_id":"../../evil","decision":"redispatch"}' @{}) -ne 400) { Fail "path-injection redispatch should be 400" }
Ok "path-injection redispatch task id -> 400"

# 1c. evil Origin -> 403
if ((PostCode "$base/api/control" '{"gate_id":"M1-T01","decision":"redispatch"}' @{ Origin = "http://evil.example.com" }) -ne 403) { Fail "evil Origin redispatch should be 403" }
Ok "cross-origin redispatch -> 403"

# 2. launch guards
# 2a. run_active live.json -> 409 (already-running)
$live = @{ schema_version="live-1"; run_active=$true; started_at="t"; updated_at="t"; mode="sequential"; total_cost_usd=0.0; budget_usd=$null; tasks=@() } | ConvertTo-Json -Depth 6
WriteUtf8 (Join-Path $porpoise "live.json") $live
if ((PostCode "$base/api/launch" '{}' @{}) -ne 409) { Fail "launch while run_active should be 409" }
Ok "POST launch while run_active -> 409 (double-launch blocked)"
Remove-Item (Join-Path $porpoise "live.json") -Force

# 2b. evil Origin -> 403
if ((PostCode "$base/api/launch" '{}' @{ Origin = "http://evil.example.com" }) -ne 403) { Fail "launch evil Origin should be 403" }
Ok "cross-origin launch -> 403"

# 2c. unknown project -> 404
if ((PostCode "$base/api/launch?project=0000000000000000" '{}' @{}) -ne 404) { Fail "launch unknown project should be 404" }
Ok "launch unknown project id -> 404 (allow-list inherited)"

# 3. GET /api/config -> defaults from scaffolded workspace.toml
$cfg = Invoke-RestMethod "$base/api/config"
if ($cfg.conductor.mode -ne "conductor") { Fail "config mode not exposed" }
if ($cfg.conductor.max_parallel -ne 1) { Fail "config max_parallel wrong" }
Ok "GET /api/config exposes effective [conductor] values"

# 4. POST /api/config valid -> updated + other sections preserved
$r4 = Invoke-RestMethod -Method Post -Uri "$base/api/config" -ContentType "application/json" -Body '{"max_parallel":4,"approval_mode":"gate"}'
if (-not $r4.ok) { Fail "config POST not ok" }
$cfg2 = Invoke-RestMethod "$base/api/config"
if ($cfg2.conductor.max_parallel -ne 4) { Fail "max_parallel not updated" }
if ($cfg2.conductor.approval_mode -ne "gate") { Fail "approval_mode not updated" }
$wsRaw = Get-Content (Join-Path $porpoise "workspace.toml") -Raw
if ($wsRaw -notmatch 'language') { Fail "other section [general] not preserved" }
Ok "POST /api/config updates [conductor], preserves [general]"

# 4b. whitelist violation -> 400 + file unchanged
$before = Get-Content (Join-Path $porpoise "workspace.toml") -Raw
if ((PostCode "$base/api/config" '{"general":"x"}' @{}) -ne 400) { Fail "non-editable key should be 400" }
$after = Get-Content (Join-Path $porpoise "workspace.toml") -Raw
if ($before -ne $after) { Fail "file changed on rejected config write" }
Ok "non-editable key -> 400, file unchanged (atomic)"

# 4c. bad value -> 400
if ((PostCode "$base/api/config" '{"max_parallel":99}' @{}) -ne 400) { Fail "out-of-range value should be 400" }
if ((PostCode "$base/api/config" '{"approval_mode":"nuke"}' @{}) -ne 400) { Fail "bad enum should be 400" }
Ok "out-of-range / bad-enum config values -> 400"

Cleanup
Write-Host "`nM37 LAUNCHER VALIDATION: PASS" -ForegroundColor Green
Write-Host "Launch guards, redispatch channel, and config writes enforced at HTTP level." -ForegroundColor Green
Write-Host "NOTE: real [함대 실행] detached spawn is an operator live check (starts a real fleet)." -ForegroundColor Yellow
