<#
.SYNOPSIS
  M35 embedded-dashboard validation (no browser, no Claude).
.DESCRIPTION
  Validates coexistence + stale filtering through the real binary:
    1. start `porpoise dashboard` (terminal A simulation)
    2. second bind attempt on the same port fails gracefully (CLI error, not hang)
    3. registry stale filter: register a project, delete it, /api/projects hides it
  (In-process serve_in_background + HTTP is covered by unit tests.)
#>
[CmdletBinding()]
param(
    [int]$Port = 7887,
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-embed-smoke")
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

$script:proc = $null
function Cleanup {
    if ($script:proc -and -not $script:proc.HasExited) { Stop-Process -Id $script:proc.Id -Force }
    # 삭제된 프로젝트는 정규화(\\?\) 경로로만 id가 일치 — registry.json에서 읽어 해제.
    # unregister 실패(이미 없음)는 무해하므로 cmd /c로 stderr를 흡수해 Stop 정책을 피한다.
    $regFile = Join-Path $env:USERPROFILE ".porpoise\registry.json"
    if (Test-Path $regFile) {
        try {
            $reg = Get-Content $regFile -Raw -Encoding UTF8 | ConvertFrom-Json
            foreach ($e in $reg.projects) {
                if ($e.path -match [regex]::Escape("porpoise-embed-smoke")) {
                    cmd /c "`"$exe`" dashboard --unregister `"$($e.path)`" >nul 2>nul"
                }
            }
        } catch {}
    }
}
function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; Cleanup; exit 1 }
function Ok($m)   { Write-Host "  OK: $m" -ForegroundColor Green }

Write-Host "=== Build (release) ===" -ForegroundColor Cyan
Push-Location $repoRoot
try { & cargo build --release; if ($LASTEXITCODE -ne 0) { exit 1 } } finally { Pop-Location }
$exe = Join-Path $repoRoot "target\release\porpoise.exe"

Write-Host "=== Scaffold main + ghost projects ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
foreach ($name in "main", "ghost") {
    New-Item -ItemType Directory -Force -Path (Join-Path $WorkDir "$name\.porpoise\sessions") | Out-Null
}
& $exe dashboard --register (Join-Path $WorkDir "main") | Out-Null
& $exe dashboard --register (Join-Path $WorkDir "ghost") | Out-Null

# 1. start dashboard on main
$script:proc = Start-Process -FilePath $exe -ArgumentList "dashboard","--no-open","--port","$Port" -WorkingDirectory (Join-Path $WorkDir "main") -PassThru -WindowStyle Hidden
$base = "http://127.0.0.1:$Port"
$up = $false
for ($i = 0; $i -lt 30; $i++) {
    try { Invoke-RestMethod "$base/api/live" -TimeoutSec 2 | Out-Null; $up = $true; break }
    catch { Start-Sleep -Milliseconds 300 }
}
if (-not $up) { Fail "server did not come up" }
Ok "dashboard up on $Port"

# 2. second instance on same port -> graceful CLI failure (exit fast, server A unaffected)
$second = Start-Process -FilePath $exe -ArgumentList "dashboard","--no-open","--port","$Port" -WorkingDirectory (Join-Path $WorkDir "main") -PassThru -WindowStyle Hidden
$exited = $second.WaitForExit(10000)
if (-not $exited) { Stop-Process -Id $second.Id -Force; Fail "second instance should exit quickly (port busy)" }
if ($second.ExitCode -eq 0) { Fail "second instance should fail (port busy), got exit 0" }
Ok "second bind fails gracefully (exit $($second.ExitCode), no hang)"
$alive = Invoke-RestMethod "$base/api/live" -TimeoutSec 3
if ($null -eq $alive.live) { Fail "original server affected by second bind attempt" }
Ok "original server unaffected (coexistence path)"

# 3. stale filter: both registered -> ghost listed; delete ghost -> hidden
$p1 = (Invoke-RestMethod "$base/api/projects").projects
if (-not ($p1 | Where-Object { $_.name -eq "ghost" })) { Fail "ghost should be listed while it exists" }
Remove-Item (Join-Path $WorkDir "ghost") -Recurse -Force
$p2 = (Invoke-RestMethod "$base/api/projects").projects
if ($p2 | Where-Object { $_.name -eq "ghost" }) { Fail "deleted ghost still listed (stale filter broken)" }
if (-not ($p2 | Where-Object { $_.name -eq "main" })) { Fail "main missing from list" }
Ok "stale project hidden from /api/projects (read-only filter)"

Cleanup
Write-Host "`nM35 EMBED/COEXIST VALIDATION: PASS" -ForegroundColor Green
Write-Host "Port coexistence and stale registry filtering verified." -ForegroundColor Green
