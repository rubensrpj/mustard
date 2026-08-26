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

/// AC-4 — the picker states that the typed `r` IS the approval.
///
/// The picker used to say the opposite in four places, and it was load-bearing
/// prose: the operator read it and routed a user who had already typed their
/// consent through a plan-mode round trip to type it again. The marker's whole
/// property is that the model cannot author it — and the text a user types is
/// exactly that class of act, so honouring it removes a gesture without
/// weakening the gate.
#[test]
fn picker_prose_states_the_typed_letter_is_the_approval() {
    let picker = read("plugin/commands/spec.md");

    // --- 1. The new contract, where each reader arrives at it -------------
    // The header line: the first thing anyone opening the command reads.
    let intro = line_with(&picker, "A letter + `r` (e.g. `ar`)")
        .expect("the picker header no longer explains the `r` suffix");
    assert!(
        intro.contains(".approved-by-user"),
        "the header says what `ar` does without naming the marker it mints: {intro}",
    );
    // Saying it approves is not the whole contract — the bare letter must NOT.
    assert!(
        intro.contains("ALONE"),
        "the header must keep the bare letter on the normal approval path: {intro}",
    );

    // The parse rule: where the letter form is actually decoded.
    let parse = line_with(&picker, "**`^[a-z]r?$`**")
        .expect("the picker no longer documents the letter-mode pattern");
    assert!(
        parse.contains(".approved-by-user"),
        "the parse rule does not say the typed `r` mints the marker: {parse}",
    );
    assert!(
        parse.contains(".clarified"),
        "the parse rule must keep the clarify gate in front of a Full approval: {parse}",
    );

    // The `Modo de seleção` block is printed LITERALLY to the user, so it is
    // the contract the operator actually sees at the picker.
    let modo = line_with(&picker, "**Modo de seleção**")
        .expect("the picker no longer carries the literal Modo de seleção block");
    assert!(
        modo.contains(".approved-by-user"),
        "the literal selection legend still describes `r` as a pre-answer: {modo}",
    );

    // The route into the approve gate: the paragraph that decides whether a
    // second gesture is asked for at all.
    let plan_route = line_with(&picker, "resume-loop **§A Approve**")
        .expect("the picker no longer routes a Plan-stage spec to §A");
    assert!(
        plan_route.contains("no second question") || plan_route.contains("no second gesture"),
        "the Plan route must state that `r` costs no further gesture: {plan_route}",
    );

    // --- 2. The superseded sentences are gone -----------------------------
    assert_superseded_gone("plugin/commands/spec.md", &picker);
}

/// AC-4 — §A takes the shortcut it already had, for the typed form too.
///
/// §A already skipped the re-approval on `approvedByUser:true`, for exactly the
/// reason that applies here: the marker exists, so asking again is the
/// redundant second gesture. The typed form mints the same marker, and the
/// section had no sentence saying so.
#[test]
fn resume_prose_skips_the_second_gesture_for_the_typed_form() {
    let loop_ref = read("plugin/refs/spec/resume-loop.md");

    // --- 1. The shortcut is written down, inside §A -----------------------
    let shortcut = line_with(&loop_ref, "Typed `{letter}r`")
        .expect("§A never tells the orchestrator what to do with a typed `{letter}r`");
    assert!(
        shortcut.contains(".approved-by-user"),
        "the shortcut does not name the marker that is already minted: {shortcut}",
    );
    assert!(
        shortcut.contains("§B"),
        "the shortcut must send the orchestrator to the dispatch, not back to a \
         question: {shortcut}",
    );
    assert!(
        shortcut.contains("--resume"),
        "the shortcut must name the flag that carries `implement now` into the \
         relay: {shortcut}",
    );
    // The counterweight, so the paragraph cannot be read as "the picker always
    // approves": a bare letter mints nothing and takes the normal gate.
    assert!(
        shortcut.contains("bare letter"),
        "the shortcut omits its limit — a letter without `r` mints no marker: {shortcut}",
    );

    // It belongs in §A, beside the approvedByUser shortcut it extends; the §B
    // loop is not where an approval decision is read.
    let already = loop_ref
        .find("**Already approved — skip re-approval")
        .expect("§A no longer carries the approvedByUser shortcut");
    let typed = loop_ref.find("Typed `{letter}r`").expect("checked above");
    assert!(
        typed > already && typed - already < 1500,
        "the typed-form shortcut must sit with the approvedByUser one it extends \
         (approvedByUser at {already}, typed form at {typed})",
    );

    // --- 2. The superseded sentences are gone -----------------------------
    assert_superseded_gone("plugin/refs/spec/resume-loop.md", &loop_ref);
}

/// AC-4 — the Full plan materialises in ONE call, and `plan-materialize` is
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

/// AC-7 — the Full path reaches the full-plan machinery BEFORE the step that
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

/// **AC-9** — the picker's own legend names the status the table can now print.
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

/// **AC-10** — the flow hands the draft the name the GATE minted, rather than
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

/// The `AskUserQuestion` fallback names the gesture that counts BEFORE it asks
/// for one.
///
/// The paragraph used to say only that "the answer mints the same marker",
/// which is true of exactly one answer and false of every other one the dialog
/// accepts: an option whose label carries no approval stem records nothing, and
/// so does an answer typed as free text. Both declines ARE explained — on
/// stderr, at a moment the operator reaches only after spending the gesture — so
/// the explanation arrives after the cost. A gate that accepts one specific
/// gesture has to name it in the message that asks for it.
///
/// Both halves are asserted. Half 1: the naming sits on the fallback line, ahead
/// of the options it offers — an ordering defect is not fixed by a true sentence
/// written below the question. Half 2: the recorder still declines the way the
/// paragraph gives as its reason, or the page teaches a condition nothing
/// enforces.
#[test]
fn the_approval_fallback_names_the_gesture_before_asking_for_it() {
    let loop_ref = read("plugin/refs/spec/resume-loop.md");

    // --- 1. The gesture is named, on the line that presents the plan --------
    let fallback = line_with(&loop_ref, "**Fallback (plan mode unavailable):**")
        .expect("§A no longer carries the plan-mode-unavailable fallback");
    assert!(
        fallback.contains("SELECTING"),
        "the fallback never says that only a SELECTED option mints the marker — an \
         answer typed as free text is the decline this omission produces: {fallback}",
    );
    assert!(
        fallback.contains("approv") && fallback.contains("aprov"),
        "the fallback must name the stems the recorder matches, or the author words \
         the option \"Sim, pode ir\" and the answer records nothing: {fallback}",
    );
    assert!(
        fallback.contains("before the question is answered")
            || fallback.contains("before the plan is presented"),
        "the fallback must say the gesture is named BEFORE the answer is spent — \
         that ordering IS the fix: {fallback}",
    );
    // The one-line way out has to be here too: it is the alternative the operator
    // can still take at this exact moment, and it costs a single prompt.
    assert!(
        fallback.contains("/mustard:spec r") && fallback.contains("WHOLE prompt"),
        "the fallback never names the bare `r` — the one-line way out inside the \
         unit's own branch — under its whole-prompt rule: {fallback}",
    );
    assert!(
        fallback.contains("work branch"),
        "the bare `r` must carry the position it is valid in, or it is taught as a \
         gesture that works anywhere: {fallback}",
    );

    // …and it precedes the options themselves. A reader who meets the menu first
    // has already chosen by the time the rule arrives.
    let fallback_at = loop_ref
        .find("**Fallback (plan mode unavailable):**")
        .expect("checked above");
    let options_at = loop_ref
        .find("**Approve and implement now — wave 1**")
        .expect("§A no longer offers the approve-and-implement option");
    assert!(
        fallback_at < options_at,
        "the gesture must be named ahead of the options it governs (fallback at \
         {fallback_at}, options at {options_at})",
    );

    // --- 2. The recorder still declines the way the paragraph claims --------
    let recorder = read("apps/rt/src/hooks/observe/approval_marker_observer.rs");
    assert!(
        recorder.contains(r#"APPROVAL_STEMS: &[&str] = &["approv", "aprov"]"#),
        "the recorder's stems changed — the fallback now teaches labels it no \
         longer accepts",
    );
    assert!(
        recorder.contains("fn is_offered"),
        "nothing separates a SELECTED label from free text any more, so the \
         fallback's central instruction explains a rule that stopped existing",
    );
}

/// **The picker registers the bare `r`** — the same approval gesture with the
/// letter left out, valid inside the unit's own work branch.
///
/// `picker_approval_observer` mints `<spec>/.approved-by-user` from
/// `/mustard:spec r` submitted as the whole prompt, resolving the spec through
/// the branch the checkout stands on. A door the pages never mention is a door
/// nobody walks through: the operator takes the plan-mode round trip instead,
/// which is the ceremony the form exists to remove.
///
/// The looseness question is asserted deliberately. The form drops the letter,
/// not the rule — one character less to type is not one condition less to meet —
/// and a page that presents it without saying so reads as a weakened gate.
#[test]
fn the_picker_registers_the_bare_r_as_an_approval() {
    let picker = read("plugin/commands/spec.md");

    // --- 1. §1 parses it, with the rule that keeps it unforgeable ----------
    let parse = line_with(&picker, "**`^r$`")
        .expect("§1 never parses the bare `r`, so the picker cannot route the gesture \
                 the observer already honours");
    assert!(
        parse.contains(".approved-by-user"),
        "the bare form is parsed without naming what it mints: {parse}",
    );
    assert!(
        parse.contains("whole prompt") && parse.contains("ENTIRE prompt"),
        "the bare form must carry the whole-prompt rule — a substring match would \
         let a message quoting the form forge the marker: {parse}",
    );
    assert!(
        parse.contains("looser in NOTHING"),
        "dropping the letter is not dropping a condition, and the page has to say \
         so or the form reads as a weakened gate: {parse}",
    );
    assert!(
        parse.contains("work branch"),
        "the bare form must state the ONE position it is valid in: {parse}",
    );
    assert!(
        parse.contains("integration base") || parse.contains("detached HEAD"),
        "the page must say what happens where the tree shows no unit — silence \
         there reads as \"approves something\": {parse}",
    );

    // The letter rule must not keep teaching the opposite about this one letter.
    let letters = line_with(&picker, "**`^[a-z]r?$`**")
        .expect("the picker no longer documents the letter-mode pattern");
    assert!(
        letters.contains("carve-out"),
        "letter mode still reads as though a lone `r` were row `r`, which \
         contradicts the rule one bullet above it: {letters}",
    );

    // --- 2. §3 routes it into the approve gate the same way ----------------
    let plan_route = line_with(&picker, "resume-loop **§A Approve**")
        .expect("the picker no longer routes a Plan-stage spec to §A");
    assert!(
        plan_route.contains("bare `/mustard:spec r`"),
        "§3 never says the bare form arrives with the approval already made, so §A \
         asks for a gesture the user has already given: {plan_route}",
    );

    // --- 3. And the observer really does honour it -------------------------
    let observer = read("apps/rt/src/hooks/observe/picker_approval_observer.rs");
    assert!(
        observer.contains("ApprovalTarget::Checkout"),
        "the picker door no longer resolves a bare `r` through the checkout — the \
         pages now register a gesture nothing mints from",
    );
    assert!(
        observer.contains("slug_of_work_branch"),
        "the bare form's spec is no longer read off the work branch, so \"the branch \
         names the unit\" is a reason the code stopped giving",
    );
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
];
