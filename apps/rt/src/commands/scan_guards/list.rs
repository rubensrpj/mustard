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
//!
//! The walk itself is NOT owned by this command: [`collect_pending`] performs
//! it ONCE and returns the raw census ([`PendingScaffolds`]), which two callers
//! project — this command into the enrich worklist, and
//! [`crate::commands::doctor::guards_scaffold_check`] into a doctor advisory. A
//! second traversal with its own ignore list would be the third copy of this
//! walk in the crate and would drift from the first two silently.

use std::path::Path;

use mustard_core::io::fs;
use serde_json::{json, Value};

use crate::commands::scan_claude::GUARDS_PENDING_OPEN;
use crate::commands::scan_patterns::list::TEST_SEGMENTS;

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
pub(crate) struct Pending {
    /// Path to the `CLAUDE.md`, as a string (lossy on non-UTF-8).
    pub(crate) path: String,
    /// Subproject directory relative to `root` (forward-slashed). Empty for the
    /// root unit — but the root is excluded, so this is always non-empty here.
    pub(crate) subproject: String,
    /// Project kind mined by Wave 1 (e.g. `rust`).
    pub(crate) kind: String,
    /// Frameworks mined by Wave 1, in caller order. Empty when none.
    pub(crate) frameworks: Vec<String>,
    /// Stack detections mined by Wave 1, as the raw `name(confidence)` tokens
    /// of the facts line (e.g. `laravel(0.95)`). Kept verbatim — no float
    /// re-parse/re-serialize churn — so the worklist round-trips the generator
    /// byte-for-byte. Empty when the line predates the segment / none inferred.
    pub(crate) stacks: Vec<String>,
}

/// The result of ONE walk of the working copy: every pending subproject
/// `CLAUDE.md`, plus the fail-open evidence of what the walk could not read.
///
/// Both vectors are already sorted by [`collect_pending`], so no projection
/// re-sorts (and none can drift from another's order).
pub(crate) struct PendingScaffolds {
    /// Pending entries, sorted by `path`.
    pub(crate) entries: Vec<Pending>,
    /// Repo-relative descriptions of what was skipped (unreadable directory /
    /// `CLAUDE.md`), sorted. Never absolute — a consumer may serialize these.
    pub(crate) errors: Vec<String>,
}

/// Walk `root` ONCE and collect every subproject `CLAUDE.md` still carrying the
/// pending sentinel. The shared core of `scan-guards-list` and the doctor
/// `guards-scaffold` advisory — see the module docs on why there is exactly one
/// walk. Fail-open: nothing here propagates an error or panics.
pub(crate) fn collect_pending(root: &Path) -> PendingScaffolds {
    let mut out = PendingScaffolds { entries: Vec::new(), errors: Vec::new() };
    // Resolved ONCE for the whole walk: the census must recognise exactly the
    // file `scan --full` produced, which under a private install is
    // `CLAUDE.local.md`. Not the tolerant reader-side `guards_file` — an entry
    // here is handed to `scan-guards-apply`, which SPLICES it, and a private
    // install must never splice the file the host repository versions.
    let owned = crate::shared::context::guards_file_name(root);
    walk(root, root, owned, &mut out, 0);
    // Stable order so every projection is deterministic across runs.
    out.entries.sort_by(|a, b| a.path.cmp(&b.path));
    out.errors.sort();
    out
}

/// Run `scan-guards-list`. Prints a JSON array to stdout; exit 0 always.
pub fn run(root: &Path) {
    let census = collect_pending(root);
    let arr: Vec<Value> = census
        .entries
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

/// Recursively walk `dir`, collecting pending subproject instruction files named
/// `owned` (see [`collect_pending`]).
/// Fail-open: an unreadable directory is skipped and recorded, never propagated.
fn walk(dir: &Path, root: &Path, owned: &str, out: &mut PendingScaffolds, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        out.errors.push(format!("{}: unreadable directory (skipped)", rel_of(dir, root)));
        return;
    };
    for entry in entries {
        if entry.is_dir {
            let name = entry.file_name.as_str();
            if IGNORE_DIRS.contains(&name) {
                continue;
            }
            // Test terrain is not a subproject to enrich. A fixture tree exists
            // so the miner has something to READ in the crate's own tests, and
            // the instruction file inside it is a prop — guards authored for it
            // would teach nothing and are counted as debt on every session
            // start. The sibling walk (the mold worklist) already discards it;
            // this one used to descend, and 6 of the 14 pendings it reported
            // here were props.
            if is_test_segment(name) {
                continue;
            }
            // Hidden dirs are skipped (build caches, VCS); `.claude` holds no
            // CLAUDE.md of its own, so excluding all dot-dirs is safe here.
            if name.starts_with('.') {
                continue;
            }
            walk(&entry.path, root, owned, out, depth + 1);
        } else if entry.file_name == owned {
            classify(&entry.path, root, out);
        }
    }
}

/// Whether a directory NAME is one of the conventional test/fixture segments.
///
/// The list is [`TEST_SEGMENTS`], owned by the mold worklist — the other half of
/// the same enrich — so the two halves can never disagree about what test
/// terrain is. Case-insensitive because a `Tests/` directory is the same
/// terrain; every segment in the list is ASCII, so the ASCII comparison is the
/// whole answer and costs no allocation inside the walk.
fn is_test_segment(name: &str) -> bool {
    TEST_SEGMENTS.iter().any(|seg| name.eq_ignore_ascii_case(seg))
}

/// Classify a `CLAUDE.md`: pushes a [`Pending`] entry iff the file carries the
/// pending marker AND is NOT the workspace-root unit. A file that cannot be read
/// is recorded in `errors` — its guards state is unknown, and dropping it
/// silently would let a scaffold hide behind an IO failure.
fn classify(path: &Path, root: &Path, out: &mut PendingScaffolds) {
    // The root `CLAUDE.md` (directly under `root`) is excluded from enrich.
    let subproject = subproject_of(path, root);
    if subproject.is_empty() {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        // The real file name, not the shared one — under a private install this
        // line would otherwise name a file the operator does not have.
        let name = path.file_name().map_or_else(
            || crate::shared::context::CLAUDE_MD.to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        out.errors
            .push(format!("{subproject}/{name}: unreadable (guards state unknown)"));
        return;
    };
    if !text.contains(GUARDS_PENDING_OPEN) {
        return;
    }
    let (kind, frameworks, stacks) = parse_facts(&text);
    out.entries.push(Pending {
        path: path.to_string_lossy().into_owned(),
        subproject,
        kind,
        frameworks,
        stacks,
    });
}

/// `dir` relative to `root`, forward-slashed; `.` for `root` itself. Used for
/// the fail-open error lines so a consumer that serializes them never carries an
/// absolute (machine-specific, byte-unstable) path.
fn rel_of(dir: &Path, root: &Path) -> String {
    let rel = match dir.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        // Outside `root` (should not happen for a tree walked from `root`).
        Err(_) => dir.to_string_lossy().replace('\\', "/"),
    };
    if rel.is_empty() {
        ".".to_string()
    } else {
        rel
    }
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

        let found = collect_pending(root).entries;
        assert_eq!(found.len(), 1, "exactly the pending subproject: {:?}", found.iter().map(|p| &p.path).collect::<Vec<_>>());
        let p = &found[0];
        assert_eq!(p.subproject, "apps/rt");
        assert_eq!(p.kind, "rust");
        assert_eq!(p.frameworks, vec!["serde".to_string(), "clap".to_string()]);
    }

    /// AC-1 — a fixture tree is not a subproject. Every conventional test
    /// segment stops the walk, so the props under it never reach the worklist
    /// and never inflate the pending-guards count the session-start notice
    /// prints. Measured in this repository before the fix: 6 of 14 pendings
    /// were fixtures under `apps/scan/tests/fixtures/`.
    #[test]
    fn guards_walk_skips_test_terrain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // One real subproject — the control inside the test.
        let real = root.join("apps").join("rt");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("CLAUDE.md"), pending_block("rust", "serde")).unwrap();

        // A prop under EVERY segment the mold worklist already discards, each
        // nested the way a real fixture is (`<crate>/tests/fixtures/<proj>`).
        for segment in TEST_SEGMENTS {
            let prop = root.join("apps").join("scan").join(segment).join("php_laravel");
            std::fs::create_dir_all(&prop).unwrap();
            std::fs::write(prop.join("CLAUDE.md"), pending_block("php", "laravel/framework"))
                .unwrap();
        }
        // …and one spelled with a capital, which is the same terrain.
        let capitalised = root.join("apps").join("scan").join("Tests").join("graph_go");
        std::fs::create_dir_all(&capitalised).unwrap();
        std::fs::write(capitalised.join("CLAUDE.md"), pending_block("go", "(none)")).unwrap();

        let census = collect_pending(root);
        assert_eq!(
            census.entries.iter().map(|p| p.subproject.as_str()).collect::<Vec<_>>(),
            vec!["apps/rt"],
            "only the production subproject is pending work",
        );
        assert!(
            census.errors.is_empty(),
            "a directory that is never descended is not an error either: {:?}",
            census.errors,
        );
    }

    #[test]
    fn scan_guards_list_parses_none_frameworks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sub = root.join("packages").join("lib");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("CLAUDE.md"), pending_block("rust", "(none)")).unwrap();

        let found = collect_pending(root).entries;
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
        assert!(collect_pending(root).entries.is_empty());
    }

    /// A `CLAUDE.md` that cannot be read is skipped AND recorded (repo-relative)
    /// — the walk continues and the healthy sibling still lands on the worklist.
    /// Dropping it silently would let a scaffold hide behind an IO failure.
    #[test]
    fn unreadable_claude_md_is_skipped_and_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Invalid UTF-8 — `read_to_string` fails on it on every platform (a
        // permission bit would not be portable).
        let broken = root.join("apps").join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("CLAUDE.md"), [0xF0, 0x9F, 0x92]).unwrap();
        let ok = root.join("apps").join("fine");
        std::fs::create_dir_all(&ok).unwrap();
        std::fs::write(ok.join("CLAUDE.md"), pending_block("rust", "serde")).unwrap();

        let census = collect_pending(root);
        assert_eq!(census.entries.len(), 1, "the healthy sibling must still be collected");
        assert_eq!(census.entries[0].subproject, "apps/fine");
        assert_eq!(census.errors.len(), 1, "the unreadable file must be recorded: {:?}", census.errors);
        assert!(census.errors[0].starts_with("apps/broken/CLAUDE.md"), "{:?}", census.errors);
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
            &[],
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
        let legacy = crate::commands::scan_claude::build_guards_block("php", &[], &[], &[]);
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

        let found = collect_pending(root).entries;
        assert_eq!(found.len(), 2);
        // Sorted by path: apps/old before apps/web.
        assert!(found[0].stacks.is_empty(), "legacy entry must keep an empty stacks list");
        assert_eq!(found[1].stacks, vec!["laravel(0.95)".to_string()]);
        assert_eq!(found[1].frameworks, vec!["laravel/framework".to_string()]);
    }
}
