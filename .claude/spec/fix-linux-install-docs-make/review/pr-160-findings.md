# PR #160 — review findings to fix (verdict: rejected, 3 critical)

Two were reproduced by hand before this list was written; the reproduction is quoted.

## CRITICAL

### C1 — `MUSTARD_VERSION` is interpolated into a path and a URL with no validation
`packaging/installer/install.sh` (~line 222, and the `VERSION` assignment near line 39)

The validation is inverted: the tag coming from the NETWORK gets two gates (`v[0-9]*`
shape + `*[!A-Za-z0-9._-]*` charset), while the environment variable — actual user
input — gets only `${VERSION#v}`.

Reproduced:
```
$ MUSTARD_VERSION=../../../etc/passwd sh install.sh --dry-run
    Pacote:  mustard_../../../etc/passwd_amd64.deb
    Origem:  .../releases/download/v../../../etc/passwd/mustard_../../../etc/passwd_amd64.deb
```
In a real run `download_file -o "$DEB"` writes OUTSIDE `$TMP_DIR`, and the cleanup
trap only does `rm -rf "$TMP_DIR"`, so the file is left behind.

Fix: apply the same two `case` gates to `$VERSION` right after it is read. Reject
with a clear message naming the expected shape.

### C2 — four of the five texts dropped `chmod +x` from the manual route
`packaging/installer/README.txt`, `packaging/installer/RELEASE-BODY.md`, `README.md`, `README.en.md`

GitHub release assets do not carry the executable bit (`build-deb.sh` chmods it in
`dist/`, which is lost on upload). A reader following the manual route gets
`bash: ./install.sh: Permission denied`.

Measured — only the tutorial still teaches it:
```
packaging/installer/TUTORIAL-LINUX.md         chmod=1
packaging/installer/README.txt                chmod=0
packaging/installer/RELEASE-BODY.md           chmod=0
README.md                                     chmod=0
README.en.md                                  chmod=0
```
Fix: the manual route in all four must include `chmod +x install.sh` before
`./install.sh` (or teach `sh install.sh`, which needs no bit — pick one and be
consistent across the five texts).

### C3 — the spec justifies README.txt with a claim that is false
`.claude/spec/fix-linux-install-docs-make/spec.md` `## Files`

It says README.txt "ships in the tar.gz bundle". No release publishes it:
`release.yml` uploads only `dist/mustard_*_amd64.deb`, `dist/install.sh`,
`dist/TUTORIAL-LINUX.md`, `dist/mustard-bins-*.tar.gz`. `build-deb.sh` copies
README.txt into `dist/` where it is dropped. Its only other consumer is
`mustard-windows-x64.zip`, which release.yml never uploads either.

Fix (the operator chose to make the file reach users): add README.txt to the
release assets in `.github/workflows/release.yml` — both the collection step and
the publish `files:` list — AND correct the sentence in the spec's `## Files` so it
states what is actually true.

## ROBUSTNESS

### R1 — `$TARGET` is validated only AFTER the full root install
`install.sh` ~line 243. `sh -s -- /caminho/errado` (the documented form) runs
`apt-get update` + `apt-get install -y` as root, installs everything, and only then
aborts with `erro: projeto-alvo não existe`. The user reads a non-zero exit and
concludes nothing was installed. Move the `[ -d ]` check into the argument loop, and
make `--dry-run` test it too (today it prints `Depois: mustard init --yes em $TARGET`
without checking).

### R2 — the local `.deb` picker takes the OLDEST match
`install.sh` ~line 146: `ls "$SCRIPT_DIR"/mustard_*_amd64.deb | head -1`. `ls` sorts
ascending, so a `~/Downloads` holding 0.1.35 and 0.2.0 silently installs 0.1.35.
Use `sort -V | tail -1`.

### R3 — the package download has no wall-clock or stall timeout
`install.sh` ~line 127: the curl branch has only `--connect-timeout 15 --retry 2`,
which bounds the handshake alone. A proxy that accepts then trickles nothing wedges
the one-liner forever, three times over. `resolve_latest_tag` already sets
`--max-time 60`; the wget branch has `--timeout=30`. Add `--max-time` (or
`--speed-limit 1024 --speed-time 30`) to the download.

### R4 — the root/sudo precondition is checked after the network round trip
`install.sh`: tag resolution runs ~line 165, the `id -u` / sudo check ~line 204. A
non-root user without sudo burns up to 60s before being told it cannot proceed.
Hoist the sudo block above resolution — it has no dependency on `$DEB`.

### R5 — tag resolution GETs the whole release page to read a redirect
`install.sh` ~line 95: `curl -fsSL -o /dev/null -w '%{url_effective}'` follows the
302 and downloads the entire `/releases/tag/vX` HTML only to discard it. `-I`
(`--head`) yields the identical value for a few hundred bytes — a real win on the
slow connections this path serves.

### R6 — the downloaded `.deb` is handed to root apt with only a non-empty check
`install.sh` ~line 224. A captive portal answering 200 with an HTML interstitial
passes `curl -f` and passes `[ ! -s ]`, then fails as an opaque dpkg parse error.
Add a `dpkg-deb --info "$DEB" >/dev/null 2>&1` sanity check (skip it gracefully when
`dpkg-deb` is absent) so the failure names its real cause.

### R7 — duplicated literal and two back-to-back `case` blocks
`install.sh` ~lines 143-144 spell the same path twice in a `[ -f X ] && DEB=X || true`
idiom; ~lines 115-121 run two sequential `case "$_tag"` blocks that should be one, so
a future edit to the accepted tag shape does not have to be made in two places.

## VERSION-IN-TEXT

### V1 — a concrete `0.1.35` is baked in as recovery advice
`install.sh` ~line 176 (tag-resolution failure path) and ~line 21 (usage header), plus
`packaging/installer/TUTORIAL-LINUX.md` ~line 66. At v0.4.0 this advice silently
installs a year-old build. TUTORIAL-LINUX.md is copied VERBATIM into the release by
`build-deb.sh` with no `{{VERSION}}` substitution (only RELEASE-BODY.md gets the
`sed`), and the doc uses `<versao>` placeholders everywhere else. Use `<versao>`.

### V2 — the release body pins every asset name but its install command says `latest`
`packaging/installer/RELEASE-BODY.md` ~line 16. A user opening the v0.1.36 page
deliberately, to stay on a known-good build, reads a table naming
`mustard_0.1.36_amd64.deb` and pastes an adjacent command that installs v0.5.0. The
body is already `sed`-substituted at release time, so
`.../download/v{{VERSION}}/install.sh` makes the page self-consistent at no cost.

## NOT IN THIS PASS

The one-liner currently resolves to the PREVIOUS release's `install.sh` (v0.1.35 is
`latest` and its asset is the old 73-line script). The operator's decision is to merge
and cut v0.1.36 right after, so the window is accepted deliberately. Do NOT reword the
one-liner and do NOT add a minimum-version caveat.