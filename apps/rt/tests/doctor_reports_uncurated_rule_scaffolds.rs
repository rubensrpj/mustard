// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! The doctor's `guards-scaffold` advisory must name the subprojects whose
//! `## Guards` block is still the `/scan` scaffold.
//!
//! Two-sided by construction: the fixture carries BOTH an uncurated subproject
//! and an enriched one, and the assertion checks each side. A check that
//! reported every subproject — or none — would pass a one-sided test and be
//! useless in the field, which is exactly the failure this guards against.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt doctor_reports_uncurated_rule_scaffolds -- --exact`,
//! and libtest matches `--exact` against the FULL test path — which equals the
//! bare function name only at the root of an integration-test binary.

use mustard_rt::commands::doctor::guards_scaffold_check;
use mustard_rt::commands::scan_claude::{GUARDS_CLOSE, GUARDS_DONE_OPEN, GUARDS_PENDING_OPEN};
use std::path::Path;

/// Seed the scan census. Without it the check is a deliberate silent no-op.
fn seed_census(root: &Path) {
    let claude = root.join(".claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::write(claude.join("grain.model.json"), "{}").unwrap();
}

/// Seed a subproject `CLAUDE.md` whose `## Guards` block is the UNCURATED
/// scaffold Wave 1 emits (body = HTML comments only).
fn seed_uncurated(root: &Path, subproject: &str) {
    let dir = root.join(subproject);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("CLAUDE.md"),
        format!(
            "# {subproject}\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n\
             <!-- facts: kind=cargo; frameworks=serde -->\n{GUARDS_CLOSE}\n"
        ),
    )
    .unwrap();
}

/// Seed a subproject `CLAUDE.md` whose `## Guards` block has been ENRICHED.
fn seed_curated(root: &Path, subproject: &str) {
    let dir = root.join(subproject);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("CLAUDE.md"),
        format!(
            "# {subproject}\n\n## Guards\n\n{GUARDS_DONE_OPEN}\n\
             <!-- facts: kind=cargo; frameworks=serde -->\n\
             - DO keep every hook fail-open\n{GUARDS_CLOSE}\n"
        ),
    )
    .unwrap();
}

#[test]
fn doctor_reports_uncurated_rule_scaffolds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_census(root);
    seed_uncurated(root, "apps/pending");
    seed_curated(root, "apps/enriched");

    let report = guards_scaffold_check::run(root)
        .expect("a scan census is present — the check must judge, not skip");

    // Side 1: the uncurated scaffold is reported.
    assert!(!report.ok, "an uncurated scaffold must not report ok: {report:?}");
    assert_eq!(report.total_uncurated, 1, "{report:?}");
    let named: Vec<&str> = report.uncurated.iter().map(|u| u.subproject.as_str()).collect();
    assert_eq!(named, vec!["apps/pending"], "{report:?}");
    assert_eq!(report.uncurated[0].kind, "cargo", "the mined kind must survive: {report:?}");

    // Side 2: the enriched one is NOT — so the check cannot pass by reporting
    // every subproject it walks.
    assert!(
        !named.contains(&"apps/enriched"),
        "a curated subproject must never be reported: {report:?}"
    );

    // Advisory, never blocking: nothing above exits the process, and the
    // fail-open evidence channel is present and empty for a healthy fixture.
    assert!(report.scanned_errors.is_empty(), "{report:?}");

    // And with the scaffold enriched, the very same tree reports clean — the
    // report tracks the block's state, not the mere existence of a subproject.
    seed_curated(root, "apps/pending");
    let after = guards_scaffold_check::run(root).expect("census still present");
    assert!(after.ok, "every block curated ⇒ ok: {after:?}");
    assert_eq!(after.total_uncurated, 0, "{after:?}");
}
