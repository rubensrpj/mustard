//! `close_gate` — the pipeline-CLOSE sensor gate.
//!
//! ## Scope (b3 Wave 4, the real sensor)
//!
//! Ports `close-gate.js` **alone** — the spec calls it out as a real sensor
//! (~645 LOC) that blocks a pipeline CLOSE on a genuine build/lint/test/QA
//! failure. It triggers on a `PreToolUse(Write|Edit)` of a
//! `.claude/.pipeline-states/*.json` file whose content transitions the phase
//! to `CLOSE`, and runs, in order:
//!
//! 1. **Debt-marker gate** — denies if the spec still carries open
//!    `TODO`/`FIXME`/`future hook`/… markers in its actionable sections.
//! 2. **Checklist gate** — denies if the spec's `## Checklist` has unmarked
//!    items.
//! 3. **QA gate (Wave 10)** — denies if no `qa.result` with `overall=pass`
//!    exists in the harness event log.
//! 4. **Build/test gate (Wave 9)** — runs `build → type → lint → test` from
//!    `mustard.json` and denies on the first real (non-env) failure.
//!
//! Each sub-gate has its own `MUSTARD_*_MODE` env var; the dominant default is
//! **`strict`** (unlike the advisory size gates) — this is the exception to
//! Mustard's fail-open hook default, by design.
//!
//! Consolidation here is a 1:1 port — the **verdict must not change**. Parity
//! tests at the bottom mirror `__tests__/harness-wave9.test.js`,
//! `__tests__/harness-wave10.test.js`, and the close-gate block of
//! `__tests__/checklist-mark.test.js`.
//!
//! ## Build-runner note
//!
//! `close-gate.js` distinguishes a *real* sensor failure (non-zero exit →
//! deny) from an *env error* (spawn failure / timeout → fail-open, never
//! deny). `bash_guard::run_build` carries a different shape (and the Wave-2
//! timeout-leak Concern); this module ports `runCommand` faithfully rather
//! than reuse it, so the env-error/real-failure distinction stays exact.

use mustard_core::error::Error;
use mustard_core::io::event_store::EventSink;
use mustard_core::io::sqlite_store::SqliteEventStore;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::util::{format_gate_message, now_iso8601};

/// Per-command timeout for the build/test stages — 5 minutes
/// (`COMMAND_TIMEOUT_MS` in `close-gate.js`).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Truncation budget for a failure-output snippet (`TRUNCATE_CHARS`).
const TRUNCATE_CHARS: usize = 500;

/// The pipeline-CLOSE sensor gate module.
pub struct CloseGate;

// ---------------------------------------------------------------------------
// Mode resolution — each sub-gate, all default `strict`
// ---------------------------------------------------------------------------

/// A three-state gate mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateMode {
    Off,
    Warn,
    Strict,
}

/// Resolve a `MUSTARD_*_MODE` env var, defaulting to `strict` — the close-gate
/// family default. An unrecognised value also falls back to `strict`.
fn resolve_mode(env_var: &str) -> GateMode {
    match std::env::var(env_var)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => GateMode::Off,
        "warn" => GateMode::Warn,
        _ => GateMode::Strict,
    }
}

// ---------------------------------------------------------------------------
// Input parsing
// ---------------------------------------------------------------------------

/// `true` if `file_path` is a pipeline-state file (`.pipeline-states/*.json`).
fn is_pipeline_state_file(file_path: &str) -> bool {
    let p = file_path.replace('\\', "/");
    let Some(idx) = p.find(".pipeline-states/") else {
        return false;
    };
    let rest = &p[idx + ".pipeline-states/".len()..];
    !rest.contains('/') && rest.ends_with(".json")
}

/// Extract the post-write content of a Write/Edit invocation. `Write` uses
/// `content`; `Edit` uses `new_string` (the JS `extractContent`).
fn extract_content(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    if let Some(c) = ti.get("content").and_then(|v| v.as_str()) {
        return Some(c.to_string());
    }
    if let Some(c) = ti.get("new_string").and_then(|v| v.as_str()) {
        return Some(c.to_string());
    }
    None
}

/// The `file_path` of a Write/Edit invocation.
fn file_path_of(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The uppercased phase from a pipeline-state JSON string. Reads `phaseName`
/// (string) then a legacy string `phase` (the JS `extractPhase`).
fn extract_phase(content: &str) -> Option<String> {
    let obj: Value = serde_json::from_str(content).ok()?;
    let raw = obj
        .get("phaseName")
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("phase").and_then(|v| v.as_str()))?;
    Some(raw.to_ascii_uppercase())
}

/// The spec name from a pipeline-state JSON string (`spec` then `specName`).
fn extract_spec(content: &str) -> Option<String> {
    let obj: Value = serde_json::from_str(content).ok()?;
    obj.get("spec")
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("specName").and_then(|v| v.as_str()))
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Debt-marker gate
// ---------------------------------------------------------------------------

/// One debt marker found in a spec.
#[derive(Debug)]
struct DebtMarker {
    line: usize,
    snippet: String,
    pattern: &'static str,
}

/// Scan the active spec for debt markers inside its actionable sections
/// (Tasks / Checklist / Acceptance Criteria, EN+PT). Port of `findDebtMarkers`.
fn find_debt_markers(cwd: &str, spec: Option<&str>) -> Vec<DebtMarker> {
    let Some(spec) = spec else {
        return Vec::new();
    };
    let spec_path = Path::new(cwd)
        .join(".claude")
        .join("spec")
        .join("active")
        .join(spec)
        .join("spec.md");
    let Ok(raw) = std::fs::read_to_string(&spec_path) else {
        return Vec::new();
    };

    let mut markers: Vec<DebtMarker> = Vec::new();
    let mut in_fence = false;
    let mut in_actionable = false;

    for (i, line) in raw.split('\n').enumerate() {
        if line.trim().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Any `## ` H2 heading toggles the actionable scope.
        if is_h2(line) {
            in_actionable = is_actionable_heading(line);
            continue;
        }
        if !in_actionable {
            continue;
        }
        // Strip inline `code` spans — markers inside backticks are examples.
        let cleaned = strip_inline_code(line);
        if let Some(pattern) = debt_pattern_match(&cleaned) {
            let snippet: String = line.trim().chars().take(140).collect();
            markers.push(DebtMarker {
                line: i + 1,
                snippet,
                pattern,
            });
        }
    }
    markers
}

/// `true` if `line` is a `## ` H2 heading.
fn is_h2(line: &str) -> bool {
    line.starts_with("## ") && line.len() > 3 && !line.as_bytes()[3].is_ascii_whitespace()
}

/// `true` if `line` is an actionable H2 heading (Tasks / Checklist /
/// Acceptance Criteria, EN+PT). Port of `isActionableHeading`.
fn is_actionable_heading(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    h2_named(&lower, "tasks")
        || h2_named(&lower, "checklist")
        || h2_named(&lower, "tarefas")
        || h2_named(&lower, "acceptance criteria")
        || h2_named(&lower, "critérios de aceitação")
}

/// `true` if a lowercased line is an H2 whose name is exactly `name`.
fn h2_named(lower: &str, name: &str) -> bool {
    let Some(rest) = lower.strip_prefix("## ") else {
        return false;
    };
    let rest = rest.trim_start();
    if !rest.starts_with(name) {
        return false;
    }
    rest.as_bytes()
        .get(name.len())
        .is_none_or(|&b| !b.is_ascii_alphanumeric() && b != b'_')
}

/// Strip inline backtick `code` spans from a line.
fn strip_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for c in line.chars() {
        if c == '`' {
            in_code = !in_code;
            continue;
        }
        if !in_code {
            out.push(c);
        }
    }
    out
}

/// The debt-marker label for a cleaned line, if any. Port of the `PATTERNS`
/// table in `findDebtMarkers`.
fn debt_pattern_match(cleaned: &str) -> Option<&'static str> {
    let lower = cleaned.to_ascii_lowercase();
    // `\bfuture\s+hook\b`.
    if has_word_pair(&lower, "future", "hook") {
        return Some("future-hook");
    }
    // `\bnot\s+part\s+of\s+(?:this\s+)?wave\s*\d*\b`.
    if has_not_part_of_wave(&lower) {
        return Some("not-part-of-wave");
    }
    // `\bnot\s+yet\s+implemented\b`.
    if has_word_triple(&lower, "not", "yet", "implemented") {
        return Some("not-yet-implemented");
    }
    // `\bTODO:[^\s]*\s+\S`, `\bFIXME:...`, `\bXXX:...`.
    for (token, label) in [("todo:", "TODO"), ("fixme:", "FIXME"), ("xxx:", "XXX")] {
        if has_marker_with_content(&lower, token) {
            return Some(label);
        }
    }
    None
}

/// `true` if `s` (lowercased) matches `\bA\s+B\b`.
fn has_word_pair(s: &str, a: &str, b: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = s[from..].find(a) {
        let start = from + rel;
        let end = start + a.len();
        let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
        let rest = &s[end..];
        let trimmed = rest.trim_start();
        let had_ws = trimmed.len() < rest.len();
        if left_ok
            && had_ws
            && trimmed.starts_with(b)
            && trimmed
                .as_bytes()
                .get(b.len())
                .is_none_or(|&c| !is_word_byte(c))
        {
            return true;
        }
        from = end;
    }
    false
}

/// `true` for `\bA\s+B\s+C\b`.
fn has_word_triple(s: &str, a: &str, b: &str, c: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = s[from..].find(a) {
        let start = from + rel;
        let end = start + a.len();
        let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
        if left_ok {
            let rest = &s[end..];
            let after_a = rest.trim_start();
            if after_a.len() < rest.len() && after_a.starts_with(b) {
                let after_b = &after_a[b.len()..];
                let after_b_trim = after_b.trim_start();
                if after_b_trim.len() < after_b.len()
                    && after_b_trim.starts_with(c)
                    && after_b_trim
                        .as_bytes()
                        .get(c.len())
                        .is_none_or(|&x| !is_word_byte(x))
                {
                    return true;
                }
            }
        }
        from = end;
    }
    false
}

/// `true` for `\bnot\s+part\s+of\s+(?:this\s+)?wave\s*\d*\b`.
fn has_not_part_of_wave(s: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = s[from..].find("not") {
        let start = from + rel;
        let end = start + 3;
        let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
        if left_ok {
            let mut cursor = &s[end..];
            let consume_word = |cur: &str, w: &str| -> Option<usize> {
                let trimmed = cur.trim_start();
                if trimmed.len() < cur.len() && trimmed.starts_with(w) {
                    Some(cur.len() - trimmed.len() + w.len())
                } else {
                    None
                }
            };
            if let Some(n) = consume_word(cursor, "part") {
                cursor = &cursor[n..];
                if let Some(n) = consume_word(cursor, "of") {
                    cursor = &cursor[n..];
                    // optional `this`.
                    if let Some(n) = consume_word(cursor, "this") {
                        cursor = &cursor[n..];
                    }
                    if let Some(n) = consume_word(cursor, "wave") {
                        cursor = &cursor[n..];
                        // `\s*\d*\b` — already at a word boundary after `wave`.
                        let _ = cursor;
                        return true;
                    }
                }
            }
        }
        from = end;
    }
    false
}

/// `true` for a `\bTOKEN[^\s]*\s+\S` marker — `TOKEN` is `todo:`/`fixme:`/`xxx:`,
/// followed (after optional non-space) by whitespace then a non-space char.
fn has_marker_with_content(lower: &str, token: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = lower[from..].find(token) {
        let start = from + rel;
        let end = start + token.len();
        let left_ok = start == 0 || !is_word_byte(lower.as_bytes()[start - 1]);
        if left_ok {
            // `[^\s]*` — optional run of non-whitespace.
            let rest = &lower[end..];
            let non_ws_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let after = &rest[non_ws_len..];
            // `\s+\S` — at least one whitespace then a non-space char.
            let trimmed = after.trim_start();
            if trimmed.len() < after.len() && !trimmed.is_empty() {
                return true;
            }
        }
        from = end;
    }
    false
}

/// `true` for an ASCII word byte.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------------------------------------------------------------------------
// Checklist gate
// ---------------------------------------------------------------------------

/// The unmarked `## Checklist` items of the active spec. Returns `(found,
/// unmarked)` — `found=false` means the spec or section is absent (skip).
/// Port of `findUnmarkedChecklistItems`.
fn find_unmarked_checklist(cwd: &str, spec: Option<&str>) -> (bool, Vec<String>) {
    let Some(spec) = spec else {
        return (false, Vec::new());
    };
    let spec_path = Path::new(cwd)
        .join(".claude")
        .join("spec")
        .join("active")
        .join(spec)
        .join("spec.md");
    let Ok(raw) = std::fs::read_to_string(&spec_path) else {
        return (false, Vec::new());
    };
    let lines: Vec<&str> = raw.split('\n').collect();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if is_checklist_heading(line) {
            start = Some(i + 1);
            break;
        }
    }
    let Some(start) = start else {
        return (false, Vec::new());
    };
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start) {
        if line.starts_with("## ") || *line == "##" {
            end = i;
            break;
        }
    }
    let mut unmarked: Vec<String> = Vec::new();
    for line in &lines[start..end] {
        if let Some(text) = unchecked_item_text(line) {
            unmarked.push(text);
        }
    }
    (true, unmarked)
}

/// `true` if `line` is the `## Checklist` heading.
fn is_checklist_heading(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = rest.trim_start();
    rest.starts_with("Checklist")
        && rest
            .as_bytes()
            .get("Checklist".len())
            .is_none_or(|&b| !is_word_byte(b))
}

/// The trimmed text of an unchecked `- [ ] <text>` item, if `line` is one.
fn unchecked_item_text(line: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = t.strip_prefix('-')?;
    let rest_trim = rest.trim_start();
    if rest_trim.len() == rest.len() {
        return None; // `-` must be followed by whitespace
    }
    let rest = rest_trim.strip_prefix("[ ]")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim().to_string())
}

// ---------------------------------------------------------------------------
// QA gate
// ---------------------------------------------------------------------------

/// The last `qa.result` for a spec. Returns `(found, overall, failed_count)`.
/// Port of `findLastQAResult` — a single replay over the SQLite harness store.
fn find_last_qa_result(cwd: &str, spec: Option<&str>) -> (bool, Option<String>, usize) {
    let events = SqliteEventStore::for_project(cwd)
        .and_then(|store| store.replay())
        .unwrap_or_default();
    let mut last: Option<HarnessEvent> = None;
    for ev in events {
        if ev.event != "qa.result" {
            continue;
        }
        // Filter by spec when one is known and the event carries one.
        if let Some(spec) = spec {
            if let Some(ev_spec) = ev.payload.get("spec").and_then(|v| v.as_str()) {
                if ev_spec != spec {
                    continue;
                }
            }
        }
        last = Some(ev);
    }
    let Some(last) = last else {
        return (false, None, 0);
    };
    let overall = last
        .payload
        .get("overall")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let failed_count = last
        .payload
        .get("criteria")
        .and_then(Value::as_array)
        .map_or(0, |arr| {
            arr.iter()
                .filter(|c| c.get("status").and_then(|v| v.as_str()) == Some("fail"))
                .count()
        });
    (true, overall, failed_count)
}

// ---------------------------------------------------------------------------
// Build/test gate
// ---------------------------------------------------------------------------

/// The build/test/lint/type commands from `mustard.json`.
struct MustardCommands {
    build: Option<String>,
    type_check: Option<String>,
    lint: Option<String>,
    test: Option<String>,
}

/// Read the command fields from `mustard.json`. `None` when the file is absent
/// or unreadable (the JS `readMustardCommands` returns `null`).
fn read_mustard_commands(cwd: &str) -> Option<MustardCommands> {
    let text = std::fs::read_to_string(Path::new(cwd).join("mustard.json")).ok()?;
    let cfg: Value = serde_json::from_str(&text).ok()?;
    let field = |k: &str| {
        cfg.get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(MustardCommands {
        build: field("buildCommand"),
        type_check: field("typeCheckCommand"),
        lint: field("lintCommand"),
        test: field("testCommand"),
    })
}

/// The outcome of a single stage command.
struct CommandResult {
    ok: bool,
    /// `true` for an env/hook bug (spawn failure, timeout, empty command) —
    /// the JS `envError`. An env error never blocks (fail-open).
    env_error: bool,
    output: String,
}

/// Run a single stage command via the system shell, under [`COMMAND_TIMEOUT`].
/// Port of `runCommand`: a non-zero exit is a real failure; a spawn failure or
/// a timeout is an env error.
fn run_command(cmd: &str, cwd: &str) -> CommandResult {
    if cmd.trim().is_empty() {
        return CommandResult {
            ok: false,
            env_error: true,
            output: "empty command".to_string(),
        };
    }
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/c", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    command
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return CommandResult {
                ok: false,
                env_error: true,
                output: err.to_string(),
            };
        }
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    std::thread::spawn(move || {
        let status = child.wait();
        let _ = tx.send((status, child));
    });

    match rx.recv_timeout(COMMAND_TIMEOUT) {
        Ok((Ok(status), _child)) => {
            let mut output = String::new();
            if let Some(mut out) = stdout {
                use std::io::Read;
                let _ = out.read_to_string(&mut output);
            }
            if let Some(mut err) = stderr {
                use std::io::Read;
                let _ = err.read_to_string(&mut output);
            }
            if status.success() {
                CommandResult {
                    ok: true,
                    env_error: false,
                    output: String::new(),
                }
            } else {
                CommandResult {
                    ok: false,
                    env_error: false,
                    output: output.trim().to_string(),
                }
            }
        }
        // Wait itself failed → env error.
        Ok((Err(err), _child)) => CommandResult {
            ok: false,
            env_error: true,
            output: err.to_string(),
        },
        // Timed out → env error (fail-open, the JS `status === null` branch).
        Err(_) => CommandResult {
            ok: false,
            env_error: true,
            output: format!("[timeout after {}ms] {cmd}", COMMAND_TIMEOUT.as_millis()),
        },
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Emit a `close-gate.check` harness event. Best-effort telemetry.
fn emit_close_gate_event(cwd: &str, payload: Value) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: "unknown".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("close-gate".to_string()),
            actor_type: None,
        },
        event: "close-gate.check".to_string(),
        payload,
        spec: None,
    };
    let _ = SqliteEventStore::for_project(cwd).and_then(|store| store.append(&event));
}

/// Truncate a string to `max` bytes (char-boundary safe).
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// The resolved mode of every close-gate sub-gate.
///
/// Resolving the four `MUSTARD_*_MODE` env vars once, up front, keeps
/// [`close_gate_with_modes`] a pure function — testable without mutating
/// process-global environment (which the crate's `#![forbid(unsafe_code)]`
/// would otherwise force into an `unsafe` block).
#[derive(Debug, Clone, Copy)]
struct CloseGateModes {
    close: GateMode,
    debt: GateMode,
    checklist: GateMode,
    qa: GateMode,
}

impl CloseGateModes {
    /// Resolve every sub-gate mode from the environment — the production path.
    fn from_env() -> Self {
        Self {
            close: resolve_mode("MUSTARD_CLOSE_GATE_MODE"),
            debt: resolve_mode("MUSTARD_DEBT_GATE_MODE"),
            checklist: resolve_mode("MUSTARD_CHECKLIST_GATE_MODE"),
            qa: resolve_mode("MUSTARD_QA_GATE_MODE"),
        }
    }
}

/// Run the full close-gate against a `PreToolUse(Write|Edit)` invocation,
/// resolving every sub-gate mode from the environment.
///
/// Returns the verdict — 1:1 with `close-gate.js`. Every JS `process.exit(0)`
/// with no stdout maps to `Allow`; a `permissionDecision: deny` maps to `Deny`.
fn close_gate(input: &HookInput, cwd: &str) -> Verdict {
    close_gate_with_modes(input, cwd, CloseGateModes::from_env())
}

/// The pure close-gate body — every `MUSTARD_*_MODE` is supplied via `modes`
/// rather than read from the environment, so it is exercised directly by the
/// parity tests.
fn close_gate_with_modes(input: &HookInput, cwd: &str, modes: CloseGateModes) -> Verdict {
    let mode = modes.close;
    if mode == GateMode::Off {
        return Verdict::Allow;
    }
    let Some(file_path) = file_path_of(input) else {
        return Verdict::Allow;
    };
    if !is_pipeline_state_file(&file_path) {
        return Verdict::Allow;
    }
    let Some(content) = extract_content(input) else {
        return Verdict::Allow;
    };
    // Only trigger on a transition to phase CLOSE.
    //
    // Post-`pipeline.phase` migration the canonical phase lives in the SQLite
    // event store, not the pipeline-state JSON — SKILL.md no longer writes
    // `phaseName`. This branch is kept defensively for any legacy state file
    // that still carries `phaseName: "CLOSE"`; in steady state the real CLOSE
    // gate runs inline in `mustard-rt run emit-phase --to CLOSE`.
    if extract_phase(&content).as_deref() != Some("CLOSE") {
        return Verdict::Allow;
    }
    let spec_name = extract_spec(&content);
    let spec_ref = spec_name.as_deref();
    run_close_gates(cwd, spec_ref, modes)
}

/// Run every close-gate sub-gate against an already-resolved `(cwd, spec)`
/// pair — the spec-aware entry point used by `mustard-rt run emit-phase --to
/// CLOSE`. No JSON dependency, no HookInput coupling.
///
/// Returns:
/// - [`Verdict::Allow`] when every gate passes (or every gate is in `off`).
/// - [`Verdict::Deny`] when any strict gate fires.
/// - [`Verdict::Warn`] only for the build/test gate in `warn` mode; the
///   debt/checklist/qa gates degrade to `Allow` in `warn`.
fn run_close_gates(cwd: &str, spec_ref: Option<&str>, modes: CloseGateModes) -> Verdict {
    let mode = modes.close;

    // ── Debt-marker gate ──────────────────────────────────────────────────
    let debt_mode = modes.debt;
    if debt_mode != GateMode::Off {
        let markers = find_debt_markers(cwd, spec_ref);
        if !markers.is_empty() {
            let top = markers
                .iter()
                .take(5)
                .map(|m| format!("  - line {} ({}): {}", m.line, m.pattern, m.snippet))
                .collect::<Vec<_>>()
                .join("\n");
            let more = if markers.len() > 5 {
                format!("\n  …and {} more", markers.len() - 5)
            } else {
                String::new()
            };
            let reason = format!(
                "{}\n{top}{more}",
                format_gate_message(
                    "Close Gate",
                    &format!(
                        "spec \"{}\" still contains {} debt marker(s)",
                        spec_ref.unwrap_or(""),
                        markers.len()
                    ),
                    "closing a spec with open TODO/FIXME hides unfinished work",
                    "resolve them or move to a follow-up spec, or set \
                     MUSTARD_DEBT_GATE_MODE=warn",
                )
            );
            if debt_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    json!({
                        "result": "deny-debt-markers",
                        "mode": mode_str(mode),
                        "debtMode": mode_str(debt_mode),
                        "spec": spec_ref,
                        "markerCount": markers.len(),
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through.
        }
    }

    // ── Checklist gate ────────────────────────────────────────────────────
    let checklist_mode = modes.checklist;
    if checklist_mode != GateMode::Off {
        let (found, unmarked) = find_unmarked_checklist(cwd, spec_ref);
        if found && !unmarked.is_empty() {
            let preview = unmarked
                .iter()
                .take(5)
                .map(|t| format!("  - {t}"))
                .collect::<Vec<_>>()
                .join("\n");
            let more = if unmarked.len() > 5 {
                format!("\n  …and {} more", unmarked.len() - 5)
            } else {
                String::new()
            };
            let reason = format!(
                "{}\n{preview}{more}",
                format_gate_message(
                    "Close Gate",
                    &format!(
                        "checklist has {} unmarked item(s) for spec \"{}\"",
                        unmarked.len(),
                        spec_ref.unwrap_or("")
                    ),
                    "an incomplete checklist means the spec is not done",
                    &format!(
                        "mark each via `bun .claude/scripts/mark-checklist-item.js \
                         --spec {} --item \"<text>\"`, or set \
                         MUSTARD_CHECKLIST_GATE_MODE=warn",
                        spec_ref.unwrap_or("")
                    ),
                )
            );
            if checklist_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    json!({
                        "result": "deny-checklist-unmarked",
                        "mode": mode_str(mode),
                        "checklistMode": mode_str(checklist_mode),
                        "spec": spec_ref,
                        "unmarkedCount": unmarked.len(),
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through.
        }
    }

    // ── QA gate (Wave 10) ─────────────────────────────────────────────────
    let qa_mode = modes.qa;
    if qa_mode != GateMode::Off {
        let (found, overall, failed_count) = find_last_qa_result(cwd, spec_ref);
        if !found {
            let reason = format_gate_message(
                "Close Gate",
                &spec_ref.map_or_else(
                    || "no QA pass recorded".to_string(),
                    |s| format!("no QA pass recorded for spec \"{s}\""),
                ),
                "CLOSE requires the acceptance criteria to be verified",
                &spec_ref.map_or_else(
                    || "run /mustard:qa before closing, or set MUSTARD_QA_GATE_MODE=warn"
                        .to_string(),
                    |s| {
                        format!(
                            "run /mustard:qa or bun .claude/scripts/qa-run.js --spec {s}, \
                             or set MUSTARD_QA_GATE_MODE=warn"
                        )
                    },
                ),
            );
            if qa_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    json!({
                        "result": "deny-qa-missing",
                        "mode": mode_str(mode),
                        "qaMode": mode_str(qa_mode),
                        "spec": spec_ref,
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through.
        } else if overall.as_deref() == Some("skip") {
            // No testable AC — QA is advisory; fall through.
        } else if overall.as_deref() != Some("pass") {
            let failed_str = if failed_count > 0 {
                format!("{failed_count} criteria failed")
            } else {
                format!("overall={}", overall.as_deref().unwrap_or("unknown"))
            };
            let reason = format_gate_message(
                "Close Gate",
                &spec_ref.map_or_else(
                    || format!("QA did not pass ({failed_str})"),
                    |s| format!("QA failed for spec \"{s}\": {failed_str}"),
                ),
                "CLOSE requires every acceptance criterion to pass",
                "fix the failing criteria and re-run /mustard:qa, or set \
                 MUSTARD_QA_GATE_MODE=warn",
            );
            if qa_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    json!({
                        "result": "deny-qa-fail",
                        "mode": mode_str(mode),
                        "qaMode": mode_str(qa_mode),
                        "spec": spec_ref,
                        "qaOverall": overall,
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through.
        }
        // QA passed → fall through.
    }

    // ── Build/test gate (Wave 9) ──────────────────────────────────────────
    let Some(cmds) = read_mustard_commands(cwd) else {
        // mustard.json absent/unreadable → fail-open skip.
        return Verdict::Allow;
    };
    let stages: Vec<(&str, String)> = [
        ("build", cmds.build),
        ("type", cmds.type_check),
        ("lint", cmds.lint),
        ("test", cmds.test),
    ]
    .into_iter()
    .filter_map(|(name, cmd)| cmd.map(|c| (name, c)))
    .collect();
    if stages.is_empty() {
        // No commands configured → fail-open skip.
        return Verdict::Allow;
    }

    let mut stage_results: Vec<Value> = Vec::new();
    let mut first_failure: Option<(&str, String)> = None;
    for (name, cmd) in &stages {
        let result = run_command(cmd, cwd);
        if !result.ok && result.env_error {
            // Env bug → fail-open: record env-error, continue.
            stage_results.push(json!({ "stage": name, "result": "env-error" }));
            continue;
        }
        if result.ok {
            stage_results.push(json!({ "stage": name, "result": "pass" }));
        } else {
            stage_results.push(json!({
                "stage": name,
                "result": "fail",
                "output": result.output,
            }));
            if first_failure.is_none() {
                first_failure = Some((name, result.output));
            }
        }
    }

    // Emit the close-gate.check event.
    emit_close_gate_event(
        cwd,
        json!({
            "result": if first_failure.is_some() { "fail" } else { "pass" },
            "stages": stage_results,
            "mode": mode_str(mode),
        }),
    );

    if let Some((stage, output)) = first_failure {
        let snippet = if output.is_empty() {
            "(no output)".to_string()
        } else {
            let t = truncate(&output, TRUNCATE_CHARS);
            let ellipsis = if output.len() > TRUNCATE_CHARS { "…" } else { "" };
            format!("{t}{ellipsis}")
        };
        let reason = format_gate_message(
            "Close Gate",
            &format!("{stage} failed: {snippet}"),
            "CLOSE requires build, type, lint, and test to pass",
            &format!(
                "fix the {stage} failure and retry, or set MUSTARD_CLOSE_GATE_MODE=warn"
            ),
        );
        if mode == GateMode::Strict {
            return Verdict::Deny { reason };
        }
        // warn mode → advisory, never deny.
        return Verdict::Warn { message: reason };
    }

    Verdict::Allow
}

/// The lowercase mode string, for event payloads.
fn mode_str(mode: GateMode) -> &'static str {
    match mode {
        GateMode::Off => "off",
        GateMode::Warn => "warn",
        GateMode::Strict => "strict",
    }
}

/// Public entry point: run every close-gate sub-gate for `(cwd, spec)` with
/// modes resolved from the environment.
///
/// Returns `Ok(())` when CLOSE is allowed (every strict gate passes) or when
/// only a build/test warning fires (still safe to proceed). Returns
/// `Err(reason)` with the formatted gate message when any strict gate denies.
///
/// Fail-open: a [`Verdict::Warn`] from the build/test gate does **not** block
/// CLOSE (matches the prior behavior — warn mode was advisory).
///
/// This is the entry point used by `mustard-rt run emit-phase --to CLOSE` to
/// run the same checks the legacy Write/Edit hook used to perform.
pub fn gate_close_for_spec(cwd: &str, spec: &str) -> Result<(), String> {
    let modes = CloseGateModes::from_env();
    match run_close_gates(cwd, Some(spec), modes) {
        Verdict::Deny { reason } => Err(reason),
        // Warn → advisory only (CLOSE proceeds). Allow / others → ok.
        _ => Ok(()),
    }
}

impl Check for CloseGate {
    /// Gate a `PreToolUse(Write|Edit)` pipeline-state write that transitions to
    /// CLOSE. The verdict is computed entirely by [`close_gate`], which carries
    /// its own `MUSTARD_*_MODE` resolution — independent of the dispatcher's
    /// module-level mode.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if !matches!(input.tool_name.as_deref(), Some("Write") | Some("Edit")) {
            return Ok(Verdict::Allow);
        }
        let cwd = if ctx.project_dir.is_empty() {
            input.cwd.as_deref().unwrap_or(".").to_string()
        } else {
            ctx.project_dir.clone()
        };
        Ok(close_gate(input, &cwd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// Build a project dir with the standard `.claude` subtree.
    fn make_project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join(".harness")).unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join(".pipeline-states")).unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join("active"))
            .unwrap();
        dir
    }

    /// A `PreToolUse(Write)` close-state input for `spec_name`.
    fn close_input(cwd: &Path, spec_name: &str) -> HookInput {
        let state_file = cwd
            .join(".claude")
            .join(".pipeline-states")
            .join(format!("{spec_name}.json"));
        HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({
                "file_path": state_file.to_string_lossy(),
                "content": json!({ "spec": spec_name, "phase": "CLOSE" }).to_string(),
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            ..HookInput::default()
        }
    }

    fn write_spec(cwd: &Path, spec_name: &str, body: &str) {
        let dir = cwd
            .join(".claude")
            .join("spec")
            .join("active")
            .join(spec_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), body).unwrap();
    }

    fn write_mustard_json(cwd: &Path, fields: Value) {
        std::fs::write(cwd.join("mustard.json"), fields.to_string()).unwrap();
    }

    fn write_qa_event(cwd: &Path, spec: &str, overall: &str, criteria: Value) {
        // Append a `qa.result` event through the SQLite harness store, the
        // same path `qa-run` uses in production.
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-19T00:00:00.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Cli,
                id: Some("qa-run".to_string()),
                actor_type: None,
            },
            event: "qa.result".to_string(),
            payload: json!({ "spec": spec, "overall": overall, "criteria": criteria }),
            spec: Some(spec.to_string()),
        };
        SqliteEventStore::for_project(cwd)
            .and_then(|store| store.append(&event))
            .unwrap();
    }

    /// The strict-cmd commands that exit non-zero / zero, cross-platform.
    fn exit_fail() -> &'static str {
        if cfg!(windows) {
            "cmd /c exit 1"
        } else {
            "sh -c \"exit 1\""
        }
    }
    fn exit_pass() -> &'static str {
        if cfg!(windows) {
            "cmd /c exit 0"
        } else {
            "sh -c \"exit 0\""
        }
    }

    /// Every sub-gate strict — the production default.
    fn all_strict() -> CloseGateModes {
        CloseGateModes {
            close: GateMode::Strict,
            debt: GateMode::Strict,
            checklist: GateMode::Strict,
            qa: GateMode::Strict,
        }
    }

    /// Strict close-gate with the QA sub-gate off — isolates the build/test /
    /// checklist / debt gates without needing a `qa.result` event.
    fn no_qa() -> CloseGateModes {
        CloseGateModes {
            qa: GateMode::Off,
            ..all_strict()
        }
    }

    // --- trigger guards -----------------------------------------------------

    #[test]
    fn skips_non_pipeline_state_files() {
        assert!(!is_pipeline_state_file("/p/src/app.json"));
        assert!(is_pipeline_state_file("/p/.claude/.pipeline-states/x.json"));
    }

    #[test]
    fn skips_non_close_phase() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let mut input = close_input(dir.path(), "spec-exec");
        // Override phase to EXECUTE.
        input.tool_input = json!({
            "file_path": input.tool_input["file_path"],
            "content": json!({ "spec": "spec-exec", "phase": "EXECUTE" }).to_string(),
        });
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict()),
            Verdict::Allow
        );
    }

    // --- Wave 9: build/test gate (harness-wave9.test.js) -------------------

    #[test]
    fn close_gate_denies_on_failing_test_command() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        // QA off + no checklist/debt → isolate the build/test gate.
        let input = close_input(dir.path(), "auth-login");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("[Close Gate]")),
            other => panic!("expected Deny on failing test, got {other:?}"),
        }
    }

    #[test]
    fn close_gate_allows_on_passing_commands() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        let input = close_input(dir.path(), "auth-login");
        let verdict = close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa());
        assert!(!verdict.is_blocking(), "passing tests must not deny");
    }

    #[test]
    fn close_gate_warn_mode_does_not_deny_failing_test() {
        // mode=warn + failing test → advisory Warn, never Deny.
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let modes = CloseGateModes {
            close: GateMode::Warn,
            ..no_qa()
        };
        let input = close_input(dir.path(), "warn-spec");
        let verdict = close_gate_with_modes(&input, dir.path().to_str().unwrap(), modes);
        assert!(!verdict.is_blocking(), "warn mode must not deny");
        assert!(matches!(verdict, Verdict::Warn { .. }));
    }

    #[test]
    fn close_gate_off_mode_skips_entirely() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let modes = CloseGateModes {
            close: GateMode::Off,
            ..all_strict()
        };
        let input = close_input(dir.path(), "off-spec");
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), modes),
            Verdict::Allow
        );
    }

    #[test]
    fn close_gate_fails_open_without_mustard_json() {
        let dir = make_project();
        let input = close_input(dir.path(), "spec2");
        // No mustard.json → fail-open, no deny.
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow
        );
    }

    // --- Wave 10: QA gate (harness-wave10.test.js) -------------------------

    #[test]
    fn close_gate_denies_when_no_qa_result() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        // QA strict, no qa.result event → deny.
        let input = close_input(dir.path(), "my-spec");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict()) {
            Verdict::Deny { reason } => {
                assert!(reason.to_lowercase().contains("qa"));
            }
            other => panic!("expected Deny for missing QA, got {other:?}"),
        }
    }

    #[test]
    fn close_gate_denies_when_qa_failed() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(
            dir.path(),
            "fail-qa-spec",
            "fail",
            json!([{ "id": "AC-1", "status": "fail" }]),
        );
        let input = close_input(dir.path(), "fail-qa-spec");
        assert!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict())
                .is_blocking()
        );
    }

    #[test]
    fn close_gate_allows_when_qa_passed() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(
            dir.path(),
            "pass-qa-spec",
            "pass",
            json!([{ "id": "AC-1", "status": "pass" }]),
        );
        let input = close_input(dir.path(), "pass-qa-spec");
        assert!(
            !close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict())
                .is_blocking()
        );
    }

    #[test]
    fn close_gate_allows_when_qa_skipped() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(dir.path(), "skip-qa-spec", "skip", json!([]));
        let input = close_input(dir.path(), "skip-qa-spec");
        assert!(
            !close_gate_with_modes(&input, dir.path().to_str().unwrap(), all_strict())
                .is_blocking()
        );
    }

    #[test]
    fn close_gate_qa_off_does_not_deny_missing_qa() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        // No qa.result, QA gate off → must not deny on QA grounds.
        let input = close_input(dir.path(), "off-qa-spec");
        assert!(
            !close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa())
                .is_blocking()
        );
    }

    // --- checklist gate (checklist-mark.test.js) ---------------------------

    #[test]
    fn close_gate_denies_unmarked_checklist() {
        let dir = make_project();
        write_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [x] first done\n- [ ] second open\n\
             - [ ] third open\n\n## Notes\n",
        );
        let input = close_input(dir.path(), "demo");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("2 unmarked")),
            other => panic!("expected Deny for unmarked checklist, got {other:?}"),
        }
    }

    #[test]
    fn close_gate_passes_fully_marked_checklist() {
        let dir = make_project();
        write_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [x] first\n- [x] second\n\n## Notes\n",
        );
        // No mustard.json → after the checklist gate passes, build gate skips.
        let input = close_input(dir.path(), "demo");
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow
        );
    }

    // --- debt-marker gate ---------------------------------------------------

    #[test]
    fn debt_markers_detected_in_actionable_section() {
        let dir = make_project();
        write_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Tasks\n\n- [x] done\n- TODO: finish the wiring\n\n## Notes\n",
        );
        let markers = find_debt_markers(dir.path().to_str().unwrap(), Some("demo"));
        assert!(markers.iter().any(|m| m.pattern == "TODO"));
    }

    #[test]
    fn debt_markers_ignored_outside_actionable_sections() {
        let dir = make_project();
        write_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Concerns\n\n- TODO: a documented follow-up\n",
        );
        // Concerns is not an actionable section → no markers.
        let markers = find_debt_markers(dir.path().to_str().unwrap(), Some("demo"));
        assert!(markers.is_empty());
    }

    #[test]
    fn debt_markers_skip_fenced_code() {
        let dir = make_project();
        write_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Tasks\n\n```\nTODO: this is an example\n```\n- [x] done\n",
        );
        let markers = find_debt_markers(dir.path().to_str().unwrap(), Some("demo"));
        assert!(markers.is_empty());
    }

    // --- close-gate.check event --------------------------------------------

    #[test]
    fn close_gate_emits_check_event() {
        let dir = make_project();
        write_mustard_json(
            dir.path(),
            json!({ "testCommand": exit_pass(), "buildCommand": exit_pass() }),
        );
        let input = close_input(dir.path(), "spec-event");
        let _ = close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa());
        let events = SqliteEventStore::for_project(dir.path().to_str().unwrap())
            .and_then(|s| s.replay())
            .unwrap();
        assert!(events.iter().any(|e| e.event == "close-gate.check"));
    }

    // --- extractPhase / extractSpec parity ---------------------------------

    #[test]
    fn extract_phase_reads_phase_name_and_legacy_phase() {
        // Real shape: numeric `phase` + string `phaseName`.
        assert_eq!(
            extract_phase(r#"{"phase":3,"phaseName":"CLOSE"}"#).as_deref(),
            Some("CLOSE")
        );
        // Legacy shape: string `phase`.
        assert_eq!(
            extract_phase(r#"{"phase":"close"}"#).as_deref(),
            Some("CLOSE")
        );
        assert_eq!(extract_phase("not json"), None);
    }

    // --- new spec-aware entry point used by `emit-phase --to CLOSE` --------

    #[test]
    fn run_close_gates_denies_on_failing_build_command() {
        // The spec-aware entry point exercised by the post-Wave-2 emit-phase
        // gate path. A failing build/test command in strict mode → Deny.
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let verdict = run_close_gates(
            dir.path().to_str().unwrap(),
            Some("spec-fail"),
            no_qa(),
        );
        assert!(verdict.is_blocking(), "failing build must deny");
    }

    #[test]
    fn run_close_gates_allows_when_everything_passes() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        write_qa_event(
            dir.path(),
            "spec-ok",
            "pass",
            json!([{ "id": "AC-1", "status": "pass" }]),
        );
        let verdict = run_close_gates(
            dir.path().to_str().unwrap(),
            Some("spec-ok"),
            all_strict(),
        );
        assert!(!verdict.is_blocking(), "all-pass must allow");
    }

    #[test]
    fn run_close_gates_denies_missing_qa_when_strict() {
        // QA strict + no qa.result event → Deny on QA grounds.
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        let verdict = run_close_gates(
            dir.path().to_str().unwrap(),
            Some("needs-qa"),
            all_strict(),
        );
        match verdict {
            Verdict::Deny { reason } => assert!(reason.to_lowercase().contains("qa")),
            other => panic!("expected Deny for missing QA, got {other:?}"),
        }
    }
}
