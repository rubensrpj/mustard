//! The harness event-sink trait.
//!
//! A *sink* is any destination that accepts [`HarnessEvent`]s. The harness
//! programs against the [`EventSink`] **trait**, not a concrete store, so a
//! test (or a hook running with telemetry disabled) can inject a fake sink in
//! place of the real one.
//!
//! The production implementation is
//! [`SqliteEventStore`](super::sqlite_store::SqliteEventStore) — a single
//! WAL-mode `.claude/.harness/mustard.db` database. (Earlier waves shipped a
//! `JsonlEventStore` over an append-only NDJSON log; the SQLite store
//! superseded it and that log was removed.)
//!
//! Every implementation must fail open: an [`EventSink::append`] that fails
//! returns [`Err`] rather than panicking, and a caller is free to ignore it —
//! telemetry is never load-bearing.

use crate::error::Result;
use crate::model::event::HarnessEvent;

/// A destination that accepts harness events.
///
/// The trait is the API consumers and the dispatcher program against.
/// Implementations must fail open: an [`EventSink::append`] that fails
/// returns [`Err`] rather than panicking, and a caller is free to ignore it
/// (telemetry is never load-bearing).
pub trait EventSink {
    /// Append one event to the sink.
    ///
    /// # Errors
    ///
    /// Returns an [`Error`](crate::error::Error) if the event could not be
    /// persisted (serialization or I/O failure).
    fn append(&self, event: &HarnessEvent) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use serde_json::json;
    use std::cell::RefCell;

    fn sample_event(name: &str) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-19T00:00:00.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("event-store-test".to_string()),
                actor_type: None,
            },
            event: name.to_string(),
            payload: json!({"k": "v"}),
            spec: None,
        }
    }

    /// A fake [`EventSink`] proves the trait is what consumers depend on:
    /// a test can collect events in memory with no filesystem at all.
    #[test]
    fn trait_supports_an_in_memory_fake() {
        struct FakeSink {
            collected: RefCell<Vec<String>>,
        }
        impl EventSink for FakeSink {
            fn append(&self, event: &HarnessEvent) -> Result<()> {
                self.collected.borrow_mut().push(event.event.clone());
                Ok(())
            }
        }

        let fake = FakeSink {
            collected: RefCell::new(Vec::new()),
        };
        fake.append(&sample_event("decision")).unwrap();
        assert_eq!(fake.collected.borrow().as_slice(), ["decision"]);
    }
}
