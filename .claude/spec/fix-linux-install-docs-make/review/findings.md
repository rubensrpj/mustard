# Review — fix-linux-install-docs-make (subproject: .)

Verdict: approved — 0 critical, 3 findings (1 major, 2 minor)

## Claims verified by the reviewer (command + real output)

| Claim | Result |
|---|---|
| AC-1 `tr -d '\r' < install.sh \| sh -s -- --dry-run` | PASS rc=0 — `Pacote: mustard_0.1.35_amd64.deb` / `Origem: .../releases/download/v0.1.35/...`. Control `sh -n` rc=0 |
| AC-2 the 7-way grep | PASS rc=0 |
| AC-3 `cargo build --workspace` | PASS rc=0, 5 crates, 1 pre-existing warning (`apps/rt/src/commands/feature.rs:488`, untouched) |
| Offline dry-run exits 0 | PASS — `env -i PATH=/usr/bin sh -s -- --dry-run` rc=0, prints "a versão não pôde ser resolvida agora — sem rede?" |
| Local .deb still preferred | PASS — fake `mustard_0.1.35_amd64.deb` in cwd → `Origem: arquivo local` |
| Documented forms actually run | PASS — `MUSTARD_VERSION=0.1.35` → pinned URL; `sh -s -- /caminho` → `mustard init --yes`; `--bogus` rc=1; two positionals rc=1 |
| Released install.sh == repo file | PASS — `build-deb.sh:176-180` copies verbatim + `sed 's/\r$//'` + chmod; `.gitattributes` pins `*.sh text eol=lf`; `git ls-files --eol` → `i/lf w/lf` |
| The one-liner URL exists | PASS — `latest/download/install.sh` → 302 to signed asset; bogus asset name → 404 (probe discriminates) |
| `mustard@mustard-local` is real | PASS — marketplace.json name `mustard-local`, source `./plugin`, plugin/ committed (34 files) |
| Tutorial no longer claims "no extra step" | PASS — `TUTORIAL-LINUX.md:142` now says "Falta **um** passo, o do item 6" |

## Findings

### MAJOR — the mid-pipeline change request landed in code under no Acceptance Criterion
Location: `.claude/spec/fix-linux-install-docs-make/spec.md:57`

`packaging/installer/README.txt` was correctly rewritten (curl one-liner at 27/29; plugin commands at
73-74; header at 7-8 no longer implies the .deb sits beside install.sh — each verified by grep). But
AC-2 still greps only the FOUR original texts; `ac-proof.json` has empty `amendments`/`additions`;
`wave-2-docs/spec.md` `## Files` and `meta.json` checklist never gained README.txt. A regression on
README.txt passes QA silently today. Not critical because the artifact itself is correct and was
confirmed by hand — but an AC amendment is owed before close.

### MINOR — unverified third-party claim in user docs
Location: `packaging/installer/TUTORIAL-LINUX.md:198`, `README.md:64`, `README.en.md:64`

The new claim that the `owner/repo` shorthand of `/plugin marketplace add` clones over SSH is backed
by nothing in the spec's `## Evidence`. The reviewer could not confirm it (Claude Code bundle
unreadable in its sandbox; docs fetch returned empty). Self-mitigating — the HTTPS fallback the text
offers works either way.

### MINOR — `resolve_latest_tag` accepts any slug
Location: `packaging/installer/install.sh:110`

Rejects only empty / `latest` / non-`[A-Za-z0-9._-]`. If `/releases/latest` ever lands on `/releases`
(every release draft), `_tag` becomes `releases` and the script builds
`.../releases/download/releases/mustard_releases_amd64.deb`, failing later as a download error rather
than a clear "no release found". A `v[0-9]*` shape check closes it.

### INFO — `ac-proof.json` holds only the red side
AC-1/AC-2 record `"proof":"red"` with `"confirmation":"not-taken"`; no green is stored. The greens
above were taken independently by the reviewer.

## No correctness defect found in install.sh
`set -eu` safe on every path exercised; the `!cmd || [ ! -s ]` precedence at line 214 is right; the
EXIT/INT double-cleanup is idempotent; `0755`/`0644` on the temp dir is the correct answer to apt's
`_apt` sandbox; no bashism survives `sh -n`.

Files reviewed: packaging/installer/install.sh, packaging/installer/TUTORIAL-LINUX.md,
packaging/installer/README.txt, packaging/installer/RELEASE-BODY.md, README.md, README.en.md,
packaging/linux/build-deb.sh, .github/workflows/release.yml.
