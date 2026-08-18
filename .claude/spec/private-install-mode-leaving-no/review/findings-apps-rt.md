## Verdict — apps/rt, round 2: APPROVED (0 critical)

All ten criteria run green, plus the whole `mustard-rt` suite.

## Verified claims — each command run, real output

| Claim | Command | Result |
|---|---|---|
| AC-1..4 | `cargo test -p mustard-core --test private_install <each filter>` | `1 passed` × 4 (`3 filtered out` each — the filters really match) |
| AC-5 | `cargo test -p mustard-rt --test private_scan ac5_…` | `1 passed` |
| AC-6 | `cargo test -p mustard-rt --test private_surface ac6_…` | `1 passed` |
| AC-7 | `cargo test -p mustard-cli --test private_init ac7_…` | `1 passed` |
| AC-8 | `cargo test -p mustard-core --test private_install_leaves_no_trace ac8_…` | `1 passed` |
| AC-10 | `cargo test -p mustard-rt --test private_guards ac10_…` | `1 passed` |
| AC-9 | `cargo build --workspace` | `0 errors, 1 warning` (`feature.rs:488`, pre-existing, untouched) |
| regression | `cargo test -p mustard-rt` / `-p mustard-core` / `-p mustard-cli` | `2074 passed (37 suites)` / `647 passed, 4 ignored` / `50 passed` |
| lints | `cargo clippy --workspace --all-targets` | `0 errors, 172 warnings` (all pedantic, pre-existing) |

The tests are not tautological: each carries a negative control, `private_guards.rs` and `private_surface.rs` drive the **built binary** so a warm process cache cannot answer "autodetected", and `git_status` returns a failing sentinel when the measurement did not happen.

## Guards + molds (apps/rt) — clean

- **Four registrations**: no new `run` subcommand, only a flag, so the rule does not bite; `run_command_surface.rs` locks names, not flags, and `template_parity.rs`'s reverse ratchet passed. `MaintCmd::Upsert` keeps `display_order = 44`, dispatch arm wired at `apps/rt/src/commands/maint/cli.rs:236` — **rt-cmd-pattern** respected.
- **No panic / no `unwrap`**: the new hook-reachable path degrades through `let-else`, `if let Ok(cache)` and `Command::…output().ok()?` to `InstallMode::Shared`.
- **Byte-stable `run` output**: the four new `UpsertReport` fields all `skip_serializing_if`, and the `unavailable` reasons are path-free constants — a shared report is byte-identical (asserted at `packages/core/tests/private_install.rs:227`).

## Change requests — all three landed

- *backup dir*: seeded under the empty-status assertion; the shared control requires git to SEE it.
- *depth*: `cover("**/.claude/")` plus the `packages/api/.claude/` fixture. Empirically proven, not read off a constant.
- *one resolver + effectiveness*: re-derived the reader list independently — a repo-wide grep for `join("CLAUDE.md")` / `== "CLAUDE.md"` in `apps/rt/src` leaves **no production site**; every remaining literal is inside `#[cfg(test)]`.

## Non-blocking findings

- **MAJOR** — `apps/rt/src/commands/agent/agent_prompt_template.md:4` is the one reader that still spells the name: `Read the ## Guards section of {subproject}/CLAUDE.md — mandatory rules`. Under a private install that sends every dispatched agent to the *client's* file, or to one that does not exist. Not blocking: the `## GUARDS` block itself is resolved correctly and AC-10 proves the rules reach the prompt — but this is exactly the "N call sites each choosing a filename" shape the change request condemned, surviving in the site that happens to be prose. (Same line exists in `plugin/agents/mustard-review.md:8` and `plugin/refs/agent-prompt/agent-prompt.md:27`, outside this subproject.)
- **MINOR** — `apps/rt/src/commands/work_unit_open.rs:627`: operator-facing prose still says `/scan` "rewrites each subproject's CLAUDE.md ## Guards"; stale under a private install.
- **MINOR** — `apps/rt/src/commands/scan_claude.rs:579`: `fix_breadcrumb` writes `> Parent: [../../CLAUDE.md]` into the new `CLAUDE.local.md`, pointing at the client's root file (or none). Cosmetic.
