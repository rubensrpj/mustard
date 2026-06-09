//! `scan-guards-list` — enumerate every subproject `CLAUDE.md` whose `## Guards`
//! block is still `pending` and emit a JSON worklist for the enrich agent.
//!
//! A file is *pending* iff it contains [`scan_claude::GUARDS_PENDING_OPEN`]. The
//! workspace-root `CLAUDE.md` (the unit whose directory is the repo root) is
//! excluded — Wave 1 never seeds the pending block there. For each pending
//! file the facts line (`<!-- facts: kind=...; frameworks=... -->`) is parsed so
//! the agent has grounding context.
//!
//! Output: a JSON array `[{path, subproject, kind, frameworks}]` to stdout.
//! Fail-open: any IO error degrades to `[]` and exit 0.

use std::path::Path;

use mustard_core::io::fs;
use serde_json::{json, Value};

use crate::commands::scan_claude::GUARDS_PENDING_OPEN;

/// Directories never descended into — mirrors `docs_stale_check::IGNORE_DIRS`
/// so the walk stays cheap and never explodes into build/vendor trees.
const IGNORE_DIRS: &[&str] = &[
    "node_modules", ".git", "dist", "bin", "obj", ".next", "vendor",
    "__pycache__", ".nuxt", ".output", "build", "coverage", "target",
    "migrations", ".vs", ".idea", "worktrees", ".worktrees",
];

/// Directory recursion depth cap — a working copy this deep is pathological.
const MAX_DEPTH: usize = 12;

/// One pending-guards worklist entry.
struct Pending {
    /// Path to the `CLAUDE.md`, as a string (lossy on non-UTF-8).
    path: String,
    /// Subproject directory relative to `root` (forward-slashed). Empty for the
    /// root unit — but the root is excluded, so this is always non-empty here.
    subproject: String,
    /// Project kind mined by Wave 1 (e.g. `rust`).
    kind: String,
    /// Frameworks mined by Wave 1, in caller order. Empty when none.
    frameworks: Vec<String>,
    /// Stack detections mined by Wave 1, as the raw `name(confidence)` tokens
    /// of the facts line (e.g. `laravel(0.95)`). Kept verbatim — no float
    /// re-parse/re-serialize churn — so the worklist round-trips the generator
    /// byte-for-byte. Empty when the line predates the segment / none inferred.
    stacks: Vec<String>,
}

/// Run `scan-guards-list`. Prints a JSON array to stdout; exit 0 always.
pub fn run(root: &Path) {
    let mut out: Vec<Pending> = Vec::new();
    walk(root, root, &mut out, 0);
    // Stable order so the worklist is deterministic across runs.
    out.sort_by(|a, b| a.path.cmp(&b.path));
    let arr: Vec<Value> = out
        .iter()
        .map(|p| {
            json!({
                "path": p.path,
                "subproject": p.subproject,
                "kind": p.kind,
                "frameworks": p.frameworks,
                "stacks": p.stacks,
            })
        })
        .collect();
    // `to_string` cannot fail for this shape; fall back to `[]` defensively.
    println!("{}", serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string()));
}

/// Recursively walk `dir`, collecting pending subproject `CLAUDE.md` files.
/// Fail-open: an unreadable directory is skipped, never propagated.
fn walk(dir: &Path, root: &Path, out: &mut Vec<Pending>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        if entry.is_dir {
            let name = entry.file_name.as_str();
            if IGNORE_DIRS.contains(&name) {
                continue;
            }
            // Hidden dirs are skipped (build caches, VCS); `.claude` holds no
            // CLAUDE.md of its own, so excluding all dot-dirs is safe here.
            if name.starts_with('.') {
                continue;
            }
            walk(&entry.path, root, out, depth + 1);
        } else if entry.file_name == "CLAUDE.md" {
            if let Some(p) = classify(&entry.path, root) {
                out.push(p);
            }
        }
    }
}

/// Classify a `CLAUDE.md`: returns a [`Pending`] entry iff the file carries the
/// pending marker AND is NOT the workspace-root unit. `None` otherwise.
fn classify(path: &Path, root: &Path) -> Option<Pending> {
    // The root `CLAUDE.md` (directly under `root`) is excluded from enrich.
    let subproject = subproject_of(path, root);
    if subproject.is_empty() {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    if !text.contains(GUARDS_PENDING_OPEN) {
        return None;
    }
    let (kind, frameworks, stacks) = parse_facts(&text);
    Some(Pending {
        path: path.to_string_lossy().into_owned(),
        subproject,
        kind,
        frameworks,
        stacks,
    })
}

/// The subproject directory of a `CLAUDE.md`, relative to `root`, forward-
/// slashed. Empty when the file sits directly in `root` (the root unit).
///
/// Single-sourced root rule: a `CLAUDE.md` is the workspace-root unit iff
/// `subproject_of(path, root).is_empty()`. Both `list` (worklist exclusion) and
/// `apply` (root refusal) classify against this same helper so they never drift.
pub(crate) fn subproject_of(claude_md: &Path, root: &Path) -> String {
    let Some(parent) = claude_md.parent() else {
        return String::new();
    };
    match parent.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        // Outside `root` (should not happen for a tree walked from `root`) — treat
        // as a subproject so it is not silently dropped.
        Err(_) => parent.to_string_lossy().replace('\\', "/"),
    }
}

/// Parse the `<!-- facts: kind=...; frameworks=a, b; stacks=x(0.95) -->` line
/// Wave 1 emits. Returns `(kind, frameworks, stacks)`; missing fields degrade
/// to `("", vec![], vec![])`. `frameworks=(none)` (Wave 1's empty sentinel)
/// yields an empty vec; an absent `stacks=` segment (legacy line / nothing
/// inferred) likewise. Stacks come back as the raw `name(confidence)` tokens,
/// verbatim, so generator → parser round-trips byte-for-byte.
fn parse_facts(text: &str) -> (String, Vec<String>, Vec<String>) {
    let Some(line) = text.lines().find(|l| l.trim_start().starts_with("<!-- facts:")) else {
        return (String::new(), Vec::new(), Vec::new());
    };
    // Strip the comment delimiters and the `facts:` prefix.
    let inner = line
        .trim()
        .trim_start_matches("<!--")
        .trim_end_matches("-->")
        .trim()
        .trim_start_matches("facts:")
        .trim();

    let mut kind = String::new();
    let mut frameworks: Vec<String> = Vec::new();
    let mut stacks: Vec<String> = Vec::new();
    for field in inner.split(';') {
        let field = field.trim();
        if let Some(v) = field.strip_prefix("kind=") {
            kind = v.trim().to_string();
        } else if let Some(v) = field.strip_prefix("frameworks=") {
            let v = v.trim();
            if v != "(none)" && !v.is_empty() {
                frameworks = v
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        } else if let Some(v) = field.strip_prefix("stacks=") {
            stacks = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    (kind, frameworks, stacks)
}

/// Collect (without printing) the pending worklist — the testable core of
/// [`run`]. Kept private to the module's tests.
#[cfg(test)]
fn run_collect(root: &Path) -> Vec<Pending> {
    let mut out: Vec<Pending> = Vec::new();
    walk(root, root, &mut out, 0);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::scan_claude::GUARDS_CLOSE;

    fn pending_block(kind: &str, fw: &str) -> String {
        format!(
            "# Sub\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n<!-- facts: kind={kind}; frameworks={fw} -->\n{GUARDS_CLOSE}\n"
        )
    }

    #[test]
    fn scan_guards_list_finds_pending_and_excludes_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Root CLAUDE.md carries the pending marker too — but must be EXCLUDED.
        std::fs::write(root.join("CLAUDE.md"), pending_block("rust", "serde")).unwrap();

        // A pending subproject.
        let sub = root.join("apps").join("rt");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("CLAUDE.md"), pending_block("rust", "serde, clap")).unwrap();

        // An already-enriched subproject (no pending marker) — skipped.
        let done = root.join("apps").join("done");
        std::fs::create_dir_all(&done).unwrap();
        std::fs::write(
            done.join("CLAUDE.md"),
            format!("# Done\n\n## Guards\n\n{}\n<!-- facts: kind=rust; frameworks=(none) -->\n{GUARDS_CLOSE}\n",
                crate::commands::scan_claude::GUARDS_DONE_OPEN),
        )
        .unwrap();

        // A build dir that must never be descended.
        let ignored = root.join("target").join("debug");
        std::fs::create_dir_all(&ignored).unwrap();
        std::fs::write(ignored.join("CLAUDE.md"), pending_block("rust", "x")).unwrap();

        let found = run_collect(root);
        assert_eq!(found.len(), 1, "exactly the pending subproject: {:?}", found.iter().map(|p| &p.path).collect::<Vec<_>>());
        let p = &found[0];
        assert_eq!(p.subproject, "apps/rt");
        assert_eq!(p.kind, "rust");
        assert_eq!(p.frameworks, vec!["serde".to_string(), "clap".to_string()]);
    }

    #[test]
    fn scan_guards_list_parses_none_frameworks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sub = root.join("packages").join("lib");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("CLAUDE.md"), pending_block("rust", "(none)")).unwrap();

        let found = run_collect(root);
        assert_eq!(found.len(), 1);
        assert!(found[0].frameworks.is_empty(), "(none) sentinel must yield an empty vec");
        assert_eq!(found[0].kind, "rust");
    }

    #[test]
    fn scan_guards_list_empty_when_no_pending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Only the root carries a pending marker → excluded → empty worklist.
        std::fs::write(root.join("CLAUDE.md"), pending_block("rust", "serde")).unwrap();
        assert!(run_collect(root).is_empty());
    }

    #[test]
    fn parse_facts_handles_missing_line() {
        let (kind, fw, stacks) = parse_facts("# No facts here\n## Guards\n");
        assert!(kind.is_empty());
        assert!(fw.is_empty());
        assert!(stacks.is_empty());
    }

    #[test]
    fn stacks_facts_parse_round_trip() {
        use mustard_core::domain::vocabulary::stacks::StackDetection;

        // Generator → parser round-trip on the REAL Wave-1 output (not a
        // hand-written line), so the two sides can never drift silently.
        let detections = vec![
            StackDetection {
                name: "laravel".into(),
                confidence: 0.95,
                signals: vec!["dep:laravel/framework".into()],
            },
            StackDetection { name: "nextjs".into(), confidence: 0.65, signals: Vec::new() },
        ];
        let block = crate::commands::scan_claude::build_guards_block(
            "php",
            &["laravel/framework".to_string()],
            &detections,
        );
        let (kind, fw, stacks) = parse_facts(&block);
        assert_eq!(kind, "php");
        assert_eq!(fw, vec!["laravel/framework".to_string()]);
        assert_eq!(
            stacks,
            vec!["laravel(0.95)".to_string(), "nextjs(0.65)".to_string()],
            "stacks tokens must round-trip verbatim"
        );

        // A legacy line without the segment degrades to an empty vec — and the
        // generator with no detections produces exactly that legacy line.
        let legacy = crate::commands::scan_claude::build_guards_block("php", &[], &[]);
        let (_, _, none) = parse_facts(&legacy);
        assert!(none.is_empty(), "absent stacks segment must yield an empty vec");
    }

    #[test]
    fn stacks_facts_worklist_carries_stacks() {
        // End-to-end: a pending CLAUDE.md whose facts line carries `stacks=`
        // surfaces the tokens on the worklist entry.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sub = root.join("apps").join("web");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("CLAUDE.md"),
            format!(
                "# Web\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n<!-- facts: kind=php; frameworks=laravel/framework; stacks=laravel(0.95) -->\n{GUARDS_CLOSE}\n"
            ),
        )
        .unwrap();

        // A sibling with a legacy facts line (no `stacks=` segment).
        let old = root.join("apps").join("old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("CLAUDE.md"), pending_block("rust", "serde")).unwrap();

        let found = run_collect(root);
        assert_eq!(found.len(), 2);
        // Sorted by path: apps/old before apps/web.
        assert!(found[0].stacks.is_empty(), "legacy entry must keep an empty stacks list");
        assert_eq!(found[1].stacks, vec!["laravel(0.95)".to_string()]);
        assert_eq!(found[1].frameworks, vec!["laravel/framework".to_string()]);
    }
}
