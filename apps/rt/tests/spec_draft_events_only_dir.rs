// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! Drafting a spec into a directory that holds only its own event log.
//!
//! Opening the work unit emits the first harness event, which creates
//! `<spec>/.events/`. The drafter then found the directory already present and
//! refused with "output exists; pass --force to overwrite" — an overwrite flag
//! demanded for a directory with nothing to overwrite, and two steps of one
//! sequence where the first blocked the second.
//!
//! Both directions are asserted: the events-only directory drafts, and a
//! directory holding a REAL spec still refuses without `--force`.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt spec_draft_accepts_an_events_only_directory --
//! --exact`, and libtest matches `--exact` against the FULL test path — which
//! equals the bare function name only at the root of an integration-test binary.

use mustard_rt::commands::spec::spec_draft::{run, SpecDraftOpts};
use std::path::{Path, PathBuf};

/// Draft options with an explicit `--output`, so the run never depends on the
/// process working directory or on the near-duplicate sibling scan.
fn opts(output: &Path) -> SpecDraftOpts {
    SpecDraftOpts {
        intent: "Record the harness safety instruments".into(),
        scope: "light".into(),
        lang: "en-US".into(),
        signals: None,
        output: Some(output.to_path_buf()),
        waves: 0,
        force: false,
        query_terms: None,
        force_scope: false,
    }
}

/// Seed `<dir>/.events/seed.ndjson` — the on-disk shape `work-unit-open` leaves
/// behind when it emits the unit's first event before any spec is drafted.
fn seed_event_log(dir: &Path) -> PathBuf {
    let events = dir.join(".events");
    std::fs::create_dir_all(&events).unwrap();
    let path = events.join("seed.ndjson");
    std::fs::write(
        &path,
        b"{\"event\":\"pipeline.scope\",\"ts\":\"2026-07-24T00:00:00.000Z\",\"v\":1}\n",
    )
    .unwrap();
    path
}

#[test]
fn spec_draft_accepts_an_events_only_directory() {
    // --- 1. A directory holding ONLY the event log drafts ------------------
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("harness-safety-instruments");
    let seeded = seed_event_log(&out);

    run(opts(&out));

    assert!(
        out.join("spec.md").exists(),
        "the draft must proceed into a directory holding only its own event log",
    );
    assert!(out.join("meta.json").exists(), "meta.json must be written too");
    assert!(
        seeded.exists(),
        "the pre-existing event log must survive the draft, not be overwritten",
    );

    // --- 2. A directory holding a REAL draft still refuses -----------------
    // The guard is narrowed, not removed: anything beyond harness state is work
    // that `--force` must still be required to replace.
    let occupied = tmp.path().join("already-drafted");
    std::fs::create_dir_all(&occupied).unwrap();
    std::fs::write(occupied.join("spec.md"), b"# Hand-written spec\n").unwrap();

    run(opts(&occupied));

    let body = std::fs::read_to_string(occupied.join("spec.md")).unwrap();
    assert_eq!(
        body, "# Hand-written spec\n",
        "an existing spec.md must not be overwritten without --force",
    );
    assert!(
        !occupied.join("meta.json").exists(),
        "the refused draft must write nothing at all",
    );
}
