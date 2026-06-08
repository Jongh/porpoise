<#
.SYNOPSIS
  conductor 라이브 스모크 테스트용 임시 프로젝트를 스캐폴딩한다 (claude 호출 직전까지).

.DESCRIPTION
  cargo lib 스켈레톤 + .porpoise/(conductor 모드, 단일 trivial task) + 초기 커밋을 생성한다.
  실제 `porpoise` 실행(토큰 소모·실제 변경)은 사용자가 직접 수행하며 관찰한다.
  M21 회귀 재현·승격 판단용 하니스.

.PARAMETER Path
  임시 프로젝트 생성 경로 (기본: D:\tmp\porpoise-smoke). 기존 내용은 삭제된다.

.PARAMETER MaxRedispatch
  [conductor] max_redispatch (기본 1).

.EXAMPLE
  pwsh scripts/conductor-smoke.ps1
  # 스캐폴딩 후, 안내된 대로 porpoise를 직접 실행
#>
param(
    [string]$Path = "D:\tmp\porpoise-smoke",
    [int]$MaxRedispatch = 1
)

$ErrorActionPreference = "Stop"
if (Get-Command cargo -ErrorAction SilentlyContinue) { } else { $env:Path += ";$env:USERPROFILE\.cargo\bin" }

Write-Host "=== conductor 스모크 스캐폴딩: $Path ===" -ForegroundColor Cyan

Remove-Item -Recurse -Force $Path -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
cargo new --lib $Path | Out-Null
New-Item -ItemType Directory -Force "$Path\.porpoise\milestones","$Path\.porpoise\sessions" | Out-Null

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
verify_timeout_secs = 180

[conductor]
mode = "conductor"
max_redispatch = $MaxRedispatch
"@ | Out-File -FilePath "$Path\.porpoise\workspace.toml" -Encoding utf8

@"
# smoke-test 프로젝트

conductor 루프 종단 간 검증용 임시 프로젝트.

## 작업 목록
- [ ] M1-T01: src/lib.rs에 두 정수를 더하는 add(a, b) 함수와 단위 테스트를 추가
"@ | Out-File -FilePath "$Path\.porpoise\project.md" -Encoding utf8

@"
# M1: conductor 스모크 테스트 (v0.1.0)

## 목표
conductor 루프가 단일 task를 Brief→Dispatch→Verify→Integrate로 끝까지 처리하는지 검증한다.

## 작업 목록
- [ ] M1-T01: src/lib.rs에 두 정수를 더하는 add(a, b) 함수와 단위 테스트를 추가

## 메타데이터
- created: smoke
- status: active
"@ | Out-File -FilePath "$Path\.porpoise\milestones\M1.md" -Encoding utf8

Add-Content -Path "$Path\.gitignore" -Value ".porpoise/" -Encoding utf8

git -C $Path add -A | Out-Null
git -C $Path commit -m "init: cargo 스켈레톤" | Out-Null

Write-Host "스캐폴딩 완료." -ForegroundColor Green
Write-Host ""
Write-Host "다음 (직접 실행 — 토큰 소모):" -ForegroundColor Yellow
Write-Host "  `$P = 'target\release\porpoise.exe'   # 또는 빌드한 바이너리 경로"
Write-Host "  Push-Location '$Path'"
Write-Host "  & `$P doctor      # conductor 활성 확인"
Write-Host "  & `$P             # conductor 실행 (프롬프트에 y)"
Write-Host "  Pop-Location"
Write-Host ""
Write-Host "실행 후 판정: git log -1 / src\lib.rs / git worktree list / git branch / .porpoise\sessions"
