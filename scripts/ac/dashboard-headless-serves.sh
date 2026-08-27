#!/usr/bin/env bash
#
# dashboard-headless-serves.sh — the dashboard server starts on a host with no
# graphical session, says where it is listening, and never reaches for a
# browser.
#
# This is the case the Tauri shell died in: with DISPLAY and WAYLAND_DISPLAY
# both empty it panicked from inside gtk/tao, printing paths into the library
# instead of anything the operator could act on. The replacement prints the URL
# and keeps serving.
#
# The "never reaches for a browser" half is checked by putting a stub `xdg-open`
# first on PATH: if the server launches it, the stub leaves a file behind.
#
# Usage: scripts/ac/dashboard-headless-serves.sh
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

# A stub launcher that records the fact it ran, ahead of the real one on PATH.
mkdir -p "$work/bin" "$work/project/.claude"
cat > "$work/bin/xdg-open" <<STUB
#!/usr/bin/env bash
echo "\$@" > "$work/browser-opened"
STUB
chmod +x "$work/bin/xdg-open"

# No graphical session, and a high port so a running dashboard cannot collide.
env -u DISPLAY -u WAYLAND_DISPLAY \
  PATH="$work/bin:$PATH" \
  "$binary" --port 47771 --root "$work/project" \
  > "$work/out.log" 2>&1 &
server_pid=$!

url=""
for _ in $(seq 1 100); do
  url="$(grep -o 'http://127\.0\.0\.1:[0-9]*/' "$work/out.log" | head -1 || true)"
  [ -n "$url" ] && break
  sleep 0.1
done
[ -n "$url" ] || { echo "FAIL: the server never printed its URL"; cat "$work/out.log"; exit 1; }
echo "listening at $url"

grep -q 'no graphical session' "$work/out.log" \
  || { echo "FAIL: a headless start must say so"; cat "$work/out.log"; exit 1; }

[ -e "$work/browser-opened" ] \
  && { echo "FAIL: a browser was launched with no graphical session"; exit 1; }

# Still serving, not merely alive.
for _ in $(seq 1 100); do
  if curl -fsS --max-time 2 "${url}api/commands" > "$work/commands.json" 2>/dev/null; then
    break
  fi
  sleep 0.1
done
grep -q 'dashboard_specs' "$work/commands.json" \
  || { echo "FAIL: /api/commands did not list the dispatch table"; cat "$work/commands.json"; exit 1; }

kill "$server_pid"
wait "$server_pid" 2>/dev/null || true
server_pid=""

echo "PASS: headless start prints the URL, opens no browser, and serves"
