<#
.SYNOPSIS
  M26 change-detection fix reproduction (no Claude needed).

.DESCRIPTION
  Reproduces, at the pure-git level, the bug M26 fixes: when an agent commits its
  work inside the conductor worktree, the OLD detection (`git diff --cached`, i.e.
  index vs current HEAD) returns EMPTY because HEAD moved to the agent's commit and
  the working tree is clean. The FIX diffs against the branch BASE commit, which
  still sees the committed work.

  Asserts: OLD approach -> empty (bug), NEW approach -> non-empty (fix).
#>
[CmdletBinding()]
param(
    [string]$WorkDir = (Join-Path $env:TEMP "porpoise-commit-detect")
)
# Continue (not Stop): git writes progress to stderr; PS 5.1 wraps native stderr as
# NativeCommandError which would abort under Stop. Control flow is driven by explicit
# Fail() checks below, so we don't rely on automatic error termination.
$ErrorActionPreference = "Continue"

function Fail($m) { Write-Host "FAIL: $m" -ForegroundColor Red; exit 1 }
function Ok($m)   { Write-Host "  OK: $m" -ForegroundColor Green }
# Capture stdout only; discard stderr (progress noise) to avoid NativeCommandError.
function G($dir, $a) { (& git -C $dir @a 2>$null) | Out-String }

Write-Host "=== Setup temp repo ===" -ForegroundColor Cyan
if (Test-Path $WorkDir) { Remove-Item $WorkDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null

G $WorkDir @("init", "-b", "main") | Out-Null
G $WorkDir @("config", "user.email", "test@example.com") | Out-Null
G $WorkDir @("config", "user.name", "Test") | Out-Null
Set-Content -Path (Join-Path $WorkDir "seed.txt") -Value "seed" -Encoding ascii
G $WorkDir @("add", "-A") | Out-Null
G $WorkDir @("commit", "-m", "init") | Out-Null

# Branch base = current HEAD (what the conductor records at worktree creation)
$base = (G $WorkDir @("rev-parse", "HEAD")).Trim()
Write-Host "base commit: $base"

Write-Host "`n=== Create worktree from HEAD ===" -ForegroundColor Cyan
$wt = Join-Path $WorkDir "wt"
G $WorkDir @("worktree", "add", "-b", "porpoise/m26", $wt, "HEAD") | Out-Null
if (-not (Test-Path $wt)) { Fail "worktree not created" }

Write-Host "`n=== Agent creates a file AND commits inside the worktree ===" -ForegroundColor Cyan
Set-Content -Path (Join-Path $wt "b.rs") -Value "fn sub(a:i64,b:i64)->i64{a-b}" -Encoding ascii
G $wt @("add", "-A") | Out-Null
G $wt @("commit", "-m", "[M26] add b.rs") | Out-Null
Ok "worktree HEAD moved to agent commit; working tree is clean"

Write-Host "`n=== OLD detection: git diff --cached (index vs current HEAD) ===" -ForegroundColor Cyan
G $wt @("add", "-A") | Out-Null
$old = (G $wt @("diff", "--cached")).Trim()
if ($old -ne "") { Fail "expected OLD approach to be empty (bug condition), got content" }
Ok "OLD approach is EMPTY -> this is the bug (committed work invisible)"

Write-Host "`n=== NEW detection (M26): git diff --cached <base> ===" -ForegroundColor Cyan
$new = (G $wt @("diff", "--cached", $base)).Trim()
if ($new -eq "")          { Fail "NEW approach empty - fix did not capture committed work" }
if ($new -notmatch "b\.rs") { Fail "NEW diff missing b.rs" }
if ($new -notmatch "sub")   { Fail "NEW diff missing sub content" }
Ok "NEW approach captures committed work (contains b.rs / sub)"

# cleanup
G $WorkDir @("worktree", "remove", "--force", $wt) | Out-Null

Write-Host "`nM26 REPRODUCTION: PASS" -ForegroundColor Green
Write-Host "Base-relative diff sees agent-committed work; HEAD-relative diff did not." -ForegroundColor Green
