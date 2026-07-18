//! `mustard-rt run tactical-fix-detect` — propose (do **not** create) tactical
//! fixes from structured `review.result` / `qa.result` payloads (F4-c item 4).
//!
//! ## Decision 6 — tactical-fix / follow-up is *semi*-automatic
//!
//! Unlike the structural auto-openers (re-wave, wave-advance, epic-fold — all
//! fully automatic), tactical-fix is **semi**: Rust detects + proposes, and the
//! orchestrator (or the user) confirms before the sub-spec is created. This
//! command is the *detect + propose* half. It **never** calls
//! [`crate::commands::spec::tactical_fix_create`] — the inviolable
//! "não auto-aprovar" rule. Creation stays a one-confirmation step downstream.
//!
//! ## Payload contract — `tactical_fix_candidates[]`
//!
//! A `review.result` or `qa.result` event MAY carry a `tactical_fix_candidates`
//! array in its payload. Each element is an object:
//!
//! ```json
//! {
//!   "description": "string (required) — one-line fix summary",
//!   "scope":       "string (optional) — affected files / area",
//!   "severity":    "string (optional) — critical | major | minor"
//! }
//! ```
//!
//! For every candidate this command emits ONE `tactical_fix.proposed` event:
//!
//! ```json
//! {
//!   "parent":      "<spec>",
//!   "description": "...",
//!   "scope":       "...",
//!   "severity":    "...",
//!   "source":      "review.result" | "qa.result",
//!   "candidate_id":"<sha256(parent|description|scope)[..16]>",
//!   "status":      "proposed"
//! }
//! ```
//!
//! ## Idempotency
//!
//! Each proposal is keyed by `candidate_id` — a SHA-256 of
//! `parent|description|scope`. Before emitting, the command scans the spec's
//! per-spec NDJSON `.events/` dir for an existing `tactical_fix.proposed` with
//! the same `candidate_id`; a match is skipped. So re-running after the same
//! review/qa payload proposes each fix exactly once.
//!
//! ## Fail-open
//!
//! A missing events dir, an unparseable payload, or a write failure all degrade
//! to "nothing proposed" — the detector is advisory telemetry, never a gate.

use crate::shared::context::{project_dir, session_id};
use mustard_core::time::now_iso8601;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde_json::{json, Value};
use std::path::Path;

/// `tactical_fix.proposed` — a Rust-detected tactical-fix candidate awaiting
/// confirmation. Distinct from the `spec.link` an actual `tactical-fix-create`
/// emits: a *proposal* never creates a sub-spec (decision 6, "não auto-aprovar").
const EVENT_TACTICAL_FIX_PROPOSED: &str = "tactical_fix.proposed";

/// One detected candidate, normalised from a `review.result` / `qa.result`
/// payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// One-line fix summary (required — candidates without it are dropped).
    pub description: String,
    /// Optional affected files / area.
    pub scope: String,
    /// Optional severity (`critical` / `major` / `minor`).
    pub severity: String,
    /// Which event carried it (`review.result` / `qa.result`).
    pub source: String,
}

impl Candidate {
    /// Stable id — `sha256(parent|description|scope)` truncated to 16 hex chars.
    /// Drives idempotency: the same (parent, description, scope) always hashes
    /// to the same id, so a re-detect proposes it at most once.
    #[must_use]
    pub(crate) fn candidate_id(&self, parent: &str) -> String {
        let material = format!("{parent}|{}|{}", self.description, self.scope);
        let mut h = crate::util::sha256::Sha256::new();
        h.update(material.as_bytes());
        h.hex_digest().chars().take(16).collect()
    }
}

/// Pull every `tactical_fix_candidates[]` entry out of one event payload.
/// Candidates without a non-empty `description` are dropped (a proposal needs a
/// summary). Pure over `(payload, source)`.
fn candidates_from_payload(payload: &Value, source: &str) -> Vec<Candidate> {
    let Some(arr) = payload.get("tactical_fix_candidates").and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|c| {
            let description = c
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if description.is_empty() {
                return None;
            }
            Some(Candidate {
                description,
                scope: c.get("scope").and_then(Value::as_str).unwrap_or("").trim().to_string(),
                severity: c
                    .get("severity")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                source: source.to_string(),
            })
        })
        .collect()
}

/// Read the spec's per-spec NDJSON `.events/` dir and collect every tactical-fix
/// candidate carried by a `review.result` / `qa.result` event. Deterministic:
/// events are scanned in chronological order. Fail-open to an empty vec.
#[must_use]
pub(crate) fn detect_candidates(cwd: &Path, spec: &str) -> Vec<Candidate> {
    let events_dir = ClaudePaths::spec_dir_or_unchecked(cwd, spec).join(".events");
    let mut events = read_harness_events_from_ndjson_dir(&events_dir);
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    let mut out = Vec::new();
    for ev in &events {
        let source = match ev.event.as_str() {
            "review.result" => "review.result",
            "qa.result" => "qa.result",
            _ => continue,
        };
        out.extend(candidates_from_payload(&ev.payload, source));
    }
    out
}

/// The set of `candidate_id`s already proposed for `spec` — the idempotency
/// guard set. Reads the same per-spec NDJSON dir.
fn already_proposed_ids(cwd: &Path, spec: &str) -> std::collections::BTreeSet<String> {
    let events_dir = ClaudePaths::spec_dir_or_unchecked(cwd, spec).join(".events");
    read_harness_events_from_ndjson_dir(&events_dir)
        .iter()
        .filter(|e| e.event == EVENT_TACTICAL_FIX_PROPOSED)
        .filter_map(|e| {
            e.payload
                .get("candidate_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

/// Emit one `tactical_fix.proposed` event. Best-effort (fail-open).
fn emit_proposal(cwd: &Path, spec: &str, candidate: &Candidate, candidate_id: &str) {
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("tactical-fix-detect".to_string()),
            actor_type: None,
        },
        event: EVENT_TACTICAL_FIX_PROPOSED.to_string(),
        payload: json!({
            "parent": spec,
            "description": candidate.description,
            "scope": candidate.scope,
            "severity": candidate.severity,
            "source": candidate.source,
            "candidate_id": candidate_id,
            "status": "proposed",
        }),
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(cwd.to_string_lossy().as_ref(), &ev);
}

/// Detect + propose, returning the list of newly-proposed `(candidate_id,
/// description)` pairs. Idempotent: candidates already proposed are skipped.
/// Pure orchestration over the helpers above — no printing.
#[must_use]
pub(crate) fn propose(cwd: &Path, spec: &str) -> Vec<(String, String)> {
    let candidates = detect_candidates(cwd, spec);
    let already = already_proposed_ids(cwd, spec);
    let mut emitted_ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut newly = Vec::new();
    for candidate in &candidates {
        let id = candidate.candidate_id(spec);
        // Skip if already on disk OR already emitted in this same run (a review
        // and a qa event can carry the same candidate).
        if already.contains(&id) || emitted_ids.contains(&id) {
            continue;
        }
        emit_proposal(cwd, spec, candidate, &id);
        emitted_ids.insert(id.clone());
        newly.push((id, candidate.description.clone()));
    }
    newly
}

/// Dispatch `mustard-rt run tactical-fix-detect --spec <name>`.
///
/// Emits one `tactical_fix.proposed` event per new candidate and writes a JSON
/// report to stdout. NEVER creates a sub-spec (decision 6 — "não auto-aprovar").
pub fn run(spec: Option<&str>) {
    let Some(spec) = spec else {
        eprintln!("Usage: tactical-fix-detect --spec <name>");
        return;
    };
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
    let newly = propose(&cwd, spec);
    let proposed: Vec<Value> = newly
        .iter()
        .map(|(id, desc)| json!({ "candidate_id": id, "description": desc }))
        .collect();
    let out = json!({
        "spec": spec,
        "proposed_count": proposed.len(),
        "proposed": proposed,
        // Explicit: this command proposes only — it does not create sub-specs.
        "created": false,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::events::writer_ndjson::write_event;
    use tempfile::tempdir;

    fn seed_review(project: &Path, spec: &str, candidates: Value) {
        let payload = json!({
            "spec": spec,
            "verdict": "rejected",
            "criticalCount": 1,
            "tactical_fix_candidates": candidates,
        });
        let _ = write_event(
            project, Some(spec), None, "s", "review.result", "review",
            Some(0), Some("s"), Some("test"), None, &payload,
        );
    }

    #[test]
    fn extracts_candidates_with_description() {
        let payload = json!({
            "tactical_fix_candidates": [
                { "description": "fix off-by-one in pager", "scope": "src/pager.rs", "severity": "major" },
                { "scope": "no-desc" },              // dropped — no description
                { "description": "   " },            // dropped — blank description
                { "description": "tighten error mapping" }
            ]
        });
        let got = candidates_from_payload(&payload, "review.result");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].description, "fix off-by-one in pager");
        assert_eq!(got[0].scope, "src/pager.rs");
        assert_eq!(got[0].severity, "major");
        assert_eq!(got[1].description, "tighten error mapping");
        assert_eq!(got[1].source, "review.result");
    }

    #[test]
    fn candidate_id_is_stable_and_short() {
        let c = Candidate {
            description: "fix x".into(),
            scope: "a.rs".into(),
            severity: "minor".into(),
            source: "qa.result".into(),
        };
        let id1 = c.candidate_id("parent-spec");
        let id2 = c.candidate_id("parent-spec");
        assert_eq!(id1, id2, "same input → same id");
        assert_eq!(id1.len(), 16);
        // Different parent → different id.
        assert_ne!(id1, c.candidate_id("other-spec"));
    }

    #[test]
    fn propose_emits_one_event_per_candidate_and_is_idempotent() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let project = dir.path();
        let spec = "tf-spec";
        seed_review(
            project,
            spec,
            json!([
                { "description": "fix A", "scope": "a.rs" },
                { "description": "fix B", "scope": "b.rs" }
            ]),
        );

        // First run proposes both.
        let first = propose(project, spec);
        assert_eq!(first.len(), 2, "first run proposes both candidates");

        // Second run proposes nothing (idempotent — both already on disk).
        let second = propose(project, spec);
        assert!(second.is_empty(), "second run must be a no-op; got {second:?}");

        // Exactly two tactical_fix.proposed events landed; no sub-spec created.
        let events_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec(spec)
            .unwrap()
            .events_dir();
        let proposed = read_harness_events_from_ndjson_dir(&events_dir)
            .into_iter()
            .filter(|e| e.event == EVENT_TACTICAL_FIX_PROPOSED)
            .count();
        assert_eq!(proposed, 2, "exactly one proposal per candidate");
        // No spec.link (would imply a sub-spec was created) — proposal only.
        let links = read_harness_events_from_ndjson_dir(&events_dir)
            .into_iter()
            .filter(|e| e.event == "spec.link")
            .count();
        assert_eq!(links, 0, "detect must NOT create a sub-spec (no spec.link)");
    }

    #[test]
    fn no_candidates_field_proposes_nothing() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let project = dir.path();
        let spec = "tf-empty";
        // A review.result with no tactical_fix_candidates field.
        let _ = write_event(
            project, Some(spec), None, "s", "review.result", "review",
            Some(0), Some("s"), Some("test"), None,
            &json!({ "spec": spec, "verdict": "approved", "criticalCount": 0 }),
        );
        assert!(propose(project, spec).is_empty());
    }
}
