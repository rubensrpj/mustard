//! `knowledge` — the consolidated knowledge-extraction module.
//!
//! ## Scope (b3 Wave 5, knowledge family)
//!
//! This module consolidates three JavaScript hooks. Each is a distinct
//! *concern* kept as its own internal section — consolidation regroups, it
//! does not merge logic:
//!
//! - `session-knowledge.js` — `SessionEnd`: extracts friction telemetry from
//!   pipeline-states into `.claude/.metrics/friction.json` and emits one
//!   `retry.attempt` event per measured hook-level retry.
//! - `session-knowledge-inc.js` — `PostToolUse(Task)`: the incremental variant
//!   — throttled, writes friction telemetry for the most recent pipeline-state.
//! - `memory-auto-extract.js` — `SessionEnd`: scans `spec/{name}/spec.md`
//!   for `## Decisions` / `## Lessons` (EN+PT) bullets and persists them as
//!   memory decisions.
//!
//! ## Contract shape
//!
//! All three are pure side effects — no verdict. `Knowledge` is an
//! [`Observer`] only.
//!
//! ## Parity notes
//!
//! - `extractPatternsFromStates` (`_lib/knowledge-extract.js`) is **empty by
//!   design** — friction signals moved out of `knowledge.json`. The JS
//!   `session-knowledge` hooks therefore persist *no* knowledge patterns; they
//!   only write friction telemetry and emit `retry.attempt`. This port
//!   reproduces exactly that: it writes `friction.json` and emits
//!   `retry.attempt`, and does **not** shell out to `memory.js` for knowledge
//!   entries (the JS `toSave` loop runs zero iterations).
//! - `memory-auto-extract.js` shells out to `.claude/scripts/memory.js`
//!   (a B4 script, out of bounds for b3 and still present). This port keeps
//!   that boundary: it invokes `memory.js` exactly as the JS `persist()` does.
//!   When `memory.js` is absent the extraction is a silent no-op — parity with
//!   the JS `if (!fs.existsSync(persistScript)) return false`.

use mustard_core::fs;
use mustard_core::projection::read_harness_events_from_ndjson_dir;
use mustard_core::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::util::now_iso8601;

/// Max memory entries persisted per session — `MAX_ENTRIES_PER_SESSION`.
const MAX_ENTRIES_PER_SESSION: usize = 5;
/// Throttle window for `session-knowledge-inc` — 1 hour, max 3 runs.
const THROTTLE_WINDOW_MS: u128 = 3_600_000;
const THROTTLE_MAX: usize = 3;
/// Friction file keeps at most this many entries.
const FRICTION_MAX_ENTRIES: usize = 100;

/// The consolidated knowledge-extraction module.
pub struct Knowledge;

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

/// The current session id — the `session_id` field, else `"unknown"`.
fn session_id(input: &HookInput) -> String {
    input
        .session_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string())
}

/// Current time as milliseconds since the Unix epoch.
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

/// Parse the `YYYY-MM-DDThh:mm:ss` prefix of an ISO-8601 string into epoch
/// millis; `0` on failure.
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

// ===========================================================================
// Friction extraction — port of _lib/knowledge-extract.js
// ===========================================================================

/// One pipeline-state object, with the filename fallback label attached.
struct StateObject {
    /// `specName`, else the state filename without `.json` — the JS `_file`.
    label: String,
    /// The whole parsed JSON.
    json: Value,
}

/// One friction entry — port of an `extractFrictionFromStates` record.
struct FrictionEntry {
    name: String,
    description: String,
    tags: Vec<String>,
    /// `retryCount` or `apiCalls`, whichever the heuristic produced.
    metric_field: (&'static str, i64),
    prescription: Option<String>,
}

/// Build the actionable prescription string. Port of `derivePrescription`.
fn derive_prescription(metrics: &Value) -> Option<String> {
    let n = |key: &str| -> i64 {
        metrics
            .get("toolBreakdown")
            .and_then(|b| b.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    let bash = n("Bash");
    let edit = n("Edit");
    let write = n("Write");
    let agent = n("Agent");
    let retries = metrics.get("retries").and_then(Value::as_i64).unwrap_or(0);
    let api_calls = metrics.get("apiCalls").and_then(Value::as_i64).unwrap_or(0);

    if bash + edit > 3 * agent && retries > 2 {
        return Some(
            "Next similar pipeline: delegate investigation via Task(general-purpose) \
             BEFORE editing files in sequence. Dominant Bash+Edit without Agent indicates \
             the parent did work that should have been delegated."
                .to_string(),
        );
    }
    if api_calls > 50 && retries > 3 {
        return Some(
            "Next similar pipeline: split into at least 2 smaller pipelines. \
             A single scope with >50 API calls and >3 retries indicates scope-creep."
                .to_string(),
        );
    }
    if edit > 15 && write < 3 {
        return Some(
            "Next similar pipeline: investigate with Read+Grep BEFORE editing. \
             High Edit with low Write count indicates trial-and-error iteration."
                .to_string(),
        );
    }
    None
}

/// Extract friction telemetry from pipeline-state objects. Port of
/// `extractFrictionFromStates`.
fn extract_friction(states: &[StateObject]) -> Vec<FrictionEntry> {
    let mut friction: Vec<FrictionEntry> = Vec::new();
    for state in states {
        let metrics = state.json.get("metrics").cloned().unwrap_or(Value::Null);
        let prescription = derive_prescription(&metrics);
        let retries = metrics.get("retries").and_then(Value::as_i64).unwrap_or(0);
        let api_calls = metrics.get("apiCalls").and_then(Value::as_i64).unwrap_or(0);
        let label = &state.label;

        if retries > 2 {
            let breakdown = metrics
                .get("toolBreakdown")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let mut tags = vec![
                "hook-retry".to_string(),
                "pipeline".to_string(),
                "friction".to_string(),
            ];
            if prescription.is_some() {
                tags.push("prescriptive".to_string());
            }
            friction.push(FrictionEntry {
                name: format!("high-hook-retry-{label}"),
                description: format!(
                    "Pipeline triggered {retries} hook-level retries \
                     (sandbox/stash-pop/re-prompts — not agent redispatches). \
                     Tool breakdown: {breakdown}"
                ),
                tags,
                metric_field: ("retryCount", retries),
                prescription: prescription.clone(),
            });
        }
        if api_calls > 50 {
            let mut tags = vec![
                "optimization".to_string(),
                "pipeline".to_string(),
                "friction".to_string(),
            ];
            if prescription.is_some() {
                tags.push("prescriptive".to_string());
            }
            friction.push(FrictionEntry {
                name: format!("heavy-pipeline-{label}"),
                description: format!(
                    "Pipeline used {api_calls} API calls. Consider splitting into smaller scope."
                ),
                tags,
                metric_field: ("apiCalls", api_calls),
                prescription,
            });
        }
    }
    friction
}

/// Persist friction telemetry to `.claude/.metrics/friction.json`, updating
/// entries in place by `name`. Port of `saveFriction`.
fn save_friction(entries: &[FrictionEntry], claude_dir: &Path) {
    if entries.is_empty() {
        return;
    }
    // Reverse-derive the project root from the passed `claude_dir` so we can
    // route through `ClaudePaths::metrics_dir`. Defensive: a malformed input
    // falls back to a no-op rather than mis-route writes.
    let metrics_dir = claude_dir
        .parent()
        .filter(|_| claude_dir.file_name().and_then(|s| s.to_str()) == Some(".claude"))
        .and_then(|root| ClaudePaths::for_project(root).ok())
        .map(|p| p.metrics_dir());
    let Some(metrics_dir) = metrics_dir else {
        return;
    };
    let _ = fs::create_dir_all(&metrics_dir);
    let friction_path = metrics_dir.join("friction.json");

    let mut store: Value = fs::read_to_string(&friction_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({ "version": 1, "entries": [] }));
    if !store.get("entries").is_some_and(Value::is_array) {
        store = json!({ "version": 1, "entries": [] });
    }
    let ts = now_iso8601();
    let Some(store_entries) = store.get_mut("entries").and_then(Value::as_array_mut) else {
        return;
    };

    for entry in entries {
        let mut record = serde_json::Map::new();
        record.insert("type".into(), json!("friction"));
        record.insert("name".into(), json!(entry.name));
        record.insert("description".into(), json!(entry.description));
        record.insert("source".into(), json!("session-knowledge"));
        record.insert("tags".into(), json!(entry.tags));
        record.insert(entry.metric_field.0.into(), json!(entry.metric_field.1));
        if let Some(p) = &entry.prescription {
            record.insert("prescription".into(), json!(p));
        }
        record.insert("updatedAt".into(), json!(ts));

        // New fields: verification metadata (AC-3).
        record.insert("verifiedAt".to_string(), Value::Null);
        record.insert("sourceFiles".to_string(), Value::Array(Vec::new()));

        let existing_idx = store_entries
            .iter()
            .position(|e| e.get("name").and_then(|n| n.as_str()) == Some(entry.name.as_str()));
        if let Some(idx) = existing_idx {
            let created = store_entries[idx]
                .get("createdAt")
                .and_then(|v| v.as_str())
                .unwrap_or(&ts)
                .to_string();
            record.insert("createdAt".into(), json!(created));
            store_entries[idx] = Value::Object(record);
        } else {
            record.insert("createdAt".into(), json!(ts));
            store_entries.push(Value::Object(record));
        }
    }
    // Keep the newest FRICTION_MAX_ENTRIES.
    store_entries.sort_by(|a, b| {
        let ta = a.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    store_entries.truncate(FRICTION_MAX_ENTRIES);

    let _ = fs::write_atomic(
        &friction_path,
        serde_json::to_string_pretty(&store).unwrap_or_default().as_bytes(),
    );
}

/// Read every `.pipeline-states/*.json` into [`StateObject`]s.
fn read_state_objects(paths: &ClaudePaths) -> Vec<StateObject> {
    let states_dir = paths.pipeline_states_dir();
    let Ok(entries) = fs::read_dir(&states_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries {
        if !std::path::Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&entry.path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let file_label = entry.file_name.trim_end_matches(".json").to_string();
        let label = json
            .get("specName")
            .and_then(|v| v.as_str())
            .map_or(file_label, str::to_string);
        out.push(StateObject { label, json });
    }
    out
}

// ===========================================================================
// session-knowledge — SessionEnd retry.attempt emission + friction
// ===========================================================================

/// `true` when the per-spec NDJSON log already carries a `retry.attempt` event.
///
/// W5: `retry.attempt` lives in the per-spec NDJSON sink, not in `pipeline_events`.
/// Existence-only probe (a single line is enough), so this returns early.
fn spec_has_retry_events(cwd: &str, spec: &str) -> bool {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return false;
    };
    let Ok(spec_paths) = paths.for_spec(spec) else {
        return false;
    };
    let events_dir = spec_paths.events_dir();
    for ev in read_harness_events_from_ndjson_dir(&events_dir) {
        if ev.event == "retry.attempt" {
            return true;
        }
    }
    false
}

/// Emit one `retry.attempt` event per measured hook-level retry. Idempotent:
/// a spec already carrying `retry.attempt` events is skipped. Port of
/// `emitRetryAttempts`.
fn emit_retry_attempts(state: &StateObject, input: &HookInput, cwd: &str) {
    let retries = state
        .json
        .get("metrics")
        .and_then(|m| m.get("retries"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if retries < 1 {
        return;
    }
    let spec = &state.label;
    if spec_has_retry_events(cwd, spec) {
        return;
    }
    for _ in 0..retries {
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: now_iso8601(),
            session_id: session_id(input),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("session-knowledge".to_string()),
                actor_type: None,
            },
            event: "retry.attempt".to_string(),
            payload: json!({ "reason": "hook-level", "tool": Value::Null }),
            spec: Some(spec.clone()),
        };
        // `retry.attempt` is non-pipeline → routed to the per-spec NDJSON
        // sink by the W5 split. `event_route::emit` is the single
        // classifier; see `apps/rt/src/run/event_route.rs`.
        let _ = crate::run::event_route::emit(cwd, &event);
    }
}

/// `session-knowledge`: on `SessionEnd`, write friction telemetry and emit
/// `retry.attempt` events. Pure side effect — fail-open throughout.
fn run_session_knowledge(input: &HookInput, cwd: &str) {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let claude = paths.claude_dir();
    // Bail if memory.js does not exist — parity with the JS bail.
    if !claude.join("scripts").join("memory.js").exists() {
        return;
    }
    let states = read_state_objects(&paths);
    if states.is_empty() {
        return;
    }
    save_friction(&extract_friction(&states), &claude);
    for state in &states {
        emit_retry_attempts(state, input, cwd);
    }
    // NOTE: `extractPatternsFromStates` is empty by design (see module docs),
    // so the JS `toSave` knowledge-persist loop runs zero iterations — no
    // `memory.js knowledge` invocation. This port reproduces that exactly.
}

/// `session-knowledge-inc`: on `PostToolUse(Task)`, write friction telemetry
/// for the most recent pipeline-state, throttled. Pure side effect.
fn run_session_knowledge_inc(cwd: &str) {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let claude = paths.claude_dir();
    if !claude.join("scripts").join("memory.js").exists() {
        return;
    }
    let seen_path = paths.knowledge_seen_path();
    let mut seen: Value = fs::read_to_string(&seen_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({ "_meta": { "recentExtractions": [] } }));

    // Throttle: prune the rolling window, bail when full.
    let now = now_millis();
    let mut recent: Vec<String> = seen
        .get("_meta")
        .and_then(|m| m.get("recentExtractions"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter(|ts| now.saturating_sub(parse_iso_millis(ts)) < THROTTLE_WINDOW_MS)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if recent.len() >= THROTTLE_MAX {
        return;
    }

    // Most-recently modified pipeline-state.
    let states_dir = paths.pipeline_states_dir();
    let Ok(entries) = fs::read_dir(&states_dir) else {
        return;
    };
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in entries {
        if !std::path::Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
            continue;
        }
        let Ok(mtime) = fs::modified(&entry.path) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(t, _)| mtime > *t) {
            newest = Some((mtime, entry.path));
        }
    }
    let Some((_, latest_path)) = newest else {
        return;
    };
    let Ok(text) = fs::read_to_string(&latest_path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let file_label = latest_path
        .file_name()
        .map(|n| n.to_string_lossy().trim_end_matches(".json").to_string())
        .unwrap_or_default();
    let label = json
        .get("specName")
        .and_then(|v| v.as_str())
        .map_or(file_label, str::to_string);
    let state = StateObject { label, json };

    save_friction(&extract_friction(std::slice::from_ref(&state)), &claude);

    // `extractPatternsFromStates` is empty → no eligible pattern to persist,
    // so the JS bails at `if (candidates.length === 0)`. The throttle window
    // is therefore *not* advanced (the JS only records an extraction after a
    // successful persist). This port mirrors that: write nothing to
    // `.knowledge-seen.json` when no pattern was persisted.
    let _ = &mut recent;
    let _ = &mut seen;
}

// ===========================================================================
// memory-auto-extract — SessionEnd decision/lesson extraction
// ===========================================================================

/// One extracted memory item.
struct MemoryItem {
    /// `"decision"` or `"lesson"`.
    item_type: &'static str,
    content: String,
}

/// `true` if `line` opens a Decisions section (EN+PT). Mirrors the JS
/// `^##\s+(?:Decisões não-óbvias|Decisions|Decisões)\b`.
fn is_decisions_heading(line: &str) -> bool {
    h2_named_any(
        line,
        &["decisões não-óbvias", "decisions", "decisões"],
    )
}

/// `true` if `line` opens a Lessons section (EN+PT). Mirrors the JS
/// `^##\s+(?:Lições|Lessons|Lições aprendidas)\b`.
fn is_lessons_heading(line: &str) -> bool {
    h2_named_any(line, &["lições aprendidas", "lições", "lessons"])
}

/// `true` if `line` is an `## <name>` heading for any `name` in `names`
/// (case-insensitive, word-boundaried). Longer names are checked first so
/// `Lições aprendidas` wins over `Lições`.
fn h2_named_any(line: &str, names: &[&str]) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = rest.trim_start().to_ascii_lowercase();
    for name in names {
        if rest.starts_with(name) {
            let boundary_ok = rest
                .as_bytes()
                .get(name.len())
                .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'));
            if boundary_ok {
                return true;
            }
        }
    }
    false
}

/// Extract `## Decisions` / `## Lessons` bullet items from a spec.md body.
/// Port of `extractFromSpec`.
fn extract_memory_items(content: &str) -> Vec<MemoryItem> {
    let mut out: Vec<MemoryItem> = Vec::new();
    let mut active: Option<&'static str> = None;
    for line in content.lines() {
        if line.starts_with("##") && line[2..].starts_with(char::is_whitespace) {
            active = if is_decisions_heading(line) {
                Some("decision")
            } else if is_lessons_heading(line) {
                Some("lesson")
            } else {
                None
            };
            continue;
        }
        let Some(item_type) = active else {
            continue;
        };
        // `^\s*[-*]\s+(.*)$`.
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('-').or_else(|| trimmed.strip_prefix('*')) else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let text = rest.trim();
        if text.is_empty() || text.chars().count() < 8 {
            continue;
        }
        // Skip placeholders.
        let lower = text.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "nenhuma" | "nenhum" | "none" | "n/a" | "tbd" | "todo"
        ) {
            continue;
        }
        out.push(MemoryItem {
            item_type,
            content: text.to_string(),
        });
    }
    out
}

/// Compute a short SHA-256-style hash of a string. The JS uses `crypto`'s
/// SHA-256 truncated to 16 hex chars; matching that exactly would need a hash
/// crate. The idempotency file (`.memory-seen.json`) is a runtime cache that
/// is never compared cross-implementation — a stable FNV-1a 64-bit hash gives
/// the same idempotency guarantee within the Rust port. The hash function only
/// needs to be deterministic and collision-resistant *enough*; correctness of
/// the gate does not depend on matching the JS digest.
fn content_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Shell out to `.claude/scripts/memory.js decision` to persist one item.
/// Port of `persist`. `memory.js` is a B4 script, intentionally still JS.
fn persist_memory(item: &MemoryItem, cwd: &str, source: &str) -> bool {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return false;
    };
    let script = paths.claude_dir().join("scripts").join("memory.js");
    if !script.exists() {
        return false;
    }
    let payload = json!({
        "type": item.item_type,
        "content": item.content,
        "source": source,
        "context": "",
        "cwd": cwd,
    })
    .to_string();

    // The JS uses `process.execPath` (the node/bun runtime). The Rust port has
    // no such handle, so it invokes the runtime by name — `bun` first, `node`
    // as a fallback — matching the shebang the script ships with.
    for runtime in ["bun", "node"] {
        let result = Command::new(runtime)
            .arg(&script)
            .arg("decision")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = result else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(payload.as_bytes());
        }
        if let Ok(status) = child.wait() {
            return status.success();
        }
    }
    false
}

/// `memory-auto-extract`: on `SessionEnd`, scan active specs for Decisions /
/// Lessons bullets and persist them via `memory.js`. Pure side effect.
fn run_memory_auto_extract(cwd: &str) {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let claude = paths.claude_dir();
    let active = paths.spec_dir();
    if !active.exists() {
        return;
    }
    let _ = fs::create_dir_all(claude.join("memory"));

    let seen_path = paths.memory_seen_path();
    let mut seen_hashes: Vec<String> = fs::read_to_string(&seen_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            v.get("hashes")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str()).map(str::to_string).collect())
        })
        .unwrap_or_default();
    let mut seen_set: std::collections::HashSet<String> = seen_hashes.iter().cloned().collect();

    let spec_files = collect_spec_files(&active);
    let mut persisted = 0;
    let mut new_hashes: Vec<String> = Vec::new();

    'outer: for spec_path in spec_files {
        if persisted >= MAX_ENTRIES_PER_SESSION {
            break;
        }
        let spec_name = spec_path
            .parent()
            .and_then(|p| p.strip_prefix(&active).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let Ok(content) = fs::read_to_string(&spec_path) else {
            continue;
        };
        for item in extract_memory_items(&content) {
            if persisted >= MAX_ENTRIES_PER_SESSION {
                break 'outer;
            }
            let hash = content_hash(&format!(
                "{spec_name}|{}|{}",
                item.item_type, item.content
            ));
            if seen_set.contains(&hash) {
                continue;
            }
            if persist_memory(&item, cwd, &format!("spec:{spec_name}")) {
                seen_set.insert(hash.clone());
                new_hashes.push(hash);
                persisted += 1;
            }
        }
    }

    if !new_hashes.is_empty() {
        seen_hashes.extend(new_hashes);
        let start = seen_hashes.len().saturating_sub(500);
        let kept = &seen_hashes[start..];
        let _ = fs::write_atomic(
            &seen_path,
            serde_json::to_string_pretty(&json!({ "hashes": kept })).unwrap_or_default().as_bytes(),
        );
    }
}

/// Recursively collect every `spec.md` file under `dir`.
fn collect_spec_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries {
        if entry.is_dir {
            out.extend(collect_spec_files(&entry.path));
        } else if entry.file_name == "spec.md" {
            out.push(entry.path);
        }
    }
    out
}

// ===========================================================================
// Contract impl
// ===========================================================================

impl Observer for Knowledge {
    /// Dispatch by trigger: `SessionEnd` runs `session-knowledge` +
    /// `memory-auto-extract`; `PostToolUse(Task)` runs `session-knowledge-inc`.
    /// Any other invocation is a no-op. Pure side effect — never panics.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let cwd = project_dir(input, ctx);
        match ctx.trigger {
            Some(Trigger::SessionEnd) => {
                run_session_knowledge(input, &cwd);
                run_memory_auto_extract(&cwd);
            }
            Some(Trigger::PostToolUse) => {
                if matches!(input.tool_name.as_deref(), Some("Task" | "Agent")) {
                    run_session_knowledge_inc(&cwd);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(trigger: Trigger, dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(trigger),
            workspace_root: None,
        }
    }

    /// Write a `.claude/scripts/memory.js` stub so the bail guard passes.
    fn write_memory_stub(dir: &Path) {
        let scripts = dir.join(".claude").join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("memory.js"), "// stub").unwrap();
    }

    /// Write a pipeline-state file.
    fn write_state(dir: &Path, name: &str, state: &Value) {
        let states = dir.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join(format!("{name}.json")), state.to_string()).unwrap();
    }

    // --- derive_prescription parity ----------------------------------------

    #[test]
    fn prescription_detects_l0_violation() {
        let metrics = json!({
            "retries": 3,
            "toolBreakdown": { "Bash": 10, "Edit": 5, "Agent": 1 },
        });
        let p = derive_prescription(&metrics).expect("L0 heuristic must fire");
        assert!(p.contains("delegate"));
    }

    #[test]
    fn prescription_none_for_clean_metrics() {
        let metrics = json!({ "retries": 0, "apiCalls": 3 });
        assert!(derive_prescription(&metrics).is_none());
    }

    // --- friction extraction parity ----------------------------------------

    #[test]
    fn high_retry_state_produces_friction_entry() {
        let state = StateObject {
            label: "demo".to_string(),
            json: json!({ "metrics": { "retries": 5, "toolBreakdown": {} } }),
        };
        let friction = extract_friction(std::slice::from_ref(&state));
        assert_eq!(friction.len(), 1);
        assert_eq!(friction[0].name, "high-hook-retry-demo");
        assert_eq!(friction[0].metric_field, ("retryCount", 5));
    }

    #[test]
    fn heavy_pipeline_state_produces_friction_entry() {
        let state = StateObject {
            label: "big".to_string(),
            json: json!({ "metrics": { "apiCalls": 99 } }),
        };
        let friction = extract_friction(std::slice::from_ref(&state));
        assert_eq!(friction.len(), 1);
        assert_eq!(friction[0].name, "heavy-pipeline-big");
    }

    #[test]
    fn low_activity_state_produces_no_friction() {
        let state = StateObject {
            label: "calm".to_string(),
            json: json!({ "metrics": { "retries": 1, "apiCalls": 10 } }),
        };
        assert!(extract_friction(std::slice::from_ref(&state)).is_empty());
    }

    #[test]
    fn session_knowledge_writes_friction_file() {
        let dir = tempdir().unwrap();
        write_memory_stub(dir.path());
        write_state(
            dir.path(),
            "noisy",
            &json!({ "specName": "noisy", "metrics": { "retries": 4, "toolBreakdown": {} } }),
        );
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            ..HookInput::default()
        };
        Knowledge.observe(&input, &ctx(Trigger::SessionEnd, dir.path().to_str().unwrap()));
        let friction = dir.path().join(".claude/.metrics/friction.json");
        assert!(friction.exists());
        let parsed: Value =
            serde_json::from_str(&std::fs::read_to_string(friction).unwrap()).unwrap();
        assert_eq!(parsed["entries"].as_array().unwrap().len(), 1);
    }

    /// Count `retry.attempt` rows across every per-spec NDJSON dir (W5).
    fn count_retry_events(project: &Path) -> usize {
        let Ok(paths) = ClaudePaths::for_project(project) else {
            return 0;
        };
        let specs_root = paths.spec_dir();
        let Ok(entries) = std::fs::read_dir(&specs_root) else {
            return 0;
        };
        let mut total = 0usize;
        for entry in entries.flatten() {
            let dir = entry.path().join(".events");
            for ev in read_harness_events_from_ndjson_dir(&dir) {
                if ev.event == "retry.attempt" {
                    total += 1;
                }
            }
        }
        total
    }

    #[test]
    fn session_knowledge_emits_retry_attempt_events() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        write_memory_stub(dir.path());
        write_state(
            dir.path(),
            "retried",
            &json!({ "specName": "retried", "metrics": { "retries": 3 } }),
        );
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            session_id: Some("s-1".to_string()),
            ..HookInput::default()
        };
        Knowledge.observe(&input, &ctx(Trigger::SessionEnd, project));
        assert_eq!(
            count_retry_events(dir.path()),
            3,
            "one retry.attempt per measured retry"
        );
    }

    #[test]
    fn retry_attempt_emission_is_idempotent() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        write_memory_stub(dir.path());
        write_state(
            dir.path(),
            "once",
            &json!({ "specName": "once", "metrics": { "retries": 2 } }),
        );
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            ..HookInput::default()
        };
        // Run twice — the second run must not double-count.
        Knowledge.observe(&input, &ctx(Trigger::SessionEnd, project));
        Knowledge.observe(&input, &ctx(Trigger::SessionEnd, project));
        assert_eq!(
            count_retry_events(dir.path()),
            2,
            "idempotent — no re-emission"
        );
    }

    // --- memory-auto-extract parity ----------------------------------------

    #[test]
    fn extract_memory_items_finds_decisions_and_lessons() {
        let content = "# Spec\n\n## Decisions\n\
                       - Use UUIDv7 for all primary keys\n\
                       - nenhuma\n\
                       - x\n\
                       ## Other\n- not extracted here\n\
                       ## Lessons\n- Always run the parity tests first\n";
        let items = extract_memory_items(content);
        // "nenhuma" (placeholder) and "x" (too short) are filtered.
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item_type, "decision");
        assert!(items[0].content.contains("UUIDv7"));
        assert_eq!(items[1].item_type, "lesson");
    }

    #[test]
    fn extract_memory_items_handles_pt_headings() {
        let content = "## Decisões não-óbvias\n\
                       - O parser de spec é idioma-agnóstico\n\
                       ## Lições aprendidas\n\
                       - Contar artefatos antes de extrapolar custo\n";
        let items = extract_memory_items(content);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item_type, "decision");
        assert_eq!(items[1].item_type, "lesson");
    }

    #[test]
    fn memory_extract_is_noop_without_memory_script() {
        // No memory.js stub → persist_memory returns false → nothing breaks.
        let dir = tempdir().unwrap();
        let active = dir.path().join(".claude/spec/demo");
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(
            active.join("spec.md"),
            "## Decisions\n- A real decision that is long enough\n",
        )
        .unwrap();
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            ..HookInput::default()
        };
        // Must not panic; memory-seen.json is not written (nothing persisted).
        Knowledge.observe(&input, &ctx(Trigger::SessionEnd, dir.path().to_str().unwrap()));
        assert!(!dir.path().join(".claude/.cache/memory-seen.json").exists());
    }

    #[test]
    fn content_hash_is_stable() {
        assert_eq!(content_hash("a|decision|b"), content_hash("a|decision|b"));
        assert_ne!(content_hash("a|decision|b"), content_hash("a|lesson|b"));
    }

    // --- routing -----------------------------------------------------------

    #[test]
    fn observe_ignores_unrelated_triggers() {
        let dir = tempdir().unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        // PreToolUse → no-op, must not panic.
        Knowledge.observe(&input, &ctx(Trigger::PreToolUse, dir.path().to_str().unwrap()));
    }

    #[test]
    fn observe_inc_ignores_non_task_post_tool_use() {
        let dir = tempdir().unwrap();
        write_memory_stub(dir.path());
        let input = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("Bash".to_string()),
            ..HookInput::default()
        };
        // PostToolUse(Bash) → session-knowledge-inc must not run.
        Knowledge.observe(&input, &ctx(Trigger::PostToolUse, dir.path().to_str().unwrap()));
    }

    #[test]
    fn knowledge_entry_carries_verification_metadata() {
        let dir = tempdir().unwrap();
        write_memory_stub(dir.path());
        // Write a pipeline-state that produces a friction entry (retries > 2).
        write_state(
            dir.path(),
            "verify-meta",
            &json!({
                "specName": "verify-meta",
                "metrics": { "retries": 4, "toolBreakdown": {} }
            }),
        );
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            ..HookInput::default()
        };
        Knowledge.observe(&input, &ctx(Trigger::SessionEnd, dir.path().to_str().unwrap()));

        let friction_path = dir.path().join(".claude/.metrics/friction.json");
        assert!(friction_path.exists(), "friction.json must be written");
        let parsed: Value =
            serde_json::from_str(&std::fs::read_to_string(&friction_path).unwrap()).unwrap();
        let entries = parsed["entries"].as_array().expect("entries array");
        assert!(!entries.is_empty(), "at least one entry expected");
        let first = &entries[0];
        assert_eq!(first["verifiedAt"], Value::Null, "verifiedAt must default to null");
        assert_eq!(
            first["sourceFiles"],
            Value::Array(Vec::new()),
            "sourceFiles must default to empty array"
        );
    }
}
