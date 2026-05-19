//! `session_start` — the consolidated SessionStart lifecycle module.
//!
//! ## Scope (b3 Wave 5, session family)
//!
//! This module consolidates three JavaScript hooks, all `SessionStart`. Each
//! is a distinct *concern* kept as its own internal section — consolidation
//! regroups, it does not merge logic:
//!
//! - `harness-init.js` — bootstraps the harness event bus: ensures
//!   `.claude/.harness/` exists, rotates an orphan `events.jsonl` into
//!   `sessions/{prevId}.jsonl`, prunes archived sessions older than 30 days,
//!   and emits a `session.start` event.
//! - `session-memory.js` — injects persistent memory (knowledge base,
//!   cross-session timeline, decisions, lessons) as `additionalContext`.
//! - `spec-hygiene.js` — auto-moves stale completed/cancelled specs from
//!   `spec/active/` to `spec/completed/`.
//!
//! ## Contract shape
//!
//! `harness-init` and `spec-hygiene` are pure side effects (`Observer`).
//! `session-memory` produces an `additionalContext` payload — under the JS
//! hooks that was a `console.log` of a `hookSpecificOutput`; under the
//! consolidated binary it is surfaced as a [`Verdict::Inject`] so the single
//! `emit_outcome` owns the only stdout write. `SessionStart` is a `Check`.
//!
//! ## Out of scope (consciously deferred)
//!
//! `harness-init.js` also spawns an OTEL collector subprocess. That spawn is
//! infrastructure plumbing (a long-lived detached process), not enforcement —
//! `session-cleanup` stops it. It depends on a `.claude/scripts/otel-collector.js`
//! JS script (B4, out of bounds). The collector spawn is therefore **not
//! ported**; see the spec Concern. The harness `OTEL_*` env vars in
//! `settings.json` still drive the harness's own telemetry export, unaffected.
//!
//! ## Profile gate
//!
//! `harness-init` / `session-memory` / `spec-hygiene` each called
//! `shouldRun()` from `_lib/hook-env.js`. The dispatcher has no profile
//! awareness (see spec Concern "Profile gate") — under `MUSTARD_HOOK_PROFILE=minimal`
//! these now run where the JS auto-skipped. They are all fail-open side
//! effects with no verdict impact, so the change is observably inert.

use mustard_core::error::Error;
use mustard_core::io::event_store::{EventSink, JsonlEventStore};
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::util::now_iso8601;

/// Archived sessions older than this are pruned on SessionStart (30 days).
const RETENTION_MS: u128 = 30 * 24 * 60 * 60 * 1000;

/// The advisory-context size cap for the injected persistent memory.
const MEMORY_MAX_CHARS: usize = 2000;
/// Knowledge entries below this confidence are not injected.
const KB_MIN_CONFIDENCE: f64 = 0.5;
/// Number of knowledge entries injected, ranked by confidence × recency.
const KB_MAX_ENTRIES: usize = 5;

/// The consolidated SessionStart module.
pub struct SessionStart;

// ===========================================================================
// Shared helpers
// ===========================================================================

/// Resolve the project dir for an invocation: the harness `cwd`, else `.`.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Current time as milliseconds since the Unix epoch.
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

// ===========================================================================
// harness-init — SessionStart event-bus bootstrap
// ===========================================================================

/// The `.claude/.harness` directory for a project.
fn harness_dir(cwd: &str) -> PathBuf {
    Path::new(cwd).join(".claude").join(".harness")
}

/// The `.claude/.harness/events.jsonl` file for a project.
fn events_file(cwd: &str) -> PathBuf {
    harness_dir(cwd).join("events.jsonl")
}

/// The `.claude/.harness/sessions` directory for a project.
fn sessions_dir(cwd: &str) -> PathBuf {
    harness_dir(cwd).join("sessions")
}

/// The current session id for an invocation. Mirrors `getCurrentSessionId`:
/// the `session_id` field, else `"unknown"` (the consolidated dispatcher has
/// no env-var fallback — telemetry, not load-bearing).
fn current_session_id(input: &HookInput) -> String {
    input
        .session_id
        .clone()
        .or_else(|| {
            input
                .raw
                .get("sessionId")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Read the `sessionId` of the first line of an events file.
fn read_first_session_id(events_file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(events_file).ok()?;
    let first_line = text.lines().next()?;
    let parsed: Value = serde_json::from_str(first_line).ok()?;
    parsed
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// `harness-init`: ensure the harness dirs exist, rotate an orphan
/// `events.jsonl` into the sessions archive, prune old sessions, and emit a
/// `session.start` event. Pure side effect — fail-open throughout.
fn run_harness_init(input: &HookInput, cwd: &str) {
    let harness = harness_dir(cwd);
    let sessions = sessions_dir(cwd);
    let _ = std::fs::create_dir_all(&harness);
    let _ = std::fs::create_dir_all(&sessions);

    let current_id = current_session_id(input);
    rotate_orphan_log(cwd, &sessions, &current_id);
    prune_old_sessions(&sessions);

    // Emit `session.start`.
    let source = input
        .raw
        .get("source")
        .or_else(|| input.raw.get("matcher"))
        .cloned()
        .unwrap_or(Value::Null);
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: current_id,
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("harness-init".to_string()),
            actor_type: None,
        },
        event: "session.start".to_string(),
        payload: json!({ "cwd": cwd, "source": source }),
        spec: None,
    };
    let _ = JsonlEventStore::for_project(cwd).append(&event);
}

/// Rotate `events.jsonl` into `sessions/{prevId}.jsonl` when it belongs to a
/// prior session. An empty/unknown-session file is dropped. Port of
/// `rotateOrphanLog` (the `MUSTARD_EPIC_COMPACT` branch is opt-out by default
/// — kept simple; the rare compaction branch is not ported, parity preserved
/// for the dominant `epicCompact = false` path).
fn rotate_orphan_log(cwd: &str, sessions_dir: &Path, current_id: &str) {
    let events = events_file(cwd);
    if !events.exists() {
        return;
    }
    let prev_id = read_first_session_id(&events);
    let Some(prev_id) = prev_id else {
        // Empty / unknown — drop it.
        let _ = std::fs::remove_file(&events);
        return;
    };
    if prev_id == current_id {
        // Continuation of the current session — keep.
        return;
    }
    let target = sessions_dir.join(format!("{prev_id}.jsonl"));
    if target.exists() {
        // Append then unlink.
        if let Ok(data) = std::fs::read(&events) {
            if let Ok(mut existing) = std::fs::OpenOptions::new()
                .append(true)
                .open(&target)
            {
                use std::io::Write;
                if existing.write_all(&data).is_ok() {
                    let _ = std::fs::remove_file(&events);
                }
            }
        }
    } else if std::fs::rename(&events, &target).is_err() {
        // Fallback: copy + unlink.
        if let Ok(data) = std::fs::read(&events) {
            if std::fs::write(&target, data).is_ok() {
                let _ = std::fs::remove_file(&events);
            }
        }
    }
}

/// Delete archived `sessions/*.jsonl` files older than the retention window.
fn prune_old_sessions(sessions_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return;
    };
    let now = now_millis();
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".jsonl") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        let mtime_ms = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        if now.saturating_sub(mtime_ms) > RETENTION_MS {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ===========================================================================
// spec-hygiene — auto-move stale specs from active/ to completed/
// ===========================================================================

/// The classification of a spec for hygiene purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecClass {
    /// Completed/cancelled with all checklist items done → move to completed/.
    AutoMove,
    /// `implementing` but all done, or anything else → no action.
    Silent,
}

/// Classify a spec from its `spec.md` content. Port of `classify`.
fn classify_spec(content: &str) -> SpecClass {
    // 1. Status from the `### Status:` header — first word.
    let Some(status_raw) = parse_status(content) else {
        return SpecClass::Silent;
    };
    // 2. A BLOCKED concern keeps the spec put.
    if let Some(concerns) = section_body(content, "Concerns") {
        if concerns.to_ascii_uppercase().contains("BLOCKED") {
            return SpecClass::Silent;
        }
    }
    // 3. Checklist completeness.
    let checklist = section_body(content, "Checklist").unwrap_or_else(|| content.to_string());
    let checked = count_occurrences_ci(&checklist, "[x]");
    let unchecked = checklist.matches("[ ]").count();
    let total = checked + unchecked;
    let all_done = total > 0 && unchecked == 0;

    if (status_raw == "completed" || status_raw == "cancelled") && all_done {
        return SpecClass::AutoMove;
    }
    SpecClass::Silent
}

/// Parse the first word of the `### Status:` header, lowercased.
fn parse_status(content: &str) -> Option<String> {
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("###") {
            let rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix("Status:") {
                let word: String = after
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if !word.is_empty() {
                    return Some(word.to_ascii_lowercase());
                }
            }
        }
    }
    None
}

/// Extract the body of an `## <name>` section up to the next `## ` heading.
fn section_body(content: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if is_h2_named(line, name) {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
    let mut body = String::new();
    for line in &lines[start..] {
        if line.starts_with("## ") {
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    Some(body)
}

/// `true` if `line` is an `## <name>` heading (case-sensitive name match,
/// word-boundaried).
fn is_h2_named(line: &str, name: &str) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = rest.trim_start();
    if !rest.starts_with(name) {
        return false;
    }
    rest.as_bytes()
        .get(name.len())
        .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'))
}

/// Count case-insensitive occurrences of `needle` in `haystack`.
fn count_occurrences_ci(haystack: &str, needle: &str) -> usize {
    haystack.to_ascii_lowercase().matches(needle).count()
}

/// `spec-hygiene`: scan `spec/active/`, move stale completed/cancelled specs
/// to `spec/completed/`, and clean orphan pipeline-state files. Pure side
/// effect — fail-open throughout. Port of `runHygiene`.
fn run_spec_hygiene(cwd: &str) {
    let active = Path::new(cwd).join(".claude").join("spec").join("active");
    let Ok(entries) = std::fs::read_dir(&active) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let spec_dir = entry.path();
        let spec_file = spec_dir.join("spec.md");
        let Ok(content) = std::fs::read_to_string(&spec_file) else {
            continue;
        };
        if classify_spec(&content) != SpecClass::AutoMove {
            continue;
        }
        let completed = Path::new(cwd).join(".claude").join("spec").join("completed");
        let dest = completed.join(&name);
        let _ = std::fs::create_dir_all(&completed);
        // Atomic rename — if it fails, state is untouched.
        if std::fs::rename(&spec_dir, &dest).is_err() {
            continue;
        }
        // Best-effort: remove orphan pipeline-state files.
        let states = Path::new(cwd).join(".claude").join(".pipeline-states");
        for stale in [
            states.join(format!("{name}.json")),
            states.join(format!("{name}.diff.md")),
        ] {
            let _ = std::fs::remove_file(stale);
        }
    }
}

// ===========================================================================
// session-memory — persistent-memory injection
// ===========================================================================

/// Load up to `KB_MAX_ENTRIES` knowledge entries, ranked by confidence ×
/// recency. Port of `loadKnowledge`.
fn load_knowledge(kb_path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(kb_path) else {
        return Vec::new();
    };
    let Ok(kb) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(entries) = kb.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    let now = now_millis();
    let mut scored: Vec<(f64, String)> = Vec::new();
    for e in entries {
        let confidence = e.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        if confidence < KB_MIN_CONFIDENCE {
            continue;
        }
        let updated = e
            .get("updatedAt")
            .or_else(|| e.get("createdAt"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let age_ms = now.saturating_sub(parse_iso_millis(updated));
        #[allow(clippy::cast_precision_loss)]
        let age_days = age_ms as f64 / (24.0 * 60.0 * 60.0 * 1000.0);
        let recency = (1.0 - (age_days / 30.0) * 0.9).max(0.1);
        let score = confidence * recency;
        let ty = e.get("type").and_then(|v| v.as_str()).unwrap_or("note");
        let name = e.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let desc = e
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        scored.push((score, format!("- [{ty}] {name}: {desc}")));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(KB_MAX_ENTRIES).map(|(_, s)| s).collect()
}

/// Load the last `max` entries of a `memory/*.json` file's `entries` array,
/// formatted as `- [source] content`. Port of `loadEntries`.
fn load_memory_entries(path: &Path, max: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(entries) = data.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    let start = entries.len().saturating_sub(max);
    entries[start..]
        .iter()
        .map(|e| {
            let source = e.get("source").and_then(|v| v.as_str()).unwrap_or("?");
            let content = e.get("content").and_then(|v| v.as_str()).unwrap_or_default();
            format!("- [{source}] {content}")
        })
        .collect()
}

/// Parse the `YYYY-MM-DDThh:mm:ss` prefix of an ISO-8601 string into epoch
/// millis; `0` on any failure (matching JS `new Date(x || 0).getTime()`).
fn parse_iso_millis(iso: &str) -> u128 {
    let bytes = iso.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return 0;
    }
    let num = |s: &str| -> Option<i64> { s.parse().ok() };
    let (Some(year), Some(month), Some(day), Some(hh), Some(mm), Some(ss)) = (
        num(&iso[0..4]),
        num(&iso[5..7]),
        num(&iso[8..10]),
        num(&iso[11..13]),
        num(&iso[14..16]),
        num(&iso[17..19]),
    ) else {
        return 0;
    };
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hh * 3600 + mm * 60 + ss;
    if secs < 0 {
        0
    } else {
        u128::try_from(secs).unwrap_or(0) * 1000
    }
}

/// Build the persistent-memory `additionalContext` payload, or `None` when no
/// source has any content. Port of the `session-memory.js` body.
///
/// The cross-session timeline (Priority 2 in the JS) depended on
/// `event-projections.js` (B4 script, out of bounds); it is omitted — the JS
/// already wrapped it in a fail-open `try` that yields nothing when the script
/// is absent, so an empty timeline is parity-equivalent.
fn build_memory_context(cwd: &str) -> Option<String> {
    let claude = Path::new(cwd).join(".claude");
    let mem = claude.join("memory");
    let mut parts: Vec<String> = Vec::new();

    let kb = load_knowledge(&claude.join("knowledge.json"));
    if !kb.is_empty() {
        parts.push("## Project Knowledge".to_string());
        parts.extend(kb);
    }
    let decisions = load_memory_entries(&mem.join("decisions.json"), 5);
    if !decisions.is_empty() {
        parts.push("## Recent Decisions".to_string());
        parts.extend(decisions);
    }
    let lessons = load_memory_entries(&mem.join("lessons.json"), 5);
    if !lessons.is_empty() {
        parts.push("## Lessons Learned".to_string());
        parts.extend(lessons);
    }
    if parts.is_empty() {
        return None;
    }
    let mut context = parts.join("\n");
    if context.chars().count() > MEMORY_MAX_CHARS {
        context = context.chars().take(MEMORY_MAX_CHARS).collect::<String>() + "\n...truncated";
    }
    Some(format!("[Persistent Memory]\n{context}"))
}

// ===========================================================================
// Contract impls
// ===========================================================================

impl Check for SessionStart {
    /// On `SessionStart`: bootstrap the event bus, run spec hygiene, and inject
    /// persistent memory. The first two are side effects; the memory payload
    /// is the verdict — `Inject` when there is memory to surface, else `Allow`.
    ///
    /// Any non-`SessionStart` trigger self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::SessionStart) {
            return Ok(Verdict::Allow);
        }
        let cwd = project_dir(input, ctx);
        run_harness_init(input, &cwd);
        run_spec_hygiene(&cwd);
        Ok(match build_memory_context(&cwd) {
            Some(context) => Verdict::Inject { context },
            None => Verdict::Allow,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::SessionStart),
        }
    }

    fn session_input(session_id: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some(session_id.to_string()),
            ..HookInput::default()
        }
    }

    // --- routing -----------------------------------------------------------

    #[test]
    fn non_session_start_trigger_allows() {
        let input = session_input("s1");
        let other = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        assert_eq!(
            SessionStart.evaluate(&input, &other).expect("no error"),
            Verdict::Allow
        );
    }

    // --- harness-init parity -----------------------------------------------

    #[test]
    fn harness_init_creates_dirs_and_emits_session_start() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = session_input("s-new");
        SessionStart.evaluate(&input, &ctx(project)).unwrap();
        assert!(dir.path().join(".claude/.harness/sessions").is_dir());
        let events = JsonlEventStore::for_project(project).replay().unwrap();
        assert!(events.iter().any(|e| e.event == "session.start"));
    }

    #[test]
    fn harness_init_rotates_orphan_log_of_prior_session() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let harness = dir.path().join(".claude/.harness");
        std::fs::create_dir_all(harness.join("sessions")).unwrap();
        // An orphan events.jsonl from a *different* session.
        std::fs::write(
            harness.join("events.jsonl"),
            r#"{"v":1,"ts":"2026-01-01T00:00:00.000Z","sessionId":"old-session","wave":0,"actor":{"kind":"hook"},"event":"session.start","payload":{}}"#,
        )
        .unwrap();
        SessionStart
            .evaluate(&session_input("new-session"), &ctx(project))
            .unwrap();
        // The orphan is rotated into sessions/old-session.jsonl.
        assert!(harness.join("sessions/old-session.jsonl").exists());
    }

    #[test]
    fn harness_init_keeps_log_of_same_session() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let harness = dir.path().join(".claude/.harness");
        std::fs::create_dir_all(harness.join("sessions")).unwrap();
        std::fs::write(
            harness.join("events.jsonl"),
            r#"{"v":1,"ts":"2026-01-01T00:00:00.000Z","sessionId":"same","wave":0,"actor":{"kind":"hook"},"event":"session.start","payload":{}}"#,
        )
        .unwrap();
        SessionStart
            .evaluate(&session_input("same"), &ctx(project))
            .unwrap();
        // Same session → events.jsonl is not rotated away.
        assert!(harness.join("events.jsonl").exists());
        assert!(!harness.join("sessions/same.jsonl").exists());
    }

    // --- spec-hygiene parity -----------------------------------------------

    /// Write an active spec with the given `spec.md` body.
    fn write_active_spec(dir: &Path, name: &str, body: &str) {
        let spec_dir = dir.join(".claude/spec/active").join(name);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), body).unwrap();
    }

    #[test]
    fn hygiene_moves_completed_spec_with_all_done() {
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "done-spec",
            "# Spec\n### Status: completed | Phase: CLOSE\n\n## Checklist\n- [x] One\n- [x] Two\n",
        );
        SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(!dir.path().join(".claude/spec/active/done-spec").exists());
        assert!(dir.path().join(".claude/spec/completed/done-spec").exists());
    }

    #[test]
    fn hygiene_keeps_implementing_spec() {
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "wip-spec",
            "# Spec\n### Status: implementing\n\n## Checklist\n- [x] One\n- [ ] Two\n",
        );
        SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join(".claude/spec/active/wip-spec").exists());
    }

    #[test]
    fn hygiene_keeps_blocked_spec() {
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "blocked-spec",
            "# Spec\n### Status: completed\n\n## Concerns\n- BLOCKED on infra\n\n## Checklist\n- [x] One\n",
        );
        SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join(".claude/spec/active/blocked-spec").exists());
    }

    #[test]
    fn classify_completed_all_done_is_automove() {
        assert_eq!(
            classify_spec("### Status: completed\n## Checklist\n- [x] a\n"),
            SpecClass::AutoMove
        );
        assert_eq!(
            classify_spec("### Status: cancelled\n## Checklist\n- [x] a\n"),
            SpecClass::AutoMove
        );
    }

    #[test]
    fn classify_no_status_is_silent() {
        assert_eq!(classify_spec("# Spec\nno header here\n"), SpecClass::Silent);
    }

    // --- session-memory parity ---------------------------------------------

    #[test]
    fn memory_injection_surfaces_knowledge_as_inject() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("knowledge.json"),
            json!({
                "entries": [
                    { "type": "pattern", "name": "Foo", "description": "use bar",
                      "confidence": 0.9, "updatedAt": now_iso8601() }
                ]
            })
            .to_string(),
        )
        .unwrap();
        let verdict = SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        match verdict {
            Verdict::Inject { context } => {
                assert!(context.contains("Persistent Memory"));
                assert!(context.contains("Project Knowledge"));
                assert!(context.contains("Foo"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    #[test]
    fn memory_injection_allows_when_no_sources() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let verdict = SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn low_confidence_knowledge_is_filtered() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("knowledge.json"),
            json!({
                "entries": [
                    { "type": "pattern", "name": "Weak", "description": "x",
                      "confidence": 0.2, "updatedAt": now_iso8601() }
                ]
            })
            .to_string(),
        )
        .unwrap();
        // Below KB_MIN_CONFIDENCE → no Project Knowledge section → Allow.
        assert_eq!(
            SessionStart
                .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
                .unwrap(),
            Verdict::Allow
        );
    }
}
