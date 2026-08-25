# Review wave-0 — VERDICT: rejected (1 critical)

Repo left exactly as found: `git status` empty, HEAD still `fe4edd95`, no stashes.

## Acceptance Criteria — all six green (measured by the reviewer)

| AC | command | real output |
|---|---|---|
| AC-1 | `cargo test -p mustard-core --test version_line bump_guard_rejects_a_lock_whose_local_crates_did_not_move` | `test result: ok. 1 passed` |
| AC-2 | `… bump_guard_checks_every_local_crate_of_each_lock` | `test result: ok. 1 passed` |
| AC-3 | `… bump_guard_rejects_a_lock_that_lost_one_of_our_crates` | `test result: ok. 1 passed` |
| AC-4 | `… dev_leg_decision_consults_what_the_work_block_repairs` | `test result: ok. 1 passed` |
| AC-5 | `cargo test -p mustard-rt --test plugin_agents scalar_` | `test result: ok. 2 passed` |
| AC-6 | `cargo build --workspace` | `Finished dev profile` (2 pre-existing `dead_code` warnings in `apps/cli/src/commands/git_flow.rs`, untouched) |
| controls | both `--test` controls + `cargo test --workspace` | `3072 passed`, zero failures |

Hard constraint (closed spec `cargo-lock-src-tauri-fica` AC-2): GREEN. `dash_pin` survived; no `ac-amend` needed.

## The tests are not decoration — mutation-tested

`guard_invocation` / `assignment` / `mentions` and the AC-4 body were extracted verbatim into a standalone binary and fed four workflows:

- `main`'s old workflow -> both FAIL (`nothing in the dev leg reads Cargo.lock into a variable`; `never runs the guard`)
- current -> both PASS
- decision with `&& [ "$root_pin" = ok ]` deleted -> FAIL; with `dash_pin` deleted -> FAIL; dashboard guard reduced to one crate -> FAIL

`check-lock-pins.sh` driven directly: real root/dashboard locks -> `rc=0`; stamp `0.1.46` -> `rc=1` naming all five; CRLF lock, block missing `version`, absent manifest, `dependencies = [...]` noise -> all `rc=1` for the right reason.

## CRITICAL — `apps/rt/tests/plugin_agents.rs:183` (`scalar_value`) approves invalid YAML the old ratchet rejected

`scalar_value` returns `inner` from `rest.split_once(quote)` and discards the whole remainder of the line unchecked. Everything after the closing quote vanishes, `#` or not. Both implementations compiled from the repo (old from `git show main:`) and run on the same inputs:

```
NEW model: "sonnet" garbage       -> value="sonnet"        accepted=true
NEW model: 'opus' junk here       -> value="opus"          accepted=true
NEW model: "claude-opus-5" papel  -> value="claude-opus-5" accepted=true
OLD model: "sonnet" garbage       -> value="sonnet\" garbage" accepted=false
OLD model: 'opus' junk here       -> value="opus' junk here"  accepted=false
```

This wave made the ratchet **strictly weaker**: two inputs it used to reject now pass. All three are invalid YAML — Claude Code cannot load such a frontmatter at all, which is a louder failure than the misspelling the ratchet exists to catch.

It contradicts three things at once:

1. the function's own doc comment (`a space, a quote or a '#' in there is not a model`);
2. AC-5's second clause (`continua reprovando valor com sobra depois do id` — `"claude-opus-5" papel` IS sobra depois do id and is accepted);
3. the spec's own `## Decisions`, which records `deixou a catraca aprovando YAML invalido` as a reason the previous attempt was reverted.

`scalar_value_still_rejects_leftovers_after_the_id` only feeds bare strings to `model_is_accepted`; it never sends a quoted-plus-leftover line through `declared`, which is why it stays green.

**Direction of fix:** after the closing quote the remainder must be empty or a `#`-comment, else the line declares no scalar.

## MAJOR (non-blocking) — `.github/scripts/check-lock-pins.sh:87` pipefail/SIGPIPE race in the ABSENT check

`printf … | cut -d' ' -f1 | grep -Fqx -- "$crate"` under `set -o pipefail`: `grep -q` exits on first match, `cut` takes SIGPIPE (141), pipefail hands 141 to `if !`, and a crate that IS present is reported missing.

Measured on a forged lock with 4001 local packages all on the stamp: `Cargo.lock: no longer pins local-1` (`local-1` is the first package), `rc=1` on 5/5 runs; the same pipeline without `pipefail` gives `rc=0`. Does not fire on the repo's 5-package locks (both verified `rc=0`) and it fails closed, so it is latent — but the guard's answer depends on a buffer race.

## MINOR — `packages/core/tests/version_line.rs:222`

`the_dashboard_lock_pins_this_repositorys_crates_at_this_version` still derives its set purely from the lock with `!ours.is_empty()` as the only floor, so a `mustard-cli` that vanished from that lock still passes. That is the exact lacuna the spec's `## Decisions` names; no AC covers it and the shell guard does close it, so it is a leftover, not a break.

## Notes

No `.claude/skills/*-pattern` molds exist in this tree, root `## Guards` is an empty seed, and `.github/scripts/` is a new directory with no siblings — no mold or guard violation. `toml = "1"` confirmed a regular dep at `packages/core/Cargo.toml:39`, so the replaced justification really was false.