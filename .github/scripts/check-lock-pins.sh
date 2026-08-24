#!/usr/bin/env bash
#
# check-lock-pins.sh — the guard that REJECTS a `Cargo.lock` which did not take
# the version stamp. Usage:
#
#     check-lock-pins.sh <Cargo.lock> <version> <crate>...
#
# The bump writes one number into four files ("the legs of the stamp") and every
# leg carries a guard. Three of those guards used to ask
# `grep -q '^version = "$nv"$' <lock>`, which is satisfied by ANY package that
# happens to sit on that number — measured on v0.1.44, where the third-party
# `tracing` was itself at 0.1.44 and the guard passed on its own, on a lock that
# had not moved. The block comment above `cargo update --workspace` in
# `bump-on-main.yml` already warned against exactly that confusion; it was
# written for the command that WORKS and never applied to the line that CHECKS.
#
# This guard matches per PACKAGE, and it fails when either half is wrong:
#
#   * a named <crate> is ABSENT from the lock. A set derived from the lock alone
#     cannot see this: the crate that vanished simply stops being asked about,
#     so the guard goes green on the one case it exists to catch — a dependency
#     dropped by accident.
#   * any local package of the lock (a `[[package]]` block with no `source`
#     line) is pinned at something other than <version>. That set is DERIVED, so
#     every crate of ours in the lock is covered rather than the single name
#     somebody typed — the dashboard lock pins `mustard-core` AND `mustard-cli`,
#     and the old guard read only the first.
#
# The lock's own root package is excluded from the derived sweep: it carries a
# version of its own on purpose (`mustard-dashboard` is at 0.1.0 while this
# repository is at 0.1.4x). Its name is read from the `Cargo.toml` beside the
# lock; a virtual workspace manifest declares no `[package]`, so nothing is
# excluded for the root lock.
#
# Exit status: 0 when the lock carries the stamp, 1 when it does not, 2 on a
# usage error. Every diagnosis goes to stderr, so a caller may read the status
# alone (`... && echo ok || echo stale`) and still leave the reason in the log.
set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "usage: $0 <Cargo.lock> <version> <crate>..." >&2
  exit 2
fi

lock=$1
want=$2
shift 2

if [ ! -f "$lock" ]; then
  echo "$lock: no such file — the leg it stamps is gone, not green" >&2
  exit 1
fi

# The package the lock's own manifest declares, if it declares one at all.
own=''
manifest="$(dirname "$lock")/Cargo.toml"
manifest=${manifest#./}
if [ -f "$manifest" ]; then
  own=$(awk '
    /^\[package\]/   { p = 1; next }
    /^\[/            { p = 0 }
    p && /^name = "/ { split($0, a, "\""); print a[2]; exit }
  ' "$manifest")
fi

# Every `[[package]]` block with no `source` line, as `name version` pairs. Any
# line at column 0 opening a table ends the block, so the quoted entries inside
# a `dependencies = [...]` list — which are indented — are never read as fields.
locals=$(awk '
  function flush() { if (n != "" && v != "" && s == 0) print n " " v; n = ""; v = ""; s = 0 }
  /^\[/          { flush(); next }
  /^name = "/    { split($0, a, "\""); n = a[2] }
  /^version = "/ { split($0, a, "\""); v = a[2] }
  /^source = "/  { s = 1 }
  END            { flush() }
' "$lock")

if [ -z "$locals" ]; then
  echo "$lock: names no local package at all — either it stopped carrying this" \
       "repository's crates or the lock format moved and this guard no longer" \
       "reads it. Both are failures; neither is a pass." >&2
  exit 1
fi

# The pairs framed by newlines, so a `case` pattern can anchor a WHOLE name:
# `$nl<crate> ` matches only a name that starts a line and is followed by its
# version, never a prefix of a longer one.
#
# `case` and not `… | grep -Fqx "$crate"`: under `set -o pipefail` a `grep -q`
# that exits on its first match leaves the producer ahead of it writing to a
# closed pipe, and the 141 it dies with becomes the pipeline's status — so a
# crate that IS present reports as missing whenever the buffer fills first.
# Measured on a forged lock of 4001 local packages: the first name was reported
# gone on 5 runs of 5. The repository's own locks are far too small to fill a
# pipe buffer, which is exactly why a race like this waits.
nl='
'
pairs="$nl$locals$nl"

missing=''
for crate in "$@"; do
  case "$pairs" in
    *"$nl$crate "*) ;;
    *) missing="$missing $crate" ;;
  esac
done

stale=''
while read -r name version; do
  if [ -z "$name" ]; then
    continue
  fi
  if [ -n "$own" ] && [ "$name" = "$own" ]; then
    continue
  fi
  if [ "$version" != "$want" ]; then
    stale="$stale $name@$version"
  fi
done <<EOF
$locals
EOF

rc=0
if [ -n "$missing" ]; then
  echo "$lock: no longer pins$missing. A crate that left the graph is the case" \
       "this guard exists for — restore the dependency or drop the name from" \
       "the guard's argument list, deliberately." >&2
  rc=1
fi
if [ -n "$stale" ]; then
  echo "$lock: still pins$stale, but the stamp is $want. Advance it with" \
       "\`cargo update --workspace --manifest-path $manifest\` and commit the lock." >&2
  rc=1
fi
exit "$rc"
