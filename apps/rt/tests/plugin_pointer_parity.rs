//! Parity ratchet for the plugin's INTERNAL pointers — the `refs/` tree and the
//! `${CLAUDE_PLUGIN_ROOT}/…` paths that reach it.
//!
//! Progressive disclosure is the whole design of this corpus: the loaded files
//! stay lean and every detail lives behind a pointer that opens on demand. That
//! makes the pointer the load-bearing part, and it is the part nothing checks.
//! Both ways it can rot are silent at runtime:
//!
//! - **A pointer to a file that is not there.** The flow reaches the line, the
//!   read fails, and the agent continues without the rule it was sent to fetch.
//!   Nothing errors — the instruction simply had no effect.
//! - **A ref nobody points at.** It ships, it is maintained, it is quoted in
//!   review, and no flow ever opens it. Orphaned prose is worse than absent
//!   prose, because it reads as if it were in force.
//!
//! Both counts are zero today. This ratchet is what keeps them there.
//!
//! Deterministic: walks the repo tree only (sorted), no network, no env vars.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The pointer spelling the plugin runtime expands.
const POINTER_PREFIX: &str = "${CLAUDE_PLUGIN_ROOT}/";

/// Pointer targets that are deliberately absent from the checkout. Each row says
/// what puts the file there instead.
///
/// A row is a claim that the path is FILLED AT INSTALL, not that it is optional.
/// The sibling test drops the row the day the file is committed. Kept sorted.
const UNCOMMITTED_POINTER_TARGETS: &[(&str, &str)] = &[(
    "bin/mustard-rt",
    "the compiled runtime, dropped into plugin/bin by `mustard-boot` on first \
     use (plugin/bin/README.md); a build artefact, never a committed file",
)];

/// The repo root, resolved from this crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Read a file as lossy UTF-8; unreadable files degrade to an empty string.
fn read_lossy(path: &Path) -> String {
    fs::read(path).map_or_else(|_| String::new(), |b| String::from_utf8_lossy(&b).into_owned())
}

/// Recursively collect files under `dir` in a deterministic (sorted) order.
fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Every markdown file that can carry a pointer: the shipped plugin tree plus
/// the compiled-in injectables, which reach the refs by the same paths.
fn pointer_corpus(root: &Path) -> Vec<PathBuf> {
    let plugin = root.join("plugin");
    assert!(plugin.is_dir(), "plugin tree missing at {}", plugin.display());
    let injectables = root.join("packages/core/templates/mustard");
    assert!(
        injectables.is_dir(),
        "injectable seeds missing at {}",
        injectables.display()
    );
    let mut files = Vec::new();
    walk_files(&plugin, &mut files);
    walk_files(&injectables, &mut files);
    files.retain(|p| p.extension().and_then(|e| e.to_str()) == Some("md"));
    files
}

/// `true` for a byte that can still belong to a pointed-at path. Deliberately
/// excludes the backtick, the quote and the closing paren that end a path in
/// prose, so `` `${CLAUDE_PLUGIN_ROOT}/x.md` `` yields `x.md` and not ``x.md` ``.
fn is_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/')
}

/// Every `${CLAUDE_PLUGIN_ROOT}/…` path a text spells, plugin-relative.
fn pointer_targets(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(POINTER_PREFIX) {
        let start = from + at + POINTER_PREFIX.len();
        from = start;
        let mut end = start;
        while end < bytes.len() && is_path_byte(bytes[end]) {
            end += 1;
        }
        // A path ending in `.` is prose that ran into a full stop.
        let target = text[start..end].trim_end_matches('.');
        if !target.is_empty() {
            out.push(target.to_string());
        }
    }
    out
}

/// Every shipped `plugin/refs/**/*.md`, as a plugin-relative path, sorted.
fn shipped_refs(root: &Path) -> Vec<String> {
    let dir = root.join("plugin/refs");
    assert!(dir.is_dir(), "refs tree missing at {}", dir.display());
    let mut files = Vec::new();
    walk_files(&dir, &mut files);
    let plugin = root.join("plugin");
    let mut rels: Vec<String> = files
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter_map(|p| p.strip_prefix(&plugin).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    rels.sort();
    rels
}

/// Every `${CLAUDE_PLUGIN_ROOT}/…` pointer resolves to a file that is there.
///
/// The prefix is expanded by Claude Code at read time, so a stale pointer is
/// indistinguishable from a live one until the flow reaches it — and then the
/// only symptom is a rule that did not apply.
#[test]
fn every_plugin_pointer_resolves() {
    let root = repo_root();
    let plugin = root.join("plugin");
    let mut broken = Vec::new();
    let mut seen = 0usize;

    for file in pointer_corpus(&root) {
        for target in pointer_targets(&read_lossy(&file)) {
            seen += 1;
            if plugin.join(&target).exists()
                || UNCOMMITTED_POINTER_TARGETS.iter().any(|(t, _)| *t == target)
            {
                continue;
            }
            let shown = file.strip_prefix(&root).unwrap_or(&file);
            broken.push(format!("{} -> {POINTER_PREFIX}{target}", shown.display()));
        }
    }
    assert!(seen > 0, "no {POINTER_PREFIX} pointers found at all - the scan is broken");
    assert!(
        broken.is_empty(),
        "plugin pointers that resolve to nothing. The read fails at runtime and \
         the flow continues WITHOUT the rule it was sent to fetch - no error, no \
         symptom. Fix the path or ship the file:\n{}",
        broken.join("\n")
    );
}

/// Every file in `refs/` is pointed at by something else.
///
/// A ref exists only to be opened on demand; one nothing opens is prose that
/// reads as in force and is not. Both spellings count — the `${CLAUDE_PLUGIN_ROOT}`
/// form the commands use and the bare `refs/…` form the injectables use — because
/// what is being asked is whether a reader is ever SENT there, not which syntax
/// sent them. A file naming itself is not a pointer.
#[test]
fn every_ref_file_is_pointed_at() {
    let root = repo_root();
    let plugin = root.join("plugin");
    let corpus = pointer_corpus(&root);
    let refs = shipped_refs(&root);
    assert!(!refs.is_empty(), "plugin/refs ships no *.md - the tree this guard locks is gone");

    let mut orphans = Vec::new();
    for rel in &refs {
        let own_path = plugin.join(rel);
        let pointed = corpus
            .iter()
            .filter(|p| **p != own_path)
            .any(|p| read_lossy(p).contains(rel.as_str()));
        if !pointed {
            orphans.push(rel.clone());
        }
    }
    assert!(
        orphans.is_empty(),
        "refs nothing points at. A ref is loaded on demand or not at all, so an \
         unreachable one is maintained prose that never reaches a flow - point a \
         command or an injectable at it, or delete it:\n{}",
        orphans.join("\n")
    );
}

/// The uncommitted-target list stays sorted, justified, and necessary.
#[test]
fn uncommitted_pointer_targets_stay_sorted_and_not_redundant() {
    let root = repo_root();
    let plugin = root.join("plugin");
    let corpus: BTreeSet<String> = pointer_corpus(&root)
        .iter()
        .flat_map(|p| pointer_targets(&read_lossy(p)))
        .collect();

    for pair in UNCOMMITTED_POINTER_TARGETS.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "UNCOMMITTED_POINTER_TARGETS must stay sorted: {} before {}",
            pair[0].0,
            pair[1].0
        );
    }
    for (target, why) in UNCOMMITTED_POINTER_TARGETS {
        assert!(
            !why.trim().is_empty(),
            "UNCOMMITTED_POINTER_TARGETS entry {target} carries no justification"
        );
        assert!(
            corpus.contains(*target),
            "UNCOMMITTED_POINTER_TARGETS entry {target} is pointed at by nothing - \
             drop the row, there is no pointer left to excuse"
        );
        assert!(
            !plugin.join(target).exists(),
            "UNCOMMITTED_POINTER_TARGETS entry {target} IS in the checkout now - the \
             row is redundant, drop it and let the pointer be checked like any other"
        );
    }
}
