//! `mustard-rt run epic-fold` — consolidate and compact harness events when
//! an epic completes.
//!
//! - `--detect` folds the **per-spec NDJSON event stream** (via
//!   [`mustard_core::view::projection::read_workspace_events`]) and lists root
//!   specs whose children are all in phase `CLOSE` (and the root itself is
//!   not). Parent→children edges come from `spec.link` events; per-spec phase
//!   comes from the latest `pipeline.phase` event — the same source of truth
//!   the rest of the runtime uses. No dependency on the legacy
//!   `.pipeline-states/*.json` sidecar (which can lag the event stream).
//! - `--epic <name>` folds one such epic: aggregates events for the epic + its
//!   children, emits an `epic.complete` event, writes an `epic-summary`
//!   knowledge entry (markdown), transitions the root to `CLOSE`, and emits
//!   an `epic.fold` tombstone.
//!
//! W4C migration: event aggregation reads per-spec NDJSON via
//! [`mustard_core::EventReader::stream`]; the `epic-summary` knowledge entry
//! is written as `.claude/knowledge/epic-{epic}.md` via
//! [`mustard_core::io::atomic_md::MarkdownStore`].
//!
//! Fail-open and idempotent.

use mustard_core::time::now_iso8601;
use mustard_core::io::atomic_md::frontmatter::Frontmatter;
use mustard_core::io::atomic_md::{MarkdownDoc, MarkdownStore};
use mustard_core::io::fs;
use mustard_core::domain::model::event::HarnessEvent;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};
use std::path::Path;

/// Read every harness event for `spec` from its per-spec NDJSON sink.
fn read_events_for_spec(cwd: &Path, spec: &str) -> Vec<HarnessEvent> {
    let Ok(cp) = ClaudePaths::for_project(cwd) else {
        return Vec::new();
    };
    let Ok(sp) = cp.for_spec(spec) else {
        return Vec::new();
    };
    mustard_core::view::projection::read_harness_events_from_ndjson_dir(&sp.events_dir())
}

/// Append a harness event for the given epic via the NDJSON route. Best-effort.
fn emit_event(project_dir: &str, event: &str, payload: Value, spec: &str) {
    let ts = now_iso8601();
    let sid = crate::shared::context::session_id();
    let kind = crate::shared::events::route::classify_kind(event);
    let _ = crate::shared::events::writer_ndjson::write_event_with_ts(
        Path::new(project_dir),
        Some(spec),
        None,
        &sid,
        event,
        kind,
        Some(0),
        Some(&sid),
        Some("epic-fold"),
        None,
        &payload,
        Some(&ts),
    );
}

/// Latest `pipeline.phase` for `spec` from a pre-folded event slice (UPPERCASE),
/// or empty when the spec never transitioned. Pure over the slice — derived
/// from the `pipeline.phase` event log without per-spec disk reads.
fn phase_from_events(events: &[HarnessEvent], spec: &str) -> String {
    events
        .iter()
        .rev()
        .find(|e| e.event == "pipeline.phase" && e.spec.as_deref() == Some(spec))
        .and_then(|e| e.payload.get("to").and_then(Value::as_str))
        .unwrap_or("")
        .to_uppercase()
}

/// The children of `epic`, reconstructed from `spec.link` events in `events`
/// (`{ parent, child }` payloads), deduplicated and sorted ascending. This is
/// the **single source of truth** for parent→child edges post-W4C: the
/// `.pipeline-states/{epic}.json` `children_specs` array is no longer written
/// ([`crate::commands::spec::spec_link`] emits only the NDJSON event + the
/// `### Parent:` header), so every consumer derives the edge from the stream.
fn children_from_events(events: &[HarnessEvent], epic: &str) -> Vec<String> {
    let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for ev in events {
        if ev.event != "spec.link" {
            continue;
        }
        if ev.payload.get("parent").and_then(Value::as_str) != Some(epic) {
            continue;
        }
        if let Some(child) = ev.payload.get("child").and_then(Value::as_str) {
            set.insert(child.to_string());
        }
    }
    set.into_iter().collect()
}

/// Detect epics ready to fold by folding the **per-spec NDJSON event stream**.
///
/// Parent→children edges are reconstructed from `spec.link` events
/// (`{ parent, child }`); a root is an epic with ≥1 child that is itself never
/// a child of another spec. An epic is "ready" when it is not yet in phase
/// `CLOSE` and **every** child's latest `pipeline.phase` is `CLOSE`. Reads no
/// `.pipeline-states/*.json` sidecar — the event stream is the single source of
/// truth, so detection never lags a freshly-emitted child CLOSE.
pub fn detect_completed_epics(cwd: &Path) -> Vec<String> {
    let mut events = mustard_core::view::projection::read_workspace_events(cwd);
    // Stable sort by ts so the "latest pipeline.phase wins" fold is deterministic
    // across multi-session / multi-file event slices (ISO-8601 lexicographic =
    // chronological for UTC). A stable sort preserves append order on ts ties.
    events.sort_by(|a, b| a.ts.cmp(&b.ts));

    // parent → set(children), and the set of all specs that are someone's child.
    let mut children_of: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut is_child: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for ev in &events {
        if ev.event != "spec.link" {
            continue;
        }
        let parent = ev.payload.get("parent").and_then(Value::as_str);
        let child = ev.payload.get("child").and_then(Value::as_str);
        if let (Some(p), Some(c)) = (parent, child) {
            children_of.entry(p.to_string()).or_default().insert(c.to_string());
            is_child.insert(c.to_string());
        }
    }

    let mut candidates = Vec::new();
    for (parent, children) in &children_of {
        // A root epic is not itself a child of another spec.
        if is_child.contains(parent) {
            continue;
        }
        if children.is_empty() {
            continue;
        }
        // Idempotency: skip when the epic itself is already in CLOSE.
        if phase_from_events(&events, parent) == "CLOSE" {
            continue;
        }
        let all_closed = children
            .iter()
            .all(|child| phase_from_events(&events, child) == "CLOSE");
        if all_closed {
            candidates.push(parent.clone());
        }
    }
    candidates.sort();
    candidates
}

/// Write an `epic-summary` markdown file under `.claude/knowledge/`.
fn write_knowledge_entry(
    cwd: &Path,
    epic: &str,
    name: &str,
    description: &str,
    content: &str,
    children: &[String],
    concluded_at: &str,
) {
    let Ok(cp) = ClaudePaths::for_project(cwd) else {
        return;
    };
    let dir = cp.claude_dir().join("knowledge");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let dest = dir.join(format!("epic-{epic}.md"));
    let mut fm = Map::new();
    fm.insert("kind".into(), json!("epic-summary"));
    fm.insert("name".into(), json!(name));
    fm.insert("confidence".into(), json!(0.85));
    fm.insert("source".into(), json!("epic-fold"));
    fm.insert("concluded_at".into(), json!(concluded_at));
    fm.insert(
        "spec_children".into(),
        json!(children.iter().cloned().collect::<Vec<_>>()),
    );
    fm.insert("status".into(), json!("active"));
    let body = format!("{description}\n\n{content}\n");
    let doc = MarkdownDoc {
        path: dest.clone(),
        frontmatter: Some(Frontmatter(Value::Object(fm))),
        body,
    };
    let _ = MarkdownStore::write_atomic(&dest, &doc);
}

/// Fold an epic — returns `true` on success (or when already folded).
///
/// **Idempotent.** Two guards make a re-run a no-op: an epic already in phase
/// `CLOSE` (idempotency 1) and an `epic.complete` event already present for the
/// epic (idempotency 2). Fully fail-open — a missing pipeline-state returns
/// `false` without erroring. Exposed (F4-c item 3) so `close_orchestrate` can
/// auto-fold a completed epic after the last child closes, without the LLM
/// having to call `epic-fold --epic`.
pub fn fold_epic(cwd: &Path, epic: &str) -> bool {
    if epic.is_empty() {
        eprintln!("[epic-fold] warn: --epic is required");
        return false;
    }
    // Children come from the `spec.link` event stream (the single source of
    // truth post-W4C) — not the `.pipeline-states/{epic}.json` sidecar, which
    // is no longer written. `spec.link` is attributed to the *child* (so it
    // lands in the child's NDJSON sink), so the edge set must be reconstructed
    // from the **workspace-wide** event walk — the same source
    // [`detect_completed_epics`] uses — not the epic's own per-spec sink.
    let workspace_events = mustard_core::view::projection::read_workspace_events(cwd);
    let children: Vec<String> = children_from_events(&workspace_events, epic);
    if children.is_empty() {
        eprintln!("[epic-fold] warn: no spec.link children found for epic \"{epic}\"");
        return false;
    }
    // Aggregate the epic's + each child's per-spec events for the summary fold.
    let mut all_events: Vec<HarnessEvent> = read_events_for_spec(cwd, epic);
    for child in &children {
        all_events.extend(read_events_for_spec(cwd, child));
    }

    // Idempotency 1: root already CLOSE.
    if phase_from_events(&all_events, epic) == "CLOSE" {
        return true;
    }

    // Idempotency 2: an `epic.complete` event already exists for this epic.
    let already_complete = all_events.iter().any(|e| {
        e.event == "epic.complete"
            && e.payload.get("epic").and_then(Value::as_str) == Some(epic)
    });
    if already_complete {
        emit_event(
            cwd.to_string_lossy().as_ref(),
            "pipeline.phase",
            json!({ "from": null, "to": "CLOSE" }),
            epic,
        );
        return true;
    }

    let spec_set: std::collections::BTreeSet<&str> = std::iter::once(epic)
        .chain(children.iter().map(String::as_str))
        .collect();
    let mut findings_count = 0usize;
    let mut decisions_count = 0usize;
    let mut lessons_count = 0usize;
    let mut tool_calls_total = 0usize;
    let mut agents_total = 0usize;
    let mut min_ts: Option<String> = None;
    let mut max_ts: Option<String> = None;
    let mut finding_events: Vec<&HarnessEvent> = Vec::new();

    for ev in &all_events {
        let Some(spec) = ev.spec.as_deref() else {
            continue;
        };
        if !spec_set.contains(spec) {
            continue;
        }
        if !ev.ts.is_empty() {
            if min_ts.as_deref().is_none_or(|m| ev.ts.as_str() < m) {
                min_ts = Some(ev.ts.clone());
            }
            if max_ts.as_deref().is_none_or(|m| ev.ts.as_str() > m) {
                max_ts = Some(ev.ts.clone());
            }
        }
        match ev.event.as_str() {
            "finding" => {
                findings_count += 1;
                finding_events.push(ev);
            }
            "decision" => decisions_count += 1,
            "lesson" => lessons_count += 1,
            "tool.use" => tool_calls_total += 1,
            "agent.start" => agents_total += 1,
            _ => {}
        }
    }

    let started_at = min_ts.clone().unwrap_or_else(now_iso8601);
    let ended_at = max_ts.clone().unwrap_or_else(now_iso8601);
    let duration_ms = match (
        min_ts.as_deref().and_then(mustard_core::time::parse_iso_millis),
        max_ts.as_deref().and_then(mustard_core::time::parse_iso_millis),
    ) {
        (Some(a), Some(b)) => (b - a).max(0),
        _ => 0,
    };

    finding_events.sort_by(|a, b| {
        let ca = a.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        let cb = b.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });
    let top3: Vec<&&HarnessEvent> = finding_events.iter().take(3).collect();

    emit_event(
        cwd.to_string_lossy().as_ref(),
        "epic.complete",
        json!({
            "epic": epic,
            "children": children,
            "findings_count": findings_count,
            "decisions_count": decisions_count,
            "lessons_count": lessons_count,
            "tool_calls_total": tool_calls_total,
            "agents_total": agents_total,
            "duration_ms": duration_ms,
            "started_at": started_at,
            "ended_at": ended_at,
        }),
        epic,
    );

    let finding_lines: Vec<String> = top3
        .iter()
        .enumerate()
        .map(|(i, fev)| {
            let content = fev.payload.get("content").and_then(Value::as_str).unwrap_or("");
            let conf = fev
                .payload
                .get("confidence")
                .and_then(Value::as_f64)
                .map_or_else(|| "?".to_string(), |c| format!("{c:.2}"));
            format!("{}. [conf={conf}] {content}", i + 1)
        })
        .collect();
    let mut content_parts: Vec<String> = Vec::new();
    if !finding_lines.is_empty() {
        content_parts.push(format!("Top findings:\n{}", finding_lines.join("\n")));
    }
    content_parts.push(format!("Decisions: {decisions_count}"));
    content_parts.push(format!("Lessons: {lessons_count}"));

    write_knowledge_entry(
        cwd,
        epic,
        epic,
        &format!(
            "Epic concluded with {} child spec(s): {}",
            children.len(),
            children.join(", ")
        ),
        &content_parts.join("\n\n"),
        &children,
        &ended_at,
    );

    emit_event(
        cwd.to_string_lossy().as_ref(),
        "pipeline.phase",
        json!({ "from": null, "to": "CLOSE" }),
        epic,
    );

    let mut compactable = vec![epic.to_string()];
    compactable.extend(children.iter().cloned());
    emit_event(
        cwd.to_string_lossy().as_ref(),
        "epic.fold",
        json!({
            "epic": epic,
            "compactable_specs": compactable,
            "folded_at": now_iso8601(),
        }),
        epic,
    );
    true
}

/// Dispatch `mustard-rt run epic-fold`.
pub fn run(detect: bool, epic: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    if detect {
        let epics = detect_completed_epics(&cwd);
        let out = json!({ "epics_ready": epics });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string()));
        return;
    }
    if let Some(epic) = epic {
        let ok = fold_epic(&cwd, epic);
        println!("{}", json!({ "ok": ok, "epic": epic }));
        return;
    }
    eprintln!("Usage:");
    eprintln!("  epic-fold --detect");
    eprintln!("  epic-fold --epic <name>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::events::writer_ndjson::write_event;
    use tempfile::tempdir;

    /// Emit a `spec.link` event (parent→child) into the **child's** NDJSON sink
    /// — matching production attribution in `spec_link::emit_link_event`.
    fn link(project: &Path, parent: &str, child: &str) {
        let payload = json!({ "parent": parent, "child": child, "reason": "test" });
        let _ = write_event(
            project, Some(child), None, "s", "spec.link", "spec",
            Some(0), Some("s"), Some("test"), None, &payload,
        );
    }

    /// Emit a `pipeline.phase` transition into `spec`'s NDJSON sink.
    fn phase(project: &Path, spec: &str, to: &str) {
        let payload = json!({ "from": null, "to": to });
        let _ = write_event(
            project, Some(spec), None, "s", "pipeline.phase", "pipeline",
            Some(0), Some("s"), Some("test"), None, &payload,
        );
    }

    #[test]
    fn detect_finds_epic_with_all_children_closed_from_ndjson() {
        let dir = tempdir().unwrap();
        link(dir.path(), "epic", "c1");
        phase(dir.path(), "epic", "EXECUTE");
        phase(dir.path(), "c1", "CLOSE");
        assert_eq!(detect_completed_epics(dir.path()), vec!["epic".to_string()]);
    }

    #[test]
    fn detect_skips_when_a_child_is_not_closed_from_ndjson() {
        let dir = tempdir().unwrap();
        link(dir.path(), "epic", "c1");
        phase(dir.path(), "epic", "EXECUTE");
        phase(dir.path(), "c1", "QA");
        assert!(detect_completed_epics(dir.path()).is_empty());
    }

    #[test]
    fn detect_skips_when_epic_already_closed_from_ndjson() {
        // Idempotency: root already in CLOSE → not re-listed even if children are.
        let dir = tempdir().unwrap();
        link(dir.path(), "epic", "c1");
        phase(dir.path(), "epic", "CLOSE");
        phase(dir.path(), "c1", "CLOSE");
        assert!(detect_completed_epics(dir.path()).is_empty());
    }

    #[test]
    fn detect_uses_latest_phase_event_per_child() {
        // A child that moved QA → CLOSE counts as closed; the newest wins.
        let dir = tempdir().unwrap();
        link(dir.path(), "epic", "c1");
        phase(dir.path(), "epic", "EXECUTE");
        phase(dir.path(), "c1", "QA");
        phase(dir.path(), "c1", "CLOSE");
        assert_eq!(detect_completed_epics(dir.path()), vec!["epic".to_string()]);
    }

    #[test]
    fn fold_missing_epic_returns_false() {
        let dir = tempdir().unwrap();
        assert!(!fold_epic(dir.path(), "ghost"));
    }

    /// An epic with `spec.link` children but never linked → no children derived
    /// from events → fold is a no-op (`false`). Confirms children now come from
    /// the event stream, not a `.pipeline-states` sidecar.
    #[test]
    fn fold_returns_false_when_no_link_children() {
        let dir = tempdir().unwrap();
        // Epic has a phase event but no spec.link → no children.
        phase(dir.path(), "epic", "EXECUTE");
        assert!(!fold_epic(dir.path(), "epic"));
    }

    /// End-to-end (event-sourced children): link a child, transition both, then
    /// fold. The fold succeeds and emits `epic.complete` + the CLOSE phase — all
    /// driven by the NDJSON stream with NO pipeline-state sidecar present.
    #[test]
    fn fold_succeeds_with_event_sourced_children() {
        let dir = tempdir().unwrap();
        link(dir.path(), "epic", "c1");
        phase(dir.path(), "epic", "EXECUTE");
        phase(dir.path(), "c1", "CLOSE");
        // No `.pipeline-states/epic.json` exists.
        if let Ok(cp) = ClaudePaths::for_project(dir.path()) {
            assert!(!cp.pipeline_state_file("epic").exists());
        }
        assert!(fold_epic(dir.path(), "epic"));
        // Idempotent: a second fold is a no-op success (epic now CLOSE).
        assert!(fold_epic(dir.path(), "epic"));
    }
}
