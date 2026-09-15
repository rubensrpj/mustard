//! `mustard-rt run approve-spec` — deterministic spec-approval event sequence.
//!
//! The command now refuses at the door and writes nothing ([`run`]): the user
//! approves a spec by answering the approval question, and the witness records
//! it. What follows describes the old body, kept until the command leaves.
//!
//! Replaces the hand-assembled `emit-pipeline` sequence the legacy approve
//! flow (now `plugin/refs/spec/resume-loop.md § A`) used to make the LLM run
//! by hand (emit `pipeline.stage Plan` then `pipeline.status
//! from:draft,to:approved`; patch the wave-1 `meta.json` for dispatch). The orchestrator now relays a
//! single `mustard-rt run approve-spec` invocation and acts on the JSON report.
//!
//! ## Emitted sequence (in order)
//!
//! 1. `pipeline.stage` → `{"stage":"Plan"}` — records planning complete.
//! 2. `pipeline.status` → `{"from":"draft","to":"approved"}` — the canonical
//!    approval signal.
//! 3. *(only with `--resume`)* `pipeline.stage` → `{"stage":"Execute"}` — the
//!    inline-resume case; without `--resume` the flow STOPS at `approved` so a
//!    fresh session resumes EXECUTE with clean context.
//!
//! With `--wave-plan`, each `pipeline.stage` payload carries `"wave":1` so the
//! existing `emit_pipeline` machinery patches the wave-1 `meta.json` sidecar
//! for dispatch (it resolves `wave-1-*` and runs the canonical
//! `Meta` read-modify-write — no parallel writer here).
//!
//! ## Reuse, not duplication
//!
//! The event sequence is defined once by [`approval_sequence`]. The CLI entry
//! [`run`] feeds each step to [`crate::commands::event::emit_pipeline::run`]
//! (module-qualified — exactly the precedent set by
//! [`crate::hooks::observe::wave_complete_observer`]); no subprocess, no
//! duplicated NDJSON-writing logic, no facade. The cwd-aware
//! [`emit_via_route`] used in tests routes the same events through
//! [`crate::shared::events::route::emit`] — the identical write path
//! `emit_pipeline::run` ends in — so the tests assert the real on-disk
//! `.events/` log without mutating the process working directory.
//!
//! ## Fail-open contract
//!
//! Every emit is best-effort (the underlying `emit_pipeline::run` / `route::emit`
//! swallow store/IO errors). The command never panics on a DB/IO error; it
//! prints a JSON report `{"ok":true,"spec":"<name>","approved":true,
//! "resumed":<bool>}` on success (mirroring the report style of
//! `tactical-fix-create`), or `{"ok":false,"error":"..."}` on a real failure
//! (an empty spec name).

use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;


/// Options for `mustard-rt run approve-spec`.
#[derive(Debug, Clone)]
pub struct ApproveSpecOpts {
    /// Spec slug under `.claude/spec/` whose approval to emit.
    pub spec: String,
    /// The spec is a wave plan — tag each `pipeline.stage` with `wave:1` so the
    /// wave-1 `meta.json` sidecar is patched for dispatch.
    pub wave_plan: bool,
    /// Inline-resume: also emit `pipeline.stage Execute` so the same session
    /// can jump straight into EXECUTE (the `r`-suffix branch of the flow).
    pub resume: bool,
}

/// JSON success report. Mirrors the `tactical-fix-create` style (flat, typed).
///
/// `witness` / `witnessAt` echo the approval the spec's state records — the
/// question, the option the user chose, and when the witness recorded it. The
/// approval may have happened in an EARLIER session, so the keys name the
/// witness, never this run. Both are omitted (not null) when nothing reads
/// back: the approved state is what governed the gate above, so a missing
/// echo degrades the report to silence, never to a failure.
#[derive(Debug, Serialize)]
struct ApproveReport {
    ok: bool,
    spec: String,
    approved: bool,
    resumed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    witness: Option<Value>,
    #[serde(rename = "witnessAt", skip_serializing_if = "Option::is_none")]
    witness_at: Option<String>,
}

/// `true` when the spec is NOT approved yet: its state, in `spec.ndjson`, is
/// not in an approved phase. The one reader every door shares
/// ([`crate::shared::spec_state::approved`]).
pub(crate) fn approval_missing(root: &str, spec: &str) -> bool {
    !crate::shared::spec_state::approved(Path::new(root), spec)
}

/// JSON failure report.
#[derive(Debug, Serialize)]
struct ApproveError {
    ok: bool,
    error: String,
}

/// One step of the approval sequence: an `emit-pipeline` kind + its JSON payload.
type Step = (&'static str, Value);

/// Build the ordered approval event sequence for the given options.
///
/// This is the single source of truth for *what* approve-spec emits; both the
/// CLI entry (via `emit_pipeline::run`) and the tests (via `route::emit`)
/// consume it, so there is exactly one definition of the order + payloads.
///
/// - `pipeline.stage {stage:"Plan"}` — planning complete.
/// - `pipeline.status {from:"draft",to:"approved"}` — the approval signal.
/// - `pipeline.stage {stage:"Execute"}` — only when `resume` (inline EXECUTE).
///
/// When `wave_plan`, the two `pipeline.stage` steps additionally carry
/// `"wave":1` so `emit_pipeline` syncs the wave-1 sidecar instead of the parent.
fn approval_sequence(wave_plan: bool, resume: bool) -> Vec<Step> {
    let stage_payload = |stage: &str| -> Value {
        if wave_plan {
            json!({ "stage": stage, "wave": 1 })
        } else {
            json!({ "stage": stage })
        }
    };

    let mut steps: Vec<Step> = vec![
        ("pipeline.stage", stage_payload("Plan")),
        (
            "pipeline.status",
            json!({ "from": "draft", "to": "approved" }),
        ),
    ];
    if resume {
        steps.push(("pipeline.stage", stage_payload("Execute")));
    }
    steps
}

// ---------------------------------------------------------------------------
// The approval gate — the user's approval.
//
// `approve-spec` may emit the `draft→approved` signal ONLY once BOTH preconditions
// hold: the user has APPROVED the plan
// (the spec's state in `spec.ndjson` is approved). Each is born from an act the
// model cannot author — the user's own
// choice of "Aprovar" in the approval question, echoed by the harness in
// `tool_response` and recorded by the approval witness. A gate the gated could open
// by running this very command is not a gate.
//
// The two preconditions are checked in ONE pass so a single refusal names EVERY
// unmet one with its remedy — the pre-refactor gate exited on the first miss and
// hid the second, costing the user a second failed run to discover it. Mode reads
// exactly like the `MUSTARD_*_GATE_MODE` close-gate family.
// ---------------------------------------------------------------------------

/// Three-state mode for the user-approval requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalMode {
    /// Emit approval unconditionally (the behaviour before the user-approval
    /// gate).
    Off,
    /// Warn on a missing approval but proceed.
    Warn,
    /// Refuse (exit≠0) on a missing approval — the default.
    Strict,
}

/// Map a mode string to [`ApprovalMode`]; an absent/unknown value is `strict`
/// (the safe default — an approval must be proven, never assumed).
fn parse_approval_mode(s: &str) -> ApprovalMode {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" => ApprovalMode::Off,
        "warn" => ApprovalMode::Warn,
        _ => ApprovalMode::Strict,
    }
}

/// Resolve `MUSTARD_APPROVAL_MODE` (default `strict`), mirroring the cascade the
/// close-gate family uses for `MUSTARD_QA_GATE_MODE` / `MUSTARD_COMMIT_GATE_MODE`
/// (`resolve_mode`): a non-empty env value wins; absent or blank → `strict`.
fn resolve_approval_mode() -> ApprovalMode {
    std::env::var("MUSTARD_APPROVAL_MODE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map_or(ApprovalMode::Strict, |v| parse_approval_mode(&v))
}

/// The gate outcome for a resolved mode + marker presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalGate {
    /// Emit the approval sequence as normal.
    Proceed,
    /// Emit, but first warn that no user approval was recorded.
    Warn,
    /// Refuse — no user approval recorded and the mode is strict.
    Block,
}

/// Decide the gate outcome. Pure: the env read and the marker existence check are
/// resolved by the caller, so the whole policy is unit-testable without touching
/// process-global state (env / cwd).
fn approval_gate(mode: ApprovalMode, marker_present: bool) -> ApprovalGate {
    match mode {
        ApprovalMode::Off => ApprovalGate::Proceed,
        _ if marker_present => ApprovalGate::Proceed,
        ApprovalMode::Warn => ApprovalGate::Warn,
        ApprovalMode::Strict => ApprovalGate::Block,
    }
}

/// Narrative sections of `spec.md` still holding the seeded placeholder, byte
/// for byte.
///
/// **The oracle is exact, not a judgement.** It compares each section's body
/// against the very string `spec-draft` seeds for it (`placeholder.fill_*` in
/// the i18n catalogue, in the spec's own language) — so it answers "was this
/// authored?", never "is this good?". A one-word section passes; only untouched
/// scaffold fails. Measuring writing quality is not this gate's business and
/// pretending otherwise would make it unpredictable.
///
/// Why it refuses at all: nothing in the pipeline read the narrative. A Full
/// spec could reach approval with `Por que agora.` under `## Contexto`, and the
/// waves inherit it — `agent-prompt-render` builds each wave's TASK out of this
/// file, so a hollow spec produces hollow prompts for every agent after it.
/// Reported from the field, and then reproduced by this very unit: a
/// `spec-draft --force` re-drafted the body from scratch, restoring the
/// placeholders, and nothing objected.
///
/// Returns the localised HEADINGS, so the refusal names what the reader sees.
/// Fail-open: an unreadable spec yields an empty list — an unreadable file is
/// the read gate's problem, not this one's.
fn scaffold_residue(root: &Path, spec: &str) -> Vec<String> {
    let Ok(body) = mustard_core::io::fs::read_to_string(
        root.join(".claude").join("spec").join(spec).join("spec.md"),
    ) else {
        return Vec::new();
    };
    // The project's text language is the spec's: `spec-draft` writes the body
    // in `language.text` and takes no other. When a spec could carry a
    // language of its own, reading the project's looked for `## Contexto` in a
    // file written with `## Context`, found no section at all, and reported
    // zero residue — so an untouched scaffold passed this gate whenever the two
    // languages differed. With one language there is no second one to differ.
    let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
    // Each section the draft seeds with a placeholder, by the heading key that
    // titles it and the placeholder key that fills it. `context` is the one
    // composite: the draft writes `{intent}.\n\n{fill_why_now}`, so the trailing
    // placeholder line is what marks it unauthored — the title alone is not
    // narrative.
    const SEEDED: &[(&str, &str)] = &[
        ("heading.spec.users", "placeholder.fill_beneficiary"),
        ("heading.spec.metric", "placeholder.fill_metric"),
        ("heading.spec.non_goals", "placeholder.fill_excluded"),
        ("heading.spec.files", "placeholder.fill_files"),
    ];
    let mut residue: Vec<String> = SEEDED
        .iter()
        .filter_map(|(heading_key, placeholder_key)| {
            let heading = mustard_core::translate(heading_key, lang);
            let placeholder = mustard_core::translate(placeholder_key, lang);
            let section = section_body(&body, heading)?;
            // Untouched when the placeholder is the ONLY prose under the
            // heading. A section the author added to still passes.
            (section == placeholder.trim()).then(|| heading.to_string())
        })
        .collect();
    let context_heading = mustard_core::translate("heading.spec.context", lang);
    let why_now = mustard_core::translate("placeholder.fill_why_now", lang);
    if let Some(section) = section_body(&body, context_heading) {
        // The seeded shape is the title echoed back plus the placeholder. An
        // author who wrote the section replaces that line; one who only kept
        // the echo has still explained nothing.
        if section.ends_with(why_now.trim()) {
            residue.push(context_heading.to_string());
        }
    }
    residue.sort();
    residue
}

/// The body between `## <heading>` and the next `##`, or `None` when the
/// heading is absent.
///
/// The heading must be a WHOLE LINE, not a substring. An unanchored search
/// matched `## Contexto` inside `## Contexto e Motivação`, and matched a
/// Decisions bullet that merely quotes `` `## Arquivos` `` before reaching the
/// real section — either way the extracted text is not the section, the
/// placeholder comparison fails, and a spec that is pure scaffold sails through
/// the gate (found in review). The same anchoring the render's section cutter
/// uses, for the same reason.
fn section_body(body: &str, heading: &str) -> Option<String> {
    let wanted = format!("## {heading}");
    let mut lines = body.lines();
    lines.by_ref().position(|l| l.trim_end() == wanted)?;
    let section: Vec<&str> = lines.take_while(|l| !l.starts_with("## ")).collect();
    Some(section.join("\n").trim().to_string())
}

/// Build the aggregated refusal that names EVERY unmet approval precondition at
/// once — each with the path that satisfies it. One message so the
/// user sees everything missing in a single run, instead of the pre-refactor
/// gate's first-miss-only refusal. Returns `None` when nothing is missing — the
/// caller then proceeds silently. Surfaced as the report `error`; the flow
/// relays `{ok:false,error}` straight to the user.
fn unmet_gate_message(
    spec: &str,
    approval_missing: bool,
    scaffold_sections: &[String],
    is_full: bool,
    open_points: Option<&str>,
) -> Option<String> {
    let mut unmet: Vec<String> = Vec::new();
    // O levantamento vem antes de tudo: com ponto aberto, a spec nem chegou
    // ao plano.
    if let Some(line) = open_points {
        unmet.push(format!("survey — {line}"));
    }
    if !scaffold_sections.is_empty() {
        // The residue gate runs on EVERY scope — the PRD sections are seeded
        // for Light and Full alike — so the remedy has to match the spec it is
        // refusing. `plan-materialize` is the Full door: it requires a
        // `--plan <plan.json>` a Light spec never has, and it emits
        // `pipeline.scope full`, so following it on a Light spec would both
        // fail and, if satisfied, silently change the spec's scope. Review
        // measured the wrong advice being handed to a Light spec.
        let remedy = if is_full {
            format!(
                "Author each one, then re-materialise with `mustard-rt run plan-materialize \
                 --spec-dir .claude/spec/{spec} --plan <plan.json>`"
            )
        } else {
            "Author each one directly in `spec.md` — a light spec carries no plan document to \
             re-materialise from"
                .to_string()
        };
        unmet.push(format!(
            "narrative — {} section(s) still hold the seeded placeholder, byte for byte: {}. \
             The spec would be approved describing nothing, and the waves inherit that: the \
             per-wave prompt is built from what this file says. {remedy}",
            scaffold_sections.len(),
            scaffold_sections.join(", "),
        ));
    }
    if approval_missing {
        unmet.push(
            "approval — the spec is not approved: ONE gesture approves it, and the model \
             cannot forge it — the user CHOOSES the option \"Aprovar\" (\"Approve\") of the \
             approval question, and the approval witness records the approved state in \
             `spec.ndjson`. Free text typed instead of choosing an option approves nothing, \
             and so does an option that does not start with \"Aprovar\""
                .to_string(),
        );
    }
    if unmet.is_empty() {
        return None;
    }
    let list = unmet
        .iter()
        .map(|u| format!("- {u}"))
        .collect::<Vec<_>>()
        .join("\n");
    // The relax hint is only honest when every unmet precondition is one the
    // mode actually governs. The open survey points are unconditional, so when
    // they are the (or a) blocker the message says so instead of pointing at a
    // switch that will not move them.
    let tail = if open_points.is_some() {
        "The open survey points are UNCONDITIONAL — MUSTARD_APPROVAL_MODE does not relax them: close \
         each one first."
    } else {
        "To temporarily relax, set MUSTARD_APPROVAL_MODE=warn or off."
    };
    Some(format!(
        "approve-spec will not self-approve a Full plan (that is what the field incident \
         did). Unmet precondition(s):\n{list}\n{tail}"
    ))
}

/// `true` when `spec`'s `meta.json#scope` declares a Full-scope spec (starts with
/// `full` after a case-insensitive trim — `"full"` or `"full (wave plan)"`). Only
/// a Full plan carries the proof gate; Light / task specs are never gated.
/// Fail-open: an unreadable `meta.json` / absent scope returns `false` (not
/// gated), the safe direction — the user-approval gate still applies to all.
fn spec_is_full(root: &str, spec: &str) -> bool {
    let Some(sp) = mustard_core::ClaudePaths::for_project(std::path::Path::new(root))
        .and_then(|p| p.for_spec(spec))
        .ok()
    else {
        return false;
    };
    mustard_core::read_meta(&sp.meta_json_path())
        .and_then(|m| m.scope)
        .map(|s| s.trim().to_ascii_lowercase().starts_with("full"))
        .unwrap_or(false)
}

/// Why no approval happened — the `error` to print, and whether the process must
/// exit non-zero.
///
/// A gate refusal exits 1 (a caller that ignores stdout still sees the failure);
/// an argument error (empty spec) stays exit 0, as it always has.
#[derive(Debug)]
struct Refused {
    error: String,
    exit_nonzero: bool,
}

/// The approval preconditions of `approve-spec` besides the user's own
/// gesture — the recorded proof of the criteria and the authored narrative.
/// With `check_approval`, the user's approved state joins them.
///
/// `None` when nothing is unmet; otherwise the aggregated message and what
/// the mode makes of it. The acceptance-criteria proof is evaluated OUTSIDE
/// the mode branch, on purpose: it is unconditional, and an unmet proof always
/// Blocks. `MUSTARD_APPROVAL_MODE` governs the approval precondition and
/// nothing else — strict Blocks, warn Warns, off mutes it.
///
/// Os pontos do levantamento ainda abertos barram sempre, como a prova dos
/// critérios: a variável do modo não desliga a trava. A lista é a mesma da
/// passagem para o plano (`survey::open_points`).
fn preconditions(
    root: &str,
    spec: &str,
    mode: ApprovalMode,
    check_approval: bool,
) -> Option<(String, ApprovalGate)> {
    let open = open_points_line(root, spec);
    let missing_approval =
        mode != ApprovalMode::Off && check_approval && approval_missing(root, spec);
    let scaffold = scaffold_residue(Path::new(root), spec);
    let message = unmet_gate_message(
        spec,
        missing_approval,
        &scaffold,
        spec_is_full(root, spec),
        open.as_deref(),
    )?;
    let gate = if open.is_some() { ApprovalGate::Block } else { approval_gate(mode, false) };
    Some((message, gate))
}

/// Os pontos do levantamento ainda abertos na spec `spec`, lidos do arquivo
/// de eventos do checkout principal, numa linha no idioma do projeto. `None`
/// sem ponto aberto, e numa spec sem arquivo de eventos.
fn open_points_line(root: &str, spec: &str) -> Option<String> {
    use mustard_core::domain::survey;
    use mustard_core::io::spec_events as store;
    let home = store::spec_root(Path::new(root));
    let log = store::spec_file(&home, spec).ok().and_then(|path| store::read(&path).ok().flatten())?;
    let open = survey::open_points(&log);
    if open.is_empty() {
        return None;
    }
    let lang = mustard_core::ProjectConfig::load(&home).language().text_or_default();
    Some(
        mustard_core::platform::i18n::translate("approve_spec.open_points", lang)
            .replace("{count}", &open.len().to_string())
            .replace("{points}", &survey::describe(&log, &open)),
    )
}

/// Decide the approval against `root` and, only if it passes, feed every event
/// of the sequence to `emit`.
///
/// The seam that makes the gate testable: on a refusal `emit` is NEVER called,
/// which is the entire point of a gate, and a test can assert exactly that by
/// passing a recorder. `run` passes the canonical
/// [`crate::commands::event::emit_pipeline::run`].
///
/// Approval gate — refuse (strict) to emit the approval signal until the
/// precondition holds: the user's own approval, born from an act the model
/// cannot forge (the witness recording the user's real choice of "Aprovar" in
/// the approval question). A background job (no user, no answer, no approved state) halts
/// cleanly here and the spec stays in PLAN instead of auto-approving. A THIRD
/// precondition joins them, unconditionally: every non-exempt acceptance
/// criterion must carry a PROVEN record in `<spec>/ac-proof.json` for the
/// command it carries today (see [`proof_state`] — fail-CLOSED). All three are
/// evaluated TOGETHER so one refusal names every unmet precondition with its
/// remedy.
fn approve_at(
    root: &str,
    opts: &ApproveSpecOpts,
    mode: ApprovalMode,
    emit: &mut dyn FnMut(&str, Value),
) -> Result<ApproveReport, Refused> {
    if opts.spec.trim().is_empty() {
        return Err(Refused {
            error: "empty spec name".to_string(),
            exit_nonzero: false,
        });
    }

    // A single decision over all the preconditions: any unmet one yields the
    // aggregated message — never a second refusal path. Nothing unmet → the
    // flow proceeds silently.
    if let Some((message, gate)) = preconditions(root, &opts.spec, mode, true) {
        match gate {
            ApprovalGate::Block => {
                return Err(Refused {
                    error: message,
                    exit_nonzero: true,
                })
            }
            ApprovalGate::Warn => eprintln!("{message}"),
            ApprovalGate::Proceed => {}
        }
    }

    for (kind, payload) in approval_sequence(opts.wave_plan, opts.resume) {
        emit(kind, payload);
    }

    // Echo the approval the state records — named for the witness, never for
    // this run. Read AFTER the gate, and independent of it: nothing read back
    // yields `None` here but never changes `approved`, which the gate above
    // already settled.
    let witness = crate::shared::spec_state::approval(Path::new(root), &opts.spec);
    Ok(ApproveReport {
        ok: true,
        spec: opts.spec.clone(),
        approved: true,
        resumed: opts.resume,
        witness: witness.as_ref().map(|a| json!({ "question": a.question, "answer": a.answer })),
        witness_at: witness.map(|a| a.at),
    })
}

/// CLI entry — `mustard-rt run approve-spec`. The command refuses at the door
/// with exit 1 and writes nothing: a spec is approved by the user's answer to
/// the approval question, which the witness records. [`run_old`] keeps the
/// old body, with its tests, until the command leaves.
pub fn run(_opts: ApproveSpecOpts) {
    crate::commands::retired::refuse(
        Path::new(&crate::shared::context::cwd()),
        "approve-by-question",
        "retired.approve_spec",
        &[],
    );
}

/// The door's old body, kept until the command leaves.
///
/// Delegates the decision + emission to [`approve_at`] against the process cwd,
/// emitting each step through the canonical
/// [`crate::commands::event::emit_pipeline::run`] (process-global cwd, like
/// `wave_complete_observer`) — no subprocess, no duplicated NDJSON logic, no
/// facade. Prints the JSON report to stdout; a gate refusal exits 1 (a gate the
/// gated cannot open by running this very command), an empty spec name exits 0.
// A porta recusa, e nada mais chama este corpo: ele espera o comando sair.
#[allow(dead_code)]
fn run_old(opts: ApproveSpecOpts) {
    let root = crate::shared::context::cwd();
    let spec = opts.spec.clone();
    let mut emit = |kind: &str, payload: Value| {
        crate::commands::event::emit_pipeline::run(
            crate::commands::event::emit_pipeline::EmitPipelineOpts {
                kind: kind.to_string(),
                spec: spec.clone(),
                payload: Some(payload.to_string()),
                allow_no_qa: false,
                intent: None,
                unit_name: None,
                base: None,
                work_kind: None,
                pending: None,
            },
        );
    };

    match approve_at(&root, &opts, resolve_approval_mode(), &mut emit) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string(&report).unwrap_or_else(|_| "{\"ok\":true}".to_string())
        ),
        Err(refused) => {
            let err = ApproveError {
                ok: false,
                error: refused.error,
            };
            println!(
                "{}",
                serde_json::to_string(&err).unwrap_or_else(|_| "{\"ok\":false}".to_string())
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
            if refused.exit_nonzero {
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use std::path::Path;
    use tempfile::tempdir;

    /// Route one approval step through the event-router against an explicit
    /// project root — the identical write path `emit_pipeline::run` ends in,
    /// minus the process-global cwd. Lets the tests assert the on-disk
    /// `.events/` log without `set_current_dir`.
    fn emit_via_route(project: &Path, spec: &str, kind: &str, payload: Value) {
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-06-02T00:00:00.000Z".to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("approve-spec".to_string()),
                actor_type: None,
            },
            event: kind.to_string(),
            payload,
            spec: Some(spec.to_string()),
        };
        crate::shared::events::route::emit(project.to_str().unwrap(), &event);
    }

    /// Drive the full approval sequence (as `run` would) against a tempdir,
    /// returning the chronologically-sorted `(event, payload)` pairs read back
    /// from the spec's `.events/` log.
    fn emit_sequence_and_read(
        project: &Path,
        spec: &str,
        wave_plan: bool,
        resume: bool,
    ) -> Vec<(String, Value)> {
        for (kind, payload) in approval_sequence(wave_plan, resume) {
            emit_via_route(project, spec, kind, payload);
        }
        let events_dir = project
            .join(".claude")
            .join("spec")
            .join(spec)
            .join(".events");
        let mut events =
            mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
        events.sort_by(|a, b| a.ts.cmp(&b.ts));
        // Keep only the first-class approval kinds the sequence emits — drop the
        // per-write economy breadcrumbs and any alias fan-out (which would carry
        // `legacy_alias`). We assert the explicit emitted set, in order.
        events
            .into_iter()
            .filter(|e| matches!(e.event.as_str(), "pipeline.stage" | "pipeline.status"))
            .filter(|e| !e.payload.get("legacy_alias").is_some_and(|v| v == &json!(true)))
            .map(|e| (e.event, e.payload))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Sequence shape (unit — no I/O)
    // -----------------------------------------------------------------------

    /// A spec whose narrative is still the seeded placeholder is
    /// refused, and the refusal names the sections.
    ///
    /// The oracle is exact: it compares each section against the very string
    /// the draft seeds. Authoring one word clears it — this gate answers "was
    /// this written?", never "is this good?".
    #[test]
    fn approval_refuses_scaffold_residue() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"version":"1.0.0","language":{"text":"pt-BR"}}"#)
            .unwrap();
        let spec_dir = root.join(".claude/spec/uma-unidade");
        std::fs::create_dir_all(&spec_dir).unwrap();

        // Straight out of the draft: every PRD section is its placeholder.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# t\n\n## Contexto\n\nfazer algo.\n\nPor que agora.\n\n\
             ## Usuários/Stakeholders\n\nQuem se beneficia.\n\n\
             ## Métrica de sucesso\n\nMétrica de sucesso.\n\n\
             ## Não-Objetivos\n\nO que fica de fora.\n\n## Arquivos\n\nListar arquivos afetados.\n",
        )
        .unwrap();

        let residue = scaffold_residue(root, "uma-unidade");
        assert_eq!(residue.len(), 5, "every seeded section is residue: {residue:?}");
        assert!(residue.iter().any(|s| s == "Contexto"), "{residue:?}");

        let msg = unmet_gate_message(
            "uma-unidade",
            false,
            &residue,
            true,
            None,
        )
        .expect("scaffold residue must refuse");
        assert!(msg.contains("Contexto"), "the refusal must name the sections: {msg}");
        assert!(msg.contains("plan-materialize"), "the refusal must name its remedy: {msg}");

        // …and on a LIGHT spec the same refusal names a remedy that spec can
        // actually follow. `plan-materialize` requires a `--plan <plan.json>` a
        // light spec never has, and it emits `pipeline.scope full` — so the
        // Full-only advice both fails and, if satisfied, changes the scope
        // (measured in review, handed to a real light spec).
        let light = unmet_gate_message(
            "uma-unidade",
            false,
            &residue,
            false,
            None,
        )
        .expect("scaffold residue must refuse a light spec too");
        assert!(light.contains("Contexto"), "it must still name the sections: {light}");
        assert!(
            !light.contains("plan-materialize"),
            "a light spec must not be sent to the Full-only door: {light}",
        );
        assert!(
            light.contains("spec.md"),
            "it must say where to author instead: {light}",
        );

        // Authored: the sections are replaced, and the gate goes quiet.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# t\n\n## Contexto\n\nO roteador nao alcanca a janela.\n\n\
             ## Usuários/Stakeholders\n\nO operador.\n\n\
             ## Métrica de sucesso\n\nNenhum caminho sem roteador.\n\n\
             ## Não-Objetivos\n\nMedir qualidade de texto.\n\n## Arquivos\n\n- `a.rs`\n",
        )
        .unwrap();
        assert!(
            scaffold_residue(root, "uma-unidade").is_empty(),
            "an authored spec must not be refused",
        );

        // Fail-open: an unreadable spec is the read gate's problem, not this one's.
        assert!(scaffold_residue(root, "nao-existe").is_empty());

        // A heading is a WHOLE LINE. `## Contexto e Motivação` is not
        // `## Contexto`, and a bullet quoting a heading is not that heading —
        // an unanchored search matched both and read the wrong text as the
        // section, so a pure-scaffold spec passed the gate.
        std::fs::write(
            spec_dir.join("spec.md"),
            concat!(
                "# t\n",
                "\n",
                "## Contexto e Motivação\n",
                "\n",
                "Por que agora.\n",
                "\n",
                "## Decisions\n",
                "\n",
                "- a secao `## Arquivos` recebe a lista\n",
                "\n",
                "## Arquivos\n",
                "\n",
                "Listar arquivos afetados.\n",
            ),
        )
        .unwrap();
        let residue = scaffold_residue(root, "uma-unidade");
        assert!(
            residue.iter().any(|s| s == "Arquivos"),
            "the real `## Arquivos` section is still placeholder and must be caught, \
             not shadowed by the bullet that quotes its name: {residue:?}",
        );
        // The project's text language decides the headings, because the spec
        // is written in it: an English project's all-scaffold spec is caught
        // under its English headings.
        let en_project = tempfile::tempdir().unwrap();
        std::fs::write(en_project.path().join("mustard.json"), r#"{"language":{"text":"en-US"}}"#)
            .unwrap();
        let en = en_project.path().join(".claude/spec/em-ingles");
        std::fs::create_dir_all(&en).unwrap();
        std::fs::write(
            en.join("spec.md"),
            concat!(
                "# t\n\n## Context\n\ndo something.\n\nfill in why now.\n",
                "\n## Users/Stakeholders\n\nfill in who benefits.\n",
                "\n## Success Metric\n\nfill in the success metric.\n",
                "\n## Non-Goals\n\nfill in what stays out.\n",
                "\n## Files\n\nfill in affected files.\n",
            ),
        )
        .unwrap();
        let en_residue = scaffold_residue(en_project.path(), "em-ingles");
        assert_eq!(
            en_residue.len(),
            5,
            "an all-scaffold spec of an English project must be caught: {en_residue:?}",
        );

        assert!(
            !residue.iter().any(|s| s == "Contexto"),
            "`## Contexto e Motivação` is a different heading and has no placeholder \
             to report: {residue:?}",
        );
    }

    #[test]
    fn approval_sequence_default_stops_at_approved() {
        let steps = approval_sequence(false, false);
        let kinds: Vec<&str> = steps.iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, vec!["pipeline.stage", "pipeline.status"]);
        assert_eq!(steps[0].1, json!({ "stage": "Plan" }));
        assert_eq!(steps[1].1, json!({ "from": "draft", "to": "approved" }));
    }

    #[test]
    fn approval_sequence_resume_appends_execute_stage() {
        let steps = approval_sequence(false, true);
        let kinds: Vec<&str> = steps.iter().map(|(k, _)| *k).collect();
        // Resume adds a trailing pipeline.stage Execute after approval.
        assert_eq!(
            kinds,
            vec!["pipeline.stage", "pipeline.status", "pipeline.stage"]
        );
        assert_eq!(steps[2].1, json!({ "stage": "Execute" }));
    }

    #[test]
    fn approval_sequence_wave_plan_tags_stage_with_wave_one() {
        let steps = approval_sequence(true, true);
        // Both pipeline.stage steps carry wave:1 so the wave-1 sidecar is
        // patched; the status step never carries a wave.
        assert_eq!(steps[0].1, json!({ "stage": "Plan", "wave": 1 }));
        assert_eq!(steps[2].1, json!({ "stage": "Execute", "wave": 1 }));
        assert!(steps[1].1.get("wave").is_none());
    }

    // -----------------------------------------------------------------------
    // Event order lands in the spec's `.events/` log (integration via route)
    // -----------------------------------------------------------------------

    #[test]
    fn fresh_session_emits_plan_then_approved_in_order() {
        let dir = tempdir().unwrap();
        let got = emit_sequence_and_read(dir.path(), "demo-approve", false, false);
        assert_eq!(
            got,
            vec![
                ("pipeline.stage".to_string(), json!({ "stage": "Plan" })),
                (
                    "pipeline.status".to_string(),
                    json!({ "from": "draft", "to": "approved" })
                ),
            ]
        );
    }

    #[test]
    fn resume_branch_emits_execute_stage_after_approval() {
        let dir = tempdir().unwrap();
        let got = emit_sequence_and_read(dir.path(), "demo-resume", false, true);
        let kinds: Vec<&str> = got.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            kinds,
            vec!["pipeline.stage", "pipeline.status", "pipeline.stage"]
        );
        // The last emitted event is the Execute stage transition.
        assert_eq!(got[2].1, json!({ "stage": "Execute" }));
    }

    // -----------------------------------------------------------------------
    // --wave-plan: emit_pipeline patches the wave-1 meta.json sidecar
    // -----------------------------------------------------------------------

    /// Seed a wave-plan spec dir with a `wave-1-general/meta.json` sidecar.
    fn seed_wave_plan(root: &Path, spec: &str) -> std::path::PathBuf {
        let wave_dir = root
            .join(".claude")
            .join("spec")
            .join(spec)
            .join("wave-1-general");
        std::fs::create_dir_all(&wave_dir).unwrap();
        let meta_path = wave_dir.join("meta.json");
        std::fs::write(
            &meta_path,
            br#"{"stage":"Draft","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR","checkpoint":null}"#,
        )
        .unwrap();
        meta_path
    }

    /// A `pipeline.stage {stage:"Plan","wave":1}` event patches the wave-1
    /// `meta.json` (the dispatch-readiness patch the ref step 4 did), reusing
    /// the canonical `emit_pipeline::patch_meta_for_transition` via the
    /// wave-aware payload path. Asserted by driving that helper directly with a
    /// wave payload — the same call `route`/`run` make after writing the event.
    #[test]
    fn wave_plan_stage_patches_wave_one_meta() {
        let dir = tempdir().unwrap();
        let meta_path = seed_wave_plan(dir.path(), "demo-wave");

        // The approval sequence tags the Plan stage with wave:1 under --wave-plan.
        let steps = approval_sequence(true, false);
        let (_kind, plan_payload) = &steps[0];
        assert_eq!(plan_payload, &json!({ "stage": "Plan", "wave": 1 }));

        // emit_pipeline resolves `wave-1-*` from a wave-tagged payload and runs
        // the canonical Meta read-modify-write. Exercise that exact path (the
        // same routine `run()` calls after writing the pipeline.stage event).
        crate::commands::event::emit_pipeline::patch_meta_for_transition(
            dir.path(),
            "demo-wave",
            "pipeline.stage",
            plan_payload,
            "2026-06-02T10:00:00Z",
        );

        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        // Wave-1 sidecar advanced to Plan/PLAN; other fields preserved.
        assert_eq!(v["stage"], json!("Plan"), "{v}");
        assert_eq!(v["phase"], json!("PLAN"), "{v}");
        assert_eq!(v["scope"], json!("full"), "{v}");
        assert_eq!(v["checkpoint"], json!("2026-06-02T10:00:00Z"), "{v}");
    }

    // -----------------------------------------------------------------------
    // The approval gate
    // -----------------------------------------------------------------------

    #[test]
    fn parse_approval_mode_maps_values() {
        assert_eq!(parse_approval_mode("off"), ApprovalMode::Off);
        assert_eq!(parse_approval_mode("warn"), ApprovalMode::Warn);
        assert_eq!(parse_approval_mode("strict"), ApprovalMode::Strict);
        assert_eq!(parse_approval_mode("STRICT"), ApprovalMode::Strict);
        assert_eq!(parse_approval_mode("  warn "), ApprovalMode::Warn);
        // Unknown / empty → strict (the safe default: prove approval, don't assume).
        assert_eq!(parse_approval_mode(""), ApprovalMode::Strict);
        assert_eq!(parse_approval_mode("banana"), ApprovalMode::Strict);
    }

    #[test]
    fn approval_gate_blocks_strict_without_marker_and_proceeds_with_it() {
        // O centro do teste: SEM marcador → strict FALHA (Block ⇒ exit≠0); COM marcador → procede.
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
        assert_eq!(approval_gate(ApprovalMode::Strict, true), ApprovalGate::Proceed);
        // Warn surfaces a nudge but never blocks; off restores the behaviour
        // before the approval gate.
        assert_eq!(approval_gate(ApprovalMode::Warn, false), ApprovalGate::Warn);
        assert_eq!(approval_gate(ApprovalMode::Warn, true), ApprovalGate::Proceed);
        assert_eq!(approval_gate(ApprovalMode::Off, false), ApprovalGate::Proceed);
        assert_eq!(approval_gate(ApprovalMode::Off, true), ApprovalGate::Proceed);
    }

    #[test]
    fn background_job_without_user_stops_at_plan() {
        // A background job poses no approval question, so the witness records
        // no approval; strict `approve-spec` then refuses (Block), and the Full
        // spec cannot leave PLAN without a human.
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    /// The state `approve-spec` gates on is the one the witness writes: the
    /// approved state in `spec.ndjson`. Recording it flips the decision.
    #[test]
    fn the_approved_state_toggles_the_decision() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        let spec_dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();

        assert!(approval_missing(root_str, spec), "no state yet");
        assert_eq!(approval_gate(ApprovalMode::Strict, !approval_missing(root_str, spec)), ApprovalGate::Block);

        crate::shared::spec_state::approve_in(&spec_dir);
        assert!(!approval_missing(root_str, spec), "approved once the witness records it");
        assert_eq!(approval_gate(ApprovalMode::Strict, !approval_missing(root_str, spec)), ApprovalGate::Proceed);
    }

    // -----------------------------------------------------------------------
    // Escopo da spec
    // -----------------------------------------------------------------------

    #[test]
    fn spec_is_full_reads_meta_scope() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        for (spec, scope, want) in [
            ("epic", "full (wave plan)", true),
            ("epic2", "full", true),
            ("small", "light", false),
        ] {
            let sp = mustard_core::ClaudePaths::for_project(root)
                .unwrap()
                .for_spec(spec)
                .unwrap();
            std::fs::create_dir_all(sp.dir()).unwrap();
            std::fs::write(
                sp.meta_json_path(),
                format!(r#"{{"scope":"{scope}","stage":"Plan","outcome":"Active"}}"#),
            )
            .unwrap();
            assert_eq!(spec_is_full(root_str, spec), want, "scope={scope}");
        }
        // Sem meta.json: não é full (falha aberta).
        assert!(!spec_is_full(root_str, "ghost"));
    }

    /// Seed a Full spec dir + `meta.json` so `spec_is_full` says yes, and return
    /// the project root string.
    fn seed_full_spec(root: &Path, spec: &str) {
        let sp = mustard_core::ClaudePaths::for_project(root)
            .unwrap()
            .for_spec(spec)
            .unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.meta_json_path(),
            r#"{"scope":"full (wave plan)","stage":"Plan","outcome":"Active"}"#,
        )
        .unwrap();
    }

    // -----------------------------------------------------------------------
    // O portão: uma recusa só nomeia tudo o que falta
    // -----------------------------------------------------------------------

    #[test]
    fn approval_missing_refuses_naming_the_gesture() {
        // Sem a aprovação do usuário a recusa sai nomeando o gesto que a
        // satisfaz, e de onde ela vem.
        let msg = unmet_gate_message("epic", true, &[], true, None)
            .expect("approval missing → a refusal");
        assert!(msg.contains("\"Aprovar\""), "names the approval gesture: {msg}");
        assert!(
            msg.contains("approval question") && msg.contains("spec.ndjson"),
            "names where the approval comes from: {msg}"
        );
        assert_eq!(approval_gate(ApprovalMode::Strict, false), ApprovalGate::Block);
    }

    /// The refusal is read by someone who has just discovered their gesture did
    /// not count, so it is where the ONE working gesture is written down: the
    /// user choosing "Aprovar" in the approval question. The doors that stopped
    /// approving — accepting plan mode and typing the slash command — are not
    /// taught, and the answers that approve nothing are named.
    #[test]
    fn the_refusal_names_the_one_gesture_that_approves() {
        let msg = unmet_gate_message("epic", true, &[], true, None)
            .expect("approval missing → a refusal");
        assert!(msg.contains("CHOOSES") && msg.contains("\"Aprovar\""), "the gesture: {msg}");
        assert!(msg.contains("Free text"), "free text approves nothing: {msg}");
        for gone in ["ExitPlanMode", "/mustard:spec", ".approved-by-user"] {
            assert!(!msg.contains(gone), "the refusal still teaches {gone}: {msg}");
        }
    }

    #[test]
    fn both_present_approves() {
        // Neither precondition unmet → no refusal message, and strict proceeds.
        assert_eq!(
            unmet_gate_message("epic", false, &[], true, None),
            None
        );
        // Uma spec light não tem prova de critério — mesmo silêncio.
        assert_eq!(
            unmet_gate_message("small", false, &[], true, None),
            None
        );
        assert_eq!(approval_gate(ApprovalMode::Strict, true), ApprovalGate::Proceed);
    }

    // -----------------------------------------------------------------------
    // The witness echo — the report surfaces what the approval recorded
    // -----------------------------------------------------------------------

    /// The report echoes the witness of the approved state — the question and
    /// the option the user chose — and when it was recorded, under keys that
    /// name the witness, never this run. With nothing to echo, both keys are
    /// omitted, never null.
    #[test]
    fn approve_spec_echoes_the_witness() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        seed_full_spec(root, spec);
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        let opts = ApproveSpecOpts { spec: spec.to_string(), wave_plan: false, resume: false };

        let report = approve_at(root_str, &opts, ApprovalMode::Strict, &mut |_, _| {})
            .expect("an approved spec approves");
        let json: Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(json["witness"], json!({ "question": "Aprovar esta spec?", "answer": "Aprovar" }));
        assert!(json["witnessAt"].as_str().is_some_and(|at| !at.is_empty()), "{json}");
        for gone in ["markerVia", "markerAt", "approvedThisSession"] {
            assert!(json.get(gone).is_none(), "{gone} left with the marker: {json}");
        }

        // `off` lets an unapproved spec through, with nothing to echo.
        let bare = tempdir().unwrap();
        seed_full_spec(bare.path(), spec);
        let report = approve_at(bare.path().to_str().unwrap(), &opts, ApprovalMode::Off, &mut |_, _| {})
            .expect("off mutes the approval precondition");
        let json: Value = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert!(json.get("witness").is_none() && json.get("witnessAt").is_none(), "{json}");
    }

    // -----------------------------------------------------------------------
    // The THIRD precondition — every criterion must carry a PROVEN record
    // -----------------------------------------------------------------------

    /// Uma spec `spec` em plano, com uma mensagem do usuário e um ponto do
    /// levantamento aberto, gravados direto no arquivo de eventos.
    fn seed_open_point(root: &Path, spec: &str) {
        let path = mustard_core::io::spec_events::spec_file(root, spec).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for (event_type, fields) in [
            ("state", json!({"phase": "plan", "author": "binary"})),
            ("message", json!({"author": "user", "text": "Travar o merge."})),
            (
                "point",
                json!({"block": "limits", "gap": "Os limites, com os valores", "from": "gap", "status": "open",
                    "origin": 2, "facts": [{"text": "f", "source": "mensagem 2"}]}),
            ),
        ] {
            mustard_core::io::spec_events::write(&path, event_type, fields.as_object().cloned().unwrap(), &[]).unwrap();
        }
    }

    /// Os pontos abertos barram a aprovação em qualquer modo: a variável do
    /// modo não desliga a trava, e a recusa diz isso.
    #[test]
    fn the_approval_mode_variable_does_not_turn_off_the_open_point_check() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        seed_open_point(root, "epic");
        let opts = ApproveSpecOpts { spec: "epic".to_string(), wave_plan: false, resume: false };
        let emitted: std::cell::RefCell<Vec<(String, Value)>> = std::cell::RefCell::new(Vec::new());
        let mut record = |kind: &str, payload: Value| emitted.borrow_mut().push((kind.to_string(), payload));
        for mode in [ApprovalMode::Off, ApprovalMode::Warn, ApprovalMode::Strict] {
            let (message, gate) =
                preconditions(root_str, "epic", mode, false).unwrap_or_else(|| panic!("{mode:?} must not relax the open point"));
            assert_eq!(gate, ApprovalGate::Block, "{mode:?}");
            assert!(message.contains("MSTD-POINT-0001") && message.contains("Os limites, com os valores"), "{message}");
            assert!(message.contains("does not relax them"), "{message}");
            let refused = approve_at(root_str, &opts, mode, &mut record)
                .err()
                .unwrap_or_else(|| panic!("{mode:?} must not approve with an open point"));
            assert!(refused.exit_nonzero, "{mode:?}");
        }
        assert!(emitted.borrow().is_empty(), "nothing emitted under any mode");
    }

    /// Lado a lado: a passagem para o plano e a conferência da aprovação veem
    /// os mesmos pontos abertos, com a mesma lista, a cada ponto que fecha.
    #[test]
    fn the_passage_and_the_approval_see_the_same_open_points() {
        use crate::commands::spec_events::write::{record, record_open};
        use mustard_core::domain::spec_events::Refusal;
        use mustard_core::domain::spec_state::PhaseWriter;
        use mustard_core::platform::i18n::{translate, Locale};
        let dir = tempdir().unwrap();
        let root = dir.path();
        let root_str = root.to_str().unwrap();
        let spec = "epic";
        let draft = |value: Value| value.as_object().cloned().unwrap();
        assert_eq!(record_open(root, spec, "feature/epic", "dev"), Ok(true));
        let goal = "Travar o merge.";
        let said =
            record(root, spec, "message", draft(json!({"author": "user", "text": goal})), PhaseWriter::Binary).unwrap().written.id;
        record(root, spec, "context", draft(json!({"text": goal, "origin": said})), PhaseWriter::Binary).unwrap();
        record(root, spec, "work_type", draft(json!({"kinds": ["fix"], "origin": said})), PhaseWriter::Binary).unwrap();
        let mut points = Vec::new();
        for key in mustard_core::domain::survey::gaps(&["fix"]) {
            let point = json!({"block": key.block(), "gap": key.name(), "from": "gap", "status": "open", "origin": said,
                "facts": [{"text": "f", "source": format!("mensagem {said}")}]});
            points.push(record(root, spec, "point", draft(point), PhaseWriter::Binary).unwrap().written.id);
        }
        let to_plan = || {
            record(root, spec, "state", draft(json!({"phase": "plan", "author": "binary"})), PhaseWriter::Binary).err()
        };
        for (closed, id) in points.iter().enumerate() {
            let Some(Refusal::SurveyOpen { count, points: listed, .. }) = to_plan() else {
                panic!("the passage must refuse with the open points");
            };
            assert_eq!(count, points.len() - closed);
            let line = open_points_line(root_str, spec).expect("the approval sees the open points");
            let expected = translate("approve_spec.open_points", Locale::PtBr)
                .replace("{count}", &count.to_string())
                .replace("{points}", &listed);
            assert_eq!(line, expected);
            let closing = json!({"block": "x", "gap": "g", "from": "gap", "status": "closed", "closes": id,
                "result": [said], "origin": said});
            record(root, spec, "point", draft(closing), PhaseWriter::Binary).unwrap();
        }
        assert_eq!(open_points_line(root_str, spec), None);
        assert!(to_plan().is_none(), "with every point closed the passage goes");
    }
}
