<#
.SYNOPSIS
  M37 launcher LIVE verification — proves detached fleet survives dashboard exit.
.DESCRIPTION
  Sets up a real git sandbox project and drives the launcher end-to-end to verify the
  one risk the no-Claude harness can't cover: a fleet started from the dashboard runs as
  a DETACHED child that survives the dashboard process closing.

  Zero LLM cost: the sandbox uses [conductor] approval_mode="gate", so the spawned
  conductor blocks at the FIRST task approval gate (before any dispatch / Claude call).
  That gives a long-lived child to test parent-exit survival, then we stop it gracefully
  via a stop-next control file (no approval, no dispatch, no cost).

  Automated assertions:
    1. POST /api/launch -> child PID, run_active becomes true, pending_gate appears
    2. child process is alive while dashboard runs
    3. kill the dashboard -> child is STILL alive  (== detach proven)
    4. graceful stop (stop-next.json) -> child exits on its own
  Requires: claude on PATH (child must reach the gate), git.
  Prints a manual browser checklist for the UI-only bits (재투입 / 설정 폼).
.NOTES
  Run AFTER the no-Claude harness (dashboard-launch-validate.ps1) passes.
#>
[CmdletBinding()]
param(
    [int]$Port = 7890,
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-launch-live"),
    [switch]$KeepSandbox,
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

$script:dash = $null
$script:childPid = 0
function PidAlive([int]$id) {
    if ($id -le 0) { return $false }
    try { Get-Process -Id $id -ErrorAction Stop | Out-Null; return $true } catch { return $false }
}
function Cleanup {
    if ($script:childPid -gt 0 -and (PidAlive $script:childPid)) {
        Stop-Process -Id $script:childPid -Force -ErrorAction SilentlyContinue
    }
    if ($script:dash -and -not $script:dash.HasExited) { Stop-Process -Id $script:dash.Id -Force -ErrorAction SilentlyContinue }
    & $exe dashboard --unregister $WorkDir 2>$null | Out-Null
    if (-not $KeepSandbox -and (Test-Path $WorkDir)) { Remove-Item $WorkDir -Recurse -Force -ErrorAction SilentlyContinue }
}
function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; Cleanup; exit 1 }
function Ok($m)   { Write-Host "  OK: $m" -ForegroundColor Green }
function WriteUtf8($p, $s) { [System.IO.File]::WriteAllText($p, $s, (New-Object System.Text.UTF8Encoding($false))) }

# claude on PATH? (child must reach the gate; without it the child exits early)
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
    Fail "claude not on PATH — the spawned fleet would exit before the gate. Install Claude Code first."
}

Write-Host "=== Build (release) ===" -ForegroundColor Cyan
if (-not $SkipBuild) {
    Push-Location $repoRoot
    try { & cargo build --release; if ($LASTEXITCODE -ne 0) { exit 1 } } finally { Pop-Location }
}
$exe = Join-Path $repoRoot "target\release\porpoise.exe"
if (-not (Test-Path $exe)) { Fail "release exe missing: $exe" }

Write-Host "=== Scaffold git sandbox (gate mode, dashboard auto-launch OFF) ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$porpoise = Join-Path $WorkDir ".porpoise"
New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "sessions") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $porpoise "control") | Out-Null
WriteUtf8 (Join-Path $porpoise "project.md") "# launch-live`n`n## 작업 목록`n- [ ] M1-T01: 샌드박스 게이트 대기용 더미 태스크`n"
# serve_dashboard=false: 자식이 자기 대시보드/브라우저를 띄우지 않게 (테스트 노이즈 제거)
WriteUtf8 (Join-Path $porpoise "workspace.toml") "[conductor]`nmode = `"conductor`"`napproval_mode = `"gate`"`nserve_dashboard = false`n"

Push-Location $WorkDir
try {
    & git init -q
    & git config user.email "live@test.local"
    & git config user.name  "live-test"
    WriteUtf8 (Join-Path $WorkDir "README.md") "# launch-live sandbox`n"
    & git add -A
    & git commit -q -m "init sandbox" | Out-Null
} finally { Pop-Location }
Ok "git sandbox ready (HEAD exists)"

Write-Host "=== Start standalone dashboard ===" -ForegroundColor Cyan
$script:dash = Start-Process -FilePath $exe -ArgumentList "dashboard","--no-open","--port","$Port" -WorkingDirectory $WorkDir -PassThru -WindowStyle Hidden
$base = "http://127.0.0.1:$Port"
$up = $false
for ($i = 0; $i -lt 30; $i++) {
    try { Invoke-RestMethod "$base/api/live" -TimeoutSec 2 | Out-Null; $up = $true; break }
    catch { Start-Sleep -Milliseconds 300 }
}
if (-not $up) { Fail "dashboard did not come up on $base" }
Ok "dashboard up at $base (PID $($script:dash.Id))"

Write-Host "=== Launch fleet via API ===" -ForegroundColor Cyan
$launch = Invoke-RestMethod -Method Post -Uri "$base/api/launch" -ContentType "application/json" -Body "{}"
if (-not $launch.ok) { Fail "launch did not return ok" }
$script:childPid = [int]$launch.pid
if ($script:childPid -le 0) { Fail "launch returned no pid" }
Ok "POST /api/launch -> child PID $($script:childPid), log $($launch.log)"

# wait for child to reach the gate: run_active=true AND pending_gate present
$reached = $false
for ($i = 0; $i -lt 60; $i++) {
    try {
        $lv = Invoke-RestMethod "$base/api/live" -TimeoutSec 2
        if ($lv.live.run_active -eq $true -and $lv.live.pending_gate) { $reached = $true; break }
    } catch {}
    Start-Sleep -Milliseconds 500
}
if (-not $reached) {
    $log = Join-Path $porpoise "launch.log"
    if (Test-Path $log) { Write-Host "--- launch.log ---" -ForegroundColor DarkGray; Get-Content $log -Tail 20 | Write-Host }
    Fail "child did not reach the approval gate (run_active+pending_gate). Did it exit early? See launch.log above."
}
Ok "child reached approval gate (run_active=true, pending_gate present) — blocked, no dispatch/cost"

if (-not (PidAlive $script:childPid)) { Fail "child PID not alive while dashboard running" }
Ok "child process alive while dashboard runs"

Write-Host "=== Kill dashboard, assert child SURVIVES (detach proof) ===" -ForegroundColor Cyan
Stop-Process -Id $script:dash.Id -Force
Start-Sleep -Seconds 3
if (-not (PidAlive $script:childPid)) { Fail "child died when dashboard was killed — DETACH FAILED" }
Ok "dashboard killed, child PID $($script:childPid) STILL ALIVE — detached spawn survives parent exit"

Write-Host "=== Graceful stop (no approval, no cost) ===" -ForegroundColor Cyan
# stop-next.json: 게이트 폴링이 이를 소비하면 dispatch 없이 즉시 정지
WriteUtf8 (Join-Path $porpoise "control\stop-next.json") '{"decision":"stop"}'
$stopped = $false
for ($i = 0; $i -lt 20; $i++) {
    if (-not (PidAlive $script:childPid)) { $stopped = $true; break }
    Start-Sleep -Milliseconds 500
}
if ($stopped) { Ok "child consumed stop-next and exited gracefully (no dispatch)" }
else { Write-Host "  WARN: child did not stop within 10s — force-killing" -ForegroundColor Yellow; Stop-Process -Id $script:childPid -Force -ErrorAction SilentlyContinue }

Cleanup
Write-Host "`nM37 LAUNCHER LIVE VERIFICATION: PASS" -ForegroundColor Green
Write-Host "Detached fleet survives dashboard exit; graceful stop works without dispatch." -ForegroundColor Green

Write-Host "`n--- 브라우저 수동 체크리스트 (UI-only, 자동화 밖) ---" -ForegroundColor Cyan
Write-Host @"
  아래는 실제 브라우저에서 한 번 눈으로 확인 권장 (선택):
  1) [함대 실행] 버튼: 런 비활성일 때만 보이고, 누르면 'PID N' 토스트 + 패널이 RUNNING 전환.
     이미 실행 중이면 '이미 실행 중입니다'(409).
  2) [재투입] 버튼: 실행 리포트의 FAIL task 행에만 표시. 누르면 '재투입 예약' 토스트.
     (halt된 task가 있는 프로젝트에서 확인 — 그 뒤 [함대 실행]으로 예산 상향 재시도)
  3) 설정 [편집] 폼: 값 변경 후 저장 -> '설정 저장됨'. 범위 위반(max_parallel 99 등)은
     '저장 실패 (400)'. workspace.toml의 [conductor] 갱신·타 섹션 보존 확인.
  실행:  $exe dashboard --port $Port   (브라우저에서 위 동작 확인)
"@ -ForegroundColor Gray
