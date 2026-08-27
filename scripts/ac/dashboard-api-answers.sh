#!/usr/bin/env bash
#
# dashboard-api-answers.sh — `POST /api/{command}` carries what `invoke` used
# to carry.
#
# The body is the same argument object the frontend handed `invoke(command,
# args)`, in the same camelCase; the response is the same JSON the command
# already returned. The three failure shapes are checked too, because they are
# the contract's other half: a rejected command is 400 with its own message
# (what `invoke` rejected with), an unregistered name is 404 (what
# `generate_handler!` could never tell you), and neither takes the server down.
#
# Usage: scripts/ac/dashboard-api-answers.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
server_pid=""

cleanup() {
  [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

cargo build --quiet -p mustard-dashboard --manifest-path "$repo_root/Cargo.toml"
binary="$repo_root/target/debug/mustard-dashboard"
[ -x "$binary" ] || { echo "FAIL: $binary was not built"; exit 1; }

project="$work/project"
mkdir -p "$project/.claude/spec/exemplo"
printf '# exemplo\n' > "$project/.claude/spec/exemplo/spec.md"

"$binary" --no-open --port 47772 --root "$project" > "$work/out.log" 2>&1 &
server_pid=$!

url=""
for _ in $(seq 1 100); do
  url="$(grep -o 'http://127\.0\.0\.1:[0-9]*/' "$work/out.log" | head -1 || true)"
  [ -n "$url" ] && break
  sleep 0.1
done
[ -n "$url" ] || { echo "FAIL: the server never printed its URL"; cat "$work/out.log"; exit 1; }

# `status_of <command> <body>` prints "<http status>\n<response body>".
status_of() {
  curl -sS --max-time 10 -o "$work/body.json" -w '%{http_code}' \
    -X POST -H 'Content-Type: application/json' \
    --data "$2" "${url}api/$1"
}

for _ in $(seq 1 100); do
  curl -fsS --max-time 2 "${url}api/commands" > /dev/null 2>&1 && break
  sleep 0.1
done

# 1. A registered command answers with its own JSON.
code="$(status_of dashboard_specs "{\"repoPath\":\"$project\"}")"
[ "$code" = "200" ] || { echo "FAIL: dashboard_specs answered $code"; cat "$work/body.json"; exit 1; }
grep -q '"exemplo"' "$work/body.json" \
  || { echo "FAIL: the spec on disk is missing from the answer"; cat "$work/body.json"; exit 1; }
echo "dashboard_specs -> 200 with the spec list"

# 2. A missing argument is the `invoke` rejection: 400, naming the argument.
code="$(status_of dashboard_specs '{}')"
[ "$code" = "400" ] || { echo "FAIL: a missing argument answered $code"; cat "$work/body.json"; exit 1; }
grep -q 'repoPath' "$work/body.json" \
  || { echo "FAIL: the 400 must name the argument"; cat "$work/body.json"; exit 1; }
echo "missing argument -> 400 naming repoPath"

# 3. A name that is not in the dispatch table is 404, not a silent success.
code="$(status_of nao_existe '{}')"
[ "$code" = "404" ] || { echo "FAIL: an unknown command answered $code"; cat "$work/body.json"; exit 1; }
echo "unknown command -> 404"

# 4. A malformed body is 400 — and the server is still serving afterwards.
code="$(status_of dashboard_specs '{nao e json')"
[ "$code" = "400" ] || { echo "FAIL: a malformed body answered $code"; exit 1; }
code="$(status_of dashboard_specs "{\"repoPath\":\"$project\"}")"
[ "$code" = "200" ] || { echo "FAIL: the server stopped serving after a bad request"; exit 1; }
echo "malformed body -> 400, server survives"

kill "$server_pid"
wait "$server_pid" 2>/dev/null || true
server_pid=""

echo "PASS: POST /api/{command} answers what invoke answered"
