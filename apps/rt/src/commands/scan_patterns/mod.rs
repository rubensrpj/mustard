//! `scan-patterns-*` — the enrich hand-off for the per-subproject
//! `{role}-pattern` skill *molds*, the pattern-mold twin of the `scan_guards`
//! pair.
//!
//! A mustard-generated mold is DERIVED: it is regenerated fresh on every scan,
//! never preserved. The mold flow is **sweep → list → author → apply**:
//!
//! - [`sweep`] deletes every mustard-generated mold (`source: scan`) BEFORE
//!   generation, so each is re-authored from the current exemplars with no bias
//!   from its old text. A hand-authored/adopted mold (`source: manual`) is
//!   preserved.
//! - [`list`] projects the mold worklist deterministically FROM
//!   `grain.model.json` (role clusters ≥3, attributed to their subproject, with
//!   real exemplars). Post-sweep everything is a create; a surviving hand mold
//!   or a recorded decline is never proposed.
//! - [`apply`] writes one authored mold create-only, path-shape-guarded and
//!   atomic, stamping the [`origin`] notice; being a `mustard-rt run` command it
//!   sidesteps the background-isolation gate that stalled the orchestrator's
//!   Write.
//! - [`decline`] records the agent's justified refusals so a dead candidate
//!   stops burning a dispatch on every scan.
//! - [`relay`] applies an agent's WHOLE return in one call — the envelope
//!   parser that keeps the per-block hand-off from scaling with cluster count.
//! - [`origin`] is the mustard-vs-hand line: the frontmatter `source:` field.
//!
//! [`read_envelope`] lives here rather than in either command because BOTH
//! [`relay`] and [`apply`] take the agent's text through `--content`, and the
//! two kept separate copies with divergent contracts. One reader means the two
//! commands cannot drift apart on what `--content` means.
//!
//! All fully fail-open per the `mustard-rt run` contract.

pub mod apply;
pub mod decline;
pub mod list;
pub(crate) mod origin;
pub mod relay;
pub mod sweep;

use std::io::Read as _;

/// What a `--content` value resolved to.
///
/// `--content` accepts THREE channels: `-` reads stdin, `@<path>` reads that
/// file, and anything else is the literal envelope. The variants exist so a
/// caller can tell "here is the text" from "this PATH could not be read" —
/// and never from "this stdin could not be read". That asymmetry is deliberate:
/// stdin is how every dispatch that works TODAY reaches the relay, so it keeps
/// its fail-open behaviour byte for byte (a failed read degrades to the empty
/// envelope and the caller proceeds), and only the new file channel reports its
/// IO error. The change ADDS a third channel; it never alters the two that
/// exist.
#[derive(Debug)]
pub(crate) enum Envelope {
    /// Plain envelope text — the literal flag value, stdin, or a file whose
    /// bytes are not a harness-persisted return.
    Raw(String),
    /// The `text` fields harvested out of a harness-persisted return, joined in
    /// the order the document lists them. Only the file channel yields this.
    Json(String),
    /// An `@<path>` that could not be read, with the IO error already phrased
    /// for a report entry.
    Unreadable(String),
}

/// Resolve a `--content` value into the envelope to parse.
///
/// The ONE reader [`relay`] and [`apply`] share. Each caller keeps only its own
/// post-processing on top (the apply's trim-to-`None`, the relay's report
/// entries) — there is no wrapper in between.
pub(crate) fn read_envelope(content: &str) -> Envelope {
    if content == "-" {
        // Unchanged: a failed stdin read degrades to the empty envelope and the
        // caller proceeds. See [`Envelope`] for why this channel never reports.
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        return Envelope::Raw(buf);
    }
    let Some(path) = file_face(content) else {
        return Envelope::Raw(content.to_string());
    };
    match std::fs::read_to_string(path) {
        Ok(text) => harness_json(&text).map_or(Envelope::Raw(text), Envelope::Json),
        Err(e) => Envelope::Unreadable(format!("cannot read {path}: {e}")),
    }
}

/// A SHORT name for where an envelope came from — what a report cites when the
/// problem is the CHANNEL rather than a block. Never the envelope itself: a
/// literal `--content` value is a whole agent return, so echoing it into the
/// report would print the return back at the caller.
pub(crate) fn content_label(content: &str) -> String {
    if content == "-" {
        return "(stdin)".to_string();
    }
    file_face(content).unwrap_or("(--content)").to_string()
}

/// The path inside the `@<path>` form, or `None` when the value is the literal
/// envelope.
///
/// `@` opens a path ONLY when the value carries no newline: a literal envelope
/// ALWAYS spans lines — the demarcators occupy whole lines by construction — so
/// the ambiguity is unreachable in practice, and the rule costs one condition
/// instead of a second flag to keep in step on two commands.
///
/// `pub(crate)` because [`relay`] must know whether the envelope came from a
/// FILE before it decides what "no blocks" means: a file that was read and
/// demarcated nothing is a failure to interpret, while a literal `--content`
/// that demarcates nothing is fail-open. [`Envelope`] cannot answer that — a
/// `Raw` covers stdin, the literal value and a file whose bytes are plain text.
pub(crate) fn file_face(content: &str) -> Option<&str> {
    let path = content.strip_prefix('@')?;
    (!path.is_empty() && !content.contains('\n')).then_some(path)
}

/// The envelope hidden inside a harness-persisted return, or `None` when `text`
/// is not one.
///
/// When a subagent's return exceeds the harness's inline limit it is persisted
/// to a file and the orchestrator is handed only a path. That file is written
/// by the HARNESS, not by this code, so its exact shape is not ours to pin:
/// matching ONE shape and falling back to raw text on anything else would
/// re-open the very silence the file face exists to close — an unmatched
/// variant would be read as prose, demarcate nothing, and print
/// `ok:true, blocks:0`. So every `text` field is harvested BY NAME, at any
/// depth, and joined by newline; an unseen variant that nests the same objects
/// is covered without inventing a format.
///
/// Verified against real persisted returns: a pretty-printed array of
/// `{"type":"text","text":…}` whose LAST element is the harness's own
/// `agentId`/`<usage>` trailer. The trailer carries no demarcator, so joining
/// it in cannot add a block.
///
/// Only an ARRAY or an OBJECT counts. A bare JSON scalar (`42`, one quoted
/// word) is a one-line envelope that happens to parse, never a persisted
/// return.
fn harness_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if !value.is_array() && !value.is_object() {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    harvest_text(&value, &mut parts);
    Some(parts.join("\n"))
}

/// Push every `text` string of `value`, at any depth, in the order the document
/// lists them. A `text` field that is not a string is descended into rather than
/// dropped, so a nesting variant still surrenders its prose.
fn harvest_text<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                harvest_text(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, nested) in map {
                match nested.as_str() {
                    Some(s) if key == "text" => out.push(s),
                    _ => harvest_text(nested, out),
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_face_opens_only_on_a_single_line_at_value() {
        assert_eq!(file_face("@a/b.json"), Some("a/b.json"));
        assert_eq!(file_face(r"@C:\tmp\return.json"), Some(r"C:\tmp\return.json"));
        // A literal envelope always spans lines, so an `@` inside one is prose.
        assert_eq!(file_face("@not a path\n=== END ===\n"), None);
        assert_eq!(file_face("@"), None);
        assert_eq!(file_face("plain text"), None);
    }

    #[test]
    fn harness_json_harvests_text_at_any_depth() {
        // The measured shape: an array of `{type, text}` closed by the usage
        // trailer.
        let flat = r#"[{"type":"text","text":"first"},{"type":"text","text":"second"}]"#;
        assert_eq!(harness_json(flat).as_deref(), Some("first\nsecond"));
        // A variant that NESTS the same objects still surrenders them — the
        // whole reason the harvest is by field name rather than by shape.
        let nested = r#"{"content":[{"type":"text","text":"first"}],"meta":{"text":"second"}}"#;
        let got = harness_json(nested).unwrap_or_default();
        assert!(got.contains("first") && got.contains("second"), "nested harvest: {got}");
        // Not a persisted return: prose, and a bare scalar that happens to parse.
        assert_eq!(harness_json("I found nothing worth a mold."), None);
        assert_eq!(harness_json("42"), None);
        assert_eq!(harness_json("\"one word\""), None);
    }
}
