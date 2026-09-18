#!/usr/bin/env bash
#
# deps-no-unused.sh — nothing is declared as a dependency that no file imports.
#
# Updating and pruning are different jobs. Updating raises the version of what
# we use; pruning removes what we do not. The previous wave did the first and
# none of the second, so this criterion measures only the second.
#
# The tool installs itself. None of `cargo-machete`, `cargo-udeps` or
# `cargo-shear` was on the machine when this was written — a criterion that
# assumes a manual install fails with "command not found" (exit 127) instead of
# with a finding, which is the opposite of what a criterion is for.
# `cargo-shear` is the one chosen: it parses `src/` with `syn` (not regex, like
# cargo-machete) and needs no nightly toolchain (unlike cargo-udeps).
#
# FALSE POSITIVES ARE NEVER SILENCED BY MUTING A TOOL. A dependency reached only
# through generated code or a macro is recorded one by one, with its reason, in
# the tool's own config:
#   - `[package.metadata.cargo-shear]` in the crate's Cargo.toml (apps/scan's
#     seven `grammar_*` aliases live only in languages.toml -> build.rs ->
#     $OUT_DIR/langs_generated.rs)
# So a dependency that goes dead tomorrow is still reported.
#
# Missing dependencies — imported but never declared — are printed as a WARN and
# do not fail this criterion: that is a different defect, and this script is
# named for the one it measures.
#
# Usage: scripts/ac/deps-no-unused.sh
set -euo pipefail

# `cargo` is NOT on PATH in the harness shell that runs this criterion — three
# criteria in the previous unit exited 127 for exactly this reason. A rustup
# install puts it here; an already-reachable cargo is unaffected.
export PATH="$HOME/.cargo/bin:$PATH"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failed=0

# ---------------------------------------------------------------- Rust half --

# Pinned to the 1.x line so a future major's new heuristics cannot flip this
# criterion without anyone changing a dependency.
if ! command -v cargo-shear > /dev/null 2>&1; then
  echo "installing cargo-shear (not present on this machine)..."
  cargo install cargo-shear --locked --version '^1.13' \
    || { echo "FAIL: could not install cargo-shear"; exit 1; }
fi
echo "cargo-shear $(cargo shear --version 2>&1 | tr -d '\n')"

# `apps/translate` is EXCLUDED from the root workspace (candle + lingua are too
# heavy for the hook binary), so `cargo shear` on the root never reaches it. It
# is its own workspace and gets its own pass — otherwise the sidecar would be the
# one place in the repo where a dead dependency is free.
for target in ".:the root workspace" "apps/translate:apps/translate"; do
  dir="$repo_root/${target%%:*}"
  label="${target#*:}"
  [ -f "$dir/Cargo.toml" ] || continue
  if cargo shear "$dir" > "$work/shear.txt" 2>&1; then
    echo "OK: no unused crate in $label"
  else
    echo "FAIL: unused Rust dependencies in $label —"
    cat "$work/shear.txt"
    failed=1
  fi
done

# ------------------------------------------------------------------ verdict --

[ "$failed" -eq 0 ] || { echo "FAIL: at least one declared dependency is imported by nothing"; exit 1; }
echo "PASS: every declared crate is imported by something"
