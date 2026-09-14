//! `close_gates` — the pipeline-CLOSE policy engine.
//!
//! ## Scope
//!
//! This is the DECISION engine for the pipeline-CLOSE gate. It runs, in order:
//!
//! 1. **Debt-marker gate** — denies if the spec still carries open
//!    `TODO`/`FIXME`/`future hook`/… markers in its actionable sections.
//! 2. **Checklist gate** — denies if the spec's `## Checklist` has unmarked
//!    items.
//! 3. **Findings gate** — collects the work unit's findings in-process and
//!    denies while any of them still owes a destination.
//! 4. **QA gate** — denies unless every acceptance criterion in the spec's
//!    `spec.ndjson` has a passing last run, and none was revised after it.
//! 5. **Build/test gate** — runs `build → type → lint → test` from
//!    `mustard.json` and denies on the first real (non-env) failure.
//!
//! Each sub-gate has its own `MUSTARD_*_MODE` env var; the dominant default is
//! **`strict`** (unlike the advisory size gates) — this is the exception to
//! Mustard's fail-open hook default, by design.
//!
//! Ported 1:1 from `close-gate.js` — the **verdict must not change**. Parity
//! tests at the bottom mirror `__tests__/harness-wave9.test.js`,
//! `__tests__/harness-wave10.test.js`, and the close-gate block of
//! `__tests__/checklist-mark.test.js`.
//!
//! ## Layering
//!
//! The engine lives in `commands/` and only commands consume it —
//! `emit-phase --to CLOSE` and `close-orchestrate` via
//! [`gate_close_for_spec`], `emit-pipeline`'s final-wave auto-settle via
//! [`qa_gate_active`]. No hook calls it: the Write/Edit adapter that fired on
//! a pipeline-state file left with that file.
//!
//! ## Build-runner note
//!
//! `close-gate.js` distinguishes a *real* sensor failure (non-zero exit →
//! deny) from an *env error* (spawn failure / timeout → fail-open, never
//! deny). `bash_guard::run_build` carries a different shape; this module ports
//! `runCommand` faithfully rather than reuse it, so the env-error/real-failure
//! distinction stays exact.
//!
//! The three shell runners in the crate — [`run_command`] here,
//! `bash_guard::run_build`, and `qa_run::run_ac_command` — deliberately stay
//! SEPARATE. Each mirrors a different JS original with a different env-error
//! taxonomy: here (and in `run_build`) a timeout / spawn failure is an
//! *env error* that never denies; in `qa-run` the same conditions are a
//! `skip`, and a `skip` is NOT a `fail` — the close-gate QA sub-gate treats an
//! all-`skip` run with acceptance criteria present as its own `deny-qa-skip`
//! verdict. `qa-run` further carries a self-invocation guard, a
//! compilation-aware variable timeout, and a `raw_arg` shell builder. A single
//! parametrized runner would have to reproduce all three taxonomies through
//! per-caller callbacks that buy no clarity and risk a silent verdict drift, so
//! the consolidation is intentionally declined.
//!
//! Timeout handling: [`run_command`] polls with `try_wait`, keeping the
//! [`std::process::Child`] owned by this thread, so its timeout branch actually
//! kills the child before failing open — closing the Wave-2 timeout-leak
//! Concern that previously orphaned the process (the earlier
//! move-`Child`-into-a-worker-thread shape could not reach it to kill).

use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use mustard_core::domain::model::contract::Verdict;
use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{self, SpecState as _};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::domain::spec::contract::{FindingItem, FindingSource};
use mustard_core::domain::text::{has_word_sequence, is_word_byte, Boundaries};
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::commands::review::finding_collect;
// The destination words come from the door itself, so the remediation this gate
// prints can never name a set `mark-finding` does not accept.
use crate::commands::spec::mark_finding::DESTINATIONS;
use crate::shared::gate_mode::{resolve_mode, GateMode};
use crate::shared::spec_state::DiskSpecState;
use crate::util::format_gate_message;
use mustard_core::time::now_iso8601;

/// Per-command timeout for the build/test stages — 5 minutes
/// (`COMMAND_TIMEOUT_MS` in `close-gate.js`).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Truncation budget for a failure-output snippet (`TRUNCATE_CHARS`).
const TRUNCATE_CHARS: usize = 500;

// ---------------------------------------------------------------------------
// Mode resolution — the shared cascade ([`crate::shared::gate_mode`]) with the
// close family's `strict` default; the QA-composition gate opts into `warn`.
// ---------------------------------------------------------------------------

/// `true` when the QA close-gate is **active** — `MUSTARD_QA_GATE_MODE`
/// resolves to a non-`off` mode (default `strict`). Reuses the shared
/// [`resolve_mode`] cascade `run_close_gates`'s QA sub-gate uses, so the
/// final-wave auto-settle in `emit-pipeline` and the CLOSE gate agree on whether
/// a spec still owes a QA pass before it can be finalized.
pub(crate) fn qa_gate_active() -> bool {
    resolve_mode("MUSTARD_QA_GATE_MODE", None, GateMode::Strict) != GateMode::Off
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
    if has_word_sequence(&lower, &["future", "hook"], Boundaries::WHOLE) {
        return Some("future-hook");
    }
    // `\bnot\s+part\s+of\s+(?:this\s+)?wave\s*\d*\b`.
    if has_not_part_of_wave(&lower) {
        return Some("not-part-of-wave");
    }
    // `\bnot\s+yet\s+implemented\b`.
    if has_word_sequence(&lower, &["not", "yet", "implemented"], Boundaries::WHOLE) {
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

// ---------------------------------------------------------------------------
// Checklist gate
// ---------------------------------------------------------------------------

/// The unmarked `## Checklist` items of the active spec. Returns `(found,
/// unmarked)` — `found=false` means the spec or section is absent (skip).
/// Port of `findUnmarkedChecklistItems`.
///
/// **Wave-plan parent:** a decomposed Full spec is a coordination doc —
/// it carries NO `## Checklist`; the actionable checklists live in each
/// `wave-N-*/` sidecar. If the parent has no checklist section AND it is a
/// wave-plan parent (its `meta.json#isWavePlan`/`totalWaves` says so, or wave
/// subdirs exist), this CONSOLIDATES the wave checklists instead of skipping —
/// otherwise CLOSE would pass having checked nothing (an orphaned gate).
///
/// **Meta-first:** each wave is read from
/// its `meta.json#checklist` (the canonical home seeded by `wave-scaffold` and
/// flipped by the auto-mark hook / `mark-checklist-item`); the wave's markdown
/// `## Checklist` section is the legacy fallback. The parent root meta carries
/// no checklist by design (explicit OUT), so the parent side stays markdown.
pub(crate) fn find_unmarked_checklist(cwd: &str, spec: Option<&str>) -> (bool, Vec<String>) {
    let Some(spec) = spec else {
        return (false, Vec::new());
    };
    let Ok(sp) = ClaudePaths::for_project(Path::new(cwd)).and_then(|p| p.for_spec(spec)) else {
        return (false, Vec::new());
    };
    let spec_path = sp.spec_md_path();

    // First, the parent's own checklist (owning Light / non-decomposed Full).
    if let Ok(raw) = fs::read_to_string(&spec_path)
        && let Some(unmarked) = checklist_unmarked_in(&raw) {
            return (true, unmarked);
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
            // `is_open()` and not `!done`: an item dropped on purpose is
            // settled work, so it must not hold CLOSE hostage as if someone
            // had forgotten it.
            .filter(|i| i.is_open())
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
    // NB: deliberately NOT `spec_sections::section_end` — this gate also treats
    // a bare `##` (no text after the hashes) as a section boundary, a shape the
    // shared scanner does not recognise. Folding it in would change behaviour.
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
    if let Some(meta) = mustard_core::read_meta(&spec_dir.join("meta.json"))
        && (meta.is_wave_plan == Some(true) || meta.total_waves.unwrap_or(0) >= 1) {
            return true;
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
// Findings gate
// ---------------------------------------------------------------------------

/// The findings of `spec` that still owe a destination.
///
/// The collector is run IN-PROCESS rather than read out of the sidecar: a
/// reviewer file or a `removal` column written after the last collection would
/// otherwise be invisible to the gate, which is the pipe-with-no-outlet this
/// sub-gate exists to close. The collection is idempotent and carries every
/// already-declared destination forward verbatim, so re-taking it at CLOSE
/// costs a directory read and settles nothing on its own.
///
/// [`FindingItem::is_open`] and never a plain `routed.is_none()`: a finding
/// deliberately DROPPED, with its reason on the record, is a decision — counting
/// it as forgotten work is the trap the predicate documents.
///
/// Fail-quiet on an unnamed spec: with no spec there is no work unit whose
/// findings could be owed, and this gate must not answer for one it cannot name.
///
/// Fail-quiet on a collection that could not be RECORDED, for a harder reason:
/// the remedy this gate prints would not work. `report.ok` is false exactly when
/// the findings had nowhere to be written — an unresolved spec, a missing or
/// unreadable `meta.json`, a failed write — and `mark-finding` writes to that
/// same sidecar, so refusing here would name a command that answers
/// `meta-not-found` and send the operator in a circle with only
/// `MUSTARD_FINDINGS_GATE_MODE=warn` as a way out. A gate whose remediation
/// cannot succeed is worse than one that stays silent: it teaches the reader
/// that the escape hatch is the normal path. The broken state itself is not
/// invisible — a spec with no readable sidecar is what the surrounding gates and
/// `doctor` already speak about.
fn open_findings(cwd: &str, spec: Option<&str>) -> Vec<FindingItem> {
    let Some(spec) = spec.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let report = finding_collect::collect(Path::new(cwd), spec);
    if !report.ok {
        return Vec::new();
    }
    report
        .findings
        .into_iter()
        .filter(FindingItem::is_open)
        .collect()
}

/// The producer's name as the record spells it — the serde words of
/// [`FindingSource`], so the refusal and the sidecar name the producer
/// identically and a reader can grep one from the other.
const fn source_word(source: FindingSource) -> &'static str {
    match source {
        FindingSource::Review => "review",
        FindingSource::ProofLedger => "proof_ledger",
    }
}

/// The refusal block for ONE open finding: who found it, what it says, and the
/// exact command that settles it.
///
/// The command is printed per finding rather than once at the bottom because a
/// gate that refuses without naming the action teaches the reader to route
/// around it — and the id is the one argument they cannot guess. `--to` and
/// `--reason` stay placeholders on purpose: the destination is the decision this
/// gate is asking for, and pre-filling it would be the gate answering its own
/// question.
fn finding_refusal(spec: &str, finding: &FindingItem) -> String {
    format!(
        "  - [{source}] {id}: {statement}\n    mustard-rt run mark-finding --spec {spec} \
         --id {id} --to <{DESTINATIONS}> --reason \"<why>\"",
        source = source_word(finding.source),
        id = finding.id,
        statement = finding.statement,
    )
}

// ---------------------------------------------------------------------------
// QA gate
// ---------------------------------------------------------------------------

/// What to say instead of "run the command that verifies the criteria": that
/// command has left the flow and refuses at its own door, so the gate hands
/// over its refusal, from the catalog, in the project's language.
fn retired_qa_hint(cwd: &str) -> String {
    crate::commands::retired::hint(Path::new(cwd), "retired.wait_close", &[("{command}", "qa-run")])
}

/// The spec's event file, through the [`SpecState`] port; `None` with no spec
/// or no `spec.ndjson`. The gate reads the criteria runs and the requests from
/// it.
fn spec_log(cwd: &str, spec: Option<&str>) -> Option<SpecLog> {
    let spec = spec.map(str::trim).filter(|s| !s.is_empty())?;
    DiskSpecState::new(Path::new(cwd)).log(spec)
}

/// The change requests recorded after the last run of the criteria
/// (`last_run`): changes the verified criteria may not cover. One short line
/// per request, `(effect) start of the text`.
fn unaddressed_change_requests(log: &SpecLog, last_run: u64) -> Vec<String> {
    spec_state::requests_after(log, last_run)
        .iter()
        .map(|request| {
            let effect = request.str_field("effect").unwrap_or_default();
            let preview: String = request.str_field("text").unwrap_or_default().chars().take(60).collect();
            if effect.is_empty() {
                preview
            } else {
                format!("({effect}) {preview}")
            }
        })
        .collect()
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

    // No native wait-with-timeout in std; spawn + poll so the `Child` stays
    // owned by THIS thread — the only shape in which the timeout branch can
    // actually kill it (the prior move-into-a-worker-thread form surrendered the
    // handle and leaked the process: the Wave-2 timeout-leak Concern). `qa-run`
    // and `verify-pipeline` use this same poll+kill loop. The env-error vs
    // real-failure classification is UNCHANGED: spawn failure / wait error /
    // timeout → env error (fail-open); non-zero exit → real failure; exit 0 → ok.
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        match child.try_wait() {
            // Exited within budget — read the piped output (stdout then stderr,
            // into one buffer, exactly as before) and classify by exit status.
            Ok(Some(status)) => {
                use std::io::Read;
                let mut output = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut output);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_string(&mut output);
                }
                return if status.success() {
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
                };
            }
            // Still running — once the budget is spent, KILL the child and fail
            // open (the JS `status === null` branch). This is the leak fix: the
            // child is owned here, so `kill` reaches it; `wait` then reaps it.
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return CommandResult {
                        ok: false,
                        env_error: true,
                        output: format!(
                            "[timeout after {}ms] {cmd}",
                            COMMAND_TIMEOUT.as_millis()
                        ),
                    };
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            // Wait itself failed → env error.
            Err(err) => {
                return CommandResult {
                    ok: false,
                    env_error: true,
                    output: err.to_string(),
                };
            }
        }
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
    // `close-gate.check` is non-pipeline → per-spec NDJSON via the event router.
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
/// Resolving the five `MUSTARD_*_MODE` env vars once, up front, keeps
/// [`run_close_gates`] a pure function — testable without mutating
/// process-global environment (which the crate's `#![forbid(unsafe_code)]`
/// would otherwise force into an `unsafe` block).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CloseGateModes {
    pub(crate) close: GateMode,
    pub(crate) debt: GateMode,
    pub(crate) checklist: GateMode,
    /// `MUSTARD_FINDINGS_GATE_MODE`. Strict like its siblings, by design: a
    /// findings gate born advisory would repeat, in another shape, the very
    /// defect it exists to close — a discovery recorded where nobody has to
    /// answer for it.
    pub(crate) findings: GateMode,
    pub(crate) qa: GateMode,
}

impl CloseGateModes {
    /// Resolve every sub-gate mode in cascade (env var → `mustard.json`
    /// `gates.<field>` → built-in `strict`) — the production path.
    ///
    /// The project config is loaded once here; only `checklist` carries a
    /// `gates.*` override field today, so the other four resolve env-only.
    pub(crate) fn resolve(cwd: &str) -> Self {
        let gates = crate::shared::context::project_config_cached(Path::new(cwd)).gates;
        Self {
            close: resolve_mode("MUSTARD_CLOSE_GATE_MODE", None, GateMode::Strict),
            debt: resolve_mode("MUSTARD_DEBT_GATE_MODE", None, GateMode::Strict),
            checklist: resolve_mode(
                "MUSTARD_CHECKLIST_GATE_MODE",
                gates.checklist.as_deref(),
                GateMode::Strict,
            ),
            findings: resolve_mode("MUSTARD_FINDINGS_GATE_MODE", None, GateMode::Strict),
            qa: resolve_mode("MUSTARD_QA_GATE_MODE", None, GateMode::Strict),
        }
    }
}

/// Notebook lines of `spec` marked as explaining the operator's reported
/// symptom.
///
/// Empty whenever the notebook cannot be read or carries no such line — this
/// feeds a gate, and a gate that blocks on a file it could not open blocks on
/// nothing it can name.
fn find_symptom_findings(cwd: &str, spec: Option<&str>) -> Vec<String> {
    let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let path = mustard_core::ClaudePaths::spec_dir_or_unchecked(std::path::Path::new(cwd), spec)
        .join("notebook.md");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .map(str::trim)
        .filter(|l| crate::commands::event::notebook::explains_symptom(l))
        .map(str::to_string)
        .collect()
}

/// Run every close-gate sub-gate against an already-resolved `(cwd, spec)`
/// pair — the spec-aware entry point used by `mustard-rt run emit-phase --to
/// CLOSE` and `close-orchestrate`, through [`gate_close_for_spec`]. No JSON
/// dependency, no `HookInput` coupling.
///
/// Returns:
/// - [`Verdict::Allow`] when every gate passes (or every gate is in `off`).
/// - [`Verdict::Deny`] when any strict gate fires.
/// - [`Verdict::Warn`] only for the build/test gate in `warn` mode; the
///   debt/checklist/qa gates degrade to `Allow` in `warn`.
// run_close_gates is a sequential gate pipeline; splitting would require threading
// many local mode/spec variables through helpers without clarity gain.
#[allow(clippy::too_many_lines)]
pub(crate) fn run_close_gates(cwd: &str, spec_ref: Option<&str>, modes: CloseGateModes) -> Verdict {
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

    // ── Explains-the-symptom gate ─────────────────────────────────────────
    //
    // A notebook item marked `--explains-symptom` is not the next cycle's
    // prompt: it is the answer to the request that is still in flight. Closing
    // the unit with one open means the operator asked why X happens, the work
    // found out, and the unit shipped something else while the answer sat in a
    // file nobody reads.
    //
    // Measured on this repository, 2026-08-26: the finding that explained the
    // reported symptom ("the plugin's `bin/` is born empty") was recorded in
    // the notebook, correctly, and classified out of scope by the model. A day
    // of work went into the other half of the problem. Nothing asked.
    //
    // Rides the checklist mode, deliberately: it is the same promise — an item
    // the unit acknowledged and did not settle blocks the close — and a knob of
    // its own would be one more thing to discover.
    if modes.checklist != GateMode::Off {
        let open = find_symptom_findings(cwd, spec_ref);
        if !open.is_empty() {
            let preview = open
                .iter()
                .take(3)
                .map(|i| format!("  - {i}"))
                .collect::<Vec<_>>()
                .join("\n");
            let extra = if open.len() > 3 {
                format!("\n  …and {} more", open.len() - 3)
            } else {
                String::new()
            };
            let reason = format!(
                "{}\n{preview}{extra}",
                format_gate_message(
                    "Close Gate",
                    &format!(
                        "the notebook of \"{}\" carries {} finding(s) marked as EXPLAINING \
                         THE REPORTED SYMPTOM",
                        spec_ref.unwrap_or(""),
                        open.len()
                    ),
                    "a finding that explains what the operator reported is the answer to the \
                     request in flight, not a note for later — closing over it decides, on \
                     their behalf, that the work goes elsewhere",
                    "settle it in this unit, or ask the operator whether to open one for it \
                     and remove the marker from the notebook line once they answer",
                )
            );
            if modes.checklist == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    spec_ref,
                    json!({
                        "result": "deny-explains-symptom",
                        "mode": mode_str(mode),
                        "spec": spec_ref,
                        "findingCount": open.len(),
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
                         --spec {spec} --item \"<text>\"`; for work you decided \
                         NOT to do, record the decision instead of marking it \
                         done: `mustard-rt run mark-checklist-item --spec \
                         {spec} --item \"<text>\" --drop --reason \"<why>\"`. \
                         Or set MUSTARD_CHECKLIST_GATE_MODE=warn",
                        spec = spec_ref.unwrap_or("")
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

    // ── Findings gate ─────────────────────────────────────────────────────
    let findings_mode = modes.findings;
    if findings_mode != GateMode::Off {
        let open = open_findings(cwd, spec_ref);
        if !open.is_empty() {
            let spec = spec_ref.unwrap_or("");
            let preview = open
                .iter()
                .take(5)
                .map(|finding| finding_refusal(spec, finding))
                .collect::<Vec<_>>()
                .join("\n");
            let extra_findings = if open.len() > 5 {
                format!("\n  …and {} more", open.len() - 5)
            } else {
                String::new()
            };
            let reason = format!(
                "{}\n{preview}{extra_findings}",
                format_gate_message(
                    "Close Gate",
                    &format!(
                        "{} finding(s) of spec \"{spec}\" still have no destination",
                        open.len()
                    ),
                    "a finding nobody decided about is not a recorded finding, it is \
                     forgotten work",
                    "declare each one with the `mustard-rt run mark-finding` command printed \
                     under it; a finding you deliberately let go IS settled, by `--to dropped \
                     --reason \"<why>\"`. Or set MUSTARD_FINDINGS_GATE_MODE=warn",
                )
            );
            if findings_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    spec_ref,
                    json!({
                        "result": "deny-findings-open",
                        "mode": mode_str(mode),
                        "findingsMode": mode_str(findings_mode),
                        "spec": spec_ref,
                        "openCount": open.len(),
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through.
        }
    }

    // ── QA gate ───────────────────────────────────────────────────────────
    // The recorded QA comes from the spec's `spec.ndjson`: the last run of
    // each criterion. No file, or criteria and no run at all, is no QA.
    let qa_mode = modes.qa;
    let log = spec_log(cwd, spec_ref);
    let recorded = log.as_ref().map(spec_state::qa).filter(|qa| qa.criteria == 0 || qa.ran > 0);
    if qa_mode != GateMode::Off {
        let failed_count = recorded.as_ref().map_or(0, |qa| qa.failed);
        let criteria_count = recorded.as_ref().map_or(0, |qa| qa.criteria);
        let unrun = recorded.as_ref().map_or(0, |qa| qa.criteria - qa.ran);
        let stale_count = recorded.as_ref().map_or(0, |qa| qa.stale);
        let passed = recorded.as_ref().is_some_and(spec_state::Qa::passed_all);
        if recorded.is_none() {
            let reason = format_gate_message(
                "Close Gate",
                &spec_ref.map_or_else(
                    || "no QA pass recorded".to_string(),
                    |s| format!("no QA pass recorded for spec \"{s}\""),
                ),
                "CLOSE requires the acceptance criteria to be verified",
                // The command that used to verify them refuses at the door,
                // so the remedy is the one its own refusal gives — never
                // "run it", which only earns a second refusal.
                &retired_qa_hint(cwd),
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
        } else if failed_count == 0 && !passed {
            // A criterion with no run is never a verification, so BOTH shapes
            // are refused. They are still told apart, because the operator's
            // next move is opposite in each case:
            // - no criterion at all → the spec declares nothing to verify, so
            //   it has nothing to claim. The remedy is to AUTHOR a criterion.
            // - criteria exist and some never ran → the remedy is to FIX and
            //   run them.
            //
            // All THREE fields differ per shape, not just the remedy: the two
            // are refused for different reasons, so a merged principle would
            // hand one shape the other's explanation.
            let (problem, principle, remedy) = if criteria_count > 0 {
                (
                    spec_ref.map_or_else(
                        || format!("QA left {unrun} of {criteria_count} acceptance criteria without a run"),
                        |s| {
                            format!(
                                "QA for spec \"{s}\" left {unrun} of {criteria_count} acceptance \
                                 criteria without a run"
                            )
                        },
                    ),
                    "a skip is not a verification — the criteria exist but were never exercised",
                    "fix the AC commands and re-run qa-run — or record the verdict from an \
                     EXTERNAL `mustard-rt run qa-run`, which is the run that can actually attempt \
                     them; confirm with the user and set MUSTARD_QA_GATE_MODE=warn to close anyway",
                )
            } else {
                (
                    spec_ref.map_or_else(
                        || "QA has nothing to run: the spec declares no acceptance criteria"
                            .to_string(),
                        |s| {
                            format!(
                                "QA for spec \"{s}\" has nothing to run: the spec declares no \
                                 acceptance criteria at all"
                            )
                        },
                    ),
                    "a skip is not a verification — and a spec with nothing to verify has nothing \
                     to claim",
                    "author one criterion — a command that is red before the work and green after \
                     — then re-run `mustard-rt run qa-run`; or confirm with the user and set \
                     MUSTARD_QA_GATE_MODE=warn to close anyway",
                )
            };
            let reason = format_gate_message("Close Gate", &problem, principle, remedy);
            if qa_mode == GateMode::Strict {
                emit_close_gate_event(
                    cwd,
                    spec_ref,
                    json!({
                        "result": "deny-qa-skip",
                        "mode": mode_str(mode),
                        "qaMode": mode_str(qa_mode),
                        "spec": spec_ref,
                        "criteriaCount": criteria_count,
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through, both shapes alike: the mode is the operator's
            // deliberate override and keeps the meaning it always had.
        } else if !passed {
            let failed_str = format!("{failed_count} criteria failed");
            let reason = format_gate_message(
                "Close Gate",
                &spec_ref.map_or_else(
                    || format!("QA did not pass ({failed_str})"),
                    |s| format!("QA failed for spec \"{s}\": {failed_str}"),
                ),
                "CLOSE requires every acceptance criterion to pass",
                "fix the failing criteria and re-run `mustard-rt run qa-run`, or set \
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
                        "failedCount": failed_count,
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through.
        } else if stale_count > 0 {
            // QA passed, but a criterion was revised AFTER its last run: the
            // green was observed against the text of before. Hold CLOSE until
            // the criteria run again.
            let reason = format_gate_message(
                "Close Gate",
                &spec_ref.map_or_else(
                    || format!("QA pass is stale — {stale_count} acceptance criteria were revised after their last run"),
                    |s| {
                        format!(
                            "QA pass for spec \"{s}\" is stale — {stale_count} acceptance criteria \
                             were revised after their last run"
                        )
                    },
                ),
                "an edit to the spec / acceptance criteria after QA means the pass was \
                 never re-verified",
                "re-run `mustard-rt run qa-run` to re-verify the current criteria, or set \
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
                        "staleCount": stale_count,
                    }),
                );
                return Verdict::Deny { reason };
            }
            // warn → fall through.
        }
        // QA passed (and fresh) → fall through.
    }

    // ── QA composition gate — unaddressed mid-spec change requests ────────
    // A `request` recorded AFTER the last run of the criteria is a mid-spec
    // request the verified criteria may not cover (a behaviour change not yet
    // folded into a criterion). Surface it at CLOSE so it is consciously
    // triaged. Default `warn` (telemetry + dashboard only); `strict` blocks.
    // Only meaningful once a run exists; a missing QA is already caught by the
    // QA gate above.
    let composition_mode = resolve_mode("MUSTARD_QA_COMPOSITION_GATE_MODE", None, GateMode::Warn);
    if composition_mode != GateMode::Off {
        let last_run = recorded.as_ref().and_then(|qa| qa.last_run);
        if let (Some(log), Some(last_run)) = (log.as_ref(), last_run) {
            let pending = unaddressed_change_requests(log, last_run);
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
                     `mustard-rt run qa-run`, or set MUSTARD_QA_COMPOSITION_GATE_MODE=warn",
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

    // ── Build/test gate ───────────────────────────────────────────────────
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
/// This is the entry point used by `mustard-rt run emit-phase --to CLOSE` and
/// by `close-orchestrate`.
pub fn gate_close_for_spec(cwd: &str, spec: &str) -> Result<(), String> {
    let modes = CloseGateModes::resolve(cwd);
    match run_close_gates(cwd, Some(spec), modes) {
        Verdict::Deny { reason } => Err(reason),
        // Warn → advisory only (CLOSE proceeds). Allow / others → ok.
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {

    /// A notebook finding marked as explaining the reported symptom BLOCKS the
    /// close; an ordinary one does not.
    ///
    /// This is the gate that did not exist on 2026-08-26, when the finding that
    /// explained the operator's symptom was recorded correctly, classified out
    /// of scope by the model, and a day of work went into the other half of the
    /// problem. Nothing asked.
    #[test]
    fn a_finding_that_explains_the_symptom_blocks_the_close() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("uma-unidade");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let cwd = root.to_str().unwrap();

        // An ordinary finding: the next cycle's prompt, and no reason to block.
        std::fs::write(
            spec_dir.join("notebook.md"),
            "# Notebook\n\n- o executor confunde dois casos de 127\n",
        )
        .unwrap();
        assert!(
            find_symptom_findings(cwd, Some("uma-unidade")).is_empty(),
            "an adjacent finding must not block",
        );

        // One that explains what the operator reported: the answer to the
        // request in flight.
        std::fs::write(
            spec_dir.join("notebook.md"),
            format!(
                "# Notebook\n\n- o executor confunde dois casos de 127\n- {} o bin do plugin nasce vazio\n",
                crate::commands::event::notebook::EXPLAINS_SYMPTOM
            ),
        )
        .unwrap();
        let open = find_symptom_findings(cwd, Some("uma-unidade"));
        assert_eq!(open.len(), 1, "exactly the marked one: {open:?}");
        assert!(open[0].contains("bin do plugin"), "{open:?}");

        // Unreadable or unnamed: a gate that blocks on a file it could not open
        // blocks on nothing it can name.
        assert!(find_symptom_findings(cwd, Some("nao-existe")).is_empty());
        assert!(find_symptom_findings(cwd, None).is_empty());
    }
    use super::*;
    use crate::shared::spec_state::{seed_event, seed_request, seed_run, seed_runs};
    use tempfile::tempdir;

    /// Build a project dir with the standard `.claude` subtree.
    fn make_project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        std::fs::create_dir_all(paths.harness_dir()).unwrap();
        std::fs::create_dir_all(paths.spec_dir())
            .unwrap();
        dir
    }

    fn write_spec(cwd: &Path, spec_name: &str, body: &str) {
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec(spec_name).unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), body).unwrap();
    }

    fn write_mustard_json(cwd: &Path, fields: Value) {
        std::fs::write(cwd.join("mustard.json"), fields.to_string()).unwrap();
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
            findings: GateMode::Strict,
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

    /// Only the QA sub-gate on. With no `mustard.json`, the build/test stage
    /// fail-open skips, so a verdict here is the QA's alone.
    fn only_qa() -> CloseGateModes {
        CloseGateModes {
            debt: GateMode::Off,
            checklist: GateMode::Off,
            findings: GateMode::Off,
            ..all_strict()
        }
    }

    /// Every door that asks "did the QA pass?" gives the same answer, side by
    /// side, reading the `spec.ndjson`: the close gates, `emit-pipeline`,
    /// `complete-spec`, the composite close, the pull-request advisory and the
    /// resume. No file, a failed criterion, a criterion never run, and every
    /// criterion passed. `close-pipeline` has its own side-by-side test: it
    /// runs the criteria and decides on the run it just made.
    #[test]
    fn every_close_gate_reads_the_same_qa_from_the_spec_file() {
        // The pull-request door names the spec by the ladder; an inherited
        // override would answer first.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        // (case, seeded results, the QA passes)
        type Case<'a> = (&'a str, Option<&'a [Option<&'a str>]>, bool);
        let cases: [Case; 4] = [
            ("no spec file", None, false),
            ("a failed criterion", Some(&[Some("pass"), Some("fail")]), false),
            ("a criterion never run", Some(&[Some("pass"), None]), false),
            ("every criterion passed", Some(&[Some("pass"), Some("pass")]), true),
        ];
        for (case, results, passes) in cases {
            let dir = make_project();
            let root = dir.path();
            let cwd = root.to_str().unwrap();
            let spec = "lado";
            crate::shared::spec_state::stand_on_spec_branch(root, spec);
            if let Some(results) = results {
                seed_runs(root, spec, results);
            }
            let doors = [
                ("close-gates", !run_close_gates(cwd, Some(spec), only_qa()).is_blocking()),
                ("emit-pipeline", crate::commands::event::emit_pipeline::qa_result_passed(root, spec)),
                ("complete-spec", crate::commands::spec::complete_spec::close_admission(root, spec).is_ok()),
                ("close-orchestrate", crate::commands::pipeline::close_orchestrate::qa_gate_passes(root, spec)),
                ("pr-qa-gate", crate::hooks::bash::pr_qa_gate::pr_qa_gate("gh pr merge 80", cwd).is_none()),
                (
                    "resume",
                    crate::commands::pipeline::resume_bootstrap::post_execute_gate::read_review_qa_state(root, spec).0,
                ),
            ];
            for (door, answer) in doors {
                assert_eq!(answer, passes, "the {door} door disagrees with {case}");
            }
        }
    }

    /// A criterion revised after its last run makes the QA stale: the close
    /// refuses until the criterion runs again, and the new run releases it.
    #[test]
    fn a_criterion_revised_after_its_run_makes_the_pass_stale() {
        let dir = make_project();
        let root = dir.path();
        let cwd = root.to_str().unwrap();
        let criteria = seed_runs(root, "feat", &[Some("pass")]);
        assert!(!run_close_gates(cwd, Some("feat"), only_qa()).is_blocking(), "a fresh pass closes");

        let revised = seed_event(
            root,
            "feat",
            "criterion",
            json!({ "when": "a obra roda", "then": "o critério novo confere", "proof": "cargo test", "replaces": criteria[0] }),
        );
        match run_close_gates(cwd, Some("feat"), only_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("stale"), "{reason}"),
            other => panic!("a pass older than its criterion must not close, got {other:?}"),
        }

        seed_run(root, "feat", revised, "pass");
        assert!(!run_close_gates(cwd, Some("feat"), only_qa()).is_blocking(), "the new run closes again");
    }

    /// A request recorded after the last run of the criteria is named at the
    /// close; one recorded before it is not.
    #[test]
    fn a_request_after_the_last_run_is_named_at_close() {
        let dir = make_project();
        let root = dir.path();
        seed_request(root, "feat", "antes do QA");
        seed_runs(root, "feat", &[Some("pass")]);
        seed_request(root, "feat", "depois do QA");

        let log = spec_log(root.to_str().unwrap(), Some("feat")).expect("the spec file is there");
        let last_run = spec_state::qa(&log).last_run.expect("a run was recorded");
        let pending = unaddressed_change_requests(&log, last_run);
        assert_eq!(pending.len(), 1, "only the request after the run is named: {pending:?}");
        assert!(pending[0].contains("depois do QA"), "{pending:?}");
        assert!(pending[0].contains("adjust_waves"), "the effect travels with it: {pending:?}");
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
        seed_runs(dir.path(), "spec-ok", &[Some("pass")]);
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

    /// Both skip shapes are refused — and each is told apart, pointed at its OWN
    /// next move.
    ///
    /// The refusals were merged into one branch when the empty-criteria
    /// carve-out was retired, and that is exactly where a fix like this goes
    /// wrong: collapsing two situations into one message leaves the operator a
    /// prohibition with the wrong remedy attached. A spec that declares nothing
    /// needs a criterion AUTHORED; criteria that exist but were never attempted
    /// need to be FIXED, or their verdict recorded by a run that can attempt
    /// them. So the assertion is on the remedy each one carries, not on the fact
    /// that both are denied.
    #[test]
    fn the_two_skip_shapes_are_refused_with_their_own_remedy() {
        let empty = deny_reason_for_skip(&[]);
        let with_acs = deny_reason_for_skip(&[Some("pass"), None]);

        assert!(
            empty.contains("author"),
            "a spec declaring nothing must be told to author a criterion: {empty}"
        );
        assert!(
            with_acs.contains("fix the AC commands"),
            "criteria that exist must be told to fix or re-run them: {with_acs}"
        );
        assert_ne!(
            empty, with_acs,
            "one message for two opposite situations is the failure this guards"
        );
    }

    // --- findings gate ------------------------------------------------------

    /// Only the findings sub-gate active, so the assertions below are about it
    /// and not about a QA pass nobody recorded. No `mustard.json` is written in
    /// these tests, so the build/test stage fail-open skips.
    fn only_findings() -> CloseGateModes {
        CloseGateModes {
            debt: GateMode::Off,
            checklist: GateMode::Off,
            qa: GateMode::Off,
            ..all_strict()
        }
    }

    /// Seed a spec whose reviewer left one findings file behind — a discovery
    /// on disk that nobody has decided anything about.
    fn seed_reviewed_spec(cwd: &Path, spec: &str) -> std::path::PathBuf {
        write_spec(cwd, spec, "# Spec\n");
        let sp = ClaudePaths::for_project(cwd).unwrap().for_spec(spec).unwrap();
        let spec_dir = sp.dir().to_path_buf();
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(spec_dir.join("review")).unwrap();
        std::fs::write(
            spec_dir.join("review").join("findings.md"),
            "# Findings\n\n- the close gate never reads this file\n",
        )
        .unwrap();
        spec_dir
    }

    /// A finding with no destination refuses CLOSE, and the refusal names the
    /// producer, the statement AND the exact command that settles it — a gate
    /// that refuses without naming the action teaches the reader to route
    /// around it.
    #[test]
    fn findings_gate_denies_open_finding_and_names_the_command() {
        let dir = make_project();
        seed_reviewed_spec(dir.path(), "found");

        let verdict = run_close_gates(dir.path().to_str().unwrap(), Some("found"), only_findings());
        let Verdict::Deny { reason } = verdict else {
            panic!("an undecided finding must refuse CLOSE, got {verdict:?}");
        };
        assert!(reason.contains("[review] F-findings"), "{reason}");
        assert!(reason.contains("the close gate never reads this file"), "{reason}");
        assert!(
            reason.contains("mustard-rt run mark-finding --spec found --id F-findings"),
            "the refusal must carry the exact command that resolves it: {reason}"
        );
        assert!(reason.contains("MUSTARD_FINDINGS_GATE_MODE=warn"), "{reason}");
    }

    /// The same tree, once the destination is declared through the `mark-finding`
    /// door: the gate has nothing left to hold. A finding deliberately DROPPED
    /// counts as settled — that is the whole difference between a decision and a
    /// forgotten discovery.
    #[test]
    fn findings_gate_allows_when_every_finding_routed_including_dropped() {
        let dir = make_project();
        let spec_dir = seed_reviewed_spec(dir.path(), "found");
        let cwd = dir.path().to_str().unwrap();

        // The first run seeds `meta.json#findings` (the collector runs in-process).
        assert!(
            run_close_gates(cwd, Some("found"), only_findings()).is_blocking(),
            "precondition: the finding starts open"
        );

        // The id is asked of the collection rather than spelled here: a finding
        // is identified by its discovery, so its id carries a fingerprint of
        // what was found.
        let open = open_findings(cwd, Some("found"));
        let [finding] = open.as_slice() else {
            panic!("exactly one open finding was seeded, got {open:?}");
        };
        assert_eq!(
            crate::commands::spec::mark_finding::mark(
                dir.path(),
                spec_dir.to_str().unwrap(),
                &finding.id,
                mustard_core::domain::spec::contract::FindingRoute::Dropped(
                    "already covered by AC-2".to_string()
                ),
            ),
            Ok(crate::commands::spec::mark_finding::MarkFindingOutcome::Routed)
        );

        let verdict = run_close_gates(cwd, Some("found"), only_findings());
        assert!(!verdict.is_blocking(), "a decided finding holds nothing: {verdict:?}");
    }

    /// A spec whose producers wrote nothing collects nothing, and the gate has
    /// no opinion — a work unit that made no discoveries is not a defect.
    #[test]
    fn findings_gate_is_silent_without_producers() {
        let dir = make_project();
        write_spec(dir.path(), "quiet", "# Spec\n");
        assert!(open_findings(dir.path().to_str().unwrap(), Some("quiet")).is_empty());
        assert!(open_findings(dir.path().to_str().unwrap(), None).is_empty());
        let verdict = run_close_gates(dir.path().to_str().unwrap(), Some("quiet"), only_findings());
        assert!(!verdict.is_blocking(), "{verdict:?}");
    }

    /// A collection that could not be RECORDED never refuses CLOSE — because the
    /// refusal would name a command that cannot succeed.
    ///
    /// The shape: a spec directory carrying a reviewer's findings file but no
    /// readable `meta.json`. The findings are real, but they have nowhere to be
    /// written, so `mark-finding` — the one remedy the refusal prints — answers
    /// `meta-not-found` too. Refusing here left the operator circling between two
    /// commands that both fail, with `MUSTARD_FINDINGS_GATE_MODE=warn` as the
    /// only exit; three consecutive reviews reported it before it was settled.
    #[test]
    fn findings_gate_stays_quiet_when_the_collection_could_not_be_recorded() {
        let dir = make_project();
        let cwd = dir.path().to_str().unwrap();
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("orphan").unwrap();
        std::fs::create_dir_all(sp.dir().join("review")).unwrap();
        std::fs::write(sp.spec_md_path(), "# Spec\n").unwrap();
        std::fs::write(
            sp.dir().join("review").join("findings.md"),
            "## MAJOR — something real was found here\n",
        )
        .unwrap();
        // No `meta.json` is written: the collection has nowhere to land.
        assert!(
            !std::fs::exists(sp.dir().join("meta.json")).unwrap_or(false),
            "the shape under test is a spec dir with no readable sidecar"
        );

        assert!(
            open_findings(cwd, Some("orphan")).is_empty(),
            "an unrecordable collection must not be read as owed work"
        );
        let verdict = run_close_gates(cwd, Some("orphan"), only_findings());
        assert!(
            !verdict.is_blocking(),
            "a gate whose remedy cannot succeed must not refuse: {verdict:?}"
        );
    }

    /// Run the close gates over a spec whose `spec.ndjson` carries one
    /// criterion per item of `results` (`None` = never run) and no failure, and
    /// return the refusal reason. Panics when the gate does NOT deny — which is
    /// the other half of the assertion.
    fn deny_reason_for_skip(results: &[Option<&str>]) -> String {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        let spec = "skip-shape-spec";
        // The spec file exists even with no criterion in it.
        seed_event(dir.path(), spec, "message", json!({ "author": "user", "text": "oi" }));
        seed_runs(dir.path(), spec, results);
        match run_close_gates(dir.path().to_str().unwrap(), Some(spec), all_strict()) {
            Verdict::Deny { reason } => reason,
            other => panic!("a skip must never open the close, got {other:?}"),
        }
    }

    // --- build/test modes -----------------------------------------------------

    /// A failing build in `warn` mode is advice, never a refusal.
    #[test]
    fn the_build_gate_in_warn_mode_warns_and_never_denies() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let modes = CloseGateModes { close: GateMode::Warn, ..no_qa() };
        let verdict = run_close_gates(dir.path().to_str().unwrap(), Some("warn-spec"), modes);
        assert!(matches!(verdict, Verdict::Warn { .. }), "warn mode must not deny: {verdict:?}");
    }

    /// The build gate `off` never refuses: a failing command is at most
    /// reported.
    #[test]
    fn the_build_gate_off_never_refuses() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_fail() }));
        let modes = CloseGateModes { close: GateMode::Off, ..no_qa() };
        let verdict = run_close_gates(dir.path().to_str().unwrap(), Some("off-spec"), modes);
        assert!(!verdict.is_blocking(), "off must not deny: {verdict:?}");
    }

    /// With no `mustard.json` there is no command to run: the build gate
    /// fails open.
    #[test]
    fn the_build_gate_fails_open_without_mustard_json() {
        let dir = make_project();
        assert_eq!(run_close_gates(dir.path().to_str().unwrap(), Some("spec2"), no_qa()), Verdict::Allow);
    }

    // --- QA modes -------------------------------------------------------------

    /// Under `warn`, both skip shapes fall through: the mode is the operator's
    /// deliberate override and keeps the meaning it always had.
    #[test]
    fn warn_mode_still_lets_both_skip_shapes_through() {
        let cases: [(&str, &[Option<&str>]); 2] =
            [("warn-skip-empty", &[]), ("warn-skip-with-acs", &[Some("pass"), None])];
        for (spec, results) in cases {
            let dir = make_project();
            write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
            seed_event(dir.path(), spec, "message", json!({ "author": "user", "text": "oi" }));
            seed_runs(dir.path(), spec, results);
            let modes = CloseGateModes { qa: GateMode::Warn, ..all_strict() };
            let verdict = run_close_gates(dir.path().to_str().unwrap(), Some(spec), modes);
            assert!(!verdict.is_blocking(), "warn must still let {spec} through: {verdict:?}");
        }
    }

    /// With the QA sub-gate off, a missing QA refuses nothing.
    #[test]
    fn qa_off_does_not_deny_a_missing_qa() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass() }));
        assert!(!run_close_gates(dir.path().to_str().unwrap(), Some("off-qa-spec"), no_qa()).is_blocking());
    }

    // --- checklist gate -------------------------------------------------------

    #[test]
    fn an_unmarked_checklist_denies_the_close() {
        let dir = make_project();
        write_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [x] first done\n- [ ] second open\n\
             - [ ] third open\n\n## Notes\n",
        );
        match run_close_gates(dir.path().to_str().unwrap(), Some("demo"), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("2 unmarked"), "{reason}"),
            other => panic!("expected Deny for unmarked checklist, got {other:?}"),
        }
    }

    /// A wave-plan parent has no `## Checklist` of its own, so the gate
    /// consolidates the WAVE checklists: an unmarked wave item refuses.
    #[test]
    fn a_wave_plan_parent_consolidates_its_wave_checklists() {
        let dir = make_project();
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
        std::fs::write(sp.dir().join("wave-1-general").join("spec.md"), "# Wave 1\n\n## Checklist\n- [x] done\n")
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

        match run_close_gates(dir.path().to_str().unwrap(), Some("epic"), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("unmarked"), "{reason}"),
            other => panic!("expected Deny for unmarked wave checklist, got {other:?}"),
        }
    }

    /// The wave's `meta.json#checklist` is the source the gate reads: a
    /// `done:false` item refuses even under a stale all-marked markdown, and
    /// flipping every `done` to `true` releases it.
    #[test]
    fn a_wave_meta_checklist_blocks_until_every_item_is_done() {
        let dir = make_project();
        let cwd = dir.path().to_str().unwrap();
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
        std::fs::write(wave_dir.join("spec.md"), "# Wave 1\n\n## Checklist\n- [x] stale markdown item\n").unwrap();
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"epic-meta","checklist":[{"label":"src/a.rs","path":"src/a.rs","done":true},{"label":"src/b.rs","path":"src/b.rs","done":false}]}"#,
        )
        .unwrap();

        let (found, unmarked) = find_unmarked_checklist(cwd, Some("epic-meta"));
        assert!(found, "wave meta checklist must be consolidated");
        assert_eq!(unmarked.len(), 1, "one done:false item: {unmarked:?}");
        assert!(unmarked[0].contains("src/b.rs"), "{unmarked:?}");
        assert!(unmarked[0].contains("wave-1-rt"), "wave label prefix: {unmarked:?}");
        match run_close_gates(cwd, Some("epic-meta"), no_qa()) {
            Verdict::Deny { reason } => assert!(reason.contains("unmarked"), "{reason}"),
            other => panic!("expected Deny for done:false wave meta item, got {other:?}"),
        }

        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"epic-meta","checklist":[{"label":"src/a.rs","path":"src/a.rs","done":true},{"label":"src/b.rs","path":"src/b.rs","done":true}]}"#,
        )
        .unwrap();
        let (found, unmarked) = find_unmarked_checklist(cwd, Some("epic-meta"));
        assert!(found);
        assert!(unmarked.is_empty(), "all done:true → release: {unmarked:?}");
        assert_eq!(run_close_gates(cwd, Some("epic-meta"), no_qa()), Verdict::Allow);
    }

    /// A wave-plan parent whose waves are all fully marked closes: nothing
    /// orphaned, no false refusal.
    #[test]
    fn a_wave_plan_with_every_wave_marked_closes() {
        let dir = make_project();
        let cwd = dir.path().to_str().unwrap();
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("epic2").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Epic2\n\n## Network\n- coord\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":1}"#,
        )
        .unwrap();
        std::fs::create_dir_all(sp.dir().join("wave-1-general")).unwrap();
        std::fs::write(sp.dir().join("wave-1-general").join("spec.md"), "# Wave 1\n\n## Checklist\n- [x] done\n")
            .unwrap();

        let (found, unmarked) = find_unmarked_checklist(cwd, Some("epic2"));
        assert!(found);
        assert!(unmarked.is_empty(), "all waves marked → no unmarked items: {unmarked:?}");
        assert_eq!(run_close_gates(cwd, Some("epic2"), no_qa()), Verdict::Allow);
    }

    /// An item dropped on purpose, with a stated reason in the sidecar, is
    /// settled work: it neither refuses the close nor reads as forgotten.
    #[test]
    fn a_dropped_checklist_item_is_not_unmarked_work() {
        let dir = make_project();
        let cwd = dir.path().to_str().unwrap();
        let sp = ClaudePaths::for_project(dir.path()).unwrap().for_spec("epic-drop").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(sp.spec_md_path(), "# Epic\n\n## Network\n- coord\n").unwrap();
        std::fs::write(
            sp.dir().join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","isWavePlan":true,"totalWaves":1}"#,
        )
        .unwrap();
        let wave_dir = sp.dir().join("wave-1-rt");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(wave_dir.join("spec.md"), "# Wave 1\n").unwrap();
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"epic-drop","checklist":[{"label":"src/a.rs","path":"src/a.rs","done":true},{"label":"src/b.rs","path":"src/b.rs","done":false,"dropped":"folded into src/a.rs"}]}"#,
        )
        .unwrap();

        let (found, unmarked) = find_unmarked_checklist(cwd, Some("epic-drop"));
        assert!(found, "the gate still has something to enforce");
        assert!(unmarked.is_empty(), "a dropped item is not unmarked work: {unmarked:?}");
        assert_eq!(run_close_gates(cwd, Some("epic-drop"), no_qa()), Verdict::Allow);
    }

    #[test]
    fn a_fully_marked_checklist_passes() {
        let dir = make_project();
        write_spec(dir.path(), "demo", "# Spec\n\n## Checklist\n\n- [x] first\n- [x] second\n\n## Notes\n");
        // No mustard.json → after the checklist gate passes, the build gate skips.
        assert_eq!(run_close_gates(dir.path().to_str().unwrap(), Some("demo"), no_qa()), Verdict::Allow);
    }

    // --- the check event -------------------------------------------------------

    /// Every run of the close gates records its `close-gate.check` event.
    #[test]
    fn the_close_gates_record_their_check_event() {
        let dir = make_project();
        write_mustard_json(dir.path(), json!({ "testCommand": exit_pass(), "buildCommand": exit_pass() }));
        let _ = run_close_gates(dir.path().to_str().unwrap(), Some("spec-event"), no_qa());

        // The event lands in the spec's event folder, or in a session folder
        // when the spec cannot be attributed.
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
        let found = candidate_dirs.iter().filter(|d| d.exists()).any(|d| {
            std::fs::read_dir(d).unwrap().any(|f| {
                std::fs::read_to_string(f.unwrap().path())
                    .unwrap_or_default()
                    .lines()
                    .any(|l| l.contains("\"event\":\"close-gate.check\""))
            })
        });
        assert!(found, "close-gate.check NDJSON line must be present");
    }
}
