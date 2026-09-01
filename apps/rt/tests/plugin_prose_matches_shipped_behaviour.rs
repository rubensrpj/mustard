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
use mustard_rt::commands::event::enrichment_gap::ENRICHMENT_STALE_TAG;

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

/// The PRODUCTION half of a Rust source file — everything before its
/// `#[cfg(test)]` module.
///
/// A negative assertion ("this sentence is gone from the shipped code") must
/// not be answered by the test that quotes the sentence in order to forbid it,
/// nor by the doc comment explaining what was removed. Whole-file `contains`
/// is right for the POSITIVE half and wrong for this one.
fn production_half(rel: &str) -> String {
    let body = read(rel);
    match body.find("#[cfg(test)]") {
        Some(at) => body[..at].to_string(),
        None => body,
    }
}

/// Where that line SITS — the only way to assert an order between two rows.
/// `line_with` answers whether a row exists, which is what let a re-ordering
/// of the unit's question pass every ratchet it had.
fn line_index(body: &str, needle: &str) -> Option<usize> {
    body.lines().position(|l| l.contains(needle))
}

/// The options a `  label:` row of the unit's question OFFERS, in order, with
/// the label stripped — columns are separated by three or more spaces, so a
/// single-space phrase like `…ou o seu` stays ONE entry instead of three.
fn offered_options<'a>(row: &'a str, label: &str) -> Vec<&'a str> {
    let (_, rest) = row.split_once(label).unwrap_or(("", row));
    rest.split("   ").map(str::trim).filter(|s| !s.is_empty()).collect()
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
/// standing on the unit's own branch is inside the work already. The picker still
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
    // The branch NAME is not rebuilt here — it is READ, which is what the prose
    // now promises. Rebuilding would need one guess per declared base and one
    // per work KIND, and the two spellings drifting is what made this shortcut
    // answer `false` from inside the very branch it was built for.
    assert!(
        classifier.contains("slug_of_work_branch"),
        "the classifier rebuilds the branch name instead of reading the slug off \
         the branch the checkout is actually on",
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
    // `seed_injectable_files` rewrites the file, but only when it RUNS: editing
    // the template alone leaves this repository's committed copy behind until
    // an install or an update lays the new body down. Silent drift is the whole
    // failure mode: the rule would ship to new projects while the one that
    // wrote it kept reading the old text.
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
/// and `spec-draft` began cutting the unit's branch in the MAIN checkout at
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
    // The isolation paragraph rides the router's SECOND half (`dispatch.md`,
    // § Dispatch) since the two-event split — read the seed that carries it,
    // or this ratchet asserts the absence of a sentence from the wrong file.
    let seed = mustard_core::DISPATCH_MD;
    for deleted in ["writes IN-PLACE", "carves out `.claude/spec/`"] {
        assert!(
            !seed.contains(deleted),
            "the dispatch seed still teaches `{deleted}` — the carve-out wave 2 removed",
        );
    }

    // Anchored: the paragraph a reader arrives at is the one that computes the
    // unit's branch, not some later section that happens to mention isolation.
    // The needle carries the SHAPE, so a return to the base-prefixed name — or
    // any other spelling of the join — moves this anchor off the paragraph.
    let branch_line = line_with(seed, "compute the unit's `{kind}/{slug}` branch")
        .expect("the dispatch seed no longer says where the unit's branch comes from");
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
    // The seeder rewrites the file, but only when it RUNS: editing the template
    // alone leaves this repository's committed copy behind.
    let delivered = read(".claude/mustard/dispatch.md");
    let delivered_line = line_with(&delivered, "compute the unit's `{kind}/{slug}` branch")
        .expect("the delivered injectable no longer says where the unit's branch comes from");
    assert_eq!(
        delivered_line, branch_line,
        "the delivered .claude/mustard/dispatch.md drifted from the seed — \
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

    // --- 5. `EnterWorktree name=` really reaches a hook that TAKES the name --
    // The prose teaches the `name=` hand-off as the route, so the hook face has
    // to accept the shape this project mints. It once refused every `/`, which
    // refused every unit — and it refused BEFORE the degrade above could answer,
    // so a non-zero exit ended the whole `EnterWorktree` instead of entering the
    // checkout that already held the branch.
    let cli = line_with(&flow, "**Foreground CLI**")
        .expect("git-flow.md no longer says how a foreground unit is isolated");
    assert!(
        cli.contains("EnterWorktree name={kind}/{slug}"),
        "the ref teaches a route around the `name=` hand-off instead of the \
         hand-off itself — prose that documents a defect: {cli}",
    );
    assert!(
        open.contains("fn unusable_worktree_name")
            && open.contains("WorkKind::is_container_segment(head)"),
        "the hook face judges the name by a hand-spelled rule again (or refuses \
         every separator), so `EnterWorktree name={{kind}}/{{slug}}` dies with \
         exit 1 and takes the isolation step with it",
    );
    assert!(
        !open.contains("`name` must not contain a path separator"),
        "the blanket separator refusal is back — it refuses every unit this \
         project mints, before the in-place degrade can answer",
    );
}

/// The router prose teaches the branch named by the KIND, and the ONE question
/// that decides it.
///
/// The code half of this shipped first and could not reach a user on its own:
/// `compute_work_branch` emits `{kind}/{slug}` and `resolve_kind_base` derives
/// the base from the declared flow instead of parsing it back out of the name —
/// but `--type` is passed by NOBODY unless the router asks for it, so every unit
/// silently takes the default and the question the operator asked for never
/// happens. The prose IS the feature here; without it the change is a prefix
/// that altered its spelling and nothing else.
///
/// So both halves are read together: the question a reader arrives at in the
/// router's own § Dispatch, and the mechanism each of its promises rests on.
#[test]
fn router_prose_teaches_the_kind_named_branch_and_its_one_question() {
    // --- 1. The shipped seed asks ONE pre-marked question ------------------
    // The compiled-in seed is what `upsert` lays down in every project. The
    // question lives in the router's SECOND half since the two-event split.
    let seed = mustard_core::DISPATCH_MD;

    let kind_row = line_with(seed, "  tipo:")
        .expect("the router seed shows no `tipo` row — the kind is never asked");
    for kind in ["feature", "fix", "hotfix"] {
        assert!(kind_row.contains(kind), "the kind row omits `{kind}`: {kind_row}");
    }
    assert!(
        kind_row.contains("[fix]"),
        "the kind row lists the options without PRE-MARKING one, so the question \
         costs a decision instead of an Enter: {kind_row}",
    );
    assert!(
        line_with(seed, "  sai de:").is_some(),
        "the question never offers the base, so a hotfix cannot be aimed at one",
    );
    let name_row = line_with(seed, "  branch:")
        .expect("the question never shows the name the branch is about to get");
    assert!(
        name_row.contains("fix/"),
        "the name shown to the operator does not carry the `{{kind}}/` prefix the \
         branch is actually cut with: {name_row}",
    );

    // The rules that make it one question instead of a form.
    //
    // The old ratchet demanded the router call `hotfix` a DESTINATION rather
    // than a kind of work. That sentence existed because the kind DERIVED the
    // base, so `hotfix` was the only way to say "not the ordinary route". With
    // the base chosen against a real list, the kind derives nothing and the
    // distinction has no work left to do — so the ratchet now checks the rule
    // that replaced it: the vocabulary is open.
    let rules = line_with(seed, "OPEN label")
        .expect("the router never says the branch type is an open label");
    assert!(
        rules.contains("git ref segment"),
        "the type is called open without saying what the actual limit is — \
         anything that can be a ref segment: {rules}",
    );
    assert!(
        rules.contains("no longer moves the base"),
        "the router leaves `hotfix/` looking like it still steers where the \
         unit lands: {rules}",
    );
    assert!(
        rules.contains("ONE branch"),
        "the router asks a question that may have a single possible answer — \
         with one candidate the base is not asked at all: {rules}",
    );
    assert!(
        rules.contains("ONCE per unit"),
        "the router never says the question is asked once and never again: {rules}",
    );

    // The answer has to REACH the gate, or the question decides nothing.
    let emit = line_with(seed, "run emit-pipeline --kind pipeline.kind")
        .expect("the router no longer shows the base-gate emit");
    assert!(
        emit.contains("--type"),
        "the dispatch call drops the answer to the question it just asked, so \
         every unit takes the default kind: {emit}",
    );

    // --- 2. This repository's delivered copy has not drifted ---------------
    // The seeder rewrites the file, but only when it RUNS: editing the template
    // alone leaves this repository's committed copy behind.
    let delivered = read(".claude/mustard/dispatch.md");
    for row in ["  tipo:", "  branch:", "run emit-pipeline --kind pipeline.kind"] {
        assert_eq!(
            line_with(&delivered, row),
            line_with(seed, row),
            "the delivered .claude/mustard/dispatch.md drifted from the seed \
             at `{row}` — re-seed it, or this project asks the old question",
        );
    }

    // --- 3. The code really behaves the way the question promises ----------
    // Without this half the block above outlives its mechanism: it would keep
    // showing `fix/…` after the join, the flow-derived base or the refusals
    // went away.
    let kinds = read("apps/rt/src/shared/work_kind.rs");
    // `WorkKind` is a newtype over a validated token now, not an enum of three:
    // the join is what this ratchet cares about, and it is unchanged.
    assert!(
        kinds.contains("struct WorkKind") && kinds.contains("format!(\"{}/{slug}\", self.0)"),
        "nothing builds `{{kind}}/{{slug}}` any more, so the name the question \
         showed is not the name the branch gets",
    );
    assert!(
        kinds.contains("SUGGESTED") && !kinds.contains("enum WorkKind"),
        "the type went back to a closed set, so the question's open vocabulary \
         promises something the code refuses",
    );
    let branch = read("apps/rt/src/commands/event/work_branch.rs");
    assert!(
        branch.contains("kind.branch_name(&slug)"),
        "compute_work_branch spells the join itself instead of taking WorkKind's \
         — two spellings of one name is what this module exists to prevent",
    );
    // The base is the OPERATOR's answer, offered from a real catalogue. The two
    // assertions this replaced demanded the opposite — that the kind imply the
    // base, and that a hotfix have its own candidate set — and both were only
    // meaningful while `sai de` chose between two entries in a config file.
    let catalogue = read("packages/core/src/platform/git_branches.rs");
    assert!(
        catalogue.contains("pub fn branch_catalog") && catalogue.contains("for-each-ref"),
        "nothing lists the branches the repository really has, so `sai de` has \
         no menu to offer",
    );
    assert!(
        catalogue.contains("pub fn protected_branches"),
        "nothing answers WHICH branches refuse a direct commit, so the picker \
         cannot mark the row the operator must not land on",
    );
    // Both refusals the prose promises, read through the messages that carry
    // them: delete either and the operator is silently coerced instead.
    assert!(
        branch.contains("fn resolve_kind_base"),
        "the gate has no resolver validating the kind/base pair the router sends",
    );
    assert!(
        branch.contains("não existe no remoto deste repositório"),
        "a --base the remote does not have is no longer refused, so the router's \
         promise that it is becomes a lie the operator only meets on a wrong branch",
    );
    // The hotfix-off-the-work-base refusal is GONE, deliberately, and this
    // ratchet now guards its absence. It was only ever a contradiction while
    // the kind implied the base; with the base picked outright, refusing it
    // would be the tool overruling the operator's own answer.
    assert!(
        !branch.contains("um hotfix não sai de"),
        "the hotfix-off-the-work-base refusal came back, but the router no longer \
         promises it — the operator would be refused for an answer they were asked to give",
    );
    let emit_src = read("apps/rt/src/commands/event/emit_pipeline.rs");
    assert!(
        emit_src.contains("work_branch::resolve_kind_base"),
        "emit-pipeline stopped calling the resolver, so nothing validates the pair",
    );
    let cli = read("apps/rt/src/commands/event/cli.rs");
    assert!(
        cli.contains("#[arg(long = \"type\")]"),
        "`--type` is not a flag of emit-pipeline, so the router's call fails",
    );
    // Old names keep working — a unit in flight must not be orphaned.
    assert!(
        kinds.contains("fn legacy_base_of"),
        "the `{{base}}_{{slug}}` shape no longer resolves, so every unit in flight \
         loses its base, its PR target and its second-unit refusal",
    );

    // --- 4. The operator's PICK survives the cut ---------------------------
    // The question above is worth asking only if its answer outlives the marker
    // that carried it. With three bases the branch name cannot say which one was
    // chosen, and the marker is consumed at the cut — so the cut writes the
    // answer into the unit's own record, and every later read prefers it.
    let git_flow = read("plugin/refs/git/git-flow.md");
    let durable = line_with(&git_flow, "The operator's pick is DURABLE")
        .expect("the /git ref never says the chosen base outlives the cut");
    for taught in ["meta.json#base", "consumed", "ambiguous-base"] {
        assert!(
            durable.contains(taught),
            "the paragraph omits `{taught}` — it must say WHERE the answer is \
             kept, WHY the marker cannot keep it, and what happens when nothing \
             was recorded: {durable}",
        );
    }
    assert!(
        kinds.contains("enum UnitBase") && kinds.contains("Ambiguous(Vec<String>)"),
        "nothing can answer `I cannot know which base this unit came from`, so \
         the outermost candidate is served as a fact again",
    );
    assert!(
        kinds.contains("fn record_cut_base") && kinds.contains("fn recorded_base_of"),
        "the cut no longer writes the chosen base down (or nothing reads it \
         back), so the pick dies with the pending marker",
    );
    let meta = read("packages/core/src/domain/meta.rs");
    assert!(
        meta.contains("pub base: Option<String>"),
        "the unit's own record has no home for the base it was cut from — a \
         second file would then have to be invented for one field",
    );
    let scaffold = read("apps/rt/src/commands/spec/spec_scaffold.rs");
    assert!(
        scaffold.contains("meta.base = read_meta(&path).and_then(|existing| existing.base)"),
        "the spec scaffold overwrites the sidecar wholesale again, which erases \
         the one answer nothing else can reconstruct",
    );
    for door in [
        "apps/rt/src/hooks/write/work_branch_gate.rs",
        "apps/rt/src/commands/event/work_branch.rs",
    ] {
        assert!(
            read(door).contains("record_cut_base"),
            "{door} cuts the branch without recording the base it cut from, so \
             the answer depends on which door opened the unit",
        );
    }

    // --- 5. …and it is written where the DRAFT can still write ------------
    // The cut runs FIRST, and a `meta.json` in the unit's directory is exactly
    // what tells `spec-draft` a spec is already drafted there. Recording the
    // base that way made step one refuse step two: the unit came out cut and
    // spec-less on the one path this record exists to serve. So the cut writes
    // harness state and the draft folds it into the sidecar — prose, guard and
    // fold asserted together, because any one of them alone reopens it.
    assert!(
        durable.contains(".cut-base"),
        "the /git ref teaches the cut writing the sidecar itself — the write that \
         left the unit cut and spec-less: {durable}",
    );
    assert!(
        kinds.contains("pub(crate) const CUT_BASE_FILE"),
        "nothing names the cut's own record any more, so the cut is back to \
         writing the one file the draft reads as somebody's draft",
    );
    let draft = read("apps/rt/src/commands/spec/spec_draft.rs");
    assert!(
        draft.contains("const HARNESS_STATE_ENTRIES") && draft.contains("work_kind::CUT_BASE_FILE"),
        "the draft's guard no longer tolerates the cut's own record BY NAME, so \
         the first step of the sequence blocks the second again",
    );
    assert!(
        scaffold.contains("work_kind::cut_base_in(output)")
            && scaffold.contains("work_kind::clear_cut_base_in(output)"),
        "the draft stopped folding the cut's record into `meta.json#base` (or \
         stopped retiring it), so the answer ends up with two homes or none",
    );

    // Where NOTHING recorded it, nobody is handed a guess. `base_for` used to
    // answer the outermost candidate with a `WARN` on stderr — which a
    // PreToolUse hook says to nobody, since it exits 0.
    assert!(
        !branch.contains("Falling back to"),
        "the base resolver guesses the outermost candidate again, and a guess \
         nobody can see is a fact",
    );
    let gate = read("apps/rt/src/hooks/write/work_branch_gate.rs");
    assert!(
        gate.contains("workbranch.base.unknown"),
        "the gate stopped SAYING it cannot know which base the emergency came \
         from, so the unit is cut somewhere nobody chose",
    );
    assert!(
        gate.contains("recorded_base.as_deref()"),
        "the reconcile drops the operator's recorded base again while it \
         corrects the branch — the retried cut then has nothing to read",
    );
}

/// The question asks WHERE the unit starts before WHAT it is called.
///
/// The ratchet above demands both rows EXIST and says nothing about their
/// order, so shipping `tipo` above `sai de` broke no test — and a type read
/// first makes the base look like its consequence, which is the implication
/// this product removed the day the base began being chosen against a real
/// catalogue.
///
/// Prose-only, deliberately: the row order is a RENDERING decision and no
/// emitter can be asked whether the block was drawn in it. The mechanism half
/// — that the base is MEASURED rather than derived from the type — is already
/// ratcheted by `router_prose_teaches_the_kind_named_branch_and_its_one_question`,
/// and duplicating it here would assert the wrong thing twice.
#[test]
fn router_asks_the_base_before_the_type() {
    let seed = mustard_core::DISPATCH_MD;
    let delivered = read(".claude/mustard/dispatch.md");

    for (label, body) in [("the seed", seed), ("the delivered copy", delivered.as_str())] {
        let base = line_index(body, "  sai de:")
            .unwrap_or_else(|| panic!("{label} shows no `sai de` row — the base is never asked"));
        let kind = line_index(body, "  tipo:")
            .unwrap_or_else(|| panic!("{label} shows no `tipo` row — the type is never asked"));
        assert!(
            base < kind,
            "{label} shows `tipo` above `sai de`, so the base reads as a consequence \
             of the type — the implication a real catalogue removed",
        );
    }

    // Both rows still open on a pre-marked answer: an Enter accepts, and the
    // re-order must not cost the operator a decision it never used to cost.
    for (row, marked) in [("  sai de:", "[dev]"), ("  tipo:", "[fix]")] {
        let line = line_with(seed, row).unwrap_or_else(|| panic!("no `{row}` row"));
        assert!(
            line.contains(marked),
            "`{row}` lists its options without PRE-MARKING one, so the re-ordered \
             question costs two decisions instead of two Enters: {line}",
        );
    }

    // Say WHY, or the order is a coincidence the next editor tidies away.
    let why = line_with(seed, "`sai de` FIRST")
        .expect("the router never says the base is asked first — nothing stops a re-order");
    assert!(
        why.contains("before what it is CALLED"),
        "the order is stated without its reason: the operator settles where the unit \
         STARTS before what it is called: {why}",
    );
}

/// The rows are independent fields, the surface has a ceiling, and `hotfix`
/// survives it.
///
/// Two silences in the router produced one defect. "Ask both together" never
/// said the fields are INDEPENDENT, so the question came back as pre-paired
/// options (`fix saindo de dev` / `hotfix saindo de main`) — the cartesian
/// product of two choices, which has no row at all for a `hotfix` cut from the
/// ordinary base. And the prose never named the surface's ceiling of four
/// options, so the renderer dropped a suggestion to fit — and the one it
/// dropped was `hotfix`, the row's whole reason for existing.
#[test]
fn router_forbids_pairing_and_pins_hotfix() {
    let seed = mustard_core::DISPATCH_MD;

    let rule = line_with(seed, "INDEPENDENT fields")
        .expect("the router never says the rows are independent fields");
    assert!(
        rule.contains("cartesian product"),
        "independence is asserted without naming what pairing actually hands back — \
         the product of two choices, in which one combination has no row: {rule}",
    );
    assert!(
        rule.contains("4 options"),
        "the prose never names the ceiling of the question surface, so the reader \
         discovers it by getting it wrong in front of the operator: {rule}",
    );
    assert!(
        rule.contains("PINNED"),
        "nothing forbids dropping `hotfix` to fit the ceiling — the exact suggestion \
         that fell out last time: {rule}",
    );

    // The block obeys its own rule: four options plus the free field, `hotfix`
    // among them, and no row spelling a pair.
    let kind_row = line_with(seed, "  tipo:").expect("the router seed shows no `tipo` row");
    let offered = offered_options(kind_row, "tipo:");
    let (free, options) = offered
        .split_last()
        .expect("the `tipo` row offers nothing at all");
    assert!(
        free.starts_with('…'),
        "the `tipo` row does not end in the free field, so a type the list omits \
         cannot be typed: {kind_row}",
    );
    assert!(
        options.len() <= 4,
        "the `tipo` row offers {} options over a surface that takes 4 — the renderer \
         will drop one, and the prose does not get to choose which: {kind_row}",
        options.len(),
    );
    assert!(
        options.contains(&"hotfix"),
        "`hotfix` is not among the offered types, so an emergency cannot be named \
         from the question: {kind_row}",
    );
    let base_row = line_with(seed, "  sai de:").expect("the router seed shows no `sai de` row");
    let base_offered = offered_options(base_row, "sai de:");
    let (base_free, base_options) = base_offered
        .split_last()
        .expect("the `sai de` row offers nothing at all");
    assert!(
        base_free.starts_with('…'),
        "the `sai de` row has no free field, so a catalogue longer than the ceiling \
         hides the branches that did not fit: {base_row}",
    );
    assert!(
        base_options.len() <= 4,
        "the `sai de` row offers {} options over a surface that takes 4: {base_row}",
        base_options.len(),
    );
    for row in [kind_row, base_row] {
        assert!(
            !row.contains("saindo de"),
            "a row of the question spells a PAIR, which is the defect itself — the \
             operator who wants `hotfix` off the ordinary base finds no line: {row}",
        );
    }

    // The code half: the chooser's suggestions are where a renderer takes its
    // four from, so `hotfix` has to survive that truncation there too.
    let kinds = read("apps/rt/src/shared/work_kind.rs");
    assert!(
        kinds.contains("[\"feature\", \"fix\", \"hotfix\", \"chore\""),
        "`hotfix` fell past the fourth SUGGESTED token, so anything taking the first \
         four to fit the surface drops it — exactly the pin the prose promises",
    );
}

/// The name is OFFERED for correction, and the correction reaches the gate.
///
/// "A unit has one name" was written against the CALLER that invented one in
/// silence, and it silenced the OPERATOR too — the only person who knows what
/// the unit should be called. So the `branch` row stops being a notice and
/// becomes a field: suggestion plus edit. The signal that carries the edit is
/// deliberately distinct from `--spec` (still a guess, still losing), because
/// a deliberate correction and a silent invention must never read alike.
#[test]
fn router_offers_the_name_for_correction() {
    let seed = mustard_core::DISPATCH_MD;

    // --- 1. The row is a field, not a read-out ----------------------------
    let name_row = line_with(seed, "  branch:")
        .expect("the question never shows the name the branch is about to get");
    assert!(
        name_row.contains("[fix/"),
        "the suggested name is not pre-marked, so accepting it costs a decision \
         instead of an Enter: {name_row}",
    );
    assert!(
        name_row.contains('…'),
        "the `branch` row is shown as a notice — nothing tells the operator the name \
         can be corrected, which is the whole point of showing it: {name_row}",
    );

    // --- 2. …and editing it edits `tipo` + name in ONE string -------------
    let field = line_with(seed, "CORRECTABLE field")
        .expect("the router never says the `branch` row is a correctable field");
    for taught in ["ONE string", "first `/`", "no third", "silence still means derived"] {
        assert!(
            field.contains(taught),
            "the paragraph omits `{taught}` — it must say that the edit is `tipo` + \
             name together, how it splits, that there is no free-standing third name, \
             and what an untouched row means: {field}",
        );
    }

    // --- 3. The correction REACHES the gate, and only when it happened -----
    let append = line_with(seed, "--unit-name {name}")
        .expect("the dispatch never shows how the operator's correction is passed");
    assert!(
        append.contains("ONLY when"),
        "the flag is shown unconditionally, so a derived name is announced as the \
         operator's and `nameFrom` lies on every unit: {append}",
    );
    let flag = line_with(seed, "`--unit-name` is the operator's correction")
        .expect("the router never explains what `--unit-name` outranks");
    for taught in ["only when", "--spec", "nameFrom", "derived-from-intent", "operator"] {
        assert!(
            flag.contains(taught),
            "the flag paragraph omits `{taught}` — it must say when it is passed, that \
             `--spec` still loses, and which values report WHO named the unit: {flag}",
        );
    }

    // --- 4. The code really takes the signal and really reports the source --
    let cli = read("apps/rt/src/commands/event/cli.rs");
    assert!(
        cli.contains("#[arg(long = \"unit-name\")]"),
        "`--unit-name` is not a flag of emit-pipeline, so the router's corrected \
         dispatch fails on a name the operator was invited to give",
    );
    let emit_src = read("apps/rt/src/commands/event/emit_pipeline.rs");
    assert!(
        emit_src.contains("\"derived-from-intent\"") && emit_src.contains("\"operator\""),
        "the gate no longer names WHO chose the unit's name, so the two cases the \
         explicit signal exists to separate become indistinguishable again",
    );
    assert!(
        emit_src.contains("done[\"nameFrom\"]"),
        "the report stopped emitting `nameFrom`, so the router promises a field no \
         reader ever receives",
    );
}

/// AC-9 — the worktree prose teaches the REFUSAL and the reaper, and teaches no
/// environment declaration.
///
/// The environment-carrying design was withdrawn after review: `link` planted a
/// Windows directory junction inside the worktree, and `git worktree remove`
/// DESCENDS a junction — so closing a unit deleted the MAIN checkout's
/// `node_modules`, with and without `--force`. The shipped prose told operators
/// to declare exactly that, which made following the documentation the way to
/// lose your dependencies. What remains is a refusal (commit or stash, then open
/// the second unit) and the orphan collector, which creates nothing and only
/// reaps the worktrees Claude Code cuts on its own.
#[test]
fn worktree_prose_teaches_the_refusal_and_the_reaper() {
    let flow = read("plugin/refs/git/git-flow.md");

    // --- 1. The refusal is taught where the gate's decision is taught -------
    let another = line_with(&flow, "ANOTHER unit's branch, with uncommitted work")
        .expect("the gate prose never says what happens when the checkout holds another unit");
    assert!(
        another.contains("REFUSED") || another.contains("denied"),
        "the row must say the edit is REFUSED: {another}",
    );
    // Naming the refusal is not teaching it — the row must name the act that
    // unblocks it, or the operator is stopped with nowhere to go.
    assert!(
        another.contains("commit or stash"),
        "the row refuses without naming what unblocks it: {another}",
    );
    assert!(
        another.contains("paths"),
        "the row must promise the paths holding the work are named: {another}",
    );
    // The counterweights, deliberately: a rule that only added "refuse more"
    // would stop on a HEAD nobody measured, and on a clean tree where nothing
    // can ride along.
    let unmeasured = line_with(&flow, "detached / unreadable HEAD")
        .expect("the gate prose no longer says what an unreadable HEAD does");
    assert!(
        unmeasured.contains("in-place"),
        "an unmeasured position must keep today's cut: {unmeasured}",
    );
    let clean = line_with(&flow, "ANOTHER unit's branch, tree CLEAN")
        .expect("the gate prose never says what a CLEAN checkout does");
    assert!(
        clean.contains("in-place"),
        "a clean checkout loses nothing, so it must keep the cut: {clean}",
    );

    // --- 2. No environment declaration survives anywhere in the ref ---------
    for withdrawn in ["mustard.json#worktree", "\"carry\"", "\"link\"", "**`carry`**", "**`link`**"]
    {
        assert!(
            !flow.contains(withdrawn),
            "the ref still teaches `{withdrawn}` — following it plants a junction \
             whose removal deletes the main checkout's directory",
        );
    }

    // --- 3. The reaper, which is NOT withdrawn ------------------------------
    let reaper = line_with(&flow, "The collector reaps what is ORPHANED")
        .expect("the contract never says what the collector does");
    for taught in ["--apply", "uncommitted", "PID"] {
        assert!(
            reaper.contains(taught),
            "the reaper paragraph omits `{taught}` — it acts, it refuses over work, \
             and it knows an orphan by its owner: {reaper}",
        );
    }

    // --- 4. The code really does each of them -------------------------------
    // Without this half every sentence above outlives its mechanism.
    let branch = read("apps/rt/src/commands/event/work_branch.rs");
    assert!(
        branch.contains("fn holds_other_work") && branch.contains("fn busy_checkout"),
        "nothing asks whether the checkout holds another unit's uncommitted work",
    );
    assert!(
        branch.contains("CutOutcome::Refused"),
        "the cut `spec-draft` takes at approval — the door that opens FIRST — no \
         longer refuses, so the gate's guard is again the only one",
    );
    assert!(
        branch.contains("fn checkout_work") && !branch.contains("dirty_paths(root)"),
        "the refusal measures with the CUT-blind probe again — the one that drops \
         the unit's own `.claude/spec/…` and reads a failed measurement as clean, \
         which is how a second unit took a checkout holding another unit's whole \
         spec, waves and proof while `git status` named all three",
    );
    let gate = read("apps/rt/src/hooks/write/work_branch_gate.rs");
    assert!(
        gate.contains("busy_checkout(Path::new(&local)"),
        "the gate no longer takes the shared refusal, so the two doors can disagree",
    );
    assert!(
        !gate.contains("hook_create"),
        "the gate cuts a worktree again — the divert the prose says is withdrawn",
    );
    for nudge in ["EnterWorktree path=", "EnterWorktree name="] {
        assert!(
            !gate.contains(nudge),
            "the gate still answers `{nudge}` — it sends the session into a worktree \
             nobody asked for, or nudges it there on every unit",
        );
    }

    let open = read("apps/rt/src/commands/work_unit_open.rs");
    for gone in ["fn carry_environment", "fn link_dir", "mklink /J", "worktree.linked()"] {
        assert!(
            !open.contains(gone),
            "`{gone}` is back: a fresh cut plants something of the harness's own, \
             and a link inside a worktree is what `git worktree remove` descends",
        );
    }

    let gc = read("apps/rt/src/commands/maint/worktree_gc.rs");
    assert!(
        gc.contains("gc(repo, DEFAULT_AGE_DAYS, /* apply = */ true)"),
        "the SessionStart probe went back to dry-run, so nothing is ever collected",
    );
    assert!(
        gc.contains("process_liveness") && gc.contains("enum Contents"),
        "the collector lost either the owner probe that makes it prompt or the \
         work probe that makes it safe",
    );
    assert!(
        !gc.contains("dirty_paths(&wt)"),
        "the collector decides by the CUT decision's probe again — the one that \
         reads a failed measurement as clean and drops the candidate's own \
         `.claude/` contents, which is how an --apply sweep deleted unsaved files",
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
    // The anchor is the CLAIM, not a slogan. It used to be the sentence
    // "Order, said out loud: …", which /feature §2.2 carried word for word —
    // one wording maintained in two files is how the two drift apart. Both
    // flows now point at `dispatch.md`, which states the order once, and each
    // states the claim in its own words; what this ratchet still holds is that
    // the paragraph names BOTH outcomes of the write.
    let order = line_with(&bugfix, "This write happens AFTER the base gate")
        .expect("the bugfix prose no longer states when the unit's branch is cut");
    assert!(
        order.contains("dispatch.md"),
        "the ordering claim must point at the file that states the order once, \
         or the rule gets restated here and drifts: {order}",
    );
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
    let close = line_with(&git_md, "finish** — one close per repo")
        .expect("`/git` no longer documents the exit ritual (`finish`, once `pr close`) at all");

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

/// Nothing refuses an operation because a branch is absent from the
/// PRE-SELECTED list.
///
/// `git.flow` used to answer two questions at once — "where may a unit be cut
/// from?" and "where is a direct commit forbidden?" — and six places read it to
/// REFUSE. The installer writes no flow at all, so every one of those refusals
/// fired on a correct installation, telling the operator that a branch their
/// project really integrates through "is not an integration base of this
/// project" and offering an edit to a configuration file as the way out.
///
/// This ratchet holds the three command doors to the measured questions: is
/// this branch somebody's WORK UNIT, and is it PROTECTED. Both are facts the
/// repository can answer; membership in a list nobody maintains is not.
#[test]
fn nothing_refuses_for_absence_from_the_preselected_list() {
    // --- 1. The sentence, and the exit it offered, are gone -----------------
    // Read the PRODUCTION half: the tests below each file quote the removed
    // sentence in order to forbid it, and a negative assertion answered by its
    // own guard proves nothing.
    let open = production_half("apps/rt/src/commands/work_unit_open.rs");
    let pr = production_half("apps/rt/src/commands/review/pr_door.rs");
    let delete = production_half("apps/rt/src/commands/git_delete.rs");
    for (name, body) in
        [("work_unit_open", &open), ("pr_door", &pr), ("git_delete", &delete)]
    {
        assert!(
            !body.contains("is not an integration base of this"),
            "{name} still pronounces a verdict about a configuration file over a \
             repository nobody asked about",
        );
        assert!(
            !body.contains("Declare it in mustard.json"),
            "{name} still offers editing the configuration as the way past a refusal \
             — the one exit this design removed",
        );
    }

    // --- 2. Neither door may decide by the pre-selected list -----------------
    //
    // This block used to assert the PRESENCE of the exact expressions each door
    // was written with. That is not a criterion: it certifies code presence, not
    // effectiveness, and it was measured GREEN while reporting "git delete
    // decides what it may remove by a declared list again" — with the shipped
    // code deleting a real `release/2026-Q3` from the remote. A presence
    // assertion can only ever pin the line it was written against, defect
    // included.
    //
    // What is asserted here now is an ABSENCE, which cannot certify a defect:
    // the pre-selected list must not appear in either door at all. The positive
    // half — that the right branches are refused and the right ones deleted — is
    // proven by driving the doors and reading the refs afterwards, in
    // `git_delete::tests::a_slashed_integration_base_is_never_deleted_and_never_refused`.
    for (name, body) in [("pr_door", &pr), ("git_delete", &delete)] {
        // The CALL form, never the bare name: both files legitimately explain in
        // prose why they do not use it, and a check that cannot tell a call from
        // a comment forbids writing down the reason.
        for forbidden in ["preselected_bases()", "flow.bases()"] {
            assert!(
                !body.contains(forbidden),
                "{name} consults `{forbidden}` again. With no `git.flow` written — what \
                 the installer produces today — that set is the hardcoded {{main, master}}, \
                 so the door would decide by two literals and nothing else",
            );
        }
        assert!(
            body.contains("has_unit_record"),
            "{name} no longer asks the project for its RECORD of the unit, so it is back \
             to deciding by the shape of a name — and `release/2026-Q3` has the shape of \
             a unit",
        );
    }
    // The target a refusal names must not end at a LITERAL nobody measured.
    //
    // The first shape of this check forbade `primary_base()` outright, and that
    // was too blunt in a way that locked in a regression: `primary_base()` only
    // floors to the hardcoded `main` when NO flow is declared — with a flow it
    // returns the project's own stated base. Forbidding it wholesale sent a unit
    // that integrates into `dev` off to `main`, measured in a repo whose flow
    // says exactly that, and the ratchet then held the wrong answer in place.
    //
    // So what is required is the ORDER, not the absence of a source: the
    // declared base is consulted only behind a declared-flow guard, and
    // `origin/HEAD` remains the last resort.
    for (name, body) in [("pr_door", &pr), ("git_delete", &delete)] {
        assert!(
            body.contains("mustard_core::default_branch("),
            "{name}'s refusal no longer falls back to origin/HEAD, so the base it \
             names can be a literal nobody measured",
        );
        assert!(
            !body.contains("primary_base()") || body.contains("declared_bases().is_empty()"),
            "{name} names the base through `primary_base()` with no declared-flow \
             guard — unguarded it ends at the hardcoded `main` for every project \
             whose install wrote no flow",
        );
    }

    // --- 3. The legacy shape is resolved by the CATALOGUE -------------------
    let work_kind = read("apps/rt/src/shared/work_kind.rs");
    assert!(
        work_kind.contains("fn legacy_base_of") && work_kind.contains("with_remote_names(project"),
        "the `{{base}}_{{slug}}` shape is read against the declaration alone again, \
         so a unit whose base the flow never named is orphaned",
    );
}

/// The doctor does not ask for a `git.flow` the installer no longer writes, and
/// reports what is REALLY protected.
///
/// The check warned that `git.flow` was empty and prescribed declaring one.
/// `project_seed` seeds an EMPTY flow on purpose — the project decides later —
/// so the warning fired on every correct installation, and the claim it made
/// (`only main/master are protected`) was false in front of
/// `protected_branches`. A diagnostic that fires on a healthy install teaches
/// the operator to ignore diagnostics.
#[test]
fn doctor_does_not_ask_for_a_flow_that_the_installer_no_longer_writes() {
    let doctor = production_half("apps/rt/src/commands/doctor/doctor.rs");

    // --- 1. The prescription is gone ----------------------------------------
    assert!(
        !doctor.contains("git.flow is empty"),
        "the doctor still reports the shape a correct install has as a finding",
    );
    assert!(
        !doctor.contains("fix: declare the flow in mustard.json"),
        "the doctor still prescribes declaring a flow to get protection it does not \
         grant",
    );

    // --- 2. What replaced it is the measurement, RUN not read ----------------
    //
    // This half used to grep `doctor.rs` for the function name and the call it
    // makes — the practice AC-3 forbids by name, and for the reason five review
    // rounds kept demonstrating: a source-substring assertion certifies that a
    // line is present, never that the behaviour holds. So the check is executed
    // against a real project in the installed shape (no `git.flow` written) and
    // the assertions are about its OUTPUT.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git")
    };
    // A REAL origin, because the thing under test is what the doctor measures.
    // Without a remote it answers, correctly, that `origin/HEAD` is unreadable
    // and protection fell back to its literals — an honest answer to a
    // different question, and asserting against it would pin the fallback
    // instead of the measurement.
    let upstream = dir.path().join("upstream.git");
    std::process::Command::new("git")
        .args(["init", "-q", "--bare"])
        .arg(&upstream)
        .output()
        .expect("bare origin");
    git(&["init", "."]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    git(&["checkout", "-b", "producao"]);
    std::fs::write(root.join("mustard.json"), r#"{"git":{"provider":"github"}}"#).expect("cfg");
    git(&["add", "-A"]);
    git(&["commit", "-m", "seed"]);
    git(&["remote", "add", "origin", &upstream.to_string_lossy()]);
    git(&["push", "-q", "-u", "origin", "producao"]);
    git(&["remote", "set-head", "origin", "producao"]);

    // `doctor` has no `--root`: it reads the project from the working directory,
    // so the test must STAND in the temp project rather than name it.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "doctor", "--check", "branch-protection"])
        .current_dir(root)
        .output()
        .expect("doctor runs");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "`doctor --check branch-protection` does not run — the name the operator is \
         told to type is not the name the binary answers to: {said}",
    );
    assert!(
        !said.contains("git.flow") || !said.to_lowercase().contains("declare"),
        "the doctor still asks a correct install to declare a flow: {said}",
    );
    assert!(
        said.contains("producao"),
        "the doctor does not report the branch it really protects — with no flow \
         written, protection rests on the remote's own default: {said}",
    );

    // --- 3. The installer really writes no flow -----------------------------
    // Without this half the assertions above outlive their reason: they are
    // right only while an empty flow is the INSTALLED shape.
    let seed = read("packages/core/src/platform/project_seed.rs");
    assert!(
        seed.contains("git.flow starts empty — the project decides"),
        "the installer seeds a flow again, which would make the removed warning \
         meaningful and this whole check wrong",
    );
}

/// The `/git` reference teaches the MEASURED model, not the erased one.
///
/// `/git` orders this file read, so whatever it says IS the model the operator
/// and the agent work from. It taught integration bases derived from the flow, a
/// table where the kind decided the base, a PR target resolved through the flow,
/// and a refusal for standing on an integration base — four mechanisms the code
/// no longer has.
#[test]
fn the_git_reference_teaches_the_measured_model() {
    let flow = read("plugin/refs/git/git-flow.md");

    // --- 1. The flow is named as a PRE-SELECTION ----------------------------
    let preselects = line_with(&flow, "PRE-SELECTS")
        .expect("the reference never says what `git.flow` actually decides");
    assert!(
        preselects.contains("refuses nothing"),
        "the reference names the flow without saying it refuses nothing: {preselects}",
    );
    assert!(
        preselects.contains("installer writes no flow"),
        "a reader following this file still believes a flow must be declared: \
         {preselects}",
    );

    // --- 2. Where each answer really comes from -----------------------------
    let forbidden = line_with(&flow, "Where is a direct commit forbidden?")
        .expect("the reference never says where protection is measured");
    assert!(
        forbidden.contains("origin/HEAD") && forbidden.contains("git.protected"),
        "protection is taught without the set that measures it: {forbidden}",
    );
    let cut = line_with(&flow, "Where may a unit be cut from?")
        .expect("the reference never says where the cut point comes from");
    assert!(
        cut.contains("every branch") && cut.contains("origin"),
        "the cut point is still taught as a declared list: {cut}",
    );

    // --- 3. The kind does NOT decide the base -------------------------------
    assert!(
        flow.contains("The prefix does NOT decide the base"),
        "the reference still lets the kind imply where a unit came from",
    );
    assert!(
        !flow.contains("| `hotfix/` | a correction that does NOT wait for that route | an integration base"),
        "the kind→base table is back — the inference this design removed",
    );
    let step0 = line_with(&flow, "still exists on `origin`")
        .expect("the reference no longer says what the recorded base is checked against");
    assert!(
        step0.contains("recorded with the unit")
            && step0.contains("never whether `git.flow` lists it"),
        "the record is taught without WHAT is checked on the way out, which is the \
         whole repair: {step0}",
    );

    // --- 4. Protection is not "being on a base" -----------------------------
    let protection = line_with(&flow, "one of the **protected** branches")
        .expect("the reference no longer states the write-op refusal");
    assert!(
        protection.contains("origin/HEAD") && protection.contains("git.protected"),
        "the refusal is still taught as 'you are on an integration base': {protection}",
    );
    // The refusal WORDING is not a program literal. The reference used to quote
    // `Cannot operate directly on protected branch '<branch>'. Create a work
    // branch first.` in backticks, and that string exists nowhere in the
    // binary — it lived only in the .md and in the anchor this assertion read
    // back OUT of the same .md, so no change to the code could ever have failed
    // it. Guard the removal instead: the rule is checked above, the wording
    // belongs to whichever gate composes the refusal.
    assert!(
        !flow.contains("Cannot operate directly on protected branch"),
        "the invented refusal literal is back — no such string exists in the \
         binary, and citing it here checks the reference against itself",
    );

    // --- 5. The code really works that way ----------------------------------
    // Without this half every sentence above outlives its mechanism.
    let branches = read("packages/core/src/platform/git_branches.rs");
    assert!(
        branches.contains("pub fn protected_branches") && branches.contains("pub fn branch_catalog"),
        "the two measured answers the reference teaches are no longer the ones the \
         product computes",
    );
    let config = read("packages/core/src/domain/config.rs");
    assert!(
        config.contains("refuses anything any more"),
        "`preselected_bases` no longer documents itself as a pre-selection",
    );
    assert!(
        !config.contains("fn integration_bases"),
        "the deprecated forward is back, and with it the name that says the set \
         permits something",
    );
    let gate = read("apps/rt/src/commands/event/base_gate.rs");
    assert!(
        gate.contains("NO membership test any more"),
        "the base gate judges membership of a declared list again",
    );
}

/// The router's rule for the base gate's enrichment signal is written against
/// the literal the gate really prints.
///
/// The two halves sit in different files by necessity: `mustard-rt` emits the
/// line, and the seeded orchestrator prose is what tells a session what to do
/// on reading one. A tag typed TWICE — once in the emitter, once in the prose —
/// is exactly the pair that drifts in silence: reword the constant and the
/// router keeps watching for a string nothing writes any more, with nothing
/// red. So the prose is asserted against the COMPILED constant, never a copy
/// of it.
#[test]
fn the_router_prose_names_the_signal_the_gate_emits() {
    // --- 1. The seed quotes the tag the emitter exports -------------------
    let seed = read("packages/core/templates/mustard/orchestrator.md");
    let signal = line_with(&seed, ENRICHMENT_STALE_TAG).unwrap_or_else(|| {
        panic!(
            "the orchestrator seed never names the signal the base gate emits \
             ({ENRICHMENT_STALE_TAG:?}), so a session reading it has no rule for it",
        )
    });

    // Anchored: the rule belongs where the router is told how to orient in the
    // census, not in an unrelated section a reader never reaches with it.
    let locating = line_index(&seed, "## Locating code")
        .expect("the orchestrator seed no longer has a `## Locating code` section");
    let efficiency = line_index(&seed, "## Efficiency")
        .expect("the orchestrator seed no longer has an `## Efficiency` section");
    let signal_at = line_index(&seed, ENRICHMENT_STALE_TAG).expect("checked above");
    assert!(
        signal_at > locating && signal_at < efficiency,
        "the enrichment signal is documented outside `## Locating code` \
         (section at {locating}, signal at {signal_at}, next section at {efficiency})",
    );

    // Naming the signal is not teaching it: the rule must say what the line
    // MEANS and what the operator is offered, or it reads as trivia.
    assert!(
        signal.contains("`scan`"),
        "the rule names the signal without offering the flow that closes it: {signal}",
    );
    assert!(
        signal.contains("unit of its OWN"),
        "the rule offers `scan` without saying it is a unit of its own — which is \
         the whole reason it cannot be taken mid-unit: {signal}",
    );
    assert!(
        signal.contains("clean tree"),
        "the rule omits the premise the enrich pass rewrites versioned files under: {signal}",
    );
    assert!(
        signal.contains("current unit closes"),
        "the rule never says WHEN the unit is dispatched, so it reads as \"do it now\": {signal}",
    );

    // --- 2. This repository's delivered copy has not drifted ---------------
    // The seeder rewrites the file, but only when it RUNS: editing the template
    // alone leaves the project that wrote the rule reading the old text.
    let delivered = read(".claude/mustard/orchestrator.md");
    assert_eq!(
        line_with(&delivered, ENRICHMENT_STALE_TAG),
        Some(signal),
        "the delivered .claude/mustard/orchestrator.md drifted from the seed — \
         re-seed it, or this project never reads the rule it just wrote",
    );

    // --- 3. The flow is DISCOVERED by the same measured signal -------------
    // The description is what makes the fallback reachable at all; while it
    // says "visibly stale" the trigger is a guess, not the line above.
    let scan = read("plugin/commands/scan.md");
    assert!(
        scan.contains(ENRICHMENT_STALE_TAG),
        "the scan flow's description names no measured trigger, so the fallback \
         is chosen on a hunch instead of on the line the gate printed",
    );

    // --- 4. The gate really emits it --------------------------------------
    // Without this half every assertion above outlives its mechanism: the
    // prose would keep teaching a signal nothing prints.
    let emit = production_half("apps/rt/src/commands/event/emit_pipeline.rs");
    assert!(
        emit.contains("enrichment_gap::report_if_stale"),
        "the pipeline-opening emit no longer reports the enrichment gap, so the \
         line the router waits for is never written",
    );
}

/// AC-13 — the shipped router teaches the delivery model this unit MEASURED,
/// and the delivered copy matches the seed.
///
/// Two claims used to be wrong in the prose, and both were load-bearing. The
/// router said sibling hooks share one ceiling (they do not — measured
/// 2026-08-25 with two 6,000-character siblings, both intact), and it attributed
/// that limit to Claude Code rather than to Mustard's own last-writer-wins
/// dispatcher fold. A maintainer reading either sentence would re-derive the
/// two-event split this unit undid.
#[test]
fn router_teaches_the_self_healing_delivery() {
    // The wrong claims are gone from the shipped seeds.
    for seed in [mustard_core::ORCHESTRATOR_MD, mustard_core::DISPATCH_MD] {
        for retired in ["two events", "sessionStart", "share one ceiling"] {
            assert!(
                !seed.contains(retired),
                "the router still teaches `{retired}`, the model the measurement replaced",
            );
        }
    }

    // The rationale moved to a ref, and the router points at it rather than
    // carrying the argument in every window.
    let orchestrator = mustard_core::ORCHESTRATOR_MD;
    assert!(
        orchestrator.contains("router-rationale.md"),
        "the router drops the rationale without saying where it went",
    );

    // SIZE IS NOT THE TEST for whether a unit opens.
    //
    // Measured in the field, 2026-08-26: a one-line fix was classified `Simple`
    // — a row that dispenses the PIPELINE — and the model stretched that
    // exemption to the opening question too, committing straight onto
    // `release`. The operator had to ask why nothing was asked. Both halves now
    // say which half `Simple` exempts, and this holds them to it.
    let orchestrator_simple = mustard_core::ORCHESTRATOR_MD;
    assert!(
        orchestrator_simple.contains("dispenses the PIPELINE, never the question"),
        "the Simple row lets the opening question be skipped for a small edit",
    );
    let dispatch_unit = mustard_core::DISPATCH_MD;
    assert!(
        dispatch_unit.contains("Every request that edits a file opens a unit"),
        "the dispatch half never says WHAT counts as a unit, so a small edit reads as none",
    );

    // The rule the conversation channel depends on rides the dispatch half.
    let dispatch = mustard_core::DISPATCH_MD;
    assert!(
        dispatch.contains("spec-material.json"),
        "the dispatch half never says a settled decision is written down when settled",
    );

    // The ref carries the measurement itself, with its date — a claim a reader
    // can check rather than take on faith.
    let rationale = read("plugin/refs/mustard/router-rationale.md");
    assert!(
        rationale.contains("2026-08-25") && rationale.contains("6,000"),
        "the rationale asserts the ceiling model without the measurement behind it",
    );

    // The delivered copies are the seeds — a project reading a stale router is
    // reading rules this repository no longer ships.
    for (delivered, seed) in [
        (".claude/mustard/orchestrator.md", mustard_core::ORCHESTRATOR_MD),
        (".claude/mustard/dispatch.md", mustard_core::DISPATCH_MD),
    ] {
        assert_eq!(
            read(delivered).replace("\r\n", "\n"),
            seed.replace("\r\n", "\n"),
            "the delivered {delivered} drifted from the seed — re-run `mustard-rt run upsert`",
        );
    }
}

// ---------------------------------------------------------------------------
// The two bootstrap twins under `plugin/bin/`.
//
// `mustard-boot.cmd` runs only under cmd.exe, and this block used to claim no
// runner here could execute it. That claim was FALSE, and it cost six releases:
// `.github/workflows/ci.yml`'s `test` job matrix carries `windows-latest`, and a
// `#[cfg(windows)]` test can hand the script to cmd.exe and read the exit code.
// So the subject itself is asked, once, in
// `windows_boot_really_parses_under_cmd` — and the models below are what stays
// checkable on the other two operating systems, pinned to the literals they
// rest on so a model and its subject can only drift together.
//
// The stakes are why they exist at all. This pair has already shipped one
// Windows-only defect that survived six releases in total silence (the trailing
// backslash of 0.1.52, documented at the top of the `.cmd`), and both properties
// below fail the same way: nothing on screen, `bin/` never populated, the whole
// harness dormant.
// ---------------------------------------------------------------------------

/// One field of a cmd `for /f` line after `%~` expansion: surrounding double
/// quotes are dropped, and ONLY when the field carries both of them — a field
/// like `"https` (what the manifest's `$schema` line yields once `:` is a
/// delimiter) is left exactly as it is.
fn cmd_unquote(field: &str) -> &str {
    match field.strip_prefix('"').and_then(|f| f.strip_suffix('"')) {
        Some(inner) if field.len() >= 2 => inner,
        _ => field,
    }
}

/// A model of cmd's `for /f "usebackq tokens=1,2 delims=:, "` over one line:
/// cut on colon, comma or space, treat a run of them as ONE cut, ignore the
/// leading ones, and hand back the first two fields as `%~a` / `%~b`.
fn cmd_for_f_tokens(line: &str) -> (Option<&str>, Option<&str>) {
    let mut fields = line
        .split([':', ',', ' '])
        .filter(|f| !f.is_empty())
        .map(cmd_unquote);
    (fields.next(), fields.next())
}

/// The whole of what `mustard-boot.cmd` computes into `VER`: walk the manifest
/// line by line, skip what cmd's default `eol=;` would skip, and take `%~b` off
/// the FIRST line whose `%~a` is `version` (`if not defined VER` — first match
/// wins).
fn cmd_boot_version(manifest: &str) -> Option<&str> {
    manifest
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.starts_with(';'))
        .find_map(|l| {
            let (key, value) = cmd_for_f_tokens(l);
            key.filter(|k| k.eq_ignore_ascii_case("version"))
                .and(value)
        })
}

/// The version `mustard-boot.cmd` will read out of the committed manifest is the
/// version the manifest actually declares.
///
/// The failure this guards is silent and total. An empty `VER` sends the Windows
/// boot to `:noversion` and leaves `bin/` unpopulated, so after a plugin version
/// bump every hook on every Windows machine goes dormant and stays dormant. The
/// parse has no other alarm: no exit code, no missing file, nothing in the
/// session but hooks that quietly do nothing.
///
/// Two halves, so the model cannot rot away from its subject:
///
/// 1. the `.cmd` still carries the exact `for /f` line modelled above; and
/// 2. the model, run over the real `plugin/.claude-plugin/plugin.json`, agrees
///    with `serde_json` — which parses that file by a completely different road.
///
/// What it does NOT prove is that cmd.exe tokenises the way this models it; only
/// a Windows box can answer that, and the one-line command that asks lives in
/// the doc comment of `every_batch_file_carries_no_percent_sequence_cmd_will_refuse`
/// below. It may NOT live in the `.cmd`: a percent sequence in a comment there
/// aborts the whole file, which is the defect that test exists to keep shut.
/// What it does prove is the half that actually moves: the manifest's shape.
/// That shape is what a careless reformat, a new key, or a minifier in the
/// release path would change.
#[test]
fn windows_boot_reads_the_version_the_manifest_actually_ships() {
    let boot_cmd = read("plugin/bin/mustard-boot.cmd");
    let shipped_line = r#"for /f "usebackq tokens=1,2 delims=:, " %%a in ("%MANIFEST%") do if not defined VER if /i "%%~a"=="version" set "VER=%%~b""#;
    assert!(
        boot_cmd.contains(shipped_line),
        "mustard-boot.cmd no longer carries the `for /f` line this test models, \
         so the model below proves nothing about the shipped parser",
    );

    let manifest = read(MANIFEST_PATH);
    let declared: serde_json::Value =
        serde_json::from_str(&manifest).expect("plugin.json is not valid JSON");
    let declared = declared
        .get("version")
        .and_then(serde_json::Value::as_str)
        .expect("plugin.json declares no `version`");

    let parsed = cmd_boot_version(&manifest).unwrap_or_else(|| {
        panic!(
            "mustard-boot.cmd would read an EMPTY version out of plugin.json — \
             every Windows machine goes dormant after the next version bump. \
             The manifest must keep `version` alone on its own line, as \
             `  \"version\": \"{declared}\",`"
        )
    });
    assert_eq!(
        parsed, declared,
        "mustard-boot.cmd would stamp bin/ with a version the manifest does not \
         declare, so the download URL points at a release that does not exist",
    );

    // The PROMPT form of that same line lives in this file's own doc comment,
    // because a percent sequence in the `.cmd` aborts it. Nothing else ties the
    // two together: in the old layout they sat three lines apart, and the
    // adjacency WAS the anchor (found in review). Pin the half that drifts —
    // the `for /f` options — so the line an operator copy-pastes on Windows
    // cannot go on reporting a PASS for a parse the script no longer performs.
    let options = shipped_line
        .split('"')
        .nth(1)
        .unwrap_or_else(|| panic!("the shipped `for /f` line carries no quoted options"));
    // The needle is the DOC-COMMENT prefix, not the one-liner's own text: a
    // plain substring search matched the source line that performs the search.
    // `file!()` rather than the path spelled out — a literal path panics with
    // "unreadable" instead of the assertion the moment the file is renamed.
    let this_file = read(file!());
    let prompt: Vec<&str> = this_file
        .lines()
        .filter(|l| l.trim_start().starts_with("/// for /f"))
        .collect();
    assert_eq!(
        prompt.len(),
        1,
        "exactly one doc comment may carry the copy-pasteable one-liner, or this \
         anchor pins whichever came first and the real one drifts unchecked",
    );
    let prompt_line = prompt[0];
    // BOTH halves that can rot: the options decide the tokenisation, the path
    // decides whether the operator parses a file that still exists.
    for pinned in [options, &manifest_in_cmd_backslashes()] {
        assert!(
            prompt_line.contains(pinned),
            "the one-liner an operator copy-pastes on a Windows box no longer \
             carries `{pinned}`, so it would report a PASS for a parse the \
             script does not perform:\n{prompt_line}",
        );
    }
}

/// The manifest, in the ONE spelling the whole file reads it by.
const MANIFEST_PATH: &str = "plugin/.claude-plugin/plugin.json";

/// The same path as the copy-pasteable one-liner spells it, in cmd's own
/// backslashes — DERIVED, so moving the manifest cannot leave a third spelling
/// pinned to the old place (found in review).
fn manifest_in_cmd_backslashes() -> String {
    MANIFEST_PATH.replace('/', "\\")
}

/// `defaultEnabled` and `displayName` stay in the manifest, and this records WHY.
///
/// The manifest's own `$schema` points at
/// `https://json.schemastore.org/claude-code-plugin-manifest.json`, and that
/// schema defines **neither** key at the root (`displayName` appears there only
/// inside `channels`). So an editor validating this file against the schema it
/// declares flags both. Checked against the source outside this repository on
/// 2026-09-01: the official Claude Code plugin reference
/// (`code.claude.com/docs/en/plugins-reference`) documents BOTH in its metadata
/// table — `displayName` as the human-readable name shown in the `/plugin`
/// picker, `defaultEnabled` as "whether the plugin starts in an enabled state
/// when the user has not set one", defaulting to `true`. The published schema is
/// therefore behind the product; the keys are real.
///
/// That divergence is worth pinning rather than "cleaning up", because the
/// cleanup is not cosmetic: dropping `"defaultEnabled": false` does not remove a
/// field, it flips the documented default, and Mustard would auto-enable itself
/// on every marketplace installation that has never expressed a preference. A
/// schema that lags the product is not a reason to change what the product does.
#[test]
fn the_manifest_keeps_the_two_keys_its_schema_does_not_define() {
    let manifest: serde_json::Value =
        serde_json::from_str(&read(MANIFEST_PATH)).expect("plugin.json is not valid JSON");
    assert_eq!(
        manifest.get("defaultEnabled").and_then(serde_json::Value::as_bool),
        Some(false),
        "`defaultEnabled: false` is gone — the plugin now auto-enables on every \
         marketplace install that never chose. It is absent from the declared \
         $schema but documented by the product; see this test's doc comment",
    );
    assert!(
        manifest
            .get("displayName")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.is_empty()),
        "`displayName` is gone — the /plugin picker falls back to the bare \
         `name`. Also absent from the declared $schema, also documented by the \
         product",
    );
}

/// Every prose pointer to the batch-file guard names a test that EXISTS.
///
/// This class already bit once, inside this very unit: the guard was renamed,
/// two doc comments and the pull request's own validation step kept the old
/// name, and the published "run this to see it fail" filter matched zero tests
/// and exited 0 — a proof that proved nothing, on the defect whose whole story
/// is a false claim nobody could check. Cheap to pin, so pin it.
#[test]
fn the_prose_names_a_guard_that_exists() {
    const GUARD: &str = "every_batch_file_carries_no_percent_sequence_cmd_will_refuse";
    let this_file = read(file!());
    assert!(
        this_file.contains(&format!("fn {GUARD}()")),
        "the guard named all over this repository is not defined here",
    );
    for (file, body) in [
        (file!(), &this_file),
        ("plugin/bin/mustard-boot.cmd", &read("plugin/bin/mustard-boot.cmd")),
    ] {
        assert!(
            body.contains(GUARD),
            "{file} points readers at the batch-file guard without naming it, \
             so the pointer cannot be followed and cannot be checked",
        );
    }
}

/// The runner that makes the subject askable at all.
///
/// Everything this unit added rests on one line of `ci.yml`, and nothing pinned
/// it. Trim the matrix to cut runner minutes and `windows_boot_really_parses_under_cmd`
/// stops running, both changed files go back to claiming something false, and CI
/// stays green — which is the exact silence this unit exists to end.
#[test]
fn the_ci_matrix_still_carries_the_windows_runner() {
    let ci = read(".github/workflows/ci.yml");
    assert!(
        ci.contains("windows-latest"),
        "no `windows-latest` in .github/workflows/ci.yml, so nothing asks \
         cmd.exe anything and the Windows guards are decoration",
    );
}

/// Every `%~` in `body` that cmd.exe would REFUSE, as `(line, excerpt)`.
///
/// An illegal one does not misbehave — it ABORTS the file. cmd expands percent
/// sequences BEFORE it notices a line is a `rem`, so a percent-tilde written to
/// ILLUSTRATE the parser is evaluated as a reference to an argument by that
/// name. There is none; cmd prints "The following usage of the path operator in
/// batch parameter substitution is invalid", stops reading and exits 255.
/// Measured on a real Windows box, 2026-08-31: the illustrative one-liner sat in
/// the `.cmd` as a comment and killed the boot ABOVE the version read — every
/// Windows install dormant from 0.1.59 to 0.1.61, nothing on screen saying why.
///
/// Read LEFT TO RIGHT, the way cmd reads, because looking backwards cannot
/// work. The first draft asked "is the previous character a percent?", then "is
/// the run of previous percents odd?" — and both answers are wrong for
/// `%VAR%%~a`, where the percent in front of the tilde is the one CLOSING an
/// expansion, not the one escaping the tilde. The heuristic called that legal;
/// cmd aborts on it. The mirror image, `%DIR%~about`, it called illegal; cmd
/// accepts it. One left-to-right pass answers both, and it is not a heuristic:
/// it consumes the same four things cmd consumes.
///
/// - `%%` — an escaped percent. Consume both and read on.
/// - `%~…` — an argument reference, the ONLY shape that can abort. The
///   modifiers are `f d p n x s a t z`, read CASE-INSENSITIVELY (`%~DP0` is
///   legal), and a digit or `*` must close them. `%~$VAR:n`, the documented
///   PATH-search form, closes on its own digit. Nothing closes `%~b`.
/// - `%VAR%` — an expansion. Consume it WHOLE, so its closing percent can never
///   be read as the opening of the next sequence.
/// - a lone `%` — cmd leaves it alone, and so does this.
fn illegal_percent_tilde(body: &str) -> Vec<(usize, String)> {
    body.lines()
        .enumerate()
        .flat_map(|(idx, line)| {
            illegal_in_line(line).into_iter().map(move |at| {
                // The excerpt cannot outrun the line: a blind character window
                // bled three CRLF lines into one message (found in review).
                (idx + 1, line[at..].chars().take(24).collect::<String>())
            })
        })
        .collect()
}

/// Byte offsets of the percent sequences in ONE line that cmd.exe would refuse.
fn illegal_in_line(line: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            Some(b'%') => i += 2,
            Some(b'~') => {
                if !closes_on_an_argument(&line[i + 2..]) {
                    out.push(i);
                }
                i += 2;
            }
            // `%1`…`%9`, `%0` and `%*` are argument references, and cmd consumes
            // them in TWO characters — they have no closing percent to look for.
            // Reading one as the opening of an expansion swallows everything up
            // to the next percent, `%~b` included (found in review).
            Some(c) if c.is_ascii_digit() || *c == b'*' => i += 2,
            _ => {
                i = match line[i + 1..].find('%') {
                    Some(rel) => i + 1 + rel + 1,
                    None => i + 1,
                };
            }
        }
    }
    out
}

/// Does the text after a `%~` name an argument cmd can actually resolve?
fn closes_on_an_argument(rest: &str) -> bool {
    const MODIFIERS: &str = "fdpnxsatz";
    let closes = |c: char| c.is_ascii_digit() || c == '*';
    if let Some(search) = rest.strip_prefix('$') {
        // The variable NAME, not "everything up to the first colon anywhere on
        // the line" — that read `%~$FOO bar:1` as a legal search form, spaces
        // and all (found in review). cmd documents this one closing on a digit.
        let name: String = search
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        return !name.is_empty()
            && matches!(
                search[name.len()..].strip_prefix(':'),
                Some(tail) if tail.starts_with(|c: char| c.is_ascii_digit())
            );
    }
    matches!(
        rest.chars().find(|c| !MODIFIERS.contains(c.to_ascii_lowercase())),
        Some(c) if closes(c)
    )
}

/// One truth table, TWO judges — and that is the whole design.
///
/// Every entry is a batch line and whether cmd.exe refuses it. The model
/// ([`illegal_percent_tilde`]) is checked against this table on every operating
/// system; on Windows, `windows_boot_really_parses_under_cmd` feeds the SAME
/// table to a real cmd.exe. Two review rounds shipped a hole each because the
/// model was only ever asserted against itself: every "cmd accepts this" label
/// was a belief. Now a belief that drifts from cmd turns the Windows leg red.
///
/// Each line here was a hole in some draft of the scanner, and a guard with
/// holes is worse than none: it says the class is closed while the exact shape
/// that cost six releases walks through.
const PERCENT_CASES: &[(&str, bool)] = &[
    // Accepted by cmd.
    (r#"set "DIR=%~dp0""#, false),
    // A COMPLETE line, and the fix is a measurement: as the bare fragment
    // `if /i "%~1"=="on"` this entry made cmd.exe refuse it — an `if` with no
    // command is a syntax error for reasons that have nothing to do with
    // percent sequences. The table's job is to name shapes, so every entry has
    // to stand alone as a batch line. Caught by the Windows leg on its first
    // run, after three review rounds had not.
    (r#"if /i "%~1"=="on" echo matched"#, false),
    (r#"set "DIR=%~DP0""#, false), // modifiers are case-insensitive
    ("echo %~$PATH:1", false),     // the documented search form
    ("for %%a in (x) do echo %%~b", false), // a FOR variable: percents doubled
    ("echo %%%%~b", false),        // two escaped pairs, then a literal tilde
    ("rem see %DIR%~about the dir", false), // an expansion, then literal text
    (r#"set "DEST=%DIR:~0,-1%""#, false), // a substring expansion
    // Refused by cmd — each one aborts the whole file.
    ("rem echo %~b", true),   // the defect this block exists for
    ("rem %%%~b", true),      // an escaped pair, then a bare `%~`
    ("echo %VAR%%~a", true),  // the CLOSING percent of an expansion
    ("echo %1 %~b", true),    // an argument reference, consumed in two chars
    ("rem %* and %~b", true), // likewise, and it swallowed the rest
    (r#"echo %~a""#, true),   // modifiers that close on no argument
    ("echo %~$PATH", true),   // the search form without its colon
    ("echo %~$PATH:x", true), // the search form closing on no digit
    ("echo %~", true),
];

/// The model agrees with the table. On Windows, so does cmd.exe.
#[test]
fn the_percent_tilde_scanner_knows_which_shapes_cmd_refuses() {
    for (line, refused) in PERCENT_CASES {
        assert_eq!(
            !illegal_percent_tilde(line).is_empty(),
            *refused,
            "the model disagrees with the table on: {line}",
        );
    }

    // The excerpt stops at the line end. A blind character window bled three
    // CRLF lines into one message, at the exact moment an operator needs it.
    assert_eq!(
        illegal_percent_tilde("rem head\r\nrem %~b tail\r\nrem next\r\n"),
        vec![(2, "%~b tail".to_string())],
    );
}

/// Every `.cmd`/`.bat` this repository TRACKS, asked of git rather than walked.
///
/// A second batch file — an installer step, a dev helper — must not arrive with
/// zero coverage of a defect class that already cost six releases. The first
/// draft hand-rolled the walk and bought three defects with it (found in
/// review): a four-name skip list that would red the suite over a developer's
/// gitignored scratch `.bat`, `is_dir()` following a symlink into a cycle, and
/// filesystem ordering that makes the same failure read differently per machine.
/// `git ls-files` has none of them — it is sorted, it never leaves the index,
/// and "what the repository ships" is precisely the question it answers.
fn tracked_batch_files() -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["ls-files", "-z", "*.cmd", "*.bat"])
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|e| panic!("could not ask git for the batch files: {e}"));
    assert!(
        out.status.success(),
        "git ls-files refused: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// No batch file in this repository carries a percent sequence cmd.exe refuses.
///
/// The scope is the FILE CLASS, not one path: `.gitattributes` already reasons
/// this way (`*.cmd text eol=crlf`), and the failure mode is identical wherever
/// it lands — silent abort, `bin/` empty, the whole harness dormant.
///
/// The one-liner that confirms the tokenisation on a real Windows box lives
/// HERE, where a percent sign is inert prose. From the repository root, at a
/// `cmd` prompt:
///
/// ```text
/// for /f "usebackq tokens=1,2 delims=:, " %a in ("plugin\.claude-plugin\plugin.json") do @if /i "%~a"=="version" @echo %~b
/// ```
///
/// It must print the manifest version and nothing else. (One percent at the
/// prompt, two inside a file — that difference is cmd, not a typo.)
#[test]
fn every_batch_file_carries_no_percent_sequence_cmd_will_refuse() {
    let files = tracked_batch_files();
    assert!(
        files.iter().any(|p| p.ends_with("plugin/bin/mustard-boot.cmd")),
        "git listed no mustard-boot.cmd, so this test is guarding nothing: {files:?}",
    );

    let mut offenders: Vec<String> = Vec::new();
    for rel in &files {
        // LOSSY, never skipped. `let Ok(body) = read_to_string(…) else
        // { continue }` turned a file re-saved in a non-UTF-8 encoding into a
        // silent pass — a guard that scans nothing while announcing the class is
        // closed (found in review). This file carries 21 em-dashes, so that is
        // one editor away.
        let Ok(bytes) = std::fs::read(repo_root().join(rel)) else {
            // A tracked path missing from the worktree is a sparse checkout or a
            // staged deletion, not a percent-sequence regression — reporting it
            // as one sends the reader to the wrong file (found in review).
            continue;
        };
        // The encodings a Windows editor actually produces, which the lossy read
        // does NOT catch: UTF-16 decodes to `%\0~\0b` with no replacement
        // character at all, and three BOM bytes make cmd choke on line 1
        // (`'\u{feff}@echo' is not recognized`). Both leave the scanner reading
        // inert prose and reporting a clean file (found in review).
        assert!(
            !bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
            "{rel} starts with a UTF-8 byte-order mark — cmd.exe refuses its \
             first line, and this scanner cannot see why",
        );
        assert!(
            !bytes.contains(&0),
            "{rel} carries NUL bytes, so it was re-saved as UTF-16 — cmd.exe \
             cannot read it, and this scanner would call it clean",
        );
        for (line, excerpt) in illegal_percent_tilde(&String::from_utf8_lossy(&bytes)) {
            offenders.push(format!("  {rel}:{line}: {excerpt}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "a batch file carries a percent sequence cmd.exe refuses, and the refusal \
         aborts the WHOLE file — every Windows machine goes dormant, in silence. \
         Write a FOR variable as `%%~x`, an argument as `%~1`, and keep \
         illustrative percent sequences out of batch files entirely:\n{}",
        offenders.join("\n"),
    );
}

/// The scanner above is a MODEL; on Windows, ask the subject.
///
/// It closes the class the scanner cannot — an unbalanced parenthesis, a bad
/// label, a line-ending regression — each of which fails the same silent way.
///
/// Three things this test does deliberately, each one a review finding on the
/// draft before it:
///
/// 1. It runs a COPY, in a temp tree carrying only the script and the manifest
///    beside it. Against `plugin/bin` the script would find a real
///    `mustard-rt.exe` (a gitignored build artifact) at `:run` and fire the live
///    `PreToolUse` hooks against this very checkout, mid-suite.
/// 2. It invokes the script DIRECTLY, not through `call` — the field
///    reproduction did, and whether an abort's 255 survives the extra hop is one
///    more thing nobody has measured.
/// 3. It MEASURES the truth table. Every `PERCENT_CASES` line goes to the same
///    cmd.exe, and its verdict has to match the one the model is checked
///    against everywhere else. Two rounds shipped a scanner hole because the
///    table was a belief; here it becomes an observation, and a wrong belief
///    turns this leg red.
/// 4. `if !cfg!(windows) { return }`, not `#[cfg(windows)]` on the item. The
///    body uses nothing Windows-specific, and gated OUT it is not even
///    type-checked on Linux or macOS — a compile error would reach only the
///    third CI leg, after the other two had gone green.
#[test]
fn windows_boot_really_parses_under_cmd() {
    if !cfg!(windows) {
        return;
    }
    // The temp tree is removed on EVERY path, failure included: the draft
    // cleaned up after the assertions, so the runs that matter — the failing
    // ones — were the runs that leaked (found in review).
    let tmp = std::env::temp_dir().join(format!(
        "mustard-boot-parse-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    ));
    let outcome = std::panic::catch_unwind(|| ask_a_real_cmd(&tmp));
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// The Windows half of the test above, so the cleanup can wrap it.
fn ask_a_real_cmd(tmp: &Path) {
    let bin = tmp.join("bin");
    for dir in [&bin, &tmp.join(".claude-plugin")] {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    }
    let copy = |from: &str, to: PathBuf| {
        std::fs::copy(repo_root().join(from), &to)
            .unwrap_or_else(|e| panic!("copying {from}: {e}"));
        to
    };
    let script = copy("plugin/bin/mustard-boot.cmd", bin.join("mustard-boot.cmd"));
    copy(
        "plugin/.claude-plugin/plugin.json",
        tmp.join(".claude-plugin").join("plugin.json"),
    );

    // `on PreToolUse` is LOAD-BEARING, not decoration: it is the argument pair
    // that trips the fetch gate (`NEED=0` for every trigger but SessionStart),
    // so the copy walks to `:run` and stops. Drop it, or pass `on
    // SessionStart`, and this test downloads a release archive on every CI run.
    let ask_cmd = |path: &Path| {
        std::process::Command::new("cmd")
            .args(["/c", &path.display().to_string(), "on", "PreToolUse"])
            .output()
            .unwrap_or_else(|e| panic!("could not spawn cmd.exe: {e}"))
    };

    // The whole truth table, measured rather than believed.
    for (idx, (line, refused)) in PERCENT_CASES.iter().enumerate() {
        let probe = bin.join(format!("case-{idx}.cmd"));
        std::fs::write(
            &probe,
            format!("@echo off\r\necho reached\r\n{line}\r\necho done\r\n"),
        )
        .unwrap_or_else(|e| panic!("writing probe {idx}: {e}"));
        assert_eq!(
            !ask_cmd(&probe).status.success(),
            *refused,
            "cmd.exe disagrees with PERCENT_CASES on: {line}",
        );
    }

    let out = ask_cmd(&script);
    assert!(
        out.status.success(),
        "cmd.exe refused mustard-boot.cmd (exit {:?}) — the script aborts, so \
         every Mustard hook on every Windows machine does nothing, in silence.\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Both twins cap the download at the SAME number of seconds.
///
/// The ceiling is not a free parameter: the tightest hook budget that can still
/// reach the fetch is 15 s (`plugin/hooks/hooks.json`, the
/// `clear|compact|resume|fork` arm of `SessionStart`), and unpacking plus the
/// hand-off still has to fit under it. Both files say in prose "keep this in
/// step with the twin", and prose is not a lock: a bump applied to one body only
/// is invisible, and shows up as `Hook cancelled` on exactly one operating
/// system. AC-1 asserts each twin has SOME deadline; this asserts they are the
/// same one, and that it fits.
#[test]
fn both_boot_twins_carry_the_same_download_deadline() {
    fn max_time(body: &str, file: &str) -> u32 {
        // Read the deadline off the COMMAND, never off the prose that explains
        // it. Both twins name `--max-time 10` in a comment several lines ABOVE
        // the `curl` that carries it, so splitting the whole body on the first
        // occurrence reads the comment — and a divergence introduced in the
        // command alone stays invisible. Found in review, 2026-08-29: with the
        // POSIX `curl` bumped to 90 and its comment left at 10, this test was
        // still green. A test that locks the prose is the exact failure its own
        // doc comment above warns about.
        let after = body
            .lines()
            .filter(|line| line.contains("curl "))
            .find_map(|line| line.split("--max-time ").nth(1))
            .unwrap_or_else(|| panic!("{file} has no `--max-time` on its curl"));
        after
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or_else(|e| panic!("{file}'s `--max-time` is not a number: {e}"))
    }

    let posix = max_time(&read("plugin/bin/mustard-boot"), "plugin/bin/mustard-boot");
    let windows = max_time(
        &read("plugin/bin/mustard-boot.cmd"),
        "plugin/bin/mustard-boot.cmd",
    );
    assert_eq!(
        posix, windows,
        "the two mustard-boot twins cap the download at different deadlines — \
         one operating system gets `Hook cancelled` and the other does not",
    );
    assert!(
        posix < 15,
        "a {posix}s download ceiling does not fit the 15s budget of the \
         tightest hook that can reach it (hooks.json, SessionStart on \
         clear|compact|resume|fork) — the unpack and the hand-off come after it",
    );
}

/// **A door does what it names — and the rule ships to every project, not just
/// to the one where it was learned.**
///
/// Measured in the field, 2026-08-31, on a repository that is not this one: a
/// bare `/mustard:pr open` produced a full test-suite run, a 328-commit drift
/// analysis, a dry-run merge in a throwaway worktree and a proposal to merge an
/// integration base into the operator's own unit — and no pull request. The
/// operator had asked for a pull request and nothing else.
///
/// Where the rule lives is the whole point of this ratchet. The door's own
/// prose (`plugin/commands/pr.md`) is read only once the door OPENS, and that
/// session never got that far — it went wide before it went anywhere. The
/// ORCHESTRATOR is the file injected at the start of every session of every
/// project, so that is the copy that has to carry it; the door carries the
/// operational half. Both, or the rule only reaches the sessions that were
/// already going to be fine.
///
/// This holds all three claims: the general rule, its two operational halves,
/// and the measurement that justifies them — a reader can check the date rather
/// than take the rule on faith.
#[test]
fn every_project_learns_that_a_door_does_what_it_names() {
    let orchestrator = mustard_core::ORCHESTRATOR_MD;

    // The general rule, in the file every session reads. It is stated ABOUT
    // doors, not about `pr open`, because the next over-reach will be at some
    // other door.
    assert!(
        orchestrator.contains("A door does what it NAMES and stops there"),
        "the injected router never says a door is bounded by its own name",
    );

    // Half one: a measurement taken for a report is not a gate. A red suite is
    // `merge`'s business; `open` states the number and publishes.
    assert!(
        orchestrator.contains("REPORTED, never investigated"),
        "the router lets a measurement taken for the body become a gate",
    );

    // Half two: the environment refusing is news, not a problem to route
    // around. Carrying an integration base into the operator's unit to make
    // someone else's failure go green is the specific move that was measured.
    assert!(
        orchestrator.contains("Never propose carrying an integration base into the operator's unit"),
        "the router leaves the 328-commit merge proposal available as an answer",
    );

    // The measurement itself, with its date — the claim above is checkable.
    assert!(
        orchestrator.contains("2026-08-31"),
        "the router asserts the door-scope rule without the field case behind it",
    );

    // The door repeats the operational half where the work actually happens, so
    // a session that DID reach the door is told there too.
    let pr_door = read("plugin/commands/pr.md");
    for claim in [
        "It does not judge, and it does not gate",
        "A red suite is REPORTED, never investigated here",
        "A push refused by the repository's own tooling is REPORTED, not routed around",
    ] {
        assert!(
            pr_door.contains(claim),
            "the `pr` door dropped `{claim}` — the operational half of the rule",
        );
    }
}
