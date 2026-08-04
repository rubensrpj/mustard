//! `mustard-rt run mark-finding` — declare the DESTINATION of one collected
//! finding, and the reason it went there.
//!
//! # Why a door of its own
//!
//! `finding-collect` seeds `meta.json#findings` deterministically from the two
//! producers that already write to disk, and it decides NOTHING: the one thing
//! on that record no machine can reproduce is what somebody chose to do about
//! each discovery. This command is that half — the only writer of
//! [`FindingItem::routed`], mirroring `mark-checklist-item --drop --reason`,
//! which solved exactly this problem one level down (an item let go on purpose
//! is a decision, and a decision with no stated reason is indistinguishable
//! from a forgotten task).
//!
//! So `--reason` is mandatory here for the same reason it is mandatory there,
//! and the type enforces it twice over: [`FindingRoute`] has no reason-less
//! variant, and [`FindingRoute::reason`] ignores a blank one — a destination
//! written mutely still reads as [`FindingItem::is_open`], which is precisely
//! what the close gate refuses.
//!
//! # Terminal, and never a silent overwrite
//!
//! A finding already carrying a destination answers `already-routed` when the
//! SAME decision is restated (the idempotent no-op), and is REFUSED when a
//! different one is asked for. Rewriting a recorded decision without saying so
//! would lose the very thing this command exists to keep; the refusal names the
//! destination and reason on the record so the caller can see what they would
//! have overwritten.
//!
//! Output (stdout): one line — `routed` | `already-routed` | `error: <reason>`.
//! Exit codes: 0 success/no-op, 1 unresolved spec / unknown id / refused
//! overwrite / failed write, 2 bad args.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec::contract::{FindingItem, FindingRoute};
use mustard_core::{read_meta, write_meta};

use crate::commands::review::ac_negative_check;
use crate::commands::review::finding_collect::one_line;

/// The sidecar the destinations are recorded in — the same file the collector
/// seeds, named by the same constant spelling on both sides.
const META_JSON: &str = "meta.json";

/// The four destination words the CLI publishes, in the order `--help` lists
/// them, spelled as one `<a|b>` placeholder so a printed command line stays one
/// argv token. Every refusal here quotes THIS constant, and so does the close
/// gate's per-finding remediation (`pub(crate)` for exactly that): a reader must
/// never be told to pick from a set that is spelled two ways.
pub(crate) const DESTINATIONS: &str = "criterion|change-request|queued|dropped";

/// Print `error: <msg>` and exit with `code` — the one-line stdout contract
/// `mark-checklist-item` publishes, kept identical so a caller can read either
/// command's answer the same way.
fn die(code: i32, msg: &str) -> ! {
    println!("error: {msg}");
    std::process::exit(code);
}

/// What one `mark-finding` call did.
///
/// Two states, because "the destination was just recorded" and "it was already
/// this one" must not be told apart by silence — the same distinction
/// `mark-checklist-item` publishes as `marked` / `already-marked`. Every
/// refusal is an `Err` instead of a variant: the exit code belongs to [`run`],
/// never to the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkFindingOutcome {
    /// The finding owed a decision and now carries one.
    Routed,
    /// The same destination, with the same reason, was already on the record.
    AlreadyRouted,
}

/// The published word for a destination — the inverse of [`parse_route`], so a
/// message about a recorded route uses the spelling the caller would type.
const fn destination_word(route: &FindingRoute) -> &'static str {
    match route {
        FindingRoute::Criterion(_) => "criterion",
        FindingRoute::ChangeRequest(_) => "change-request",
        FindingRoute::QueuedWork(_) => "queued",
        FindingRoute::Dropped(_) => "dropped",
    }
}

/// Build the destination `to` names, carrying `reason`. `None` for a word the
/// closed set does not contain — a producer this build cannot name must not be
/// folded into one it can.
fn parse_route(to: &str, reason: String) -> Option<FindingRoute> {
    match to {
        "criterion" => Some(FindingRoute::Criterion(reason)),
        "change-request" => Some(FindingRoute::ChangeRequest(reason)),
        "queued" => Some(FindingRoute::QueuedWork(reason)),
        "dropped" => Some(FindingRoute::Dropped(reason)),
        _ => None,
    }
}

/// Resolve `--to` + `--reason` into the route to record, or the message that
/// refuses them.
///
/// Pure and separate from [`run`] on purpose: this is where "a destination
/// without a stated reason is not a destination" is enforced for the caller,
/// and a refusal that only exists inside a `process::exit` path cannot be
/// asserted by a test.
fn resolve_route(to: Option<&str>, reason: Option<&str>) -> Result<FindingRoute, String> {
    let Some(to) = to.map(str::trim).filter(|t| !t.is_empty()) else {
        return Err(format!("--to is required: one of {DESTINATIONS}"));
    };
    let Some(reason) = reason.map(one_line).filter(|r| !r.is_empty()) else {
        return Err(
            "--to requires --reason \"<why>\": a destination without a stated reason is not a \
             destination — it is the same silence the finding started in"
                .to_string(),
        );
    };
    parse_route(to, reason)
        .ok_or_else(|| format!("unknown destination \"{to}\": one of {DESTINATIONS}"))
}

/// Record `route` as the destination of the finding `id` carries in `spec`.
///
/// The project root is a PARAMETER for the reason the collector takes one: the
/// tool cuts a worktree per work unit, so the engine runs off-root as a matter
/// of course — and a test must be able to name its own tree. The spec is
/// resolved through [`ac_negative_check::resolve_spec_file`], the SAME rule the
/// collector uses, because two resolvers are how the writer and the seeder end
/// up pointing at different specs for one name.
pub(crate) fn mark(
    root: &Path,
    spec: &str,
    id: &str,
    route: FindingRoute,
) -> Result<MarkFindingOutcome, String> {
    let Some(spec_dir) = ac_negative_check::resolve_spec_file(root, spec)
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
    else {
        return Err(format!("spec not found: {spec}"));
    };
    let meta_path = spec_dir.join(META_JSON);
    let Some(mut meta) = read_meta(&meta_path) else {
        return Err(format!(
            "spec \"{spec}\" carries no readable {META_JSON}, so no finding was ever seeded — \
             take a collection with `mustard-rt run finding-collect --spec {spec}` first"
        ));
    };

    // Located by id alone: the two producers mint disjoint id spaces by
    // construction (a reviewer finding is `F-<file stem>`, a ledger finding is
    // the criterion's own id), so one id names one finding.
    let Some(idx) = meta.findings.iter().position(|f| f.id == id) else {
        return Err(unknown_id_message(spec, id, &meta.findings));
    };

    if let Some(recorded) = meta.findings[idx].route() {
        if *recorded == route {
            return Ok(MarkFindingOutcome::AlreadyRouted);
        }
        return Err(format!(
            "finding \"{id}\" already went to `{word}` — \"{reason}\". A destination is a decision \
             on the record, and this command never overwrites one silently",
            word = destination_word(recorded),
            reason = recorded.reason().unwrap_or_default(),
        ));
    }

    meta.findings[idx].routed = Some(route);
    write_meta(&meta_path, &meta).map_err(|e| format!("cannot write {META_JSON}: {e}"))?;
    Ok(MarkFindingOutcome::Routed)
}

/// The refusal for an id no finding carries. It names the ids that ARE on the
/// record (or says the record is empty), because "no such finding" alone leaves
/// the caller guessing between a typo and a collection nobody took.
fn unknown_id_message(spec: &str, id: &str, findings: &[FindingItem]) -> String {
    if findings.is_empty() {
        return format!(
            "spec \"{spec}\" carries no findings — take a collection with `mustard-rt run \
             finding-collect --spec {spec}` first"
        );
    }
    let known = findings
        .iter()
        .map(|f| f.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("no finding with id \"{id}\" in spec \"{spec}\"; collected ids: {known}")
}

/// Dispatch `mustard-rt run mark-finding`.
pub fn run(spec: Option<&str>, id: Option<&str>, to: Option<&str>, reason: Option<&str>) {
    let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) else {
        die(2, "--spec is required");
    };
    let Some(id) = id.map(str::trim).filter(|i| !i.is_empty()) else {
        die(2, "--id is required: the finding id `finding-collect` reported");
    };
    let route = match resolve_route(to, reason) {
        Ok(route) => route,
        Err(message) => die(2, &message),
    };
    let root = PathBuf::from(crate::shared::context::project_dir());
    match mark(&root, spec, id, route) {
        Ok(MarkFindingOutcome::Routed) => println!("routed"),
        Ok(MarkFindingOutcome::AlreadyRouted) => println!("already-routed"),
        Err(message) => die(1, &message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::spec::contract::FindingSource;
    use tempfile::tempdir;

    /// Seed a spec directory carrying one open finding; returns
    /// `(project, spec_dir)`. The spec is addressed by DIRECTORY, one of the
    /// three spellings `resolve_spec_file` accepts.
    fn seed(findings: &str) -> (tempfile::TempDir, PathBuf) {
        let project = tempdir().unwrap();
        let spec_dir = project.path().join(".claude").join("spec").join("demo");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Demo\n").unwrap();
        std::fs::write(
            spec_dir.join(META_JSON),
            format!(r#"{{"stage":"Execute","outcome":"Active","findings":{findings}}}"#),
        )
        .unwrap();
        (project, spec_dir)
    }

    /// One open reviewer finding.
    const ONE_OPEN: &str =
        r#"[{"id":"F-findings","source":"review","statement":"the close gate never reads this"}]"#;

    /// The four published words each mint their own destination, and the
    /// reason is folded to one line before it is stored.
    #[test]
    fn mark_finding_parses_the_four_destinations() {
        assert_eq!(
            resolve_route(Some("criterion"), Some("asserts the gate refuses")),
            Ok(FindingRoute::Criterion("asserts the gate refuses".to_string()))
        );
        assert_eq!(
            resolve_route(Some("change-request"), Some("rewrite AC-1")),
            Ok(FindingRoute::ChangeRequest("rewrite AC-1".to_string()))
        );
        assert_eq!(
            resolve_route(Some("queued"), Some("queued as a follow-up spec")),
            Ok(FindingRoute::QueuedWork("queued as a follow-up spec".to_string()))
        );
        assert_eq!(
            resolve_route(Some("dropped"), Some("duplicate of\n  F-1")),
            Ok(FindingRoute::Dropped("duplicate of F-1".to_string())),
            "a multi-line reason is folded, so one finding stays one record"
        );
        // And the word a destination reports is the word the caller types.
        assert_eq!(
            destination_word(&FindingRoute::QueuedWork("later".to_string())),
            "queued"
        );
    }

    /// A destination with nothing stated is refused before anything is read or
    /// written — the whole point of the door.
    #[test]
    fn mark_finding_refuses_a_destination_without_a_reason() {
        for reason in [None, Some(""), Some("   \n ")] {
            let refusal = resolve_route(Some("dropped"), reason).expect_err("must refuse");
            assert!(refusal.contains("--reason"), "{refusal}");
        }
        let missing_to = resolve_route(None, Some("why")).expect_err("must refuse");
        assert!(missing_to.contains(DESTINATIONS), "{missing_to}");
        let unknown = resolve_route(Some("later"), Some("why")).expect_err("must refuse");
        assert!(unknown.contains(DESTINATIONS), "{unknown}");
    }

    /// The destination lands in the sidecar WITH its reason, the finding stops
    /// being open, restating the SAME decision is an idempotent no-op — and a
    /// destination stated with no reason is refused. Both halves live here
    /// because both halves are what AC-6 claims; the refusal is covered in more
    /// detail by its own test above.
    #[test]
    fn mark_finding_records_route_and_refuses_without_reason() {
        let (project, spec_dir) = seed(ONE_OPEN);
        let spec_arg = spec_dir.to_str().unwrap().to_string();
        let route = FindingRoute::QueuedWork("queued as a follow-up spec".to_string());

        assert_eq!(
            mark(project.path(), &spec_arg, "F-findings", route.clone()),
            Ok(MarkFindingOutcome::Routed)
        );

        let meta = read_meta(&spec_dir.join(META_JSON)).expect("reads");
        let finding = &meta.findings[0];
        assert_eq!(finding.source, FindingSource::Review);
        assert_eq!(
            finding.route().and_then(FindingRoute::reason),
            Some("queued as a follow-up spec")
        );
        assert!(!finding.is_open(), "a routed finding owes nobody a decision");

        assert_eq!(
            mark(project.path(), &spec_arg, "F-findings", route),
            Ok(MarkFindingOutcome::AlreadyRouted),
            "restating the same decision changes nothing and says so"
        );

        // The other half AC-6 claims: a destination stated with no reason is
        // refused before anything is read or written. A destination recorded
        // mutely is exactly what `is_open` would keep counting as owed work.
        let refusal = resolve_route(Some("queued"), Some("  ")).expect_err("must refuse");
        assert!(refusal.contains("--reason"), "{refusal}");
    }

    /// A decision already on the record is never overwritten in silence, and
    /// the refusal names what it would have replaced.
    #[test]
    fn mark_finding_refuses_to_overwrite_a_decided_destination() {
        let (project, spec_dir) = seed(ONE_OPEN);
        let spec_arg = spec_dir.to_str().unwrap().to_string();
        let first = FindingRoute::Dropped("duplicate of F-1".to_string());
        assert_eq!(
            mark(project.path(), &spec_arg, "F-findings", first),
            Ok(MarkFindingOutcome::Routed)
        );

        let refusal = mark(
            project.path(),
            &spec_arg,
            "F-findings",
            FindingRoute::Criterion("assert it instead".to_string()),
        )
        .expect_err("a second, different destination must be refused");
        assert!(refusal.contains("dropped"), "{refusal}");
        assert!(refusal.contains("duplicate of F-1"), "{refusal}");

        let meta = read_meta(&spec_dir.join(META_JSON)).expect("reads");
        assert_eq!(
            meta.findings[0].route().and_then(FindingRoute::reason),
            Some("duplicate of F-1"),
            "the first decision stands"
        );
    }

    /// A mute route — a destination written with a blank reason — is still the
    /// OPEN position, so this command settles it instead of refusing.
    #[test]
    fn mark_finding_settles_a_route_recorded_without_a_reason() {
        let (project, spec_dir) = seed(
            r#"[{"id":"AC-1","source":"proof_ledger","statement":"survived the removal",
                 "routed":{"kind":"dropped","reason":"   "}}]"#,
        );
        let spec_arg = spec_dir.to_str().unwrap().to_string();
        assert_eq!(
            mark(
                project.path(),
                &spec_arg,
                "AC-1",
                FindingRoute::Criterion("rewrite it to assert the behaviour".to_string())
            ),
            Ok(MarkFindingOutcome::Routed)
        );
        let meta = read_meta(&spec_dir.join(META_JSON)).expect("reads");
        assert!(!meta.findings[0].is_open());
    }

    /// An id nobody carries is refused with the ids that exist; a spec with no
    /// findings at all is told to take a collection first.
    #[test]
    fn mark_finding_refuses_an_unknown_id() {
        let (project, spec_dir) = seed(ONE_OPEN);
        let spec_arg = spec_dir.to_str().unwrap().to_string();
        let route = FindingRoute::Dropped("x".to_string());

        let typo = mark(project.path(), &spec_arg, "F-finding", route.clone())
            .expect_err("an unknown id must be refused");
        assert!(typo.contains("F-findings"), "the real ids must be named: {typo}");

        let (empty_project, empty_dir) = seed("[]");
        let empty = mark(
            empty_project.path(),
            empty_dir.to_str().unwrap(),
            "F-findings",
            route.clone(),
        )
        .expect_err("no findings at all must be refused");
        assert!(empty.contains("finding-collect"), "{empty}");

        let missing = mark(project.path(), "no-such-spec", "F-findings", route)
            .expect_err("an unresolved spec must be refused");
        assert!(missing.contains("spec not found"), "{missing}");
    }
}
