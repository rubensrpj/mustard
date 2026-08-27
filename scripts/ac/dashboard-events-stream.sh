#!/usr/bin/env bash
#
# dashboard-events-stream.sh — `GET /api/events` carries what `AppHandle::emit`
# used to carry.
#
# The watcher is unchanged: it still debounces a write burst and still throttles
# per kind. Only the destination moved — from the desktop shell's event channel
# to one long-lived HTTP response the browser reconnects to on its own. This
# script opens that response, writes an NDJSON event shard into a watched
# project, and waits for the `dashboard:fs-change` frame to come back.
#
# Usage: scripts/ac/dashboard-events-stream.sh
set -euo pipefail

# `cargo` is NOT on PATH in the harness shell that runs this criterion — the
# spec's own AC-4 and AC-10 carry this prefix for exactly that reason, and
# omitting it here left three criteria exiting 127 (found in review). A
# rustup install puts it here; an already-reachable cargo is unaffected.
export PATH="$HOME/.cargo/bin:$PATH"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
server_pid=""
stream_pid=""

cleanup() {
  [ -n "$stream_pid" ] && kill "$stream_pid" 2>/dev/null || true
  [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

cargo build --quiet -p mustard-dashboard --manifest-path "$repo_root/Cargo.toml"
binary="$repo_root/target/debug/mustard-dashboard"
[ -x "$binary" ] || { echo "FAIL: $binary was not built"; exit 1; }

project="$work/project"
events_dir="$project/.claude/spec/exemplo/.events"
mkdir -p "$events_dir"

"$binary" --no-open --port 47773 --root "$project" > "$work/out.log" 2>&1 &
server_pid=$!

url=""
for _ in $(seq 1 100); do
  url="$(grep -o 'http://127\.0\.0\.1:[0-9]*/' "$work/out.log" | head -1 || true)"
  [ -n "$url" ] && break
  sleep 0.1
done
[ -n "$url" ] || { echo "FAIL: the server never printed its URL"; cat "$work/out.log"; exit 1; }

for _ in $(seq 1 100); do
  curl -fsS --max-time 2 "${url}api/commands" > /dev/null 2>&1 && break
  sleep 0.1
done

# Hold the stream open in the background, unbuffered, writing frames to a file.
curl -sS -N --max-time 30 -H 'Accept: text/event-stream' "${url}api/events" \
  > "$work/stream.sse" 2>/dev/null &
stream_pid=$!

# The opening comment proves the headers were flushed before any change landed
# — an EventSource needs that to fire `onopen`.
for _ in $(seq 1 100); do
  grep -q 'mustard dashboard events' "$work/stream.sse" 2>/dev/null && break
  sleep 0.1
done
grep -q 'mustard dashboard events' "$work/stream.sse" \
  || { echo "FAIL: the stream never opened"; cat "$work/stream.sse"; exit 1; }
echo "stream open"

# Attach the watcher to the project, exactly as the frontend does on mount.
curl -fsS --max-time 10 -X POST -H 'Content-Type: application/json' \
  --data "{\"repoPaths\":[\"$project\"]}" "${url}api/dashboard_watch_repos" > /dev/null \
  || { echo "FAIL: dashboard_watch_repos was rejected"; exit 1; }

# One NDJSON shard write — the canonical data-change signal.
printf '%s\n' \
  '{"event":"pipeline.phase","kind":"pipeline","ts":"2026-08-27T10:00:00.000Z","spec":"exemplo","payload":{"phase":"EXECUTE"}}' \
  > "$events_dir/ac.ndjson"

# The debouncer coalesces over ~200 ms; allow generously for a loaded runner.
for _ in $(seq 1 150); do
  grep -q 'dashboard:fs-change' "$work/stream.sse" 2>/dev/null && break
  sleep 0.1
done
grep -q 'event: dashboard:fs-change' "$work/stream.sse" \
  || { echo "FAIL: the shard write never reached the stream"; cat "$work/stream.sse"; exit 1; }
grep -q '"kind":"events"' "$work/stream.sse" \
  || { echo "FAIL: the frame lost its payload"; cat "$work/stream.sse"; exit 1; }
echo "shard write -> dashboard:fs-change frame"

kill "$stream_pid" 2>/dev/null || true
stream_pid=""
kill "$server_pid"
wait "$server_pid" 2>/dev/null || true
server_pid=""

echo "PASS: GET /api/events streams the watcher's notifications"
