# PR #160 — second review round, findings to fix (verdict: rejected)

Scope decided by the operator: the Linux install path PLUS the `mustard init`
closing message and the installer's own closing block. Windows/macOS tutorials are
OUT (already in the unit's notebook, they become their own unit).

Three of these were reproduced by hand; the reproduction is quoted.

## CRITICAL

### K1 — AC-1 goes green without proving anything when there is no network
`packaging/installer/install.sh` (`--dry-run` block, ~line 263) and the spec's `## Boundaries`

Reproduced:
```
$ env -i PATH=/usr/bin sh install.sh --dry-run
==> --dry-run: nada será instalado.
    Pacote:  mustard_<versao>_amd64.deb
    Origem:  .../releases/latest  (a versão não pôde ser resolvida agora — sem rede?)
rc=0
```
AC-1's command IS that invocation, with no `Expect:` regex, so exit 0 is the whole
verdict. On a CI runner with no egress the criterion passes while the feature is
not exercised at all.

Fix: `--dry-run` must exit NON-ZERO when it could not determine what it would
install — no resolved version AND no local `.deb`. It keeps exiting 0 when it
resolved the tag, and also when a local `.deb` is present (offline is fine there:
the package is known). Then update the spec's `## Boundaries`, which currently
promises "Offline it still exits 0" — that sentence is what created this hole and
must be replaced by the rule above. Do NOT touch AC-1's command text.

### K2 — `mustard init`'s closing message prints the exact broken command
`apps/cli/src/commands/init.rs:780` (`print_next_steps`)

`install.sh:310` runs `mustard init --yes`, so for a one-liner user this block is
the LAST thing on screen — and they downloaded no files, so no document is ever
opened. It currently prints:
```
/plugin marketplace add <mustard repo or local directory>  →  /plugin install mustard
```
A placeholder, and an install command missing `@mustard-local`. Typed verbatim it
produces `Plugin "mustard" not found in any marketplace` — the field failure this
whole unit exists to remove.

Fix: print the concrete two commands, one per line, as the five texts now do:
`/plugin marketplace add rubensrpj/mustard` then
`/plugin install mustard@mustard-local`. Keep the message in English (this file's
user-facing strings are English) and keep it short. Adjust the existing test in the
same file if it asserts on this text.

### K3 — the installer's own closing block never names the plugin step
`packaging/installer/install.sh` (~line 314, the "Pronto" block)

Same reasoning as K2 from the other side: the terminal is the only surface a
one-liner user sees. The block lists CLI, Dashboard, "Preparar um projeto" and
"Desinstalar", and says nothing about the marketplace.

Fix: add the plugin step to that block, in pt-BR like the rest of the script,
naming both commands and saying they are typed INSIDE Claude Code, not in the
terminal.

## FALSE STATEMENT

### F1 — "os hooks já ficam ligados via `.claude/settings.json`" is false
`packaging/installer/TUTORIAL-LINUX.md:142-143` and `packaging/installer/README.txt:83-84`

Measured: `grep -c hooks packages/core/templates/settings.json` → `0`. The seed has
no `hooks` key; the hooks live in `plugin/hooks/hooks.json` and arrive with the
plugin. Worse, README.txt:83 contradicts README.txt:73-79, which this same PR added
to say the hooks come from the plugin.

Fix both sentences so they say what is true: `mustard init` writes `.claude/` and
`mustard.json`; the hooks arrive with the plugin, installed in the step that
follows. Only these two files — other occurrences elsewhere in the repo are out of
scope for this pass.

## REGRESSIONS INTRODUCED BY THE PREVIOUS FIX PASS

### G1 — the "pinned" one-liner does not pin the installed version
`packaging/installer/RELEASE-BODY.md` ~line 14

V2 changed the URL to `.../releases/download/v{{VERSION}}/install.sh`, which pins
only WHICH install.sh runs. That script still resolves `/releases/latest`, so a
user on the v0.1.35 page after v0.2.0 shipped installs 0.2.0 while believing they
pinned. Correct form: `... | MUSTARD_VERSION={{VERSION}} sh`. Keep AC-2 green (the
file must still contain the `releases/latest/download/install` string somewhere
honest — the always-latest URL offered as the explicit opt-in).

### G2 — `HEAD`-only tag resolution has no `GET` fallback
`packaging/installer/install.sh` ~line 147

R5 replaced the GET with `curl -fsSLI`. A proxy answering 405/403 to HEAD — common
in corporate networks — now kills the entire one-liner path, with an error blaming
the network even though a plain GET would have worked. Keep HEAD as the first
attempt and fall back to `curl -fsSL -o /dev/null -w '%{url_effective}'` when it
fails.

## PRE-EXISTING

### P1 — `valid_version()` bracket ranges are locale-collated
`packaging/installer/install.sh` ~line 49

`*[!A-Za-z0-9._-]*` and `[0-9]*` are range expressions; POSIX defines range
membership by the current locale's collating sequence, not ASCII. Under some
UTF-8 locales, characters the comment promises are rejected can pass — and
`$VERSION` flows into a filesystem path and a URL. Set `LC_ALL=C` (exported) at the
top of the script, which also stabilises every other `case` and `sort` in it.

### P2 — the wget branch discards wget's exit status
`packaging/installer/install.sh` ~line 152

`_url=$(wget ... 2>&1 | sed ... | head -1)` takes its status from `head`, always 0.
DNS failure, TLS rejection, 404 and "redirect present but unparsed" all collapse to
an empty `_url` and the same generic message. Capture wget's own status (write to a
temp file, or `set -o pipefail` is NOT available in POSIX sh — use an intermediate
variable/file) and distinguish "could not reach GitHub" from "reached it but found
no tag". The branch is still untestable here (no wget on this machine) — say so
rather than claiming it was exercised.

## VERIFICATION REQUIRED

Report the REAL output of each. On this machine `sh` is absent from PATH; `bash` is
the WSL one and `/bin/sh` there is dash. The checkout has CRLF, so normalise with
`tr -d '\r'`. Run these from git-bash (NOT PowerShell — it eats `$`).

1. `bash -c "tr -d '\r' < packaging/installer/install.sh | sh -n"` → rc 0
2. AC-1 WITH network: `bash -c "tr -d '\r' < packaging/installer/install.sh | sh -s -- --dry-run"` → rc 0, prints a `releases/download/v…` URL
3. K1 fixed: `env -i PATH=/usr/bin:/bin HOME=/tmp sh i.sh --dry-run` (on a normalised copy) → rc NON-ZERO
4. K1 does not over-reject: same offline env but WITH a `mustard_9.9.9_amd64.deb`
   beside the script → rc 0, `Origem: arquivo local`
5. AC-2 (the seven-way grep, unchanged) → rc 0
6. `MUSTARD_VERSION=../../../etc/passwd` still refused, rc non-zero, no `../` in output
7. `MUSTARD_VERSION=0.1.35` and `=v0.1.35` still accepted, rc 0
8. `cargo build --workspace` → rc 0 (init.rs is Rust; this one is required)
9. The init.rs test suite for that file: `cargo test -p mustard-cli init` (or the
   suite covering `print_next_steps`) → report the real pass count