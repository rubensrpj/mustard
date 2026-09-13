//! `prompt_submit_inject` — the UserPromptSubmit gate module.
//!
//! ## Scope (prompt family + orchestrator-redesign injectables)
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
//! - `followup-cancel-gate` (the port of the old JS hook): when the prompt invokes
//!   `/mustard:feature`, `/mustard:bugfix`, or `/mustard:task`, close any open
//!   per-session amendment window — the previous follow-up window is over, so
//!   subsequent edits belong to a new context.
//! - **declared injectables** (orchestrator-redesign): the
//!   `mustard.json#inject` entries with `on: userPromptSubmit` (canonically
//!   the orchestrator rules in `.claude/mustard/orchestrator.md`) are spliced
//!   into the window via [`crate::hooks::session::injectables::collect`] —
//!   once per session when `once: true`. A `/mustard:*` prompt gets NO
//!   injectables (the slash command is already inside the flow).
//! - **writing rule**: every installed project carries a one-paragraph rule
//!   for how the answer is written — the tone is one, plain and didactic, with
//!   no key to turn it off. It is the one concern a `/mustard:*` prompt still
//!   receives, and deliberately so: it governs how the ANSWER is written, and
//!   the answer to a slash command is read by the same person as any other.
//!   Delivered on EVERY prompt rather than once per session — the thing it
//!   governs is always the newest message, so a rule delivered once only
//!   drifts further from it.
//! - **regra de idioma** (`mustard.json` `language.text`): todo projeto
//!   instalado recebe em todo prompt a ordem de responder no idioma do
//!   usuário. Anda logo depois da regra de escrita e, como ela, também chega a
//!   um comando com barra.
//!
//! The three injecting concerns compose into a SINGLE [`Verdict::Inject`]:
//! injectables first, banner next, the writing and language rules last (the
//! previous reply's clarity defects no longer ride here: the end-of-turn check
//! hands them over in its own block). The
//! dispatcher fold would join separate Injects too, but in registry order;
//! this is the only `Check` that
//! injects on this event, so composing here keeps the order stated in one
//! place. The composed text is ONE hook response under ONE 10,000-character
//! ceiling.
//!
//! ## Contract shape
//!
//! `followup-cancel-gate.js` never blocked — it always `process.exit(0)`. The
//! hook contract classes `prompt_gate` as a [`Check`], which is exactly why the
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
//! ## Migration off SQLite
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

/// Pipeline-in-flight reminder: surfaced when the user's prompt is
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
///
/// **Nor does a slash token followed by a SENTENCE.** An earlier version told
/// commands from paths with a closed list of filesystem roots, and everything
/// else with a word-shaped first segment counted as a command. Application
/// route names are word-shaped and cannot be enumerated, so `/login nao
/// funciona`, `/checkout quebrou em producao` and `/api retorna 500` all read
/// as commands and lost the router — reproducing, silently, the very field
/// symptom this unit exists to remove (measured in review, against the binary).
///
/// Two rules, because the two shapes carry different amounts of evidence.
///
/// A NAMESPACED token — one holding a `:` — is always a command. `/mustard:pr
/// merge 212` cannot be a route: no URL path segment carries a colon, and the
/// namespace is the plugin declaring the command as its own. Arguments after it
/// are unrestricted.
///
/// A BARE token is weaker evidence, so it is a command only when it stands
/// alone or carries a single short argument: `/grill-me`, `/init`, `/review
/// 212`. Anything longer is a sentence, and a sentence about `/login` is a bug
/// report that needs the router.
///
/// The line falls where it does because the two mistakes do not cost the same.
/// Reading a work request as a command drops the router SILENTLY, and the
/// operator never learns why the unit opened on the wrong branch. Reading a
/// command as work adds a paragraph to a turn that already had its own context
/// — visible, and harmless. A bare command that takes several arguments pays
/// that harmless cost; a bug report never pays the silent one.
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
    // command: the operator is asking about a directory, and reading it as a
    // command costs them the router on that prompt.
    //
    // Told apart by NAME, not by length. A length threshold cut both ways —
    // `/init`, `/help`, `/cost`, `/plan` are real commands at four characters
    // or fewer, and `/proc` is a directory at five (found in review). The
    // filesystem roots are a short, closed list; everything else is a command.
    const FS_ROOTS: &[&str] = &[
        "tmp", "usr", "var", "etc", "opt", "bin", "sbin", "lib", "home", "root", "proc", "sys",
        "dev", "boot", "mnt", "media", "srv", "run",
    ];
    if FS_ROOTS.iter().any(|r| name.eq_ignore_ascii_case(r)) {
        return false;
    }
    // A namespaced token is unambiguous — no route segment carries a colon —
    // so its arguments are unrestricted.
    if name.contains(':') {
        return true;
    }
    // A BARE token is weaker evidence: a command invocation, or a route named
    // at the start of a sentence about it. Told apart by length, leaning toward
    // routing. See the doc above.
    const MAX_BARE_ARGUMENT_WORDS: usize = 1;
    rest.split_whitespace().count() <= 1 + MAX_BARE_ARGUMENT_WORDS
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
// writing rule — carried with every prompt of every installed project
// ===========================================================================

/// How to write the answer, carried with EVERY prompt of every project with a
/// `mustard.json`.
///
/// The tone is one, plain and didactic, and no key turns it off: the end of
/// the answer measures the same rule in every project (`clarity_check`), so
/// the rule and its measure answer to the same condition. It used to ride only
/// the projects that declared a tone, and the operator found the gap the
/// honest way: by not understanding an explanation, twice.
///
/// **Every prompt, not once per session.** Delivered once, the rule drifts
/// further away with each exchange while the thing it governs — the next
/// answer — is always the newest. Measured before this was accepted: 126
/// tokens, about 0.04% of a long session, against the thousand-plus a single
/// misunderstanding costs in a wrong answer, a correction and a rewrite.
///
/// `None` for a project with no `mustard.json`: there the hooks stay silent.
fn writing_rule(root: &Path) -> Option<String> {
    ProjectConfig::exists(root).then(|| {
        "[Mustard] Write every user-facing answer so it can be read once, by someone who did \
         not write this code: ONE idea per sentence; every technical term translated the first \
         time it appears IN THIS CONVERSATION — including names this project invented; no \
         acronym without its full words; and no path of reasoning longer than the point needs. \
         Prefer the short true sentence to the complete one. This governs what you SAY, never \
         what you write into code, commits or specs."
            .to_string()
    })
}

/// Em que idioma responder, levado em TODO prompt de todo projeto com
/// `mustard.json`: o idioma do usuário, que é o do projeto (`mustard.json`
/// `language.text`). Em 10/09/2026 o assistente respondeu em inglês por vários
/// turnos a quem escreve em português: tinha acabado de ler skills e
/// relatórios em inglês, e nada na regra falava de idioma. O usuário pediu que
/// valesse para todo projeto. `None` sem `mustard.json`: num projeto sem o
/// Mustard os ganchos ficam calados.
///
/// O idioma só é nomeado quando o projeto o DECLAROU: o padrão resolvido é
/// pt-BR, e dizer "o deste projeto é pt-BR" a um projeto em inglês que nunca
/// declarou idioma mandaria responder na língua errada. Sem declaração, a regra
/// manda seguir o idioma do usuário sem nomear nenhum.
fn language_rule(root: &Path) -> Option<String> {
    ProjectConfig::exists(root).then(|| {
        let named = match ProjectConfig::load(root).language().text {
            Some(lang) => format!(" — this project's is {lang} (`mustard.json` `language.text`) —"),
            None => ",".to_string(),
        };
        format!(
            "[Mustard] Answer the user in the language they write in{named} even after reading \
             skills, references or reports written in another language; code, commits and \
             subagent prompts keep their own conventions."
        )
    })
}

/// As regras de escrita — a da escrita e a do idioma. Só o irmão que carrega
/// os blocos do evento as leva. `None` sem regra ou fora desse irmão. Os
/// defeitos da resposta anterior não andam mais aqui: a conferência do fim da
/// resposta os entrega no próprio bloqueio.
fn writing_blocks(writing: Option<String>, language: Option<String>, carries: bool) -> Option<String> {
    if !carries {
        return None;
    }
    let blocks: Vec<String> = [writing, language].into_iter().flatten().collect();
    (!blocks.is_empty()).then(|| blocks.join("\n\n"))
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
    /// `on: userPromptSubmit`) and the pipeline-in-flight banner into
    /// ONE `Inject` — injectables first, banner after, the writing and
    /// language rules last; any one alone also injects. A `/mustard:*`
    /// prompt receives neither injectables nor banner (it is already inside the
    /// flow) but DOES carry the writing rule, which governs how the ANSWER is
    /// written rather than the work. Any non-`UserPromptSubmit` trigger
    /// self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::UserPromptSubmit) {
            return Ok(Verdict::Allow);
        }
        let prompt = input.user_prompt().unwrap_or_default();
        let cwd = ctx.project_dir_or_cwd(input);
        // Installation gate — BEFORE everything else (without an installation
        // there is no amend window to close and nothing to inject): any
        // `/mustard:*` command except the bootstrap door `/mustard:upsert` is
        // denied when `mustard.json` is absent from the project root. Normal
        // prompts are never gated — the hooks stay silent on uninstalled
        // projects.
        //
        // **Every sibling emits this Deny, and it is not worth silencing.** The
        // duplicate was raised in review, and the obvious fix — electing one
        // sibling, as the writing rule below does — is DEAD CODE on this path:
        // the election reads the declared inject list, and this gate only fires
        // when `mustard.json` is absent, so that list is empty and every
        // sibling elects itself. An election that can never elect is worse than
        // the duplicate it hides, because the next reader trusts it.
        //
        // The honest alternative would gate on the invocation's own `--inject`
        // ordering, read from the manifest — real work, for a cosmetic gain, on
        // the one path where being loud is the safe direction: any single Deny
        // blocks the prompt, so a silenced sibling can only ever cost the
        // block, never duplicate it.
        let carries_shared_blocks = carries_shared_blocks(&cwd, ctx.inject_only.as_deref());
        if is_mustard_command(prompt)
            && !is_upsert_prompt(prompt)
            && !ProjectConfig::exists(Path::new(&cwd))
        {
            return Ok(Verdict::Deny { reason: NOT_INSTALLED_REASON.to_string() });
        }
        if is_pipeline_prompt(prompt) {
            // Close any open amendment windows for this session — the user is
            // starting a new pipeline, so the window's context is done.
            if let Some(session_id) = input.session_id.as_deref()
                && !session_id.is_empty() {
                    close_amend_windows_for_session(&cwd, session_id);
                }
        }
        // How to WRITE for this operator: every installed project.
        let writing = writing_rule(Path::new(&cwd));
        // Em que idioma responder: vale para todo projeto instalado.
        let language = language_rule(Path::new(&cwd));
        // As regras de escrita, só por este irmão quando é ele quem carrega os
        // blocos do evento.
        let writing = writing_blocks(writing, language, carries_shared_blocks);
        // ANY slash command — Mustard's or a third party's — receives neither
        // injectables nor the banner: the flow that expanded owns the turn, and
        // a router that reclassifies an interview's answers opens a work unit
        // inside someone else's protocol. The writing rule is the exception,
        // and deliberately so: it governs how the ANSWER is written, and the
        // answer to a slash command is read by the same person as any other.
        // Excluding it here would drop the rule from precisely the messages
        // that produce the longest explanations.
        // `carries_shared_blocks` was resolved above, before the installation
        // gate, because a slash-command prompt still delivers the writing rule
        // and would otherwise deliver it once per sibling (found in review:
        // every `/mustard:*` prompt of this repo got the paragraph twice).
        if is_slash_command(prompt) {
            return Ok(match writing {
                Some(context) => Verdict::Inject { context },
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
        // Inject a single-line reminder when a spec is active. The
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
        // ONE composed Inject, in the order stated here: injectables first,
        // banner after, the writing rule last — it is about the answer, not
        // about the work. The dispatcher fold would join separate Injects too,
        // but in registry order; composing keeps the order local. It is still
        // ONE response under ONE 10,000-character ceiling. Across sibling hooks
        // the fold does not apply: each is its own invocation, and Claude Code
        // keeps every hook's additionalContext.
        let parts: Vec<String> = [injected, banner, writing].into_iter().flatten().collect();
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
/// Routes via `crate::shared::events::route::emit` (NDJSON for
/// non-`pipeline.*` events, SQLite lifecycle index for `pipeline.*`).
#[cfg(test)]
mod tests {
    use super::*;

    /// Build a [`Ctx`] with a unique tempdir project path so the active-spec
    /// resolver (`current_spec`) cannot accidentally find a real pipeline-state.
    fn ctx() -> (tempfile::TempDir, Ctx) {
        // SAFETY: env mutation is local to the test process; we restore on drop.
        // Used to neutralise a `MUSTARD_ACTIVE_SPEC` that might be set by the
        // outer shell.
        // Note: we cannot call `std::env::remove_var` from safe Rust on stable;
        // instead, isolate via a unique project_dir (so `current_spec` falls
        // through to the FS branch and finds nothing).
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
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

    // --- writing rule ---------------------------------------------------------

    /// Um projeto que declarou o português do Brasil como idioma do texto.
    const PT_PROJECT: &str = r#"{"language":{"text":"pt-BR"}}"#;

    /// Seed a project with this `mustard.json`, and return its dir.
    fn project_with(config: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("mustard.json"), config).expect("write config");
        dir
    }

    /// The verdict for one prompt in a project with this `mustard.json`,
    /// through the REAL gate — never the private helper.
    ///
    /// A test that called the rule's helper directly is what shipped an
    /// unprovable criterion: a review removed the rule from the ordinary-prompt
    /// composition and the test stayed green, because it never asked the gate
    /// anything. Everything below goes through `evaluate`, so deleting the
    /// wiring fails the criterion that claims to guard it.
    fn verdict_for(config: &str, prompt: &str) -> (tempfile::TempDir, Verdict) {
        let dir = project_with(config);
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        let verdict =
            PromptSubmitInject.evaluate(&prompt_input(prompt), &c).expect("the gate never errors");
        (dir, verdict)
    }

    /// An ORDINARY prompt carries the writing rule, through the gate.
    ///
    /// Every prompt, not once per session: delivered once, the rule drifts
    /// away while the thing it governs — the next answer — is always the
    /// newest.
    #[test]
    fn the_writing_rule_rides_every_prompt() {
        let (_dir, verdict) = verdict_for(PT_PROJECT, "uma mensagem comum");

        match verdict {
            Verdict::Inject { context } => {
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

    /// A regra de escrita manda responder no idioma em que o usuário
    /// escreve, o do projeto, mesmo depois de ler material em outro idioma. O
    /// idioma nomeado sai do `mustard.json`, nunca de um valor fixo.
    #[test]
    fn the_writing_rule_demands_the_user_language() {
        let (_dir, verdict) = verdict_for(PT_PROJECT, "uma mensagem comum");
        let Verdict::Inject { context } = verdict else {
            panic!("an ordinary prompt must carry the rule, got {verdict:?}");
        };
        assert!(context.contains("in the language they write in"), "{context}");
        assert!(context.contains("this project's is pt-BR"), "{context}");
        assert!(context.contains("skills, references or reports"), "{context}");
        assert!(context.contains("subagent prompts keep their own conventions"), "{context}");

        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("mustard.json"), r#"{"language":{"text":"en-US"}}"#)
            .expect("write config");
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        let verdict =
            PromptSubmitInject.evaluate(&prompt_input("a plain message"), &c).expect("the gate never errors");
        let Verdict::Inject { context } = verdict else {
            panic!("an ordinary prompt must carry the rule, got {verdict:?}");
        };
        assert!(context.contains("this project's is en-US"), "{context}");
    }

    /// Sem `language.text` no `mustard.json`, o idioma nunca é suposto, e as
    /// chaves antigas de idioma não contam como declaração: a regra manda
    /// seguir o idioma do usuário sem nomear idioma de projeto, e uma resposta
    /// em inglês não ganha defeito de idioma. Com pt-BR declarado, o defeito
    /// continua.
    #[test]
    fn undeclared_language_is_never_assumed() {
        use crate::hooks::task::end_of_turn_check::EndOfTurnCheck;

        let english = "The wave is done and the tests pass.\n\
            The check now compares the language of the reply with the language of the project.\n\
            It counts the common words of each language.\n\
            A short reply is not judged at all.";
        let stop = HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s1".to_string()),
            raw: serde_json::json!({ "last_assistant_message": english }),
            ..HookInput::default()
        };
        let project = |config: &str| {
            let dir = tempfile::tempdir().expect("temp dir");
            std::fs::write(dir.path().join("mustard.json"), config).expect("write config");
            let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
            (dir, c)
        };
        // O texto do veredito: a regra que o prompt leva, ou o bloqueio e o
        // aviso do fim da resposta.
        let context_of = |verdict: Verdict| match verdict {
            Verdict::Inject { context } => context,
            Verdict::Deny { reason } => reason,
            _ => String::new(),
        };

        for config in ["{}", r#"{"specLang":"pt-BR","lang":"pt-BR"}"#] {
            let (_dir, c) = project(config);
            let rule = context_of(
                PromptSubmitInject
                    .evaluate(&prompt_input_with_session("uma mensagem comum", "s1"), &c)
                    .expect("the gate never errors"),
            );
            assert!(rule.contains("in the language they write in"), "{config}: {rule}");
            for named in ["this project's is", "pt-BR", "en-US"] {
                assert!(!rule.contains(named), "{config}: an undeclared language is named ({named}): {rule}");
            }

            let on_stop = Ctx { trigger: Some(Trigger::Stop), ..c.clone() };
            let block = context_of(EndOfTurnCheck.evaluate(&stop, &on_stop).expect("the check never errors"));
            assert!(!block.contains("resposta em"), "{config}: no language verdict: {block}");
        }

        // Com pt-BR declarado, a mesma resposta continua barrada pelo idioma.
        let (_dir, c) = project(PT_PROJECT);
        let on_stop = Ctx { trigger: Some(Trigger::Stop), ..c };
        let block = context_of(EndOfTurnCheck.evaluate(&stop, &on_stop).expect("the check never errors"));
        assert!(block.contains("resposta em en-US; o idioma do projeto e do usuário é pt-BR"), "{block}");
    }

    /// O veredito de um prompt que não recebe injetável nem aviso num projeto
    /// instalado: só as regras de escrita e de idioma, que valem para todo
    /// projeto e regem a resposta, não o trabalho.
    fn writing_rules_only(root: &Path) -> Verdict {
        Verdict::Inject {
            context: writing_blocks(writing_rule(root), language_rule(root), true)
                .expect("an installed project has the rules"),
        }
    }

    /// A regra de idioma e a medição de idioma valem para todo projeto com
    /// `mustard.json`. Com ou sem a antiga chave do tom, o prompt leva a regra
    /// de idioma e a de escrita, e uma resposta em inglês num projeto em pt-BR
    /// é barrada no fim da resposta com o defeito de idioma; a mensagem
    /// seguinte não o repete. Sem `mustard.json`, nada.
    #[test]
    fn language_rule_reaches_every_mustard_project() {
        use crate::hooks::task::end_of_turn_check::EndOfTurnCheck;

        let english = "The wave is done and the tests pass.\n\
            The check now compares the language of the reply with the language of the project.\n\
            It counts the common words of each language.\n\
            A short reply is not judged at all.";
        let defect = "resposta em en-US; o idioma do projeto e do usuário é pt-BR";
        let stop = HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s1".to_string()),
            raw: serde_json::json!({ "last_assistant_message": english }),
            ..HookInput::default()
        };

        for config in [PT_PROJECT, r#"{"language":{"text":"pt-BR"},"tone":"technical"}"#] {
            let dir = tempfile::tempdir().expect("temp dir");
            std::fs::write(dir.path().join("mustard.json"), config).expect("write config");
            let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));

            let verdict = PromptSubmitInject
                .evaluate(&prompt_input_with_session("uma mensagem comum", "s1"), &c)
                .expect("the gate never errors");
            let Verdict::Inject { context } = verdict else {
                panic!("{config}: every installed project carries the language rule, got {verdict:?}");
            };
            assert!(context.contains("in the language they write in"), "{config}: {context}");
            assert!(context.contains("this project's is pt-BR"), "{config}: {context}");
            assert!(context.contains("ONE idea per sentence"), "{config}: the writing rule rides too: {context}");

            let on_stop = Ctx { trigger: Some(Trigger::Stop), ..c.clone() };
            let Verdict::Deny { reason } =
                EndOfTurnCheck.evaluate(&stop, &on_stop).expect("the check never errors")
            else {
                panic!("{config}: an English reply in a pt-BR project is blocked");
            };
            assert!(reason.contains(defect), "{config}: {reason}");

            let next = PromptSubmitInject
                .evaluate(&prompt_input_with_session("e agora?", "s1"), &c)
                .expect("the gate never errors");
            let Verdict::Inject { context: next } = next else {
                panic!("{config}: the next prompt still carries the rule, got {next:?}");
            };
            assert!(!next.contains(defect), "{config}: the defect rode the block, not the next prompt: {next}");
        }

        // Sem `mustard.json` os ganchos ficam calados: nem regra, nem medição.
        let none = tempfile::tempdir().expect("temp dir");
        let c = Ctx::for_test(none.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        let verdict = PromptSubmitInject
            .evaluate(&prompt_input_with_session("uma mensagem comum", "s1"), &c)
            .expect("the gate never errors");
        assert!(
            !matches!(verdict, Verdict::Inject { ref context } if context.contains("in the language they write in")),
            "an uninstalled project gets no rule: {verdict:?}",
        );
        let on_stop = Ctx { trigger: Some(Trigger::Stop), ..c };
        assert_eq!(EndOfTurnCheck.evaluate(&stop, &on_stop).expect("the check never errors"), Verdict::Allow);
    }

    /// Todo projeto com `mustard.json` leva a regra de escrita, declare ou não
    /// o idioma; a antiga chave do tom não a desliga, porque não é mais lida.
    #[test]
    fn the_writing_rule_rides_every_installed_project() {
        for config in ["{}", r#"{"tone":"technical"}"#, PT_PROJECT] {
            let (_dir, verdict) = verdict_for(config, "uma mensagem comum");
            assert!(
                matches!(verdict, Verdict::Inject { ref context } if context.contains("ONE idea per sentence")),
                "{config}: every installed project carries the rule: {verdict:?}",
            );
        }

        // …and a project with NO `mustard.json` at all gets nothing: there
        // the hooks stay silent.
        let none = tempfile::tempdir().expect("temp dir");
        let c = Ctx::for_test(none.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
        let verdict = PromptSubmitInject
            .evaluate(&prompt_input("uma mensagem comum"), &c)
            .expect("the gate never errors");
        assert!(
            matches!(verdict, Verdict::Allow),
            "an uninstalled project declared nothing, so the hooks stay silent: {verdict:?}",
        );
    }

    /// A `/mustard:*` prompt carries it too. That branch drops the
    /// injectables and the banner because a slash command knows its own
    /// context; the writing rule is different in kind, because it governs how
    /// the ANSWER is written and that answer is read by the same person.
    #[test]
    fn the_writing_rule_rides_a_slash_command_too() {
        let dir = project_with(PT_PROJECT);
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
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
        // prompt itself is a `/mustard:*` command, so the pipeline-in-flight banner is
        // suppressed either way. The project is INSTALLED (mustard.json
        // present) so the installation gate stays out of the way.
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let v = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:feature x"), &c)
            .unwrap();
        // Num comando `/mustard:*` nunca há injetável nem aviso, qualquer que
        // seja o estado da spec — só as regras de escrita e de idioma, que todo
        // projeto instalado recebe.
        assert_eq!(v, writing_rules_only(dir.path()), "unexpected verdict: {v:?}");
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
        // No spec branch and no binding in our tempdir, so `current_spec`
        // returns None and the pipeline-in-flight banner stays silent.
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
        // When a spec is active, the user's free-text prompt gets a
        // single-line banner injected.
        let (dir, _) = ctx();
        crate::shared::spec_state::stand_on_spec_branch(dir.path(), "active-feature-xyz");
        let c = Ctx::for_test(dir.path().to_string_lossy().to_string(), Some(Trigger::UserPromptSubmit));
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
        let other = Ctx::for_test(".".to_string(), Some(Trigger::PreToolUse));
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
        // MUSTARD_ACTIVE_SPEC (the pipeline-in-flight banner) — assert on the CONTENT.
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
        assert_eq!(v, writing_rules_only(dir.path()), "slash command must not receive injectables");
        assert!(
            !dir.path()
                .join(".claude/.session/sess-1/injected-orchestrator.md")
                .exists(),
            "no marker burned on a slash-command prompt"
        );
    }

    /// ANY slash command owns its turn, not just Mustard's own.
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
            assert_eq!(v, writing_rules_only(dir.path()), "`{prompt}` must not receive injectables");
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

    /// An application ROUTE named at the start of a work request is not a
    /// command, and must keep the router.
    ///
    /// Route names are word-shaped and cannot be enumerated, so the closed list
    /// of filesystem roots could never separate them. What separates them is
    /// what follows: a command carries arguments, a route is mentioned inside a
    /// sentence. Every row below was measured against the binary in review,
    /// dropping the router — the exact field symptom this unit exists to
    /// remove.
    #[test]
    fn a_route_name_inside_a_sentence_still_gets_the_router() {
        for prompt in [
            "/login nao funciona",
            "/checkout quebrou em producao",
            "/api endpoint retorna 500",
            "/health check falha",
            "/dashboard esta lento",
            "/admin precisa de auth",
            "/settings nao salva",
            "/node_modules deve ser ignorado",
        ] {
            assert!(!is_slash_command(prompt), "must keep the router: {prompt}");
        }

        // …and a real command, with or without arguments, is still a command.
        for prompt in [
            "/mustard:spec",
            "/mustard:spec ar",
            "/mustard:pr merge 212",
            "/grill-me",
            "/review 212",
            "/init",
        ] {
            assert!(is_slash_command(prompt), "must stay a command: {prompt}");
        }
    }

    /// O pior caso da primeira mensagem da sessão cabe numa resposta de
    /// gancho: o arquivo de regras injetado (`orchestrator.md`, o que a
    /// instalação semeia), o aviso de pipeline em curso, a regra de escrita e a de
    /// idioma. Tudo sai numa só resposta, sob um só teto de 10.000 caracteres —
    /// medido na resposta inteira, já em JSON. Os defeitos da resposta
    /// anterior não entram mais aqui: vão no bloqueio do fim da resposta.
    #[test]
    fn prompt_submit_inject_stays_under_hook_ceiling() {
        use crate::hook_output::hook_specific_output;
        use mustard_core::domain::model::contract::Outcome;

        let (dir, c) = ctx();
        let root = dir.path();
        std::fs::write(
            root.join("mustard.json"),
            r#"{"language":{"text":"pt-BR"},"inject":[{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true}]}"#,
        )
        .unwrap();
        let mustard_dir = root.join(".claude").join("mustard");
        std::fs::create_dir_all(&mustard_dir).unwrap();
        std::fs::write(mustard_dir.join("orchestrator.md"), mustard_core::ORCHESTRATOR_MD).unwrap();
        crate::shared::spec_state::stand_on_spec_branch(root, "uma-unidade-com-um-nome-bem-comprido");

        let verdict =
            PromptSubmitInject.evaluate(&prompt_input_with_session("como eu faço X?", "s1"), &c).unwrap();
        let mut outcome = Outcome::allow();
        outcome.fold(verdict);
        let json = hook_specific_output("UserPromptSubmit", &outcome).expect("the prompt speaks");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let context = parsed["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap_or_else(|| panic!("{json}"));

        let rules = mustard_core::ORCHESTRATOR_MD.lines().find(|l| !l.trim().is_empty()).unwrap();
        for (what, needle) in [
            ("the injected rules", rules),
            ("the banner", PIPELINE_IN_FLIGHT_BANNER),
            ("the writing rule", "ONE idea per sentence"),
            ("the language rule", "in the language they write in"),
        ] {
            assert!(context.contains(needle), "{what} missing: {context}");
        }
        let size = json.chars().count();
        assert!(size < 10_000, "{size} characters in one hook response");
    }

    /// O `fold` junta os `Inject` de uma invocação, e no `UserPromptSubmit` e
    /// no `SessionStart` um só `Check` injeta: a regra de escrita chega uma
    /// vez, na ordem de quem a compõe. Morava no `clarity_check`, quando ele
    /// guardava os defeitos para esta mensagem levar.
    #[test]
    fn prompt_and_session_start_have_one_injecting_check() {
        use crate::registry::Registry;
        use mustard_core::domain::model::contract::Outcome;

        let (dir, c) = ctx();
        std::fs::write(dir.path().join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
        let registry = Registry::new();
        let on_prompt = prompt_input_with_session("e agora?", "s1");
        let on_start = HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some("s1".to_string()),
            ..HookInput::default()
        };
        for (name, trigger, input) in [
            ("UserPromptSubmit", Trigger::UserPromptSubmit, &on_prompt),
            ("SessionStart", Trigger::SessionStart, &on_start),
        ] {
            let at = Ctx { trigger: Some(trigger), ..c.clone() };
            let mut outcome = Outcome::allow();
            let mut injecting = Vec::new();
            for module in registry.applicable(trigger, None) {
                let Some(check) = &module.check else { continue };
                let verdict = check.evaluate(input, &at).unwrap_or(Verdict::Allow);
                if matches!(verdict, Verdict::Inject { .. }) {
                    injecting.push(module.id);
                }
                outcome.fold(verdict);
            }
            assert!(injecting.len() <= 1, "{name}: {injecting:?} would share one response");
            if name == "UserPromptSubmit" {
                let Verdict::Inject { context } = &outcome.verdict else {
                    panic!("the prompt carries the rule: {:?}", outcome.verdict);
                };
                assert_eq!(context.matches("ONE idea per sentence").count(), 1, "{context}");
            }
        }
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
