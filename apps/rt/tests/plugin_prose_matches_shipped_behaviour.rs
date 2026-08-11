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

/// AC-3 — the resume prose reads `insideWorkBranch`, and the engine emits it.
///
/// The work unit is the branch plus everything the work produced, so a caller
/// standing on `{base}_{slug}` is inside the work already. The picker still
/// printed a header and asked *"Implementar agora?"* there — a question about
/// entering a place the caller cannot leave without checking out. Both surfaces
/// a reader arrives at must name the field, and `resume-bootstrap` must really
/// report it: a prose rule pointing at a field nobody emits is a rule that
/// silently never fires.
#[test]
fn resume_inside_own_branch_prose_and_engine_agree() {
    // --- 1. The picker reads it where it routes an EXEC-stage spec --------
    let picker = read("plugin/commands/spec.md");
    let exec_route = line_with(&picker, "resume-loop **§B Loop**")
        .expect("the picker no longer routes an EXEC-stage spec to §B");
    assert!(
        exec_route.contains("insideWorkBranch"),
        "the §B route never reads the field that says the caller is already \
         inside the unit: {exec_route}",
    );
    // Naming the field is not the contract — what it BUYS is.
    for dropped in ["no table", "no header", "Implementar agora?"] {
        assert!(
            exec_route.contains(dropped),
            "the §B route names the field without saying `{dropped}` is dropped: {exec_route}",
        );
    }

    // --- 2. The loop ref teaches the same entry, inside §B ----------------
    let loop_ref = read("plugin/refs/spec/resume-loop.md");
    let no_ceremony = line_with(&loop_ref, "no ceremony")
        .expect("§B never tells the orchestrator what a no-ceremony entry looks like");
    assert!(
        no_ceremony.contains("insideWorkBranch"),
        "the §B entry describes the shortcut without naming its signal: {no_ceremony}",
    );
    let section_at = loop_ref
        .find("## §B — The loop")
        .expect("the loop ref no longer has a §B");
    let entry_at = loop_ref.find("no ceremony").expect("checked above");
    assert!(
        entry_at > section_at && entry_at - section_at < 900,
        "the no-ceremony entry must sit at the TOP of §B, where a resumed \
         session arrives (§B at {section_at}, entry at {entry_at})",
    );

    // --- 3. resume-bootstrap really reports it ----------------------------
    let bootstrap = read("apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs");
    assert!(
        bootstrap.contains("\"insideWorkBranch\""),
        "the prose sends the reader to a field resume-bootstrap does not emit",
    );
    let classifier = read("apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs");
    assert!(
        classifier.contains("fn inside_own_work_branch"),
        "the field has no classifier behind it — it would report `false` forever",
    );
    // The branch NAME is not re-derived here: the same function that minted the
    // pending marker computes it, or the two spellings drift and the shortcut
    // stops firing on exactly the branch it was built for.
    assert!(
        classifier.contains("compute_work_branch"),
        "the classifier re-derives the `{{base}}_{{slug}}` name instead of reusing \
         the one the work-branch marker was minted with",
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

/// The AC-authoring ref teaches the shell the executor actually spawns.
///
/// This one is here because its absence had a cost. The ref taught `cmd.exe`
/// workarounds — `bash -c '…'` prefixes, a list of POSIX constructs to avoid —
/// and that guidance made a defect invisible rather than fixing it: under
/// `cmd.exe` the single quote is not a quote character, so `rg 'token' path`
/// searched for a literal `'token'`, matched nothing in any tree state, exited 1
/// with an empty stderr, and `ac-negative-check` (whose whole red rule is
/// `exit != 0`) stamped it `proven: red`. Nothing checked that page against the
/// executor, so it kept teaching the workaround after the shell was fixed.
#[test]
fn cross_shell_prose_teaches_the_shell_the_executor_spawns() {
    let body = read("plugin/refs/feature/ac-cross-shell.md");

    // --- 1. The prose no longer teaches the routed-around contract ---------
    assert!(
        !body.contains("prefix with `bash -c"),
        "the ref still teaches the explicit `bash -c` workaround the fix removed",
    );
    let lower = body.to_lowercase();
    assert!(
        line_with(&lower, "127").is_some(),
        "the ref must name the code an unrunnable command comes back with",
    );
    // The two readers of exit 127 answer opposite questions and must BOTH be
    // named. A ref that mentions only the negative test's `unproven` reads as if
    // an unrunnable criterion were tolerated at QA — which is the regression
    // that shipped, written down as guidance.
    assert!(
        lower.contains("qa-run") && lower.contains("block"),
        "the ref must say a criterion nobody could run BLOCKS the close",
    );
    assert!(
        lower.contains("unproven"),
        "and that the negative test refuses to count it as proof",
    );

    // --- 2. The shell really consumes POSIX quoting ------------------------
    // Without this half the sentence outlives the selection it describes: the
    // page would keep promising POSIX after a regression put `cmd.exe` back,
    // which is the exact pairing that went missing the first time.
    let out = mustard_rt::util::platform::build_shell_command("echo 'a b'")
        .output()
        .expect("the platform shell spawns");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "a b",
        "the ref promises ordinary POSIX, but the shell handed the quotes to the program",
    );
}

/// The isolation prose teaches the branch the harness actually cuts.
///
/// This one is here because its absence had a cost, and the cost was paid every
/// turn. Wave 2 removed the `.claude/spec/` carve-out from `work_branch_gate`
/// and `spec-draft` began cutting `{base}_{slug}` in the MAIN checkout at
/// approval — the branch became the isolation. The orchestrator's own paragraph
/// kept teaching the opposite ("writes IN-PLACE … on the base branch with NO
/// worktree"), and that file is injected on EVERY user prompt: the router was
/// told the deleted behaviour once per turn while the gate denied it. The
/// paragraphs on either side were rewritten in the same wave; this one was not,
/// and nothing guarded the sentence.
#[test]
fn isolation_prose_teaches_the_branch_cut_at_approval() {
    // --- 1. The shipped seed no longer teaches the deleted carve-out -------
    // The compiled-in seed is what `upsert` lays down in every project, so this
    // reads the text that actually ships.
    let seed = mustard_core::ORCHESTRATOR_MD;
    for deleted in ["writes IN-PLACE", "carves out `.claude/spec/`"] {
        assert!(
            !seed.contains(deleted),
            "the orchestrator seed still teaches `{deleted}` — the carve-out wave 2 removed",
        );
    }

    // Anchored: the paragraph a reader arrives at is the one that computes the
    // unit's branch, not some later section that happens to mention isolation.
    let branch_line = line_with(seed, "compute the unit's `{base}_{slug}` branch")
        .expect("the orchestrator seed no longer says where the unit's branch comes from");
    assert!(
        branch_line.contains("cut at APPROVAL"),
        "the paragraph never says WHEN the branch is cut: {branch_line}",
    );
    assert!(
        branch_line.contains("inPlace"),
        "the paragraph never names what EXECUTE answers when the branch is \
         already out: {branch_line}",
    );
    // Naming the degrade is not teaching it: the worktree has to be shown as
    // the parallel case, or a reader still takes it for the default step.
    assert!(
        branch_line.contains("parallel-work"),
        "the paragraph demotes the worktree without saying what it is FOR: {branch_line}",
    );

    // --- 2. The `/git` ref defines the same contract ----------------------
    let flow = read("plugin/refs/git/git-flow.md");
    assert!(
        !flow.contains("Every unit runs in its OWN worktree"),
        "git-flow.md still teaches one-worktree-per-unit as the arrangement",
    );
    let contract = line_with(&flow, "## Isolation contract")
        .expect("git-flow.md no longer defines the isolation contract");
    assert!(
        contract.contains("parallel"),
        "the contract heading still sells the worktree as the rule: {contract}",
    );

    // --- 3. This repository's delivered copy has not drifted ---------------
    // `seed_injectable_files` PRESERVES an existing file on merge, so editing
    // the template does not update an already-seeded project.
    let delivered = read(".claude/mustard/orchestrator.md");
    let delivered_line = line_with(&delivered, "compute the unit's `{base}_{slug}` branch")
        .expect("the delivered injectable no longer says where the unit's branch comes from");
    assert_eq!(
        delivered_line, branch_line,
        "the delivered .claude/mustard/orchestrator.md drifted from the seed — \
         re-seed it, or this project reads the behaviour it just deleted",
    );

    // --- 4. The code really behaves the way the prose now promises ---------
    // Without this half the sentences above outlive the mechanism: they would
    // keep promising a denial and a degrade after either was reverted.
    let gate = read("apps/rt/src/hooks/write/work_branch_gate.rs");
    assert!(
        !gate.contains("rel.starts_with(\".claude/spec/\") => return Ok(Verdict::Allow)"),
        "the gate carves `.claude/spec/` out again, so the prose promises a \
         denial that never fires",
    );
    let draft = read("apps/rt/src/commands/spec/spec_draft.rs");
    assert!(
        draft.contains("fn cut_work_branch"),
        "spec-draft no longer cuts the unit's branch, so nothing makes the \
         branch the isolation the prose describes",
    );
    let open = read("apps/rt/src/commands/work_unit_open.rs");
    assert!(
        open.contains("checkout_holding_branch") && open.contains("\"inPlace\""),
        "the isolation step no longer reports a branch already checked out — \
         it would fail with exit 128 on the arrangement that is now the default",
    );
}

/// AC-8 — the bugfix prose carries the diagnosis INTO the spec through the
/// material channel, instead of leaving it to be retyped.
///
/// `spec-draft --material` shipped for `/feature` and `/bugfix` never used it,
/// so every DIAGNOSE ended the same way: a located root cause, with its file and
/// line, summarised by hand into prose that dropped both. The channel was not
/// missing — it was undocumented on the one flow whose whole output is a
/// verified finding.
///
/// The same paragraph also names the sanctioned scratch path and the limit that
/// makes it honest, because that carve-out has the identical failure mode: a
/// mechanism nobody is told about is a mechanism nobody takes.
#[test]
fn bugfix_prose_teaches_the_material_channel() {
    let bugfix = read("plugin/commands/bugfix.md");

    // --- 1. The draft call the reader copies passes the channel ------------
    let draft_call = line_with(&bugfix, "run spec-draft")
        .expect("the bugfix prose no longer shows the spec-draft call it makes");
    assert!(
        draft_call.contains("--material"),
        "the spec-draft call omits the channel the diagnosis rides in on: {draft_call}",
    );
    assert!(
        draft_call.contains(".claude/.cache/spec-material.json"),
        "the call names the flag without the file it takes: {draft_call}",
    );

    // --- 2. Assembling comes BEFORE drafting, and names the three kinds ----
    // Order is the whole point: a flow that drafts first invites the retype the
    // channel exists to remove.
    let assemble_at = bugfix
        .find("Assemble the material FIRST")
        .expect("the bugfix prose never tells the flow to assemble before it drafts");
    let draft_at = bugfix.find("run spec-draft").expect("checked above");
    assert!(
        assemble_at < draft_at,
        "the assembly must be taught BEFORE the draft call, or the material is \
         written after the spec it was meant to fill (assemble at {assemble_at}, \
         draft at {draft_at})",
    );
    // …and the ordering warning must describe the mechanism the way `/feature`
    // §2.2 does. "Refused, the flow dead-ends" alone warns of a wall the common
    // path never hits: once the base gate has NAMED the unit, the pending marker
    // makes the auto-branch hook cut the branch on this very write and it lands.
    // Two flows describing one mechanism differently is how a reader learns to
    // trust neither (found in review, 2026-08-11).
    let order = line_with(&bugfix, "Order, said out loud")
        .expect("the bugfix prose no longer states when the unit's branch is cut");
    assert!(
        order.contains("pending marker"),
        "the ordering warning omits the case that actually happens — the marker \
         cutting the branch on this write: {order}",
    );
    assert!(
        order.contains("REFUSED"),
        "…and it must still name the case that IS a dead end: {order}",
    );
    let kinds = line_with(&bugfix, ".claude/.cache/spec-material.json")
        .expect("checked above");
    for kind in ["definitions", "decisions", "findings"] {
        assert!(
            kinds.contains(&format!("`{kind}`")),
            "the material paragraph never says what `{kind}` carries: {kinds}",
        );
    }
    // The root cause is the ONE thing this flow must not lose, so the prose has
    // to say which kind it lands in — otherwise it arrives as loose prose again.
    assert!(
        kinds.contains("root cause"),
        "the paragraph lists the kinds without saying where the located root \
         cause goes: {kinds}",
    );
    assert!(
        bugfix.contains("FAIL-CLOSED"),
        "the prose never warns that the channel aborts the draft instead of \
         degrading to an empty one",
    );

    // --- 3. The weight rule: a demonstrated cause drafts the minimal spec ---
    let weight = line_with(&bugfix, "MINIMAL spec")
        .expect("the bugfix prose states no weight rule for an already-demonstrated cause");
    for kept in ["## Contexto", "## Acceptance Criteria", "## Limites"] {
        assert!(
            weight.contains(&format!("`{kept}`")),
            "the minimal spec must still name `{kept}`: {weight}",
        );
    }
    for dropped in ["## Causa raiz", "## Plano"] {
        assert!(
            weight.contains(&format!("`{dropped}`")),
            "the rule drops sections without naming `{dropped}`, so a reader \
             cannot tell what is being dropped: {weight}",
        );
    }
    // Naming the sections is not the rule — WHEN it applies is, and the
    // argued-cause case has to survive or the rule reads as "always minimal".
    assert!(
        weight.contains("ARGUED"),
        "the rule never says which diagnoses still keep the discovery \
         sections: {weight}",
    );

    // --- 4. The scratch carve-out and the limit that bounds it -------------
    let scratch = line_with(&bugfix, ".claude/scratch/")
        .expect("DIAGNOSE is never told where runnable evidence may be written");
    assert!(
        scratch.contains("cargo does not compile"),
        "the scratch paragraph sells the carve-out without its limit — evidence \
         that must COMPILE cannot live under `.claude/`: {scratch}",
    );

    // --- 5. The mechanisms the prose promises really exist -----------------
    // Without this half every assertion above passes over a deleted channel.
    let cli = read("apps/rt/src/commands/spec/cli.rs");
    assert!(
        cli.contains("material: Option<PathBuf>"),
        "`spec-draft` no longer accepts the `--material` flag the prose passes",
    );
    let draft = read("apps/rt/src/commands/spec/spec_draft.rs");
    assert!(
        draft.contains("fn load_material") && draft.contains("fn append_material_sections"),
        "the draft neither reads the material file nor writes its sections, so \
         the flow would hand over a payload nothing consumes",
    );
    let gate = read("apps/rt/src/hooks/write/work_branch_gate.rs");
    assert!(
        gate.contains("rel.starts_with(\".claude/scratch/\")"),
        "the write gate no longer carves out `.claude/scratch/`, so the prose \
         sends a diagnosis at a path the gate denies",
    );
    assert!(
        gate.contains("context::pending_branch_for"),
        "the gate no longer reads a pending marker, so §3's ordering warning \
         describes a landing nothing performs",
    );
    let ignore = read("packages/core/templates/.gitignore");
    assert!(
        ignore.contains("scratch/"),
        "the seeded ignore no longer hides scratch, so throwaway evidence would \
         reach the diff the prose promises it never reaches",
    );
}

/// AC-9 — the hygiene question fires on a collision, not on every run.
///
/// Step 3 asked whether to continue an in-progress spec unconditionally,
/// including in the case that is by far the most common: the user just asked for
/// the new work, in the same message, and it touches something else entirely.
/// There the answer was already given, the question was answered "no" every
/// time, and a step routinely skipped without consequence teaches the reader to
/// judge every OTHER step of the protocol case by case too.
#[test]
fn hygiene_prose_teaches_the_collision_condition() {
    let hygiene = read("plugin/refs/feature/spec-hygiene.md");

    // --- 1. The condition is stated where step 3 is stated -----------------
    let step_at = hygiene
        .find("3. In-progress specs")
        .expect("the hygiene ref no longer has a step 3 for in-progress specs");
    let ask_at = hygiene
        .find("AskUserQuestion")
        .expect("the hygiene ref no longer describes the question at all");
    assert!(
        ask_at > step_at,
        "the question must be described inside step 3 (step at {step_at}, \
         question at {ask_at})",
    );
    let condition_at = hygiene.find("ask ONLY when").unwrap_or_else(|| {
        panic!("step 3 still asks unconditionally — no condition gates the question")
    });
    assert!(
        condition_at > step_at && condition_at < ask_at,
        "the condition must be read BEFORE the question, not appended after it \
         (condition at {condition_at}, question at {ask_at})",
    );

    // Both triggers must be named. Either one alone is enough to ask, and
    // dropping either turns the step back into something a reader guesses at.
    let step_3 = &hygiene[step_at..];
    assert!(
        step_3.contains("overlap") || step_3.contains("OVERLAP"),
        "the condition never names the collision with the active spec",
    );
    assert!(
        step_3.contains("explicitly requested"),
        "the condition never covers work the pipeline inferred rather than the \
         user asking for it in the same message",
    );

    // --- 2. The silent path is a RECORD, not a silence ---------------------
    // "Proceed quietly" would leave the operator wondering whether the audit ran
    // at all; the one line is what makes the skip auditable.
    assert!(
        step_3.contains("[HYGIENE] spec {name} remains parked"),
        "the no-ask path never records the line that says the spec was left alone",
    );

    // --- 3. The reason is stated, because that is what makes it a rule -----
    assert!(
        step_3.contains("case by case"),
        "the ref makes the step conditional without saying WHY — that a protocol \
         whose steps are routinely skipped teaches the reader to judge every \
         step case by case",
    );

    // --- 4. The ref really reaches both flows, and the parked spec is real --
    // A conditional nobody opens never fires; and "remains parked" is a promise
    // only the harness can keep — it has to tolerate a second active unit.
    for flow in ["plugin/commands/feature.md", "plugin/commands/bugfix.md"] {
        let body = read(flow);
        assert!(
            body.contains("refs/feature/spec-hygiene.md"),
            "{flow} no longer loads the hygiene ref, so its condition reaches no reader",
        );
    }
    let active = read("apps/rt/src/commands/spec/active_specs.rs");
    assert!(
        active.contains("pub specs: Vec<ActiveSpec>"),
        "the harness no longer reports a LIST of active specs, so a spec left \
         parked beside new work would have nowhere to be listed",
    );
}

/// The exit ritual's REFUSAL reaches the operator who has to act on it.
///
/// This unit turned the settle's verdict from a certificate into a gate: a pass
/// that cannot advance the base now prunes nothing, restores an in-place unit to
/// its work branch and names the command to rerun. Every one of those is a
/// mechanism the operator only meets through `pr close` — and the procedure
/// still said "(pull, remove the worktree, delete local + remote branch)",
/// which describes the happy path only. A field emitted for a reader who is
/// never told to read it is the inert half this ratchet file exists to catch
/// (found in review, 2026-08-11, by both reviewers independently).
#[test]
fn settle_refusal_prose_teaches_the_fields_the_gate_now_emits() {
    let git_md = read("plugin/commands/git.md");
    let close = line_with(&git_md, "pr close** — one close per repo")
        .expect("`/git` no longer documents the `pr close` procedure at all");

    // --- 1. The shape of a refusal, and that it touched nothing -------------
    assert!(
        close.contains("base-behind"),
        "the procedure never names the refusal this unit introduces: {close}",
    );
    for promise in ["PRUNES NOTHING", "nextAction", "restoredToUnit"] {
        assert!(
            close.contains(promise),
            "the refusal is described without `{promise}`, so the operator meets \
             it first in raw JSON: {close}",
        );
    }
    // The obstacle names are the whole reason `baseAdvance` exists: without them
    // "base-behind" reads as one situation when it is three, and two of the
    // three are not fixed by cleaning the tree.
    for reason in ["baseAdvance", "dirty-tree", "ahead-of-origin"] {
        assert!(
            close.contains(reason),
            "the prose sends the reader to a verdict without `{reason}`: {close}",
        );
    }
    // The move the operator would otherwise invent is exactly the one the
    // refusal declined to make.
    assert!(
        close.contains("never finish a refused settle by hand"),
        "nothing warns against finishing the ritual manually, which is the \
         improvisation the refusal exists to prevent: {close}",
    );

    // --- 2. The engine really emits every field the prose promises ----------
    // Without this half the paragraph could outlive the mechanism it describes.
    let settle = read("apps/rt/src/commands/git_settle.rs");
    for field in ["\"restoredToUnit\"", "report[\"nextAction\"]", "report[\"baseAdvance\"]"] {
        assert!(
            settle.contains(field),
            "git-settle no longer emits {field}, so `/git` documents a field \
             nobody prints",
        );
    }
    for reason in ["\"base-behind\"", "\"dirty-tree\"", "\"ahead-of-origin\""] {
        assert!(
            settle.contains(reason),
            "git-settle no longer produces the reason {reason} the prose teaches",
        );
    }
}

/// The role names `full-plan.md` declares reserved. `review`/`qa` are one pair
/// of names for one agent, which is why six names spell five reservations.
const RESERVED_ROLES: &[&str] = &["plan", "explore", "review", "qa", "guards", "patterns"];

/// The names that same paragraph offers as ordinary writing roles.
const WRITING_ROLE_EXAMPLES: &[&str] = &["backend", "proof", "discovery", "bootstrap"];
