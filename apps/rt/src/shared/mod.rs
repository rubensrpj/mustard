//! `shared` — cross-face infrastructure consumed by **both** the enforcement
//! face (`hooks`) and the script face (`commands`).
//!
//! Keeping these here (instead of under `commands/`) preserves a clean
//! dependency DAG: `hooks` and `commands` both depend on `shared`, and `shared`
//! never depends back on either. A hook reaching into a command module would
//! invert that layering — this module exists to make that impossible.
//!
//! - [`branch_state`] — the ONE sweep of work-unit branches (local AND remote)
//!   and the classifier that says what state each is in, behind a PR-lookup
//!   port. Both faces ask it: the exit ritual (`commands::git_settle`), the spec
//!   inventory and the statusline.
//! - [`context`] — run-context resolution (cwd / session-id / current-spec),
//!   the port of `hook-env.js`'s runtime probing.
//! - [`gate_mode`] — the three-state gate mode (`off`/`warn`/`strict`) and its
//!   cascade resolver, shared by the size gates and the close-gate engine.
//! - [`events`] — the NDJSON event bus: classification/routing ([`events::route`])
//!   and the append-only writer ([`events::writer_ndjson`]).
//! - [`prompt`] — tells a person's prompt apart from the runtime's own notices,
//!   which reach the session through the same `UserPromptSubmit` channel. One
//!   owner for the rule, shared by every observer on that trigger.
//! - [`pr_provider`] — the pull-request ACTIONS (open/edit/ready/view) as a
//!   port, the acting twin of `branch_state`'s read-only `PrLookup`: callers
//!   depend on the trait, adapters are the only place a provider and its
//!   CLI/API are named, and the factory picks by the provider in force.
//! - [`proc`] — signal-free, cross-platform process/port primitives (kill by
//!   port, liveness probe) shared by the collector-spawning hook and the
//!   collector-stopping `run` command, plus [`proc::run_shell_with_deadline`]
//!   — the ONE shell-command runner that drains both pipes concurrently and
//!   waits under a deadline, shared by `verify-pipeline` and `qa-run`.
//! - [`translate`] — fail-open client for the optional `mustard-translate`
//!   sidecar (local MT), shared by the `feature` auto-gloss and the
//!   `scan-equivalences` artifact generation.
//! - [`work_kind`] — WHAT a work unit is (`feature`/`fix`/`hotfix`), the
//!   `{kind}/{slug}` name built from it, and the project's base model derived
//!   from `git.flow`. The crate's ONE parser of a work-branch name, in both the
//!   current shape and the `{base}_{slug}` shape units in flight still carry —
//!   and the one reader/writer of the base a unit was actually CUT from, which
//!   only the unit's own record can remember once the pending marker is
//!   consumed.

pub mod branch_state;
pub mod context;
pub mod events;
pub mod gate_mode;
// The bin target sees this port as unreached until the pr/git doors move
// behind it (next waves) — the allow leaves with the first caller.
#[allow(dead_code)]
pub mod pr_provider;
pub mod proc;
pub mod prompt;
// Test-only: cloning git fixture scenery instead of rebuilding it per test.
#[cfg(test)]
pub mod test_fixture;
pub mod translate;
pub mod work_kind;
