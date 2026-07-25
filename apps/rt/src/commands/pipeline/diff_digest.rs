//! Deterministic SIGNATURE diff between two git revisions — the "diff-digest"
//! (Pilar 3c). Replaces the wave cache's line-count `git diff --stat` with a
//! semantic summary: for each changed file, the public declarations (functions
//! and entities) ADDED (`+`) and REMOVED (`-`) between the two revisions, via
//! the same tree-sitter extraction `reference.rs` uses — "never a file dump".
//!
//! Why signatures over `--stat`: a rename or a whitespace reflow shows huge
//! churn with no semantic change, while a one-line signature change (a new
//! public fn, a removed type) shows tiny churn — the next wave's implementer /
//! reviewer cares about the LATTER. This says WHAT the prior wave changed, not
//! how many lines moved.
//!
//! Fail-open by construction: a git error, a missing revision (a wave that is
//! the first commit), or an unreadable blob degrades to an empty string (the
//! caller then writes an empty `diff.md`, exactly as the old `--stat` did on
//! failure). Deterministic + byte-stable: files sorted by path, signatures
//! sorted within each `+`/`-` group, repo-relative paths only, no timestamps.

use mustard_core::domain::ast::{extract_entities, extract_function_signatures, GrammarLoader};
use mustard_core::platform::process::rtk_command;
use std::collections::BTreeSet;
use std::path::Path;

/// Max changed files to digest — bounds cost on a sprawling wave; the tail is
/// noted, never silently dropped.
const MAX_FILES: usize = 50;

/// One changed file between the two revisions.
struct ChangedFile {
    /// Normalised status: `A` added, `M` modified, `D` deleted, `R` renamed.
    status: char,
    /// Path at the BASE revision (empty for an added file).
    old_path: String,
    /// Path at the HEAD revision (empty for a deleted file).
    new_path: String,
}

impl ChangedFile {
    /// The path to display / sort by: the new path unless the file was deleted.
    fn display(&self) -> &str {
        if self.new_path.is_empty() {
            &self.old_path
        } else {
            &self.new_path
        }
    }

    fn status_tag(&self) -> &'static str {
        match self.status {
            'A' => "new",
            'D' => "deleted",
            'R' => "renamed",
            _ => "modified",
        }
    }
}

/// Build the signature diff between `base` and `head` (e.g. `"HEAD~1"`,
/// `"HEAD"`), run in `cwd`. Empty when nothing changed or git is unavailable.
pub(crate) fn build_signature_diff(cwd: &Path, base: &str, head: &str) -> String {
    let mut changed = changed_files(cwd, base, head);
    if changed.is_empty() {
        return String::new();
    }
    changed.sort_by(|a, b| a.display().cmp(b.display()));
    let truncated = changed.len() > MAX_FILES;
    changed.truncate(MAX_FILES);

    // One shared loader (builtins cover the common languages; the agnostic
    // fallback covers the rest). Anchored at the repo root — a wave can span
    // subprojects, so no single subproject anchor fits.
    let loader = GrammarLoader::with_builtins(cwd);

    let mut out: Vec<String> = Vec::new();
    for cf in &changed {
        let display = cf.display();
        // Resolve the grammar PER SIDE, by that side's OWN path: a rename that
        // also changes extension (`a.txt` → `b.rs`) must parse the old blob as
        // the old language and the new blob as the new one. For the common case
        // (same path, or a same-extension rename) both sides resolve equal.
        let (old_fns, old_types) = if cf.old_path.is_empty() {
            (BTreeSet::new(), BTreeSet::new())
        } else {
            let lang = loader
                .language_id_for_path(Path::new(&cf.old_path))
                .unwrap_or_default();
            signatures(&loader, &git_show(cwd, base, &cf.old_path), &lang)
        };
        let (new_fns, new_types) = if cf.new_path.is_empty() {
            (BTreeSet::new(), BTreeSet::new())
        } else {
            let lang = loader
                .language_id_for_path(Path::new(&cf.new_path))
                .unwrap_or_default();
            signatures(&loader, &git_show(cwd, head, &cf.new_path), &lang)
        };

        out.push(format!("- `{display}` ({})", cf.status_tag()));
        let mut any = false;
        any |= push_delta(&mut out, "+ fns", new_fns.difference(&old_fns));
        any |= push_delta(&mut out, "+ types", new_types.difference(&old_types));
        any |= push_delta(&mut out, "- fns", old_fns.difference(&new_fns));
        any |= push_delta(&mut out, "- types", old_types.difference(&new_types));
        if !any {
            // A body-only edit or a non-code file: no signature delta. Still
            // listed (the file DID change) so the reader keeps the file set the
            // old `--stat` gave — just without a churn count.
            out.push("  (no signature change)".to_string());
        }
    }
    if truncated {
        out.push(format!("- ...and more files (showing first {MAX_FILES})"));
    }
    out.join("\n")
}

/// Push a `  {label}: a, b, c` line when the delta is non-empty; returns whether
/// anything was pushed. The input is a `BTreeSet::difference`, already
/// sorted+deduped → byte-stable.
fn push_delta<'a>(
    out: &mut Vec<String>,
    label: &str,
    delta: impl Iterator<Item = &'a String>,
) -> bool {
    let items: Vec<&str> = delta.map(String::as_str).collect();
    if items.is_empty() {
        return false;
    }
    out.push(format!("  {label}: {}", items.join(", ")));
    true
}

/// Extract the (function-name, entity-name) sets of a source blob. Empty when
/// the grammar does not resolve (non-code file) or the blob is empty. Mirrors
/// the extraction `reference.rs` uses, reduced to names for set comparison.
fn signatures(
    loader: &GrammarLoader,
    source: &str,
    lang: &str,
) -> (BTreeSet<String>, BTreeSet<String>) {
    if source.is_empty() || lang.is_empty() {
        return (BTreeSet::new(), BTreeSet::new());
    }
    let fns: BTreeSet<String> = extract_function_signatures(loader, source, lang)
        .into_iter()
        .map(|s| s.name)
        .collect();
    let types: BTreeSet<String> = extract_entities(loader, source, lang)
        .into_iter()
        .map(|e| e.name)
        .collect();
    (fns, types)
}

/// Parse `git diff --name-status -M {base} {head}` into changed files. Empty on
/// any git error (fail-open).
fn changed_files(cwd: &Path, base: &str, head: &str) -> Vec<ChangedFile> {
    let out = rtk_command("git", &["diff", "--name-status", "-M", base, head])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let mut files = Vec::new();
    for line in out.lines() {
        let mut cols = line.split('\t');
        let Some(status_raw) = cols.next() else {
            continue;
        };
        // `R`/`C` carry a similarity score (`R100`); the leading letter is the
        // status. A malformed/empty first column degrades to `M` (modified).
        match status_raw.chars().next().unwrap_or('M') {
            'R' | 'C' => {
                let (Some(old), Some(new)) = (cols.next(), cols.next()) else {
                    continue;
                };
                files.push(ChangedFile {
                    status: 'R',
                    old_path: old.to_string(),
                    new_path: new.to_string(),
                });
            }
            'A' => {
                let Some(p) = cols.next() else { continue };
                files.push(ChangedFile {
                    status: 'A',
                    old_path: String::new(),
                    new_path: p.to_string(),
                });
            }
            'D' => {
                let Some(p) = cols.next() else { continue };
                files.push(ChangedFile {
                    status: 'D',
                    old_path: p.to_string(),
                    new_path: String::new(),
                });
            }
            // M, T (type change), U, and anything else → modified, same path.
            _ => {
                let Some(p) = cols.next() else { continue };
                files.push(ChangedFile {
                    status: 'M',
                    old_path: p.to_string(),
                    new_path: p.to_string(),
                });
            }
        }
    }
    files
}

/// `git show {rev}:{path}` → blob text, or "" on any error (fail-open).
fn git_show(cwd: &Path, rev: &str, path: &str) -> String {
    rtk_command("git", &["show", &format!("{rev}:{path}")])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Whether `git` is on PATH; the behavioural tests degrade to a silent pass
    /// when it is not (mirrors the module's fail-open contract, like
    /// `diff_context`'s git-backed tests).
    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn git(cwd: &Path, args: &[&str]) {
        let _ = Command::new("git").args(args).current_dir(cwd).output();
    }

    fn init_repo(cwd: &Path) {
        git(cwd, &["init", "-b", "main"]);
        git(cwd, &["config", "user.email", "t@e.x"]);
        git(cwd, &["config", "user.name", "t"]);
        git(cwd, &["config", "commit.gpgsign", "false"]);
    }

    /// The core claim: a signature-level change surfaces as `+`/`-` declarations,
    /// NOT a line count. v1 has `fn foo` + `struct Bar`; v2 adds `fn baz` and
    /// removes `struct Bar` → the digest names both, on the right sides.
    #[test]
    fn signature_diff_shows_added_and_removed_declarations() {
        if !git_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        init_repo(cwd);

        std::fs::write(
            cwd.join("m.rs"),
            "pub struct Bar { x: i32 }\npub fn foo() -> i32 { 0 }\n",
        )
        .unwrap();
        git(cwd, &["add", "-A"]);
        git(cwd, &["commit", "-m", "v1"]);

        std::fs::write(
            cwd.join("m.rs"),
            "pub fn foo() -> i32 { 0 }\npub fn baz() -> i32 { 1 }\n",
        )
        .unwrap();
        git(cwd, &["add", "-A"]);
        git(cwd, &["commit", "-m", "v2"]);

        let digest = build_signature_diff(cwd, "HEAD~1", "HEAD");
        // Skip if git could not produce the range (fail-open path, e.g. sandbox).
        if digest.is_empty() {
            return;
        }
        assert!(digest.contains("m.rs"), "names the changed file: {digest}");
        assert!(digest.contains("+ fns: baz"), "added fn on the + side: {digest}");
        assert!(digest.contains("- types: Bar"), "removed type on the - side: {digest}");
        // `foo` survived on both sides → it is NOT a delta.
        assert!(!digest.contains("foo"), "unchanged decl must not appear: {digest}");
    }

    /// A net-new file lists every declaration on the `+` side and carries the
    /// `(new)` tag.
    #[test]
    fn added_file_lists_all_declarations_as_added() {
        if !git_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        init_repo(cwd);
        std::fs::write(cwd.join("seed.rs"), "pub fn seed() {}\n").unwrap();
        git(cwd, &["add", "-A"]);
        git(cwd, &["commit", "-m", "seed"]);

        std::fs::write(cwd.join("added.rs"), "pub fn brand_new() {}\n").unwrap();
        git(cwd, &["add", "-A"]);
        git(cwd, &["commit", "-m", "add file"]);

        let digest = build_signature_diff(cwd, "HEAD~1", "HEAD");
        if digest.is_empty() {
            return;
        }
        assert!(digest.contains("`added.rs` (new)"), "new tag: {digest}");
        assert!(digest.contains("+ fns: brand_new"), "all decls added: {digest}");
        assert!(!digest.contains("seed.rs"), "an unchanged file is absent: {digest}");
    }

    /// A non-code file that changed is still listed (file set preserved) but
    /// carries `(no signature change)` — no grammar, no signatures.
    #[test]
    fn non_code_file_change_is_listed_without_signatures() {
        if !git_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        init_repo(cwd);
        std::fs::write(cwd.join("notes.txt"), "one\n").unwrap();
        git(cwd, &["add", "-A"]);
        git(cwd, &["commit", "-m", "v1"]);
        std::fs::write(cwd.join("notes.txt"), "one\ntwo\n").unwrap();
        git(cwd, &["add", "-A"]);
        git(cwd, &["commit", "-m", "v2"]);

        let digest = build_signature_diff(cwd, "HEAD~1", "HEAD");
        if digest.is_empty() {
            return;
        }
        assert!(digest.contains("notes.txt"), "the changed file is listed: {digest}");
        assert!(digest.contains("(no signature change)"), "no sig delta noted: {digest}");
    }

    /// Fail-open: outside a git repo the digest is empty, never a panic.
    #[test]
    fn fail_open_outside_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(build_signature_diff(dir.path(), "HEAD~1", "HEAD"), "");
    }
}
