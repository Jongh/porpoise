#!/usr/bin/env bash
# M38 — Unix port of dashboard-launch-live.ps1.
#
# Verifies the launcher on Unix: a fleet started from the dashboard runs as a DETACHED
# child (process_group(0)) that survives the dashboard process exiting, and that after the
# run ends the run lock no longer blocks an immediate relaunch (M38 dead-PID lock).
#
# Zero LLM cost: sandbox uses [conductor] approval_mode="gate" + serve_dashboard=false, so
# the spawned conductor blocks at the FIRST task approval gate (before any dispatch/Claude).
# We never approve; we stop via a stop-next control file.
#
# Requires: claude on PATH (child must reach the gate), git, curl.
# Usage: scripts/dashboard-launch-live.sh [PORT]
set -u
PORT="${1:-7891}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKDIR="${TMPDIR:-/tmp}/porpoise-launch-live-$$"
EXE="$REPO_ROOT/target/release/porpoise"
DASH_PID=""
CHILD_PID=""
CHILD2_PID=""

ok()   { printf '  OK: %s\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1"; cleanup; exit 1; }
alive() { [ -n "$1" ] && kill -0 "$1" 2>/dev/null; }
cleanup() {
  for p in "$CHILD_PID" "$CHILD2_PID" "$DASH_PID"; do alive "$p" && kill -9 "$p" 2>/dev/null; done
  "$EXE" dashboard --unregister "$WORKDIR" >/dev/null 2>&1 || true
  rm -rf "$WORKDIR"
}
launch_pid() { # POST /api/launch, echo child pid from {"ok":true,"pid":N,...}
  curl -s -X POST "http://127.0.0.1:$PORT/api/launch" -H 'Content-Type: application/json' --data "${1:-{}}" \
    | grep -o '"pid":[0-9]*' | grep -o '[0-9]*'
}
http_code() { curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/api/launch" -H 'Content-Type: application/json' --data "${1:-{}}"; }

command -v claude >/dev/null 2>&1 || fail "claude not on PATH (child would exit before the gate)"
command -v git    >/dev/null 2>&1 || fail "git not on PATH"
command -v curl   >/dev/null 2>&1 || fail "curl not on PATH"

echo "=== Build (release) ==="
( cd "$REPO_ROOT" && cargo build --release ) || exit 1
[ -x "$EXE" ] || fail "release exe missing: $EXE"

echo "=== Scaffold git sandbox (gate mode, serve_dashboard=false) ==="
rm -rf "$WORKDIR"; mkdir -p "$WORKDIR/.porpoise/sessions" "$WORKDIR/.porpoise/control"
printf '# launch-live\n\n## 작업 목록\n- [ ] M1-T01: 게이트 대기용 더미 태스크\n' > "$WORKDIR/.porpoise/project.md"
printf '[conductor]\nmode = "conductor"\napproval_mode = "gate"\nserve_dashboard = false\n' > "$WORKDIR/.porpoise/workspace.toml"
( cd "$WORKDIR" && git init -q && git config user.email live@test.local && git config user.name live-test \
  && printf '# sandbox\n' > README.md && git add -A && git commit -q -m "init sandbox" )
ok "git sandbox ready"

echo "=== Start standalone dashboard ==="
( cd "$WORKDIR" && "$EXE" dashboard --no-open --port "$PORT" ) >/dev/null 2>&1 &
DASH_PID=$!
up=0
for _ in $(seq 1 30); do curl -s "http://127.0.0.1:$PORT/api/live" >/dev/null 2>&1 && { up=1; break; }; sleep 0.3; done
[ "$up" = 1 ] || fail "dashboard did not come up"
ok "dashboard up (PID $DASH_PID)"

echo "=== Launch fleet ==="
CHILD_PID="$(launch_pid '{}')"
[ -n "$CHILD_PID" ] || fail "launch returned no pid"
ok "POST /api/launch -> child PID $CHILD_PID"

reached=0
for _ in $(seq 1 60); do
  body="$(curl -s "http://127.0.0.1:$PORT/api/live")"
  echo "$body" | grep -q '"run_active":true' && echo "$body" | grep -q '"pending_gate"' && { reached=1; break; }
  sleep 0.5
done
[ "$reached" = 1 ] || { tail -n 20 "$WORKDIR/.porpoise/launch.log" 2>/dev/null; fail "child did not reach the gate"; }
ok "child reached approval gate (run_active + pending_gate) — no dispatch/cost"
alive "$CHILD_PID" || fail "child not alive while dashboard running"
ok "child alive while dashboard runs"

echo "=== Kill dashboard, assert child SURVIVES (detach proof) ==="
kill -9 "$DASH_PID" 2>/dev/null; DASH_PID=""
sleep 3
alive "$CHILD_PID" || fail "child died when dashboard killed — DETACH FAILED"
ok "dashboard killed, child PID $CHILD_PID STILL ALIVE — process_group(0) detach survives"

echo "=== Graceful stop (no approval, no cost) ==="
printf '{"decision":"stop"}' > "$WORKDIR/.porpoise/control/stop-next.json"
stopped=0
for _ in $(seq 1 20); do alive "$CHILD_PID" || { stopped=1; break; }; sleep 0.5; done
[ "$stopped" = 1 ] || { kill -9 "$CHILD_PID" 2>/dev/null; fail "child did not stop within 10s"; }
ok "child consumed stop-next and exited gracefully"

echo "=== M38: immediate relaunch after stop (dead-PID lock must not block) ==="
# run.lock now holds the dead child's PID; a new launch must NOT be 409.
( cd "$WORKDIR" && "$EXE" dashboard --no-open --port "$PORT" ) >/dev/null 2>&1 &
DASH_PID=$!
for _ in $(seq 1 30); do curl -s "http://127.0.0.1:$PORT/api/live" >/dev/null 2>&1 && break; sleep 0.3; done
code="$(http_code '{}')"
[ "$code" = "200" ] || fail "immediate relaunch should be 200 (dead-PID lock ignored), got $code"
CHILD2_PID="$(grep -o '"pid":[0-9]*' "$WORKDIR/.porpoise/launch.log" 2>/dev/null | tail -1 | grep -o '[0-9]*')"
# fetch pid from a fresh launch response instead if log parse fails
ok "immediate relaunch after run end -> 200 (M38 run-lock fix)"
printf '{"decision":"stop"}' > "$WORKDIR/.porpoise/control/stop-next.json"
sleep 2

cleanup
echo ""
echo "M38 LAUNCHER LIVE VERIFICATION (Unix): PASS"
echo "Detached fleet survives dashboard exit; dead-PID run lock allows immediate relaunch."
