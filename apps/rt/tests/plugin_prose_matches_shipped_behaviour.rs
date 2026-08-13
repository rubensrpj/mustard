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
    let seed = mustard_core::ORCHESTRATOR_MD;
    for deleted in ["writes IN-PLACE", "carves out `.claude/spec/`"] {
        assert!(
            !seed.contains(deleted),
            "the orchestrator seed still teaches `{deleted}` — the carve-out wave 2 removed",
        );
    }

    // Anchored: the paragraph a reader arrives at is the one that computes the
    // unit's branch, not some later section that happens to mention isolation.
    // The needle carries the SHAPE, so a return to the base-prefixed name — or
    // any other spelling of the join — moves this anchor off the paragraph.
    let branch_line = line_with(seed, "compute the unit's `{kind}/{slug}` branch")
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
    let delivered_line = line_with(&delivered, "compute the unit's `{kind}/{slug}` branch")
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
    // The compiled-in seed is what `upsert` lays down in every project.
    let seed = mustard_core::ORCHESTRATOR_MD;

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

    // The three rules that make it one question instead of a form.
    let rules = line_with(seed, "DESTINATION")
        .expect("the router never says a hotfix is a destination rather than a kind of work");
    assert!(
        rules.contains("never inferred") || rules.contains("never pre-marked"),
        "hotfix is named a destination without saying it is therefore never \
         guessed from the request: {rules}",
    );
    assert!(
        rules.contains("ONE candidate base"),
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
    // `seed_injectable_files` PRESERVES an existing file on merge, so editing
    // the template does not update an already-seeded project.
    let delivered = read(".claude/mustard/orchestrator.md");
    for row in ["  tipo:", "  branch:", "run emit-pipeline --kind pipeline.kind"] {
        assert_eq!(
            line_with(&delivered, row),
            line_with(seed, row),
            "the delivered .claude/mustard/orchestrator.md drifted from the seed \
             at `{row}` — re-seed it, or this project asks the old question",
        );
    }

    // --- 3. The code really behaves the way the question promises ----------
    // Without this half the block above outlives its mechanism: it would keep
    // showing `fix/…` after the join, the flow-derived base or the refusals
    // went away.
    let kinds = read("apps/rt/src/shared/work_kind.rs");
    assert!(
        kinds.contains("enum WorkKind") && kinds.contains("format!(\"{}/{slug}\", self.token())"),
        "nothing builds `{{kind}}/{{slug}}` any more, so the name the question \
         showed is not the name the branch gets",
    );
    let branch = read("apps/rt/src/commands/event/work_branch.rs");
    assert!(
        branch.contains("kind.branch_name(&slug)"),
        "compute_work_branch spells the join itself instead of taking WorkKind's \
         — two spellings of one name is what this module exists to prevent",
    );
    // The base is a CONSEQUENCE of the kind, read from the flow.
    assert!(
        kinds.contains("WorkKind::Feature | WorkKind::Fix => self.work"),
        "an ordinary unit no longer answers the base ordinary work is cut from",
    );
    assert!(
        kinds.contains("fn emergency_bases") && kinds.contains("fn emergency_is_ambiguous"),
        "nothing answers WHICH bases a hotfix may take, so `sai de` cannot know \
         whether it has one candidate or several",
    );
    // Both refusals the prose promises, read through the messages that carry
    // them: delete either and the operator is silently coerced instead.
    assert!(
        branch.contains("fn resolve_kind_base"),
        "the gate has no resolver validating the kind/base pair the router sends",
    );
    assert!(
        branch.contains("não é uma base de integração deste projeto"),
        "an undeclared --base is no longer refused, so the router's promise that \
         it is becomes a lie the operator only meets on a wrong branch",
    );
    assert!(
        branch.contains("um hotfix não sai de"),
        "a hotfix cut from the WORK base is no longer refused — that contradiction \
         IS the difference between a fix and a hotfix",
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

/// The role names `full-plan.md` declares reserved. `review`/`qa` are one pair
/// of names for one agent, which is why six names spell five reservations.
const RESERVED_ROLES: &[&str] = &["plan", "explore", "review", "qa", "guards", "patterns"];

/// The names that same paragraph offers as ordinary writing roles.
const WRITING_ROLE_EXAMPLES: &[&str] = &["backend", "proof", "discovery", "bootstrap"];
