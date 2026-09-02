// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! Drift ratchet between the doctor's hook-event set and the shipped
//! `plugin/hooks/hooks.json` manifest.
//!
//! The wiring check used to validate `mustard-rt on <event>` command strings
//! against a hand-written list. It drifted in both directions: it carried
//! `PreCompact`, which nothing registers, and omitted `Stop` and
//! `WorktreeCreate`, which are registered. `doctor::known_hook_events` now
//! derives the set from the manifest; this test reads the manifest a second,
//! independent time off disk and fails on a disagreement either way — so a
//! reverted derivation, or a parser that stops seeing a shape the manifest
//! uses, is a test failure rather than a silent FAIL in the field.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt known_events_match_shipped_hooks -- --exact`, and
//! libtest matches `--exact` against the FULL test path — which equals the bare
//! function name only at the root of an integration-test binary.

use mustard_rt::commands::doctor::doctor::known_hook_events;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Locate the shipped hook manifest by walking up from the crate directory.
/// `CARGO_MANIFEST_DIR` is `<repo>/apps/rt`; the manifest is
/// `<repo>/plugin/hooks/hooks.json`.
fn shipped_manifest_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dir = manifest.as_path();
    loop {
        let candidate = dir.join("plugin").join("hooks").join("hooks.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

/// The event names the manifest registers — read straight off disk, so the
/// assertion compares two derivations instead of one value against itself.
fn shipped_events() -> BTreeSet<String> {
    let path = shipped_manifest_path().expect("plugin/hooks/hooks.json must be reachable");
    let text = std::fs::read_to_string(&path).expect("hooks.json must be readable");
    let manifest: serde_json::Value =
        serde_json::from_str(&text).expect("hooks.json must be valid JSON");
    manifest
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .expect("hooks.json must carry a `hooks` object")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn known_events_match_shipped_hooks() {
    let shipped = shipped_events();
    assert!(
        !shipped.is_empty(),
        "the shipped manifest registers no hook event — the fixture, not the doctor, is broken"
    );

    let known = known_hook_events();
    assert!(
        !known.is_empty(),
        "doctor derived an empty event set — the embedded manifest did not parse"
    );

    let missing: Vec<&String> = shipped.difference(&known).collect();
    assert!(
        missing.is_empty(),
        "shipped hook events the doctor would call unknown: {missing:?}"
    );

    let extra: Vec<&String> = known.difference(&shipped).collect();
    assert!(
        extra.is_empty(),
        "hook events the doctor accepts but nothing ships: {extra:?}"
    );
}

/// AC-3 — every way a session can start is covered by a matcher.
///
/// `SessionStart` fires with one of five sources: `startup`, `resume`, `clear`,
/// `compact`, `fork`. A source no matcher names gets no hook at all, so the
/// window opens with the router absent and nothing says so — which is exactly
/// what `fork` did until this unit.
#[test]
fn sessionstart_matchers_cover_fork() {
    let path = shipped_manifest_path().expect("plugin/hooks/hooks.json must be reachable");
    let text = std::fs::read_to_string(&path).expect("hooks.json must be readable");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

    let matchers: Vec<String> = manifest["hooks"]["SessionStart"]
        .as_array()
        .expect("SessionStart must register at least one entry")
        .iter()
        .filter_map(|e| e.get("matcher").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect();

    for source in ["startup", "resume", "clear", "compact", "fork"] {
        assert!(
            matchers.iter().any(|m| m.split('|').any(|alt| alt.trim() == source)),
            "no SessionStart matcher covers `{source}` — a session started that way \
             opens with no hook at all. Registered: {matchers:?}",
        );
    }
}

/// Each router injectable is claimed by its OWN sibling hook.
///
/// The 10,000-character `additionalContext` ceiling is per hook RESPONSE, and
/// Claude Code keeps the context of every sibling (measured 2026-08-25). One
/// hook per injectable is what turns that into a ceiling each document owns —
/// registering two on one hook would put them back under a single response.
///
/// The set is asked of the SEED (`injectable_declared_paths`), never listed
/// here. A hand-written list went blind the day a third injectable was added:
/// the new document rode a sibling nothing checked, and the test that exists to
/// prove each one is claimed alone stayed green while covering two of three.
#[test]
fn each_router_injectable_rides_its_own_sibling_hook() {
    let path = shipped_manifest_path().expect("plugin/hooks/hooks.json must be reachable");
    let text = std::fs::read_to_string(&path).expect("hooks.json must be readable");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

    let commands: Vec<String> = manifest["hooks"]["UserPromptSubmit"]
        .as_array()
        .expect("UserPromptSubmit must register at least one entry")
        .iter()
        .flat_map(|entry| {
            entry["hooks"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|h| h.get("command").and_then(serde_json::Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();

    let seeded = mustard_core::injectable_declared_paths();
    assert!(
        !seeded.is_empty(),
        "the seed carries no injectable — this test would pass by measuring nothing"
    );
    for file in seeded {
        let claimants = commands.iter().filter(|c| c.contains(&file)).count();
        assert_eq!(
            claimants, 1,
            "`{file}` is claimed by {claimants} UserPromptSubmit hook(s); it needs exactly \
             one, so it is measured alone against the per-response ceiling. Registered: \
             {commands:?}",
        );
    }
}
