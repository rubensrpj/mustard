//! `budget` — the consolidated Task-prompt / agent-return size module.
//!
//! ## Scope (b3 Wave 3, Task family)
//!
//! This module ports the **size** concerns of two JavaScript hooks:
//!
//! - `context-budget.js` — a `PreToolUse(Task)` gate that blocks a Task
//!   dispatch whose `prompt` length exceeds the per-role char budget, and
//!   advises (never blocks) when the estimated context crosses the "Dumb Zone"
//!   percentage of the model window.
//! - `output-budget.js` — a `PostToolUse(Task)` advisory that warns when an
//!   agent's `tool_response` exceeds the per-role line cap.
//!
//! Consolidation **regroups, it does not re-decide** — every verdict is a 1:1
//! port of the JS decision logic. The parity tests at the bottom of this file
//! mirror `__tests__/integration.test.js` (Suite 2 / Suite 3) and
//! `hooks.test.js` ("context-budget.js metrics emission") case by case.
//!
//! ## Budget↔tracker boundary
//!
//! `budget` owns only **size of a prompt** and **size of a return**. The
//! tool-use / main-context *count* caps live in [`crate::hooks::task::tracker`] —
//! see that module's header for the rationale. There is deliberately no
//! counting logic here.
//!
//! ## `BudgetGuard` is a `Check` for both budgets
//!
//! `context-budget` is a gate (`Check`) on `PreToolUse(Task)`. `output-budget`
//! is an advisory on `PostToolUse(Task)` — it never blocks and never rewrites.
//!
//! Through Wave 3 `output-budget` was an [`Observer`] that, on an over-budget
//! return, wrote the `hookSpecificOutput.additionalContext` advisory **direct
//! to stdout** with a raw `println!`, bypassing the dispatcher's single
//! `emit_outcome` (b3 Wave-3 Concern "`budget::observe` stdout bypass" — under
//! the consolidated binary two JSON objects could leave one invocation).
//!
//! Wave 5 resolves it: `output-budget` is now part of the `Check` path. On
//! `PostToolUse(Task)` [`BudgetGuard::evaluate`] emits the return-size metric
//! and, when over budget, returns a [`Verdict::Inject`] carrying the advisory.
//! The dispatcher folds that `Inject` into the single `Outcome`, so exactly
//! one JSON object is emitted per invocation. `BudgetGuard` no longer
//! implements [`Observer`].
//!
//! ## Mode
//!
//! `context-budget.js` reads `CONTEXT_BUDGET_MODE` (`observe` / `warn` /
//! `strict`, default `strict`) — note this is *not* the `MUSTARD_*_MODE`
//! naming, so the gate resolves its own mode rather than relying on the
//! dispatcher's module-level mode. The dispatcher repasses the verdict without
//! downgrade.

use mustard_core::domain::economy::estimator;
use mustard_core::platform::error::Error;
use mustard_core::platform::metrics::{MetricLine, emit_metric};
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json};

use crate::shared::context::current_spec;
use crate::util::{format_gate_message, now_iso8601};

/// Emit a `pipeline.economy.savings.budget-output-cut` NDJSON event for an
/// over-budget agent return. The `tokens_saved` field carries the estimated
/// token count we avoided re-injecting into the parent context.
/// Fail-open on every error path — telemetry never blocks the verdict.
fn record_output_cut(
    project_dir: &str,
    dropped_tail: &str,
    role_label: &str,
    model_hint: Option<&str>,
) {
    if dropped_tail.is_empty() {
        return;
    }
    let saved = i64::from(estimator::estimate_output_tokens(dropped_tail, model_hint.unwrap_or("")));
    let saved = saved.max(1);
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: "unknown".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("budget".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.savings.budget-output-cut".to_string(),
        payload: json!({
            "source": "BudgetOutputCut",
            "tokens_saved": saved,
            "model_target": model_hint,
            "role": role_label,
            "spec_id": current_spec(project_dir),
            "wave_id": std::env::var("MUSTARD_ACTIVE_WAVE").ok().filter(|s| !s.is_empty()),
        }),
        spec: current_spec(project_dir),
    };
    let _ = crate::shared::events::route::emit(project_dir, &event);
}

// ---------------------------------------------------------------------------
// Shared role classification
// ---------------------------------------------------------------------------

/// The pipeline role a Task dispatch represents, as far as both budgets care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// `subagent_type == "explore"`.
    Explore,
    /// `subagent_type == "plan"`.
    Plan,
    /// `subagent_type == "general-purpose"` with `review` in the description.
    GeneralReview,
    /// `subagent_type == "general-purpose"`, any other description.
    General,
    /// Any other / unknown `subagent_type`.
    Unknown,
}

/// Classify a Task dispatch into a [`Role`] from its `subagent_type` and
/// `description`. Mirrors the lowercase comparison both JS hooks do.
fn classify_role(subagent_type: &str, description: &str) -> Role {
    let ty = subagent_type.to_ascii_lowercase();
    let desc = description.to_ascii_lowercase();
    match ty.as_str() {
        "explore" => Role::Explore,
        "plan" => Role::Plan,
        "general-purpose" => {
            if desc.contains("review") {
                Role::GeneralReview
            } else {
                Role::General
            }
        }
        _ => Role::Unknown,
    }
}

// ---------------------------------------------------------------------------
// context-budget — PreToolUse(Task) prompt-size gate
// ---------------------------------------------------------------------------

/// Role char budgets (1 token ≈ 4 chars), from `context-budget.js`.
const BUDGET_EXPLORE: usize = 10_000; // 2,500 tokens × 4
const BUDGET_REVIEW: usize = 12_000; // 3,000 tokens × 4
const BUDGET_GENERAL: usize = 30_000; // 7,500 tokens × 4

/// The char budget for a role, or `None` for an advisory-only role (`Plan` and
/// unknown types have no hard block — `getBudget` returns `null`).
fn prompt_budget(role: Role) -> Option<usize> {
    match role {
        Role::Explore => Some(BUDGET_EXPLORE),
        Role::GeneralReview => Some(BUDGET_REVIEW),
        Role::General => Some(BUDGET_GENERAL),
        Role::Plan | Role::Unknown => None,
    }
}

/// Public, read-only accessor for the per-role prompt char budget — used by
/// the Wave 4 `context-resolve` resolver to truncate a graph closure that
/// would exceed the role's ceiling. Accepts the same role labels the
/// `subagent_type` / description heuristic emits (case-insensitive, with
/// `general-purpose(review)` recognised as the review tier). Returns `None`
/// for advisory-only roles (`plan`, `unknown`, or any unmapped label) — the
/// caller then leaves the closure unconstrained.
///
/// The mapping is identical to [`prompt_budget`]; this wrapper exists only
/// so callers outside the `hooks::budget` module can read the table without
/// either duplicating the constants or perturbing the gate's logic.
#[must_use]
pub fn role_prompt_budget(role_label: &str) -> Option<usize> {
    let role = match role_label.to_ascii_lowercase().as_str() {
        "explore" => Role::Explore,
        "plan" => Role::Plan,
        // Accept both the synthesised label and the raw subagent type.
        "general-purpose(review)" | "review" => Role::GeneralReview,
        "general-purpose" | "general" => Role::General,
        _ => Role::Unknown,
    };
    prompt_budget(role)
}

/// The human role label `context-budget.js` uses in its messages/metrics.
fn prompt_role_label(role: Role, subagent_type: &str) -> String {
    match role {
        Role::GeneralReview => "general-purpose(review)".to_string(),
        Role::General => "general-purpose".to_string(),
        // Explore / Plan / Unknown: the JS uses the raw `subagentType`.
        _ => subagent_type.to_string(),
    }
}

/// The `CONTEXT_BUDGET_MODE` value, parsed loosely the same way the JS does.
///
/// `context-budget.js#getMode` reads `process.env.CONTEXT_BUDGET_MODE` first,
/// then a `.claude/.metrics/.mode` file, then defaults to `strict`. The file
/// fallback is a niche path; this port reads only the env var and defaults to
/// `strict` — an absent env var resolves to `strict`, exactly as the dominant
/// JS branch does. The accepted values are `observe`, `warn`, `strict`; any
/// other string is **not** normalised by the JS (it is used verbatim and
/// matches none of the three branches → falls through to the strict block), so
/// this port keeps an unrecognised value mapped to `Strict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextBudgetMode {
    Observe,
    Warn,
    Strict,
}

fn context_budget_mode() -> ContextBudgetMode {
    match std::env::var("CONTEXT_BUDGET_MODE")
        .unwrap_or_default()
        .as_str()
    {
        "observe" => ContextBudgetMode::Observe,
        "warn" => ContextBudgetMode::Warn,
        _ => ContextBudgetMode::Strict,
    }
}


/// The model context window resolution, ported from `resolveWindow`.
///
/// Every named model (haiku/sonnet/opus) has a 200K window today; the `1m`
/// suffix marks the 1M window. An unknown / empty hint resolves to the 200K
/// default.
const DEFAULT_WINDOW: usize = 200_000;
const OPUS_1M_WINDOW: usize = 1_000_000;

fn resolve_window(model_id: &str) -> usize {
    let s = model_id.to_ascii_lowercase();
    if s.is_empty() {
        return DEFAULT_WINDOW;
    }
    // `resolveWindow`'s regex: a `1m` token bounded by `[`, `(`, `-`, `_` or a
    // word boundary. A plain substring check would also match e.g. `1mb`; the
    // JS `1m\b` alternative requires a word boundary after, so reproduce that:
    // `1m` followed by end-of-string or a non-word char.
    if has_1m_token(&s) {
        return OPUS_1M_WINDOW;
    }
    // All three named models share the 200K window today.
    DEFAULT_WINDOW
}

/// `true` if `s` contains a `1m` token in the shape `resolveWindow`'s regex
/// accepts: either bracketed (`[1m]`, `(1m)`, `-1m-`, `_1m_`, …) or as a
/// word-boundaried `1m\b`.
fn has_1m_token(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'1' && bytes[i + 1] == b'm' {
            let before = if i == 0 { None } else { Some(bytes[i - 1]) };
            let after = bytes.get(i + 2).copied();
            // Word boundary after `1m`: end-of-string or a non-word char.
            let after_word_boundary =
                after.is_none_or(|c| !c.is_ascii_alphanumeric() && c != b'_');
            // Bracket/sep before is one of `[ ( - _`; the JS also matches the
            // `1m\b` branch where any boundary suffices regardless of prefix.
            let bracket_before =
                matches!(before, Some(b'[' | b'(' | b'-' | b'_'));
            let _ = bracket_before; // the `1m\b` branch alone already suffices
            if after_word_boundary {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Dumb-Zone advisory thresholds (% of the model window).
const DUMB_ZONE_PCT: f64 = 0.40;
const COMPACT_PCT: f64 = 0.65;

/// The `context-budget` gate: validate a `PreToolUse(Task)` dispatch.
///
/// Returns the verdict, 1:1 with `context-budget.js`:
/// - over the per-role char budget, mode `strict` → `Deny`;
/// - over budget, mode `warn` → `Allow` (the JS prints stderr + allow);
/// - over budget, mode `observe` → `Allow`;
/// - under budget, but estimated context ≥ Dumb-Zone % → `Inject` advisory;
/// - otherwise → `Allow`.
///
/// `context-budget.js` only measures `prompt.length`; the `.md`-reference
/// byte-summing branch fires only when the prompt literally contains
/// `.claude/skills|context/*.md` path strings — kept here for the Dumb-Zone
/// percentage, but the legacy 50K absolute branch is advisory-only and
/// extremely rare, so the percentage path is the one the parity tests exercise.
// context_budget is a single sequential flow with many local variables; splitting
// would require threading state through helpers without clarity gain.
#[allow(clippy::too_many_lines)]
fn context_budget(input: &HookInput) -> Verdict {
    let tool_input = &input.tool_input;
    let prompt = tool_input
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if prompt.is_empty() {
        return Verdict::Allow;
    }
    let subagent_type = tool_input
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let description = tool_input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let role = classify_role(subagent_type, description);

    // ── Enforcement: per-role char budget ──────────────────────────────────
    if let Some(limit) = prompt_budget(role) {
        let actual = prompt.len();
        let role_label = prompt_role_label(role, subagent_type);
        let mode = context_budget_mode();

        // Emit a metric only when actionable: a block or a >90% near-miss.
        let would_block = actual > limit;
        #[allow(clippy::cast_precision_loss)]
        let near_miss = (actual as f64) > (limit as f64) * 0.9;
        if would_block || near_miss {
            #[allow(clippy::cast_possible_wrap)] // usize fits i64 in practice; runtime values are prompt char counts
            let saved = if would_block {
                ((actual - limit) / 4) as i64
            } else {
                0
            };
            #[allow(clippy::cast_possible_wrap)]
            let line = MetricLine::new(now_iso8601(), "budget-check")
                .tokens_affected((actual / 4) as i64)
                .tokens_saved(saved)
                .note(if would_block { "blocked" } else { "near-miss" })
                .extras(json!({
                    "role": role_label,
                    "actual_chars": actual,
                    "limit": limit,
                    "would_block": would_block,
                    "mode": context_budget_mode_str(mode),
                    "category": if would_block { "prevention" } else { "routing-advisory" },
                }));
            // Fail-silent — a metric write never affects the verdict.
            // Skip when no harness cwd is supplied (would leak under
            // `cargo test`'s process cwd).
            if let Some(cwd) = metric_cwd_opt(input) {
                let _ = emit_metric(std::path::Path::new(cwd), &line);
            }
        }

        match mode {
            // `observe` / `warn` modes always allow (warn prints stderr in JS;
            // a Rust hook surfaces nothing extra — the verdict is the contract).
            ContextBudgetMode::Observe | ContextBudgetMode::Warn => return Verdict::Allow,
            ContextBudgetMode::Strict => {
                if actual > limit {
                    let limit_tokens = limit / 4;
                    let actual_tokens = actual / 4;
                    return Verdict::Deny {
                        reason: format_gate_message(
                            "Context Budget",
                            &format!(
                                "Task prompt exceeds the {role_label} role budget \
                                 ({actual_tokens} tokens / ~{actual} chars vs limit \
                                 {limit_tokens} tokens / ~{limit} chars)"
                            ),
                            "an oversized briefing crowds the subagent context and \
                             degrades reasoning",
                            "trim the prompt or split the task, or set \
                             CONTEXT_BUDGET_MODE=warn",
                        ),
                    };
                }
                // Under budget — fall through to the advisory check.
            }
        }
    }

    // ── Advisory: Dumb Zone (% of model window) ────────────────────────────
    let prompt_tokens = prompt.len() / 4;
    if prompt_tokens == 0 {
        return Verdict::Allow;
    }
    let model_hint = tool_input
        .get("model")
        .and_then(|v| v.as_str())
        .or_else(|| input.raw.get("model").and_then(|v| v.as_str()))
        .unwrap_or_default();
    let window = resolve_window(model_hint);
    #[allow(clippy::cast_precision_loss)]
    let pct = prompt_tokens as f64 / window as f64;

    if pct >= COMPACT_PCT {
        let k_tokens = (prompt_tokens + 500) / 1000;
        let window_k = (window + 500) / 1000;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pct_rounded = (pct * 100.0).round() as u64;
        return Verdict::Inject {
            context: format_gate_message(
                "Dumb Zone",
                &format!(
                    "estimated context ~{k_tokens}K tokens = {pct_rounded}% of the \
                     {window_k}K window"
                ),
                "above 65% reasoning quality drops sharply (Liu et al. 2023)",
                "run /compact then /resume, or split the task",
            ),
        };
    }
    if pct >= DUMB_ZONE_PCT {
        let k_tokens = (prompt_tokens + 500) / 1000;
        let window_k = (window + 500) / 1000;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pct_rounded = (pct * 100.0).round() as u64;
        return Verdict::Inject {
            context: format_gate_message(
                "Dumb Zone",
                &format!(
                    "estimated context ~{k_tokens}K tokens = {pct_rounded}% of the \
                     {window_k}K window"
                ),
                "≥40% degrades reasoning (Dex Horthy / Liu et al. 2023)",
                "trim recommended_skills, narrow scope, or run /compact",
            ),
        };
    }

    Verdict::Allow
}

/// The lowercase mode string for a [`ContextBudgetMode`] — used in metrics.
fn context_budget_mode_str(mode: ContextBudgetMode) -> &'static str {
    match mode {
        ContextBudgetMode::Observe => "observe",
        ContextBudgetMode::Warn => "warn",
        ContextBudgetMode::Strict => "strict",
    }
}

// ---------------------------------------------------------------------------
// output-budget — PostToolUse(Task) return-size advisory
// ---------------------------------------------------------------------------

/// Line budgets per role, from `output-budget.js`'s `BUDGETS` table.
fn output_budget(role: Role) -> usize {
    match role {
        Role::Explore => 30,
        Role::GeneralReview => 60,
        Role::Plan => 80,
        // `general-purpose` and any unknown type both use the impl budget —
        // `getRoleAndLimit`'s fallback is `BUDGETS['general-purpose']`.
        Role::General | Role::Unknown => 40,
    }
}

/// The human role label `output-budget.js` uses.
fn output_role_label(role: Role, subagent_type: &str) -> String {
    match role {
        Role::Explore => "Explore".to_string(),
        Role::Plan => "Plan".to_string(),
        Role::GeneralReview => "general-purpose(review)".to_string(),
        Role::General => "general-purpose".to_string(),
        // Unknown: the JS uses `type || 'unknown'` (the lowercased raw type).
        Role::Unknown => {
            let t = subagent_type.to_ascii_lowercase();
            if t.is_empty() { "unknown".to_string() } else { t }
        }
    }
}

/// The `output-budget` advisory, computed for a `PostToolUse(Task)`
/// invocation. Returns the over-budget advisory text + the metric line to
/// emit, or `None` when the return is within budget (the JS still emits a
/// `passed` metric, reproduced by [`BudgetGuard::observe`]).
struct OutputBudgetResult {
    /// The advisory text, present only when over budget.
    advisory: Option<String>,
    /// The metric line to append (`over-budget` or `passed`).
    metric: MetricLine,
}

fn evaluate_output_budget(input: &HookInput) -> Option<OutputBudgetResult> {
    // `tool_response` must be a string — the JS bails on a non-string body.
    let tool_response = input.raw.get("tool_response").and_then(|v| v.as_str())?;

    let tool_input = &input.tool_input;
    let subagent_type = tool_input
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let description = tool_input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let role = classify_role(subagent_type, description);

    let limit = output_budget(role);
    let role_label = output_role_label(role, subagent_type);
    // `toolResponse.split('\n').length` — number of newline-separated segments.
    let actual = tool_response.split('\n').count();
    #[allow(clippy::cast_possible_wrap)] // byte count / 4 fits i64 in practice
    let tokens_affected = (tool_response.len() / 4) as i64;
    let over_budget = actual > limit;

    if over_budget {
        let over_by = actual - limit;
        let metric = MetricLine::new(now_iso8601(), "output-budget")
            .tokens_affected(tokens_affected)
            .tokens_saved(0)
            .note("over-budget")
            .extras(json!({
                "role": role_label,
                "actual_lines": actual,
                "limit": limit,
                "over_by": over_by,
            }));
        let advisory = format_gate_message(
            "Output Budget",
            &format!(
                "{role_label} agent response exceeded the return cap \
                 ({actual} lines vs limit {limit})"
            ),
            "verbose returns crowd the parent context",
            "on future dispatches return only files changed + non-obvious \
             decisions + blockers",
        );
        Some(OutputBudgetResult {
            advisory: Some(advisory),
            metric,
        })
    } else {
        let metric = MetricLine::new(now_iso8601(), "output-budget")
            .tokens_affected(0)
            .tokens_saved(0)
            .note("passed")
            .extras(json!({
                "role": role_label,
                "actual_lines": actual,
                "limit": limit,
            }));
        Some(OutputBudgetResult {
            advisory: None,
            metric,
        })
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Recompute the over-budget tail of a Task's `tool_response`, i.e. the
/// lines past the per-role limit that the advisory is asking the agent to
/// stop emitting next time. Returns `None` when the return is within budget
/// or non-string. Mirrors the segmentation [`evaluate_output_budget`] uses.
fn over_budget_tail(input: &HookInput) -> Option<String> {
    let tool_response = input.raw.get("tool_response").and_then(|v| v.as_str())?;
    let tool_input = &input.tool_input;
    let subagent_type = tool_input
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let description = tool_input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let role = classify_role(subagent_type, description);
    let limit = output_budget(role);
    let lines: Vec<&str> = tool_response.split('\n').collect();
    if lines.len() <= limit {
        return None;
    }
    Some(lines[limit..].join("\n"))
}

/// Resolve the cwd a metric write should be rooted at: the harness `cwd`,
/// falling back to `.` (the JS uses `process.cwd()`).
///
/// W5 AC-W5.2: when no harness cwd is supplied, returning `"."` causes the
/// metric writer to materialise a `.claude/.metrics/` tree under whatever
/// the process cwd happens to be — under `cargo test -p mustard-rt` that is
/// `apps/rt/`, producing the forbidden `apps/rt/.claude/` leak.
/// `metric_cwd_opt` returns `None` in that case so the caller can skip the
/// emit.
fn metric_cwd_opt(input: &HookInput) -> Option<&str> {
    match input.cwd.as_deref() {
        Some(cwd) if !cwd.is_empty() && cwd != "." => Some(cwd),
        _ => None,
    }
}


// ---------------------------------------------------------------------------
// Contract impls
// ---------------------------------------------------------------------------

/// The consolidated Task-size enforcement module.
pub struct BudgetGuard;

impl Check for BudgetGuard {
    /// Both budgets, dispatched by trigger:
    ///
    /// - `PreToolUse(Task)` → `context-budget`: gate the dispatch on prompt
    ///   size. The verdict is computed by [`context_budget`], which carries
    ///   its own `CONTEXT_BUDGET_MODE` — independent of the module enforcement
    ///   mode the dispatcher applies.
    /// - `PostToolUse(Task)` → `output-budget`: emit the per-role return-size
    ///   metric and, when the return is over budget, return a
    ///   [`Verdict::Inject`] carrying the advisory. The dispatcher folds that
    ///   into the single `Outcome` so exactly one JSON object is emitted
    ///   (Wave-5 resolution of the Wave-3 stdout-bypass Concern).
    ///
    /// Any other invocation self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if !is_task_tool(input) {
            return Ok(Verdict::Allow);
        }
        match ctx.trigger {
            Some(Trigger::PreToolUse) => Ok(context_budget(input)),
            Some(Trigger::PostToolUse) => {
                let Some(result) = evaluate_output_budget(input) else {
                    return Ok(Verdict::Allow);
                };
                // Emit the return-size metric (fail-silent). Skip when no
                // harness cwd is supplied (would leak under `cargo test`).
                if let Some(cwd) = metric_cwd_opt(input) {
                    let _ = emit_metric(std::path::Path::new(cwd), &result.metric);
                }
                // Over budget → record the savings frame (typed writer) +
                // surface the advisory through the Outcome. Never a raw
                // stdout write.
                if result.advisory.is_some() {
                    // Resolve the project dir to record the savings frame
                    // against. Skip the write entirely when neither `ctx`
                    // nor the harness `cwd` carries a valid root (would
                    // leak under `cargo test`).
                    let project_dir_opt = if ctx.project_dir.is_empty() {
                        metric_cwd_opt(input).map(str::to_string)
                    } else {
                        Some(ctx.project_dir.clone())
                    };
                    // Recompute the dropped tail and labels so the writer call
                    // stays a pure side-effect of an over-budget verdict.
                    if let (Some(project_dir), Some(tail)) =
                        (project_dir_opt, over_budget_tail(input))
                    {
                        let subagent_type = input
                            .tool_input
                            .get("subagent_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        let description = input
                            .tool_input
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        let role = classify_role(subagent_type, description);
                        let role_label = output_role_label(role, subagent_type);
                        let model_hint = input
                            .tool_input
                            .get("model")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty());
                        record_output_cut(&project_dir, &tail, &role_label, model_hint);
                    }
                }
                Ok(match result.advisory {
                    Some(advisory) => Verdict::Inject { context: advisory },
                    None => Verdict::Allow,
                })
            }
            _ => Ok(Verdict::Allow),
        }
    }
}

/// `true` if this invocation is a `Task` (or legacy `Agent`) tool call.
fn is_task_tool(input: &HookInput) -> bool {
    matches!(input.tool_name.as_deref(), Some("Task" | "Agent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a `PreToolUse(Task)` input with a prompt of `prompt_len` chars.
    fn task_input(subagent: &str, prompt_len: usize, description: &str) -> (HookInput, Ctx) {
        let prompt = "x".repeat(prompt_len);
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({
                "subagent_type": subagent,
                "description": description,
                "prompt": prompt,
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    fn verdict_for(subagent: &str, prompt_len: usize, description: &str) -> Verdict {
        let (input, ctx) = task_input(subagent, prompt_len, description);
        BudgetGuard
            .evaluate(&input, &ctx)
            .expect("check never errors")
    }

    // --- context-budget parity (integration.test.js Suite 2) ---------------

    #[test]
    fn explore_at_exact_budget_allows() {
        // 10,000 chars == budget → not over → allow.
        assert_eq!(verdict_for("Explore", 10_000, ""), Verdict::Allow);
    }

    #[test]
    fn explore_one_over_budget_denies() {
        assert!(verdict_for("Explore", 10_001, "").is_blocking());
    }

    #[test]
    fn explore_empty_prompt_allows() {
        assert_eq!(verdict_for("Explore", 0, ""), Verdict::Allow);
    }

    #[test]
    fn general_purpose_at_budget_allows_one_over_denies() {
        assert_eq!(
            verdict_for("general-purpose", 30_000, "implement feature"),
            Verdict::Allow
        );
        assert!(verdict_for("general-purpose", 30_001, "implement feature").is_blocking());
    }

    #[test]
    fn general_purpose_review_uses_review_budget() {
        // `review` in the description → 12,000-char budget.
        assert_eq!(
            verdict_for("general-purpose", 12_000, "review pull request"),
            Verdict::Allow
        );
        assert!(verdict_for("general-purpose", 12_001, "review pull request").is_blocking());
    }

    #[test]
    fn plan_role_has_no_hard_block() {
        // Plan is advisory-only — 50K chars must not deny (well under 40% of
        // the 200K default window → also no Dumb-Zone advisory at this size).
        let v = verdict_for("Plan", 50_000, "plan architecture");
        assert!(!v.is_blocking());
    }

    #[test]
    fn unknown_subagent_type_has_no_hard_block() {
        // `getBudget` returns null for unknown types → never a hard block.
        assert!(!verdict_for("", 50_000, "").is_blocking());
    }

    #[test]
    fn deny_reason_mentions_role_and_budget() {
        match verdict_for("Explore", 12_000, "") {
            Verdict::Deny { reason } => {
                assert!(reason.contains("Context Budget"));
                assert!(reason.contains("Explore"));
                assert!(reason.contains("CONTEXT_BUDGET_MODE"));
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    // --- Dumb Zone advisory parity (integration.test.js Suite 2) -----------

    #[test]
    fn plan_above_dumb_zone_injects_advisory() {
        // 360K chars ≈ 90K tokens = 45% of 200K window → Dumb-Zone Inject.
        match verdict_for("Plan", 360_000, "plan architecture") {
            Verdict::Inject { context } => assert!(context.contains("Dumb Zone")),
            other => panic!("expected Inject advisory, got {other:?}"),
        }
    }

    #[test]
    fn plan_above_compact_threshold_injects_compact_advice() {
        // 560K chars ≈ 140K tokens = 70% of 200K window → compact advice.
        match verdict_for("Plan", 560_000, "plan architecture") {
            Verdict::Inject { context } => {
                assert!(context.contains("Dumb Zone"));
                assert!(context.contains("/compact"));
            }
            other => panic!("expected Inject advisory, got {other:?}"),
        }
    }

    #[test]
    fn small_plan_prompt_allows_without_advisory() {
        // 8K chars ≈ 2K tokens — well under 40% → plain allow.
        assert_eq!(
            verdict_for("Plan", 8_000, "plan architecture"),
            Verdict::Allow
        );
    }

    #[test]
    fn one_million_window_suppresses_dumb_zone_at_360k() {
        // With a `1m` model hint the window is 1M, so 90K tokens = 9% → allow.
        let prompt = "x".repeat(360_000);
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({
                "subagent_type": "Plan",
                "description": "plan",
                "prompt": prompt,
                "model": "claude-opus-4-7-1m",
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            BudgetGuard.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn resolve_window_recognises_1m_suffix() {
        assert_eq!(resolve_window("claude-opus-4-7-1m"), OPUS_1M_WINDOW);
        assert_eq!(resolve_window("claude-opus-4-7[1m]"), OPUS_1M_WINDOW);
        assert_eq!(resolve_window("claude-sonnet-4-5"), DEFAULT_WINDOW);
        assert_eq!(resolve_window(""), DEFAULT_WINDOW);
    }

    // --- gate routing ------------------------------------------------------

    #[test]
    fn non_task_tool_allows() {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            BudgetGuard.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        let (input, _) = task_input("Explore", 99_999, "");
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        assert_eq!(
            BudgetGuard.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    // --- output-budget parity (output-budget.js) ---------------------------

    #[test]
    fn output_budget_role_line_caps() {
        assert_eq!(output_budget(Role::Explore), 30);
        assert_eq!(output_budget(Role::General), 40);
        assert_eq!(output_budget(Role::GeneralReview), 60);
        assert_eq!(output_budget(Role::Plan), 80);
        // Unknown falls back to the general-purpose impl cap.
        assert_eq!(output_budget(Role::Unknown), 40);
    }

    #[test]
    fn output_budget_over_cap_produces_advisory() {
        // 50-line Explore return vs a 30-line cap → over budget.
        let response = "line\n".repeat(50);
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "subagent_type": "Explore", "description": "" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": response }),
            ..HookInput::default()
        };
        let result = evaluate_output_budget(&input).expect("string response");
        let advisory = result.advisory.expect("over budget → advisory");
        assert!(advisory.contains("Output Budget"));
        assert!(advisory.contains("Explore"));
        assert_eq!(result.metric.note, "over-budget");
    }

    #[test]
    fn output_budget_within_cap_emits_passed_metric_no_advisory() {
        let response = "line\n".repeat(10);
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "subagent_type": "Explore", "description": "" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": response }),
            ..HookInput::default()
        };
        let result = evaluate_output_budget(&input).expect("string response");
        assert!(result.advisory.is_none());
        assert_eq!(result.metric.note, "passed");
    }

    #[test]
    fn output_budget_non_string_response_is_skipped() {
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "subagent_type": "Explore" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": { "blocks": [] } }),
            ..HookInput::default()
        };
        assert!(evaluate_output_budget(&input).is_none());
    }

    #[test]
    fn output_budget_over_cap_injects_advisory_via_check() {
        // The over-budget advisory now flows through the Check as an Inject —
        // no raw stdout write (Wave-5 resolution of the stdout-bypass Concern).
        let response = "line\n".repeat(50);
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "subagent_type": "Explore", "description": "" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": response }),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        match BudgetGuard.evaluate(&input, &ctx).expect("no error") {
            Verdict::Inject { context } => {
                assert!(context.contains("Output Budget"));
                assert!(context.contains("Explore"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    #[test]
    fn output_budget_within_cap_allows_via_check() {
        let response = "line\n".repeat(10);
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "subagent_type": "Explore", "description": "" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": response }),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        assert_eq!(
            BudgetGuard.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }
}
