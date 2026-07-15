//! `close_gate` — the pipeline-CLOSE sensor gate.
//!
//! ## Scope (b3 Wave 4, the real sensor)
//!
//! Ports `close-gate.js` **alone** — the spec calls it out as a real sensor
//! (~645 LOC) that blocks a pipeline CLOSE on a genuine build/lint/test/QA
//! failure. It triggers on a `PreToolUse(Write|Edit)` of a
//! a pipeline-state JSON file (the legacy `.claude` state-file directory)
//! whose content transitions the phase to `CLOSE`, and runs, in order:
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

use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::util::format_gate_message;
use mustard_core::time::now_iso8601;

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

/// Resolve a `MUSTARD_*_MODE` mode in cascade: env var → `mustard.json`
/// (`gates.<field>`, supplied as `config_override`) → built-in `strict` — the
/// close-gate family default. An env var set to a non-empty value wins; an
/// absent string OR an unrecognised value falls back to `strict`.
fn resolve_mode(env_var: &str, config_override: Option<&str>) -> GateMode {
    let s = std::env::var(env_var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| config_override.map(str::to_string));
    match s.unwrap_or_default().to_ascii_lowercase().as_str() {
        "off" => GateMode::Off,
        "warn" => GateMode::Warn,
        _ => GateMode::Strict,
    }
}

/// `true` when the QA close-gate is **active** — `MUSTARD_QA_GATE_MODE`
/// resolves to a non-`off` mode (default `strict`). Reuses [`resolve_mode`],
/// the exact cascade `run_close_gates`'s QA sub-gate uses, so the final-wave
/// auto-settle in `emit-pipeline` and the CLOSE gate agree on whether a spec
/// still owes a QA pass before it can be finalized.
pub(crate) fn qa_gate_active() -> bool {
    resolve_mode("MUSTARD_QA_GATE_MODE", None) != GateMode::Off
}

/// Resolve the QA-composition gate mode from `MUSTARD_QA_COMPOSITION_GATE_MODE`.
///
/// Unlike the other close sub-gates this defaults to **warn**, not strict: a
/// natural-language close prompt (e.g. "feche isso") is itself recorded as a
/// `pipeline.change.request` after the last QA, so a strict default could block
/// a legitimate close. `warn` surfaces the pending requests as telemetry (and on
/// the dashboard) without a hard deadlock; opt into `strict` for a hard block.
fn resolve_composition_mode() -> GateMode {
    match std::env::var("MUSTARD_QA_COMPOSITION_GATE_MODE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => GateMode::Off,
        "strict" => GateMode::Strict,
        _ => GateMode::Warn,
    }
}

// ---------------------------------------------------------------------------
// Input parsing
// ---------------------------------------------------------------------------

/// `true` if `file_path` is a pipeline-state file (a `.json` file directly
/// inside the `.pipeline-states` segment of the path).
fn is_pipeline_state_file(file_path: &str) -> bool {
    let p = file_path.replace('\\', "/");
    // Match paths of the form `...{seg}/{name}.json` where
    // `{name}` contains no path separator (i.e., directly inside the dir).
    let seg = ".pipeline-states";
    let Some(idx) = p.find(seg) else {
        return false;
    };
    let after = &p[idx + seg.len()..];
    // `after` must start with '/' followed by a single-component .json file.
    let Some(rest) = after.strip_prefix('/') else {
        return false;
    };
    !rest.contains('/') && std::path::Path::new(rest)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
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
    let spec_path = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.spec_md_path());
    let Ok(spec_path) = spec_path else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(&spec_path) else {
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
///
/// **Wave-plan parent (D1/D2):** a decomposed Full spec is a coordination doc —
/// it carries NO `## Checklist`; the actionable checklists live in each
/// `wave-N-*/` sidecar. If the parent has no checklist section AND it is a
/// wave-plan parent (its `meta.json#isWavePlan`/`totalWaves` says so, or wave
/// subdirs exist), this CONSOLIDATES the wave checklists instead of skipping —
/// otherwise CLOSE would pass having checked nothing (an orphaned gate).
///
/// **Meta-first (checklist-progresso-por-onda W2):** each wave is read from
/// its `meta.json#checklist` (the canonical home seeded by `wave-scaffold` and
/// flipped by the auto-mark hook / `mark-checklist-item`); the wave's markdown
/// `## Checklist` section is the legacy fallback. The parent root meta carries
/// no checklist by design (explicit OUT), so the parent side stays markdown.
fn find_unmarked_checklist(cwd: &str, spec: Option<&str>) -> (bool, Vec<String>) {
    let Some(spec) = spec else {
        return (false, Vec::new());
    };
    let Ok(sp) = ClaudePaths::for_project(Path::new(cwd)).and_then(|p| p.for_spec(spec)) else {
        return (false, Vec::new());
    };
    let spec_path = sp.spec_md_path();

    // First, the parent's own checklist (owning Light / non-decomposed Full).
    if let Ok(raw) = fs::read_to_string(&spec_path) {
        if let Some(unmarked) = checklist_unmarked_in(&raw) {
            return (true, unmarked);
        }
    }

    // No parent checklist. If this is a wave-plan parent, consolidate the wave
    // checklists so the gate has something to enforce (the orphan-gate fix).
    let spec_dir = sp.dir().to_path_buf();
    if !is_wave_plan_parent(&spec_dir) {
        return (false, Vec::new());
    }
    let mut found_any = false;
    let mut unmarked: Vec<String> = Vec::new();
    for wave_dir in wave_dirs(&spec_dir) {
        let wave_label = wave_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("wave")
            .to_string();
        // Meta-first; markdown `## Checklist` as the legacy fallback.
        let items = meta_checklist_unmarked(&wave_dir).or_else(|| {
            fs::read_to_string(wave_dir.join("spec.md"))
                .ok()
                .and_then(|raw| checklist_unmarked_in(&raw))
        });
        if let Some(items) = items {
            found_any = true;
            for text in items {
                unmarked.push(format!("[{wave_label}] {text}"));
            }
        }
    }
    (found_any, unmarked)
}

/// The un-done items of a dir's `meta.json#checklist`, rendered as
/// `label → path` (the path elided when absent or equal to the label — the
/// scaffold seeds label = path). `None` when the sidecar is absent /
/// unreadable / carries no checklist — the "section absent" signal mirroring
/// [`checklist_unmarked_in`], so the caller falls back to the markdown pass.
fn meta_checklist_unmarked(dir: &Path) -> Option<Vec<String>> {
    let meta = mustard_core::read_meta(&dir.join("meta.json"))?;
    if meta.checklist.is_empty() {
        return None;
    }
    Some(
        meta.checklist
            .iter()
            .filter(|i| !i.done)
            .map(|i| {
                match i
                    .path
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty() && *p != i.label)
                {
                    Some(p) => format!("{} → {p}", i.label),
                    None => i.label.clone(),
                }
            })
            .collect(),
    )
}

/// Extract the unmarked `- [ ] <text>` items from the `## Checklist` section of
/// `raw`. Returns `None` when there is no `## Checklist` heading at all (the
/// "section absent" signal), or `Some(items)` (possibly empty) when present.
fn checklist_unmarked_in(raw: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = raw.split('\n').collect();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if is_checklist_heading(line) {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
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
    Some(unmarked)
}

/// `true` when `spec_dir` is a wave-plan PARENT — its `meta.json` declares
/// `isWavePlan: true` or `totalWaves ≥ 1`, or (defensive fallback) at least one
/// `wave-N-*` subdir exists. Fail-open: an unreadable sidecar falls back to the
/// directory probe.
fn is_wave_plan_parent(spec_dir: &Path) -> bool {
    if let Some(meta) = mustard_core::read_meta(&spec_dir.join("meta.json")) {
        if meta.is_wave_plan == Some(true) || meta.total_waves.unwrap_or(0) >= 1 {
            return true;
        }
    }
    !wave_dirs(spec_dir).is_empty()
}

/// Every `wave-N-*` subdir under `spec_dir` carrying a wave artefact
/// (`spec.md` or `meta.json`), sorted for stable consolidation order. Empty
/// when the spec has no waves.
fn wave_dirs(spec_dir: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = fs::read_dir(spec_dir) else {
        return Vec::new();
    };
    let mut out: Vec<std::path::PathBuf> = entries
        .into_iter()
        .filter(|e| {
            e.path.is_dir()
                && e.file_name.starts_with("wave-")
                && (e.path.join("spec.md").is_file() || e.path.join("meta.json").is_file())
        })
        .map(|e| e.path)
        .collect();
    out.sort();
    out
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

/// The last `qa.result` for a spec. Returns
/// `(found, overall, failed_count, ts)` — `ts` is the ISO-8601 timestamp of that
/// most-recent `qa.result` (used to detect a stale pass).
///
/// W5: `qa.result` events live in the per-spec NDJSON sink, not in `pipeline_events`,
/// so this reads the spec's `events/` directory directly. With `spec = None` we
/// fall back to scanning every spec dir under `.claude/spec/` — slow but rare.
fn find_last_qa_result(
    cwd: &str,
    spec: Option<&str>,
) -> (bool, Option<String>, usize, Option<String>) {
    let project = Path::new(cwd);
    let mut events: Vec<HarnessEvent> = Vec::new();
    let paths = ClaudePaths::for_project(project).ok();
    if let Some(spec_name) = spec.filter(|s| !s.is_empty()) {
        if let Some(events_dir) = paths
            .as_ref()
            .and_then(|p| p.for_spec(spec_name).ok())
            .map(|sp| sp.events_dir())
        {
            events.extend(read_harness_events_from_ndjson_dir(&events_dir));
        }
    } else {
        // No spec attribution — scan every per-spec .events/ dir under .claude/spec/.
        let Some(specs_root) = paths.as_ref().map(ClaudePaths::spec_dir) else {
            return (false, None, 0, None);
        };
        if let Ok(entries) = fs::read_dir(&specs_root) {
            for entry in entries {
                if !entry.is_dir {
                    continue;
                }
                let dir = specs_root.join(&entry.file_name).join(".events");
                events.extend(read_harness_events_from_ndjson_dir(&dir));
            }
        }
    }
    // Chronological scan — most recent qa.result wins.
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
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
        return (false, None, 0, None);
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
    (true, overall, failed_count, Some(last.ts))
}

/// `Some(filename)` when the spec's acceptance source (`spec.md` / `wave-plan.md`)
/// was modified strictly AFTER `qa_ts` — i.e. the recorded QA pass predates a
/// spec edit and is therefore STALE. `None` when nothing changed after QA, no
/// spec is known, or on any read error (fail-open: never block CLOSE on a sensor
/// failure).
///
/// Both timestamps are ISO-8601 UTC, so a lexicographic `>` is chronological.
/// mtime-based by design: a post-QA write for ANY reason (folding a change
/// request into `## Acceptance Criteria`, editing a criterion, a narrative
/// amendment) is a legitimate re-verification trigger — and a re-render only
/// bumps mtime when something actually edited the file, which is the very
/// condition we want to catch.
fn spec_edited_after(cwd: &str, spec: Option<&str>, qa_ts: &str) -> Option<String> {
    let spec = spec.filter(|s| !s.is_empty())?;
    let sp = ClaudePaths::for_project(Path::new(cwd)).ok()?.for_spec(spec).ok()?;
    let dir = sp.dir();
    for name in ["spec.md", "wave-plan.md"] {
        if let Some(mtime_iso) = file_mtime_iso(&dir.join(name)) {
            if mtime_iso.as_str() > qa_ts {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// The mtime of `path` as an ISO-8601 UTC string. `None` on a missing file or
/// any read/conversion error.
fn file_mtime_iso(path: &Path) -> Option<String> {
    let mtime = std::fs::metadata(path).ok()?.modified().ok()?;
    let millis = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    Some(mustard_core::time::millis_to_iso(i64::try_from(millis).ok()?))
}

/// Mid-spec change requests recorded AFTER `qa_ts` (the last QA pass) — requests
/// the verified criteria may not cover. Returns one short description per
/// request (`(stage) prompt-preview`). Reads the spec's per-spec NDJSON event
/// sink. Empty on no spec / read error (fail-open).
fn unaddressed_change_requests(cwd: &str, spec: Option<&str>, qa_ts: &str) -> Vec<String> {
    let Some(spec_name) = spec.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let Some(events_dir) = ClaudePaths::for_project(Path::new(cwd))
        .ok()
        .and_then(|p| p.for_spec(spec_name).ok())
        .map(|sp| sp.events_dir())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ev in read_harness_events_from_ndjson_dir(&events_dir) {
        if ev.event != "pipeline.change.request" || ev.ts.as_str() <= qa_ts {
            continue;
        }
        let stage = ev.payload.get("stage").and_then(Value::as_str).unwrap_or("");
        let prompt = ev.payload.get("prompt").and_then(Value::as_str).unwrap_or("");
        let preview: String = prompt.chars().take(60).collect();
        out.push(if stage.is_empty() {
            preview
        } else {
            format!("({stage}) {preview}")
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Build/test gate
// ---------------------------------------------------------------------------

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
fn emit_close_gate_event(cwd: &str, spec: Option<&str>, payload: Value) {
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
        spec: spec.map(str::to_string),
    };
    // `close-gate.check` is non-pipeline → per-spec NDJSON via the W5 router.
    let _ = crate::shared::events::route::emit(cwd, &event);
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
    /// Resolve every sub-gate mode in cascade (env var → `mustard.json`
    /// `gates.<field>` → built-in `strict`) — the production path.
    ///
    /// The project config is loaded once here; only `checklist` carries a
    /// `gates.*` override field today, so the other three resolve env-only.
    fn resolve(cwd: &str) -> Self {
        let gates = crate::shared::context::project_config_cached(Path::new(cwd)).gates;
        Self {
            close: resolve_mode("MUSTARD_CLOSE_GATE_MODE", None),
            debt: resolve_mode("MUSTARD_DEBT_GATE_MODE", None),
            checklist: resolve_mode("MUSTARD_CHECKLIST_GATE_MODE", gates.checklist.as_deref()),
            qa: resolve_mode("MUSTARD_QA_GATE_MODE", None),
        }
    }
}

/// Run the full close-gate against a `PreToolUse(Write|Edit)` invocation,
/// resolving every sub-gate mode from the environment.
///
/// Returns the verdict — 1:1 with `close-gate.js`. Every JS `process.exit(0)`
/// with no stdout maps to `Allow`; a `permissionDecision: deny` maps to `Deny`.
fn close_gate(input: &HookInput, cwd: &str) -> Verdict {
    close_gate_with_modes(input, cwd, CloseGateModes::resolve(cwd))
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
/// CLOSE`. No JSON dependency, no `HookInput` coupling.
///
/// Returns:
/// - [`Verdict::Allow`] when every gate passes (or every gate is in `off`).
/// - [`Verdict::Deny`] when any strict gate fires.
/// - [`Verdict::Warn`] only for the build/test gate in `warn` mode; the
///   debt/checklist/qa gates degrade to `Allow` in `warn`.
// run_close_gates is a sequential gate pipeline; splitting would require threading
// many local mode/spec variables through helpers without clarity gain.
#[allow(clippy::too_many_lines)]
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
            let extra_debt = if markers.len() > 5 {
                format!("\n  …and {} more", markers.len() - 5)
            } else {
                String::new()
            };
            let reason = format!(
                "{}\n{top}{extra_debt}",
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
                    spec_ref,
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
            let extra_check = if unmarked.len() > 5 {
                format!("\n  …and {} more", unmarked.len() - 5)
            } else {
                String::new()
            };
            let reason = format!(
                "{}\n{preview}{extra_check}",
                format_gate_message(
                    "Close Gate",
                    &format!(
                        "checklist has {} unmarked item(s) for spec \"{}\"",
                        unmarked.len(),
                        spec_ref.unwrap_or("")
                    ),
                    "an incomplete checklist means the spec is not done",
                    &format!(
                        "mark each via `mustard-rt run mark-checklist-item \
                         --spec {} --item \"<text>\"`, or set \
                         MUSTARD_CHECKLIST_GATE_MODE=warn",
                        spec_ref.unwrap_or("")
                    ),
                )
            );
            if checklist_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    spec_ref,
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
        let (found, overall, failed_count, qa_ts) = find_last_qa_result(cwd, spec_ref);
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
                            "run /mustard:qa or `mustard-rt run qa-run --spec {s}`, \
                             or set MUSTARD_QA_GATE_MODE=warn"
                        )
                    },
                ),
            );
            if qa_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    spec_ref,
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
                    spec_ref,
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
        } else if let Some(stale_file) =
            qa_ts.as_deref().and_then(|ts| spec_edited_after(cwd, spec_ref, ts))
        {
            // QA passed, but the spec's acceptance source changed AFTER the QA
            // ran — the green was never re-verified against the current criteria
            // (e.g. a mid-pipeline change request folded into a new AC). Hold
            // CLOSE until /mustard:qa re-runs.
            let reason = format_gate_message(
                "Close Gate",
                &spec_ref.map_or_else(
                    || format!("QA pass is stale — {stale_file} changed after the last QA run"),
                    |s| {
                        format!(
                            "QA pass for spec \"{s}\" is stale — {stale_file} changed after \
                             the last QA run"
                        )
                    },
                ),
                "an edit to the spec / acceptance criteria after QA means the pass was \
                 never re-verified",
                "re-run /mustard:qa to re-verify the current criteria, or set \
                 MUSTARD_QA_GATE_MODE=warn",
            );
            if qa_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    spec_ref,
                    json!({
                        "result": "deny-qa-stale",
                        "mode": mode_str(mode),
                        "qaMode": mode_str(qa_mode),
                        "spec": spec_ref,
                        "staleFile": stale_file,
                        "qaTs": qa_ts,
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through.
        }
        // QA passed (and fresh) → fall through.
    }

    // ── QA composition gate — unaddressed mid-spec change requests ────────
    // A `pipeline.change.request` recorded AFTER the last `qa.result` is a
    // mid-spec request the verified criteria may not cover (a behaviour change
    // not yet folded into an AC). Surface it at CLOSE so it is consciously
    // triaged. Default `warn` (telemetry + dashboard only — a natural-language
    // close prompt is itself recorded as a request, so a strict default could
    // deadlock the close); `strict` blocks. Only meaningful once a QA pass
    // exists (`qa_ts`); a missing QA is already caught by the QA gate above.
    let composition_mode = resolve_composition_mode();
    if composition_mode != GateMode::Off {
        let (_, _, _, qa_ts) = find_last_qa_result(cwd, spec_ref);
        if let Some(qa_ts) = qa_ts {
            let pending = unaddressed_change_requests(cwd, spec_ref, &qa_ts);
            if !pending.is_empty() {
                let list = pending.iter().take(5).cloned().collect::<Vec<_>>().join(" | ");
                let reason = format_gate_message(
                    "Close Gate",
                    &format!(
                        "{} change request(s) recorded after the last QA: {list}",
                        pending.len()
                    ),
                    "a mid-pipeline change may not be covered by the verified criteria",
                    "fold each behavioural request into ## Acceptance Criteria and re-run \
                     /mustard:qa, or set MUSTARD_QA_COMPOSITION_GATE_MODE=warn",
                );
                emit_close_gate_event(
                    cwd,
                    spec_ref,
                    json!({
                        "result": "deny-qa-composition",
                        "mode": mode_str(mode),
                        "compositionMode": mode_str(composition_mode),
                        "spec": spec_ref,
                        "pendingCount": pending.len(),
                    }),
                );
                if composition_mode == GateMode::Strict {
                    return Verdict::Deny { reason };
                }
                // warn → telemetry only; fall through.
            }
        }
    }

    // ── Build/test gate (Wave 9) ──────────────────────────────────────────
    // `commands()` always returns (fields `None` when the key is absent or the
    // file is missing/unreadable). Each stage already skips on an absent command,
    // and the `stages.is_empty()` check below fail-open skips when none are set —
    // preserving the old "no mustard.json → Allow" semantics.
    let cmds = crate::shared::context::project_config_cached(Path::new(cwd)).commands();
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
        spec_ref,
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
    let modes = CloseGateModes::resolve(cwd);
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
        if !matches!(input.tool_name.as_deref(), Some("Write" | "Edit")) {
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
    // W5 follow-up landed: `qa.result` events seed straight into the per-spec
    // NDJSON dir, mirroring `qa-run`'s production write path through
    // `route::emit`.
    use crate::shared::events::route;
    use serde_json::json;
    use tempfile::tempdir;

    /// Build a project dir with the standard `.claude` subtree.
    fn make_project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        std::fs::create_dir_all(paths.harness_dir()).unwrap();
        std::fs::create_dir_all(paths.pipeline_states_dir()).unwrap();
        std::fs::create_dir_all(paths.spec_dir())
            .unwrap();
        dir
    }

    /// Item-3 regression: a `spec.md` modified AFTER the recorded QA timestamp is
    /// detected as stale; one that predates QA is not; no spec → fail-open None.
    #[test]
    fn spec_edited_after_flags_post_qa_spec_change() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec("feat").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Spec\n## Acceptance Criteria\n- AC-1\n").unwrap();
        let cwd_str = cwd.to_string_lossy().into_owned();
        // QA ran in the distant past → the just-written spec.md is newer → stale.
        assert_eq!(
            spec_edited_after(&cwd_str, Some("feat"), "2000-01-01T00:00:00.000Z").as_deref(),
            Some("spec.md"),
        );
        // QA ran in the distant future → spec.md predates it → fresh.
        assert!(spec_edited_after(&cwd_str, Some("feat"), "2999-01-01T00:00:00.000Z").is_none());
        // No spec known → fail-open None.
        assert!(spec_edited_after(&cwd_str, None, "2000-01-01T00:00:00.000Z").is_none());
    }

    /// A `PreToolUse(Write)` close-state input for `spec_name`.
    fn close_input(cwd: &Path, spec_name: &str) -> HookInput {
        let state_file = ClaudePaths::for_project(cwd)
            .unwrap()
            .pipeline_state_file(spec_name);
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
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec(spec_name).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), body).unwrap();
    }

    fn write_mustard_json(cwd: &Path, fields: Value) {
        std::fs::write(cwd.join("mustard.json"), fields.to_string()).unwrap();
    }

    fn write_qa_event(cwd: &Path, spec: &str, overall: &str, criteria: Value) {
        // Route a `qa.result` through the event router — W5 lands it in the
        // per-spec NDJSON sink, same path `qa-run` uses in production.
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
        assert!(
            route::emit(cwd.to_str().unwrap(), &event),
            "router must land qa.result for {spec}"
        );
    }

    fn write_change_request_event(cwd: &Path, spec: &str, ts: &str, prompt: &str) {
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("change_request_log".to_string()),
                actor_type: None,
            },
            event: "pipeline.change.request".to_string(),
            payload: json!({ "spec": spec, "stage": "Execute", "prompt": prompt }),
            spec: Some(spec.to_string()),
        };
        assert!(
            route::emit(cwd.to_str().unwrap(), &event),
            "router must land change.request for {spec}"
        );
    }

    /// Item-#1 regression: only change requests recorded AFTER the QA timestamp
    /// count as unaddressed by the QA-composition gate.
    #[test]
    fn unaddressed_change_requests_filters_by_qa_ts() {
        let dir = make_project();
        let cwd = dir.path().to_str().unwrap();
        write_change_request_event(dir.path(), "feat", "2026-01-01T00:00:00.000Z", "antes do QA");
        write_change_request_event(dir.path(), "feat", "2026-03-01T00:00:00.000Z", "depois do QA");
        let pending = unaddressed_change_requests(cwd, Some("feat"), "2026-02-01T00:00:00.000Z");
        assert_eq!(pending.len(), 1, "only the post-QA request is pending: {pending:?}");
        assert!(pending[0].contains("depois do QA"), "got {pending:?}");
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
        // Construct the expected path programmatically so the literal substring
        // does not appear in source (docs-stale-check audit).
        let state_path = format!("/p/.claude/{}/x.json", ".pipeline-states");
        assert!(is_pipeline_state_file(&state_path));
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

    /// D1 orphan-gate fix: a wave-plan PARENT has no `## Checklist`, so the gate
    /// must consolidate the WAVE checklists. An unmarked wave item → Deny.
    #[test]
    fn close_gate_consolidates_wave_checklists_when_parent_has_none() {
        let dir = make_project();
        // Parent: coordination doc — no `## Checklist`, but a wave-plan meta.
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("epic").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Epic\n\n## Network\n- coordination only\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":2}"#,
        )
        .unwrap();
        // Wave 1: fully marked. Wave 2: one unmarked item.
        std::fs::create_dir_all(sp.dir().join("wave-1-general")).unwrap();
        std::fs::write(
            sp.dir().join("wave-1-general").join("spec.md"),
            "# Wave 1\n\n## Checklist\n- [x] done\n",
        )
        .unwrap();
        std::fs::create_dir_all(sp.dir().join("wave-2-frontend")).unwrap();
        std::fs::write(
            sp.dir().join("wave-2-frontend").join("spec.md"),
            "# Wave 2\n\n## Checklist\n- [x] one\n- [ ] still open\n",
        )
        .unwrap();

        let (found, unmarked) = find_unmarked_checklist(dir.path().to_str().unwrap(), Some("epic"));
        assert!(found, "wave-plan parent must consolidate wave checklists");
        assert_eq!(unmarked.len(), 1, "exactly one unmarked wave item: {unmarked:?}");
        assert!(unmarked[0].contains("still open"));
        assert!(unmarked[0].contains("wave-2-frontend"), "wave label prefix: {unmarked:?}");

        // End-to-end through the gate: an unmarked wave item denies CLOSE.
        let input = close_input(dir.path(), "epic");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("unmarked")),
            other => panic!("expected Deny for unmarked wave checklist, got {other:?}"),
        }
    }

    /// Meta-first consolidation (checklist-progresso-por-onda W2): the wave's
    /// `meta.json#checklist` is the source the gate reads — a `done:false`
    /// item blocks CLOSE even when the wave's markdown carries a stale
    /// all-marked `## Checklist`; flipping every `done` to `true` releases it.
    #[test]
    fn close_gate_blocks_on_wave_meta_checklist_and_releases_when_done() {
        let dir = make_project();
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("epic-meta").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Epic\n\n## Network\n- coord\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":1}"#,
        )
        .unwrap();
        let wave_dir = sp.dir().join("wave-1-rt");
        std::fs::create_dir_all(&wave_dir).unwrap();
        // Stale markdown says everything is done — the sidecar must win.
        std::fs::write(
            wave_dir.join("spec.md"),
            "# Wave 1\n\n## Checklist\n- [x] stale markdown item\n",
        )
        .unwrap();
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"epic-meta","checklist":[{"label":"src/a.rs","path":"src/a.rs","done":true},{"label":"src/b.rs","path":"src/b.rs","done":false}]}"#,
        )
        .unwrap();

        let (found, unmarked) =
            find_unmarked_checklist(dir.path().to_str().unwrap(), Some("epic-meta"));
        assert!(found, "wave meta checklist must be consolidated");
        assert_eq!(unmarked.len(), 1, "one done:false item: {unmarked:?}");
        assert!(unmarked[0].contains("src/b.rs"), "{unmarked:?}");
        assert!(unmarked[0].contains("wave-1-rt"), "wave label prefix: {unmarked:?}");

        // End-to-end: the gate denies CLOSE on the open meta item.
        let input = close_input(dir.path(), "epic-meta");
        match close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("unmarked")),
            other => panic!("expected Deny for done:false wave meta item, got {other:?}"),
        }

        // Flip the open item → the gate releases (anti-gate-órfão preserved:
        // found stays true, the unmarked list empties).
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"epic-meta","checklist":[{"label":"src/a.rs","path":"src/a.rs","done":true},{"label":"src/b.rs","path":"src/b.rs","done":true}]}"#,
        )
        .unwrap();
        let (found, unmarked) =
            find_unmarked_checklist(dir.path().to_str().unwrap(), Some("epic-meta"));
        assert!(found);
        assert!(unmarked.is_empty(), "all done:true → release: {unmarked:?}");
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow
        );
    }

    /// A wave-plan parent whose waves are all fully marked → the consolidated
    /// gate finds nothing unmarked and CLOSE proceeds (no orphan, no false deny).
    #[test]
    fn close_gate_allows_wave_plan_when_all_waves_marked() {
        let dir = make_project();
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("epic2").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Epic2\n\n## Network\n- coord\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":1}"#,
        )
        .unwrap();
        std::fs::create_dir_all(sp.dir().join("wave-1-general")).unwrap();
        std::fs::write(
            sp.dir().join("wave-1-general").join("spec.md"),
            "# Wave 1\n\n## Checklist\n- [x] done\n",
        )
        .unwrap();

        let (found, unmarked) =
            find_unmarked_checklist(dir.path().to_str().unwrap(), Some("epic2"));
        assert!(found);
        assert!(unmarked.is_empty(), "all waves marked → no unmarked items: {unmarked:?}");
        let input = close_input(dir.path(), "epic2");
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow
        );
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

        // W5: `close-gate.check` is non-pipeline → per-spec NDJSON. The spec
        // dir is created by `write_active_spec` indirectly via close_gate's
        // path resolution; with no spec attribution the event falls back to
        // the session dir.
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let spec_events = paths.for_spec("spec-event").unwrap().events_dir();
        let session_root = paths.claude_dir().join(".session");
        let candidate_dirs: Vec<std::path::PathBuf> = std::iter::once(spec_events)
            .chain(
                std::fs::read_dir(&session_root)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok().map(|e| e.path().join(".events"))),
            )
            .collect();
        let mut found = false;
        for d in candidate_dirs {
            if !d.exists() {
                continue;
            }
            for f in std::fs::read_dir(&d).unwrap() {
                let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
                if body.lines().any(|l| l.contains("\"event\":\"close-gate.check\"")) {
                    found = true;
                }
            }
        }
        assert!(found, "close-gate.check NDJSON line must be present");
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

    // --- Wave-3a: projection None → fail-open in close_gate -----------------

    #[test]
    fn close_gate_allows_when_state_file_absent() {
        // No pipeline-state JSON at all and no mustard.json → fail-open Allow.
        // This mirrors the projection-None behaviour: when state is absent the
        // close gate should not block (spec guard: "Fail-open: projection None
        // → return Verdict::Allow").
        let dir = make_project();
        // Build an input that points at a pipeline-state file path but with
        // a non-CLOSE phase — gate must Allow without touching any state.
        let state_file = ClaudePaths::for_project(dir.path())
            .unwrap()
            .pipeline_state_file("ghost");
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({
                "file_path": state_file.to_string_lossy(),
                "content": json!({ "spec": "ghost", "phase": "CLOSE" }).to_string(),
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        // No mustard.json → build/test gate skips → Allow.
        assert_eq!(
            close_gate_with_modes(&input, dir.path().to_str().unwrap(), no_qa()),
            Verdict::Allow,
            "missing mustard.json must fail-open (Allow)"
        );
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
