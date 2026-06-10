<#
.SYNOPSIS
  M31 live streaming validation (no Claude, no browser).
.DESCRIPTION
  Starts the dashboard, replays a conductor run by writing live.json through its
  lifecycle (start -> dispatch -> verify -> merged -> finish), subscribes to the
  SSE stream over raw HTTP, and asserts that change events arrive with the right
  content. Also checks the /api/live fallback endpoint.
#>
[CmdletBinding()]
param(
    [int]$Port = 7881,
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-live-smoke")
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }; exit 1 }
function Ok($m)   { Write-Host "  OK: $m" -ForegroundColor Green }

Write-Host "=== Build (release) ===" -ForegroundColor Cyan
Push-Location $repoRoot
try { & cargo build --release; if ($LASTEXITCODE -ne 0) { exit 1 } } finally { Pop-Location }
$exe = Join-Path $repoRoot "target\release\porpoise.exe"

Write-Host "=== Scaffold ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
$porpoise = Join-Path $WorkDir ".porpoise"
New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "sessions") | Out-Null
function WriteLive($obj) {
    $json = $obj | ConvertTo-Json -Depth 6
    [System.IO.File]::WriteAllText((Join-Path $porpoise "live.json"), $json, (New-Object System.Text.UTF8Encoding($false)))
}

Write-Host "=== Start dashboard ===" -ForegroundColor Cyan
$proc = Start-Process -FilePath $exe -ArgumentList "dashboard","--no-open","--port","$Port" -WorkingDirectory $WorkDir -PassThru -WindowStyle Hidden
$base = "http://127.0.0.1:$Port"
$up = $false
for ($i = 0; $i -lt 30; $i++) {
    try { Invoke-RestMethod "$base/api/live" -TimeoutSec 2 | Out-Null; $up = $true; break }
    catch { Start-Sleep -Milliseconds 300 }
}
if (-not $up) { Fail "server did not come up" }
Ok "server up"

# 1) /api/live fallback: idle when no live.json
$idle = Invoke-RestMethod "$base/api/live"
if ($idle.live.run_active -ne $false) { Fail "expected idle (run_active=false)" }
Ok "/api/live idle fallback"

# 2) Subscribe SSE on a background runspace collecting raw lines
$collector = [System.Collections.Concurrent.ConcurrentQueue[string]]::new()
$rs = [runspacefactory]::CreateRunspace(); $rs.Open()
$ps = [powershell]::Create(); $ps.Runspace = $rs
[void]$ps.AddScript({
    param($url, $q)
    $req = [System.Net.HttpWebRequest]::Create($url)
    $req.ReadWriteTimeout = 30000
    $resp = $req.GetResponse()
    $reader = New-Object System.IO.StreamReader($resp.GetResponseStream())
    while (-not $reader.EndOfStream) {
        $line = $reader.ReadLine()
        if ($line) { $q.Enqueue($line) }
    }
}).AddArgument("$base/api/events").AddArgument($collector)
$handle = $ps.BeginInvoke()
Start-Sleep -Milliseconds 800   # allow initial event

# 3) Replay a conductor lifecycle in live.json
Write-Host "=== Replay lifecycle ===" -ForegroundColor Cyan
$nowIso = "2026-06-10T12:00:00+09:00"
WriteLive @{ schema_version="live-1"; run_active=$true; started_at=$nowIso; updated_at=$nowIso; mode="sequential"; total_cost_usd=0.0; budget_usd=1.0; tasks=@() }
Start-Sleep -Milliseconds 900
WriteLive @{ schema_version="live-1"; run_active=$true; started_at=$nowIso; updated_at=$nowIso; mode="sequential"; total_cost_usd=0.0; budget_usd=1.0; tasks=@(@{task_id="M1-T01"; phase="dispatch"; redispatch=0}) }
Start-Sleep -Milliseconds 900
WriteLive @{ schema_version="live-1"; run_active=$true; started_at=$nowIso; updated_at=$nowIso; mode="sequential"; total_cost_usd=0.07; budget_usd=1.0; tasks=@(@{task_id="M1-T01"; phase="merged"; redispatch=0}) }
Start-Sleep -Milliseconds 900
WriteLive @{ schema_version="live-1"; run_active=$false; started_at=$nowIso; updated_at=$nowIso; mode="sequential"; total_cost_usd=0.07; budget_usd=1.0; tasks=@(@{task_id="M1-T01"; phase="merged"; redispatch=0}) }
Start-Sleep -Milliseconds 1200

# 4) Assert SSE events
$lines = @(); $tmpLine = ""
while ($collector.TryDequeue([ref]$tmpLine)) { $lines += $tmpLine }
$dataLines = $lines | Where-Object { $_ -like "data:*" }
Write-Host ("received {0} SSE data events" -f $dataLines.Count)
if ($dataLines.Count -lt 4) { Fail "expected >=4 SSE events (initial + 3+ changes), got $($dataLines.Count)" }
Ok "SSE event count >= 4 (initial + changes)"
if (-not ($dataLines | Where-Object { $_ -match '"phase":"dispatch"' })) { Fail "no dispatch-phase event" }
Ok "dispatch phase event observed"
if (-not ($dataLines | Where-Object { $_ -match '"phase":"merged"' })) { Fail "no merged-phase event" }
Ok "merged phase event observed"
if (-not ($dataLines | Where-Object { $_ -match '"run_active":false' })) { Fail "no finish (run_active=false) event" }
Ok "finish event observed (run_active=false)"

# 5) Server still responsive while SSE connection is open (thread isolation)
$alive = Invoke-RestMethod "$base/api/live" -TimeoutSec 3
if ($alive.live.run_active -ne $false) { Fail "/api/live wrong after replay" }
Ok "server responsive during long-lived SSE (per-request threads)"

$ps.Stop(); $rs.Close()
Stop-Process -Id $proc.Id -Force
Write-Host "`nM31 LIVE STREAMING VALIDATION: PASS" -ForegroundColor Green
Write-Host "live.json lifecycle was streamed over SSE with correct content." -ForegroundColor Green
