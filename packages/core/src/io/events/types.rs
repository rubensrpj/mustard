//! NDJSON event types for the git-source-of-truth event log.
//!
//! [`Event`] is the single row unit read from `.claude/.harness/events/*.ndjson`
//! files. The struct is intentionally lenient: unknown fields are silently
//! ignored so that new event kinds from the harness never break readers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One event from a per-spec NDJSON event log.
///
/// `kind` is the event discriminator (e.g. `"tool.use"`, `"pipeline.status"`).
/// `payload` is an open-ended JSON object; shape depends on `kind`.
///
/// Lenient by design: omits `#[serde(deny_unknown_fields)]` and uses a
/// catch-all `raw` field so new harness fields never break deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Event discriminator — the `kind` field in the NDJSON line.
    pub kind: String,
    /// Free-form payload; shape is event-kind-specific.
    pub payload: Value,
    /// Catch-all for any other fields emitted by the harness now or in the
    /// future. Stored as a flat JSON map; never serialized back with extra keys
    /// unless explicitly re-encoded.
    #[serde(flatten)]
    pub raw: Value,
}
