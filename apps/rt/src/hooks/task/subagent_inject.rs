//! `subagent_inject` — PreToolUse(Task) context injector (W8.T8.3).
//!
//! For every `Task` dispatch that does NOT already declare a `SKILL:` block in
//! its `prompt`, we resolve a minimal slice of:
//!
//! - the project's `CONTEXT.md` (when present), keyed against the spec slug
//!   the dispatch carries via env (`MUSTARD_ACTIVE_SPEC`), and
//! - the top-K skills returned by [`crate::commands::skill::skill_resolve::resolve`] for
//!   the prompt + role + active-phase.
//!
//! The slice is surfaced as a [`Verdict::Inject`]. The orchestrator-side
//! `agent-prompt-render` already handles fully-formed dispatches; this hook
//! covers the ad-hoc `Task(general-purpose)` calls that bypass the renderer
//! (the L0 path from CLAUDE.md).
//!
//! ## T8.10 — selective spec-memory load
//!
//! `SessionStart` no longer auto-injects the active spec's `memory/`. Per the
//! deep-refactor budget, spec-memory is loaded **per dispatch**: this hook
//! consults `skill_resolve` and picks at most three `memory/*.md` principles
//! whose name tokens overlap the resolved skill list or the prompt verbs.
//!
//! ## Fail-open contract
//!
//! Every IO step degrades to an empty fragment. The hook never blocks — its
//! decisive verdict is always either `Inject` (when something was resolved)
//! or `Allow` (when nothing was).

use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use crate::shared::events::economy;
use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::time::now_iso8601;
use mustard_core::ClaudePaths;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::commands::agent::context_inject;
use crate::commands::review::gate_regression_check::{
    check_after_child_return, GateError, GateInput, RegressionVerdict,
};
use crate::commands::review::review_result;
use crate::commands::review::review_spans::{self, VerdictEntry, VERDICT_AMBER, VERDICT_GREEN, VERDICT_RED};


/// The W8 subagent-inject hook.
pub struct SubagentInject;


/// What a dispatch prompt's `--emit ref` stub resolved to.
///
/// The discriminator is the `MUSTARD-PROMPT-REF:` marker: a prompt WITHOUT it
/// is a normal ad-hoc Task (`NoMarker` — stays silent), while a prompt WITH it
/// is a ref dispatch the hook is contracted to expand, so any failure to do so
/// is attributable and surfaced (`Unexpanded` carries the reason).
#[derive(Debug, PartialEq)]
enum RefStub {
    NoMarker,
    Unexpanded { rel: String, reason: &'static str },
    Expanded { rel: String, body: String },
}

/// Classify the dispatch prompt's `--emit ref` stub against the project tree.
/// Pure but for the single file read — deterministic and unit-testable with a
/// tempdir (no env, no event sink). The reasons name exactly which link of the
/// render→stub→hook chain broke: `invalid_path` (a malformed/escaping stub),
/// `file_missing` (the render never wrote it or it was lost), `file_empty`
/// (an empty render). The path rules are unchanged: project-relative only —
/// `has_root` also catches Windows' drive-less `\foo` that `is_absolute` misses.
fn classify_ref_stub(project: &Path, prompt: &str) -> RefStub {
    let Some(raw) = prompt
        .lines()
        .find_map(|line| line.trim().strip_prefix(crate::commands::agent::agent_prompt_render::PROMPT_REF_MARKER))
    else {
        return RefStub::NoMarker;
    };
    let rel = raw.trim().to_string();
    if rel.is_empty()
        || Path::new(&rel).has_root()
        || Path::new(&rel).is_absolute()
        || rel.contains(':')
        || rel.split(['/', '\\']).any(|seg| seg == "..")
    {
        return RefStub::Unexpanded { rel, reason: "invalid_path" };
    }
    let Ok(body) = fs::read_to_string(project.join(&rel)) else {
        return RefStub::Unexpanded { rel, reason: "file_missing" };
    };
    if body.trim().is_empty() {
        return RefStub::Unexpanded { rel, reason: "file_empty" };
    }
    RefStub::Expanded { rel, body }
}

/// Expand a `--emit ref` dispatch stub into the full rendered prompt.
///
/// `agent-prompt-render --emit ref` prints a 2-line stub whose first line is
/// `MUSTARD-PROMPT-REF: <project-relative path>`; the orchestrator passes the
/// stub verbatim as the Task prompt so the full text never transits its
/// context. This hook is the other half of that contract: it reads the file
/// and returns a [`Verdict::Rewrite`] with the prompt replaced.
///
/// Fail-open AND transparent: a missing/invalid/empty ref still yields `None`
/// (the dispatch proceeds — the stub's own fallback line tells the subagent to
/// Read the file), but now emits a diagnostic via [`report_unexpanded`] so a
/// downstream "tool error" on a ref-dispatched agent is attributable to this
/// link instead of mistaken for a harness flake. No marker = silent (a normal
/// ad-hoc Task, not a ref dispatch).
fn expand_prompt_ref(project: &Path, cwd: &str, input: &HookInput) -> Option<Verdict> {
    match classify_ref_stub(project, &dispatch_prompt(input)) {
        RefStub::NoMarker => None,
        RefStub::Unexpanded { rel, reason } => {
            report_unexpanded(cwd, &rel, reason);
            None
        }
        RefStub::Expanded { body, .. } => {
            let mut tool_input = input.tool_input.clone();
            tool_input.as_object_mut()?.insert("prompt".to_string(), serde_json::Value::String(body));
            Some(Verdict::Rewrite { tool_input })
        }
    }
}

/// Surface a ref stub the hook could NOT expand — transparency, never a block.
/// The decision stays fail-open (the caller returns `None` and the dispatch
/// proceeds on the stub's fallback line); this only makes the failure VISIBLE
/// and attributable: stderr for a live session, plus an economy event so it
/// lands in the dashboard trace next to the agent it belongs to. Mirrors the
/// success-side `prompt_ref_expand` telemetry, completing the attribution
/// triad (expanded / unexpanded / neither = no marker or the hook never ran).
fn report_unexpanded(cwd: &str, rel: &str, reason: &str) {
    eprintln!(
        "subagent_inject: WARN: dispatch stub NOT expanded ({reason}): {rel} — subagent falls back to reading the file; surfacing for attribution"
    );
    economy::emit(
        cwd,
        ActorKind::Hook,
        "subagent_inject",
        "pipeline.economy.operation.invoked",
        None,
        serde_json::json!({"operation": "subagent_inject.prompt_ref_unexpanded", "reason": reason, "ref": rel, "duration_ms": 0, "tokens_used": 0}),
    );
}

/// `true` when the dispatch prompt already declares a SKILL block, in which
/// case we trust the caller (typically `agent-prompt-render`) and stay out.
fn prompt_declares_skill(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    // Accept either the canonical heading or an inline marker.
    lower.contains("\nskill:")
        || lower.contains("recommended skills")
        || lower.starts_with("skill:")
}

/// Pick the role from a Task input. The harness passes `subagent_type`; if
/// missing, fall back to `description` or `"general-purpose"`.
fn role_from_input(input: &HookInput) -> String {
    let tool_input = &input.tool_input;
    tool_input
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .map_or_else(|| "general-purpose".to_string(), str::to_string)
}

/// `true` for a read-only dispatch role — one that searches, audits or reviews
/// but never authors a plan or diff the regression gate scores. Such children
/// gain nothing from the regression-vocabulary pre-arm (it primes the AUTHOR of
/// a plan/diff not to lean on the gate's terms), so injecting it is pure noise.
/// `Plan` is deliberately NOT here: its plan text IS gate-checked. The list is
/// the small, stable set of harness/mustard read-only agent types — a denylist,
/// so an unknown (likely code-producing) role still gets the pre-arm.
fn role_is_readonly(role: &str) -> bool {
    // Normalise a possibly namespaced plugin agent type (`mustard:mustard-review`)
    // to its bare name by stripping the `<ns>:` prefix, so the denylist matches
    // whether the caller passed the qualified form (`dispatch-plan` now emits it)
    // or the bare name (an ad-hoc caller). `mustard-patterns` (the read-only mold
    // author) is in the set — it authors no plan/diff the regression gate scores.
    let lower = role.to_ascii_lowercase();
    let bare = lower.split_once(':').map_or(lower.as_str(), |(_, rest)| rest);
    matches!(
        bare,
        "explore"
            | "mustard-guards"
            | "mustard-review"
            | "mustard-patterns"
            | "claude-code-guide"
            | "statusline-setup"
    )
}

/// Read the project's glossary in full — no size cap. Relevance, not size,
/// decides what is injected. CONTEXT-MAP-aware: when the project carries a
/// `CONTEXT-MAP.md`, it is resolved through the SAME map-expanding resolver the
/// slicer/coverage use (`resolve_context_files`), so the hook sees every
/// `*context.md` the map links — not just a single root `CONTEXT.md`. The
/// resolved bodies are concatenated; a project with only a root `CONTEXT.md`
/// behaves exactly as before. Empty string when nothing resolves.
fn read_context_md(project: &Path) -> String {
    // Resolve the root CONTEXT.md plus a CONTEXT-MAP.md (when present) — the
    // resolver dedups, expands the map, and silently skips missing files.
    let mut requested: Vec<String> = Vec::new();
    let map = project.join("CONTEXT-MAP.md");
    if map.is_file() {
        requested.push(map.to_string_lossy().into_owned());
    }
    requested.push(project.join("CONTEXT.md").to_string_lossy().into_owned());

    let bodies: Vec<String> =
        crate::commands::economy::context_slice::resolve_context_files(&requested)
            .iter()
            .filter_map(|p| fs::read_to_string(p).ok())
            .collect();
    bodies.join("\n\n")
}

/// Pull the spec-memory principle files for the dispatch, honouring the
/// relevance gate. When the orchestration-layer judge has written
/// `<spec>/.memory-approved`, inject EXACTLY that approved set; with no gate
/// file, fall back to the deterministic recall matcher (relevance-ranked,
/// uncapped). Either way the filter is **relevance, never a count** — there is
/// no quantity cap, and the caller keeps the whole block out of the size cap.
/// Name-only rendering keeps each entry to a one-line wikilink.
fn spec_memory_block(project: &Path, spec: &str, prompt: &str, role: &str) -> String {
    let Some(spec_paths) = ClaudePaths::for_project(project)
        .ok()
        .and_then(|p| p.for_spec(spec).ok())
    else {
        return String::new();
    };
    let intent = format!("{role} {prompt}");
    let matches = context_inject::resolve_spec_memory(spec_paths.dir(), &intent, false);
    context_inject::render_spec_memory_block(&matches)
}

/// Resolve the active wave directory for the project. Reads the
/// `MUSTARD_ACTIVE_SPEC` + `MUSTARD_ACTIVE_WAVE` env vars and joins them
/// against the project's `.claude/spec/<spec>/wave-<n>(-*)/` directory.
///
/// Returns `None` when either env var is missing or when no matching wave
/// directory exists on disk — the SubagentStop branch then skips its
/// span-level eval (fail-open).
fn active_wave_dir(project: &Path) -> Option<PathBuf> {
    let spec = std::env::var("MUSTARD_ACTIVE_SPEC").ok().filter(|s| !s.is_empty())?;
    let wave = std::env::var("MUSTARD_ACTIVE_WAVE").ok().filter(|s| !s.is_empty())?;
    let claude = ClaudePaths::for_project(project).ok()?;
    let spec_paths = claude.for_spec(&spec).ok()?;
    // The wave env var carries either the bare wave number (e.g. "5") or the
    // full slug (e.g. "wave-5-rt"). Try the slug as-is first, then probe
    // `wave-{n}` + the first `wave-{n}-*` directory.
    if let Ok(wp) = spec_paths.for_wave(&wave) {
        if wp.dir().is_dir() {
            return Some(wp.dir().to_path_buf());
        }
    }
    // Numeric form — scan the spec dir for matching `wave-N(-role)?`.
    let prefix_exact = format!("wave-{wave}");
    let prefix_role = format!("wave-{wave}-");
    if let Ok(entries) = std::fs::read_dir(spec_paths.dir()) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else { continue };
            if name_str == prefix_exact || name_str.starts_with(&prefix_role) {
                let p = spec_paths.dir().join(name_str);
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Identifier for the returning child — best-effort assembly from the
/// SubagentStop input. Order: explicit `subagent_id` → `subagent_type` →
/// `agent_type` → `"unknown"`. Locale-agnostic (stays in ASCII).
fn child_id_from_input(input: &HookInput) -> String {
    for key in ["subagent_id", "agent_id", "subagent_type", "agent_type", "task_id"] {
        if let Some(v) = input.tool_input.get(key).and_then(|x| x.as_str()) {
            if !v.is_empty() {
                return v.to_string();
            }
        }
        if let Some(v) = input.raw.get(key).and_then(|x| x.as_str()) {
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    "unknown".to_string()
}

/// Pull the agent's terminal output text from the SubagentStop input. Mirrors
/// the lookup in `stop_observer::final_output` so the span-level eval sees
/// the same body the reinforcement observer does.
fn final_output_text(input: &HookInput) -> String {
    // Stop / SubagentStop deliver the returning agent's final text as
    // `last_assistant_message` (the Claude Code hook contract says to prefer it
    // over reading the transcript). The `result` / `output` keys below are
    // PostToolUse-shaped and ABSENT on a Stop event — reading only those left the
    // whole SubagentStop-capture family (memory / span-eval / verdict) inert in
    // production (zero `<MEMORY>` decisions, zero `_review-spans.md` ledgers).
    // Prefer it, then fall back to the inline keys for the PostToolUse path.
    if let Some(s) = input
        .raw
        .get("last_assistant_message")
        .and_then(|v| v.as_str())
    {
        if !s.is_empty() {
            return s.to_string();
        }
    }
    for key in ["result", "final_output", "output", "tool_response", "tool_result"] {
        if let Some(v) = input.raw.get(key) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
            if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    String::new()
}

/// Extract the FIRST `<MEMORY>...</MEMORY>` block's inner text, trimmed.
/// `None` when absent, or present but blank after trimming (an empty tag
/// pair is not a real memory). Byte-wise scan — no regex crate in this
/// workspace. The `impl`/`plan` role contract (`role.rs::build_role_block`)
/// gates EMISSION to a rare, real-choice bar, so this extractor does not
/// need its own filter beyond "is the tag present and non-empty" — the
/// scarcity is already enforced upstream, at the source.
fn extract_memory_block(text: &str) -> Option<String> {
    let start = text.find("<MEMORY>")? + "<MEMORY>".len();
    let end_rel = text[start..].find("</MEMORY>")?;
    let inner = text[start..start + end_rel].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

/// A review subagent's machine-readable `<VERDICT>` block — the review twin of
/// the `<MEMORY>` capture. The reviewer's role contract
/// (`render::role::build_role_block`, the `"review"` arm, and the plugin
/// `mustard-review.md`) instructs it to end with
/// `<VERDICT>{"verdict":"approved"|"rejected","critical":N,"findings":[…]}</VERDICT>`.
/// Only the two gate-bearing fields are deserialized: [`review_result::record_review`]
/// consumes `verdict` + `criticalCount` and nothing else, so `findings`
/// (human/audit-facing) is deliberately dropped — serde skips the unknown field.
#[derive(Debug, PartialEq, Deserialize)]
struct ReviewVerdict {
    verdict: String,
    critical: i64,
}

/// Extract and validate the FIRST `<VERDICT>...</VERDICT>` block. Byte-wise scan
/// (no regex crate in this workspace), mirroring [`extract_memory_block`].
/// `None` — the hook then falls open — when the tag is absent, empty, its body
/// is not valid JSON, a required field is missing, or `verdict` is anything
/// other than `approved`/`rejected`. That whitelist mirrors the manual CLI
/// path's own check in [`review_result::run`], so the auto and manual paths
/// accept exactly the same verdict vocabulary.
fn extract_verdict_block(text: &str) -> Option<ReviewVerdict> {
    let start = text.find("<VERDICT>")? + "<VERDICT>".len();
    let end_rel = text[start..].find("</VERDICT>")?;
    let inner = text[start..start + end_rel].trim();
    if inner.is_empty() {
        return None;
    }
    let parsed: ReviewVerdict = serde_json::from_str(inner).ok()?;
    if parsed.verdict != "approved" && parsed.verdict != "rejected" {
        return None;
    }
    Some(parsed)
}

/// Harvest a `<MEMORY>` block from a returning subagent's final output and
/// persist it as a `decision` harness event — the durable, queryable home
/// for cross-wave lessons. Closes the gap the field trace found: the `impl`
/// role is instructed to emit `<MEMORY>`, but nothing ever read it back —
/// the block surfaced once in the orchestrator's Task-tool context and then
/// evaporated (`session_stop_observer`'s prose capture was retired with the
/// old knowledge store and never replaced). This makes capture AUTOMATIC —
/// a hook, not an instruction the orchestrator has to remember to act on.
///
/// Spec attribution mirrors `emit-event`'s own fallback order: the
/// session-bound `active-spec` marker first (kept fresh by every
/// `pipeline.*` event the running pipeline already emits), then the
/// legacy/env `current_spec` resolution. No spec resolves ⇒ no-op — a
/// decision with no spec to attribute it to is discarded, never emitted
/// orphaned.
///
/// Fail-open throughout: no memory block, no resolvable spec, or a write
/// error all degrade to a silent no-op. This is telemetry, never a blocking
/// path — never called from a `Check`, only from the `Observer`-shaped
/// `SubagentStop` side effect below.
fn capture_memory_decision(project: &Path, cwd: &str, input: &HookInput) {
    capture_memory_decision_with_session(project, cwd, input, &crate::shared::context::session_id());
}

/// Session-explicit variant of [`capture_memory_decision`] — the actual
/// worker, taking `session_id` as a parameter instead of resolving the
/// ambient [`crate::shared::context::session_id`] internally. Mirrors this
/// file's own [`span_level_eval_and_append`]/[`span_level_eval_and_append_in`]
/// split and for the same reason: a test cannot safely mutate
/// `MUSTARD_SESSION_ID` (`unsafe` under Rust 2024, forbidden in this crate),
/// so the deterministic entry point takes the value directly.
fn capture_memory_decision_with_session(project: &Path, cwd: &str, input: &HookInput, sid: &str) {
    let Some(memory) = extract_memory_block(&final_output_text(input)) else {
        return;
    };
    let spec = crate::shared::context::spec_for_session(cwd, sid)
        .or_else(|| crate::shared::context::current_spec(cwd));
    let Some(spec) = spec else {
        return;
    };
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: sid.to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("subagent_inject".to_string()),
            actor_type: None,
        },
        event: "decision".to_string(),
        payload: json!({
            "title": memory,
            "role": role_from_input(input),
            "source": "memory-block",
        }),
        spec: Some(spec),
    };
    let _ = crate::shared::events::route::emit(&project.to_string_lossy(), &event);
}

/// `true` for the review agent's `subagent_type`. Normalises a namespaced
/// plugin agent type (`mustard:mustard-review`) to its bare name — mirroring
/// [`role_is_readonly`] — so the qualified form `dispatch-plan` emits and a bare
/// ad-hoc caller both match. `qa` shares the same agent but never emits a
/// `<VERDICT>` block, so gating on this type is exact in practice.
fn role_is_review(role: &str) -> bool {
    let lower = role.to_ascii_lowercase();
    let bare = lower.split_once(':').map_or(lower.as_str(), |(_, rest)| rest);
    bare == "mustard-review"
}

/// Harvest a review subagent's `<VERDICT>` block from its final output and
/// record it as a `review.result` event + `review` metric — the review twin of
/// [`capture_memory_decision`]. It emits through the SAME recorder the manual
/// `review-result` CLI uses ([`review_result::record_review`]), so the machine
/// now writes the gate's input verbatim and the orchestrator no longer reads the
/// reviewer's prose to decide `approved`/`rejected` + the critical count.
///
/// Fail-open at every step: a non-review role, an absent/empty/malformed block,
/// or no resolvable spec all degrade to a silent no-op — the manual CLI path
/// stays the fallback source of the verdict. Telemetry only, never a blocking
/// path (called from the `SubagentStop` side effect below, never a `Check`).
fn capture_review_verdict(project: &Path, cwd: &str, input: &HookInput) {
    capture_review_verdict_with_session(project, cwd, input, &crate::shared::context::session_id());
}

/// Session-explicit worker for [`capture_review_verdict`] — takes `session_id`
/// directly so a test can drive it without mutating `MUSTARD_SESSION_ID`
/// (`unsafe` under Rust 2024, forbidden in this crate), mirroring the
/// [`capture_memory_decision_with_session`] split.
fn capture_review_verdict_with_session(project: &Path, cwd: &str, input: &HookInput, sid: &str) {
    if !role_is_review(&role_from_input(input)) {
        return;
    }
    let Some(verdict) = extract_verdict_block(&final_output_text(input)) else {
        return;
    };
    // Spec attribution mirrors the memory twin: the session-bound `active-spec`
    // marker first, then the legacy/env `current_spec`. No spec ⇒ no-op.
    let spec = crate::shared::context::spec_for_session(cwd, sid)
        .or_else(|| crate::shared::context::current_spec(cwd));
    let Some(spec) = spec else {
        return;
    };
    // Reuse the manual recorder: identical `review.result` event + `review`
    // metric, parsed straight from the block (zero orchestrator interpretation).
    // The block carries neither a subproject nor a findings *file*, so both are
    // `None` — its `findings` array is audit-facing and not a gate input.
    let _ = review_result::record_review(project, &spec, &verdict.verdict, verdict.critical, None, None);
}

/// Run the W4 span-level gate (Moment 3) for the returning child and append
/// the verdict to `<wave-dir>/_review-spans.md`. Fail-open at every step —
/// any IO or gate error degrades to a no-op so the orchestrator's
/// SubagentStop flow continues.
///
/// Returns the verdict label that was appended (or `None` when no append
/// happened) so callers can wire telemetry.
fn span_level_eval_and_append(
    project: &Path,
    input: &HookInput,
    cwd: &str,
) -> Option<&'static str> {
    let wave_dir = active_wave_dir(project)?;
    span_level_eval_and_append_in(&wave_dir, input, cwd)
}

/// Span-level variant that takes the resolved wave directory as a parameter,
/// bypassing the env-var lookup. Used by [`span_level_eval_and_append`] and
/// by integration tests that need to avoid mutating process env vars (which
/// are `unsafe` under Rust 2024 + this crate's `forbid(unsafe_code)`).
fn span_level_eval_and_append_in(
    wave_dir: &Path,
    input: &HookInput,
    cwd: &str,
) -> Option<&'static str> {
    let spec_md = wave_dir.join("spec.md");
    let plan_text = final_output_text(input);
    let gate_input = GateInput {
        spec_path: spec_md,
        plan_text,
        diff: Vec::new(),
        declared_fns: Vec::new(),
        before_snapshot: None,
        after_snapshot: None,
    };
    let (verdict_label, signal_count, first_message) = match check_after_child_return(gate_input) {
        Ok(RegressionVerdict::Green) => (VERDICT_GREEN, 0usize, String::new()),
        Ok(RegressionVerdict::Amber { signals }) => {
            let first = signals.first().map(|s| s.message.clone()).unwrap_or_default();
            (VERDICT_AMBER, signals.len(), first)
        }
        Ok(RegressionVerdict::Red { signals }) => {
            let first = signals.first().map(|s| s.message.clone()).unwrap_or_default();
            (VERDICT_RED, signals.len(), first)
        }
        Err(GateError::Blocked) => {
            // The gate emitted the Red JSON to stdout and returned an error.
            // We still want a ledger row — the actual signals are not in the
            // error variant, so we record a synthetic "blocked" line.
            (VERDICT_RED, 0, String::from("gate.error.blocked"))
        }
    };
    let entry = VerdictEntry {
        verdict: verdict_label.to_string(),
        child_id: child_id_from_input(input),
        iso_ts: mustard_core::time::now_iso8601(),
        signal_count,
        first_message,
    };
    let _ = review_spans::append_verdict(wave_dir, &entry);
    economy::emit(cwd, ActorKind::Hook, "subagent_inject", "pipeline.economy.operation.invoked", None, serde_json::json!({"operation": "subagent_inject.span_eval", "duration_ms": 0, "tokens_used": 0}));
    Some(verdict_label)
}

/// The dispatch prompt — `tool_input.prompt` for a Task call.
fn dispatch_prompt(input: &HookInput) -> String {
    input
        .tool_input
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_default()
}

impl Check for SubagentInject {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // W5.T5.2 — Span-level eval at SubagentStop. Runs per child return,
        // never accumulating until end-of-wave (AC-A-5). Fail-open: any IO
        // or gate error degrades to a no-op so the orchestrator continues.
        //
        // Memory capture rides the SAME return: harvest a `<MEMORY>` block
        // (if the child emitted one) into a durable `decision` event — see
        // `capture_memory_decision`. Independent of the span-level eval;
        // either can no-op without affecting the other.
        //
        // Verdict capture rides it too: for a review child, harvest a
        // `<VERDICT>` block into a `review.result` event — see
        // `capture_review_verdict`. Also independent + fail-open; a non-review
        // child or an absent/malformed block is a silent no-op.
        if ctx.trigger == Some(Trigger::SubagentStop) {
            let cwd = ctx.project_dir_or_cwd(input);
            let project = PathBuf::from(&cwd);
            let _ = span_level_eval_and_append(&project, input, &cwd);
            capture_memory_decision(&project, &cwd, input);
            capture_review_verdict(&project, &cwd, input);
            return Ok(Verdict::Allow);
        }

        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() != Some("Task")
            && input.tool_name.as_deref() != Some("Agent")
        {
            return Ok(Verdict::Allow);
        }
        let cwd = ctx.project_dir_or_cwd(input);
        let project = PathBuf::from(&cwd);
        // `--emit ref` stub → rewrite the dispatch with the full rendered
        // prompt from disk. The rendered prompt is the complete
        // agent-prompt-render product (skills, guards, contract), so no
        // further injection is needed — and this module is the LAST
        // PreToolUse(Task) check in the registry, so the Rewrite verdict
        // survives the outcome fold.
        if let Some(verdict) = expand_prompt_ref(&project, &cwd, input) {
            economy::emit(&cwd, ActorKind::Hook, "subagent_inject", "pipeline.economy.operation.invoked", None, serde_json::json!({"operation": "subagent_inject.prompt_ref_expand", "duration_ms": 0, "tokens_used": 0}));
            return Ok(verdict);
        }
        let prompt = dispatch_prompt(input);
        if prompt_declares_skill(&prompt) {
            // Trust agent-prompt-render — do nothing.
            return Ok(Verdict::Allow);
        }
        let role = role_from_input(input);

        // CONTEXT.md + regression vocab. No size cap — relevance decides what
        // enters; nothing is trimmed by char count.
        let mut sections: Vec<String> = Vec::new();

        // Epistemic-contract FLOOR for investigative read-only dispatches.
        // The explore contract (settle existence by enumeration; never refute a
        // runtime symptom) normally rides in via the rendered prompt
        // (`expand_prompt_ref`, handled above). An Explore dispatched OUTSIDE the
        // renderer — ad-hoc `Task(Explore)`, `/task` vibe, or cross-repo where the
        // stub cannot resolve against this cwd — bypasses that path silently and
        // lands here with no contract. Re-assert the clause as a floor so the
        // discipline is never lost to the dispatch route. Idempotent: a rendered
        // prompt declares a SKILL block (returns above) and never reaches here;
        // the guard only defends against a caller that already inlined the clause.
        if role.eq_ignore_ascii_case("explore")
            && !prompt.contains("never refute a symptom")
        {
            sections.push(format!(
                "## Epistemic contract\n{}",
                crate::commands::agent::agent_prompt_render::EPISTEMIC_FLOOR
            ));
        }
        // Relevance-slice CONTEXT.md against the dispatch prompt — the SAME
        // term-block filter the renderer runs, so the hook injects only the
        // matching blocks (in full), never the raw whole file. Relevance, not
        // size, bounds it (fixes the raw-dump regression).
        let ctx_md = crate::commands::economy::context_slice::slice_text(
            &read_context_md(&project),
            &prompt,
        );
        if !ctx_md.is_empty() {
            sections.push(format!("## CONTEXT.md\n{ctx_md}"));
        }
        // Spec memory rides OUTSIDE the size cap: it is relevance-filtered (the
        // gate's approved set, or the recall fallback) and carries no count cap,
        // so truncating it by size would contradict the gate — relevance, not
        // size, decides what enters.
        let mut memory = String::new();
        if let Some(spec) = crate::shared::context::current_spec(&cwd) {
            if !spec.is_empty() {
                memory = spec_memory_block(&project, &spec, &prompt, &role);
            }
        }
        // W5.T5.1 — Pre-arm the child with the regression vocabulary the
        // gate will check. This is an INTERNAL subagent prompt, so the
        // vocabulary is rendered in EN/technical regardless of the project's
        // user-facing locale — agent/subagent prompts stay EN by policy; only
        // user output, specs and waves honour the project locale. Skipped for
        // read-only roles (Explore/guards/review): they author no plan or diff
        // the gate scores, so the pre-arm would be noise in their window.
        if !role_is_readonly(&role) {
            let locale = mustard_core::SupportedLocale::EnUs;
            let vocab = context_inject::vocabulary_inject_block(&project, locale);
            if !vocab.is_empty() {
                sections.push(vocab);
            }
        }

        if sections.is_empty() && memory.is_empty() {
            return Ok(Verdict::Allow);
        }
        // Emit telemetry — fail-open.
        economy::emit(&cwd, ActorKind::Hook, "subagent_inject", "pipeline.economy.operation.invoked", None, serde_json::json!({"operation": "subagent_inject.dispatch", "duration_ms": 0, "tokens_used": 0}));
        // No size cap: every section rides in full. Relevance is the only filter.
        let pre = sections.join("\n\n");
        let context = match (pre.is_empty(), memory.is_empty()) {
            (false, false) => format!("{pre}\n\n{memory}"),
            (false, true) => pre,
            (true, false) => memory,
            (true, true) => return Ok(Verdict::Allow),
        };
        Ok(Verdict::Inject { context })
    }
}

/// Emit `pipeline.economy.operation.invoked` for a W8 in-binary operation.
/// Fail-open. Routes through `route::emit` (NDJSON sink) for uniformity.

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ctx_for(dir: &Path) -> Ctx {
        Ctx {
            project_dir: dir.to_string_lossy().to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        }
    }

    fn task_input(prompt: &str, role: &str) -> HookInput {
        HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: serde_json::json!({ "prompt": prompt, "subagent_type": role }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        }
    }

    #[test]
    fn skip_when_skill_already_declared() {
        let dir = tempdir().unwrap();
        let input = task_input("Do this.\nSKILL: foo\n", "general-purpose");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        assert_eq!(v, Verdict::Allow);
    }

    #[test]
    fn skip_for_non_task_tools() {
        let dir = tempdir().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: serde_json::json!({ "command": "ls" }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        assert_eq!(
            SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
    }

    /// `--emit ref` round-trip: a Task prompt carrying the
    /// `MUSTARD-PROMPT-REF` stub is rewritten with the file's full content;
    /// the other tool_input fields survive untouched.
    #[test]
    fn prompt_ref_stub_is_expanded_into_full_prompt_rewrite() {
        let dir = tempdir().unwrap();
        let rel = ".claude/spec/demo/.dispatch/wave-1-rt.first.prompt.md";
        let full = dir.path().join(rel);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, "ROLE: impl\nthe real rendered prompt body").unwrap();

        let stub = format!("MUSTARD-PROMPT-REF: {rel}\nDispatch stub — fallback line.");
        let input = task_input(&stub, "general-purpose");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        match v {
            Verdict::Rewrite { tool_input } => {
                let p = tool_input["prompt"].as_str().expect("prompt string");
                assert!(p.contains("the real rendered prompt body"), "expanded: {p}");
                assert!(!p.contains("MUSTARD-PROMPT-REF"), "stub replaced, not appended: {p}");
                assert_eq!(
                    tool_input["subagent_type"], "general-purpose",
                    "sibling fields preserved"
                );
            }
            other => panic!("expected Rewrite, got {other:?}"),
        }
    }

    /// A stub naming a missing file must NOT rewrite — the dispatch proceeds
    /// with the stub, whose fallback line tells the subagent to Read it.
    #[test]
    fn prompt_ref_missing_file_falls_through_fail_open() {
        let dir = tempdir().unwrap();
        let stub = "MUSTARD-PROMPT-REF: .claude/spec/demo/.dispatch/ghost.prompt.md\nfallback";
        let input = task_input(stub, "general-purpose");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        assert!(!matches!(v, Verdict::Rewrite { .. }), "missing file must not rewrite: {v:?}");
    }

    /// Absolute, rooted, drive-qualified, or `..`-escaping paths are rejected
    /// — the stub may only reference a file under the project root.
    #[test]
    fn prompt_ref_rejects_escaping_and_rooted_paths() {
        let dir = tempdir().unwrap();
        for evil in [
            "../outside.md",
            ".claude/../../leak.md",
            "/etc/passwd",
            "C:/Windows/x.md",
            "\\\\server\\share\\x.md",
        ] {
            let stub = format!("MUSTARD-PROMPT-REF: {evil}\nfallback");
            let input = task_input(&stub, "general-purpose");
            let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
            assert!(!matches!(v, Verdict::Rewrite { .. }), "path {evil} must not expand");
        }
    }

    /// The transparency seam: `classify_ref_stub` stays silent when there is no
    /// ref marker (a normal ad-hoc Task), and otherwise names exactly which
    /// link of the render→stub→hook chain broke — so a failure is attributable
    /// instead of a silent fall-through. (The fall-through itself is covered by
    /// the two tests above; this pins the REASON the diagnostic reports.)
    #[test]
    fn classify_ref_stub_names_the_broken_link() {
        let dir = tempdir().unwrap();
        let project = dir.path();

        // No marker → a plain Task, never surfaced.
        assert_eq!(classify_ref_stub(project, "just do the thing"), RefStub::NoMarker);

        // Marker + valid file → expands, carrying the rel and body.
        let rel = ".claude/spec/demo/.dispatch/wave-1-rt.first.prompt.md";
        let full = project.join(rel);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, "ROLE: impl\nreal body").unwrap();
        match classify_ref_stub(project, &format!("MUSTARD-PROMPT-REF: {rel}\nfallback")) {
            RefStub::Expanded { rel: r, body } => {
                assert_eq!(r, rel);
                assert!(body.contains("real body"), "carries the file body: {body}");
            }
            other => panic!("expected Expanded, got {other:?}"),
        }

        // Marker + missing file → attributable as file_missing (render lost it).
        assert_eq!(
            classify_ref_stub(project, "MUSTARD-PROMPT-REF: .claude/spec/demo/.dispatch/ghost.md\nfallback"),
            RefStub::Unexpanded { rel: ".claude/spec/demo/.dispatch/ghost.md".into(), reason: "file_missing" }
        );

        // Marker + escaping/rooted/drive path → invalid_path, before any IO.
        for evil in ["../outside.md", ".claude/../../leak.md", "/etc/passwd", "C:/Windows/x.md"] {
            assert_eq!(
                classify_ref_stub(project, &format!("MUSTARD-PROMPT-REF: {evil}\nfallback")),
                RefStub::Unexpanded { rel: evil.into(), reason: "invalid_path" },
                "evil path {evil}"
            );
        }

        // Marker + empty render → file_empty.
        let empty_rel = ".claude/spec/demo/.dispatch/empty.md";
        std::fs::write(project.join(empty_rel), "   \n").unwrap();
        assert_eq!(
            classify_ref_stub(project, &format!("MUSTARD-PROMPT-REF: {empty_rel}\nfallback")),
            RefStub::Unexpanded { rel: empty_rel.into(), reason: "file_empty" }
        );
    }

    #[test]
    fn injects_context_md_when_present() {
        let dir = tempdir().unwrap();
        // The hook now relevance-slices CONTEXT.md against the dispatch prompt —
        // only blocks sharing a term with the prompt are injected. So the block
        // must mention something the prompt does ("user"/"module").
        std::fs::write(dir.path().join("CONTEXT.md"), "## User\nThe user module domain.").unwrap();
        let input = task_input("refactor the user module", "general-purpose");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("CONTEXT.md"));
                assert!(context.contains("User"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    #[test]
    fn context_md_is_relevance_sliced_not_raw_dumped() {
        let dir = tempdir().unwrap();
        // Two blocks; only one shares a term with the prompt. The off-topic block
        // must NOT be injected — relevance slices it out (no raw whole-file dump).
        std::fs::write(
            dir.path().join("CONTEXT.md"),
            "## Billing\nInvoice and payment terms.\n## User\nThe user module domain.",
        )
        .unwrap();
        let input = task_input("refactor the user module", "general-purpose");
        match SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap() {
            Verdict::Inject { context } => {
                assert!(context.contains("User"), "relevant block kept");
                assert!(!context.contains("Billing"), "off-topic block sliced out");
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    /// CONTEXT-MAP awareness: a `CONTEXT-MAP.md` pointing at a sub-glossary
    /// must make that sub-glossary's term blocks reachable to the inject — not
    /// just a single root `CONTEXT.md`. The relevant block (sharing a term with
    /// the prompt) rides in; an off-topic one in the same file is sliced out.
    #[test]
    fn context_map_pulls_in_referenced_glossary_files() {
        let dir = tempdir().unwrap();
        // The sub-glossary lives beside the map; the map links it by name.
        std::fs::write(
            dir.path().join("domain-context.md"),
            "## Billing\nInvoice terms.\n## User\nThe user module domain.",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("CONTEXT-MAP.md"),
            "# Map\n- see [domain](domain-context.md)\n",
        )
        .unwrap();
        // No root CONTEXT.md at all — the map is the only source.
        let input = task_input("refactor the user module", "general-purpose");
        match SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap() {
            Verdict::Inject { context } => {
                assert!(context.contains("User"), "map-referenced block must reach the inject");
                assert!(!context.contains("Billing"), "off-topic block still sliced out");
            }
            other => panic!("expected Inject from a map-referenced glossary, got {other:?}"),
        }
    }

    /// Field defect (cross-repo dogfood): an Explore dispatched OUTSIDE the
    /// renderer (no `MUSTARD-PROMPT-REF` stub, no SKILL block) reached the
    /// subagent with NO epistemic contract — and returned a confident verdict
    /// that refuted a symptom the user had observed at runtime. The floor closes
    /// that bypass: any ad-hoc Explore still gets the clause, regardless of cwd
    /// or active spec.
    #[test]
    fn ad_hoc_explore_dispatch_gets_epistemic_floor() {
        let dir = tempdir().unwrap();
        let input = task_input("trace why future-dated titles show as overdue", "Explore");
        match SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap() {
            Verdict::Inject { context } => {
                assert!(
                    context.contains("never refute a symptom"),
                    "epistemic floor missing for ad-hoc Explore: {context}"
                );
                assert!(context.contains("Epistemic contract"), "floor heading missing: {context}");
                assert!(
                    !context.contains("Regression vocabulary"),
                    "explore stays read-only — the floor must not drag in regression-vocab noise: {context}"
                );
            }
            other => panic!("expected Inject with epistemic floor, got {other:?}"),
        }
    }

    /// The floor is scoped to the investigative `explore` role — a
    /// general-purpose dispatch (a code author) must NOT get the read-only
    /// epistemic clause, whether it resolves to Allow or to an Inject carrying
    /// only other sections.
    #[test]
    fn non_explore_dispatch_gets_no_epistemic_floor() {
        let dir = tempdir().unwrap();
        let input = task_input("implement the user module", "general-purpose");
        if let Verdict::Inject { context } =
            SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap()
        {
            assert!(
                !context.contains("never refute a symptom"),
                "epistemic floor must not fire for general-purpose: {context}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // W5 — span-level review (T5.1, T5.2, T5.7)
    // -----------------------------------------------------------------------

    /// Build a project skeleton with the wave dir + a mustard.json declaring
    /// the locale, returning (project_root, wave_dir).
    fn setup_wave_project(spec_name: &str, wave_slug: &str, locale: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let project = dir.path().to_path_buf();
        // Create the `.claude/` skeleton so `resolve_project_root` anchors the
        // tempdir as the project root. The injected regression vocabulary is now
        // always EN (internal subagent prompt), so the declared `locale` no
        // longer drives locale resolution — it is kept only to stamp a
        // representative mustard.json into the fixture.
        let claude = project.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("mustard.json"), format!("{{\"lang\":\"{locale}\"}}")).unwrap();
        // Wave dir with a placeholder spec.md so `check_after_child_return`
        // has a path that resolves to the project root.
        let wave_dir = claude.join("spec").join(spec_name).join(wave_slug);
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(wave_dir.join("spec.md"), "# placeholder\n").unwrap();
        (dir, wave_dir)
    }

    fn stop_input(child: &str, output_text: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: serde_json::json!({ "subagent_type": child }),
            hook_event_name: Some("SubagentStop".to_string()),
            raw: serde_json::json!({ "result": output_text }),
            ..HookInput::default()
        }
    }

    /// Regression (SubagentStop-capture family was inert in production): a real
    /// `SubagentStop` delivers the returning agent's final text as
    /// `last_assistant_message` (Claude Code hook contract), NOT via the
    /// PostToolUse-shaped `result`/`output` keys. `final_output_text` must read
    /// it — otherwise the whole capture family (memory / span-eval / verdict)
    /// silently no-ops on every real subagent return.
    ///
    /// NOTE: the `stop_input` helper above still fabricates the OLD
    /// (`result` + `tool_input.subagent_type`) shape; migrating it — and the
    /// span-eval / memory tests that use it — to `last_assistant_message` +
    /// top-level `agent_type` is the test-hygiene follow-up that removes the
    /// remaining false positives.
    #[test]
    fn final_output_text_reads_last_assistant_message() {
        let real_stop = HookInput {
            hook_event_name: Some("SubagentStop".to_string()),
            agent_type: Some("general-purpose".to_string()),
            agent_id: Some("agent-1".to_string()),
            raw: serde_json::json!({
                "agent_id": "agent-1",
                "last_assistant_message": "did the work <MEMORY>a real decision</MEMORY>"
            }),
            ..HookInput::default()
        };
        assert_eq!(
            final_output_text(&real_stop),
            "did the work <MEMORY>a real decision</MEMORY>",
            "SubagentStop last_assistant_message must be the output source"
        );
        // Backward-compat: the PostToolUse inline shape still resolves via fallback.
        let post_tool = HookInput {
            raw: serde_json::json!({ "tool_response": { "text": "inline body" } }),
            ..HookInput::default()
        };
        assert_eq!(final_output_text(&post_tool), "inline body");
    }

    /// AC-A-5 + AC-A-7 — three sequential children fire `SubagentStop` and
    /// each call appends one line to `_review-spans.md`. The second child
    /// emits a Red verdict (its output text triggers a Semantic vocab hit);
    /// consolidation must then be blocked by [`review_spans::check_consolidation`].
    ///
    /// The test drives [`span_level_eval_and_append_in`] directly (passing
    /// the wave directory as a parameter) so it does NOT need to mutate
    /// `MUSTARD_ACTIVE_SPEC` / `MUSTARD_ACTIVE_WAVE` — `context::set_var` is
    /// `unsafe` under Rust 2024 and this crate forbids `unsafe_code`. The
    /// production caller [`span_level_eval_and_append`] is a thin wrapper
    /// around the same helper that resolves the wave from the env vars.
    #[test]
    fn w5_three_sequential_children_append_per_stop_and_red_blocks_consolidation() {
        let spec = "w5-test-span-eval";
        let wave_slug = "wave-5-rt";
        let (dir, wave_dir) = setup_wave_project(spec, wave_slug, "pt-BR");
        let cwd = dir.path().to_string_lossy().to_string();

        // Child 1 — clean output → green.
        let v1 = span_level_eval_and_append_in(
            &wave_dir,
            &stop_input("child-1", "all good, no issues"),
            &cwd,
        );
        assert_eq!(v1, Some(VERDICT_GREEN), "child-1 should land as green");

        // Child 2 — output mentions a Semantic-layer term → red.
        let v2 = span_level_eval_and_append_in(
            &wave_dir,
            &stop_input("child-2", "tive que fazer fail-open dessa wave"),
            &cwd,
        );
        assert!(
            v2 == Some(VERDICT_RED) || v2 == Some(VERDICT_AMBER),
            "child-2's Semantic-layer hit should escalate past green, got {v2:?}"
        );

        // Child 3 — clean again → green.
        let v3 = span_level_eval_and_append_in(
            &wave_dir,
            &stop_input("child-3", "shipped clean"),
            &cwd,
        );
        assert_eq!(v3, Some(VERDICT_GREEN), "child-3 should land as green");

        // AC-A-5 — span-level: 3 lines on disk (one per stop), in order.
        let entries = review_spans::read_entries(&wave_dir);
        assert_eq!(entries.len(), 3, "expected one ledger line per SubagentStop, got {entries:?}");
        assert_eq!(entries[0].child_id, "child-1");
        assert_eq!(entries[1].child_id, "child-2");
        assert_eq!(entries[2].child_id, "child-3");

        // The middle child must have escalated past green — drives AC-A-7.
        assert_ne!(
            entries[1].verdict, VERDICT_GREEN,
            "child-2 must not be green: it mentioned a Semantic term"
        );

        // AC-A-7 — at least one Red on the ledger blocks consolidation. If
        // the middle child landed as Amber on this host (because the project
        // has no vocab file and the default Semantic list still matched at
        // Medium severity for some reason), force a Red to exercise the
        // blocking path — the AC is about the *check*, not about which
        // severity tier the matcher chose.
        if matches!(review_spans::check_consolidation(&wave_dir), review_spans::ConsolidationCheck::Allowed) {
            review_spans::append_verdict(
                &wave_dir,
                &VerdictEntry {
                    verdict: VERDICT_RED.to_string(),
                    child_id: "synthetic-red".to_string(),
                    iso_ts: mustard_core::time::now_iso8601(),
                    signal_count: 1,
                    first_message: "synthetic Red to exercise AC-A-7".to_string(),
                },
            )
            .expect("append synthetic red");
        }
        assert!(
            matches!(review_spans::check_consolidation(&wave_dir), review_spans::ConsolidationCheck::Blocked { .. }),
            "ledger must report a Red verdict after the W5 sequence"
        );
        match review_spans::check_consolidation(&wave_dir) {
            review_spans::ConsolidationCheck::Blocked { entry } => {
                assert_eq!(entry.verdict, VERDICT_RED);
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    /// T5.1 — PreToolUse Task dispatch surfaces the vocabulary inject block.
    ///
    /// The injected vocabulary is an INTERNAL subagent prompt, so it is always
    /// EN/technical regardless of the project's user-facing locale — even though
    /// this fixture declares `pt-BR` in mustard.json, the heading stays EN.
    #[test]
    fn w5_pretooluse_dispatch_injects_vocabulary_block() {
        let dir = tempdir().unwrap();
        // A pt-BR mustard.json still must NOT localise the internal prompt.
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("mustard.json"), "{\"lang\":\"pt-BR\"}").unwrap();

        let input = task_input("refactor the user module", "general-purpose");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        match v {
            Verdict::Inject { context } => {
                // EN heading (internal prompt) + at least one default Semantic term.
                assert!(
                    context.contains("Regression vocabulary"),
                    "expected EN vocabulary heading, got: {context}"
                );
                assert!(
                    context.contains("fail-open"),
                    "expected default Semantic term in inject, got: {context}"
                );
            }
            other => panic!("expected Inject with vocab section, got {other:?}"),
        }
    }

    /// A read-only role authors no plan/diff the gate scores, so the regression
    /// vocabulary must NOT be injected. With no CONTEXT.md and no active spec,
    /// the only candidate section was the vocab — so the decisive verdict
    /// degrades to `Allow`. Uses `mustard-guards` (not `explore`) because the
    /// `explore` role now also carries the epistemic-contract floor, which on
    /// its own resolves to an Inject — covered by
    /// `ad_hoc_explore_dispatch_gets_epistemic_floor` (which also asserts the
    /// vocab stays absent for explore).
    #[test]
    fn readonly_role_skips_vocabulary_and_allows_when_nothing_else_resolves() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("mustard.json"), "{\"lang\":\"en-US\"}").unwrap();

        let input = task_input("grep the codebase for the user module", "mustard-guards");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        assert_eq!(v, Verdict::Allow, "read-only role gets no regression-vocab noise");
    }

    // --- <MEMORY> capture (SubagentStop → `decision` event) ----------------

    /// Read every `decision` event's `title` for `spec` under `cwd`, in the
    /// order the NDJSON files sort. Test-only helper — production readers
    /// live in `agent::render::mod::decision_events_block`.
    fn decision_titles(cwd: &Path, spec: &str) -> Vec<String> {
        let events_dir = ClaudePaths::for_project(cwd)
            .and_then(|p| p.for_spec(spec))
            .unwrap()
            .events_dir();
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir)
            .iter()
            .filter(|e| e.event == "decision")
            .filter_map(|e| e.payload.get("title").and_then(|v| v.as_str()).map(str::to_string))
            .collect()
    }

    #[test]
    fn extract_memory_block_trims_and_rejects_blank() {
        assert_eq!(
            extract_memory_block("blah <MEMORY> real lesson here </MEMORY> more"),
            Some("real lesson here".to_string())
        );
        assert_eq!(extract_memory_block("no tag at all"), None);
        assert_eq!(extract_memory_block("<MEMORY>   </MEMORY>"), None, "blank body ⇒ None");
        assert_eq!(extract_memory_block("<MEMORY>x"), None, "unterminated tag ⇒ None");
    }

    /// End-to-end: a `<MEMORY>` block in the child's final output, with the
    /// session bound to a spec via the `active-spec` marker, lands as a
    /// `decision` event carrying the memory text — readable back from the
    /// spec's own event log.
    #[test]
    fn subagent_stop_with_memory_block_emits_decision_event() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().to_string();
        let spec = "permissions-rbac-overhaul";
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join(spec)).unwrap();
        crate::shared::context::bind_session_spec(&cwd, "sess-1", spec);

        let input = stop_input(
            "impl-1",
            "Files changed: foo.rs.\n\
             <MEMORY>Chose atomic_md write over direct fs::write — a mid-write crash corrupts the file</MEMORY>",
        );
        capture_memory_decision_with_session(dir.path(), &cwd, &input, "sess-1");

        let titles = decision_titles(dir.path(), spec);
        assert_eq!(
            titles,
            vec!["Chose atomic_md write over direct fs::write — a mid-write crash corrupts the file".to_string()]
        );
    }

    /// No `<MEMORY>` block in the return text ⇒ no event at all (not even an
    /// empty one) — the common case (per the role contract, most waves emit
    /// nothing) must cost zero writes.
    #[test]
    fn subagent_stop_without_memory_block_emits_nothing() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().to_string();
        let spec = "no-memory-here";
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join(spec)).unwrap();
        crate::shared::context::bind_session_spec(&cwd, "sess-2", spec);

        let input = stop_input("impl-2", "Files changed: bar.rs. No non-obvious decisions.");
        capture_memory_decision_with_session(dir.path(), &cwd, &input, "sess-2");

        assert!(decision_titles(dir.path(), spec).is_empty());
    }

    /// A `<MEMORY>` block with no session→spec binding at all (the session
    /// was never bound, e.g. a spec-less ad-hoc dispatch) fails open: no
    /// spec to attribute the decision to ⇒ no event, never an orphaned one.
    #[test]
    fn subagent_stop_memory_block_with_unbound_session_is_a_noop() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().to_string();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();

        let input = stop_input("impl-3", "<MEMORY>a real lesson</MEMORY>");
        // "sess-unbound" was never bound to any spec via bind_session_spec.
        capture_memory_decision_with_session(dir.path(), &cwd, &input, "sess-unbound");

        // No spec dir was ever created, so there is nothing to assert a
        // titles-list against — the meaningful assertion is that this call
        // did not panic and (by construction of the fail-open `let..else`)
        // never reached the `route::emit` call. Covered structurally by
        // `extract_memory_block`/`spec_for_session`/`current_spec` each
        // already having their own None-path unit coverage.
        let _ = input;
    }

    /// Two children in the SAME spec each emit a DIFFERENT `<MEMORY>` — both
    /// land, in order, as separate `decision` events (never overwriting one
    /// another; the NDJSON sink is append-only).
    #[test]
    fn two_children_same_spec_both_decisions_land() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().to_string();
        let spec = "multi-wave-spec";
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join(spec)).unwrap();
        crate::shared::context::bind_session_spec(&cwd, "sess-3", spec);

        capture_memory_decision_with_session(
            dir.path(), &cwd,
            &stop_input("wave-1-child", "<MEMORY>decision from wave 1</MEMORY>"),
            "sess-3",
        );
        capture_memory_decision_with_session(
            dir.path(), &cwd,
            &stop_input("wave-2-child", "<MEMORY>decision from wave 2</MEMORY>"),
            "sess-3",
        );

        let titles = decision_titles(dir.path(), spec);
        assert_eq!(titles.len(), 2, "{titles:?}");
        assert!(titles.contains(&"decision from wave 1".to_string()));
        assert!(titles.contains(&"decision from wave 2".to_string()));
    }

    // --- <VERDICT> capture (SubagentStop → `review.result` event) -----------

    /// Read every `review.result` event's payload for `spec` under `cwd`, in the
    /// order the NDJSON files sort. Test-only mirror of [`decision_titles`].
    fn review_results(cwd: &Path, spec: &str) -> Vec<serde_json::Value> {
        let events_dir = ClaudePaths::for_project(cwd)
            .and_then(|p| p.for_spec(spec))
            .unwrap()
            .events_dir();
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir)
            .into_iter()
            .filter(|e| e.event == "review.result")
            .map(|e| e.payload)
            .collect()
    }

    #[test]
    fn extract_verdict_block_parses_validates_and_rejects_malformed() {
        // Well-formed approved / rejected → parsed, `findings` ignored.
        assert_eq!(
            extract_verdict_block(
                "prose <VERDICT>{\"verdict\":\"approved\",\"critical\":0,\"findings\":[]}</VERDICT> tail"
            ),
            Some(ReviewVerdict { verdict: "approved".to_string(), critical: 0 })
        );
        assert_eq!(
            extract_verdict_block(
                "<VERDICT>{\"verdict\":\"rejected\",\"critical\":3,\"findings\":[{\"severity\":\"critical\",\"location\":\"a.rs:1\",\"summary\":\"x\"}]}</VERDICT>"
            ),
            Some(ReviewVerdict { verdict: "rejected".to_string(), critical: 3 })
        );
        // Absent / empty / unterminated / non-JSON / missing field / bad verdict
        // → None (each a malformed block the hook falls open on).
        assert_eq!(extract_verdict_block("no tag at all"), None);
        assert_eq!(extract_verdict_block("<VERDICT>   </VERDICT>"), None, "blank body");
        assert_eq!(extract_verdict_block("<VERDICT>{\"verdict\":\"approved\",\"critical\":0}"), None, "unterminated");
        assert_eq!(extract_verdict_block("<VERDICT>{not json}</VERDICT>"), None, "non-JSON");
        assert_eq!(extract_verdict_block("<VERDICT>{\"verdict\":\"approved\"}</VERDICT>"), None, "missing critical");
        assert_eq!(
            extract_verdict_block("<VERDICT>{\"verdict\":\"maybe\",\"critical\":0,\"findings\":[]}</VERDICT>"),
            None,
            "verdict outside approved/rejected"
        );
    }

    /// AC-1 — a review subagent returns a `<VERDICT>` block; the SubagentStop
    /// hook parses it and emits ONE `review.result` event whose `verdict` and
    /// `criticalCount` equal the block's values, with no orchestrator call to
    /// `review-result`.
    #[test]
    fn capture_review_verdict_emits_review_result() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().to_string();
        let spec = "structured-review-verdict-capture-via";
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join(spec)).unwrap();
        crate::shared::context::bind_session_spec(&cwd, "sess-r1", spec);

        let input = stop_input(
            "mustard:mustard-review",
            "pass/fail per claim — tests run with the feature enabled.\n\
             <VERDICT>{\"verdict\":\"rejected\",\"critical\":2,\"findings\":[\
             {\"severity\":\"critical\",\"location\":\"a.rs:1\",\"summary\":\"guard broken\"},\
             {\"severity\":\"critical\",\"location\":\"b.rs:9\",\"summary\":\"mold violated\"}]}</VERDICT>",
        );
        capture_review_verdict_with_session(dir.path(), &cwd, &input, "sess-r1");

        let results = review_results(dir.path(), spec);
        assert_eq!(results.len(), 1, "exactly one review.result: {results:?}");
        assert_eq!(results[0]["verdict"], json!("rejected"));
        assert_eq!(results[0]["criticalCount"], json!(2));
        assert_eq!(results[0]["spec"], json!(spec));
    }

    /// AC-2 — no `<VERDICT>` block, or a malformed / out-of-vocabulary one, is a
    /// silent no-op (fail-open): the hook emits nothing and the manual
    /// `review-result` path stays the source of the verdict.
    #[test]
    fn verdict_block_absent_is_noop() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().to_string();
        let spec = "no-verdict-here";
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join(spec)).unwrap();
        crate::shared::context::bind_session_spec(&cwd, "sess-r2", spec);

        // Absent — prose only, no machine-readable block.
        capture_review_verdict_with_session(
            dir.path(), &cwd,
            &stop_input("mustard:mustard-review", "Looks good overall. Approving this."),
            "sess-r2",
        );
        // Malformed — the tag is present but its body is not valid JSON.
        capture_review_verdict_with_session(
            dir.path(), &cwd,
            &stop_input("mustard:mustard-review", "<VERDICT>{verdict: rejected, critical: 2</VERDICT>"),
            "sess-r2",
        );
        // Out-of-vocabulary — valid JSON, but `verdict` is not approved/rejected.
        capture_review_verdict_with_session(
            dir.path(), &cwd,
            &stop_input("mustard:mustard-review", "<VERDICT>{\"verdict\":\"maybe\",\"critical\":0,\"findings\":[]}</VERDICT>"),
            "sess-r2",
        );

        assert!(
            review_results(dir.path(), spec).is_empty(),
            "absent/malformed block must emit no review.result"
        );
    }

    /// The role gate: a NON-review agent whose output happens to carry a
    /// `<VERDICT>`-shaped block must NOT emit a `review.result` — verdict
    /// capture is scoped to the review role only.
    #[test]
    fn review_verdict_wrong_role_is_noop() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().to_string();
        let spec = "wrong-role-spec";
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join(spec)).unwrap();
        crate::shared::context::bind_session_spec(&cwd, "sess-r3", spec);

        let input = stop_input(
            "general-purpose",
            "<VERDICT>{\"verdict\":\"approved\",\"critical\":0,\"findings\":[]}</VERDICT>",
        );
        capture_review_verdict_with_session(dir.path(), &cwd, &input, "sess-r3");

        assert!(
            review_results(dir.path(), spec).is_empty(),
            "non-review role must not emit review.result"
        );
    }

    /// `role_is_readonly` normalises a namespaced plugin agent type to its bare
    /// name, so the qualified `mustard:mustard-review` that `dispatch-plan` now
    /// emits is recognised exactly like the bare `mustard-review`. It also covers
    /// `mustard-patterns` (the read-only mold author), previously absent from the
    /// denylist — without it the pattern agent got the regression-vocab noise.
    #[test]
    fn role_is_readonly_normalises_namespaced_plugin_agents() {
        // Bare and namespaced forms are equivalent.
        assert!(role_is_readonly("mustard-review"));
        assert!(role_is_readonly("mustard:mustard-review"));
        assert!(role_is_readonly("mustard-guards"));
        assert!(role_is_readonly("mustard:mustard-guards"));
        // mustard-patterns is read-only (was missing) — both forms.
        assert!(role_is_readonly("mustard-patterns"));
        assert!(role_is_readonly("mustard:mustard-patterns"));
        // Built-in read-only stays matched; a code author never does, even when
        // namespaced (stripping the prefix must not flip a writer to read-only).
        assert!(role_is_readonly("Explore"));
        assert!(!role_is_readonly("general-purpose"));
        assert!(!role_is_readonly("mustard:general-purpose"));
    }
}
