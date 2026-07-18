//! `shared` — cross-face infrastructure consumed by **both** the enforcement
//! face (`hooks`) and the script face (`commands`).
//!
//! Keeping these here (instead of under `commands/`) preserves a clean
//! dependency DAG: `hooks` and `commands` both depend on `shared`, and `shared`
//! never depends back on either. A hook reaching into a command module would
//! invert that layering — this module exists to make that impossible.
//!
//! - [`context`] — run-context resolution (cwd / session-id / current-spec),
//!   the port of `hook-env.js`'s runtime probing.
//! - [`gate_mode`] — the three-state gate mode (`off`/`warn`/`strict`) and its
//!   cascade resolver, shared by the size gates and the close-gate engine.
//! - [`events`] — the NDJSON event bus: classification/routing ([`events::route`])
//!   and the append-only writer ([`events::writer_ndjson`]).
//! - [`proc`] — signal-free, cross-platform process/port primitives (kill by
//!   port, liveness probe) shared by the collector-spawning hook and the
//!   collector-stopping `run` command.
//! - [`translate`] — fail-open client for the optional `mustard-translate`
//!   sidecar (local MT), shared by the `feature` auto-gloss and the
//!   `scan-equivalences` artifact generation.

pub mod context;
pub mod events;
pub mod gate_mode;
pub mod proc;
pub mod translate;
