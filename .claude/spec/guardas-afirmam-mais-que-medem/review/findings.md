# Re-review wave-0 @ 4d6f6ac6 — VERDICT: approved (0 critical)

Supersedes the 04:25 `rejected` verdict, which predates the fix commit.
Repo left as found: `git status` empty, HEAD `4d6f6ac6`, 0 stashes. All mutation
work ran in `git archive` throwaway trees.

## The three findings of the previous review — all closed, each proven

| # | severity | location | how it was proven closed |
|---|---|---|---|
| 1 | critical | `apps/rt/tests/plugin_agents.rs:190` | 376 generated lines through current / `fe4edd95` / `main` readers against a PyYAML oracle: **0 inputs where current accepts and `fe4edd95` rejects**, 0 valid models rejected. Reverting only `scalar_value` to the old body makes `scalar_value_still_rejects_leftovers_after_the_id` FAIL on `model: "sonnet" garbage`. |
| 2 | major | `.github/scripts/check-lock-pins.sh` | Forged 4001-local-package lock: current `rc=0` 5/5; `fe4edd95`'s script on the same fixture `rc=1` 5/5 naming `local-1`. `case` glob is quoted, so `mustard-c*e` / `mustard-cor?` / `mustard-cor[ez]` match nothing. Prefix, CRLF, `version`-less block, `dependencies=[…]` noise, own-package exclusion, missing lock, `<3` args — all correct. |
| 3 | minor | `packages/core/tests/version_line.rs:220` | Deleting the `mustard-cli` block from the dashboard lock makes the test FAIL naming which crate vanished; the same mutated lock with the `fe4edd95` test passes. `DASHBOARD_LOCK_MUST_PIN = ["mustard-cli","mustard-core"]` equals the workflow argv at `bump-on-main.yml:108/143/170`. |

## Acceptance Criteria — six green, verbatim commands

AC-1..AC-4 `1 passed` each · AC-5 `2 passed` · AC-6 `Finished dev profile` rc=0.
Controls: `version_line` 8 passed, `plugin_agents` 5 passed.
`cargo test --workspace`: 3072 passed, 0 failed, 6 ignored, 78 suites.
`cargo clippy --workspace --all-targets`: exit 0 (220 of 222 warnings pre-existing).

## Workflow

Both legs call the script for both locks (92, 108 / 156, 170; decision at 142-143).
The dev-leg decision now reads all four legs:
`[ "$dv" = "$nv" ] && [ "$cv" = "$nv" ] && [ "$root_pin" = ok ] && [ "$dash_pin" = ok ]`.
Mutation A (drop `root_pin`) -> AC-4 FAILS naming it; mutation B (dashboard call
reduced to one crate) -> AC-2 FAILS. The two surviving `grep -q '^version = "$nv"$'`
at 76 and 152 target `$cargo_toml`, not a lock — out of the spec's Evidence.

## Hard constraint

Closed spec `cargo-lock-src-tauri-fica` AC-2, run verbatim: GREEN, rc=0.
`dash_pin` survives at line 143. No `ac-amend` needed.

## Scope

`git diff --stat 9a5ab4ed..HEAD` = exactly the four files of `## Arquivos`. Every
`cargo update` and `sed` byte-identical (OUT respected). `agent_frontmatter`'s BOM
panic untouched (OUT).

## Non-blocking observations — none flip the verdict

- `packages/core/tests/version_line.rs:176` — `DASHBOARD_LOCK_MUST_PIN` and the
  workflow argv are two hand-maintained copies of one list with no test binding
  them. `guard_invocations()` is already in the same file and could close it in
  one assertion.
- `.github/scripts/check-lock-pins.sh:69` — the awk sweep would read
  `[[patch.unused]]` blocks as local packages; the Rust `toml` reader would not.
  Fails closed, absent from both locks.
- `packages/core/tests/version_line.rs:324,326` — two new
  `clippy::format_push_string` pedantic warnings from `forge_lock`. CI clippy runs
  without `-D warnings` and without `--all-targets`.
- `model: "sonnet"\t# c` is accepted while PyYAML rejects it. YAML 1.2 permits tab
  as separation space and both predecessors behave the same — no regression.
