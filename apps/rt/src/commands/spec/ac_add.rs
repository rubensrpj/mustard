//! `mustard-rt run ac-add` — introduce an acceptance criterion the spec does
//! NOT yet carry, AFTER the artefacts are frozen, and prove it knows how to
//! fail before it lands anywhere.
//!
//! ## Why this exists
//!
//! The flow already carries the rule: a mid-pipeline request that gets
//! implemented but is named by no criterion makes the gate report green without
//! ever verifying it. The rule was written and nothing implemented it — the only
//! criterion-editing operation, [`super::ac_amend`], REPLACES an id that already
//! exists and refuses one it does not know. So when a review demanded a
//! criterion for a finding, there was no door, and criteria were typed into
//! `spec.md` by hand against the role contract that forbids touching the spec.
//!
//! ## Why not a flag on amend
//!
//! Amend's whole contract is that a replacement must prove it can fail against a
//! tree where the criterion it supersedes already lived — that is what makes
//! `superseded_command` meaningful and what the INEXECUTABLE exception is keyed
//! to. An added id has no predecessor to supersede, so folding it in would blur
//! the one rule that makes amend trustworthy. Two doors, one engine.
//!
//! ## The same proof, no softer
//!
//! The new criterion goes through [`ac_negative_check::prove_one`] and is
//! REFUSED on anything but a red. A door that admitted a criterion nobody proved
//! would import exactly the vacuous-criterion defect the proof was built to
//! stop — and would do it at the one moment nobody is watching, mid-pipeline.
//!
//! ## Where it lands, and why not last
//!
//! The criterion is inserted directly ABOVE the section's LAST criterion, never
//! appended after it. The trailing criterion is exempt by position
//! ([`ac_negative_check::is_exempt`]) — it is the build-green safety net — so
//! appending would silently move the exemption onto the new criterion AND demand
//! a red proof from a build command that is green by design. Position is not
//! decoration here; it is a rule two mechanisms read.
//!
//! It lands in EVERY plan artefact that carries criteria — the root `spec.md`,
//! `wave-plan.md` and each `wave-*/spec.md` — for the reason amend rewrites all
//! of them: the scaffold is frozen after approval, and a root-only write leaves
//! the dispatched agent reading a list the criterion is not on. Transcripts
//! (`qa/`, `review/`) are NOT artefacts: they are records of a run, and writing
//! a criterion into a past run's report would forge evidence.
//!
//! ## Refusal, not silence
//!
//! Five refusals, each of which writes NOTHING anywhere: a blank reason, a blank
//! statement, an unknown spec, an id the spec ALREADY carries (that is an
//! amendment — a different door), and a command the negative test does not
//! report as proven. Every refusal names its reason AND the one action that
//! clears it.
//!
//! ## One parser, one ledger
//!
//! The criteria are read through [`super::ac_amend`]'s shared helpers, which are
//! themselves [`qa_run`]'s parser and the negative test's ledger. The write is
//! confirmed by RE-READING each artefact: the addition reports whether it landed
//! instead of assuming it did. The record goes in the ledger's `additions`
//! array — never `amendments`, which means a supersession that did not happen.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::commands::review::ac_negative_check::{
    self, AcProof, AC_PROOF_JSON,
};
use crate::commands::review::qa_run;
use crate::commands::spec::ac_amend::{
    criteria_of, landed, normalise_id, read_ledger, write_ledger, AC_SECTION_KEY,
};
use crate::commands::spec::spec_sections;
use mustard_core::io::fs as mfs;

/// Options for `mustard-rt run ac-add`.
#[derive(Debug, Clone)]
pub struct AcAddOpts {
    /// Spec slug under `.claude/spec/`.
    pub spec: String,
    /// The criterion id to introduce (`AC-9`, `AC-W4-3`, …). Must not exist.
    pub ac: String,
    /// The EARS statement the criterion asserts. Never blank — a criterion
    /// nobody stated is one a later reader cannot check the command against.
    pub statement: String,
    /// The command that asserts the new behaviour.
    pub command: String,
    /// The `Expect:` evidence regex, when the criterion carries one.
    pub expect: Option<String>,
    /// Why the criterion is being added. Never blank.
    pub reason: String,
}

/// JSON report printed on stdout. Deterministic: repo-relative paths, sorted,
/// and no timestamp (that lives in the ledger).
#[derive(Debug, Serialize)]
pub(crate) struct AcAddReport {
    /// `true` only when the addition was accepted AND every write is confirmed.
    pub(crate) ok: bool,
    /// The spec the criterion joins.
    pub(crate) spec: String,
    /// The criterion id, normalised (`AC-9`).
    pub(crate) ac: String,
    /// The statement the criterion now carries.
    pub(crate) statement: String,
    /// The command it now carries.
    pub(crate) command: String,
    /// The evidence regex it now carries, when it has one.
    pub(crate) expect: Option<String>,
    /// The proof the negative test took — recorded whichever way it fell, so a
    /// refusal shows exactly what the engine saw.
    pub(crate) proof: Option<AcProof>,
    /// Every artefact the criterion was written into AND confirmed on re-read,
    /// as repo paths with forward slashes.
    pub(crate) written: Vec<String>,
    /// Where the proof ledger lives, when it was updated.
    pub(crate) ledger: Option<String>,
    /// Refusal / failure code: `blank_reason`, `blank_statement`,
    /// `unknown_spec`, `duplicate_criterion`, `criterion_not_proven`,
    /// `write_failed`, `ledger_write_failed`.
    pub(crate) error: Option<String>,
    /// The one action that clears the refusal. Absent when nothing is wrong.
    pub(crate) remedy: Option<String>,
}

impl AcAddReport {
    /// A report for an addition that was refused before anything was written.
    fn refused(opts: &AcAddOpts, ac: &str, error: &str, remedy: &str) -> Self {
        Self {
            ok: false,
            spec: opts.spec.clone(),
            ac: ac.to_string(),
            statement: opts.statement.clone(),
            command: opts.command.clone(),
            expect: opts.expect.clone(),
            proof: None,
            written: Vec::new(),
            ledger: None,
            error: Some(error.to_string()),
            remedy: Some(remedy.to_string()),
        }
    }
}

/// One addition's entry in the ledger's `additions` array — the id, what it
/// asserts, how it is checked, why it was added, and when.
#[derive(Debug, Serialize)]
struct Addition {
    /// The criterion that joined the spec.
    id: String,
    /// When it was accepted. The ledger is where a timestamp belongs: stdout
    /// must stay byte-stable.
    at: String,
    /// The stated reason — the whole point of recording an addition.
    reason: String,
    /// The statement the criterion carries.
    statement: String,
    /// The command it carries.
    command: String,
    /// The evidence regex it carries, when it has one.
    expect: Option<String>,
    /// The artefacts it was written into, as repo paths.
    wrote: Vec<String>,
}

// ---------------------------------------------------------------------------
// Line surgery
//
// The reader side is `qa_run::parse_ac_items`: a drafter-form criterion is a
// header line plus an indented `Command:` line and an optional `Expect:` line.
// The writer below emits exactly that, and the unit tests assert the ROUND TRIP
// — an inserted block is parsed back by the same parser and must yield the new
// criterion. That round trip, not a shared template, is what keeps the two
// honest.
// ---------------------------------------------------------------------------

/// The criterion block, in the drafter's canonical shape, with `eol` appended to
/// every line so a CRLF document survives the insertion. Pure, total.
fn criterion_block(
    id: &str,
    statement: &str,
    command: &str,
    expect: Option<&str>,
    eol: &str,
) -> Vec<String> {
    let mut block = vec![
        format!("- **{id}** — {}{eol}", statement.trim()),
        format!("  Command: `{command}`{eol}"),
    ];
    if let Some(expect) = expect {
        block.push(format!("  Expect: `{expect}`{eol}"));
    }
    block
}

/// Insert `id`'s criterion block into the `## Acceptance Criteria` section of
/// one markdown document. `None` when the document declares no such section —
/// which is how a wave artefact that carries no criteria is left alone.
///
/// Among HOMONYMOUS sections (legacy drafts duplicated the heading) the one
/// carrying criteria wins, mirroring [`spec_sections::section_block`]'s own
/// defensive pick — writing into the placeholder copy would land the criterion
/// where no reader looks.
fn insert_criterion(
    body: &str,
    id: &str,
    statement: &str,
    command: &str,
    expect: Option<&str>,
) -> Option<String> {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut carrying: Option<(usize, usize)> = None;
    let mut first: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < lines.len() {
        if !spec_sections::is_heading(lines[i], AC_SECTION_KEY) {
            i += 1;
            continue;
        }
        let end = spec_sections::section_end(&lines, i);
        if first.is_none() {
            first = Some((i, end));
        }
        if carrying.is_none()
            && (i + 1..end).any(|j| qa_run::parse_ac_header(lines[j]).is_some())
        {
            carrying = Some((i, end));
        }
        i = end;
    }
    let (start, end) = carrying.or(first)?;

    // The anchor: the section's LAST criterion header. The new block lands
    // ABOVE it so the trailing build-green criterion STAYS trailing — the
    // positional exemption must not move onto the criterion being added.
    let anchor = (start + 1..end).rfind(|j| qa_run::parse_ac_header(lines[*j]).is_some());
    let at = match anchor {
        Some(j) => j,
        // An empty section: after its last non-blank line, so the block does
        // not weld onto the heading or trail behind the section's blank tail.
        None => (start + 1..end)
            .rev()
            .find(|j| !lines[*j].trim().is_empty())
            .map_or(end, |j| j + 1),
    };

    // CRLF is a property of the DOCUMENT, not of the anchor line: a file whose
    // lines end `\r\n` must not gain three lone-LF lines in the middle.
    let eol = if body.contains("\r\n") { "\r" } else { "" };
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
    let block = criterion_block(id, statement, command, expect, eol);
    out.splice(at..at, block);
    Some(out.join("\n"))
}

/// Every PLAN artefact under a spec directory that can carry criterion lines:
/// the root `spec.md` / `wave-plan.md` and each `wave-*/spec.md`.
///
/// Named rather than walked. The amendment door walks every markdown two levels
/// down because it only ever rewrites a line that is already there; an ADD
/// writes a line that is not, and the same walk would reach `qa/report.md` and
/// `review/` transcripts — records of a run that finished, into which a new
/// criterion would be forged evidence. Sorted, so the report and the ledger are
/// byte-stable.
fn plan_artefacts(spec_dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = ["spec.md", "wave-plan.md"]
        .into_iter()
        .map(|name| spec_dir.join(name))
        .filter(|p| p.is_file())
        .collect();
    if let Ok(entries) = std::fs::read_dir(spec_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_wave = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("wave-"));
            if path.is_dir() && is_wave && path.join("spec.md").is_file() {
                found.push(path.join("spec.md"));
            }
        }
    }
    found.sort();
    found
}

// ---------------------------------------------------------------------------
// The operation
// ---------------------------------------------------------------------------

/// Add one criterion to `spec` under an explicit project `root`.
///
/// `root` is a PARAMETER, never re-derived from the process working directory:
/// this tool cuts a worktree per work unit, so the command runs off-root as a
/// matter of course — and the unit tests drive it against a temp tree.
pub(crate) fn add(root: &Path, opts: &AcAddOpts) -> AcAddReport {
    let id = normalise_id(&opts.ac);

    // A reason nobody stated is an addition nobody can audit later.
    let reason = opts.reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if reason.is_empty() {
        return AcAddReport::refused(
            opts,
            &id,
            "blank_reason",
            "state WHY the criterion is being added: `--reason \"<sentence>\"`. The reason is the \
             only part of an addition a later reader cannot reconstruct from the diff",
        );
    }

    // A criterion with no statement is a command with no claim beside it — the
    // reader cannot tell whether the command checks what was asked for.
    let statement = opts.statement.split_whitespace().collect::<Vec<_>>().join(" ");
    if statement.is_empty() {
        return AcAddReport::refused(
            opts,
            &id,
            "blank_statement",
            "state WHAT the criterion asserts: `--statement \"when <trigger>, then <outcome>\"`. \
             A command with no statement cannot be checked against what was asked for",
        );
    }

    // A slug with no spec markdown is a typo, not a new spec.
    let Some(spec_file) = qa_run::spec_file_for(root, &opts.spec) else {
        return AcAddReport::refused(
            opts,
            &id,
            "unknown_spec",
            "no spec markdown under `.claude/spec/<slug>/` for that name — check the slug with \
             `mustard-rt run active-specs`",
        );
    };
    let spec_dir = spec_file.parent().unwrap_or(root).to_path_buf();

    let Ok(markdown) = mfs::read_to_string(&spec_file) else {
        return AcAddReport::refused(
            opts,
            &id,
            "unknown_spec",
            "the spec markdown could not be read — check the file exists and is readable",
        );
    };
    let items = criteria_of(&markdown);
    // An id the spec ALREADY carries is an amendment, and amendments have their
    // own door with their own rule. Routing it here would let a replacement skip
    // the supersession record entirely.
    if items.iter().any(|item| item.id == id) {
        return AcAddReport::refused(
            opts,
            &id,
            "duplicate_criterion",
            &format!(
                "the spec already declares {id} — changing a criterion that exists is an \
                 AMENDMENT: `mustard-rt run ac-amend --spec {} --ac {id} …`. To add a new one, \
                 pick an id the spec does not carry",
                opts.spec
            ),
        );
    }

    let expect = opts.expect.clone().filter(|e| !e.trim().is_empty());

    // THE gate — the same engine, at the same strictness. The criterion is
    // inserted ABOVE the trailing one, so it is never the exempt position: it
    // owes a red proof like any criterion the plan declared.
    let proof = ac_negative_check::prove_one(root, &id, &opts.command, expect.as_deref(), false);
    if proof.proof != ac_negative_check::Proof::Red {
        let why = proof.reason.clone().unwrap_or_default();
        let mut report = AcAddReport::refused(
            opts,
            &id,
            "criterion_not_proven",
            &format!(
                "the criterion being ADDED does not clear the negative test, so it would join the \
                 spec verifying exactly nothing — {why}"
            ),
        );
        report.proof = Some(proof);
        return report;
    }

    // Accepted. From here on the writes happen; every one of them is re-read.
    let mut written: Vec<String> = Vec::new();
    for path in plan_artefacts(&spec_dir) {
        let Ok(body) = mfs::read_to_string(&path) else {
            continue;
        };
        let Some(updated) =
            insert_criterion(&body, &id, &statement, &opts.command, expect.as_deref())
        else {
            continue;
        };
        if mfs::write_atomic(&path, updated.as_bytes()).is_err() {
            continue;
        }
        if landed(&path, &id, &opts.command) {
            written.push(ac_negative_check::repo_relative(root, &path));
        }
    }
    written.sort();

    let mut report = AcAddReport {
        ok: false,
        spec: opts.spec.clone(),
        ac: id.clone(),
        statement: statement.clone(),
        command: opts.command.clone(),
        expect: expect.clone(),
        proof: Some(proof.clone()),
        written: written.clone(),
        ledger: None,
        error: None,
        remedy: None,
    };

    // The ROOT spec is the one artefact that must have changed: it is the list
    // every other reader is derived from. Nothing confirmed there is a lost
    // write, reported.
    let root_path = ac_negative_check::repo_relative(root, &spec_file);
    if !written.contains(&root_path) {
        report.error = Some("write_failed".to_string());
        report.remedy = Some(format!(
            "the criterion was not written into `{root_path}` — re-read the file and check it \
             declares a `## Acceptance Criteria` section"
        ));
        return report;
    }

    let ledger_path = spec_dir.join(AC_PROOF_JSON);
    let mut ledger = read_ledger(&ledger_path);
    ledger.spec = spec_dir
        .file_name()
        .map_or_else(|| opts.spec.clone(), |n| n.to_string_lossy().into_owned());
    // The proof record joins the ledger so the approval door accepts the new
    // criterion on the evidence it just produced, never on a later re-run.
    ledger.criteria.retain(|c| c.id != id);
    ledger.criteria.push(proof);
    ledger.criteria.sort_by(|a, b| a.id.cmp(&b.id));

    let entry = Addition {
        id: id.clone(),
        at: mustard_core::time::now_iso8601(),
        reason,
        statement,
        command: opts.command.clone(),
        expect,
        wrote: written,
    };
    if let Ok(value) = serde_json::to_value(&entry) {
        ledger.additions.push(value);
    }
    if !write_ledger(&ledger_path, &ledger) {
        report.error = Some("ledger_write_failed".to_string());
        report.remedy = Some(
            "the artefacts were written but the proof ledger did not land — re-run \
             `mustard-rt run ac-negative-check --spec <slug>` to rebuild it"
                .to_string(),
        );
        return report;
    }
    report.ledger = Some(ac_negative_check::repo_relative(root, &ledger_path));
    report.ok = true;
    report
}

/// The process exit code for a finished report: `0` accepted, `1` refused.
fn exit_code(report: &AcAddReport) -> i32 {
    i32::from(!report.ok)
}

/// CLI entry — `mustard-rt run ac-add`.
pub fn run(opts: AcAddOpts) {
    let root = PathBuf::from(crate::shared::context::project_dir());
    let report = add(&root, &opts);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(exit_code(&report));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::tempdir;

    /// A command that comes back RED on both shells (`cmd.exe` and `sh`): the
    /// directory does not exist, so `cd` exits non-zero.
    const RED_COMMAND: &str = "cd no-such-directory-abc";
    /// A command that comes back GREEN on both shells — `cd .` is a builtin
    /// everywhere and always succeeds.
    const GREEN_COMMAND: &str = "cd .";

    /// Seed `<root>/.claude/spec/<spec>/` with a root `spec.md`, the frozen
    /// `wave-plan.md` and a wave artefact — the three shapes a plan artefact
    /// takes — plus a `qa/report.md` transcript that must NOT be written into.
    fn seed(root: &Path, spec: &str) -> PathBuf {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(dir.join("wave-1-rt")).unwrap();
        std::fs::create_dir_all(dir.join("qa")).unwrap();
        let criteria = format!(
            "## Acceptance Criteria\n\
             - **AC-1** — when the work lands, then the behaviour holds.\n  Command: `{RED_COMMAND}`\n\
             - **AC-2** — the project build passes green.\n  Command: `{GREEN_COMMAND}`\n"
        );
        std::fs::write(dir.join("spec.md"), format!("# S\n\n{criteria}")).unwrap();
        std::fs::write(dir.join("wave-plan.md"), format!("# Plan\n\n{criteria}")).unwrap();
        std::fs::write(
            dir.join("wave-1-rt").join("spec.md"),
            format!("# Wave 1\n\n{criteria}"),
        )
        .unwrap();
        std::fs::write(dir.join("qa").join("report.md"), format!("# QA\n\n{criteria}")).unwrap();
        dir
    }

    fn opts(spec: &str, ac: &str, command: &str) -> AcAddOpts {
        AcAddOpts {
            spec: spec.to_string(),
            ac: ac.to_string(),
            statement: "when the finding is fixed, then the gate refuses the old shape".to_string(),
            command: command.to_string(),
            expect: None,
            reason: "the review found a defect no criterion names".to_string(),
        }
    }

    /// AC-1 — the accepted direction. A criterion the spec does not carry is
    /// introduced, and it lands in the artefacts and in the ledger ONLY after
    /// taking the same red proof a planned criterion takes.
    ///
    /// Three claims, all load-bearing:
    ///
    /// 1. **The proof came first.** The ledger records the new id with a RED
    ///    proof — the evidence the approval gate reads.
    /// 2. **Every plan artefact carries it**, not just the root: the frozen
    ///    `wave-plan.md` and the wave scaffold are what a dispatched agent
    ///    reads. The `qa/` transcript is left alone — it is a record of a run.
    /// 3. **The trailing criterion stays trailing**, so the positional exemption
    ///    does not move onto the criterion just added.
    #[test]
    fn ac_add_lands_only_after_taking_the_proof() {
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "added");
        let qa_before = std::fs::read_to_string(spec_dir.join("qa").join("report.md")).unwrap();

        let mut o = opts("added", "AC-3", RED_COMMAND);
        o.expect = Some("1 passed".to_string());
        let report = add(dir.path(), &o);

        assert!(report.ok, "unexpected refusal: {:?} / {:?}", report.error, report.remedy);
        assert_eq!(exit_code(&report), 0);
        let proof = report.proof.clone().expect("the addition records its proof");
        assert_eq!(proof.proof, ac_negative_check::Proof::Red, "{proof:?}");

        // Every PLAN artefact, and only those.
        assert_eq!(
            report.written,
            vec![
                ".claude/spec/added/spec.md".to_string(),
                ".claude/spec/added/wave-1-rt/spec.md".to_string(),
                ".claude/spec/added/wave-plan.md".to_string(),
            ],
            "the root, the frozen plan AND the wave scaffold"
        );
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("qa").join("report.md")).unwrap(),
            qa_before,
            "a finished run's transcript is a record, never an artefact to write into"
        );

        for name in ["spec.md", "wave-plan.md"] {
            let body = std::fs::read_to_string(spec_dir.join(name)).unwrap();
            let items = criteria_of(&body);
            let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
            assert_eq!(ids, ["AC-1", "AC-3", "AC-2"], "{name}: the build criterion stays LAST");
            let added = items.iter().find(|i| i.id == "AC-3").unwrap();
            assert_eq!(added.command, RED_COMMAND, "{name}");
            assert_eq!(added.expect.as_deref(), Some("1 passed"), "{name}");
            assert!(
                added.statement.contains("the gate refuses the old shape"),
                "{name}: {}",
                added.statement
            );
        }

        // The ledger: the new criterion's proof is there for the approval gate,
        // and the addition is recorded APART from the amendment history.
        let ledger: Value =
            serde_json::from_str(&std::fs::read_to_string(spec_dir.join(AC_PROOF_JSON)).unwrap())
                .unwrap();
        let record = ledger["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "AC-3")
            .unwrap();
        assert_eq!(record["verdict"], "proven");
        assert_eq!(record["proof"], "red");
        assert!(
            ledger["amendments"].as_array().is_some_and(Vec::is_empty),
            "an addition supersedes nothing: {ledger}"
        );
        let addition = &ledger["additions"].as_array().unwrap()[0];
        assert_eq!(addition["id"], "AC-3");
        assert_eq!(addition["reason"], "the review found a defect no criterion names");
        assert!(
            addition["at"].as_str().is_some_and(|s| s.ends_with('Z')),
            "the timestamp lives in the ledger: {addition}"
        );
        // ...and NOT on stdout, which is snapshot-compared.
        let printed = serde_json::to_string(&report).unwrap();
        assert!(!printed.contains("\"at\""), "no timestamp on stdout: {printed}");
    }

    /// AC-2 — the load-bearing refusal. A criterion whose command ALREADY passes
    /// against the tree as it is verifies nothing, so the door refuses it and
    /// NOTHING is written: not the artefacts, not the ledger.
    ///
    /// Two-sided within one test: the same seed accepts a RED criterion, so the
    /// refusal cannot pass by the door being inert. The other four refusals —
    /// blank reason, blank statement, unknown spec and an id the spec already
    /// carries — are checked the same way, each writing nothing.
    #[test]
    fn ac_add_refuses_a_criterion_that_cannot_fail() {
        let dir = tempdir().unwrap();
        let spec_dir = seed(dir.path(), "refused");
        let before = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        let plan_before = std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap();

        let report = add(dir.path(), &opts("refused", "AC-3", GREEN_COMMAND));
        assert!(!report.ok, "a criterion that passes now must be refused");
        assert_eq!(report.error.as_deref(), Some("criterion_not_proven"));
        let remedy = report.remedy.clone().unwrap_or_default();
        assert!(remedy.contains("verifying exactly nothing"), "{remedy}");
        assert!(remedy.contains("rewrite the command"), "the engine's own remedy: {remedy}");
        assert_eq!(exit_code(&report), 1, "a refusal exits non-zero");
        assert!(report.written.is_empty(), "{:?}", report.written);
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("spec.md")).unwrap(),
            before,
            "the root spec must be byte-identical after a refusal"
        );
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap(),
            plan_before,
            "and so must the frozen plan"
        );
        assert!(
            !spec_dir.join(AC_PROOF_JSON).exists(),
            "a refusal must not even create the ledger"
        );

        // The input refusals, each writing nothing.
        let blank_reason = AcAddOpts { reason: "  ".to_string(), ..opts("refused", "AC-3", RED_COMMAND) };
        let blank_statement =
            AcAddOpts { statement: String::new(), ..opts("refused", "AC-3", RED_COMMAND) };
        for (o, code) in [
            (blank_reason, "blank_reason"),
            (blank_statement, "blank_statement"),
            (opts("no-such-spec", "AC-3", RED_COMMAND), "unknown_spec"),
            (opts("refused", "AC-1", RED_COMMAND), "duplicate_criterion"),
        ] {
            let report = add(dir.path(), &o);
            assert!(!report.ok, "{code} must refuse");
            assert_eq!(report.error.as_deref(), Some(code));
            assert!(
                report.remedy.is_some_and(|r| !r.trim().is_empty()),
                "{code} must name what to do about it"
            );
            assert!(report.written.is_empty(), "{code} wrote an artefact");
            assert!(!spec_dir.join(AC_PROOF_JSON).exists(), "{code} wrote a ledger");
        }
        // The duplicate refusal points at the door that DOES change an existing
        // criterion, instead of leaving the caller to guess.
        let dup = add(dir.path(), &opts("refused", "AC-1", RED_COMMAND));
        let remedy = dup.remedy.unwrap_or_default();
        assert!(remedy.contains("ac-amend"), "{remedy}");
        assert!(remedy.contains("AMENDMENT"), "{remedy}");

        // Two-sided: the same seed accepts a RED criterion.
        let ok = add(dir.path(), &opts("refused", "AC-3", RED_COMMAND));
        assert!(ok.ok, "unexpected refusal: {:?} / {:?}", ok.error, ok.remedy);
    }

    /// Round trip on the insertion: what is written is what the SAME parser
    /// reads back, in every document shape — with and without an `Expect:`
    /// line, over LF and CRLF, and into a section that carries no criterion yet.
    #[test]
    fn inserted_blocks_parse_back_as_the_new_criterion() {
        for (original, expect) in [
            ("## Acceptance Criteria\n- **AC-1** — first.\n  Command: `cd a`\n", None),
            (
                "## Acceptance Criteria\n- **AC-1** — first.\n  Command: `cd a`\n",
                Some("2 passed"),
            ),
            (
                "## Acceptance Criteria\r\n- **AC-1** — first.\r\n  Command: `cd a`\r\n",
                Some("2 passed"),
            ),
            // A section with no criterion at all: the block is the whole list.
            ("## Acceptance Criteria\n\nnone yet.\n\n## Files\n\n- `a.rs`\n", None),
        ] {
            let updated = insert_criterion(original, "AC-9", "when x, then y", "cd new", expect)
                .unwrap_or_else(|| panic!("nothing inserted into {original:?}"));
            let items = criteria_of(&updated);
            let added = items
                .iter()
                .find(|i| i.id == "AC-9")
                .unwrap_or_else(|| panic!("AC-9 unreadable after insertion: {updated:?}"));
            assert_eq!(added.command, "cd new", "{updated:?}");
            assert_eq!(added.expect.as_deref(), expect, "{updated:?}");
            assert_eq!(added.statement, "when x, then y", "{updated:?}");
            if original.contains("\r\n") {
                // `split('\n')` and not `lines()`: the latter strips the `\r`
                // this assertion is looking for.
                assert!(
                    !updated
                        .split('\n')
                        .any(|l| l.contains("AC-9") && !l.ends_with('\r')),
                    "a CRLF document must not gain lone-LF lines: {updated:?}"
                );
            }
            // The sibling section is never disturbed.
            if original.contains("## Files") {
                assert!(updated.contains("## Files\n\n- `a.rs`"), "{updated:?}");
            }
        }
        // A document with no acceptance-criteria section is left alone entirely
        // — that is how a wave artefact carrying no criteria is skipped.
        assert!(insert_criterion("# Wave\n\n## Tasks\n\n- do it\n", "AC-9", "s", "c", None).is_none());
    }

    /// The new criterion never takes the trailing slot, because the trailing
    /// slot is EXEMPT from the proof — moving it would demand a red from a
    /// build command that is green by design and hand the exemption to the
    /// criterion that most needs proving.
    #[test]
    fn the_addition_never_takes_the_exempt_trailing_slot() {
        let md = "## Acceptance Criteria\n\
                  - **AC-1** — first.\n  Command: `cd a`\n\
                  - **AC-2** — build green.\n  Command: `cargo build`\n";
        let updated = insert_criterion(md, "AC-3", "when x, then y", "cd new", None)
            .expect("the criterion was inserted");
        let items = criteria_of(&updated);
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["AC-1", "AC-3", "AC-2"], "{updated:?}");
        let last = items.len() - 1;
        assert!(
            ac_negative_check::is_exempt(last, items.len()) && items[last].id == "AC-2",
            "the build criterion keeps the exemption: {ids:?}"
        );
    }

    /// Ids are normalised through the amendment door's own rule, so `--ac 3`
    /// and `--ac ac-3` reach the same criterion through either door.
    #[test]
    fn criterion_ids_are_normalised_through_one_rule() {
        let dir = tempdir().unwrap();
        seed(dir.path(), "norm");
        let report = add(dir.path(), &opts("norm", "3", RED_COMMAND));
        assert!(report.ok, "unexpected refusal: {:?}", report.error);
        assert_eq!(report.ac, "AC-3");
        // And the lowercase spelling of an id that now exists is a duplicate.
        let dup = add(dir.path(), &opts("norm", "ac-3", RED_COMMAND));
        assert_eq!(dup.error.as_deref(), Some("duplicate_criterion"));
    }
}
