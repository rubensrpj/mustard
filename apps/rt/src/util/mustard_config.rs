//! Typed, fail-open accessors for the scan/wave gate overrides stored in
//! `<project_root>/mustard.json`.
//!
//! Before this module each gate that wanted a `mustard.json` override hand-rolled
//! its own `read_to_string → from_str` peek (see `spec_draft::read_mustard_tone`,
//! `spec_validate`, `complete_spec`). The scan/wave agnosticism work (F0-e) needs
//! three more such overrides — `sourceExtensions`, `primaryExt` and
//! `rolePatterns` — so this is their single home. Every accessor is fail-open: a
//! missing, unreadable, or malformed `mustard.json`, or a key of the wrong type,
//! reads as "no override" rather than panicking or erroring. A gate must never be
//! blocked by a config typo, and the agnostic fallback always stands behind it.
//!
//! All accessors take the parsed JSON [`Value`] so a caller that already read
//! `mustard.json` (e.g. to pull `tone` and `rolePatterns` in one pass) does a
//! single disk read. [`load`] is the convenience reader for callers that only
//! need this file.

use serde_json::Value;
use std::path::Path;

/// Read and parse `<project_root>/mustard.json`, fail-open to `None`.
///
/// Returns `None` when the file is absent, unreadable, or not valid JSON —
/// callers then fall back to their agnostic defaults.
#[must_use]
pub fn load(project_root: &Path) -> Option<Value> {
    crate::util::json_io::read_json(&project_root.join("mustard.json"))
}

/// Normalise a user-supplied extension token to the dotted form the scanners
/// compare against (`rb` → `.rb`, `.rb` → `.rb`). Empty / whitespace-only
/// tokens yield `None`.
fn normalize_ext(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    Some(if t.starts_with('.') {
        t.to_string()
    } else {
        format!(".{t}")
    })
}

/// Additional source-file extensions from `mustard.json#sourceExtensions`.
///
/// The value must be an array of strings; each entry is normalised to the
/// dotted form (`["rb", ".zig"]` → `[".rb", ".zig"]`). A missing key, a
/// non-array value, or non-string entries are skipped (fail-open ⇒ empty list).
/// This is **additive**: the visitor already treats unknown extensions as
/// generic source, so this list only lets a user force-include extensions the
/// heuristics would otherwise treat as a non-source asset.
#[must_use]
pub fn source_extensions(config: &Value) -> Vec<String> {
    config
        .get("sourceExtensions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .filter_map(normalize_ext)
                .collect()
        })
        .unwrap_or_default()
}

/// Explicit primary extension override from `mustard.json#primaryExt`.
///
/// Lets a user pin the dominant extension the cluster/convention gates operate
/// on, overriding both the per-stack table and the frequency-derived fallback.
/// A missing key or a non-string value yields `None`.
#[must_use]
pub fn primary_ext(config: &Value) -> Option<String> {
    config
        .get("primaryExt")
        .and_then(Value::as_str)
        .and_then(normalize_ext)
}

/// Explicit architecture-style override from `mustard.json#architecture`.
///
/// Lets a user pin the architectural style the scan reports (`clean`,
/// `hexagonal`, `layered`, `ddd`, …), overriding the deterministic folder /
/// import-graph inference. The value is trimmed and lowercased so the registry
/// tag is normalised. A missing key, a non-string value, or an empty / blank
/// string yields `None` (fall back to detection).
#[must_use]
pub fn architecture(config: &Value) -> Option<String> {
    let raw = config.get("architecture").and_then(Value::as_str)?.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_ascii_lowercase())
    }
}

/// One `{ pattern, role }` mapping from `mustard.json#rolePatterns`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePattern {
    /// Lowercased substring (or simple `*` glob) tested against the file path.
    pub pattern: String,
    /// The role assigned on the first matching pattern.
    pub role: String,
}

/// Ordered role-classification overrides from `mustard.json#rolePatterns`.
///
/// The value must be an array of `{ "pattern": "...", "role": "..." }` objects.
/// Order is preserved — the first matching pattern wins, exactly like the
/// built-in classifier. Entries missing either field, or of the wrong type, are
/// skipped (fail-open). The `pattern` is lowercased here so matching is
/// case-insensitive without re-lowering per file.
#[must_use]
pub fn role_patterns(config: &Value) -> Vec<RolePattern> {
    config
        .get("rolePatterns")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let pattern = entry.get("pattern").and_then(Value::as_str)?.trim();
                    let role = entry.get("role").and_then(Value::as_str)?.trim();
                    if pattern.is_empty() || role.is_empty() {
                        return None;
                    }
                    Some(RolePattern {
                        pattern: pattern.to_lowercase(),
                        role: role.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Test whether `pattern` (lowercased) matches `haystack` (lowercased).
///
/// Supports a simple glob: `*` is a wildcard for "any run of characters". A
/// pattern with no `*` is a plain substring test (matching the built-in
/// classifier's `contains` semantics). Anchors (`^`/`$`) are not special — the
/// match is unanchored unless the pattern itself begins/ends with `*`-free
/// segments at the string boundary.
#[must_use]
pub fn glob_matches(pattern: &str, haystack: &str) -> bool {
    if !pattern.contains('*') {
        return haystack.contains(pattern);
    }
    // Split on `*`; every non-empty segment must appear in order. Leading and
    // trailing empty segments (from a leading/trailing `*`) relax the
    // corresponding anchor.
    let segments: Vec<&str> = pattern.split('*').collect();
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');
    let mut cursor = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        if seg.is_empty() {
            continue;
        }
        let Some(found) = haystack[cursor..].find(seg) else {
            return false;
        };
        let abs = cursor + found;
        // First non-empty segment with an anchored start must sit at index 0.
        if anchored_start && i == 0 && abs != 0 {
            return false;
        }
        cursor = abs + seg.len();
    }
    // Anchored end: the last non-empty segment must reach the string end.
    if anchored_end {
        if let Some(last) = segments.iter().rev().find(|s| !s.is_empty()) {
            return haystack.ends_with(last);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_extensions_normalises_and_skips_garbage() {
        let cfg = json!({ "sourceExtensions": ["rb", ".zig", "", 42, "  swift "] });
        assert_eq!(
            source_extensions(&cfg),
            vec![".rb".to_string(), ".zig".to_string(), ".swift".to_string()]
        );
    }

    #[test]
    fn source_extensions_absent_or_wrong_type_is_empty() {
        assert!(source_extensions(&json!({})).is_empty());
        assert!(source_extensions(&json!({ "sourceExtensions": "rb" })).is_empty());
    }

    #[test]
    fn primary_ext_override() {
        assert_eq!(primary_ext(&json!({ "primaryExt": "foo" })), Some(".foo".to_string()));
        assert_eq!(primary_ext(&json!({ "primaryExt": ".bar" })), Some(".bar".to_string()));
        assert_eq!(primary_ext(&json!({})), None);
        assert_eq!(primary_ext(&json!({ "primaryExt": 7 })), None);
    }

    #[test]
    fn architecture_override_normalises_and_skips_blank() {
        assert_eq!(architecture(&json!({ "architecture": "Clean" })), Some("clean".to_string()));
        assert_eq!(architecture(&json!({ "architecture": "  Hexagonal " })), Some("hexagonal".to_string()));
        assert_eq!(architecture(&json!({ "architecture": "" })), None);
        assert_eq!(architecture(&json!({ "architecture": "   " })), None);
        assert_eq!(architecture(&json!({})), None);
        assert_eq!(architecture(&json!({ "architecture": 7 })), None);
    }

    #[test]
    fn role_patterns_parse_in_order_and_skip_bad_entries() {
        let cfg = json!({
            "rolePatterns": [
                { "pattern": "Controllers", "role": "api" },
                { "pattern": "Views", "role": "ui" },
                { "role": "missing-pattern" },
                { "pattern": "no-role" },
                "not-an-object"
            ]
        });
        let pats = role_patterns(&cfg);
        assert_eq!(
            pats,
            vec![
                RolePattern { pattern: "controllers".to_string(), role: "api".to_string() },
                RolePattern { pattern: "views".to_string(), role: "ui".to_string() },
            ]
        );
    }

    #[test]
    fn role_patterns_absent_is_empty() {
        assert!(role_patterns(&json!({})).is_empty());
    }

    #[test]
    fn glob_matches_substring_and_wildcard() {
        assert!(glob_matches("controller", "src/usercontroller.rb"));
        assert!(!glob_matches("controller", "src/user.rb"));
        assert!(glob_matches("src/*.rb", "src/foo.rb"));
        assert!(glob_matches("*controller*", "src/usercontroller.rb"));
        assert!(glob_matches("src/*/handlers", "src/api/handlers"));
        assert!(!glob_matches("src/*.rb", "lib/foo.rb"));
        // Anchored end: must finish with the trailing segment.
        assert!(glob_matches("*.rb", "x.rb"));
        assert!(!glob_matches("*.rb", "x.rs"));
    }
}
