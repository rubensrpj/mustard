# PR #160 — third review round: the EIGHT to fix, then merge

The operator cut the list deliberately: fix the eight that break a real user, merge,
and let the rest live in the unit's notebook. Do NOT fix anything outside this file
— the exclusions are listed at the bottom and were decided, not overlooked.

Three of these were reproduced by hand; the reproduction is quoted.

## BREAKS THE PR'S OWN PURPOSE

### T1 — `apt-get` is not shielded from the script's own stdin
`packaging/installer/install.sh:359`

Under `curl … | sh` the script's stdin IS the pipe carrying its own not-yet-parsed
source. `$SUDO apt-get install -y "$DEB"` runs with that stdin. A dpkg conffile
prompt or any debconf question `-y` does not suppress reads from it and eats the
remaining bytes of install.sh — so the `mustard init` block and the whole plugin
block (the point of this PR) silently vanish, or half a line executes.

Fix: `DEBIAN_FRONTEND=noninteractive` on the apt calls AND `< /dev/null` on the
install invocation. Apply the same to `apt-get update` — it takes the same pipe.

### T2 — `set -eu` lets a failing `mustard init` swallow the plugin block
`packaging/installer/install.sh:28` (`set -eu`) and `:365`

`( cd "$TARGET" && mustard init --yes )` aborts the whole script when init bails —
templates dir not locatable, an unwritable target, or `probe_rtk` exiting 1. The
apt install already succeeded, but the user sees a non-zero exit and NOTHING about
the plugin: the exact failure this unit exists to remove.

Fix: guard it so a failed init degrades to a warning naming what to run by hand,
and let the plugin block print regardless.

### T3 — the documented pin command cannot be pasted
`packaging/installer/TUTORIAL-LINUX.md:67` and the usage header of
`packaging/installer/install.sh:22`

Reproduced:
```
$ MUSTARD_VERSION=<versao> sh
sh: versao: No such file or directory
```
`<` is an input redirection, so the shell sets an EMPTY variable, redirects stdin
from a file named `versao`, and never mentions MUSTARD_VERSION. RELEASE-BODY.md
gets this right because `{{VERSION}}` is substituted at release time.

Fix: use a concrete number in both places (e.g. `MUSTARD_VERSION=0.1.35 sh`), with
a sentence saying to swap it for the version on the Releases page.

## BREAKS A REAL USER

### T4 — no architecture guard: arm64 gets an amd64 package after the sudo prompt
`packaging/installer/install.sh` (~line 279, the URL construction)

`_amd64.deb` is hardcoded. On arm64 Ubuntu (Raspberry Pi, Graviton, UTM on Apple
Silicon) the one-liner resolves the tag, downloads a VALID amd64 .deb (so
`dpkg-deb --info` passes), asks for the sudo password, runs `apt-get update`, and
only then dies with `package architecture (amd64) does not match system (arm64)`.
Before this PR the user typed the `_amd64` filename themselves and saw it.

Fix: check `dpkg --print-architecture` BEFORE downloading and refuse early with a
message naming the detected architecture and the fact that only amd64 is published.
Do NOT try to build an arm64 URL — no such asset exists. When `dpkg` is absent, say
so rather than assuming.

### T5 — `sudo sh -s -- /projeto` creates a root-owned `.claude/`
`packaging/installer/install.sh:365`

The script needs root for apt and the tutorial lists `sudo` as a prerequisite, so
`curl … | sudo sh -s -- ~/meu-projeto` is the natural shape. `mustard init --yes`
then runs as root and `.claude/` + `mustard.json` come out owned by `root:root`;
every later non-root write fails with EACCES.

Fix: when `SUDO_USER` is set, drop back to that user for the init step
(`sudo -u "$SUDO_USER" …`), or refuse the project path with a message telling the
user to run `mustard init` themselves. Either is acceptable; state which you chose.

### T6 — `[ -f "$0" ]` is not "the script is a real file"
`packaging/installer/install.sh:238`

Under `curl | sh`, `$0` is the literal string `sh`, resolved against the CURRENT
directory. Run the one-liner from a directory that happens to hold a file named
`sh` and the test passes: SCRIPT_DIR becomes the cwd and the local-.deb picker
installs whatever stale or foreign-architecture package is lying there instead of
the release the user asked for.

Fix: test that `$0` actually names a path (`case "$0" in */*)`) in addition to
being a readable file.

## DEAD CODE / WRONG COMMENT

### T7 — the `HDR_FILE` cleanup branch can never run
`packaging/installer/install.sh:82` (trap) and `:181` (assignment)

`TAG=$(resolve_latest_tag)` forks a command-substitution subshell, so
`HDR_FILE=$(mktemp …)` sets the SUBSHELL's copy and the parent's stays empty
forever. The trap's `[ -n "$HDR_FILE" ]` guard is always false, so Ctrl-C during
the wget probe leaks the temp file — precisely what the comment promises cannot
happen.

Fix: either have the caller own the path and pass it in, or drop the parent-level
plumbing and clean up inside the function. Do not leave a comment claiming a
guarantee the code does not provide.

### T8 — the `init.rs` doc comment states something false, and the flow now repeats itself
`apps/cli/src/commands/init.rs:777`

The comment claims this block is "the LAST thing on screen" for a one-liner user.
It is not: install.sh runs init and then prints ~20 more lines, including the same
two plugin commands — in Portuguese, right after init printed them in English. So
`curl … | sh -s -- /projeto` ends with a duplicated bilingual instruction and the
comment's premise is wrong.

Fix: correct the comment to say what is true (the CLI surface must stand on its own
because init is also run directly, outside the installer), and remove the
duplication — the installer's own block should not reprint what init just printed
when it ran init itself. Keep both surfaces correct when used ALONE.

## EXPLICITLY EXCLUDED — decided, not overlooked

Do not touch these; they are in the unit's notebook as follow-up work:
- The harness artifact inconsistencies (`review/verdict.md` vs `review/findings.md`,
  `.summary.json` waves marked in_progress).
- Lifting the wget branch's error discrimination into the curl branch.
- `release.yml`'s `if-no-files-found` semantics and the RELEASE-BODY assets footer.
- AC-2 not covering README.txt (structurally blocked — `ac-amend` refuses a
  criterion that is already green).
- The docs advertising `latest/download/install.sh` before a release serves the new
  script — the operator merges and cuts v0.1.36 right after, deliberately.
- AC-1 now requiring the network. That is the intended trade: a criterion that
  passes offline was proving nothing, which is what round 2 fixed.

## VERIFICATION REQUIRED

Report the REAL output of each, labelled. Run from git-bash, NOT PowerShell (it
eats `$`). `sh` is absent from PATH here; `bash` is the WSL one and its `/bin/sh` is
dash. The checkout has CRLF — normalise with `tr -d '\r'` onto a copy first.

1. `bash -c "tr -d '\r' < packaging/installer/install.sh | sh -n"` → rc 0
2. AC-1 with network: `… | sh -s -- --dry-run` → rc 0, prints a `releases/download/v…` URL
3. AC-1 offline, nothing resolvable → rc NON-ZERO (round 2's contract must survive)
4. AC-1 offline WITH a local `mustard_9.9.9_amd64.deb` → rc 0, `Origem: arquivo local`
5. AC-2 (the seven-way grep, unchanged) → rc 0
6. `MUSTARD_VERSION=../../../etc/passwd` → rc non-zero, no `../` in output
7. T3: the command as printed in TUTORIAL-LINUX.md:67 and the install.sh header now
   pastes and runs — quote both lines and show the paste working
8. T4: force the arch check to see a non-amd64 value (stub `dpkg` on PATH printing
   `arm64`) → refuses BEFORE any download and BEFORE any sudo prompt; show that no
   temp file and no apt call happened
9. T6: create a file named `sh` in the cwd, run the piped form → must NOT treat the
   cwd as SCRIPT_DIR
10. `cargo build --workspace` → rc 0
11. `cargo test -p mustard-cli init` → report the real pass count