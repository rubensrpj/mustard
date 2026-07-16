//! `rtk gain` normalisation — a port of `scripts/_rtk-gain.js`.
//!
//! The JS `_rtk-gain.js` was a shared helper, not a standalone script: it
//! shells `rtk gain --all --format json` and normalises the result across rtk
//! versions. This module keeps it as a helper consumed by `run statusline`.
//!
//! Fail-open: `rtk` missing, a timeout, or unparseable JSON yields `None`,
//! exactly like the JS helper returning `null`.

use serde_json::Value;
use std::process::{Command, Stdio};

/// Normalised `rtk gain` summary — the fields the statusline segment consumes.
#[derive(Debug, Clone)]
pub struct RtkGain {
    /// Total tokens saved by RTK rewrites.
    pub saved: i64,
    /// Average savings percentage.
    pub pct: f64,
}

/// Read a numeric field from a `serde_json` object, tolerating string numbers
/// and the alternate key spellings `_rtk-gain.js` accepted.
fn num(obj: &Value, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(n) = v.as_f64() {
                return n;
            }
            if let Some(s) = v.as_str() {
                if let Ok(n) = s.parse::<f64>() {
                    return n;
                }
            }
        }
    }
    0.0
}

/// Shell `rtk gain --all --format json` and normalise the result.
///
/// Returns `None` on any failure (rtk absent, non-zero exit, bad JSON), or
/// when both `saved` and `commands` are non-positive — the JS guard
/// `if (saved <= 0 && commands <= 0) return null`.
#[must_use]
pub fn get_rtk_gain() -> Option<RtkGain> {
    let output = Command::new("rtk")
        .args(["gain", "--all", "--format", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let data: Value = serde_json::from_str(&raw).ok()?;
    // The JS reads `data.summary` when present, else `data` itself.
    let summary = data.get("summary").unwrap_or(&data);

    let saved = num(summary, &["total_saved", "saved_tokens", "savedTokens"]) as i64;
    let pct = num(summary, &["avg_savings_pct", "savings_pct", "savingsPct"]);
    let commands = num(summary, &["total_commands", "commands"]) as i64;

    if saved <= 0 && commands <= 0 {
        return None;
    }
    Some(RtkGain { saved, pct })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn num_reads_string_and_numeric() {
        let v = json!({ "a": "12.5", "b": 7, "c": "nope" });
        assert!((num(&v, &["a"]) - 12.5).abs() < f64::EPSILON);
        assert!((num(&v, &["b"]) - 7.0).abs() < f64::EPSILON);
        assert_eq!(num(&v, &["c"]), 0.0);
        assert_eq!(num(&v, &["missing"]), 0.0);
    }
}
