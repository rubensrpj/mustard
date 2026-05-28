//! Generic JSON file I/O for `Value` payloads (notably the
//! `.pipeline-states/{spec}.json` sidecars).
//!
//! Before this module, an identical `read_json` / `read_state`
//! (`read_to_string → from_str → Option<Value>`) was copy-pasted into
//! `exec_rewave_check`, `epic_fold`, `spec_link`, `complete_spec`, and
//! `event_projections`, and a matching pretty-print writer lived in `spec_link`.
//! This is the single home: every call site reads via [`read_json`] and writes
//! via [`write_json`].
//!
//! Both are fail-open / fail-soft: a missing, unreadable, or unparseable file
//! reads as `None`; a write failure returns `false`. JSON state is best-effort
//! and never load-bearing on the hot path.

use std::path::Path;

use mustard_core::io::fs;
use serde_json::Value;

/// Read a JSON file into a [`Value`], returning `None` on any error (missing,
/// unreadable, or unparseable).
#[must_use]
pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Write `value` as pretty JSON with a trailing newline, atomically. Creates
/// the parent directory when absent. Returns `false` on any serialization or
/// write failure (fail-soft).
pub fn write_json(path: &Path, value: &Value) -> bool {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(value) {
        Ok(text) => fs::write_atomic(path, format!("{text}\n").as_bytes()).is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("state.json");
        let value = json!({ "spec": "demo", "wave": 2 });
        assert!(write_json(&path, &value));
        assert_eq!(read_json(&path), Some(value));
    }

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
