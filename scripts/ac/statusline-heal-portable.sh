#!/usr/bin/env sh
#
# statusline-heal-portable.sh — the statusline self-heal writes the PORTABLE
# command, and rewrites an absolute path already on disk back to that form.
#
# The plugin prepends its own `bin/` to PATH before Claude Code runs anything,
# so the bare token `mustard-rt` already resolves to the copy the harness is
# meant to run — on every machine, on every OS. A path does not: it pins the
# machine to one executable forever.
#
# That is not theory. On the field machine of 2026-08-28 a forgotten build
# inside a source clone (C:/atiz/mustard/plugin/bin/mustard-rt.exe, version
# 0.1.47) ran once, recorded its own absolute path here, and from then on it
# WAS the binary the status bar started — so it re-recorded that same path,
# session after session, in a directory no installer can reach. Three
# reinstalls of the .exe changed nothing. This criterion is the lock on that
# door: two cases, one for each direction the defect can come from.
#
# POSIX sh on purpose — the spec runs this criterion with `sh`, so no bashisms.
#
# Usage: sh scripts/ac/statusline-heal-portable.sh
set -eu

# `cargo` is NOT on PATH in the harness shell that runs this criterion — three
# criteria in an earlier unit exited 127 for exactly that reason. A rustup
# install puts it here; an already-reachable cargo is unaffected.
export PATH="$HOME/.cargo/bin:$PATH"

AQUI=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1
RAIZ=$(CDPATH= cd -- "$AQUI/../.." && pwd) || exit 1

TMP=$(mktemp -d) || exit 1
trap 'rm -rf "$TMP"' EXIT
trap 'rm -rf "$TMP"; exit 130' INT TERM HUP

PORTAVEL="mustard-rt run statusline"

cargo build --quiet -p mustard-rt --manifest-path "$RAIZ/Cargo.toml"
BINARIO="$RAIZ/target/debug/mustard-rt"
[ -x "$BINARIO" ] || { echo "FAIL: $BINARIO was not built"; exit 1; }

mkdir -p "$TMP/home"

# A throwaway project. The workspace-root anchor is the PAIR `mustard.json` +
# `.claude/`, so both have to exist or the observer resolves no root and the
# criterion would pass by doing nothing.
novo_projeto() {
  proj="$TMP/$1"
  mkdir -p "$proj/.claude"
  printf '{}\n' > "$proj/mustard.json"
  printf '%s' "$proj"
}

# One SessionStart, one module. `check <id>` takes the trigger from the input,
# so the observer sees the event it waits for without every other SessionStart
# module running alongside it.
#
# HOME points into the sandbox on purpose. Before any face runs, `main()` hands
# the whole invocation to a NEWER mustard-rt recorded in ~/.claude/plugins — so
# on a machine whose plugin is ahead of this build, the criterion would measure
# a binary nobody just changed. An empty HOME has no such registry, and the
# build under test is the one that answers.
conserta() {
  printf '{"session_id":"ac-statusline","hook_event_name":"SessionStart","cwd":"%s"}' "$1" \
    | ( cd "$1" && HOME="$TMP/home" "$BINARIO" check statusline_heal_observer ) > /dev/null
}

exigir_portavel() {
  rotulo="$1"; arquivo="$2"
  if ! grep -qF -- "\"command\": \"$PORTAVEL\"" "$arquivo"; then
    echo "FAIL: $rotulo — the recorded command is not the portable one:"
    cat "$arquivo"
    exit 1
  fi
  echo "OK: $rotulo"
}

# --- case 1: nothing recorded yet -------------------------------------------
# A clean install has no statusLine at all. What gets written here is the shape
# every machine inherits, so it is the one that must never carry a path.
VAZIO=$(novo_projeto vazio)
conserta "$VAZIO"
ALVO="$VAZIO/.claude/settings.local.json"
[ -f "$ALVO" ] || { echo "FAIL: the heal wrote no settings.local.json at all"; exit 1; }
exigir_portavel "with nothing recorded, the heal writes the portable command" "$ALVO"

# --- case 2: an absolute path already recorded ------------------------------
# The exact shape the field machine was pinned by, with an unrelated key beside
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
conserta "$SUJO"
exigir_portavel "an absolute path already on disk is healed back to it" "$ALVO"

if grep -qF -- "C:/atiz/mustard" "$ALVO"; then
  echo "FAIL: the stale clone path survived the heal:"
  cat "$ALVO"
  exit 1
fi
if ! grep -qF -- "mustard-memory" "$ALVO"; then
  echo "FAIL: the heal took an unrelated key with it:"
  cat "$ALVO"
  exit 1
fi

echo "PASS: the statusline heal writes, and restores, the portable command"
