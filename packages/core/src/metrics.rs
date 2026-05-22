//! Enforcement metrics — a behavioural port of `_lib/metrics-emit.js`.
//!
//! Every gate hook records what it did by appending one JSON line to
//! `.claude/.metrics/<event>.jsonl`. `metrics-report.js` later iterates every
//! `*.jsonl` in that directory, so per-event sharding is part of the contract.
//!
//! The JS `emitMetric` is **fail-silent**: any error (mkdir, append, a
//! malformed `extras` object) is swallowed so a hook calling it never sees a
//! throw. This port keeps that guarantee — [`MetricLine`] is the pure,
//! testable value, and [`emit_metric`] is the fail-open writer that builds the
//! path, serialises, and appends, returning `bool` (`true` on a successful
//! disk write) exactly like the JS.
//!
//! The line schema, field-for-field with `metrics-emit.js`:
//! `{ ts, event, tokens_affected, tokens_saved, note, ...extras }`.

use crate::error::Result;
use crate::fs::append_line;
use serde_json::{Map, Value};
use std::path::Path;

/// One metric line, before serialisation.
///
/// Pure data — building a [`MetricLine`] never touches the filesystem. The
/// `ts` field is supplied by the caller (an RFC-3339 string from the harness
/// clock) rather than read from a clock here, so the type stays free of side
/// effects and is trivially testable. [`emit_metric`] is the side-effecting
/// half.
#[derive(Debug, Clone)]
pub struct MetricLine {
    /// RFC-3339 / ISO-8601 timestamp, as JS `new Date().toISOString()` emits.
    pub ts: String,
    /// The metric event name, e.g. `"budget-check"`, `"rtk-rewrite"`. Also the
    /// shard file stem: the line lands in `<event>.jsonl`.
    pub event: String,
    /// Conservative count of tokens this event touched. Defaults to `0`.
    pub tokens_affected: i64,
    /// Tokens this event prevented from entering context. Defaults to `0`.
    pub tokens_saved: i64,
    /// Short human label, e.g. `"blocked"`, `"passed"`. Defaults to empty.
    pub note: String,
    /// Extra fields merged flat into the JSON line. A non-object is ignored
    /// (matches the JS `opts.extras && typeof === 'object'` guard).
    pub extras: Value,
}

impl MetricLine {
    /// A metric line with the given timestamp and event name; numeric fields
    /// zeroed, `note` empty, `extras` empty — the JS `opts` defaults.
    #[must_use]
    pub fn new(ts: impl Into<String>, event: impl Into<String>) -> Self {
        Self {
            ts: ts.into(),
            event: event.into(),
            tokens_affected: 0,
            tokens_saved: 0,
            note: String::new(),
            extras: Value::Null,
        }
    }

    /// Set `tokens_affected`. Builder-style.
    #[must_use]
    pub fn tokens_affected(mut self, n: i64) -> Self {
        self.tokens_affected = n;
        self
    }

    /// Set `tokens_saved`. Builder-style.
    #[must_use]
    pub fn tokens_saved(mut self, n: i64) -> Self {
        self.tokens_saved = n;
        self
    }

    /// Set `note`. Builder-style.
    #[must_use]
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    /// Set `extras`. Builder-style.
    #[must_use]
    pub fn extras(mut self, extras: Value) -> Self {
        self.extras = extras;
        self
    }

    /// Render this line to its JSON object.
    ///
    /// The fixed fields (`ts`, `event`, `tokens_affected`, `tokens_saved`,
    /// `note`) are written first; `extras` keys are then merged flat on top —
    /// the JS spread `...extras`. An `extras` value that is not an object is
    /// ignored. A genuine name collision (an `extras` key equal to a fixed
    /// field) lets `extras` win, matching JS object-spread order.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("ts".into(), Value::String(self.ts.clone()));
        map.insert("event".into(), Value::String(self.event.clone()));
        map.insert(
            "tokens_affected".into(),
            Value::Number(self.tokens_affected.into()),
        );
        map.insert(
            "tokens_saved".into(),
            Value::Number(self.tokens_saved.into()),
        );
        map.insert("note".into(), Value::String(self.note.clone()));
        if let Some(extra) = self.extras.as_object() {
            for (key, value) in extra {
                map.insert(key.clone(), value.clone());
            }
        }
        Value::Object(map)
    }
}

/// The `.claude/.metrics/<event>.jsonl` shard path for an event under `cwd`.
///
/// Mirrors the JS `path.join(cwd, '.claude', '.metrics', event + '.jsonl')`.
#[must_use]
pub fn metric_file_path(cwd: &Path, event: &str) -> std::path::PathBuf {
    cwd.join(".claude")
        .join(".metrics")
        .join(format!("{event}.jsonl"))
}

/// Append a metric line to its shard file. Fail-silent port of `emitMetric`.
///
/// Builds `<cwd>/.claude/.metrics/<line.event>.jsonl`, creating the directory
/// if missing, serialises `line`, and appends it. Returns `true` only on a
/// successful disk write; **any** error — an empty event name, a serialisation
/// failure, an I/O failure — yields `false` and is otherwise swallowed, so a
/// hook calling this never observes an error. This is the metrics analogue of
/// the io-layer fail-open rule: better to drop a metric than crash a hook.
///
/// `line.event` must be non-empty (JS rejects a falsy `event`); an empty event
/// returns `false` without touching the filesystem.
#[must_use]
pub fn emit_metric(cwd: &Path, line: &MetricLine) -> bool {
    emit_metric_inner(cwd, line).is_ok()
}

/// The fallible core of [`emit_metric`], kept separate so the `?` operator can
/// be used. [`emit_metric`] collapses the `Result` to `bool` — fail-silent.
fn emit_metric_inner(cwd: &Path, line: &MetricLine) -> Result<()> {
    if line.event.trim().is_empty() {
        return Err(crate::error::Error::config("metric event name is empty"));
    }
    let path = metric_file_path(cwd, &line.event);
    let serialized = serde_json::to_string(&line.to_json())?;
    append_line(&path, &serialized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn to_json_has_fixed_fields_with_defaults() {
        let line = MetricLine::new("2026-05-19T00:00:00.000Z", "budget-check");
        let value = line.to_json();
        assert_eq!(value["event"], json!("budget-check"));
        assert_eq!(value["tokens_affected"], json!(0));
        assert_eq!(value["tokens_saved"], json!(0));
        assert_eq!(value["note"], json!(""));
    }

    #[test]
    fn to_json_merges_extras_flat() {
        let line = MetricLine::new("ts", "rtk-rewrite")
            .note("rewrote")
            .tokens_saved(120)
            .extras(json!({ "command": "git status", "hook": "rtk" }));
        let value = line.to_json();
        assert_eq!(value["tokens_saved"], json!(120));
        assert_eq!(value["note"], json!("rewrote"));
        // extras merged at top level, not nested.
        assert_eq!(value["command"], json!("git status"));
        assert_eq!(value["hook"], json!("rtk"));
    }

    #[test]
    fn to_json_ignores_non_object_extras() {
        let line = MetricLine::new("ts", "ev").extras(json!("not an object"));
        let value = line.to_json();
        // Only the fixed five keys survive.
        assert_eq!(value.as_object().map(serde_json::Map::len), Some(5));
    }

    #[test]
    fn emit_metric_appends_line_to_shard() {
        let dir = tempdir().unwrap();
        let line = MetricLine::new("2026-05-19T00:00:00.000Z", "spec-hygiene-move").note("ok");
        assert!(emit_metric(dir.path(), &line));

        let shard = metric_file_path(dir.path(), "spec-hygiene-move");
        let contents = crate::fs::read_to_string(&shard).unwrap();
        assert!(contents.ends_with('\n'));
        let parsed: Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(parsed["event"], json!("spec-hygiene-move"));
        assert_eq!(parsed["note"], json!("ok"));
    }

    #[test]
    fn emit_metric_appends_multiple_lines() {
        let dir = tempdir().unwrap();
        assert!(emit_metric(dir.path(), &MetricLine::new("t1", "ev")));
        assert!(emit_metric(dir.path(), &MetricLine::new("t2", "ev")));
        let shard = metric_file_path(dir.path(), "ev");
        let lines: Vec<_> = crate::fs::read_to_string(&shard)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn emit_metric_rejects_empty_event_without_io() {
        let dir = tempdir().unwrap();
        let line = MetricLine::new("ts", "   ");
        assert!(!emit_metric(dir.path(), &line));
        // Nothing was written.
        assert!(!dir.path().join(".claude").join(".metrics").exists());
    }
}
