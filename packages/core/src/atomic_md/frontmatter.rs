//! Generic YAML frontmatter extractor for any Mustard markdown file.
//!
//! ## Design
//!
//! - **No `serde_yaml` dependency.** The workspace has deliberately kept
//!   `serde_yaml` out (see `skill/frontmatter.rs` commentary). The parsed value
//!   is stored as a `serde_json::Value` (already a workspace dep); the YAML
//!   body is converted with a small hand-rolled subset that handles the shapes
//!   Mustard itself produces.
//! - **Fail-open.** Invalid or absent frontmatter returns `None` without panic.
//!   The caller receives the remainder of the document unchanged.
//! - **Idempotent on round-trip.** `parse` only consumes the leading
//!   `---\n…\n---` block; the rest of the file is passed through as `body`.

use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Parsed YAML frontmatter from a markdown document.
///
/// The inner [`Value`] is always a JSON object (`Value::Object`). Unknown or
/// complex YAML constructs are preserved as string values rather than dropped,
/// keeping round-trips lossless for the common shapes Mustard produces.
#[derive(Debug, Clone, PartialEq)]
pub struct Frontmatter(pub Value);

impl Frontmatter {
    /// Retrieve a top-level string field by key.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

    /// Retrieve a top-level field as a [`Value`] reference.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Expose the inner JSON object map for iteration.
    #[must_use]
    pub fn as_object(&self) -> Option<&Map<String, Value>> {
        self.0.as_object()
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract the leading YAML frontmatter block from a markdown document.
///
/// Returns `(Some(frontmatter), body)` when the document starts with a
/// `---\n…\n---` fence; returns `(None, text)` when the fence is absent or
/// the YAML fails to parse.
///
/// The `body` slice is a sub-slice of `text` — no allocations for the body
/// path, only the frontmatter string itself is copied.
#[must_use]
pub fn parse(text: &str) -> (Option<Frontmatter>, &str) {
    let Some((yaml_body, rest)) = extract_fence(text) else {
        return (None, text);
    };
    let fm = parse_yaml_to_object(&yaml_body);
    (Some(Frontmatter(Value::Object(fm))), rest)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Strip the `---\n…\n---` fence. Returns `(yaml_body, rest_of_document)` or
/// `None` when no valid fence is found.
fn extract_fence(text: &str) -> Option<(String, &str)> {
    // Normalise CRLF so all matching is on `\n`.
    let normalized: std::borrow::Cow<str> = if text.contains('\r') {
        std::borrow::Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(text)
    };

    let rest = normalized.strip_prefix("---\n")?;
    // Locate the closing `\n---` (possibly followed by `\n` or EOF).
    let end = rest.find("\n---")?;
    let yaml_body = rest[..end].to_string();

    // How far did we consume in the *original* (possibly CRLF) text?
    // Re-compute offset from the original bytes: `"---\n"` (4) + `end` chars
    // of body + `"\n---"` (4) + optional trailing newline.
    let consumed_in_normalized = 4 + end + 4;
    let tail_normalized = &normalized[consumed_in_normalized..];
    let tail_normalized = tail_normalized.strip_prefix('\n').unwrap_or(tail_normalized);

    // Map the normalized tail back to the original text via byte offset.
    // Since we only swapped `\r\n` → `\n`, the tail in the original starts
    // no earlier than the corresponding position — but to keep things simple
    // and avoid an unsafe transmute, we search for the tail content in the
    // original string. For the typical (LF-only) case this is a zero-copy
    // no-op because `Cow::Borrowed`.
    let body_start = if tail_normalized.is_empty() {
        text.len()
    } else {
        // Find the first occurrence of the tail's initial bytes in `text`.
        text.find(&*tail_normalized).unwrap_or(text.len())
    };

    Some((yaml_body, &text[body_start..]))
}

/// Minimal YAML object parser: handles top-level scalar and block-list / flow-
/// list values for the shapes Mustard generates. Anything more exotic lands as
/// a `Value::String` of the raw literal.
fn parse_yaml_to_object(yaml: &str) -> Map<String, Value> {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut map = Map::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        // Only process top-level keys (column 0).
        if line.starts_with([' ', '\t']) {
            i += 1;
            continue;
        }
        let Some((raw_key, raw_val)) = line.split_once(':') else {
            i += 1;
            continue;
        };
        let key = raw_key.trim().to_string();
        let val_str = raw_val.trim_start();

        if val_str.starts_with('[') && val_str.trim_end().ends_with(']') {
            // Flow list: `key: [a, b, c]`
            let inner = &val_str.trim_end()[1..val_str.trim_end().len() - 1];
            let items: Vec<Value> = inner
                .split(',')
                .map(|s| Value::String(unquote(s.trim()).to_string()))
                .filter(|v| v.as_str().map_or(true, |s| !s.is_empty()))
                .collect();
            map.insert(key, Value::Array(items));
            i += 1;
        } else if val_str.is_empty() || val_str == "|" || val_str == ">" {
            // Block sequence or literal/folded block — read indented children.
            let mut items: Vec<Value> = Vec::new();
            let mut j = i + 1;
            while j < lines.len() && lines[j].starts_with([' ', '\t']) {
                let child = lines[j].trim();
                if let Some(rest) = child.strip_prefix("- ") {
                    items.push(Value::String(unquote(rest.trim()).to_string()));
                } else if !child.is_empty() && !child.starts_with('#') {
                    // Nested scalar block line — collect as string continuation.
                    items.push(Value::String(unquote(child).to_string()));
                }
                j += 1;
            }
            if items.is_empty() {
                map.insert(key, Value::Null);
            } else if items.len() == 1 {
                // Single-line block scalar — unwrap.
                map.insert(key, items.remove(0));
            } else {
                map.insert(key, Value::Array(items));
            }
            i = j;
            continue;
        } else {
            map.insert(key, Value::String(unquote(val_str).to_string()));
            i += 1;
        }
    }
    map
}

/// Strip one layer of `"…"` or `'…'` quotes.
fn unquote(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2
        && ((t.starts_with('"') && t.ends_with('"'))
            || (t.starts_with('\'') && t.ends_with('\'')))
    {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_frontmatter() {
        let text = "---\nstage: Execute\noutcome: Active\n---\n## Body\n";
        let (fm, body) = parse(text);
        let fm = fm.expect("frontmatter present");
        assert_eq!(fm.get_str("stage"), Some("Execute"));
        assert_eq!(fm.get_str("outcome"), Some("Active"));
        assert!(body.contains("## Body"));
    }

    #[test]
    fn returns_none_when_no_fence() {
        let text = "## Just a body\nno frontmatter\n";
        let (fm, body) = parse(text);
        assert!(fm.is_none());
        assert_eq!(body, text);
    }

    #[test]
    fn lenient_on_invalid_yaml_shape() {
        // Malformed YAML — should not panic, just produce partial/empty map.
        let text = "---\n: bad key\nstage: Execute\n---\nbody";
        let (fm, _body) = parse(text);
        // At minimum it should not panic; stage may or may not be captured.
        let _ = fm;
    }

    #[test]
    fn parses_flow_list() {
        let text = "---\ntags: [add, fix]\n---\nbody";
        let (fm, _) = parse(text);
        let fm = fm.expect("present");
        let tags = fm.get("tags").and_then(|v| v.as_array()).expect("array");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].as_str(), Some("add"));
    }

    #[test]
    fn parses_block_list() {
        let text = "---\ntags:\n  - add\n  - refactor\n---\nbody";
        let (fm, _) = parse(text);
        let fm = fm.expect("present");
        let tags = fm.get("tags").and_then(|v| v.as_array()).expect("array");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[1].as_str(), Some("refactor"));
    }

    #[test]
    fn body_starts_after_fence() {
        let text = "---\nk: v\n---\nLine one\nLine two\n";
        let (_, body) = parse(text);
        assert!(body.starts_with("Line one"), "body = {body:?}");
    }
}
