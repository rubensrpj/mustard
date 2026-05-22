//! `mustard-rt run docs-stale-check` — narrative-drift linter.
//!
//! Closed architectural specs publish an audit entry in
//! `.claude/.docs-audit.json` listing the strings that became obsolete after
//! they shipped. This subcommand scans the **source-of-truth** markdown
//! surface and emits a JSON report of the hits.
//!
//! Default scope (source-of-truth only):
//!
//! - `{root}/CLAUDE.md` (single file at repo root)
//! - `{root}/apps/*/CLAUDE.md` (each subproject's maintainer-authored root doc)
//! - `{root}/.claude/pipeline-config.md`
//! - `{root}/.claude/refs/**/*.md`
//! - `{root}/.claude/commands/**/*.md`
//!
//! Nested installed-payload copies under `apps/*/.claude/**` are **excluded
//! by design**: they are downstream installs of the Mustard CLI templates and
//! a wave that touches a single source doc must not be forced to update every
//! shipped copy. Pass `--include-nested` (or set
//! `MUSTARD_DOCS_AUDIT_INCLUDE_NESTED=1`) to opt back into a full-monorepo
//! audit.
//!
//! Other always-excluded directories: `target/`, `node_modules/`, `.git/`,
//! `dist/`, `bin/`, `obj/`, plus the rest of [`IGNORE_DIRS`].
//!
//! `/mustard:close` invokes this in its Verification Gate: by default a hit
//! prints a warning; setting `MUSTARD_DOCS_AUDIT_MODE=strict` (or passing
//! `--strict` here) makes the subcommand exit `1` so the gate fails.
//!
//! Fail-open: any IO error on a single target is recorded in `scanned_errors`
//! and the scan continues. The process never panics on bad input — a malformed
//! audit file produces an empty report.
//!
//! Pattern matching: the audit entries hold simple regex-shaped strings
//! (literals + `.*`), but `mustard-rt` carries no `regex` crate. The matcher
//! splits a pattern on `.*` and checks each piece is present, in order, on
//! the same line — covering every shape the seed audits use today.

use crate::run::env::project_dir;
use mustard_core::fs;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Directories never descended into — mirrors `security_scan::IGNORE_DIRS`,
/// plus `.git` so a working copy never explodes the scan.
const IGNORE_DIRS: &[&str] = &[
    "node_modules", ".git", "dist", "bin", "obj", ".next", "vendor",
    "__pycache__", ".nuxt", ".output", "build", "coverage", "target",
    "migrations", ".vs", ".idea",
];

/// Directory recursion depth cap — a working copy this deep is pathological.
const MAX_DEPTH: usize = 12;

/// One audit entry loaded from `.claude/.docs-audit.json`.
struct Audit {
    from_spec: String,
    obsolete_terms: Vec<String>,
    hint: String,
}

/// One detected stale-doc hit.
struct Hit {
    file: String,
    line: usize,
    pattern: String,
    from_spec: String,
    hint: String,
}

/// Load and parse the audit file. Returns an empty list when the file is
/// missing or unreadable — every error is reported in `errors`.
fn load_audits(path: &Path, errors: &mut Vec<String>, only: Option<&str>) -> Vec<Audit> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            errors.push(format!("audit-file: {e}"));
            return Vec::new();
        }
    };
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("audit-parse: {e}"));
            return Vec::new();
        }
    };
    let Some(audits) = parsed.get("audits").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in audits {
        let from_spec = entry
            .get("from_spec")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if from_spec.is_empty() {
            continue;
        }
        if let Some(filter) = only {
            if from_spec != filter {
                continue;
            }
        }
        let obsolete_terms: Vec<String> = entry
            .get("obsolete_terms")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if obsolete_terms.is_empty() {
            continue;
        }
        let hint = entry
            .get("replacement_hint")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        out.push(Audit { from_spec, obsolete_terms, hint });
    }
    out
}

/// Test whether `line` matches `pattern`. The pattern is the audit's
/// regex-shaped string; it is reduced to a sequence of literal pieces split on
/// `.*`, with each piece's `\.` taken as a literal dot. A line matches when
/// every piece is present, in order.
fn line_matches(line: &str, pattern: &str) -> bool {
    let pieces: Vec<String> = pattern
        .split(".*")
        .map(|p| p.replace("\\.", "."))
        .collect();
    let mut cursor = 0usize;
    for piece in &pieces {
        if piece.is_empty() {
            continue;
        }
        match line[cursor..].find(piece.as_str()) {
            Some(rel) => cursor += rel + piece.len(),
            None => return false,
        }
    }
    true
}

/// Scan a single file against every term of every audit, appending hits.
fn scan_file(path: &Path, audits: &[Audit], hits: &mut Vec<Hit>, errors: &mut Vec<String>) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            errors.push(format!("read {}: {e}", path.display()));
            return;
        }
    };
    for (idx, raw) in text.split('\n').enumerate() {
        let line = raw.trim_end_matches('\r');
        for audit in audits {
            for pattern in &audit.obsolete_terms {
                if line_matches(line, pattern) {
                    hits.push(Hit {
                        file: path.to_string_lossy().replace('\\', "/"),
                        line: idx + 1,
                        pattern: pattern.clone(),
                        from_spec: audit.from_spec.clone(),
                        hint: audit.hint.clone(),
                    });
                }
            }
        }
    }
}

/// Whether `path` is a markdown file the linter should inspect.
fn is_target(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name == "CLAUDE.md" || name == "pipeline-config.md" {
        return true;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if ext != "md" {
        return false;
    }
    // Inside `.claude/refs/` or `.claude/commands/`? Walk upwards.
    for ancestor in path.ancestors().skip(1) {
        let ancestor_name = ancestor
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if ancestor_name == "refs" || ancestor_name == "commands" {
            if let Some(parent) = ancestor.parent() {
                if parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n == ".claude")
                    .unwrap_or(false)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether `dir` is a nested installed-payload `.claude/` we should skip
/// by default. Returns `true` when `dir` is named `.claude` AND lives
/// directly under `apps/<name>/` (relative to repo root) — these are
/// downstream copies of the Mustard CLI templates, not source-of-truth docs.
fn is_nested_install_claude(dir: &Path, root: &Path) -> bool {
    let dir_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if dir_name != ".claude" {
        return false;
    }
    let Some(parent) = dir.parent() else {
        return false;
    };
    // parent must be apps/<name>/ → its parent must be `apps`, and `apps`'
    // parent must be `root` (or canonicalize-equivalent).
    let Some(grandparent) = parent.parent() else {
        return false;
    };
    let grand_name = grandparent
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if grand_name != "apps" {
        return false;
    }
    // Treat any `apps/<x>/.claude` under the repo root as nested install.
    // Comparing path equality rather than canonicalising — both come from
    // walking down `root` so they share the same prefix bytes.
    grandparent.parent().map(|p| p == root).unwrap_or(false)
}

/// Recursively walk `dir`, calling `scan_file` on every matching markdown file.
fn walk(
    dir: &Path,
    root: &Path,
    include_nested: bool,
    audits: &[Audit],
    hits: &mut Vec<Hit>,
    errors: &mut Vec<String>,
    scanned: &mut usize,
    depth: usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            errors.push(format!("read_dir {}: {e}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let name = entry.file_name.clone();
        let path = entry.path.clone();
        let is_dir = entry.is_dir;
        if is_dir {
            // Skip ignored dirs, but allow `.claude` (it is a hidden dir
            // that holds the docs we audit).
            if IGNORE_DIRS.contains(&name.as_str()) {
                continue;
            }
            if name.starts_with('.') && name != ".claude" {
                continue;
            }
            // Default: skip nested installed-payload `.claude` copies under
            // `apps/<name>/`. Opt-in via `--include-nested` for a full audit.
            if !include_nested && is_nested_install_claude(&path, root) {
                continue;
            }
            walk(&path, root, include_nested, audits, hits, errors, scanned, depth + 1);
        } else if is_target(&path) {
            *scanned += 1;
            scan_file(&path, audits, hits, errors);
        }
    }
}

/// Build the JSON report.
fn to_json(scanned: usize, errors: &[String], hits: &[Hit]) -> Value {
    json!({
        "scanned": scanned,
        "scanned_errors": errors,
        "hits": hits.iter().map(|h| json!({
            "file": h.file,
            "line": h.line,
            "pattern": h.pattern,
            "from_spec": h.from_spec,
            "hint": h.hint,
        })).collect::<Vec<_>>(),
    })
}

/// Dispatch `mustard-rt run docs-stale-check [--from <spec>] [--strict] [--include-nested]`.
pub fn run(from: Option<&str>, strict: bool, include_nested: bool) {
    let root = PathBuf::from(project_dir());
    let audit_path = root.join(".claude").join(".docs-audit.json");

    // CLI flag wins; env var is the documented equivalent.
    let include_nested = include_nested
        || std::env::var("MUSTARD_DOCS_AUDIT_INCLUDE_NESTED")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);

    let mut errors: Vec<String> = Vec::new();
    let audits = load_audits(&audit_path, &mut errors, from);

    let mut hits: Vec<Hit> = Vec::new();
    let mut scanned: usize = 0;
    if !audits.is_empty() {
        walk(&root, &root, include_nested, &audits, &mut hits, &mut errors, &mut scanned, 0);
    }

    let report = to_json(scanned, &errors, &hits);
    let serialized = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into());
    println!("{serialized}");

    if strict && !hits.is_empty() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn line_matches_literal_substring() {
        assert!(line_matches("see harness-views.js for details", "harness-views\\.js"));
        assert!(!line_matches("see harness-other.js", "harness-views\\.js"));
    }

    #[test]
    fn line_matches_pattern_with_wildcard() {
        assert!(line_matches(
            "the events.jsonl file is the truth source today",
            "events\\.jsonl.*truth source",
        ));
        // Order matters — pieces must appear left-to-right.
        assert!(!line_matches(
            "truth source comes before events.jsonl here",
            "events\\.jsonl.*truth source",
        ));
    }

    #[test]
    fn line_matches_phase_name_pattern() {
        assert!(line_matches(
            "still uses phaseName from the pipeline-state json",
            "phaseName.*pipeline-state",
        ));
        assert!(!line_matches("just phaseName, no follow-up", "phaseName.*pipeline-state"));
    }

    #[test]
    fn is_target_matches_claude_md_and_pipeline_config() {
        assert!(is_target(Path::new("/repo/CLAUDE.md")));
        assert!(is_target(Path::new("/repo/.claude/pipeline-config.md")));
    }

    #[test]
    fn is_target_matches_refs_and_commands_tree() {
        assert!(is_target(Path::new("/repo/.claude/refs/feature/x.md")));
        assert!(is_target(Path::new(
            "/repo/.claude/commands/mustard/close/SKILL.md"
        )));
        // A random .md outside the audited tree is not a target.
        assert!(!is_target(Path::new("/repo/docs/random.md")));
    }

    #[test]
    fn end_to_end_picks_up_a_hit_and_reports_zero_when_clean() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        // Audit registry: one term.
        let audit = r#"{
            "version": 1,
            "audits": [
                {
                    "from_spec": "demo-spec",
                    "closed_at": "2026-05-19",
                    "obsolete_terms": ["harness-views\\.js"],
                    "replacement_hint": "use the new face"
                }
            ]
        }"#;
        std::fs::write(root.join(".claude").join(".docs-audit.json"), audit).unwrap();

        // Dirty doc that should match.
        std::fs::write(
            root.join("CLAUDE.md"),
            "Top line\nsee harness-views.js for details\n",
        )
        .unwrap();

        let mut errors: Vec<String> = Vec::new();
        let audits = load_audits(
            &root.join(".claude").join(".docs-audit.json"),
            &mut errors,
            None,
        );
        assert_eq!(audits.len(), 1);
        let mut hits = Vec::new();
        let mut scanned = 0;
        walk(root, root, false, &audits, &mut hits, &mut errors, &mut scanned, 0);
        assert_eq!(hits.len(), 1, "expected one hit, errors={errors:?}");
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].pattern, "harness-views\\.js");
        assert_eq!(hits[0].from_spec, "demo-spec");

        // Clean doc, no hit.
        std::fs::write(root.join("CLAUDE.md"), "Nothing obsolete here\n").unwrap();
        let mut hits2 = Vec::new();
        let mut scanned2 = 0;
        walk(root, root, false, &audits, &mut hits2, &mut errors, &mut scanned2, 0);
        assert!(hits2.is_empty());
    }

    #[test]
    fn default_scope_excludes_nested_apps_claude_copies() {
        // Tree:
        //   root/CLAUDE.md                              (source-of-truth)
        //   root/.claude/pipeline-config.md             (source-of-truth, has hit)
        //   root/apps/dashboard/.claude/commands/foo.md (nested install, has hit)
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::create_dir_all(root.join("apps/dashboard/.claude/commands")).unwrap();

        let audit = r#"{
            "version": 1,
            "audits": [
                {
                    "from_spec": "demo-spec",
                    "closed_at": "2026-05-19",
                    "obsolete_terms": ["memory-persist\\.js"],
                    "replacement_hint": ""
                }
            ]
        }"#;
        std::fs::write(root.join(".claude").join(".docs-audit.json"), audit).unwrap();
        std::fs::write(root.join("CLAUDE.md"), "top\n").unwrap();
        std::fs::write(
            root.join(".claude/pipeline-config.md"),
            "this references memory-persist.js still\n",
        )
        .unwrap();
        std::fs::write(
            root.join("apps/dashboard/.claude/commands/foo.md"),
            "also references memory-persist.js\n",
        )
        .unwrap();

        let mut errors: Vec<String> = Vec::new();
        let audits = load_audits(
            &root.join(".claude").join(".docs-audit.json"),
            &mut errors,
            None,
        );
        assert_eq!(audits.len(), 1);

        // Default: nested install copies excluded → only the source-of-truth hit.
        let mut hits = Vec::new();
        let mut scanned = 0;
        walk(root, root, false, &audits, &mut hits, &mut errors, &mut scanned, 0);
        let files: Vec<&str> = hits.iter().map(|h| h.file.as_str()).collect();
        assert_eq!(
            hits.len(),
            1,
            "default scope must skip nested apps/*/.claude, got files={files:?}",
        );
        assert!(
            hits[0].file.ends_with(".claude/pipeline-config.md"),
            "expected the source-of-truth file, got {}",
            hits[0].file,
        );

        // Opt-in: include nested install copies → both hits surface.
        let mut hits_all = Vec::new();
        let mut scanned_all = 0;
        walk(root, root, true, &audits, &mut hits_all, &mut errors, &mut scanned_all, 0);
        let files_all: Vec<&str> = hits_all.iter().map(|h| h.file.as_str()).collect();
        assert_eq!(
            hits_all.len(),
            2,
            "include_nested must surface nested install copies, got files={files_all:?}",
        );
    }

    #[test]
    fn is_nested_install_claude_recognises_apps_subtree() {
        let root = Path::new("/repo");
        assert!(is_nested_install_claude(
            Path::new("/repo/apps/dashboard/.claude"),
            root,
        ));
        assert!(is_nested_install_claude(
            Path::new("/repo/apps/cli/.claude"),
            root,
        ));
        // Source-of-truth root `.claude` is NOT nested.
        assert!(!is_nested_install_claude(Path::new("/repo/.claude"), root));
        // Deeper than `apps/<x>/.claude` does not match.
        assert!(!is_nested_install_claude(
            Path::new("/repo/apps/dashboard/sub/.claude"),
            root,
        ));
        // Different repo root → not classified as nested.
        assert!(!is_nested_install_claude(
            Path::new("/other/apps/dashboard/.claude"),
            root,
        ));
    }

    #[test]
    fn from_filter_narrows_to_a_single_audit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("audit.json");
        std::fs::write(
            &path,
            r#"{
              "version": 1,
              "audits": [
                { "from_spec": "a", "obsolete_terms": ["foo"], "replacement_hint": "" },
                { "from_spec": "b", "obsolete_terms": ["bar"], "replacement_hint": "" }
              ]
            }"#,
        )
        .unwrap();
        let mut errors: Vec<String> = Vec::new();
        let only_a = load_audits(&path, &mut errors, Some("a"));
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].from_spec, "a");
    }

    #[test]
    fn missing_audit_file_yields_empty_audits_and_records_error() {
        let dir = tempdir().unwrap();
        let mut errors: Vec<String> = Vec::new();
        let audits = load_audits(&dir.path().join("missing.json"), &mut errors, None);
        assert!(audits.is_empty());
        assert_eq!(errors.len(), 1);
    }
}
