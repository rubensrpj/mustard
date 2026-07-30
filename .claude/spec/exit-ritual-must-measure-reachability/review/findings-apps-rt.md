# Review — apps/rt, spec exit-ritual-must-measure-reachability (waves 1+2)

Branch `dev_exit-ritual-must-measure-reachability` @ `6ab4353a`. Verdict: **approved**, 0 critical.

Note: all commands run through `rtk proxy` — plain `cargo` is rewritten by the local rtk
filter, which swallows the `test result:` lines the ACs grep for. Environment artifact,
not a defect (and itself an instance of the very filter this spec reports upstream).

## Acceptance Criteria — 8/8 PASS

| AC | Command | Real output |
|---|---|---|
| 1 | `cargo test -p mustard-rt moved_after_merge` | `ok. 2 passed` |
| 2 | `… settle_refuses_when_a_ref_moved_after_merge` / control `contract_refuses_on_base` | `ok. 1 passed` each |
| 3 | `… gitlink_only_dirt` / control `in_place_unit_settles` | `ok. 1 passed` each |
| 4 | `… base_behind_downgrades_ok` / control `single_repo_unit_reports_itself_complete` | `ok. 1 passed` each |
| 5 | `… diff_context_reads_ranges_via_rev_list`; control grep | `ok. 1 passed`; grep exit 1 (gone) |
| 6 | `--test git_prose_rules git_prose_conditions_gitlink_on_reachability` | `ok. 1 passed` |
| 7 | `--test git_prose_rules git_prose_routes_destructive_decisions_through_rev_list` | `ok. 1 passed` |
| 8 | `cargo test --workspace`; `cargo build --workspace` | 64 × `ok`, zero FAILED; build Finished |

## Independent confirmations (not taken on the implementer's word)

- Old `is_merged` really did spawn `Command::new("gh") … --state merged --limit 1`
  (`git show b33d4264:…git_settle.rs`) — the hand-written copy is gone.
- `gh 2.96.0`: `gh pr list --head dev --state merged --json state,headRefOid` returns a
  distinct `headRefOid` per merged PR — the frozen-head premise holds.
- `rev-list --pretty=oneline --abbrev-commit --no-commit-header <range>` is byte-identical
  to `log --oneline` (908 = 908 bytes on `b33d4264~4..b33d4264`); `rtk_command` still the
  spawner, Golden Rule intact.
- Live smoke: `mustard-rt run git-settle --report` → exit 0, per-ref `refs[]`, no panic.
- `git submodule update -- sub` on a scratch monorepo really re-seats a detached submodule;
  `rev-parse --abbrev-ref HEAD` == `"HEAD"` when detached (detection at git_settle.rs:456).

## Guards / molds

No new `run` subcommand (four-registration rule N/A). No `.unwrap()/.expect()` outside
`#[cfg(test)]`. No hook/`main.rs` touched. `clippy --workspace --all-targets` zero errors.
Output stays sorted and timestamp-free. `tests/git_prose_rules.rs` copies the sibling
carve-out header verbatim.

## Findings (non-blocking)

1. **major** — `apps/rt/src/commands/git_settle.rs:1514`: AC-3 states `submodule update`
   "aligns ONLY detached submodules", but the test asserts only the *left-on-branch* half.
   An inert `sync_submodule_pointers` that never updated anything would still pass. The
   dangerous half (never yank live work) does bite; the benign half could ship untested.
   Reviewer reproduced the missing half by hand against real git — code correct, test
   discipline incomplete.
2. **minor** — `apps/rt/src/shared/branch_state.rs:827`: `MovedAfterMerge` is also the
   answer when the reachability read merely FAILED (fail-open empty set) — an unmeasured
   state printed as a measured "the branch moved". Tension with the module's own
   principle 3. Never authorises a prune; `refs[]` exposes the raw evidence.
3. **minor** — `apps/rt/src/commands/git_settle.rs:689`: reason `base-behind` also fires
   when the unit's base is *ahead* of origin, or checked out in another worktree
   (`fetch origin b:b` refuses → `updated:false`) — those cases are mislabelled.

## Change requests

- "segue" — no obligation.
- "nunca implementar em dev" — verified in fact: `git reflog show dev` carries only
  `pull`/`fetch`, and the single wave commit sits on a branch `Created from dev`. No AC
  covers it, correctly: it demands no apps/rt behaviour change.
