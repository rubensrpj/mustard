//! The OTEL ports (Wave 6 of the b4 script port).
//!
//! Two `run` subcommands share a SQLite store and an OTLP/JSON projection:
//!
//! - [`collector`] — `mustard-rt run otel-collector`, the local OTLP/JSON
//!   receiver (port of `scripts/otel-collector.js`).
//! - [`diagnose`] — `mustard-rt run diagnose-otel`, the pipeline health check
//!   (port of `scripts/diagnose-otel.js`).
//!
//! Unlike the JS scripts — which reached `claude_code_otel` through the
//! `_lib/event-store.js` CJS wrapper — the Rust ports open
//! `.claude/.harness/mustard.db` directly via `rusqlite` (the `bundled`
//! feature compiles SQLite into the binary). The [`store`] module keeps the
//! schema byte-identical so a database is interchangeable between runtimes.

pub mod collector;
pub mod diagnose;
pub mod project;
pub mod store;
