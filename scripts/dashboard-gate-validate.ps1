<#
.SYNOPSIS
  M33 gate control validation (no browser, no Claude).
.DESCRIPTION
  Starts the dashboard on a temp project, then validates the control API and the
  full gate round-trip at the HTTP level:
    1. POST approve -> control/gate-<id>.json written with correct content
    2. POST stop without gate_id -> control/stop-next.json (graceful stop)
    3. evil Origin -> 403, path-injection gate_id -> 400, unknown project -> 404
    4. fake pending gate in live.json -> /api/live exposes it (UI data path)
  Cleans up registry entries at the end.
#>
[CmdletBinding()]
param(
    [int]$Port = 7886,
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-gate-smoke")
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

Write-Host "=== Build (release) ===" -ForegroundColor Cyan
Push-Location $repoRoot
try { & cargo build --release; if ($LASTEXITCODE -ne 0) { exit 1 } } finally { Pop-Location }
$exe = Join-Path $repoRoot "target\release\porpoise.exe"

Write-Host "=== Scaffold + start dashboard ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
$porpoise = Join-Path $WorkDir ".porpoise"
New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "sessions") | Out-Null
function WriteUtf8($p, $s) { [System.IO.File]::WriteAllText($p, $s, (New-Object System.Text.UTF8Encoding($false))) }
WriteUtf8 (Join-Path $porpoise "project.md") "# gate-smoke`n`n## 작업 목록`n- [ ] M1-T01: t`n"

$script:proc = Start-Process -FilePath $exe -ArgumentList "dashboard","--no-open","--port","$Port" -WorkingDirectory $WorkDir -PassThru -WindowStyle Hidden
$base = "http://127.0.0.1:$Port"
$up = $false
for ($i = 0; $i -lt 30; $i++) {
    try { Invoke-RestMethod "$base/api/live" -TimeoutSec 2 | Out-Null; $up = $true; break }
    catch { Start-Sleep -Milliseconds 300 }
}
if (-not $up) { Fail "server did not come up" }
Ok "server up"

# 1. POST approve -> gate file written
$r = Invoke-RestMethod -Method Post -Uri "$base/api/control" -ContentType "application/json" -Body '{"gate_id":"m1-t01-120000","decision":"approve"}'
if (-not $r.ok) { Fail "approve POST not ok" }
$gateFile = Join-Path $porpoise "control\gate-m1-t01-120000.json"
if (-not (Test-Path $gateFile)) { Fail "gate response file not written" }
if ((Get-Content $gateFile -Raw) -notmatch "approve") { Fail "gate file content wrong" }
Ok "POST approve -> control/gate-*.json written"

# 2. POST stop without gate_id -> stop-next.json
$r2 = Invoke-RestMethod -Method Post -Uri "$base/api/control" -ContentType "application/json" -Body '{"decision":"stop"}'
if (-not (Test-Path (Join-Path $porpoise "control\stop-next.json"))) { Fail "stop-next.json not written" }
Ok "POST stop (no gate_id) -> stop-next.json (graceful stop)"

# 3a. evil Origin -> 403
$code = 0
try { Invoke-RestMethod -Method Post -Uri "$base/api/control" -ContentType "application/json" -Headers @{ Origin = "http://evil.example.com" } -Body '{"decision":"stop"}' | Out-Null }
catch { $code = [int]$_.Exception.Response.StatusCode }
if ($code -ne 403) { Fail "evil Origin should be 403, got $code" }
Ok "cross-origin POST -> 403 (CSRF blocked)"

# 3b. path-injection gate_id -> 400
$code = 0
try { Invoke-RestMethod -Method Post -Uri "$base/api/control" -ContentType "application/json" -Body '{"gate_id":"../../evil","decision":"approve"}' | Out-Null }
catch { $code = [int]$_.Exception.Response.StatusCode }
if ($code -ne 400) { Fail "path injection should be 400, got $code" }
Ok "path-injection gate_id -> 400"

# 3c. unknown project -> 404
$code = 0
try { Invoke-RestMethod -Method Post -Uri "$base/api/control?project=0000000000000000" -ContentType "application/json" -Body '{"decision":"stop"}' | Out-Null }
catch { $code = [int]$_.Exception.Response.StatusCode }
if ($code -ne 404) { Fail "unknown project should be 404, got $code" }
Ok "unknown project id -> 404 (allow-list inherited)"

# 4. fake pending gate in live.json -> /api/live exposes it (M34: kind 포함)
$live = @{ schema_version="live-1"; run_active=$true; started_at="t"; updated_at="t"; mode="sequential"; total_cost_usd=0.0; budget_usd=$null; tasks=@(); pending_gate=@{ id="m1-t01-999999"; prompt="'M1-T01' 작업을 지휘하시겠습니까?"; kind="text" } } | ConvertTo-Json -Depth 6
WriteUtf8 (Join-Path $porpoise "live.json") $live
$lv = Invoke-RestMethod "$base/api/live"
if ($lv.live.pending_gate.id -ne "m1-t01-999999") { Fail "/api/live missing pending_gate" }
if ($lv.live.pending_gate.kind -ne "text") { Fail "pending_gate.kind not exposed" }
Ok "/api/live exposes pending_gate with kind (UI data path)"

# 5. M34: stop_pending visibility — stop-next.json 존재가 payload에 반영
if ($lv.stop_pending -ne $true) { Fail "stop_pending should be true (stop-next.json from step 2 exists)" }
Ok "stop_pending=true exposed (stop reservation visible)"
Remove-Item (Join-Path $porpoise "control\stop-next.json") -Force
$lv2 = Invoke-RestMethod "$base/api/live"
if ($lv2.stop_pending -ne $false) { Fail "stop_pending should clear after consume" }
Ok "stop_pending clears when stop-next consumed"

# 6. M34: text gate response roundtrip — POST with text -> file contains escaped text
$r6 = Invoke-RestMethod -Method Post -Uri "$base/api/control" -ContentType "application/json" -Body '{"gate_id":"rel-100","decision":"approve","text":"v9.9.9"}'
$relFile = Join-Path $porpoise "control\gate-rel-100.json"
if (-not (Test-Path $relFile)) { Fail "text gate response not written" }
$relJson = Get-Content $relFile -Raw | ConvertFrom-Json
if ($relJson.text -ne "v9.9.9") { Fail "text not preserved in response file" }
Ok "text gate response roundtrip (text preserved)"

Cleanup
Write-Host "`nM33 GATE CONTROL VALIDATION: PASS" -ForegroundColor Green
Write-Host "Control API writes confined to control/, security boundaries enforced." -ForegroundColor Green
