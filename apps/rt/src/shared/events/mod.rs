//! `shared::events` — the NDJSON event bus shared across faces.
//!
//! - [`route`] — the single classification/routing entry point for every event.
//! - [`writer_ndjson`] — the append-only NDJSON writer (hot path), fail-open.

pub mod route;
pub mod writer_ndjson;
pub mod economy;
