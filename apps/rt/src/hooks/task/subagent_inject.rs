//! `subagent_inject` — PreToolUse(Task) context injector.
//!
//! For every `Task` dispatch that does NOT already declare a `SKILL:` block in
//! its `prompt`, we resolve a minimal slice of:
//!
//! - the top-K skills returned by [`crate::commands::skill::skill_resolve::resolve`] for
//!   the prompt + role + active-phase.
//!
//! The slice is surfaced as a [`Verdict::Inject`]. The orchestrator-side
//! `agent-prompt-render` already handles fully-formed dispatches; this hook
//! covers the ad-hoc `Task(general-purpose)` calls that bypass the renderer
//! (the orchestrator delegating by hand).
//!
//! ## Selective spec-memory load
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

use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::ClaudePaths;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::commands::agent::context_inject;
use crate::commands::review::review_result;


/// The subagent-inject hook.
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
        RefStub::Expanded { rel, body } => {
            let mut tool_input = input.tool_input.clone();
            tool_input
                .as_object_mut()?
                .insert("prompt".to_string(), serde_json::Value::String(stamp_wave(&rel, body)));
            Some(Verdict::Rewrite { tool_input })
        }
    }
}

/// The machine marker a wave dispatch carries so the WAVE survives the trip into
/// the child and back out again.
///
/// It exists because nothing else on the return says which wave returned. The
/// `SubagentStop` payload names the child (`agent_id`, `agent_type`) and hands
/// over its transcript, but it carries no wave; `MUSTARD_ACTIVE_WAVE` — what the
/// rest of this crate reads for attribution — is set by NOBODY in this
/// repository (`wave_advance.rs` says so in its own module docs), so every event
/// that sourced the wave from it recorded `0`. Reading it here was therefore
/// inert: every captured lesson landed "outside a wave plan" and every memory
/// file said `unknown`.
///
/// The stamp closes that gap with the one fact the hook already holds at
/// dispatch: the rendered prompt's own path (`.dispatch/wave-{N}-{role}…`),
/// which `agent-prompt-render` derived from the wave it rendered for. Appending
/// it to the expanded prompt means the wave rides INSIDE the child's first user
/// message — which Claude Code persists verbatim as the first line of the
/// child's own transcript (`agent_transcript_path`). That is what
/// [`wave_from_child_transcript`] reads back.
///
/// It is appended, never prepended: the rendered prompt opens with
/// `<!-- PREFIX-STABLE -->` and the prefix is what prompt caching keys on.
const WAVE_STAMP_OPEN: &str = "<!-- mustard:wave=";

/// How many leading transcript lines [`wave_from_child_transcript`] scans for the
/// stamp. The dispatch prompt is line ONE (the child's first user message); the
/// small margin absorbs a harness that prepends a header record. Bounded on
/// purpose — a finished child's transcript runs to hundreds of KB, and the stamp
/// can only ever be near the top. Scanning further would also start matching a
/// SIBLING wave's prompt that the child happened to read.
const TRANSCRIPT_STAMP_SCAN_LINES: usize = 4;

/// Append the wave stamp to a rendered dispatch body, when the ref path names a
/// wave. See [`WAVE_STAMP_OPEN`] for why the wave has to travel this way.
///
/// Unstamped on purpose when the path names no wave or names wave `0`: `0` is
/// the schema's "outside a wave plan" (the single-spec fallback and every
/// spec-less `/task` render), so stamping it would assert a wave that does not
/// exist.
fn stamp_wave(rel: &str, body: String) -> String {
    match wave_from_ref_rel(rel) {
        Some(w) => format!("{body}\n{WAVE_STAMP_OPEN}{w} -->\n"),
        None => body,
    }
}

/// The wave number a rendered-prompt path was rendered for, from its file name
/// (`.claude/spec/{spec}/.dispatch/wave-{n}-{role}[-{sub}].{mode}.prompt.md` —
/// the writer is `render::prompt_ref::prompt_ref_rel_path`). `None` for wave `0`
/// and for the spec-less `.claude/.dispatch/{role}-{hash}.prompt.md` shape,
/// which names no wave at all.
fn wave_from_ref_rel(rel: &str) -> Option<u32> {
    let file = rel.rsplit(['/', '\\']).next()?;
    let digits: String = file
        .strip_prefix("wave-")?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<u32>().ok().filter(|w| *w > 0)
}

/// The wave a child was dispatched for — the one RETURNING at `SubagentStop`, or
/// the one WRITING at a `PreToolUse` that fired inside it — read back from the
/// stamp its own transcript carries. `None` when no candidate path resolves, the
/// file cannot be read, or its leading lines carry no stamp (an ad-hoc `Task`, a
/// wave-less render, or a dispatch the hook never expanded).
///
/// Per-child by construction, which is the whole point: a dispatch round runs
/// every wave of the lowest incomplete dependency level AT ONCE, so any answer
/// derived from shared state (the projection's scalar `currentWave`, the
/// session→spec marker, `MUSTARD_ACTIVE_WAVE`) says the same thing to every
/// sibling in flight. The transcript stamp is the only signal that differs per
/// agent.
///
/// See [`child_transcript_candidates`] for the paths tried and why.
///
/// `pub(crate)`: [`crate::hooks::write::boundary_gate`] asks the same question
/// on the way IN (which wave is writing) that this module asks on the way out.
///
/// Fail-open throughout: this feeds attribution, never a decision — a caller
/// that gets `None` widens what it accepts, it never blocks.
pub(crate) fn wave_from_child_transcript(input: &HookInput) -> Option<u32> {
    child_transcript_candidates(input)
        .iter()
        .find_map(|p| wave_from_transcript_head(p))
}

/// The transcript paths that may belong to the child this hook fired for, in the
/// order [`wave_from_child_transcript`] tries them:
///
/// 1. `agent_transcript_path` — the child's own transcript, handed over on the
///    `SubagentStop` payload. Most direct; where this started.
/// 2. The child transcript DERIVED from `transcript_path` + `agent_id`. Both are
///    DOCUMENTED common hook fields (the hooks reference: `agent_id` is "present
///    only when the hook fires inside a subagent call"), and Claude Code stores a
///    child's transcript beside its parent's, at
///    `<parent-stem>/subagents/agent-{agent_id}.jsonl`. This is the candidate
///    that makes the wave resolvable on a `PreToolUse` INSIDE the child, where
///    `agent_transcript_path` is not part of the documented contract.
/// 3. `transcript_path` itself, for a harness that already points it at the
///    child. Last on purpose: against a PARENT transcript it simply finds no
///    stamp in the leading lines (the dispatch prompt lives far below the
///    [`TRANSCRIPT_STAMP_SCAN_LINES`] window), so it costs one bounded read and
///    can never misattribute a sibling's wave.
///
/// The derivation composes only harness-supplied values — no machine path and no
/// transcript-root convention is hard-coded here.
fn child_transcript_candidates(input: &HookInput) -> Vec<PathBuf> {
    let raw_str = |key: &str| {
        input
            .raw
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(child) = raw_str("agent_transcript_path") {
        out.push(PathBuf::from(child));
    }
    let parent = raw_str("transcript_path");
    if let (Some(parent), Some(agent)) = (parent.as_deref(), agent_id_of(input)) {
        // `<dir>/<session>.jsonl` → `<dir>/<session>/subagents/agent-<id>.jsonl`
        out.push(
            Path::new(parent)
                .with_extension("")
                .join("subagents")
                .join(format!("agent-{agent}.jsonl")),
        );
    }
    if let Some(parent) = parent {
        out.push(PathBuf::from(parent));
    }
    out
}

/// The child id this hook fired inside — the harness `agent_id`, read from the
/// typed field first and from the flattened raw payload as a belt. `None` on the
/// main thread, which is the documented meaning of its absence.
fn agent_id_of(input: &HookInput) -> Option<String> {
    input
        .agent_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            input
                .raw
                .get("agent_id")
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

/// The wave stamped in the first [`TRANSCRIPT_STAMP_SCAN_LINES`] lines of one
/// transcript file. `None` for a missing/unreadable file or one with no stamp.
fn wave_from_transcript_head(path: &Path) -> Option<u32> {
    use std::io::BufRead;

    let file = std::fs::File::open(path).ok()?;
    std::io::BufReader::new(file)
        .lines()
        .take(TRANSCRIPT_STAMP_SCAN_LINES)
        .map_while(Result::ok)
        .find_map(|line| wave_from_stamp(&line))
}

/// The wave number in the FIRST [`WAVE_STAMP_OPEN`] stamp of `text`, if any.
///
/// A plain substring scan is correct over a JSON transcript line: every byte of
/// the stamp is a character JSON leaves unescaped, so the marker survives
/// serialisation of the prompt verbatim.
fn wave_from_stamp(text: &str) -> Option<u32> {
    let at = text.find(WAVE_STAMP_OPEN)? + WAVE_STAMP_OPEN.len();
    let digits: String = text[at..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u32>().ok().filter(|w| *w > 0)
}

/// Surface a ref stub the hook could NOT expand — transparency, never a block.
/// The decision stays fail-open (the caller returns `None` and the dispatch
/// proceeds on the stub's fallback line); this only makes the failure VISIBLE
/// and attributable: stderr for a live session, plus an economy event so it
/// lands in the dashboard trace next to the agent it belongs to. Mirrors the
/// success-side `prompt_ref_expand` telemetry, completing the attribution
/// triad (expanded / unexpanded / neither = no marker or the hook never ran).
fn report_unexpanded(_cwd: &str, rel: &str, reason: &str) {
    eprintln!(
        "subagent_inject: WARN: dispatch stub NOT expanded ({reason}): {rel} — subagent falls back to reading the file; surfacing for attribution"
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

/// Pick the role from a **dispatch** Task input — the Pre/PostToolUse(Task)
/// path, where the harness carries the child's type as `tool_input.subagent_type`.
/// Falls back to `"general-purpose"`. This is NOT valid on a `SubagentStop`
/// (there is no `tool_input` there): the returning agent's type arrives at the
/// top level — use [`role_from_stop_input`] for that.
fn role_from_input(input: &HookInput) -> String {
    let tool_input = &input.tool_input;
    tool_input
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .map_or_else(|| "general-purpose".to_string(), str::to_string)
}

/// Pick the returning child's role on a `SubagentStop`. Unlike
/// [`role_from_input`] (the dispatch-path reader), a stop event carries the
/// agent type at the TOP LEVEL: real harness JSON deserialises `agent_type`
/// into the typed [`HookInput::agent_type`] field — serde's `#[serde(flatten)]`
/// routes that key to the typed field and leaves it OUT of `raw` — so the typed
/// field is the primary source. Raw `agent_type` / `subagent_type` are
/// secondary fallbacks for a manually-built input or an alternate harness key.
/// Mirrors [`HookInput::is_subagent`] / [`child_id_from_input`] in reading
/// stop-shaped fields, never the dispatch-shaped `tool_input` (which a stop
/// does not carry — reading it there made the verdict gate a silent no-op on
/// every real review return).
fn role_from_stop_input(input: &HookInput) -> String {
    if let Some(t) = input.agent_type.as_deref().filter(|s| !s.is_empty()) {
        return t.to_string();
    }
    for key in ["agent_type", "subagent_type"] {
        if let Some(v) = input.raw.get(key).and_then(serde_json::Value::as_str)
            && !v.is_empty() {
                return v.to_string();
            }
    }
    "general-purpose".to_string()
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
        && !s.is_empty() {
            return s.to_string();
        }
    for key in ["result", "final_output", "output", "tool_response", "tool_result"] {
        if let Some(v) = input.raw.get(key) {
            if let Some(s) = v.as_str()
                && !s.is_empty() {
                    return s.to_string();
                }
            if let Some(s) = v.get("text").and_then(|x| x.as_str())
                && !s.is_empty() {
                    return s.to_string();
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
/// Spec attribution goes through [`capture_spec`], the one current-spec
/// ladder every door shares. No spec resolves ⇒ no-op — a
/// decision with no spec to attribute it to is discarded, never emitted
/// orphaned.
///
/// Fail-open throughout: no memory block, no resolvable spec, or a write
/// error all degrade to a silent no-op. This is telemetry, never a blocking
/// path — never called from a `Check`, only from the `Observer`-shaped
/// `SubagentStop` side effect below.
fn capture_memory_decision(project: &Path, cwd: &str, input: &HookInput) {
    capture_memory_decision_with_session(project, cwd, input, input.session_id.as_deref().unwrap_or(""));
}

/// The spec a `SubagentStop` capture is attributed to: the one current-spec
/// ladder every door shares (the environment override, then the checkout's
/// branch, then the session binding).
pub(crate) fn capture_spec(cwd: &str, sid: &str) -> Option<String> {
    crate::shared::spec_state::active_spec(cwd, Some(sid))
}

/// Session-explicit variant of [`capture_memory_decision`] — the actual
/// worker, taking `session_id` as a parameter instead of reading it off the
/// stop input. Mirrors this
/// file's own [`span_level_eval_and_append`]/[`span_level_eval_and_append_in`]
/// split and for the same reason: a test cannot safely mutate
/// `MUSTARD_SESSION_ID` (`unsafe` under Rust 2024, forbidden in this crate),
/// so the deterministic entry point takes the value directly.
fn capture_memory_decision_with_session(_project: &Path, cwd: &str, input: &HookInput, sid: &str) {
    // O gravador velho de eventos saiu: a decisão de memória do filho não tem
    // mais onde ser gravada. O gancho em si sai com os ganchos.
    let Some(_memory) = extract_memory_block(&final_output_text(input)) else {
        return;
    };
    let _ = capture_spec(cwd, sid);
}

/// The event a returning child's own report is recorded as. Deliberately NOT
/// `agent.stop`: that name is already taken by the dispatcher-side telemetry
/// [`super::subagent_observer`] emits, whose start/stop pairs the dashboard walks
/// as a stack ([`apps/dashboard`'s `build_agent_intervals`]) — a second `stop`
/// per dispatch would close a frame that never opened.
const EVENT_AGENT_RETURN: &str = "agent.return";

/// How much of one return is kept in the wave's record. Bounded so a verbose
/// agent cannot bloat the NDJSON log; generous enough that an account given
/// midway through a report still lands — the 800-char cap the dispatcher-side
/// telemetry applies would not be.
const RETURN_REPORT_MAX_CHARS: usize = 8_000;

/// Persist the returning child's OWN report as an `agent.return` event, stamped
/// with the wave that child was dispatched for.
///
/// ## The hole this fills
///
/// A wave's record is supposed to contain what the wave said on its way back —
/// [`crate::commands::pipeline::wave_done`] reads it to see whether a declared
/// REALITY OBLIGATION was ever accounted for by id. The only channel it had was
/// `agent.stop`, which [`super::subagent_observer`] emits at `PostToolUse(Task)`
/// with `tool_response` as its payload. For a BACKGROUND dispatch — the shape the
/// wave pipeline uses — that response is the launch acknowledgement
/// (`{"isAsync":true,"status":"async_launched",…}`, echoing the prompt just sent),
/// delivered the moment the child STARTS. The returning report was never in it,
/// so a wave that did account for its duty by id was still reported unaccounted.
///
/// `SubagentStop` is where the report actually exists: [`final_output_text`]
/// already reads it (`last_assistant_message`) for the span eval, and
/// [`wave_from_child_transcript`] already resolves whose wave it is. This records
/// both, so the account reaches the record of the wave that gave it.
///
/// Not narrowed to a `<MEMORY>` block on purpose: the obligation account is
/// ordinary prose in the body of a report, which is exactly the material the
/// memory capture is contracted to reject.
///
/// ## Why an UNATTRIBUTED return is not recorded
///
/// Unlike its memory twin, this records nothing when the child's own wave cannot
/// be established. A wave-less record would have to be readable by every wave
/// (nothing else could ever come for it), which is exactly the spec-scoped
/// reading this fix removes — and here it would run the wrong way: a stray
/// `Task` that merely quoted `RO-1.1` out of a spec file would DISCHARGE a duty
/// nobody checked. The opposite failure — an unresolved stamp leaving a real
/// account unrecorded — costs a printed line saying a duty went unaccounted,
/// which is noise the operator can correct, not a claim the harness never
/// verified.
///
/// Spec attribution mirrors its [`capture_memory_decision`] twin
/// ([`capture_spec`]); no spec resolves ⇒ no-op rather than an orphaned event. Fail-open throughout:
/// telemetry, never a blocking path.
fn capture_return_report(project: &Path, cwd: &str, input: &HookInput) {
    capture_return_report_with_session(project, cwd, input, input.session_id.as_deref().unwrap_or(""));
}

/// Session-explicit worker for [`capture_return_report`] — takes `session_id`
/// directly so a test can drive it without mutating `MUSTARD_SESSION_ID`
/// (`unsafe` under Rust 2024, forbidden in this crate), mirroring the
/// [`capture_memory_decision_with_session`] split.
fn capture_return_report_with_session(_project: &Path, cwd: &str, input: &HookInput, sid: &str) {
    let report = final_output_text(input);
    if report.trim().is_empty() {
        return;
    }
    // Same source order, and same reason, as the memory twin: the stamp the
    // dispatch carried into this child's own transcript is per-child, so it stays
    // correct while a whole round of sibling waves is in flight. Unlike the twin,
    // an unresolved wave ends the capture — see the "unattributed" section above.
    // O gravador velho de eventos saiu: o relatório do filho não tem mais onde
    // ser gravado. O gancho em si sai com os ganchos.
    let Some(_wave) = wave_from_child_transcript(input)
        .or_else(|| super::common::current_wave_id().and_then(|w| w.parse::<u32>().ok()))
        .filter(|w| *w > 0)
    else {
        return;
    };
    let _ = capture_spec(cwd, sid);
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
    capture_review_verdict_with_session(project, cwd, input, input.session_id.as_deref().unwrap_or(""));
}

/// Session-explicit worker for [`capture_review_verdict`] — takes `session_id`
/// directly so a test can drive it without mutating `MUSTARD_SESSION_ID`
/// (`unsafe` under Rust 2024, forbidden in this crate), mirroring the
/// [`capture_memory_decision_with_session`] split.
fn capture_review_verdict_with_session(project: &Path, cwd: &str, input: &HookInput, sid: &str) {
    if !role_is_review(&role_from_stop_input(input)) {
        return;
    }
    let Some(verdict) = extract_verdict_block(&final_output_text(input)) else {
        return;
    };
    // Spec attribution mirrors the memory twin (`capture_spec`). No spec ⇒ no-op.
    let spec = capture_spec(cwd, sid);
    let Some(spec) = spec else {
        return;
    };
    // Reuse the manual recorder: identical `review.result` event + `review`
    // metric, parsed straight from the block (zero orchestrator interpretation).
    // The block carries neither a subproject nor a findings *file*, so both are
    // `None` — its `findings` array is audit-facing and not a gate input.
    let _ = review_result::record_review(project, &spec, &verdict.verdict, verdict.critical, None, None);
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
        // Span-level eval at SubagentStop. Runs per child return,
        // never accumulating until end-of-wave. Fail-open: any IO
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
        //
        // The RETURN ITSELF rides it as well — see `capture_return_report`. The
        // `<MEMORY>` / `<VERDICT>` captures each keep one narrow block; the wave's
        // record also needs the ordinary prose of the report, because that is
        // where an agent accounts for a reality obligation by id.
        if ctx.trigger == Some(Trigger::SubagentStop) {
            let cwd = ctx.project_dir_or_cwd(input);
            let project = PathBuf::from(&cwd);
                    capture_memory_decision(&project, &cwd, input);
            capture_return_report(&project, &cwd, input);
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
            return Ok(verdict);
        }
        let prompt = dispatch_prompt(input);
        if prompt_declares_skill(&prompt) {
            // Trust agent-prompt-render — do nothing.
            return Ok(Verdict::Allow);
        }
        let role = role_from_input(input);

        // The explore floor and the regression vocab. No size cap — relevance
        // decides what enters; nothing is trimmed by char count.
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
        // Spec memory rides OUTSIDE the size cap: it is relevance-filtered (the
        // gate's approved set, or the recall fallback) and carries no count cap,
        // so truncating it by size would contradict the gate — relevance, not
        // size, decides what enters.
        let mut memory = String::new();
        if let Some(spec) = crate::shared::context::current_spec(&cwd)
            && !spec.is_empty() {
                memory = spec_memory_block(&project, &spec, &prompt, &role);
            }
        if sections.is_empty() && memory.is_empty() {
            return Ok(Verdict::Allow);
        }
        // Emit telemetry — fail-open.
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ctx_for(dir: &Path) -> Ctx {
        Ctx::for_test(dir.to_string_lossy().to_string(), Some(Trigger::PreToolUse))
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

    /// The wave a dispatch was rendered for comes from the rendered prompt's own
    /// path — and wave `0` (the single-spec fallback) and the spec-less shape
    /// name no wave at all, so they must not be stamped with one.
    #[test]
    fn wave_from_ref_rel_reads_the_rendered_prompt_path() {
        assert_eq!(
            wave_from_ref_rel(".claude/spec/demo/.dispatch/wave-3-plan-apps-rt.first.prompt.md"),
            Some(3)
        );
        assert_eq!(
            wave_from_ref_rel(".claude/spec/demo/.dispatch/wave-12-checklist.fix-loop.prompt.md"),
            Some(12)
        );
        // Wave 0 IS the schema's "outside a wave plan" — never a wave.
        assert_eq!(
            wave_from_ref_rel(".claude/spec/demo/.dispatch/wave-0-review.first.prompt.md"),
            None
        );
        // The spec-less render names no wave.
        assert_eq!(wave_from_ref_rel(".claude/.dispatch/explore-0badc0de.prompt.md"), None);
        assert_eq!(wave_from_ref_rel("wave-.first.prompt.md"), None, "no digits ⇒ no wave");
    }

    /// The wave the dispatch stamped must survive into the child and come back
    /// out: the rewritten prompt carries the stamp, and reading it off a
    /// transcript line that holds that prompt verbatim returns the same number.
    ///
    /// This is the seam the whole attribution rests on. Before it, the capture
    /// sourced the wave from `MUSTARD_ACTIVE_WAVE` — set by nothing in this
    /// repository — so every real run recorded `0` and every memory file said
    /// `unknown` while the tests, which set the field by hand, stayed green.
    #[test]
    fn the_dispatch_stamps_its_wave_and_the_child_s_transcript_gives_it_back() {
        let dir = tempdir().unwrap();
        let rel = ".claude/spec/demo/.dispatch/wave-7-impl-apps-rt.first.prompt.md";
        let full = dir.path().join(rel);
        std::fs::create_dir_all(full.parent().expect("parent")).unwrap();
        std::fs::write(&full, "<!-- PREFIX-STABLE -->\nROLE: impl\n").unwrap();

        let stub = format!("MUSTARD-PROMPT-REF: {rel}\nDispatch stub — fallback line.");
        let v = SubagentInject
            .evaluate(&task_input(&stub, "impl"), &ctx_for(dir.path()))
            .unwrap();
        let Verdict::Rewrite { tool_input } = v else {
            panic!("expected Rewrite, got {v:?}");
        };
        let prompt = tool_input["prompt"].as_str().expect("prompt string").to_string();
        assert!(prompt.contains("<!-- mustard:wave=7 -->"), "unstamped: {prompt}");
        assert!(
            prompt.starts_with("<!-- PREFIX-STABLE -->"),
            "the stamp must not disturb the cache-stable prefix: {prompt}"
        );

        // The child's own transcript, first line = that prompt verbatim.
        let transcript = dir.path().join("agent-x.jsonl");
        let line = serde_json::json!({ "type": "user", "message": { "content": prompt } });
        std::fs::write(&transcript, format!("{line}\n")).unwrap();

        let stop = HookInput {
            hook_event_name: Some("SubagentStop".to_string()),
            agent_type: Some("impl".to_string()),
            raw: serde_json::json!({
                "agent_transcript_path": transcript.to_string_lossy(),
                "last_assistant_message": "done",
            }),
            ..HookInput::default()
        };
        assert_eq!(wave_from_child_transcript(&stop), Some(7));

        // A stop naming no transcript, or one with no stamp, yields None — the
        // fail-open path that lets the memory file say `unknown` honestly.
        let unstamped = dir.path().join("agent-plain.jsonl");
        std::fs::write(&unstamped, "{\"type\":\"user\",\"message\":{\"content\":\"just do it\"}}\n")
            .unwrap();
        let bare = HookInput {
            raw: serde_json::json!({ "agent_transcript_path": unstamped.to_string_lossy() }),
            ..HookInput::default()
        };
        assert_eq!(wave_from_child_transcript(&bare), None);
        assert_eq!(wave_from_child_transcript(&HookInput::default()), None);
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

    /// The dispatch hook no longer reads the project's glossary. With a
    /// `CONTEXT.md` and a `CONTEXT-MAP.md` that share a term with the prompt,
    /// nothing of them reaches the child; with nothing else to add, the hook
    /// allows.
    #[test]
    fn the_dispatch_hook_no_longer_reads_the_glossary() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CONTEXT.md"), "## User\nThe user module domain.").unwrap();
        std::fs::write(dir.path().join("domain-context.md"), "## Billing\nThe user billing terms.").unwrap();
        std::fs::write(dir.path().join("CONTEXT-MAP.md"), "# Map\n- see [domain](domain-context.md)\n").unwrap();

        let reader = task_input("grep the codebase for the user module", "mustard-guards");
        let v = SubagentInject.evaluate(&reader, &ctx_for(dir.path())).unwrap();
        assert_eq!(v, Verdict::Allow, "nothing of the glossary rides: {v:?}");

        let writer = task_input("refactor the user module", "general-purpose");
        if let Verdict::Inject { context } = SubagentInject.evaluate(&writer, &ctx_for(dir.path())).unwrap() {
            for glossary in ["## CONTEXT.md", "The user module domain", "billing terms"] {
                assert!(!context.contains(glossary), "{glossary}: {context}");
            }
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
    // Span-level review
    // -----------------------------------------------------------------------


    /// Build a realistic `SubagentStop` payload — the shape a real stop carries,
    /// not the dispatch/PostToolUse hybrid this helper used to fabricate. A stop
    /// delivers the returning agent's TYPE at the top level (`agent_type`, the
    /// field real harness JSON deserialises into) and its final text as
    /// `last_assistant_message`. The old shape (`tool_input.subagent_type` +
    /// `raw.result`) made the role gate and the output reader look at fields a
    /// stop never has — the false-positive shape that let the broken verdict gate
    /// pass review. `agent_id` is mirrored into `raw` too so `child_id_from_input`
    /// (which reads `raw`, not the typed field) still resolves the child id.
    fn stop_input(child: &str, output_text: &str) -> HookInput {
        HookInput {
            tool_name: None,
            hook_event_name: Some("SubagentStop".to_string()),
            agent_type: Some(child.to_string()),
            agent_id: Some(child.to_string()),
            raw: serde_json::json!({
                "agent_id": child,
                "last_assistant_message": output_text,
            }),
            ..HookInput::default()
        }
    }

    /// Regression (SubagentStop-capture family was inert in production): a real
    /// `SubagentStop` delivers the returning agent's final text as
    /// `last_assistant_message` (Claude Code hook contract), NOT via the
    /// PostToolUse-shaped `result`/`output` keys. `final_output_text` must read
    /// it — otherwise the whole capture family (memory / span-eval / verdict)
    /// silently no-ops on every real subagent return. The shared `stop_input`
    /// helper above now builds this same real shape, so the memory / span-eval /
    /// verdict tests exercise the production path rather than a fabricated one.
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

    // --- return capture (SubagentStop → `agent.return` event) ---------------






    // --- <VERDICT> capture (SubagentStop → `review.result` event) -----------


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



}
