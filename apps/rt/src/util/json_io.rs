//! Generic JSON file I/O for `Value` payloads (notably the
//! `.pipeline-states/{spec}.json` sidecars).
//!
//! Before this module, an identical `read_json` / `read_state`
//! (`read_to_string → from_str → Option<Value>`) was copy-pasted into
//! `exec_rewave_check`, `epic_fold`, `complete_spec`, and `event_projections`.
//! This is the single home: every call site reads via [`read_json`].
//!
//! Fail-open / fail-soft: a missing, unreadable, or unparseable file reads as
//! `None`. JSON state is best-effort and never load-bearing on the hot path.

use std::path::Path;

use mustard_core::io::fs;
use serde_json::Value;

/// Read a JSON file into a [`Value`], returning `None` on any error (missing,
/// unreadable, or unparseable).
#[must_use]
pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn read_missing_is_none() {
        let dir = tempdir().unwrap();
        assert_eq!(read_json(&dir.path().join("nope.json")), None);
    }

    #[test]
    fn read_malformed_is_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(read_json(&path), None);
    }
}
