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
//! They read the `plugin/` prose ONLY. Nothing here asserts the code that
//! implements the shortened path: the two live in different waves, and a prose
//! test that compiles against its sibling's work is a test that cannot be run
//! until the sibling lands.
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
