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

use mustard_rt::commands::agent::render::recommended_subagent_type;

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

/// AC-6 — the orchestrator's Verdict rule names a MEASUREMENT an agent claims
/// as the second thing never relayed on a briefing alone.
///
/// The rule used to cover one claim only: a runtime symptom the user reported.
/// So an orchestrator following it to the letter relayed "13 of 13 passed"
/// because an agent said so — which happened, and was false. The second half
/// says a measurement is not evidence until the orchestrator takes it itself.
///
/// The counterweight is asserted too, and deliberately: a rule that only added
/// "verify more" would license re-deriving the whole briefing and spending a
/// subagent to double-check one's own work. Both halves must survive together
/// or the sentence teaches the opposite failure.
#[test]
fn orchestrator_prose_teaches_the_measurement_half_of_the_verdict_rule() {
    // --- 1. The shipped seed states both claims, measurement second -------
    // The compiled-in seed is what `upsert` lays down in every project, so
    // this reads the text that actually ships — not a stray copy on disk.
    let seed = mustard_core::ORCHESTRATOR_MD;
    let verdict =
        line_with(seed, "Verdict rule").expect("the orchestrator seed states no Verdict rule");

    let symptom_at = verdict
        .find("runtime symptom")
        .expect("the Verdict rule dropped its first half — the user-reported symptom");
    let measurement_at = verdict.find("MEASUREMENT").unwrap_or_else(|| {
        panic!("the Verdict rule never names a measurement an agent claims: {verdict}")
    });
    assert!(
        measurement_at > symptom_at,
        "the measurement claim must be the SECOND thing the rule refuses to relay, \
         after the reported symptom (symptom at {symptom_at}, measurement at {measurement_at})",
    );

    // Naming it is not teaching it: the line must say what turns the claim
    // into evidence, which is taking the measurement again.
    assert!(
        verdict.contains("take it yourself"),
        "the rule names a claimed measurement without saying who has to take it: {verdict}",
    );
    assert!(
        verdict.contains("re-run the command"),
        "the rule must name the act that settles it — re-running the command: {verdict}",
    );

    // The counterweight, so the rule cannot be read as "verify everything".
    assert!(
        verdict.contains("double-checking your own work"),
        "the rule adds verification without its limit — the rest of a briefing IS \
         the answer, and no subagent re-checks your own work: {verdict}",
    );

    // --- 2. The seed is really the file a session reads -------------------
    // Without this half the sentence is a template nobody is served.
    let project_seed = read("packages/core/src/platform/project_seed.rs");
    assert!(
        project_seed.contains("(\"orchestrator.md\", ORCHESTRATOR_MD)"),
        "nothing seeds orchestrator.md any more, so the rule reaches no window",
    );
    let config = read("packages/core/src/domain/config.rs");
    assert!(
        config.contains(".claude/mustard/orchestrator.md"),
        "the default inject no longer declares the orchestrator injectable",
    );

    // --- 3. This repository's own delivered copy has not drifted -----------
    // `seed_injectable_files` PRESERVES an existing file on merge, so editing
    // the template does not update an already-seeded project. Silent drift is
    // the whole failure mode: the rule would ship to new projects while the
    // one that wrote it kept the old text.
    let delivered = read(".claude/mustard/orchestrator.md");
    let delivered_verdict = line_with(&delivered, "Verdict rule")
        .expect("the delivered injectable states no Verdict rule");
    assert_eq!(
        delivered_verdict, verdict,
        "the delivered .claude/mustard/orchestrator.md drifted from the seed — \
         re-seed it, or this project never reads the rule it just wrote",
    );
}

/// AC-8 — the plan schema names the reserved role names that resolve to
/// read-only agents.
///
/// `role` reads like a free label in the schema example, and it is not: five
/// names pick a tool-restricted agent. A writing wave named `plan` received an
/// agent that physically cannot write while its rendered prompt still said
/// "you implement" — the two halves disagreed and the wave produced nothing.
#[test]
fn plan_prose_teaches_the_reserved_role_names() {
    // --- 1. The prose warns where the schema is documented ----------------
    let plan = read("plugin/refs/feature/full-plan.md");
    let reserved = line_with(&plan, "RESERVED")
        .expect("full-plan.md never warns that some role names are reserved");

    // Anchored: the warning belongs beside the schema a reader is copying,
    // not in an unrelated section further down.
    let schema_at = plan
        .find("\"role\":")
        .expect("full-plan.md no longer shows the wave schema with a role field");
    let reserved_at = plan.find("RESERVED").expect("checked above");
    assert!(
        reserved_at > schema_at && reserved_at - schema_at < 2000,
        "the reserved-role warning must sit with the schema it qualifies \
         (schema at {schema_at}, warning at {reserved_at})",
    );

    for role in RESERVED_ROLES {
        assert!(
            reserved.contains(&format!("`{role}`")),
            "the warning omits the reserved role `{role}`: {reserved}",
        );
    }
    // Naming them is not teaching them: say what the reservation costs.
    assert!(
        reserved.contains("read-only") || reserved.contains("cannot write"),
        "the warning lists names without saying they cannot write: {reserved}",
    );
    for role in WRITING_ROLE_EXAMPLES {
        assert!(
            reserved.contains(&format!("`{role}`")),
            "the warning must show a name that IS a writing role, and `{role}` is gone: {reserved}",
        );
    }

    // --- 2. The dispatch really restricts exactly those names -------------
    // Without this half the paragraph outlives the map it describes — and a
    // reader would trust a reservation the dispatch stopped honouring.
    for role in RESERVED_ROLES {
        assert_ne!(
            recommended_subagent_type(role),
            "general-purpose",
            "the prose calls `{role}` reserved but the dispatch hands it the writing agent",
        );
    }
    for role in WRITING_ROLE_EXAMPLES {
        assert_eq!(
            recommended_subagent_type(role),
            "general-purpose",
            "the prose calls `{role}` a writing role but the dispatch restricts it",
        );
    }
}

/// The role names `full-plan.md` declares reserved. `review`/`qa` are one pair
/// of names for one agent, which is why six names spell five reservations.
const RESERVED_ROLES: &[&str] = &["plan", "explore", "review", "qa", "guards", "patterns"];

/// The names that same paragraph offers as ordinary writing roles.
const WRITING_ROLE_EXAMPLES: &[&str] = &["backend", "proof", "discovery", "bootstrap"];
