// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! The flows stop instructing the two gestures the engine no longer needs.
//!
//! Two ceremonies were removed from the pipeline — steps that cost a call
//! without changing what the pipeline is allowed to do. The engine change alone
//! does not remove them: the operator does what the FLOWS say, so a shortened
//! engine behind unshortened prose is a ceremony that still gets performed.
//!
//! Every test here reads BOTH halves of one page and requires them to agree:
//!
//! 1. the new instruction is PRESENT, at the place a reader actually arrives
//!    at it — not merely somewhere in the file; and
//! 2. the sentence it SUPERSEDES is gone.
//!
//! Half 2 is what makes these more than spell-checks, and it is the half that
//! could fail: each needle in [`SUPERSEDED`] was verified to exist verbatim in
//! the shipped page BEFORE this wave edited it, so the assertion runs red
//! against the old content instead of passing over prose nobody changed. Half 1
//! alone would go green the moment a sentence is appended, leaving the
//! contradicting instruction in place two paragraphs above — which is exactly
//! how a flow ends up teaching both contracts at once.
//!
//! The three ceremony tests read the `plugin/` prose ONLY. Nothing in them
//! asserts the code that implements the shortened path: the two lived in
//! different waves, and a prose test that compiles against its sibling's work is
//! a test that cannot be run until the sibling lands.
//!
//! [`the_full_path_reaches_full_plan_before_the_census_step`] is the exception,
//! and deliberately: its sentence exists to explain an engine behaviour (an
//! empty census can only abstain), so it reads that engine too — the pairing
//! every prose test in `plugin_prose_matches_shipped_behaviour.rs` makes. A
//! reason nobody re-checks is how a page ends up explaining a mechanism that was
//! deleted.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt --test spec_flow_prose`, which selects this binary
//! by name.

use std::path::{Path, PathBuf};

/// The repository root — two levels up from this crate's manifest.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Read a repo-relative file, failing with the path when it is missing.
fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{rel} unreadable at {}: {e}", path.display()))
}

/// The first line of `body` containing `needle`, or `None`.
fn line_with<'a>(body: &'a str, needle: &str) -> Option<&'a str> {
    body.lines().find(|l| l.contains(needle))
}

/// Fail when `body` still carries any sentence [`SUPERSEDED`] lists for `rel`.
///
/// Kept as one sweep per page rather than a needle per assertion: the point is
/// that NO surviving copy of the old contract is left behind, and a reader who
/// meets the second copy is instructed by it just as much as by the first.
fn assert_superseded_gone(rel: &str, body: &str) {
    let survivors: Vec<&str> = SUPERSEDED
        .iter()
        .filter(|(file, _)| *file == rel)
        .map(|(_, needle)| *needle)
        .filter(|needle| body.contains(needle))
        .collect();
    assert!(
        survivors.is_empty(),
        "{rel} still teaches the superseded contract — a reader who arrives at \
         one of these is instructed by it:\n  {}",
        survivors.join("\n  "),
    );
}

/// Choosing a spec is not approving it.
///
/// Three doors used to mint one approval: a letter typed as the whole prompt,
/// an accepted plan-mode plan and a selected option of the approval question.
/// Two of them are about something else — a letter picks a row, and accepting
/// a plan can be about anything — so the approval kept one door: the user
/// chooses "Aprovar" in the question, and the witness records it. The page
/// must say so where each reader arrives at the picker, and the sentences of
/// the old contract must be gone.
#[test]
fn picker_prose_says_selecting_is_not_approving() {
    let picker = read("plugin/commands/spec.md");

    // --- 1. The one door, where each reader arrives at it ------------------
    let intro = line_with(&picker, "**Selecting is not approving:**")
        .expect("the picker header no longer says that selecting a row does not approve it");
    for needle in ["\"Aprovar\"", "witness", "`/clear`", "approve nothing"] {
        assert!(intro.contains(needle), "the header misses {needle}: {intro}");
    }

    let parse = line_with(&picker, "**`^[a-z]r?$`**")
        .expect("the picker no longer documents the letter-mode pattern");
    assert!(
        parse.contains("SELECTS") && parse.contains("approves nothing"),
        "the parse rule must say the letter only selects the row: {parse}",
    );

    // The `Modo de seleção` block is printed LITERALLY to the user, so it is
    // the contract the operator actually sees at the picker.
    let modo = line_with(&picker, "**Modo de seleção**")
        .expect("the picker no longer carries the literal Modo de seleção block");
    assert!(
        modo.contains("Aprovar esta spec?") && modo.contains("\"Aprovar\""),
        "the literal selection legend must say where the approval happens: {modo}",
    );

    let plan_route = line_with(&picker, "resume-loop **§A Approve**")
        .expect("the picker no longer routes a Plan-stage spec to §A");
    for needle in ["**Aprovar**", "**Ajustar**", "`/clear`", ".clarified", "approvedByUser:true"] {
        assert!(plan_route.contains(needle), "the Plan route misses {needle}: {plan_route}");
    }

    // --- 2. The superseded sentences are gone -----------------------------
    assert_superseded_gone("plugin/commands/spec.md", &picker);
    assert_superseded_gone("plugin/pipeline-config.md", &read("plugin/pipeline-config.md"));
}

/// §A asks one question, and the approval ends the window that asked it.
///
/// A spec already approved is not asked again, and its resume starts the
/// execution. A spec not yet approved gets the one question, "Aprovar esta
/// spec?", with "Aprovar" and "Ajustar"; choosing "Aprovar" is the whole
/// approval, and the next step is `/clear`, never a dispatch in the window
/// that asked. The typed letter and plan mode approve nothing.
#[test]
fn resume_prose_asks_one_question_and_suggests_clear() {
    let loop_ref = read("plugin/refs/spec/resume-loop.md");

    let already = line_with(&loop_ref, "**Already approved — skip re-approval")
        .expect("§A no longer carries the approvedByUser shortcut");
    for needle in ["approvedByUser", "--resume", "§B"] {
        assert!(already.contains(needle), "the shortcut misses {needle}: {already}");
    }

    let typed = line_with(&loop_ref, "**A typed picker letter approves nothing.**")
        .expect("§A never says the typed letter approves nothing");
    assert!(typed.contains("ExitPlanMode"), "plan mode must be named as no door: {typed}");

    let approve = line_with(&loop_ref, "- **Aprovar** →")
        .expect("§A no longer says what choosing Aprovar does");
    assert!(
        approve.contains("`/clear`") && approve.contains("do not dispatch"),
        "choosing Aprovar must end in /clear, never in a dispatch here: {approve}",
    );
    let adjust = line_with(&loop_ref, "- **Ajustar** →").expect("§A no longer handles Ajustar");
    assert!(adjust.contains("wave-collapse"), "Ajustar keeps the reject path: {adjust}");

    // All of it lives in §A, where the approval is decided.
    let section = loop_ref.find("## §A").expect("the loop ref no longer has a §A");
    let loop_at = loop_ref.find("## §B").expect("the loop ref no longer has a §B");
    for needle in ["- **Aprovar** →", "**A typed picker letter approves nothing.**"] {
        let at = loop_ref.find(needle).expect("checked above");
        assert!(section < at && at < loop_at, "{needle} must sit inside §A");
    }

    // --- 2. The superseded sentences are gone -----------------------------
    assert_superseded_gone("plugin/refs/spec/resume-loop.md", &loop_ref);
}

/// The Full plan materialises in ONE call, and `plan-materialize` is
/// named as the RE-materialisation door.
///
/// The old steps 2 and 3 spent two calls on one decision the first call had
/// already recorded: `spec-draft` wrote `isWavePlan:true` with the wave count
/// and created no wave directory, and the layout appeared only after a
/// hand-written plan file and a second command. `plan-materialize` survives
/// because reconciling a layout onto an EDITED plan is load-bearing — that is
/// the door the prose must now describe it as.
#[test]
fn full_plan_prose_materialises_in_one_call() {
    let plan = read("plugin/refs/feature/full-plan.md");

    // --- 1. Step 2 is the one-call door -----------------------------------
    let step_two = line_with(&plan, "2. Materialise the WHOLE layout in ONE call")
        .expect("the PLAN order no longer opens with a one-call materialisation");
    assert!(
        step_two.contains("spec-draft") && step_two.contains("--plan"),
        "step 2 does not name the flag that fuses the materialisation: {step_two}",
    );
    for artefact in ["wave-plan.md", "meta.json", "spec.md"] {
        assert!(
            step_two.contains(artefact),
            "step 2 must say `{artefact}` comes out of that one call: {step_two}",
        );
    }
    // The gate that is NOT being removed has to stay visible on the fused path,
    // or a shortened flow reads as a dropped refusal.
    assert!(
        step_two.contains("NEGATIVE TEST"),
        "step 2 drops the negative proof from the fused call: {step_two}",
    );
    assert!(
        step_two.contains("leaves NO layout behind"),
        "step 2 must say a refusal materialises nothing, or a retry meets a \
         directory it did not create: {step_two}",
    );

    // --- 2. Step 3 is the RE-materialisation door -------------------------
    let step_three = line_with(&plan, "3. RE-materialise")
        .expect("the PLAN order no longer has a re-materialisation step");
    assert!(
        step_three.contains("plan-materialize"),
        "step 3 no longer names the command it is about: {step_three}",
    );
    assert!(
        step_three.contains("RE-materialisation door"),
        "step 3 must name plan-materialize as the RE-materialisation door: {step_three}",
    );
    assert!(
        step_three.contains("ONLY when the plan CHANGES"),
        "step 3 must be conditional — a first materialisation does not come \
         through here: {step_three}",
    );

    // --- 3. The superseded sentences are gone -----------------------------
    assert_superseded_gone("plugin/refs/feature/full-plan.md", &plan);
}

/// The Full path reaches the full-plan machinery BEFORE the step that
/// reads the `## Files` census.
///
/// `/feature` §2 ordered `plan-prepare` immediately after the draft, and on the
/// Full path the census that call reads is authored LATER — inside
/// `full-plan.md` step 2, out of the lapidated wave bodies folded into the plan
/// JSON. So the first call could only ever answer `scope:"abstain"` with
/// `filesSectionEmpty:true`: a step whose verdict is settled by the order it
/// sits in rather than by the spec it reads. Measured on this unit's own run.
///
/// Both halves are asserted. Half 1: the fork is written where the reader
/// arrives at it, which for an ORDERING defect means one step earlier than the
/// call it supersedes — a sentence appended below `plan-prepare` would be true
/// and useless. Half 2: the engine still answers the way the fork gives as its
/// reason, because a paragraph that explains a deleted mechanism reads as
/// ceremony and gets re-ordered back.
#[test]
fn the_full_path_reaches_full_plan_before_the_census_step() {
    let feature = read("plugin/commands/feature.md");

    // --- 1. The fork is written down, and it names its door ----------------
    let fork = line_with(&feature, "full path continues in")
        .expect("/feature never sends the Full path on to the full-plan machinery");
    assert!(
        fork.contains("full-plan.md"),
        "the fork names no document to continue in: {fork}",
    );
    // The two documents disagreed about the FIRST materialisation: `/feature`
    // described a `spec-draft` with no `--plan`, which lands a Full spec in
    // `plan-materialize` — the door full-plan.md classifies as the EDIT one.
    assert!(
        fork.contains("spec-draft --plan"),
        "the fork does not name the one-call first materialisation, so the two \
         pages still disagree about which door a Full spec goes through: {fork}",
    );
    // Naming the door is not teaching it — the reason has to travel with it, or
    // the steps get re-ordered back the moment a reader is in a hurry.
    assert!(
        fork.contains("abstain") && fork.contains("filesSectionEmpty"),
        "the fork never says WHY the census step cannot help a Full spec: {fork}",
    );

    // --- 2. And it sits BEFORE the census-dependent step -------------------
    let fork_at = feature.find("full path continues in").expect("checked above");
    let census_at = feature
        .find("run plan-prepare")
        .expect("/feature no longer calls plan-prepare at all");
    assert!(
        fork_at < census_at,
        "the fork must precede the census-dependent step — that IS the fix; a \
         reader who meets `plan-prepare` first has already spent the one call \
         that can only abstain (fork at {fork_at}, plan-prepare at {census_at})",
    );

    // The page it forks INTO must still open with that same call, or `/feature`
    // now points at a step its target document no longer describes.
    let plan_doc = read("plugin/refs/feature/full-plan.md");
    let one_call = line_with(&plan_doc, "2. Materialise the WHOLE layout in ONE call")
        .expect("full-plan.md no longer opens PLAN with a one-call materialisation");
    assert!(
        one_call.contains("spec-draft") && one_call.contains("--plan"),
        "full-plan.md step 2 no longer names the call `/feature` now points at: {one_call}",
    );

    // --- 3. The engine still answers the way the fork's reason claims ------
    // Without this half the paragraph outlives the mechanism: it would keep
    // explaining an abstention the classifier had stopped emitting.
    let classifier = read("apps/rt/src/commands/spec/scope_decompose.rs");
    assert!(
        classifier.contains("fn stamp_files_zero"),
        "nothing stamps the zero-census downgrade any more, so a Full spec's \
         first `plan-prepare` no longer abstains and the fork lost its reason",
    );
    for stamped in ["json!(\"abstain\")", "\"filesSectionEmpty\""] {
        assert!(
            classifier.contains(stamped),
            "the zero-path census no longer emits `{stamped}` — the fork explains \
             a verdict the engine stopped producing",
        );
    }
}

/// The picker's own legend names the status the table can now print.
///
/// The renderer gained `W{N} a iniciar` for a plan that was scaffolded and never
/// dispatched, and `active_specs` pins the legend it renders itself. The picker
/// page carries a SECOND legend — the literal `Siglas` block the operator reads
/// to decode the table — and nothing pinned it. A legend shorter than the
/// behaviour is the same defect this unit removes, in miniature: the reader
/// meets a status the key does not explain and has to open the source to learn
/// whether `a iniciar` means start it or resume it.
#[test]
fn the_picker_legend_names_the_not_yet_started_status() {
    let picker = read("plugin/commands/spec.md");
    let legend = line_with(&picker, "**Siglas**")
        .expect("the picker no longer carries the Siglas legend at all");

    assert!(
        legend.contains("W{N} em exec"),
        "the legend dropped the dispatched-and-running status: {legend}",
    );
    assert!(
        legend.contains("W{N} a iniciar"),
        "the legend never names the scaffolded-but-never-dispatched status the \
         table now prints, so the reader meets a status the key cannot decode: {legend}",
    );
    // Naming it is not explaining it: the two statuses ask for OPPOSITE actions,
    // and that is the whole reason the status was split in two.
    assert!(
        legend.contains("nothing dispatched yet"),
        "the legend names `a iniciar` without saying what it means for the \
         reader's next action — start it, never resume it: {legend}",
    );

    // And the renderer still prints it, or the legend explains a status the
    // table stopped producing.
    let renderer = read("apps/rt/src/commands/spec/active_specs.rs");
    assert!(
        renderer.contains("a iniciar"),
        "`active_specs` no longer renders `a iniciar` — the picker legend now \
         decodes a status nothing emits",
    );

    // The key must not be shorter than the behaviour in EITHER direction, so the
    // remaining values `derive_status` can return are asserted too. `⚠ malformed`
    // and `closed-followup` were reachable long before this unit and the picker
    // legend never named them: the reader met them with no key at all.
    for status in ["⚠ malformed", "closed-followup"] {
        assert!(
            // The literal `derive_status` returns it as, quotes included.
            renderer.contains(&format!("\"{status}\"")),
            "`active_specs` no longer emits `{status}` — drop it from the picker \
             legend rather than leaving a key for a status nothing prints",
        );
        assert!(
            legend.contains(status),
            "the legend never names `{status}`, a status the table prints today: {legend}",
        );
    }

    // And nothing the table CANNOT print may sit in the key. `BLOCK` did — a
    // status no branch of `derive_status` returns — which is the phantom half of
    // the same defect: a key entry that guarantees a behaviour nobody wrote.
    assert!(
        !legend.contains("BLOCK"),
        "the legend names `BLOCK`, which `derive_status` never returns — a key \
         entry for a status the table cannot print: {legend}",
    );
    assert!(
        !renderer.contains("BLOCK"),
        "`active_specs` started emitting `BLOCK` — teach the picker legend the \
         status before the operator meets it undecoded",
    );
}

/// The flow hands the draft the name the GATE minted, rather than
/// letting it derive a second one.
///
/// The engine no longer depends on this — `spec-draft` reads the slug half of
/// the unit's branch when no `--slug` arrives, which is what closes the chain on
/// the shipped path. But the call the operator reads is where the one-name rule
/// is either visible or invisible, and a flow that never mentions the gate's
/// answer teaches that the draft is free to name the unit.
#[test]
fn the_draft_call_carries_the_name_the_gate_minted() {
    let feature = read("plugin/commands/feature.md");
    let draft = line_with(&feature, "run spec-draft --intent")
        .expect("/feature no longer calls spec-draft");
    assert!(
        draft.contains("--slug"),
        "the draft call does not carry the unit's name, so the flow reads as if \
         the draft may invent one: {draft}",
    );
    assert!(
        draft.contains("NOT yours to choose"),
        "the call passes `--slug` without saying where its value comes from — a \
         reader who fills it in from their own head mints the second name this \
         unit exists to remove: {draft}",
    );

    // The page that MINTS it must say the report is the source, or the two ends
    // of the hand-off name different things.
    let router = read("packages/core/templates/mustard/dispatch.md");
    let gate = line_with(&router, "That call is also where the unit is NAMED")
        .expect("the router never says the base gate names the unit");
    assert!(
        gate.contains("renamedFrom") && gate.contains("spec-draft --slug"),
        "the router names the unit without telling the reader to carry that \
         value into the draft: {gate}",
    );
}

/// The approval question names the gesture that counts BEFORE it asks.
///
/// A gate that accepts one specific gesture has to name it in the message
/// that asks for it: choosing the option "Aprovar". Free text typed instead of
/// choosing, and an option without the approval word, approve nothing — and
/// the witness can only say so after the gesture is spent. Both halves are
/// asserted: the naming sits on the line that presents the plan, ahead of the
/// options; and the witness still declines the way that line says.
#[test]
fn the_approval_question_names_the_gesture_before_asking_for_it() {
    let loop_ref = read("plugin/refs/spec/resume-loop.md");

    // --- 1. The gesture is named, on the line that presents the plan --------
    let ask = line_with(&loop_ref, "Present for approval")
        .expect("§A no longer presents the plan for approval");
    for needle in [
        "Aprovar esta spec?",
        "**Aprovar**",
        "**Ajustar**",
        "free text",
        "before the question is answered",
        "preview",
    ] {
        assert!(ask.contains(needle), "the approval line misses {needle}: {ask}");
    }
    let ask_at = loop_ref.find("Present for approval").expect("checked above");
    let options_at = loop_ref.find("- **Aprovar** →").expect("§A no longer lists the Aprovar option");
    assert!(
        ask_at < options_at,
        "the gesture must be named ahead of the options it governs (ask at {ask_at}, \
         options at {options_at})",
    );

    // --- 2. The witness still declines the way the line claims --------------
    let witness = read("apps/rt/src/hooks/observe/approval_witness.rs");
    assert!(
        witness.contains(r#"translate("approval.option", lang)"#),
        "the witness no longer takes the approve option from the catalog — the question \
         now teaches a label it may not accept",
    );
    assert!(
        witness.contains("fn is_offered"),
        "nothing separates a chosen option from free text any more",
    );
}

/// The bare `r` names the unit the checkout stands in, and approves nothing.
///
/// Inside the unit's own work branch the branch already names the unit, so the
/// picker opens it without a table; elsewhere the lone `r` reads as the row
/// letter it looks like. Either way it only chooses WHICH spec is open — and
/// the slash-command door that used to record an approval from it records
/// nothing now.
#[test]
fn the_bare_r_names_the_unit_and_approves_nothing() {
    let picker = read("plugin/commands/spec.md");
    let parse = line_with(&picker, "**`^r$`").expect("§1 never parses the bare `r`");
    for needle in ["work branch", "approves nothing"] {
        assert!(parse.contains(needle), "the bare form misses {needle}: {parse}");
    }
    assert!(
        parse.contains("integration base") || parse.contains("detached HEAD"),
        "the page must say what happens where the tree shows no unit: {parse}",
    );
    let letters = line_with(&picker, "**`^[a-z]r?$`**")
        .expect("the picker no longer documents the letter-mode pattern");
    assert!(
        letters.contains("carve-out"),
        "letter mode still reads as though a lone `r` were row `r`: {letters}",
    );

    // And the slash-command door records nothing.
    let observer = read("apps/rt/src/hooks/observe/picker_approval_observer.rs");
    for writes in ["write_atomic", "spec_events::write", "record("] {
        assert!(!observer.contains(writes), "the slash-command door still writes: {writes}");
    }
}

/// The sentences this wave supersedes, verbatim as the pages carried them
/// BEFORE the edit — each one verified present at that point, so the sweep
/// above genuinely fails against the old content rather than asserting nothing.
///
/// A page may state the new contract and still carry one of these two
/// paragraphs away; the reader who arrives at the survivor is instructed by it.
const SUPERSEDED: &[(&str, &str)] = &[
    // The picker: `r` grants nothing / bypasses nothing.
    ("plugin/commands/spec.md", "it never grants or skips the approval itself"),
    ("plugin/commands/spec.md", "pre-answers the §3 EXECUTE continuation"),
    ("plugin/commands/spec.md", "`r` never bypasses it"),
    ("plugin/commands/spec.md", "pre-answers only the *implement now* continuation"),
    // The picker's literal selection legend, promising the marker to a letter
    // answered INTO the table — the two-step form, which never reaches the
    // observer. Only `/mustard:spec ar` typed in full does.
    ("plugin/commands/spec.md", "the text you typed mints"),
    // The focused resume that asked for a confirmation it already had: the
    // caller was standing inside the unit's own branch.
    ("plugin/commands/spec.md", "In focused mode, first print a one-line header"),
    // §A: the second gesture, asked for twice.
    (
        "plugin/refs/spec/resume-loop.md",
        "a letter-mode `r` that pre-answers *implement now*",
    ),
    (
        "plugin/refs/spec/resume-loop.md",
        "A letter-mode `r` pre-answers only the EXECUTE continuation",
    ),
    // The Full plan: the two-call materialisation, and its own copy of the
    // approval sentence.
    ("plugin/refs/feature/full-plan.md", "`r` never skips the approval itself"),
    (
        "plugin/refs/feature/full-plan.md",
        "Materialise the scaffold — AFTER the conversation material is assembled",
    ),
    (
        "plugin/refs/feature/full-plan.md",
        "Fold the body into the plan JSON (never by hand after the scaffold).",
    ),
    (
        "plugin/refs/feature/full-plan.md",
        "the `wave-scaffold` renderer inside `plan-materialize` owns `wave-plan.md`",
    ),
    // Publicar só em SSH, e como pedido do usuário: o contrato que o "sempre
    // publicar" de 10/09/2026 substituiu.
    ("plugin/commands/spec.md", "**Remote session — the page has to travel.**"),
    (
        "plugin/commands/spec.md",
        "In a remote (SSH) session the link that counts is the page published to claude.ai, when a publishing tool exists",
    ),
    (
        "plugin/commands/spec.md",
        "and hand the user its `url` (the `file://…/resumo.html`) as a clickable link",
    ),
    (
        "packages/core/src/platform/i18n.rs",
        "Peça ao assistente para publicar a página no claude.ai",
    ),
    // O gancho do fim da resposta que mandava republicar a página e listava
    // os `scp` saiu: a prosa que contava com ele ensinava um passo que ninguém
    // mais dá.
    ("plugin/commands/spec.md", "the end-of-turn hook speaks only when the page changes"),
    ("plugin/commands/spec.md", "the hook blocks that ending with the order to publish"),
    ("plugin/commands/spec.md", "the `scp` commands the end-of-turn message lists"),
    // The approval kept a single door, the question: the typed letter and plan
    // mode stopped approving, and the approval marker left.
    ("plugin/commands/spec.md", "**Selecting IS approving:**"),
    ("plugin/commands/spec.md", "A bare letter MINTS"),
    ("plugin/commands/spec.md", "plan mode first, the approve/implement `AskUserQuestion` as fallback"),
    ("plugin/commands/spec.md", "<spec>/.approved-by-user"),
    ("plugin/refs/spec/resume-loop.md", "**Plan mode is PRIMARY**"),
    ("plugin/refs/spec/resume-loop.md", "the marker is already minted, so go straight to the dispatch"),
    ("plugin/refs/spec/resume-loop.md", "<spec>/.approved-by-user"),
    ("plugin/refs/feature/full-plan.md", "`ExitPlanMode` acceptance mints"),
    ("plugin/refs/feature/full-plan.md", "\"Approve wave plan for later\""),
    ("plugin/refs/feature/full-plan.md", "<spec>/.approved-by-user"),
    ("plugin/pipeline-config.md", "no `<spec>/.approved-by-user` marker"),
];

/// Em sessão remota, o roteiro do `/mustard:spec` sabe que o `file://`
/// não chega ao usuário, e os `scp` que ele ensina são o último recurso, só sem
/// ferramenta de publicação — sempre ANTES da pergunta de aprovação.
///
/// Numa sessão por SSH o `file://` aponta para o disco do servidor, e o
/// navegador de lá nunca chega ao usuário: foi assim que ele ficou sem ler a
/// spec em 10/09/2026. A prosa é lida: o parágrafo do §3, entre a chamada do
/// `spec-doc` e o roteamento, e o inviolável. O gancho do fim da resposta que
/// também reconhecia a sessão remota (`spec_doc_present`) saiu, e a regra fica
/// só na prosa, com os comandos `scp` escritos nela.
#[test]
fn spec_door_teaches_remote_publishing() {
    // O checkout do Windows entrega a prosa com CRLF, e o recorte por
    // parágrafo abaixo divide em "\n\n": sem normalizar, o arquivo inteiro vira
    // um parágrafo só e a posição da regra sai errada.
    let picker = read("plugin/commands/spec.md").replace("\r\n", "\n");
    let paragraph = picker
        .split("\n\n")
        .find(|p| p.contains("SSH_CONNECTION"))
        .unwrap_or_else(|| panic!("no paragraph of spec.md names the remote (SSH) session"));
    // Os comandos `scp` estão na própria prosa: nenhuma mensagem os lista.
    for needle in [
        "SSH_CLIENT",
        "claude.ai",
        "resumo.html",
        "BEFORE",
        "AskUserQuestion",
        "scp",
        "last resort",
        "`$USER`",
        "third field of `SSH_CONNECTION`",
        "xdg-open",
    ] {
        assert!(paragraph.contains(needle), "the remote-session rule misses {needle}:\n{paragraph}");
    }

    // No §3, depois da chamada do `spec-doc` e antes do roteamento pelo
    // estágio — o ponto em que a pergunta de aprovação ainda não foi feita.
    let at = picker.find(paragraph).unwrap();
    let call = picker.find("rtk mustard-rt run spec-doc --spec").expect("the spec-doc call");
    let route = picker.find("Route on the returned `stage`").expect("the stage routing");
    assert!(call < at && at < route, "the remote rule must sit between the spec-doc call and the routing");

    let inviolable = &picker[picker.find("## Inviolable").expect("the Inviolable section")..];
    let rule = line_with(inviolable, "The page precedes the question").expect("the page rule");
    assert!(
        rule.contains("claude.ai") && rule.contains("SSH"),
        "the inviolable never says what counts as the page in a remote session: {rule}",
    );
}

/// Em TODA retomada, em qualquer etapa, o roteiro do `/mustard:spec` entrega o
/// link publicado (`publishedUrl`) numa linha própria e, sem endereço gravado,
/// publica a página e grava o endereço com `--published-url`. Publicar vale
/// sempre, não só em SSH: os `scp` ficam como último recurso.
///
/// Nenhum gancho entrega o link nem manda republicar, então quem retomava uma
/// unidade de página parada nunca via o link de novo; e a página que muda só é
/// republicada porque a prosa manda o próprio assistente fazer isso, no mesmo
/// endereço. As duas metades são lidas: a prosa do §3, entre a chamada do
/// `resume-bootstrap` e a regra que vale só para o plano, e o motor que a
/// sustenta — o campo que a retomada devolve e a flag que o `spec-doc`
/// declara, com os nomes da prosa.
#[test]
fn spec_door_hands_the_published_link_on_every_resume() {
    let picker = read("plugin/commands/spec.md").replace("\r\n", "\n");
    let paragraph = picker
        .split("\n\n")
        .find(|p| p.contains("publishedUrl"))
        .unwrap_or_else(|| panic!("no paragraph of spec.md reads publishedUrl off the resume"));
    for needle in [
        "Every resume",
        "every stage",
        "on a line of its own",
        "`null`",
        "claude.ai",
        "rtk mustard-rt run spec-doc --spec {specName} --published-url",
        "never an option offered to the user",
        "over SSH alike",
        "republish it yourself",
        "`changed`",
    ] {
        assert!(paragraph.contains(needle), "the resume rule misses {needle}:\n{paragraph}");
    }

    // No §3, depois da retomada e antes da regra do plano: vale para toda
    // etapa, não só para a spec que espera aprovação.
    let at = picker.find(paragraph).unwrap();
    let boot = picker.find("rtk mustard-rt run resume-bootstrap").expect("the resume call");
    let plan = picker.find("**On a `Plan`-stage spec").expect("the Plan-stage rule");
    let route = picker.find("Route on the returned `stage`").expect("the stage routing");
    assert!(
        boot < at && at < plan && plan < route,
        "the resume rule must sit after resume-bootstrap and ahead of the Plan-only rule",
    );

    // Os `scp` são o último recurso, só sem ferramenta de publicação.
    let fallback = picker
        .split("\n\n")
        .find(|p| p.contains("`scp`"))
        .unwrap_or_else(|| panic!("no paragraph of spec.md carries the scp fallback"));
    assert!(
        fallback.contains("last resort") && fallback.contains("Only when no tool that publishes"),
        "the copy commands must be the fallback, not the rule:\n{fallback}",
    );

    assert_superseded_gone("plugin/commands/spec.md", &picker);
    assert_superseded_gone(
        "packages/core/src/platform/i18n.rs",
        &read("packages/core/src/platform/i18n.rs"),
    );

    // A metade do motor: a retomada devolve `publishedUrl`, e o `spec-doc`
    // declara a flag que a prosa manda usar.
    let resume = read("apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs");
    assert!(
        resume.contains("#[serde(rename = \"publishedUrl\")]"),
        "resume-bootstrap no longer reports publishedUrl",
    );
    let cli = read("apps/rt/src/commands/spec/cli.rs");
    assert!(
        cli.contains("#[arg(long = \"published-url\")]"),
        "spec-doc no longer declares --published-url",
    );
}
