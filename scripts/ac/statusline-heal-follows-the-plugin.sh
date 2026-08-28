#!/usr/bin/env sh
#
# statusline-heal-follows-the-plugin.sh — the statusline self-heal records the
# PLUGIN's copy of mustard-rt, learned from Claude Code's plugin registry, and
# never the path of whatever binary happens to be running.
#
# Two wrong answers frame the right one, and this repo has now shipped both.
#
# The first was `std::env::current_exe()`. On the field machine of 2026-08-28 a
# forgotten build inside a source clone (C:/atiz/mustard/plugin/bin/
# mustard-rt.exe, 0.1.47) ran once, recorded its own path here, and from then on
# it WAS the binary the bar started — so it re-recorded that path forever, in a
# directory no installer can reach. Three reinstalls of the .exe changed nothing.
#
# The second was the bare token `mustard-rt`, on the belief that the plugin
# prepends its own bin/ to PATH. Measured on 2026-08-28: Claude Code APPENDS it,
# last of 21 entries, so the bare token resolves to the SYSTEM copy (/usr/bin).
# The bar would then report the system version, `stamped == current`, and the
# plugin-vs-system drift marker — the `m0.1.56↑0.1.47` that started the whole
# two-hour diagnosis — would never draw again.
#
# So the answer is a path, just never a path we INFER. The registry at
# ~/.claude/plugins/installed_plugins.json is Claude Code's own record of where
# the plugin lives; it follows the plugin across updates and can never name a
# source clone. When it cannot be read, the heal writes NOTHING — guessing is
# how the machine got pinned in the first place.
#
# POSIX sh on purpose — the spec runs this criterion with `sh`, so no bashisms.
#
# Usage: sh scripts/ac/statusline-heal-follows-the-plugin.sh
set -eu

# `cargo` is NOT on PATH in the harness shell that runs this criterion.
export PATH="$HOME/.cargo/bin:$PATH"

AQUI=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1
RAIZ=$(CDPATH= cd -- "$AQUI/../.." && pwd) || exit 1

TMP=$(mktemp -d) || exit 1
trap 'rm -rf "$TMP"' EXIT
trap 'rm -rf "$TMP"; exit 130' INT TERM HUP

cargo build --quiet -p mustard-rt --manifest-path "$RAIZ/Cargo.toml"
BINARIO="$RAIZ/target/debug/mustard-rt"
[ -x "$BINARIO" ] || { echo "FAIL: $BINARIO was not built"; exit 1; }

# The plugin install the registry will point at, and the command that must come
# out of every case below.
PLUGIN="$TMP/cache/mustard-local/mustard/0.0.1"
mkdir -p "$PLUGIN/bin" "$TMP/home"
printf '#!/bin/sh\nexit 0\n' > "$PLUGIN/bin/mustard-rt"
chmod +x "$PLUGIN/bin/mustard-rt"
ESPERADO="$PLUGIN/bin/mustard-rt run statusline"

# Version 0.0.1 on purpose. Before any face runs, `main()` hands the whole
# invocation to a NEWER install the registry records — a fabricated newer
# version would make this criterion measure a stub instead of the build.
CONFIG="$TMP/config"
mkdir -p "$CONFIG/plugins"
cat > "$CONFIG/plugins/installed_plugins.json" <<JSON
{
  "version": 2,
  "plugins": {
    "mustard@mustard-local": [
      { "scope": "user", "installPath": "$PLUGIN", "version": "0.0.1" }
    ]
  }
}
JSON

# The workspace-root anchor is the PAIR `mustard.json` + `.claude/`; both have
# to exist or the observer resolves no root and every case would pass by doing
# nothing at all.
novo_projeto() {
  proj="$TMP/$1"
  mkdir -p "$proj/.claude"
  printf '{}\n' > "$proj/mustard.json"
  printf '%s' "$proj"
}

# One SessionStart, one module. `check <id>` takes the trigger from the input,
# so the observer sees the event it waits for without every other SessionStart
# module running alongside it. $2 is the config dir; empty means NO registry.
conserta() {
  printf '{"session_id":"ac-statusline","hook_event_name":"SessionStart","cwd":"%s"}' "$1" \
    | ( cd "$1" && CLAUDE_CONFIG_DIR="$2" HOME="$TMP/home" \
        "$BINARIO" check statusline_heal_observer ) > /dev/null
}

exigir_o_plugin() {
  rotulo="$1"; arquivo="$2"
  if ! grep -qF -- "\"command\": \"$ESPERADO\"" "$arquivo"; then
    echo "FAIL: $rotulo — the recorded command is not the plugin's copy."
    echo "      expected: $ESPERADO"
    cat "$arquivo"
    exit 1
  fi
  # The running binary must never be the answer: that inference is the whole
  # defect, and it is what pinned the field machine to a 0.1.47 clone.
  if grep -qF -- "$BINARIO" "$arquivo"; then
    echo "FAIL: $rotulo — the heal recorded the RUNNING binary, not the plugin's:"
    cat "$arquivo"
    exit 1
  fi
  echo "OK: $rotulo"
}

# --- case 1: nothing recorded yet -------------------------------------------
VAZIO=$(novo_projeto vazio)
conserta "$VAZIO" "$CONFIG"
ALVO="$VAZIO/.claude/settings.local.json"
[ -f "$ALVO" ] || { echo "FAIL: the heal wrote no settings.local.json at all"; exit 1; }
exigir_o_plugin "with nothing recorded, the heal records the plugin's copy" "$ALVO"

# --- case 2: pinned to a forgotten clone ------------------------------------
# The exact shape the field machine was stuck on, with an unrelated key beside
# it: healing must reach the first and leave the second untouched.
SUJO=$(novo_projeto contaminado)
ALVO="$SUJO/.claude/settings.local.json"
cat > "$ALVO" <<'JSON'
{
  "enabledMcpjsonServers": [
    "mustard-memory"
  ],
  "statusLine": {
    "command": "C:/atiz/mustard/plugin/bin/mustard-rt.exe run statusline",
    "padding": 1,
    "type": "command"
  }
}
JSON
conserta "$SUJO" "$CONFIG"
exigir_o_plugin "a forgotten clone's path is replaced by the plugin's" "$ALVO"
if grep -qF -- "C:/atiz/mustard" "$ALVO"; then
  echo "FAIL: the stale clone path survived the heal:"; cat "$ALVO"; exit 1
fi
if ! grep -qF -- "mustard-memory" "$ALVO"; then
  echo "FAIL: the heal took an unrelated key with it:"; cat "$ALVO"; exit 1
fi

# --- case 3: no registry to read --------------------------------------------
# Nothing can name the plugin's copy here. Writing anything would be a guess,
# and a guess is what pinned the field machine — so the heal must abstain.
CEGO=$(novo_projeto sem-registro)
conserta "$CEGO" ""
if [ -e "$CEGO/.claude/settings.local.json" ]; then
  echo "FAIL: with no plugin registry the heal still wrote a statusline:"
  cat "$CEGO/.claude/settings.local.json"
  exit 1
fi
echo "OK: with no registry to read, the heal writes nothing rather than guessing"

echo "PASS: the statusline heal follows the plugin, and never the running binary"
