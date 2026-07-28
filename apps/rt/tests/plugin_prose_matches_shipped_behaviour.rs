// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! A mechanism nobody is told about is a mechanism nobody takes.
//!
//! Three mechanisms shipped in this spec reached no reader: the CONFIRMATION
//! pass (`ac-negative-check --confirm`, now taken by `close-pipeline`), the
//! `neverDispatched` field on `resume-bootstrap`, and the `Onde` column of the
//! active-spec table. Each was emitted by the binary while the operator-facing
//! prose still described the world without it — which is the same defect this
//! spec exists to remove, one layer up: the harness asserting a completeness it
//! had not verified.
//!
//! Every test here reads BOTH halves and requires them to agree:
//!
//! 1. the shipped prose under `plugin/` names the mechanism, at the place a
//!    reader actually arrives at it — not merely somewhere in the file; and
//! 2. the CODE still emits or takes what that prose promises.
//!
//! Half 2 is what makes these more than spell-checks. A prose assertion alone
//! passes forever once the sentence is written, even after the mechanism is
//! deleted; asserting the emitter too means the pair can only be broken
//! together, deliberately.
//!
//! They live in `tests/` rather than in-file because each acceptance criterion
//! runs `cargo test -p mustard-rt <fn>` and libtest matches the FULL test path —
//! which equals the bare function name only at the root of an integration-test
//! binary.

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

/// AC-10 — the CLOSE prose teaches the confirmation pass the pipeline takes.
///
/// The red proof ("this criterion knows how to fail") shipped with prose; the
/// green half shipped as a flag nobody was told existed and nothing called.
/// Both operator-facing docs that describe CLOSE must now carry it, and
/// `close-pipeline` must actually take it — a doc promising a pass that no code
/// runs is the inert half all over again.
#[test]
fn close_prose_teaches_the_confirmation_pass() {
    // --- 1. The loop ref teaches it where it teaches CLOSE ---------------
    let loop_ref = read("plugin/refs/spec/resume-loop.md");
    assert!(
        loop_ref.contains("--confirm"),
        "the loop ref never names the flag that takes the confirmation",
    );
    // Anchored: the paragraph that composes CLOSE is where a reader arrives.
    let close_line = line_with(&loop_ref, "`close-pipeline` composes the CLOSE tail")
        .expect("the loop ref no longer describes what close-pipeline composes");
    assert!(
        close_line.contains("confirmation"),
        "the CLOSE composition still lists only the red half: {close_line}",
    );
    // The three readings are spelled out, including the one that is NOT a
    // verdict — `taken:false` must never be readable as a pass.
    for reading in ["taken:false", "unproven", "advisory"] {
        assert!(
            loop_ref.contains(reading),
            "the confirmation prose never explains `{reading}`",
        );
    }

    // --- 2. The config ref teaches it beside the proof gate ---------------
    let config = read("plugin/pipeline-config.md");
    assert!(
        config.contains("--confirm"),
        "pipeline-config.md documents the RED proof and not the confirmation",
    );
    assert!(
        config.contains("advisory"),
        "pipeline-config.md must say the confirmation adds no refusal to the gate table",
    );

    // --- 3. The mechanism the prose promises is really taken --------------
    // Without this half the assertions above pass over a deleted mechanism —
    // exactly the state the review found: documented nowhere, called nowhere.
    let close_pipeline = read("apps/rt/src/commands/pipeline/close_pipeline.rs");
    assert!(
        close_pipeline.contains("ac_negative_check::confirm_in_process"),
        "close-pipeline does not TAKE the confirmation the prose promises",
    );
}

/// AC-11 — the picker prose carries the `Onde` legend the table now prints.
///
/// `commands/spec.md` §2 orders the Siglas block printed "literally", so that
/// block IS the legend the operator reads. It listed every column but the one
/// that decides whether acting on a row requires switching branch first.
#[test]
fn picker_prose_teaches_the_onde_column() {
    // --- 1. The literal block carries it ---------------------------------
    let picker = read("plugin/commands/spec.md");
    let siglas = line_with(&picker, "**Siglas**")
        .expect("the picker prose no longer carries a Siglas block");
    assert!(
        siglas.contains("Onde"),
        "the Siglas block, printed literally, omits the Onde column: {siglas}",
    );
    assert!(
        siglas.contains("em voo") || siglas.contains("branch"),
        "the legend must say what a non-`-` value MEANS, not just name the column: {siglas}",
    );

    // --- 2. The table really prints that column ---------------------------
    let active_specs = read("apps/rt/src/commands/spec/active_specs.rs");
    assert!(
        active_specs.contains("| Onde"),
        "the legend describes a column the table no longer renders",
    );
}

/// AC-12 — the resume prose names `neverDispatched` beside `currentWave`.
///
/// `currentWave` names a wave; it never claimed one started. The scaffold
/// materialises every wave directory before any agent runs, so "wave 1 of 5"
/// reads identically for a plan in flight and a plan nobody dispatched. The
/// field that separates them is emitted — and the prose still sent the reader
/// to `currentWave` alone.
#[test]
fn resume_prose_teaches_never_dispatched() {
    // --- 1. The prose names it where it names the snapshot ---------------
    let loop_ref = read("plugin/refs/spec/resume-loop.md");
    assert!(
        loop_ref.contains("neverDispatched"),
        "the loop ref never mentions the field that separates never-dispatched from wave 1",
    );
    let snapshot_at = loop_ref
        .find("`currentWave`")
        .expect("the loop ref no longer tells the orchestrator to read currentWave");
    let field_at = loop_ref
        .find("neverDispatched")
        .expect("checked above");
    assert!(
        field_at > snapshot_at && field_at - snapshot_at < 1200,
        "neverDispatched must be taught WITH currentWave, not in an unrelated section \
         (currentWave at {snapshot_at}, neverDispatched at {field_at})",
    );

    // --- 2. resume-bootstrap really emits it ------------------------------
    let bootstrap = read("apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs");
    assert!(
        bootstrap.contains("\"neverDispatched\""),
        "the prose sends the reader to a field resume-bootstrap no longer emits",
    );
}

/// AC-5 — the dispatch prose teaches the precheck SKIP marker beside the `ok`
/// reading.
///
/// On an unsupported stack the dependency gate DECLINES to judge: it answers
/// `ok: true` with an empty scope and a `skipped` reason, and `wave-advance`
/// deliberately carries that marker through its trim. The prose still taught
/// `{ok:true}` → dispatch and nothing else, so the one green that means "nobody
/// looked" reached the operator spelled exactly like the one that means "the
/// symbols are there" — a mechanism with no documented reader.
#[test]
fn dispatch_prose_teaches_the_precheck_skip() {
    // --- 1. The prose teaches it where it teaches the dispatch decision -----
    let loop_ref = read("plugin/refs/spec/resume-loop.md");
    let dispatch_line = line_with(&loop_ref, "check its `precheck`")
        .expect("the loop ref no longer tells the orchestrator to read the precheck");
    assert!(
        dispatch_line.contains("skipped"),
        "the precheck reading omits the skip marker: {dispatch_line}",
    );
    // Naming the key is not teaching it — the line must say what the marker
    // MEANS, which is that the green is not a clearance.
    assert!(
        dispatch_line.contains("DECLINED") || dispatch_line.contains("nobody looked"),
        "the prose names `skipped` without saying the green is not a clearance: {dispatch_line}",
    );

    // --- 2. The marker really survives to the reader ------------------------
    // Without this half the sentence above outlives the mechanism it describes.
    let precheck = read("apps/rt/src/commands/review/dependency_precheck.rs");
    assert!(
        precheck.contains("pub(crate) const SKIPPED_KEY: &str = \"skipped\""),
        "the gate no longer emits the `skipped` key the prose teaches",
    );
    let advance = read("apps/rt/src/commands/pipeline/wave_advance.rs");
    assert!(
        advance.contains("dependency_precheck::skip_reason"),
        "wave-advance trims the marker away again, so no dispatch ever sees it",
    );
}
