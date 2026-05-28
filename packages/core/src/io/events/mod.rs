//! NDJSON event primitives shared by all no-sqlite sub-specs (W2-W7).
//!
//! - [`types::Event`] — the single row unit from a per-spec event log.
//! - [`reader::EventReader`] — streaming, cached, zero-copy-filter access.

pub mod reader;
pub mod types;

pub use reader::EventReader;
pub use types::Event;
