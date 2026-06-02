//! Repo-wide spec-metadata invariant test.
//!
//! Since the meta-sidecar migration, **`meta.json` is the single source of
//! truth** for every machine-parseable lifecycle field and `spec.md` carries no
//! lifecycle header at all. This test scans every `.claude/spec/**/spec.md` (and
//! `wave-plan.md`) in the real Mustard repo and asserts:
//!
//! - no metadata header lines remain in the markdown — neither the legacy
//!   `### Status:` / `### Phase:` nor the canonical `### Stage:` / `### Outcome:`
//!   / `### Flags:` / `### Scope:` / `### Lang:` / `### Checkpoint:` /
//!   `### Parent:` / `### Total waves:`,
//! - every spec dir carries a `meta.json` whose parsed `(Stage, Outcome, Flags)`
//!   triple is a legal `SpecState` (the W1 invariants hold for every on-disk
//!   spec).
//!
//! ## Empty workspace
//!
//! The test resolves `.claude/spec` from `CARGO_MANIFEST_DIR`. A clean checkout
//! / sandbox may have no specs on disk; there is then nothing to validate, so
//! the test **skips** (the invariant holds vacuously) rather than failing the
//! suite on an empty workspace.

use mustard_core::{read_meta, Flags, Outcome, SpecState, Stage};
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

/// `true` when an `### <Key>:` header line (case-insensitive on the key) is
/// present outside fenced code blocks. Lines inside ```` ``` ```` fences are
/// ignored — a documentation example like `### Stage: {stage}` is illustrative,
/// not a real header.
fn has_header(spec_md: &str, key: &str) -> bool {
    let want = key.to_ascii_lowercase();
    let mut in_fence = false;
    for line in spec_md.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("###") else {
            continue;
        };
        let rest = rest.trim_start();
        let lower = rest.to_ascii_lowercase();
        if let Some(after_key) = lower.strip_prefix(&want) {
            if after_key.trim_start().starts_with(':') {
                return true;
            }
        }
    }
    false
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

/// The metadata header keys that must never appear in a spec markdown body.
const METADATA_KEYS: &[&str] = &[
    "Status", "Phase", "Stage", "Outcome", "Flags", "Scope", "Lang", "Checkpoint",
    "Parent", "Total waves",
];

#[test]
fn no_metadata_headers_remain_and_meta_json_is_valid() {
    let Some(root) = spec_root() else {
        eprintln!("[skip] .claude/spec not found from CARGO_MANIFEST_DIR — nothing to validate");
        return;
    };
    let files = collect_md(&root);
    if files.is_empty() {
        // Environmental: a clean checkout / sandbox has no specs on disk. The
        // invariant holds vacuously, so skip rather than fail an empty workspace.
        eprintln!("[skip] no spec markdown under {root:?} — nothing to validate");
        return;
    }

    let mut violations: Vec<String> = Vec::new();

    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            violations.push(format!("{}: unreadable", path.display()));
            continue;
        };

        // No metadata header line may remain in the markdown.
        for key in METADATA_KEYS {
            if has_header(&content, key) {
                violations.push(format!("{}: still has `### {key}:` header", path.display()));
            }
        }

        // Every `spec.md` / `wave-plan.md` must have a `meta.json` beside it with
        // a legal lifecycle triple — EXCEPT inside a `qa/` or `review/` phase
        // directory (D3): those are pipeline phases, not specs, so they carry no
        // lifecycle sidecar (their result lives in `report.md` / `verdict.md`).
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let parent_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let is_phase_dir = matches!(parent_name, "qa" | "review");
        if (name == "spec.md" || name == "wave-plan.md") && !is_phase_dir {
            let Some(dir) = path.parent() else { continue };
            let meta_path = dir.join("meta.json");
            let Some(meta) = read_meta(&meta_path) else {
                violations.push(format!("{}: missing/unreadable meta.json sidecar", path.display()));
                continue;
            };
            let Some(stage) = meta.stage.as_deref().and_then(Stage::parse) else {
                violations.push(format!(
                    "{}: meta.json stage {:?} does not parse",
                    path.display(),
                    meta.stage
                ));
                continue;
            };
            let outcome = meta
                .outcome
                .as_deref()
                .and_then(Outcome::parse)
                .unwrap_or(Outcome::Active);
            // Qualifier flags now live in `meta.json#flags` — read them so the
            // legality check covers the persisted triple.
            let flags: Flags = meta.flags.clone().into();
            if let Err(e) = SpecState::new(stage, outcome, flags) {
                violations.push(format!("{}: illegal SpecState from meta.json: {e}", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "spec-metadata invariant violations ({}):\n{}",
        violations.len(),
        violations.join("\n")
    );
}
