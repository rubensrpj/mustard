//! `mustard-rt run complete-spec` — single-stage pipeline close to `completed`.
//!
//! A spec that closes goes **straight to `completed`** — there is no
//! intermediate `closed-followup` grace window. Follow-up work is handled by a
//! separate linked sub-spec (or a future "reopen" action), not a TTL sweep.
//!
//! - **complete** (default): snapshot `affectedFiles` from harness events
//!   (NDJSON) and `git diff`, then emit — all coupled —
//!   `pipeline.status → completed` + `pipeline.complete { closedAt,
//!   affectedFiles }` into the per-spec NDJSON sink AND patch the root
//!   `meta.json` to `Close/Completed`. Idempotent: a second call is a no-op flip
//!   (skipped when the projection already shows `completed`/`cancelled`).
//! - **archive** (`--archive`): an idempotent alias of the single complete —
//!   re-emits `completed` + meta sync, safe to call twice. No filesystem move.
//!
//! All I/O is fail-soft. The stdout JSON line stays shape-compatible with the
//! `/close` command that parses it (the `{ ok, mode, spec, ... }` line).
//!
//! W4C migration: every SQLite reader/writer was removed. Events are written
//! via [`crate::shared::events::writer_ndjson::write_event_with_ts`] and read via
//! [`mustard_core::view::projection::read_harness_events_from_ndjson_dir`].

use crate::shared::context::session_id;
use mustard_core::io::fs;
use mustard_core::view::projection::{read_harness_events_from_ndjson_dir, read_workspace_events};
use mustard_core::ClaudePaths;
use mustard_core::domain::capability::{
    diff_requirements, CapabilityDeclared, EVENT_CAPABILITY_DECLARED, EVENT_CAPABILITY_UPDATE,
};
use mustard_core::domain::model::event::{
    Actor, ActorKind, HarnessEvent, EVENT_PIPELINE_COMPLETE, EVENT_PIPELINE_STATUS,
    PipelineCompletePayload, PipelineStatusPayload, SCHEMA_VERSION,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run the project VCS binary in `cwd`, returning trimmed stdout or `""` on any
/// error. The binary is read from `mustard.json#vcs` (default `git`); callers
/// resolve it once via [`mustard_core::ProjectConfig::vcs`] and thread it
/// here so the spec affected-files diff/log is not hardcoded to `git`.
fn vcs_run(vcs_bin: &str, cwd: &Path, args: &[&str]) -> String {
    Command::new(vcs_bin)
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Resolve the parent (merge-base) branch for `current_branch` from the
/// project's `mustard.json#git.flow` promotion map: the branch's own promotion
/// target, else the wildcard `*` default, else `main`.
fn parent_branch_for(config: &mustard_core::ProjectConfig, current_branch: &str) -> String {
    config
        .git
        .flow
        .get(current_branch)
        .or_else(|| config.git.flow.get("*"))
        .cloned()
        .unwrap_or_else(|| "main".to_string())
}

/// Resolve the per-spec NDJSON `.events/` directory.
fn spec_events_dir(cwd: &Path, spec: &str) -> Option<PathBuf> {
    ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())
}

/// Read every harness event ever written for `spec` from its per-spec NDJSON
/// `.events/` directory.
fn read_events_for_spec(cwd: &Path, spec: &str) -> Vec<mustard_core::domain::model::event::HarnessEvent> {
    let Some(dir) = spec_events_dir(cwd, spec) else {
        return Vec::new();
    };
    read_harness_events_from_ndjson_dir(&dir)
}

/// Collect the files a spec touched: harness `target.file` payloads + the VCS
/// diff against the parent branch.
///
/// Shared by the post-EXECUTE `applied`-edge inference in
/// [`crate::commands::event::emit_phase`]: both "what did this spec touch"
/// callers derive the file set from the same two sources (per-spec NDJSON
/// `target.file` events + a VCS diff vs the parent branch), so the derivation
/// lives here once and is called module-qualified rather than duplicated.
///
/// The VCS binary is read from `mustard.json#vcs` (default `git`). A user who
/// pins `"vcs": ""` opts out of VCS-derived files entirely — only the NDJSON
/// `target.file` events then feed the set. The diff/log invocation shape stays
/// git-style (`rev-parse` + `diff --name-only`); a full multi-VCS abstraction
/// (jj/hg argument variants) is intentionally deferred — `vcs_run` fail-opens to
/// an empty result when the pinned binary does not understand those args, so a
/// non-git VCS degrades to events-only rather than erroring.
pub fn collect_affected_files(cwd: &Path, spec: &str) -> Vec<String> {
    let mut files: BTreeSet<String> = BTreeSet::new();

    // 1. Harness events tagged with this spec (NDJSON).
    for ev in read_events_for_spec(cwd, spec) {
        if let Some(f) = ev
            .payload
            .get("target")
            .and_then(|t| t.get("file"))
            .and_then(Value::as_str)
        {
            if !f.is_empty() {
                files.insert(f.to_string());
            }
        }
    }

    // 2. VCS diff against the parent branch — only when a VCS is configured
    //    (default git; `vcs: ""` is an explicit opt-out → skip this source).
    let config = mustard_core::ProjectConfig::load(cwd);
    if let Some(vcs_bin) = config.vcs() {
        let branch = vcs_run(&vcs_bin, cwd, &["rev-parse", "--abbrev-ref", "HEAD"]);
        if !branch.is_empty() {
            let parent = parent_branch_for(&config, &branch);
            if !parent.is_empty() && branch != parent {
                let range = format!("{parent}...HEAD");
                let diff = vcs_run(&vcs_bin, cwd, &["diff", "--name-only", &range]);
                for f in diff.lines() {
                    let t = f.trim();
                    if !t.is_empty() {
                        files.insert(t.to_string());
                    }
                }
            }
        }
    }

    files.into_iter().collect()
}

/// Emit a typed pipeline event via the canonical NDJSON sink.
fn emit_ndjson(cwd: &Path, spec: &str, event_name: &str, payload: &Value, ts: &str) {
    let sid = session_id();
    let kind = crate::shared::events::route::classify_kind(event_name);
    let _ = crate::shared::events::writer_ndjson::write_event_with_ts(
        cwd,
        Some(spec),
        None,
        &sid,
        event_name,
        kind,
        Some(0),
        Some(&sid),
        Some("complete-spec"),
        None,
        payload,
        Some(ts),
    );
}

/// Terminal complete — mark the spec `completed` by emitting the coupled
/// close transition into the per-spec NDJSON sink AND syncing the root
/// `meta.json`, all in one stage:
///   1. `pipeline.phase: CLOSE` (idempotent)
///   2. `pipeline.status` with `{ from: <current>, to: "completed" }`
///   3. `pipeline.complete` with `{ closedAt, affectedFiles: [...] }`
///   4. `meta.json` → `Close/Completed/CLOSE` (via `patch_meta_complete`)
///
/// Idempotent: skips the whole flip when the projection already shows
/// `completed` or `cancelled` (mirrors the guard the legacy archive stage used),
/// so a second call — or `--archive` after the auto-finalize — is a no-op. This
/// guarantees the event projection and the sidecar never diverge: both end on
/// `completed`.
fn mark_complete(cwd: &Path, spec: &str) -> Value {
    let affected = collect_affected_files(cwd, spec);

    // Read current projection status so we can record `from` and short-circuit
    // an already-terminal spec.
    let events = read_events_for_spec(cwd, spec);
    let current_status = crate::commands::event::event_projections::pipeline_state_from_events(&events, spec, None)
        .and_then(|v| v.status);
    if matches!(current_status.as_deref(), Some("completed" | "cancelled")) {
        return json!({
            "ok": true,
            "mode": "complete",
            "spec": spec,
            "affectedFiles": affected.len(),
        });
    }

    let now = mustard_core::time::now_iso8601();
    let status_payload = serde_json::to_value(PipelineStatusPayload {
        from: current_status,
        to: "completed".to_string(),
    })
    .unwrap_or(Value::Null);
    let complete_payload = serde_json::to_value(PipelineCompletePayload {
        closed_at: Some(now.clone()),
        affected_files: affected.clone(),
    })
    .unwrap_or(Value::Null);

    // Emit phase=CLOSE before flipping the status. Idempotent.
    emit_phase_close(cwd, spec);
    emit_ndjson(cwd, spec, EVENT_PIPELINE_STATUS, &status_payload, &now);
    emit_ndjson(cwd, spec, EVENT_PIPELINE_COMPLETE, &complete_payload, &now);

    // Sync the **root** `meta.json` to `stage=Close, outcome=Completed`. These
    // events go straight through `writer_ndjson` (not through `emit-pipeline
    // run`, which is where the sidecar-sync otherwise lives), so without this
    // call a completed spec would stay stuck at `Plan/Active` in its sidecar
    // while its event log says it is done. Coupling the meta patch here is what
    // keeps `status_word(&Meta)` and `status_word(&SpecState)` consistent.
    // Fail-open: a missing spec dir or write error is a silent no-op.
    crate::commands::event::emit_pipeline::patch_meta_complete(cwd, spec, &now);

    // Merge-on-close: for every `cap.{slug}` linked in the spec's
    // `## Capabilities` section whose doc exists, ensure the spec backlink and
    // emit a `capability.declared` carrying the capability's current state. The
    // capability DOC is the living source — we never parse delta lines.
    // FAIL-OPEN: any capability error here is swallowed; the close already
    // landed above and must succeed regardless.
    merge_capabilities_on_close(cwd, spec, &now);

    // Drop the session→spec binding now that the spec is terminal: events in
    // the gap after the close must not inherit this just-finished spec. Only on
    // the completed close, and only when a real session is resolvable (a
    // `"unknown"` id is skipped inside `unbind_session_spec`).
    let sid = session_id();
    crate::shared::context::unbind_session_spec(&cwd.to_string_lossy(), &sid);

    json!({
        "ok": true,
        "mode": "complete",
        "spec": spec,
        "affectedFiles": affected.len(),
    })
}


// ---------------------------------------------------------------------------
// Merge-on-close — fold the spec's declared capabilities into their docs.
//
// Analogous to OpenSpec's `archive`: when a spec closes, the capabilities it
// authored are folded back into the durable record. The capability DOC under
// `.claude/capabilities/{slug}.md` is the single living source — we read its
// CURRENT full state and re-publish it, never parsing fragile delta lines.
// ---------------------------------------------------------------------------

/// For every `cap.{slug}` linked in `<spec>/spec.md`'s `## Capabilities`
/// section whose `.claude/capabilities/{slug}.md` exists:
///   1. ensure the capability's `specs` backlink includes `spec.{spec}` (load
///      via `capability::parse`, add if absent, dedup + sort, re-render + atomic
///      write ONLY when changed), then
///   2. **compute** the per-requirement change-log by diffing the doc's CURRENT
///      state against the capability's PRIOR declared snapshot
///      ([`diff_requirements`]) and emit one `capability.update` per delta
///      (Added / Modified / Removed) — the change-log is computed, never
///      hand-authored as fragile delta lines, then
///   3. emit `capability.declared` carrying the CURRENT full state, attributed
///      to this spec, so the projection folds it (newest declaration wins).
///
/// Order is updates-then-declared. First declaration (no prior snapshot) ⇒
/// `diff_requirements` returns empty ⇒ only `capability.declared` (creation is
/// carried by the snapshot, not per-requirement Added noise).
///
/// PRIOR state is read ONCE before the loop from the workspace events
/// ([`read_workspace_events`] over `project_capabilities`) — the doc snapshot is
/// authoritative, so the prior `capability.declared` (from whatever spec last
/// declared it) is the baseline. Reading once is correct: the snapshot we emit
/// in this loop for one capability never feeds another capability's `prev`.
///
/// A linked `cap.*` whose doc is missing is skipped + warned (never invents a
/// doc). FAIL-OPEN: every error path is a `continue` / silent no-op — this can
/// never block the close, which has already landed by the time it runs.
fn merge_capabilities_on_close(cwd: &Path, spec: &str, ts: &str) {
    let linked = crate::commands::capability::linked_capability_ids(cwd, spec);
    if linked.is_empty() {
        return; // no `## Capabilities` section (or no `cap.*` links) ⇒ no-op.
    }

    let Ok(paths) = ClaudePaths::for_project(cwd) else {
        return;
    };
    let caps_dir = paths.capabilities_dir();
    let spec_backlink = format!("spec.{spec}");

    // Prior state per capability, read ONCE: the last declared snapshot for each
    // id (doc-faithful baseline for the computed delta). `None` for any id never
    // declared before ⇒ first declaration ⇒ empty delta.
    //
    // Sort by `ts` before folding: `read_workspace_events` returns events in
    // filesystem (read_dir) order across spec dirs, but `project_capabilities`
    // is event-order-faithful ("newest declaration wins"), so the caller must
    // feed chronological order — the same discipline epic_fold / verify_emit use.
    let mut workspace = read_workspace_events(cwd);
    workspace.sort_by(|a, b| a.ts.cmp(&b.ts));
    let prior = mustard_core::view::projection::project_capabilities(&workspace);
    let prior_of = |id: &str| -> Option<mustard_core::domain::capability::Capability> {
        prior
            .capabilities
            .iter()
            .find(|c| c.capability.id == id)
            .map(|c| c.capability.clone())
    };

    for id in linked {
        // `cap.{slug}` → `{slug}` (the doc file stem). A malformed id with no
        // slug after the prefix is skipped.
        let Some(slug) = id.strip_prefix("cap.").map(str::trim).filter(|s| !s.is_empty())
        else {
            continue;
        };
        let doc_path = caps_dir.join(format!("{slug}.md"));
        if !fs::exists(&doc_path) {
            // Linked but missing — do NOT invent a doc; surface a warning.
            eprintln!(
                "[complete-spec] capability {id} linked by spec {spec} has no doc at {} — skipped",
                doc_path.display()
            );
            continue;
        }
        let Ok(md) = fs::read_to_string(&doc_path) else {
            continue; // unreadable ⇒ fail-open skip.
        };

        // The doc is the living source: parse its CURRENT state.
        let mut cap = crate::commands::capability::parse(&md);

        // 1. Ensure the spec backlink (dedup + sorted); re-render ONLY if it
        //    actually changed so an unrelated re-close is byte-stable.
        if !cap.specs.iter().any(|s| s == &spec_backlink) {
            cap.specs.push(spec_backlink.clone());
            cap.specs.sort();
            cap.specs.dedup();
            let body = crate::commands::capability::render(&cap);
            let _ = fs::write_atomic(&doc_path, body.as_bytes());
        }

        // 2. COMPUTE the per-requirement change-log against the prior snapshot
        //    and emit one `capability.update` per delta (updates BEFORE the
        //    declared snapshot). First declaration ⇒ empty ⇒ no updates.
        let prev = prior_of(&id);
        for delta in diff_requirements(prev.as_ref(), &cap) {
            let payload = serde_json::to_value(delta).unwrap_or(Value::Null);
            emit_capability_event(cwd, spec, ts, EVENT_CAPABILITY_UPDATE, payload);
        }

        // 3. Emit `capability.declared` with the capability's current full state,
        //    attributed to this spec. The projection folds it (newest wins), so
        //    the event log + recall reflect the capability as of this close.
        let payload = serde_json::to_value(CapabilityDeclared { capability: cap })
            .unwrap_or(Value::Null);
        emit_capability_event(cwd, spec, ts, EVENT_CAPABILITY_DECLARED, payload);
    }
}

/// Route one `capability.*` event for `spec` through the canonical sink. Shared
/// by the computed `capability.update` deltas and the `capability.declared`
/// snapshot so the envelope (actor / spec attribution / ts) is built once.
/// Fail-open: a routing error is swallowed by the caller's discard.
fn emit_capability_event(cwd: &Path, spec: &str, ts: &str, event_name: &str, payload: Value) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.to_string(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("complete-spec".to_string()),
            actor_type: None,
        },
        event: event_name.to_string(),
        payload,
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(&cwd.to_string_lossy(), &event);
}

/// `--archive` — idempotent alias of the single complete. Re-runs
/// [`mark_complete`] (a no-op flip when the spec is already terminal) and drops
/// any legacy `.pipeline-states/{spec}.json` sidecar. No filesystem move.
fn archive(cwd: &Path, spec: &str) -> (bool, bool) {
    let _ = mark_complete(cwd, spec);
    let states_path = ClaudePaths::for_project(cwd)
        .map(|p| p.pipeline_state_file(spec))
        .unwrap_or_else(|_| cwd.join(format!("{spec}.json")));
    let had_legacy_state = fs::exists(&states_path);
    if had_legacy_state {
        let _ = fs::remove_file(&states_path);
    }
    (true, had_legacy_state)
}

/// Idempotently emit `pipeline.phase: CLOSE` when the spec's latest phase is
/// not already CLOSE.
fn emit_phase_close(cwd: &Path, spec: &str) {
    let last = crate::commands::event::emit_phase::last_phase_for_spec(cwd, spec);
    if last.as_deref() == Some("CLOSE") {
        return;
    }
    let ts = mustard_core::time::now_iso8601();
    let sid = session_id();
    let payload = json!({ "from": last, "to": "CLOSE" });
    let kind = crate::shared::events::route::classify_kind("pipeline.phase");
    let _ = crate::shared::events::writer_ndjson::write_event_with_ts(
        cwd,
        Some(spec),
        None,
        &sid,
        "pipeline.phase",
        kind,
        Some(0),
        Some(&sid),
        Some("complete-spec"),
        None,
        &payload,
        Some(&ts),
    );
}

/// Terminal close for `spec` (→ `completed` + `pipeline.complete` + meta sync)
/// as a reusable, non-printing Rust entry point.
///
/// Mirrors the default `run(...)` path: a fail-open QA pass, the coupled
/// terminal complete via [`mark_complete`], then a fail-open registry rebuild.
/// Returns the complete JSON value (`{ ok, mode: "complete", spec,
/// affectedFiles }`) so callers — e.g.
/// [`crate::commands::pipeline::close_orchestrate`] auto-chaining after every
/// gate passes — can fold it into their own report without spawning a
/// subprocess. Deterministic and idempotent (the underlying emits are
/// idempotent on phase/status and short-circuit an already-terminal spec).
pub(crate) fn run_complete(cwd: &Path, spec: &str) -> Value {
    run_qa_fail_open(cwd, spec);
    finalize(cwd, spec)
}

/// The QA-less tail of [`run_complete`]: the coupled terminal complete
/// ([`mark_complete`]) plus the fail-open registry rebuild. Reused by
/// `close-pipeline`, which has ALREADY run QA itself (and gated on
/// `overall == pass`) — calling `run_complete` there would re-execute every AC
/// command a second time.
pub(crate) fn finalize(cwd: &Path, spec: &str) -> Value {
    let complete_value = mark_complete(cwd, spec);
    rebuild_one_fail_open(cwd, spec);
    complete_value
}

/// Dispatch `mustard-rt run complete-spec`, writing one JSON line to stdout.
///
/// `--archive-stale` / `--archive-followups` are retained as harmless no-ops:
/// the single-stage close no longer produces `closed-followup` specs, so there
/// is nothing for the old TTL sweep to find. Each prints
/// `{ scanned: 0, archived: 0 }` so any caller still passing the flag keeps a
/// shape-compatible JSON line.
pub fn run(spec: Option<&str>, archive_flag: bool, archive_stale: bool, archive_followups_flag: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());

    if archive_stale {
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive-stale", "scanned": 0, "archived": 0 })
        );
        return;
    }
    if archive_followups_flag {
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive-followups", "scanned": 0, "archived": 0 })
        );
        return;
    }

    let Some(spec) = spec else {
        eprintln!(
            "usage: complete-spec <spec-name> [--archive] | --archive-stale | --archive-followups"
        );
        std::process::exit(2);
    };

    if archive_flag {
        run_qa_fail_open(&cwd, spec);
        let (moved_spec, had_state) = archive(&cwd, spec);
        rebuild_one_fail_open(&cwd, spec);
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive", "spec": spec, "movedSpec": moved_spec, "hadState": had_state })
        );
        return;
    }

    let complete_value = run_complete(&cwd, spec);
    println!("{complete_value}");
}

fn rebuild_one_fail_open(cwd: &Path, spec: &str) {
    let project_dir = cwd.to_string_lossy();
    let _ = crate::commands::spec::rebuild_specs::rebuild_one(&project_dir, spec);
}

fn run_qa_fail_open(_cwd: &Path, spec: &str) {
    let outcome = crate::commands::review::qa_run::run_for_spec_with_options(
        spec,
        crate::commands::review::qa_run::QaRunOptions { self_invoked: true },
    );
    eprintln!(
        "[complete-spec] QA: spec={} overall={} passed={}/{}",
        outcome.spec, outcome.overall, outcome.passed, outcome.total,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::capability::{Capability, CapabilityUpdate, Requirement, Scenario};
    use mustard_core::view::projection::project_capabilities;
    use tempfile::tempdir;

    /// Seed `<cwd>/.claude/spec/{spec}/spec.md` with `body`.
    fn seed_spec_md(cwd: &Path, spec: &str, body: &str) {
        let dir = cwd.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), body).unwrap();
    }

    /// Seed `<cwd>/.claude/capabilities/{slug}.md` from a `Capability`.
    fn seed_capability(cwd: &Path, slug: &str, cap: &Capability) {
        let dir = cwd.join(".claude").join("capabilities");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{slug}.md")),
            crate::commands::capability::render(cap),
        )
        .unwrap();
    }

    fn sample_cap(slug: &str) -> Capability {
        Capability {
            id: format!("cap.{slug}"),
            title: "Sample capability".into(),
            status: "active".into(),
            requirements: vec![Requirement {
                statement: "The system SHALL do the thing.".into(),
                scenarios: vec![Scenario {
                    name: "happy".into(),
                    when: "x".into(),
                    then: "y".into(),
                    command: Some("true".into()),
                }],
            }],
            ..Capability::default()
        }
    }

    /// On close, a spec linking `[[cap.x]]` (doc present) gains the `spec.*`
    /// backlink in the capability doc AND emits a `capability.declared` event
    /// the projection folds (newest-declaration-wins) to the current state.
    #[test]
    fn close_merges_linked_capability_backlink_and_declares() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "invoicing-feature";

        seed_capability(cwd, "invoicing", &sample_cap("invoicing"));
        seed_spec_md(
            cwd,
            spec,
            "# Invoicing\n\nNarrative.\n\n## Capabilities\n- [[cap.invoicing]]\n",
        );

        mark_complete(cwd, spec);

        // 1. The capability doc gained the spec backlink (dedup + sorted).
        let cap_md = std::fs::read_to_string(
            cwd.join(".claude").join("capabilities").join("invoicing.md"),
        )
        .unwrap();
        let cap = crate::commands::capability::parse(&cap_md);
        assert_eq!(
            cap.specs,
            vec!["spec.invoicing-feature".to_string()],
            "spec backlink added to the capability doc"
        );

        // 2. A `capability.declared` event landed under the spec; the projection
        //    folds it to the capability's current full state.
        let events = read_events_for_spec(cwd, spec);
        assert!(
            events.iter().any(|e| e.event == EVENT_CAPABILITY_DECLARED),
            "capability.declared emitted on close"
        );
        // First declaration ⇒ NO computed `capability.update` (creation rides on
        // the snapshot, not per-requirement Added noise).
        assert!(
            !events.iter().any(|e| e.event == EVENT_CAPABILITY_UPDATE),
            "first declaration emits no capability.update deltas"
        );
        let rollup = project_capabilities(&events);
        assert_eq!(rollup.capabilities.len(), 1, "projection shows the capability");
        let st = &rollup.capabilities[0];
        assert_eq!(st.capability.id, "cap.invoicing");
        assert_eq!(st.capability.title, "Sample capability");
        assert!(st.history.is_empty(), "no change-log on first declaration");
        // The declared state carries the backlink we just wrote.
        assert_eq!(st.capability.specs, vec!["spec.invoicing-feature".to_string()]);
        // Attribution: the event envelope names the spec.
        let declared = events
            .iter()
            .find(|e| e.event == EVENT_CAPABILITY_DECLARED)
            .unwrap();
        assert_eq!(declared.spec.as_deref(), Some(spec));
    }

    /// End-to-end computed delta convention across three closes against one
    /// living capability doc (the doc is the single source; the change-log is
    /// COMPUTED by diffing on close):
    ///   - spec A declares the cap (R1) → only `capability.declared`, no update.
    ///   - spec B closes after the doc grew to R1+R2 → one
    ///     `capability.update{Added: R2}` + `declared`; projection state = {R1,R2}
    ///     with history ending in [Added R2].
    ///   - spec C closes after the doc dropped R1 → `update{Removed: R1}`;
    ///     projection state = {R2}.
    ///
    /// Drives `merge_capabilities_on_close` with explicit, strictly increasing
    /// timestamps so the prior-snapshot fold (`project_capabilities` is
    /// newest-declaration-wins, hence `ts`-order-sensitive) is deterministic —
    /// `mark_complete`'s wall-clock `ts` would tie across same-millisecond
    /// closes. The full diff → emit → project path is exercised verbatim.
    #[test]
    fn close_computes_capability_delta_against_prior_declaration() {
        use mustard_core::domain::capability::UpdateOp;

        let dir = tempdir().unwrap();
        let cwd = dir.path();

        let req = |s: &str| Requirement { statement: s.into(), scenarios: vec![] };
        let doc_path = cwd.join(".claude").join("capabilities").join("orders.md");
        let updates_in = |spec: &str| -> Vec<(UpdateOp, String)> {
            read_events_for_spec(cwd, spec)
                .into_iter()
                .filter(|e| e.event == EVENT_CAPABILITY_UPDATE)
                .map(|e| {
                    let u: CapabilityUpdate =
                        serde_json::from_value(e.payload).unwrap_or_default();
                    (u.op, u.requirement.statement)
                })
                .collect()
        };
        // Workspace projection over `ts`-sorted events (the same discipline the
        // production close uses) — deterministic newest-declaration-wins.
        let project_workspace = || {
            let mut ws = read_workspace_events(cwd);
            ws.sort_by(|a, b| a.ts.cmp(&b.ts));
            project_capabilities(&ws)
        };

        // --- spec A: first declaration (doc has R1 only) -------------------
        let mut cap = Capability { id: "cap.orders".into(), ..Capability::default() };
        cap.requirements = vec![req("R1")];
        seed_capability(cwd, "orders", &cap);
        seed_spec_md(cwd, "spec-a", "# A\n\n## Capabilities\n- [[cap.orders]]\n");
        merge_capabilities_on_close(cwd, "spec-a", "2026-06-17T00:00:01.000Z");

        assert!(updates_in("spec-a").is_empty(), "first declaration ⇒ no delta");
        assert!(
            read_events_for_spec(cwd, "spec-a")
                .iter()
                .any(|e| e.event == EVENT_CAPABILITY_DECLARED),
            "spec A still emits the declared snapshot (creation)"
        );

        // --- spec B: doc now R1+R2 → Added R2 ------------------------------
        // The doc is the living source: grow it, then close spec B.
        cap = crate::commands::capability::parse(&std::fs::read_to_string(&doc_path).unwrap());
        cap.requirements = vec![req("R1"), req("R2")];
        std::fs::write(&doc_path, crate::commands::capability::render(&cap)).unwrap();
        seed_spec_md(cwd, "spec-b", "# B\n\n## Capabilities\n- [[cap.orders]]\n");
        merge_capabilities_on_close(cwd, "spec-b", "2026-06-17T00:00:02.000Z");

        assert_eq!(
            updates_in("spec-b"),
            vec![(UpdateOp::Added, "R2".to_string())],
            "exactly one Added(R2) delta on spec B"
        );
        // Updates precede the declared snapshot in the event stream.
        let b_events = read_events_for_spec(cwd, "spec-b");
        let upd_pos = b_events.iter().position(|e| e.event == EVENT_CAPABILITY_UPDATE);
        let dec_pos = b_events.iter().position(|e| e.event == EVENT_CAPABILITY_DECLARED);
        assert!(upd_pos < dec_pos, "capability.update emitted before capability.declared");

        // Workspace projection: current state = {R1,R2}; history = [Added R2].
        let st = project_workspace()
            .capabilities
            .into_iter()
            .find(|c| c.capability.id == "cap.orders")
            .expect("capability present");
        let state_reqs: Vec<&str> =
            st.capability.requirements.iter().map(|r| r.statement.as_str()).collect();
        assert_eq!(state_reqs, vec!["R1", "R2"], "state from declared snapshot");
        assert_eq!(st.history.len(), 1, "one change-log entry across all closes so far");
        assert_eq!(st.history[0].op, UpdateOp::Added);
        assert_eq!(st.history[0].requirement.statement, "R2");
        assert_eq!(st.history[0].spec.as_deref(), Some("spec-b"));

        // --- spec C: doc now R2 only → Removed R1 --------------------------
        cap = crate::commands::capability::parse(&std::fs::read_to_string(&doc_path).unwrap());
        cap.requirements = vec![req("R2")];
        std::fs::write(&doc_path, crate::commands::capability::render(&cap)).unwrap();
        seed_spec_md(cwd, "spec-c", "# C\n\n## Capabilities\n- [[cap.orders]]\n");
        merge_capabilities_on_close(cwd, "spec-c", "2026-06-17T00:00:03.000Z");

        assert_eq!(
            updates_in("spec-c"),
            vec![(UpdateOp::Removed, "R1".to_string())],
            "exactly one Removed(R1) delta on spec C"
        );
        // Final state = {R2}; full change-log = [Added R2, Removed R1].
        let st = project_workspace()
            .capabilities
            .into_iter()
            .find(|c| c.capability.id == "cap.orders")
            .expect("capability present");
        let state_reqs: Vec<&str> =
            st.capability.requirements.iter().map(|r| r.statement.as_str()).collect();
        assert_eq!(state_reqs, vec!["R2"], "newest declared snapshot wins");
        let log: Vec<(UpdateOp, &str)> =
            st.history.iter().map(|h| (h.op, h.requirement.statement.as_str())).collect();
        assert_eq!(
            log,
            vec![(UpdateOp::Added, "R2"), (UpdateOp::Removed, "R1")],
            "history is the full computed change-log in event order"
        );
    }

    /// Re-closing a spec whose backlink already exists does NOT rewrite the doc
    /// (byte-stable) but still re-declares (idempotent on the projection).
    #[test]
    fn close_is_idempotent_on_capability_doc() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "idem-feature";

        let mut cap = sample_cap("idem");
        cap.specs = vec!["spec.idem-feature".into()]; // backlink already present.
        seed_capability(cwd, "idem", &cap);
        let before =
            std::fs::read_to_string(cwd.join(".claude").join("capabilities").join("idem.md"))
                .unwrap();
        seed_spec_md(cwd, spec, "# Idem\n\n## Capabilities\n- [[cap.idem]]\n");

        // Call the merge directly (mark_complete short-circuits a 2nd close).
        merge_capabilities_on_close(cwd, spec, "2026-06-17T00:00:00Z");
        let after =
            std::fs::read_to_string(cwd.join(".claude").join("capabilities").join("idem.md"))
                .unwrap();
        assert_eq!(before, after, "no rewrite when the backlink is already present");
    }

    /// A linked-but-missing capability is skipped and the close still succeeds
    /// (no doc invented, no panic, projection has nothing to show).
    #[test]
    fn close_skips_missing_capability_doc_and_still_completes() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "ghost-feature";

        // Link a capability whose doc was never authored.
        seed_spec_md(cwd, spec, "# Ghost\n\n## Capabilities\n- [[cap.ghost]]\n");

        let out = mark_complete(cwd, spec);
        assert_eq!(out.get("ok").and_then(Value::as_bool), Some(true), "close succeeds");

        // No doc was invented.
        assert!(
            !cwd.join(".claude").join("capabilities").join("ghost.md").exists(),
            "missing capability doc must NOT be invented"
        );
        // No capability.declared event for the missing cap.
        let events = read_events_for_spec(cwd, spec);
        assert!(
            !events.iter().any(|e| e.event == EVENT_CAPABILITY_DECLARED),
            "missing capability ⇒ no declared event"
        );
        // The spec still reached `completed`.
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, spec, None,
        )
        .expect("projection exists after close");
        assert_eq!(view.status.as_deref(), Some("completed"));
    }

    /// A spec with no `## Capabilities` section is a clean no-op: no
    /// capability.* events, no error, close completes.
    #[test]
    fn close_with_no_capabilities_section_is_a_noop() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "plain-feature";

        seed_spec_md(cwd, spec, "# Plain\n\nJust narrative, no capabilities.\n");

        let out = mark_complete(cwd, spec);
        assert_eq!(out.get("ok").and_then(Value::as_bool), Some(true));

        let events = read_events_for_spec(cwd, spec);
        assert!(
            !events.iter().any(|e| e.event.starts_with("capability.")),
            "no capabilities section ⇒ no capability.* events"
        );
        assert!(project_capabilities(&events).capabilities.is_empty());
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, spec, None,
        )
        .expect("projection exists after close");
        assert_eq!(view.status.as_deref(), Some("completed"));
    }

    #[test]
    fn parse_iso_millis_round_trips() {
        let ms = mustard_core::time::parse_iso_millis("2026-05-19T00:00:00.000Z").unwrap();
        assert_eq!(ms, 1_779_148_800_000);
    }

    #[test]
    fn parse_iso_millis_without_fraction() {
        assert!(mustard_core::time::parse_iso_millis("2026-05-19T12:30:45Z").is_some());
        assert!(mustard_core::time::parse_iso_millis("garbage").is_none());
    }

    /// The deterministic finalize step (the body of `run_complete`, minus the
    /// fail-open QA / registry side-effects that resolve against the live cwd):
    /// `mark_complete` flips the spec straight to `completed` and emits
    /// `pipeline.complete` + `pipeline.phase: CLOSE` into the per-spec NDJSON.
    /// This is what `close_orchestrate` auto-chains in-process when all gates
    /// pass. Idempotent: a second call leaves the projection on the same status.
    #[test]
    fn mark_complete_emits_close_and_complete_to_ndjson() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();

        let out = mark_complete(cwd, "demo");
        assert_eq!(out.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(out.get("mode").and_then(Value::as_str), Some("complete"));

        // Projection reflects the terminal `completed` status — no
        // `closed-followup` intermediate.
        let events = read_events_for_spec(cwd, "demo");
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, "demo", None,
        )
        .expect("projection exists after mark_complete");
        assert_eq!(view.status.as_deref(), Some("completed"));

        // The pipeline.complete event landed and is verifiable via the same
        // reusable helper close_orchestrate uses for its auto-verify step.
        assert!(crate::commands::event::verify_emit::verify_event_landed(
            cwd,
            "pipeline.complete",
            Some("demo"),
            Some("1h"),
        ));

        // Phase is CLOSE.
        assert_eq!(
            crate::commands::event::emit_phase::last_phase_for_spec(cwd, "demo").as_deref(),
            Some("CLOSE"),
        );

        // Idempotency: a second flip keeps the same terminal status.
        let _ = mark_complete(cwd, "demo");
        let events2 = read_events_for_spec(cwd, "demo");
        let view2 = crate::commands::event::event_projections::pipeline_state_from_events(
            &events2, "demo", None,
        )
        .expect("projection still exists");
        assert_eq!(view2.status.as_deref(), Some("completed"));
    }

    /// No-divergence guarantee: after the auto-finalize, BOTH the event
    /// projection AND `meta.json` say `completed`. This is the bug the
    /// single-stage close fixes — the old `closed-followup` mark left the
    /// projection on `closed-followup` while the (separate) archive stage put
    /// the sidecar on `Completed`, so the two sources disagreed.
    #[test]
    fn mark_complete_keeps_projection_and_meta_consistent() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "demo-consistent";

        // Seed a spec dir whose meta.json is still mid-pipeline (Execute/Active).
        let spec_dir = cwd.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","phase":"EXECUTE","scope":"full","lang":"pt-BR"}"#,
        )
        .unwrap();

        mark_complete(cwd, spec);

        // Event projection → completed.
        let events = read_events_for_spec(cwd, spec);
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, spec, None,
        )
        .expect("projection exists after mark_complete");
        assert_eq!(view.status.as_deref(), Some("completed"));

        // meta.json sidecar → Close/Completed/CLOSE (same verdict, no divergence).
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["stage"], json!("Close"), "{meta}");
        assert_eq!(meta["outcome"], json!("Completed"), "{meta}");
        assert_eq!(meta["phase"], json!("CLOSE"), "{meta}");
        // Other fields preserved.
        assert_eq!(meta["scope"], json!("full"), "{meta}");
        assert_eq!(meta["lang"], json!("pt-BR"), "{meta}");

        // The status word derived from the projection and from the meta agree.
        let meta_outcome = meta["outcome"].as_str().unwrap_or_default();
        assert_eq!(
            view.status.as_deref(),
            Some("completed"),
            "projection status must equal the meta outcome ({meta_outcome})",
        );
        assert_eq!(meta_outcome, "Completed");
    }

    /// FRONT 3 (D-lifecycle): the terminal complete emits the `completed` event
    /// AND syncs the root `meta.json` to `Close/Completed`. Before the fix the
    /// status flowed straight through `writer_ndjson`, leaving a finished spec
    /// stuck at its last `Plan/Active` sidecar value. The meta-sync now lives in
    /// `mark_complete` (the single close stage).
    #[test]
    fn complete_syncs_root_meta() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "demo-complete";

        // Seed a spec dir with a meta.json still in Plan/Active.
        let spec_dir = cwd.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Plan","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR"}"#,
        )
        .unwrap();

        super::mark_complete(cwd, spec);

        // The terminal status event landed.
        let events = read_events_for_spec(cwd, spec);
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, spec, None,
        )
        .expect("projection exists after mark_complete");
        assert_eq!(view.status.as_deref(), Some("completed"));

        // The ROOT meta.json sidecar is now Close/Completed (the bug fix).
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["stage"], json!("Close"), "{meta}");
        assert_eq!(meta["outcome"], json!("Completed"), "{meta}");
        assert_eq!(meta["phase"], json!("CLOSE"), "{meta}");
        // Other fields preserved.
        assert_eq!(meta["scope"], json!("full"), "{meta}");
        assert_eq!(meta["lang"], json!("pt-BR"), "{meta}");
    }
}
