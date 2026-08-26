//! `prompt_submit_inject` — the UserPromptSubmit gate module.
//!
//! ## Scope (b3 Wave 5, prompt family + orchestrator-redesign injectables)
//!
//! Four concerns ride `UserPromptSubmit`, in this order:
//!
//! - **installation gate** (orchestrator-redesign): a `/mustard:*` command in
//!   a project with NO `mustard.json` at the root is denied with a didactic
//!   pointer to `/mustard:upsert` — the one command exempted (it is the
//!   bootstrap door; the bare `/mustard` help never matches the `/mustard:`
//!   prefix and passes too). The gate runs BEFORE the injectables: without an
//!   installation there is nothing to inject. A free-text prompt is never
//!   gated — the hooks stay silent on uninstalled projects.
//! - `followup-cancel-gate` (the b3 port): when the prompt invokes
//!   `/mustard:feature`, `/mustard:bugfix`, or `/mustard:task`, close any open
//!   per-session amendment window — the previous follow-up window is over, so
//!   subsequent edits belong to a new context.
//! - **declared injectables** (orchestrator-redesign): the
//!   `mustard.json#inject` entries with `on: userPromptSubmit` (canonically
//!   the orchestrator rules in `.claude/mustard/orchestrator.md`) are spliced
//!   into the window via [`crate::hooks::session::injectables::collect`] —
//!   once per session when `once: true`. A `/mustard:*` prompt gets NO
//!   injectables (the slash command is already inside the flow).
//! - **writing rule** (`mustard.json#tone`): a project that DECLARED
//!   `tone: didactic` carries a one-paragraph rule for how the answer is
//!   written. It is the one concern a `/mustard:*` prompt still receives, and
//!   deliberately so: it governs how the ANSWER is written, and the answer to a
//!   slash command is read by the same person as any other. Delivered on EVERY
//!   prompt rather than once per session — the thing it governs is always the
//!   newest message, so a rule delivered once only drifts further from it.
//!
//! The three injecting concerns compose into a SINGLE [`Verdict::Inject`] (the
//! dispatcher fold is last-writer-wins, so separate Injects would drop one):
//! injectables first, banner next, writing rule last.
//!
//! ## Contract shape
//!
//! `followup-cancel-gate.js` never blocked — it always `process.exit(0)`. The
//! b3 spec classes `prompt_gate` as a [`Check`], which is exactly why the
//! installation gate could land here: `UserPromptSubmit` is the seam where a
//! prompt gate denies, and `main.rs` maps a [`Verdict::Deny`] on this event to
//! the harness's `{"decision": "block", "reason": …}` shape. Every other path
//! still allows.
//!
//! ## Single-stage close
//!
//! The old `closed-followup` archival sweep was removed with the single-stage
//! close (a spec now goes straight to `completed`, with no follow-up grace
//! window to archive). What remains on a new-pipeline prompt is closing the
//! session's amendment window.
//!
//! ## W3C migration
//!
//! `emit_economy_operation` routes economy events via
//! `crate::shared::events::route::emit` (NDJSON path) instead of the old SQLite
//! event sink.

use mustard_core::domain::model::event::ActorKind;
use crate::shared::events::economy;
use crate::hooks::observe::amend_window_inject::close_amend_windows_for_session;
use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::ProjectConfig;
use std::path::Path;

/// W8.T8.2 — pipeline-in-flight reminder: surfaced when the user's prompt is
/// NOT a `/mustard:*` invocation AND a spec is active. Keeps the agent aware
/// that a pipeline is owning the conversation without bloating every prompt.
const PIPELINE_IN_FLIGHT_BANNER: &str = "Pipeline em curso";

/// The UserPromptSubmit gate module.
pub struct PromptSubmitInject;

/// `true` if `prompt` invokes a pipeline command. Mirrors the JS regex
/// `^\s*\/mustard:(feature|bugfix|task)\b` (case-insensitive).
fn is_pipeline_prompt(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    let Some(rest) = t.strip_prefix("/mustard:") else {
        return false;
    };
    for cmd in ["feature", "bugfix", "task"] {
        if rest.starts_with(cmd) {
            // `\b` after the command word.
            let boundary_ok = rest
                .as_bytes()
                .get(cmd.len())
                .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'));
            if boundary_ok {
                return true;
            }
        }
    }
    false
}

/// Does THIS invocation carry the blocks that belong to the whole event?
///
/// The pipeline banner and the writing rule are about the invocation, not about
/// any one injectable, so exactly one sibling hook must carry them: emitting
/// from each hands the window one copy per hook, and emitting from none drops
/// them entirely.
///
/// Delegates to the dispatcher's election so the two cannot drift — they answer
/// the same question about the same invocation, and a second implementation is
/// a second answer.
fn carries_shared_blocks(project_dir: &str, inject_only: Option<&str>) -> bool {
    crate::dispatch::carries_shared_modules(project_dir, "userpromptsubmit", inject_only)
}

/// `true` if `prompt` starts with any `/mustard:` namespaced command. The bare
/// `/mustard` help (no colon) deliberately does NOT match: it is the
/// orientation door and must keep working on an uninstalled project.
///
/// Narrower than [`is_slash_command`] on purpose — this one guards the
/// INSTALLATION gate, which may only speak for Mustard's own doors. Denying a
/// third party's command for a missing `mustard.json` would break a skill that
/// has nothing to do with this harness.
fn is_mustard_command(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    t.starts_with("/mustard:")
}

/// `true` if `prompt` invokes ANY slash command, Mustard's or a third party's.
///
/// A slash command knows its own context, so the router has nothing to add and
/// a great deal to break: an interview skill asks a question, the operator
/// answers it, and a router that reclassifies that answer opens a work unit in
/// the middle of someone else's flow. **The flow that expanded owns the turn.**
///
/// This used to match `/mustard:` alone, so only Mustard's own doors were
/// spared and every third-party skill was routed over.
///
/// The bare `/mustard` help (no colon) deliberately does NOT match: it is the
/// orientation door and must keep working on an uninstalled project. Nor does a
/// lone `/`, or a path-looking prompt (`/etc/hosts`, `/usr/bin`) — a command
/// name is a word, so the first segment must start with a letter and hold only
/// name characters.
fn is_slash_command(prompt: &str) -> bool {
    let t = prompt.trim_start();
    let Some(rest) = t.strip_prefix('/') else {
        return false;
    };
    if t.eq_ignore_ascii_case("/mustard") || t.to_ascii_lowercase().starts_with("/mustard ") {
        return false;
    }
    let name: &str = rest.split_whitespace().next().unwrap_or_default();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic())
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_'))
    {
        return false;
    }
    // A bare `/tmp` or `/usr` satisfies every rule above and is a PATH, not a
    // command — the operator typing one is asking about a directory, and
    // treating it as a slash command costs them the router on that prompt. A
    // real command name is longer or namespaced, so require one of the two.
    // (`/pr` and `/qa` would be false negatives; neither exists, and the cost
    // of being wrong in this direction is one extra injection.)
    name.contains(':') || name.len() > 4
}

/// `true` if `prompt` invokes `/mustard:upsert` — the bootstrap door the
/// installation gate exempts. Same word-boundary rule as
/// [`is_pipeline_prompt`], so `/mustard:upsertish` does not sneak through.
fn is_upsert_prompt(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    let Some(rest) = t.strip_prefix("/mustard:") else {
        return false;
    };
    const CMD: &str = "upsert";
    rest.starts_with(CMD)
        && rest
            .as_bytes()
            .get(CMD.len())
            .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'))
}

// ===========================================================================
// writing rule — `mustard.json#tone`, carried with every prompt
// ===========================================================================

/// The writing rule this project declares, carried with EVERY prompt.
///
/// `mustard.json#tone` already existed and already meant this — it was read in
/// exactly one place, `agent-prompt-render`, which shapes the prompts of the
/// agents that WRITE FILES. Nothing carried it into the conversation, so a
/// project that had asked for plain language got it only when the model
/// remembered to. The operator found this the honest way: by not understanding
/// an explanation, twice, in a project whose config said `didactic` all along.
///
/// **Every prompt, not once per session.** Delivered once, the rule drifts
/// further away with each exchange while the thing it governs — the next
/// answer — is always the newest. Measured before this was accepted: 126
/// tokens, about 0.04% of a long session, against the thousand-plus a single
/// misunderstanding costs in a wrong answer, a correction and a rewrite.
///
/// `None` for any other tone, and for a project with no `mustard.json`.
fn tone_rule(root: &Path) -> Option<String> {
    // The RAW field, never the resolved one. `ProjectConfig::load` fails open to
    // a default when the file is absent, and that default IS `didactic` — a
    // resolved read would put this paragraph in front of every project that
    // merely has a `mustard.json`, including the ones that never asked. A
    // default is the absence of a choice; this rule only answers a written one.
    //
    // Parsed by the CANONICAL parser, never by a hand-rolled match. `Tone::parse`
    // accepts `didactic`, `didatico` AND `didático` — and the accented spelling
    // is the one a Brazilian operator writes. A local `eq_ignore_ascii_case`
    // pair silently rejected it, so a project declaring the word in its own
    // language was treated as never having declared: the very defect this
    // function exists to remove, reintroduced one line below the fix.
    ProjectConfig::load(root)
        .tone
        .as_deref()
        .and_then(mustard_core::Tone::parse)
        .filter(|tone| *tone == mustard_core::Tone::Didactic)
        .map(|_| {
            "[Mustard] This project declares `tone: didactic`. Write every user-facing answer so \
             it can be read once, by someone who did not write this code: ONE idea per sentence; \
             every technical term translated the first time it appears IN THIS CONVERSATION — \
             including names this project invented; no acronym without its full words; and no \
             path of reasoning longer than the point needs. Prefer the short true sentence to \
             the complete one. This governs what you SAY, never what you write into code, \
             commits or specs."
                .to_string()
        })
}

/// The installation-gate refusal (didactic, short, technical EN).
const NOT_INSTALLED_REASON: &str = "Mustard is not installed in this project (no mustard.json at \
     the root). Run /mustard:upsert to install it — everything else stays disabled until then.";

impl Check for PromptSubmitInject {
    /// On `UserPromptSubmit`: first the installation gate — a `/mustard:*`
    /// command (except `/mustard:upsert`) is denied when the project has no
    /// `mustard.json` at the root. Then close the session's amendment window
    /// when the prompt starts a new pipeline. For a non-`/mustard:*` prompt
    /// the verdict composes the declared injectables (`mustard.json#inject`,
    /// `on: userPromptSubmit`) and the W8.T8.2 pipeline-in-flight banner into
    /// ONE `Inject` — injectables first, banner after, the writing rule
    /// (`mustard.json#tone`) last; any one alone also injects. A `/mustard:*`
    /// prompt receives neither injectables nor banner (it is already inside the
    /// flow) but DOES carry the writing rule, which governs how the ANSWER is
    /// written rather than the work. Any non-`UserPromptSubmit` trigger
    /// self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::UserPromptSubmit) {
            return Ok(Verdict::Allow);
        }
        let prompt = input
            .raw
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let cwd = ctx.project_dir_or_cwd(input);
        // Installation gate — BEFORE everything else (without an installation
        // there is no amend window to close and nothing to inject): any
        // `/mustard:*` command except the bootstrap door `/mustard:upsert` is
        // denied when `mustard.json` is absent from the project root. Normal
        // prompts are never gated — the hooks stay silent on uninstalled
        // projects.
        if is_mustard_command(prompt)
            && !is_upsert_prompt(prompt)
            && !ProjectConfig::exists(Path::new(&cwd))
        {
            return Ok(Verdict::Deny { reason: NOT_INSTALLED_REASON.to_string() });
        }
        if is_pipeline_prompt(prompt) {
            // Close any open amendment windows for this session — the user is
            // starting a new pipeline, so the window's context is done.
            if let Some(session_id) = input.session_id.as_deref() {
                if !session_id.is_empty() {
                    close_amend_windows_for_session(&cwd, session_id);
                }
            }
        }
        // How to WRITE for this operator, from `mustard.json#tone`.
        let tone = tone_rule(Path::new(&cwd));
        // ANY slash command — Mustard's or a third party's — receives neither
        // injectables nor the banner: the flow that expanded owns the turn, and
        // a router that reclassifies an interview's answers opens a work unit
        // inside someone else's protocol. The writing rule is the exception,
        // and deliberately so: it governs how the ANSWER is written, and the
        // answer to a slash command is read by the same person as any other.
        // Excluding it here would drop the rule from precisely the messages
        // that produce the longest explanations.
        // Which sibling carries what belongs to the INVOCATION rather than to
        // one injectable — computed BEFORE the early return below, because a
        // slash-command prompt still delivers the writing rule and would
        // otherwise deliver it once per sibling (found in review: this repo
        // declares `tone: didactic`, so every `/mustard:*` prompt got the
        // paragraph twice).
        let carries_shared_blocks = carries_shared_blocks(&cwd, ctx.inject_only.as_deref());
        if is_slash_command(prompt) {
            return Ok(match tone.filter(|_| carries_shared_blocks) {
                Some(rule) => Verdict::Inject { context: rule },
                None => Verdict::Allow,
            });
        }
        // Declared injectables (`on: userPromptSubmit`) — fail-open; `once`
        // entries are tracked per session via `injected-*` markers.
        let injected = crate::hooks::session::injectables::collect(
            &cwd,
            input.session_id.as_deref(),
            "userpromptsubmit",
            false,
            ctx.inject_only.as_deref(),
        );
        // W8.T8.2 — inject a single-line reminder when a spec is active. The
        // per-prompt entrypoints census that used to fill the no-spec branch
        // was REMOVED: lexical prompt-token × path-token matching measured 1
        // useful hit in 17 across two field sessions — location is on-demand
        // work (Grep for literals, the digest for concepts), not a per-prompt
        // guess. Fail-open throughout.
        let banner = carries_shared_blocks
            .then(|| crate::shared::context::current_spec(&cwd))
            .flatten()
            .filter(|s| !s.is_empty())
            .map(|spec| {
                economy::emit(&cwd, ActorKind::Hook, "prompt_gate", "pipeline.economy.operation.invoked", None, serde_json::json!({"operation": "prompt_gate.pipeline_in_flight_banner", "duration_ms": 0, "tokens_used": 0}));
                format!("{PIPELINE_IN_FLIGHT_BANNER}: {spec}")
            });
        // ONE composed Inject — the dispatcher fold is last-writer-wins, so
        // the concerns of THIS invocation must share a verdict. Injectables
        // first, banner after, the writing rule last: it is about the answer,
        // not about the work. Across sibling hooks the fold does not apply:
        // Claude Code keeps every hook's additionalContext.
        let tone = carries_shared_blocks.then_some(tone).flatten();
        let parts: Vec<String> = [injected, banner, tone].into_iter().flatten().collect();
        let context = (!parts.is_empty()).then(|| parts.join("\n\n"));
        Ok(match context {
            Some(context) => Verdict::Inject { context },
            None => Verdict::Allow,
        })
    }
}

/// Emit a `pipeline.economy.operation.invoked` event via the NDJSON route.
/// Fail-open: any error degrades to a no-op.
///
/// W3C: routes via `crate::shared::events::route::emit` (NDJSON for
/// non-`pipeline.*` events, SQLite lifecycle index for `pipeline.*`).

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::ClaudePaths;

    /// Build a [`Ctx`] with a unique tempdir project path so the W8.T8.2 active-spec
    /// resolver (`current_spec`) cannot accidentally find a real pipeline-state.
    fn ctx() -> (tempfile::TempDir, Ctx) {
        // SAFETY: env mutation is local to the test process; we restore on drop.
        // Used to neutralise a `MUSTARD_ACTIVE_SPEC` that might be set by the
        // outer shell.
        // Note: we cannot call `std::env::remove_var` from safe Rust on stable;
        // instead, isolate via a unique project_dir (so `current_spec` falls
        // through to the FS branch and finds nothing).
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
            inject_only: None,
        };
        (dir, ctx)
    }

    fn prompt_input(prompt: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            raw: serde_json::json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    // --- pipeline-prompt recognition (parity with TRIGGER_RE) --------------

    #[test]
    fn recognises_pipeline_commands() {
        assert!(is_pipeline_prompt("/mustard:feature add-login"));
        assert!(is_pipeline_prompt("  /mustard:bugfix fix-thing"));
        assert!(is_pipeline_prompt("/MUSTARD:TASK do-it"));
        assert!(is_pipeline_prompt("/mustard:feature"));
    }

    #[test]
    fn rejects_non_pipeline_prompts() {
        assert!(!is_pipeline_prompt("just a normal message"));
        assert!(!is_pipeline_prompt("/mustard:git"));
        assert!(!is_pipeline_prompt("/mustard:featureish thing"));
        assert!(!is_pipeline_prompt("text /mustard:feature mid-line"));
    }

    // --- writing rule from `tone` -------------------------------------------

    /// Seed a project whose `mustard.json` declares `tone`, and return its dir.
    fn project_declaring_tone(tone: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("mustard.json"),
            format!(r#"{{"specLang":"pt-BR","tone":"{tone}"}}"#),
        )
        .expect("write config");
        dir
    }

    /// The verdict for one prompt in a project declaring `tone`, through the
    /// REAL gate — never the private helper.
    ///
    /// A test that called `tone_rule` directly is what shipped an unprovable
    /// criterion: a review removed `tone` from the ordinary-prompt composition
    /// and the test stayed green, because it never asked the gate anything.
    /// Everything below goes through `evaluate`, so deleting the wiring fails
    /// the criterion that claims to guard it.
    fn verdict_for(tone: &str, prompt: &str) -> (tempfile::TempDir, Verdict) {
        let dir = project_declaring_tone(tone);
        let c = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
            inject_only: None,
        };
        let verdict =
            PromptSubmitInject.evaluate(&prompt_input(prompt), &c).expect("the gate never errors");
        (dir, verdict)
    }

    /// AC-1 — an ORDINARY prompt carries the writing rule, through the gate.
    ///
    /// Every prompt, not once per session: delivered once, the rule drifts
    /// away while the thing it governs — the next answer — is always the
    /// newest.
    #[test]
    fn the_writing_rule_rides_every_prompt() {
        let (_dir, verdict) = verdict_for("didactic", "uma mensagem comum");

        match verdict {
            Verdict::Inject { context } => {
                assert!(context.contains("tone: didactic"), "names its source: {context}");
                assert!(
                    context.contains("ONE idea per sentence"),
                    "and carries the rule: {context}",
                );
                assert!(
                    context.contains("never what you write into code"),
                    "and bounds itself to speech: {context}",
                );
            }
            other => panic!("an ordinary prompt must carry the rule, got {other:?}"),
        }
    }

    /// The accented spelling a Brazilian operator actually writes is accepted.
    /// A hand-rolled `didactic`/`didatico` match rejected it, so a project
    /// declaring the word in its own language was read as never having
    /// declared — the very defect this unit exists to remove.
    #[test]
    fn the_accented_spelling_counts_as_declared() {
        let (_dir, verdict) = verdict_for("didático", "uma mensagem comum");
        assert!(
            matches!(verdict, Verdict::Inject { ref context } if context.contains("ONE idea per sentence")),
            "`didático` is the canonical parser's own spelling: {verdict:?}",
        );
    }

    /// AC-2 — a project that declared nothing gets nothing. The RESOLVED tone
    /// defaults to `didactic`, so reading it would put this paragraph in front
    /// of every project that merely has a `mustard.json`.
    #[test]
    fn an_undeclared_tone_injects_nothing() {
        // A project that chose a DIFFERENT tone, asked through the gate.
        let (_dir, verdict) = verdict_for("technical", "uma mensagem comum");
        assert!(
            !matches!(verdict, Verdict::Inject { ref context } if context.contains("ONE idea per sentence")),
            "a technical project asked for nothing: {verdict:?}",
        );

        // A config with NO `tone` key never chose one. The resolved value
        // defaults to `didactic`, so this is the case a resolved read would
        // get wrong — and the one that would put the rule in front of every
        // project that merely has a `mustard.json`.
        let bare = tempfile::tempdir().expect("temp dir");
        std::fs::write(bare.path().join("mustard.json"), r#"{"specLang":"pt-BR"}"#)
            .expect("write config");
        let c = Ctx {
            project_dir: bare.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
            inject_only: None,
        };
        let verdict = PromptSubmitInject
            .evaluate(&prompt_input("uma mensagem comum"), &c)
            .expect("the gate never errors");
        assert!(
            !matches!(verdict, Verdict::Inject { ref context } if context.contains("ONE idea per sentence")),
            "the default is not a choice: {verdict:?}",
        );

        // …and a project with NO `mustard.json` at all. The criterion names
        // three cases and this is the third; asserting two of them left the
        // uninstalled project — where the hooks are supposed to stay silent —
        // proved only by hand.
        let none = tempfile::tempdir().expect("temp dir");
        let c = Ctx {
            project_dir: none.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
            inject_only: None,
        };
        let verdict = PromptSubmitInject
            .evaluate(&prompt_input("uma mensagem comum"), &c)
            .expect("the gate never errors");
        assert!(
            matches!(verdict, Verdict::Allow),
            "an uninstalled project declared nothing, so the hooks stay silent: {verdict:?}",
        );
    }

    /// AC-3 — a `/mustard:*` prompt carries it too. That branch drops the
    /// injectables and the banner because a slash command knows its own
    /// context; the writing rule is different in kind, because it governs how
    /// the ANSWER is written and that answer is read by the same person.
    #[test]
    fn the_writing_rule_rides_a_slash_command_too() {
        let dir = project_declaring_tone("didactic");
        let c = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
            inject_only: None,
        };
        let verdict = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:pr merge"), &c)
            .expect("the gate never errors");

        match verdict {
            Verdict::Inject { context } => assert!(
                context.contains("ONE idea per sentence"),
                "a slash command must carry the writing rule: {context}",
            ),
            other => panic!("expected the writing rule to ride along, got {other:?}"),
        }
    }

    // --- verdict — always allow --------------------------------------------

    #[test]
    fn pipeline_prompt_allows() {
        // The amendment-window close is a no-op without an open window; the
        // verdict is Allow when no spec is active (and the prompt itself is a
        // `/mustard:*` command, so the W8.T8.2 banner is suppressed either way).
        // The project is INSTALLED (mustard.json present) so the installation
        // gate stays out of the way — this is the historical behavior.
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let v = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:feature x"), &c)
            .unwrap();
        // For a `/mustard:*` command, never Inject regardless of spec state.
        assert!(matches!(v, Verdict::Allow), "unexpected verdict: {v:?}");
    }

    // --- installation gate --------------------------------------------------

    #[test]
    fn gate_denies_mustard_command_without_installation() {
        // No mustard.json in the tempdir: any `/mustard:*` command (pipeline
        // or not) is denied with the didactic pointer to /mustard:upsert.
        let (_dir, c) = ctx();
        for prompt in ["/mustard:feature x", "/mustard:git", "  /MUSTARD:QA"] {
            let v = PromptSubmitInject.evaluate(&prompt_input(prompt), &c).unwrap();
            match v {
                Verdict::Deny { reason } => {
                    assert!(
                        reason.contains("/mustard:upsert"),
                        "reason must point at the bootstrap door: {reason}"
                    );
                    assert!(
                        reason.contains("mustard.json"),
                        "reason must name the missing anchor: {reason}"
                    );
                }
                other => panic!("expected Deny for {prompt:?} without mustard.json, got {other:?}"),
            }
        }
    }

    #[test]
    fn gate_allows_upsert_without_installation() {
        // The bootstrap door itself must pass — it is how the project gets
        // installed. Word-boundary: a hypothetical `/mustard:upsertish` is a
        // different (unknown) command and stays gated.
        let (_dir, c) = ctx();
        let v = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:upsert"), &c)
            .unwrap();
        assert_eq!(v, Verdict::Allow, "/mustard:upsert must pass the gate");
        let v = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:upsertish"), &c)
            .unwrap();
        assert!(matches!(v, Verdict::Deny { .. }), "boundary must hold: {v:?}");
    }

    #[test]
    fn gate_allows_bare_mustard_help_without_installation() {
        // The bare `/mustard` (no colon) is the orientation door — it must
        // keep working so it can point the user at /mustard:upsert.
        let (_dir, c) = ctx();
        let v = PromptSubmitInject.evaluate(&prompt_input("/mustard"), &c).unwrap();
        assert!(
            matches!(v, Verdict::Allow | Verdict::Inject { .. }),
            "bare /mustard must not be denied: {v:?}"
        );
    }

    #[test]
    fn gate_ignores_normal_prompts_without_installation() {
        // Free-text prompts are never gated — the hooks stay silent on
        // uninstalled projects (Allow, or an env-var banner Inject; never Deny).
        let (_dir, c) = ctx();
        let v = PromptSubmitInject.evaluate(&prompt_input("hello there"), &c).unwrap();
        assert!(
            !matches!(v, Verdict::Deny { .. }),
            "a normal prompt must never be denied: {v:?}"
        );
    }

    #[test]
    fn non_pipeline_prompt_allows_without_active_spec() {
        // No `.claude/.pipeline-states/` in our tempdir, so `current_spec`
        // returns None and the W8.T8.2 banner stays silent.
        let (_dir, c) = ctx();
        // The env-var branch can still inject; guard by checking either Allow
        // (the expected case in CI) or Inject (when MUSTARD_ACTIVE_SPEC is set
        // by the outer shell).
        let v = PromptSubmitInject.evaluate(&prompt_input("hello there"), &c).unwrap();
        assert!(
            matches!(v, Verdict::Allow | Verdict::Inject { .. }),
            "unexpected verdict: {v:?}",
        );
    }

    #[test]
    fn non_pipeline_prompt_injects_with_active_spec() {
        // W8.T8.2: when a spec is active, the user's free-text prompt gets a
        // single-line banner injected.
        let (dir, _) = ctx();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(paths.pipeline_state_file("active-feature-xyz"), "{}").unwrap();
        let c = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
            inject_only: None,
        };
        let v = PromptSubmitInject
            .evaluate(&prompt_input("how do I do X?"), &c)
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(
                    context.contains(PIPELINE_IN_FLIGHT_BANNER),
                    "banner missing: {context}"
                );
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    #[test]
    fn non_user_prompt_submit_trigger_allows() {
        let other = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
            inject_only: None,
        };
        assert_eq!(
            PromptSubmitInject
                .evaluate(&prompt_input("/mustard:feature x"), &other)
                .unwrap(),
            Verdict::Allow
        );
    }

    // --- declared injectables (orchestrator-redesign) ----------------------

    fn prompt_input_with_session(prompt: &str, session: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some(session.to_string()),
            raw: serde_json::json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    /// Declare one `on: userPromptSubmit, once: true` injectable + its file.
    fn seed_injectable(dir: &std::path::Path, body: &str) {
        std::fs::write(
            dir.join("mustard.json"),
            r#"{"inject":[{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true}]}"#,
        )
        .unwrap();
        let mustard_dir = dir.join(".claude").join("mustard");
        std::fs::create_dir_all(&mustard_dir).unwrap();
        std::fs::write(mustard_dir.join("orchestrator.md"), body).unwrap();
    }

    #[test]
    fn first_prompt_injects_declared_file_and_records_marker() {
        let (dir, c) = ctx();
        seed_injectable(dir.path(), "ORCH-RULES-BODY\n");

        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("how do I add a field?", "sess-1"), &c)
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("ORCH-RULES-BODY"), "injectable missing: {context}");
            }
            other => panic!("expected Inject with the declared file, got {other:?}"),
        }
        assert!(
            dir.path()
                .join(".claude/.session/sess-1/injected-orchestrator.md")
                .is_file(),
            "delivery marker must be recorded"
        );
    }

    #[test]
    fn second_prompt_same_session_does_not_repeat_once_injectable() {
        let (dir, c) = ctx();
        seed_injectable(dir.path(), "ORCH-RULES-BODY\n");
        let input = prompt_input_with_session("first question", "sess-1");
        let _ = PromptSubmitInject.evaluate(&input, &c).unwrap();

        // Same session, next prompt: the once-entry stays quiet. The verdict
        // may still be an Inject when the outer shell exports
        // MUSTARD_ACTIVE_SPEC (the W8.T8.2 banner) — assert on the CONTENT.
        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("second question", "sess-1"), &c)
            .unwrap();
        if let Verdict::Inject { context } = v {
            assert!(
                !context.contains("ORCH-RULES-BODY"),
                "once injectable must not re-deliver in the same session: {context}"
            );
        }
    }

    #[test]
    fn mustard_command_prompt_gets_no_injectables() {
        let (dir, c) = ctx();
        seed_injectable(dir.path(), "ORCH-RULES-BODY\n");
        // A `/mustard:*` prompt is already inside the flow — strict Allow, and
        // no delivery marker is burned (the next free-text prompt still gets it).
        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("/mustard:git", "sess-1"), &c)
            .unwrap();
        assert_eq!(v, Verdict::Allow, "slash command must not receive injectables");
        assert!(
            !dir.path()
                .join(".claude/.session/sess-1/injected-orchestrator.md")
                .exists(),
            "no marker burned on a slash-command prompt"
        );
    }

    /// AC-8 — ANY slash command owns its turn, not just Mustard's own.
    ///
    /// The carve-out used to match `/mustard:` alone, so a third party's
    /// interview skill was routed over: the operator answered one of its
    /// questions and the router read that answer as a fresh request, opening a
    /// work unit inside someone else's protocol.
    #[test]
    fn any_slash_command_prompt_gets_no_injectables() {
        let (dir, c) = ctx();
        seed_injectable(dir.path(), "ORCH-RULES-BODY\n");
        for prompt in ["/grill-me", "/review-pr 42", "/some-plugin:deploy"] {
            let v = PromptSubmitInject
                .evaluate(&prompt_input_with_session(prompt, "sess-1"), &c)
                .unwrap();
            assert_eq!(v, Verdict::Allow, "`{prompt}` must not receive injectables");
        }
        assert!(
            !dir.path().join(".claude/.session/sess-1/injected-orchestrator.md").exists(),
            "no marker burned on a slash-command prompt",
        );
        // Free text still routes — the carve-out must not swallow ordinary work.
        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("arrume o botao de login", "sess-2"), &c)
            .unwrap();
        assert!(matches!(v, Verdict::Inject { .. }), "free text still gets the router");
    }

    /// The bare `/mustard` help and path-shaped prompts are NOT slash commands.
    ///
    /// `/mustard` (no colon) is the orientation door and must keep working on an
    /// uninstalled project; a prompt that merely opens with a path is ordinary
    /// work and still needs the router.
    #[test]
    fn the_help_door_and_paths_are_not_slash_commands() {
        assert!(!is_slash_command("/mustard"));
        assert!(!is_slash_command("/mustard como funciona"));
        assert!(!is_slash_command("/etc/hosts esta errado"));
        assert!(!is_slash_command("/"));
        assert!(is_slash_command("/mustard:git"));
        assert!(is_slash_command("  /grill-me"));
        // A bare short path satisfies the character rules and is NOT a command:
        // the operator asking about `/tmp` would lose the router on that prompt.
        assert!(!is_slash_command("/tmp"));
        assert!(!is_slash_command("/usr"));
        assert!(!is_slash_command("/opt"));
    }

    #[test]
    fn missing_declared_file_stays_fail_open() {
        let (dir, c) = ctx();
        // Declared, but the file was never materialised on disk.
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"inject":[{"on":"userPromptSubmit","file":".claude/mustard/gone.md","once":true}]}"#,
        )
        .unwrap();
        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("hello", "sess-1"), &c)
            .unwrap();
        // Allow in a clean environment; an env-var active spec may still
        // banner-inject — either way the missing file must not break the hook.
        assert!(
            matches!(v, Verdict::Allow | Verdict::Inject { .. }),
            "unexpected verdict: {v:?}"
        );
        assert!(
            !dir.path().join(".claude/.session/sess-1/injected-gone.md").exists(),
            "no marker for an undelivered entry"
        );
    }
}
