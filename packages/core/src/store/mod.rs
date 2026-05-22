//! Side-effecting infrastructure — the `io` layer.
//!
//! Where [`model`](crate::model) is pure data, `io` is everything that
//! touches the filesystem. Each capability is exposed **behind a trait** so
//! consumers and tests inject a fake instead of the concrete implementation
//! (Dependency Inversion):
//!
//! - [`event_store`] — the [`EventSink`](event_store::EventSink) trait, the
//!   API every consumer and the dispatcher program against.
//! - [`sqlite_store`] — [`SqliteEventStore`](sqlite_store::SqliteEventStore),
//!   the SQLite-backed (WAL-mode) implementation of the
//!   [`EventSink`](event_store::EventSink) trait over
//!   `.claude/.harness/mustard.db`, plus read APIs over the harness
//!   projections (specs, metrics, spans, FTS5 knowledge search). It is the
//!   single store the harness reads from and writes to.
//! - [`pipeline_repo`] — the [`PipelineRepo`](pipeline_repo::PipelineRepo)
//!   trait and [`FsPipelineRepo`](pipeline_repo::FsPipelineRepo), read/write
//!   of `.claude/.pipeline-states/{specName}.json`.
//! - [`fs`] — the fail-open primitives (atomic write, append, read) that the
//!   filesystem-backed stores above are built on.
//!
//! Every operation in this layer is fail-open: it returns
//! [`Result`](crate::error::Result) and never panics, so the hooks that
//! consume it can degrade safely on any I/O failure.

pub mod db_cache;
pub mod event_store;
pub mod fs;
pub mod migrations;
pub mod pipeline_repo;
pub mod sqlite_store;
pub mod wikilinks;
