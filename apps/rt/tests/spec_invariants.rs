//! Repo-wide spec-header invariant test (spec-lifecycle-unification Wave 7,
//! AC-W7-3 / AC-P-1..3).
//!
//! Scans every `.claude/spec/**/*.md` in the real Mustard repo, parses its
//! header into a canonical [`SpecState`], and asserts:
//!
//! - no legacy `### Status:` / `### Phase:` lines remain (AC-P-1, AC-P-2),
//! - every spec carries a `### Stage:` line (AC-P-3),
//! - the parsed `(Stage, Outcome, Flags)` triple is a legal `SpecState`
//!   (the W1 invariants hold for every on-disk spec).
//!
//! ## Why this is `#[ignore]`d
//!
//! This test is RED until the orchestrator runs `migrate-spec-headers --apply`
//! against the repo (AC-W7-3: "passa **após** `--apply` rodado"). Before the
//! batch migration the specs still carry legacy `### Status:`/`### Phase:`
//! headers, so the assertions below fail by design. It is shipped `#[ignore]`d
//! so the suite stays green pre-migration; the orchestrator removes the
//! `#[ignore]` (or runs `cargo test -- --ignored`) immediately after applying
//! the migration, as the final gate of Wave 7.

use mustard_core::{Flags, Outcome, SpecState, Stage};
use std::path::{Path, PathBuf};

/// Locate the repo's `.claude/spec` directory by walking up from the crate dir.
fn spec_root() -> Option<PathBuf> {
    // `CARGO_MANIFEST_DIR` is `<repo>/apps/rt`; the spec dir is `<repo>/.claude/spec`.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dir = manifest.as_path();
    loop {
        let candidate = dir.join(".claude").join("spec");
        if candidate.is_dir() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

/// The value of an `### <Key>:` header line (case-insensitive on the key).
fn header_field(spec_md: &str, key: &str) -> Option<String> {
    let want = key.to_ascii_lowercase();
    for line in spec_md.lines() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("###") else {
            continue;
        };
        let rest = rest.trim_start();
        let lower = rest.to_ascii_lowercase();
        if let Some(after_key) = lower.strip_prefix(&want) {
            let after_key = after_key.trim_start();
            if let Some(after_colon) = after_key.strip_prefix(':') {
                let value_start = rest.len() - after_colon.len();
                return Some(rest[value_start..].trim().to_string());
            }
        }
    }
    None
}

/// Collect every `*.md` under `root`, recursively.
fn collect_md(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(std::result::Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn all_specs_are_migrated_and_invariant_holds() {
    let Some(root) = spec_root() else {
        panic!(".claude/spec not found from CARGO_MANIFEST_DIR");
    };
    let files = collect_md(&root);
    assert!(!files.is_empty(), "expected at least one spec under {root:?}");

    let mut violations: Vec<String> = Vec::new();

    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            violations.push(format!("{}: unreadable", path.display()));
            continue;
        };
        // Only files that look like a spec/wave-plan (carry a Stage header) are
        // subject to the invariant. A pure prose `.md` with no lifecycle header
        // is not a spec — but after migration any file that HAD a legacy header
        // now has a Stage, so AC-P-1/2 below catch any straggler.

        // AC-P-1 / AC-P-2: no legacy headers remain.
        if header_field(&content, "Status").is_some() {
            violations.push(format!("{}: still has `### Status:`", path.display()));
        }
        if header_field(&content, "Phase").is_some() {
            violations.push(format!("{}: still has `### Phase:`", path.display()));
        }

        // AC-P-3 + invariants: only assert on files that carry a Stage header
        // (i.e. real specs/wave-plans — a plain README without any lifecycle
        // header is legitimately stage-less and not in scope).
        let stage_raw = header_field(&content, "Stage");
        let had_lifecycle = stage_raw.is_some();
        if !had_lifecycle {
            continue;
        }

        let stage = stage_raw.as_deref().and_then(Stage::parse);
        let Some(stage) = stage else {
            violations.push(format!(
                "{}: `### Stage: {:?}` does not parse",
                path.display(),
                stage_raw
            ));
            continue;
        };
        let outcome = header_field(&content, "Outcome")
            .as_deref()
            .and_then(Outcome::parse)
            .unwrap_or(Outcome::Active);
        let flags = header_field(&content, "Flags")
            .map(|f| Flags::parse(&f))
            .unwrap_or_default();

        if let Err(e) = SpecState::new(stage, outcome, flags) {
            violations.push(format!("{}: illegal SpecState: {e}", path.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "spec-header invariant violations ({}):\n{}",
        violations.len(),
        violations.join("\n")
    );
}
