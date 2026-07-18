//! The hook contract — the seam shared by every Mustard consumer.
//!
//! This module defines what a hook *receives* ([`HookInput`]), what it can
//! *decide* ([`Verdict`]), how those decisions *consolidate* ([`Outcome`]),
//! which harness lifecycle event *triggered* it ([`Trigger`]), and the two
//! behaviours a component can have: [`Check`] (may affect the result) and
//! [`Observer`] (telemetry only, never blocks).
//!
//! **Frozen at the end of b2 Wave 1.** B3 (hooks → Rust) and B4 (scripts →
//! Rust) build a dispatcher on top of these types; a late change here
//! propagates everywhere. Add to it via `#[non_exhaustive]`, do not reshape it.

use crate::platform::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

// ---------------------------------------------------------------------------
// Trigger
// ---------------------------------------------------------------------------

/// The harness lifecycle event that caused a hook to run.
///
/// Mirrors Claude Code's hook event names (the `hook_event_name` field of the
/// stdin JSON). `#[non_exhaustive]` so the harness can add lifecycle events
/// without breaking the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Trigger {
    /// Before a tool runs — the only point a hook can deny or rewrite.
    PreToolUse,
    /// After a tool runs — observe results, cannot deny the call.
    PostToolUse,
    /// A new Claude Code session started.
    SessionStart,
    /// A Claude Code session ended.
    SessionEnd,
    /// Before context compaction.
    PreCompact,
    /// A delegated subagent started.
    SubagentStart,
    /// A delegated subagent finished.
    SubagentStop,
    /// The user submitted a prompt.
    UserPromptSubmit,
    /// The user interrupted the session (Ctrl+C or `/stop`).
    Stop,
    /// The harness emitted a Notification (idle prompt, completion ping).
    Notification,
}

impl Trigger {
    /// Parse the harness `hook_event_name` string into a [`Trigger`].
    ///
    /// Returns `None` for an unrecognised value — callers fail open rather
    /// than panic, matching the JS hooks' behaviour.
    #[must_use]
    pub fn from_event_name(name: &str) -> Option<Self> {
        match name {
            "PreToolUse" => Some(Self::PreToolUse),
            "PostToolUse" => Some(Self::PostToolUse),
            "SessionStart" => Some(Self::SessionStart),
            "SessionEnd" => Some(Self::SessionEnd),
            "PreCompact" => Some(Self::PreCompact),
            "SubagentStart" => Some(Self::SubagentStart),
            "SubagentStop" => Some(Self::SubagentStop),
            "UserPromptSubmit" => Some(Self::UserPromptSubmit),
            "Stop" => Some(Self::Stop),
            "Notification" => Some(Self::Notification),
            _ => None,
        }
    }

    /// The canonical harness string for this trigger (inverse of
    /// [`Trigger::from_event_name`]).
    #[must_use]
    pub fn as_event_name(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::PreCompact => "PreCompact",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::Stop => "Stop",
            Self::Notification => "Notification",
        }
    }
}

impl fmt::Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_event_name())
    }
}

// ---------------------------------------------------------------------------
// HookInput
// ---------------------------------------------------------------------------

/// The stdin JSON the harness passes to a hook.
///
/// **Lenient by design.** The harness controls this JSON and adds fields
/// over time; a strict struct would reject any new field. The known fields
/// are typed for ergonomic access, and [`HookInput::raw`] (via
/// `#[serde(flatten)]`) captures *every* field — including ones not listed
/// here — so a hook can always reach a new harness field without a crate
/// release. Internal crate types may stay strict; this boundary type does not.
///
/// Field names derived from `_lib/hook-env.js` and the JS hooks
/// (`bash-safety.js`, `model-routing-gate.js`): `tool_name`, `tool_input`,
/// `hook_event_name`, `cwd`, `session_id`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookInput {
    /// Name of the tool being used, e.g. `"Bash"`, `"Task"`, `"Write"`.
    /// Absent for non-tool lifecycle events (`SessionStart`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    /// Tool-specific arguments. Shape depends on `tool_name`, so it stays an
    /// untyped [`Value`] — a `Bash` call carries `command`, a `Task` call
    /// carries `model` / `subagent_type` / `description`, etc.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub tool_input: Value,

    /// The harness lifecycle event name, e.g. `"PreToolUse"`. Parse it into a
    /// [`Trigger`] with [`Trigger::from_event_name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_event_name: Option<String>,

    /// Working directory the harness reports for this invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Worktree path to REMOVE — `WorktreeRemove` only. The isolation events do
    /// NOT share a payload: `WorktreeRemove` carries this path, while
    /// `WorktreeCreate` carries only [`Self::worktree_name`] and expects the
    /// hook to decide the path and echo it. Reading this field on a create is
    /// always `None` — the mistake that left worktree isolation dead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,

    /// Worktree NAME to create (harness field `name`) — `WorktreeCreate` only.
    /// A name, never a path: a configured hook REPLACES Claude Code's native
    /// `git worktree add`, so it chooses where the worktree lands (the harness
    /// convention is `.claude/worktrees/{name}`), creates it, and echoes the
    /// absolute path on stdout. A non-zero exit aborts the creation.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub worktree_name: Option<String>,

    /// Session identifier (`session_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Unique identifier of the subagent this hook fired inside, when any.
    ///
    /// **Harness-provided, subagent-only.** Per Claude Code's hook contract
    /// (the "Common input fields" table) `agent_id` is "present only when the
    /// hook fires inside a subagent call — use this to distinguish subagent
    /// hook calls from main-thread calls." It is therefore the authoritative
    /// per-invocation actor signal: `Some(_)` ⇒ a `Task` subagent is the actor,
    /// `None` ⇒ the main orchestrator. Contrast [`Self::agent_type`], which the
    /// harness *also* sets when the MAIN session runs with `--agent`, so it is
    /// not a reliable main-vs-subagent discriminator on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// Agent type/name of the subagent or `--agent` session, when any
    /// (e.g. `"Explore"`, `"security-reviewer"`). Set both inside a subagent
    /// AND when the main session runs with `--agent`; prefer [`Self::agent_id`]
    /// for the main-vs-subagent decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,

    /// Every field of the original JSON, including ones not modelled above.
    /// Use this to read new harness fields without changing the crate.
    #[serde(flatten)]
    pub raw: Value,
}

impl HookInput {
    /// The [`Trigger`] for this input, parsed from `hook_event_name`.
    ///
    /// `None` when the field is missing or unrecognised — callers fail open.
    #[must_use]
    pub fn trigger(&self) -> Option<Trigger> {
        self.hook_event_name
            .as_deref()
            .and_then(Trigger::from_event_name)
    }

    /// `true` when this hook fired inside a `Task` subagent, `false` when it
    /// fired in the main orchestrator context.
    ///
    /// Derived from the harness-provided [`Self::agent_id`], which the Claude
    /// Code contract documents as "present only when the hook fires inside a
    /// subagent call." A blank `agent_id` is treated as absent (main). This is
    /// the per-invocation, authoritative actor signal — unlike a shared depth
    /// gauge maintained out-of-band by lifecycle hooks. Falls back to reading
    /// `raw["agent_id"]` so it keeps working even if a caller deserialised the
    /// payload before this field was modelled.
    #[must_use]
    pub fn is_subagent(&self) -> bool {
        let typed = self.agent_id.as_deref().filter(|s| !s.is_empty()).is_some();
        typed
            || self
                .raw
                .get("agent_id")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
    }

    /// The `file_path` a Write/Edit (or Read) invocation targets, accepting the
    /// legacy `path` key (`tool_input.file_path || tool_input.path`).
    ///
    /// Returns the raw string exactly as the harness sent it — `None` when
    /// neither key holds a string. Callers that need forward-slash normalisation
    /// apply it at their own boundary (their `relative_to_cwd` re-normalises
    /// regardless). Before this method the same body was copy-pasted
    /// byte-identically into seven hook modules across `mustard-rt`.
    #[must_use]
    pub fn file_path(&self) -> Option<String> {
        let ti = &self.tool_input;
        ti.get("file_path")
            .or_else(|| ti.get("path"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// A single decision a [`Check`] can reach about a hook invocation.
///
/// Illegal states are unrepresentable: each variant carries exactly the data
/// that decision needs and nothing it does not. `Rewrite` *must* carry the
/// replacement; `Inject` *must* carry the text; `Deny` / `Warn` *must* carry
/// a human-readable reason. `Allow` carries nothing.
///
/// `#[non_exhaustive]`: a future check kind can add a variant without breaking
/// downstream `match` arms (they keep a wildcard arm).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Verdict {
    /// Permit the action with no change. The neutral verdict.
    #[default]
    Allow,

    /// Block the action. The string is the user-facing reason
    /// (`permissionDecisionReason` in the JS `PreToolUse` protocol).
    Deny {
        /// Why the action was blocked.
        reason: String,
    },

    /// Permit the action but surface an advisory message. Non-blocking.
    Warn {
        /// The advisory message shown to the user / agent.
        message: String,
    },

    /// Permit the action but with rewritten tool input. Carries the full
    /// replacement so the dispatcher never has to reconstruct it.
    Rewrite {
        /// The tool input that replaces the original.
        tool_input: Value,
    },

    /// Permit the action and inject extra context for the agent
    /// (`additionalContext` in the JS hook protocol).
    Inject {
        /// The text injected into the agent's context.
        context: String,
    },
}

impl Verdict {
    /// `true` if this verdict blocks the action (only [`Verdict::Deny`]).
    #[must_use]
    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }
}



// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// The consolidated result of running one or more [`Check`]s against a hook
/// invocation.
///
/// The B3 dispatcher folds every [`Verdict`] produced for an invocation into
/// one `Outcome`, then turns it into stdout JSON + a process exit code. A
/// blocking [`Verdict::Deny`] dominates; otherwise warnings, rewrites, and
/// injections accumulate.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Outcome {
    /// The decisive verdict for the invocation. [`Verdict::Allow`] unless a
    /// check denied, rewrote, or injected.
    pub verdict: Verdict,
    /// Advisory messages collected from non-blocking [`Verdict::Warn`] checks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl Outcome {
    /// An outcome that allows the action with no warnings.
    #[must_use]
    pub fn allow() -> Self {
        Self::default()
    }

    /// Fold a [`Verdict`] into this outcome.
    ///
    /// A [`Verdict::Deny`] is sticky — once denied, the outcome stays denied.
    /// [`Verdict::Warn`] appends to [`Outcome::warnings`]. [`Verdict::Allow`]
    /// is a no-opinion verdict and never overrides a prior decisive verdict
    /// (Rewrite / Inject); it only stays as the resting state when no module
    /// produced a decisive verdict. Other verdicts (Rewrite, Inject) replace
    /// [`Outcome::verdict`] when the outcome is not already blocking — within
    /// the same priority tier, last writer wins.
    pub fn fold(&mut self, verdict: Verdict) {
        if self.verdict.is_blocking() {
            return;
        }
        match verdict {
            Verdict::Warn { message } => self.warnings.push(message),
            Verdict::Allow => {} // No opinion — preserve any prior decisive verdict.
            other => self.verdict = other,
        }
    }

    /// `true` if the consolidated outcome blocks the action.
    #[must_use]
    pub fn is_blocking(&self) -> bool {
        self.verdict.is_blocking()
    }
}

// ---------------------------------------------------------------------------
// Ctx
// ---------------------------------------------------------------------------

/// Ambient context handed to a [`Check`] alongside the [`HookInput`].
///
/// **Minimal placeholder for Wave 1.** It carries only what a check needs to
/// resolve "where am I": the project directory and the [`Trigger`]. b2 Wave 3
/// grows this with enforcement config, the event sink, and pipeline-state
/// access; B3 may extend it further. New fields are additive.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    /// Absolute path to the project root for this invocation.
    pub project_dir: String,
    /// The lifecycle event that triggered the hook, if known.
    pub trigger: Option<Trigger>,
    /// The resolved Mustard workspace root (the directory containing
    /// `mustard.json` + `.claude/`). Populated once by the dispatcher via
    /// [`crate::io::workspace::workspace_root`] — modules should prefer this over
    /// re-resolving from [`Self::project_dir`] (which is the raw `cwd` and may
    /// point at a monorepo subproject). `None` when resolution failed; the
    /// dispatcher fails open in that case.
    pub workspace_root: Option<std::path::PathBuf>,
}

impl Ctx {
    /// The project directory for this invocation: the dispatcher-resolved
    /// [`Ctx::project_dir`] when populated, else the harness-provided
    /// [`HookInput::cwd`], else `"."`.
    ///
    /// This is the hook-face *reader* of an already-resolved directory — the
    /// dispatcher resolves the workspace root once (via
    /// [`crate::io::workspace::workspace_root`]) and stashes it in
    /// `self.project_dir`. It is deliberately distinct from the `run`-face
    /// `context::project_dir()`, which re-resolves from the environment on
    /// every call. Before this method the same body was copy-pasted
    /// byte-identically into 15 hook modules.
    #[must_use]
    pub fn project_dir_or_cwd(&self, input: &HookInput) -> String {
        if !self.project_dir.is_empty() {
            return self.project_dir.clone();
        }
        match input.cwd.as_deref() {
            Some(c) if !c.is_empty() => c.to_string(),
            _ => ".".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// A component that *can affect the result* of a hook invocation — a gate, a
/// rewriter, or an injector.
///
/// Interface Segregation: [`Check`] is the only trait allowed to return a
/// [`Verdict`]. Telemetry-only components implement [`Observer`] instead, so
/// they can never accidentally block.
///
/// Implementations must fail open semantically: prefer returning
/// `Ok(Verdict::Allow)` over an `Err` unless the input was genuinely
/// unusable. A returned [`Error`] signals the dispatcher that the check could
/// not run; the dispatcher decides how to degrade (never by panicking).
pub trait Check {
    /// Evaluate this check against a hook invocation.
    ///
    /// `input` is the lenient stdin JSON; `ctx` is the ambient context.
    ///
    /// # Errors
    ///
    /// Returns an [`Error`] only when the check could not reach a decision —
    /// e.g. the input was malformed ([`Error::InvalidInput`]) or the check's
    /// own logic failed ([`Error::CheckFailed`]). Implementations fail open:
    /// prefer `Ok(Verdict::Allow)` over an `Err` whenever the input is usable.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error>;
}

/// A pure-telemetry component: it *observes* a hook invocation and never
/// affects its result.
///
/// Interface Segregation in action — [`Observer`] deliberately returns `()`,
/// not a [`Verdict`], so a metrics or logging component is structurally
/// incapable of blocking an action. Observers must also be infallible from
/// the dispatcher's view: swallow errors internally (fail-open).
pub trait Observer {
    /// React to a hook invocation for telemetry purposes only.
    fn observe(&self, input: &HookInput, ctx: &Ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_input_is_lenient_about_unknown_fields() {
        // `future_field` is not modelled — it must land in `raw`, not error.
        let raw = r#"{"tool_name":"Bash","hook_event_name":"PreToolUse","tool_input":{"command":"ls"},"future_field":42}"#;
        let input: HookInput = serde_json::from_str(raw).expect("lenient parse");
        assert_eq!(input.tool_name.as_deref(), Some("Bash"));
        assert_eq!(input.trigger(), Some(Trigger::PreToolUse));
        assert_eq!(input.raw["future_field"], serde_json::json!(42));
    }

    #[test]
    fn unknown_trigger_fails_open_to_none() {
        assert_eq!(Trigger::from_event_name("Bogus"), None);
    }

    #[test]
    fn is_subagent_reads_harness_agent_id() {
        // A PostToolUse inside a subagent carries `agent_id` → is_subagent.
        let sub = r#"{"hook_event_name":"PostToolUse","tool_name":"Edit","tool_input":{"file_path":"a.rs"},"agent_id":"explore-42","agent_type":"Explore"}"#;
        let input: HookInput = serde_json::from_str(sub).expect("lenient parse");
        assert_eq!(input.agent_id.as_deref(), Some("explore-42"));
        assert_eq!(input.agent_type.as_deref(), Some("Explore"));
        assert!(input.is_subagent(), "agent_id present ⇒ subagent");

        // Same event with no agent_id is the main orchestrator.
        let main = r#"{"hook_event_name":"PostToolUse","tool_name":"Edit","tool_input":{"file_path":"a.rs"}}"#;
        let input: HookInput = serde_json::from_str(main).expect("lenient parse");
        assert_eq!(input.agent_id, None);
        assert!(!input.is_subagent(), "absent agent_id ⇒ main");

        // An empty-string agent_id is treated as absent (main), not a subagent.
        let blank = r#"{"hook_event_name":"PostToolUse","tool_name":"Edit","agent_id":""}"#;
        let input: HookInput = serde_json::from_str(blank).expect("lenient parse");
        assert!(!input.is_subagent(), "blank agent_id ⇒ main");
    }

    #[test]
    fn deny_dominates_outcome_fold() {
        let mut outcome = Outcome::allow();
        outcome.fold(Verdict::Warn { message: "be careful".into() });
        outcome.fold(Verdict::Deny { reason: "blocked".into() });
        // A later non-blocking verdict cannot un-block the outcome.
        outcome.fold(Verdict::Inject { context: "ignored".into() });
        assert!(outcome.is_blocking());
        assert_eq!(outcome.warnings, vec!["be careful".to_string()]);
    }

    #[test]
    fn allow_does_not_clobber_prior_decisive_verdict() {
        // Regression guard for spec 2026-05-20-restore-rtk-rewrite: when one
        // module returns Rewrite and a later module (tool_use_counter /
        // main_context_counter) returns Allow, the Rewrite must survive —
        // otherwise rtk-rewrite is silently swallowed by the dispatcher.
        let mut outcome = Outcome::allow();
        let rewrite = Verdict::Rewrite {
            tool_input: serde_json::json!({ "command": "rtk git status" }),
        };
        outcome.fold(rewrite.clone());
        outcome.fold(Verdict::Allow);
        assert_eq!(outcome.verdict, rewrite);
        // Same protection for Inject — no-opinion Allow must not erase it.
        let mut outcome = Outcome::allow();
        outcome.fold(Verdict::Inject { context: "hint".into() });
        outcome.fold(Verdict::Allow);
        assert_eq!(
            outcome.verdict,
            Verdict::Inject { context: "hint".into() }
        );
    }

    #[test]
    fn verdict_serializes_with_decision_tag() {
        let json = serde_json::to_value(Verdict::Deny {
            reason: "no".into(),
        })
        .expect("serialize verdict");
        assert_eq!(json["decision"], serde_json::json!("deny"));
        assert_eq!(json["reason"], serde_json::json!("no"));
    }
}
