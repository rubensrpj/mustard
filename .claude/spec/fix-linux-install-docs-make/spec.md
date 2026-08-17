---
id: spec.fix-linux-install-docs-make
---

# the Linux install docs teach a manual multi-download route and omit the Claude Code plugin step, and install.sh cannot install without a .deb beside it — so no one-line curl install exists

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Installing Mustard on Ubuntu today costs three manual downloads, a checksum
step and a permission change, because the installer script only automates the
package manager — it never fetches the package itself. And whoever finishes the
tutorial is told no further step
is needed, which is false: the harness is a Claude Code plugin and needs a
marketplace registered. A field install this week ended at
`/plugin install mustard` → *"not found in any marketplace"*, with the tutorial
having declared the job done.

Why now: v0.1.35 is the first release whose assets a stranger can install
unassisted. The install text is the product's first surface, and both halves of
it are wrong at once — the route is heavier than it needs to be, and it stops
one step short of working.

## Users/Stakeholders

Anyone installing Mustard on Ubuntu from a published Release — testers first,
since they follow the text literally and have no repo checkout to fall back on.

## Success Metric

A person with a fresh Ubuntu 22.04 and Claude Code installed reaches a working
`/mustard:*` inside Claude Code by following the tutorial top to bottom, with no
step improvised and no diagnosis session in between. The install command itself
is one line.

## Non-Goals

- Signing the packages, or publishing an apt repository.
- A public plugin marketplace (the `add` still points at the repo).
- Changing what the `.deb` contains or where it installs.
- Any macOS or Windows install text.

## Acceptance Criteria

- **AC-1** — when the installer is piped into `sh` with no package beside it —
  the exact shape of a `curl … | sh` install — then it resolves the package from
  the release and reports what it would install, instead of aborting with
  *"rode o install.sh de dentro da pasta do pacote"*
  Command: `bash -c "tr -d '\r' < packaging/installer/install.sh | sh -s -- --dry-run"`
  Control: `bash -c "tr -d '\r' < packaging/installer/install.sh | sh -n"`
- **AC-2** — when the four install texts are checked, then each one carries the
  one-line curl command, and the Linux tutorial and both READMEs carry the
  concrete marketplace command
  Command: `bash -c "grep -q 'releases/latest/download/install' packaging/installer/TUTORIAL-LINUX.md && grep -q 'releases/latest/download/install' packaging/installer/RELEASE-BODY.md && grep -q 'releases/latest/download/install' README.md && grep -q 'releases/latest/download/install' README.en.md && grep -q 'plugin marketplace add rubensrpj/mustard' packaging/installer/TUTORIAL-LINUX.md && grep -q 'plugin marketplace add rubensrpj/mustard' README.md && grep -q 'plugin marketplace add rubensrpj/mustard' README.en.md"`
  Control: `bash -c "tr -d '\r' < packaging/installer/install.sh | sh -n"`
- **AC-3** — the project build passes green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `packaging/installer/install.sh` — download fallback, stdin-safe package resolution, `--dry-run`
- `packaging/installer/TUTORIAL-LINUX.md` — one-liner first, manual route kept, new plugin step
- `packaging/installer/RELEASE-BODY.md` — the Linux line becomes the one-liner
- `README.md` — Linux row + concrete marketplace commands
- `README.en.md` — same, in English
- `packaging/installer/README.txt` — the fifth install text, folded in mid-pipeline
  by a change request (it taught only the manual route). It does NOT ship in the
  tar.gz bundle: `build-deb.sh` copies it into `dist/`, where every release so far
  dropped it — no published asset ever carried it. This pass adds it to the release
  assets in `release.yml` so the text reaches the reader it is written for
- `.github/workflows/release.yml` — publishes README.txt as a release asset
  (collection step + publish `files:` list)

## Boundaries

IN: the Linux install path only — `install.sh`, the Linux tutorial, the release
body's Linux line, and the Linux row plus plugin step of both READMEs.

A `--dry-run` flag is IN, and it is not decoration: it is the only way a
criterion can prove the installer resolves a package without invoking `apt` as
root on the machine running the check. It resolves the package (local file or
release URL), prints what it would install, and touches nothing on the system.

Its exit status is the verdict, and it is NOT always 0. It exits 0 only when it
knows what it would install: a `.deb` sitting beside the script (knowable
offline — the package is right there), or a release tag it resolved or was
handed in `MUSTARD_VERSION`. When it has neither, it exits NON-ZERO. Exiting 0
while naming a URL it never resolved is what let a criterion whose whole verdict
is the exit status go green on a runner with no egress, without the feature
being exercised once.

OUT: the `.deb` layout and `build-deb.sh`; the Windows and macOS tutorials;
`mustard init` and its closing message; signing, apt repositories and a public
marketplace.

Of `release.yml`, only the README.txt asset is IN — the fifth install text was
rewritten here and no release published it, so without that one line the rewrite
reaches nobody. Every other asset the workflow publishes stays untouched.

## Definitions

- **one-liner install** — a single `curl -fsSL <install.sh URL> | sh` that installs Mustard on Ubuntu with no file downloaded by hand and no checksum step
- **the plugin step** — registering the marketplace and installing the `mustard` plugin INSIDE Claude Code — separate from the .deb, which ships only binaries and templates and never touches ~/.claude

## Decisions

- install.sh downloads the .deb from the GitHub Release when no .deb sits beside it, keeping the local .deb as the preferred source when present
  Reason: the script aborts at install.sh:24 when no package is beside it, which is exactly what makes `curl | sh` impossible today; keeping the local path preserves the offline/manual install already documented
- the download resolves the release tag by following the /releases/latest redirect, not by hardcoding a version and not by calling the GitHub API
  Reason: asset names carry the version (mustard_<ver>_amd64.deb) so a stable latest/download/<name> URL cannot exist for the .deb; the unauthenticated API is rate-limited, while the redirect was verified to resolve to /releases/tag/v0.1.35
- install.sh must run correctly when read from stdin, not only from a file on disk
  Reason: `curl | sh` gives the script no $0 path, so the SCRIPT_DIR resolution at install.sh:21 cannot be the only way the package is located
- every Linux install text leads with the one-liner and keeps the manual .deb + install.sh route as a documented alternative
  Reason: the user asked for curl, and the manual route is the only one that allows verifying sha256 before installing
- the marketplace/plugin commands become concrete (rubensrpj/mustard) in every install text, and the Linux tutorial gains the plugin step it lacks
  Reason: the placeholder at README.md:60 and the outright claim at TUTORIAL-LINUX.md:115 that no extra step is needed are what left a field install with `/plugin install mustard` answering 'not found in any marketplace'

## Evidence

- install.sh locates the .deb only beside itself and exits 1 when none is found, so a piped curl install dies before reaching apt
  Evidence: `packaging/installer/install.sh:24`
- SCRIPT_DIR is derived from $0, which is not a usable path when the script is piped into sh
  Evidence: `packaging/installer/install.sh:21`
- TUTORIAL-LINUX.md tells the reader that after `mustard init` no extra step is necessary — the plugin/marketplace step is absent from the whole document
  Evidence: `packaging/installer/TUTORIAL-LINUX.md:115`
- TUTORIAL-LINUX.md step 2 instructs the reader to place install.sh and the .deb in the same folder, describing only the manual route
  Evidence: `packaging/installer/TUTORIAL-LINUX.md:44`
- README.md documents the plugin step but with a non-runnable placeholder '<repositório do marketplace>' instead of the real repo
  Evidence: `README.md:60`
- README.md's Linux row repeats the same-folder instruction, so fixing only the tutorial leaves the README teaching the old route
  Evidence: `README.md:44`
- RELEASE-BODY.md — the text GitHub shows on the release page, the first thing a user reads — carries the same-folder instruction
  Evidence: `packaging/installer/RELEASE-BODY.md:14`
- the release publishes mustard_*_amd64.deb, install.sh and TUTORIAL-LINUX.md as loose assets, so install.sh already has a stable latest/download URL while the .deb does not
  Evidence: `.github/workflows/release.yml:297`
- the .deb installs real binaries under /usr/lib/mustard/bin and the postinst symlinks them into /usr/bin; nothing is written to ~/.claude, confirming the plugin is a separate half of the install
  Evidence: `packaging/linux/build-deb.sh:134`
- REFUTED: that the .deb or `mustard init` registers the marketplace — init deliberately plants neither enabledPlugins nor extraKnownMarketplaces, and a test locks that behaviour
  Evidence: `apps/cli/src/commands/init.rs:999`