//! `scan-guards-apply` — splice the enrich agent's authored guards into a
//! subproject `CLAUDE.md`'s pending `## Guards` block.
//!
//! The block was seeded by Wave 1's `scan_claude` renderer:
//!
//! ```text
//! ## Guards
//!
//! <!-- mustard:guards pending -->
//! <!-- facts: kind=rust; frameworks=serde, clap -->
//! <!-- /mustard:guards -->
//! ```
//!
//! `apply` locates the span between [`scan_claude::GUARDS_PENDING_OPEN`] and
//! [`scan_claude::GUARDS_CLOSE`], replaces ONLY the body between the markers
//! with the agent's guards (preserving every other byte of the file), enforces
//! a line cap, and flips the opening marker to [`scan_claude::GUARDS_DONE_OPEN`]
//! so a re-run of `scan-guards-list` no longer picks the file up (idempotence).
//!
//! Refuses the workspace-root `CLAUDE.md` (never seeded with a pending block —
//! the root is excluded from enrich). Writes atomically via the same primitive
//! `scan_claude` uses (`mustard_core::io::fs::write_atomic`).
//!
//! Fail-open per the `mustard-rt run` contract: every recoverable error prints
//! a clear stderr line and exits 0; the only non-zero exit is a flat refusal of
//! the root (a caller bug worth surfacing).

use std::io::Read as _;
use std::path::Path;

use mustard_core::io::fs as mfs;

use crate::commands::scan_claude::{GUARDS_CLOSE, GUARDS_DONE_OPEN, GUARDS_PENDING_OPEN};
use crate::commands::scan_guards::list::subproject_of;

/// Hard ceiling on the authored guard body. The contract is "3-6 lines of
/// do/don't"; anything beyond this is prose run amok, so the body is truncated
/// (with a stderr warning) rather than shipped whole. The `facts` comment line
/// Wave 1 left inside the block does NOT count against this — it is preserved
/// separately as grounding context.
const MAX_GUARD_LINES: usize = 6;

/// Run `scan-guards-apply`.
///
/// - `path`: the subproject `CLAUDE.md` to enrich.
/// - `root`: the workspace root the scan ran from, used to classify `path`.
/// - `guards`: the agent's authored guard text, or `-` to read it from stdin.
///
/// On success prints a one-line confirmation to stdout and exits 0. A refusal
/// of the workspace root exits 1; every other recoverable error is fail-open
/// (stderr warning, exit 0).
pub fn run(path: &Path, root: &Path, guards: &str) {
    // Refuse the workspace-root CLAUDE.md: the root is excluded from enrich, so
    // it never carries a pending block. This is a caller bug, so it is the one
    // non-zero exit — surfaced clearly rather than silently degraded.
    if is_root_claude_md(path, root) {
        eprintln!(
            "scan-guards-apply: refusing the workspace-root CLAUDE.md ({}) — the root is excluded from enrich",
            path.display()
        );
        std::process::exit(1);
    }

    let Some(body) = resolve_guards(guards) else {
        eprintln!("scan-guards-apply: empty guards body — nothing to apply");
        return;
    };

    let Ok(content) = mfs::read_to_string(path) else {
        eprintln!("scan-guards-apply: cannot read {} — skipping", path.display());
        return;
    };

    match splice(&content, &body) {
        Some(updated) => {
            if let Err(e) = mfs::write_atomic(path, updated.as_bytes()) {
                eprintln!("scan-guards-apply: cannot write {}: {e}", path.display());
                return;
            }
            println!("scan-guards-apply: enriched {}", path.display());
        }
        None => {
            // No pending block — either already enriched (idempotent no-op) or a
            // file that was never seeded. Either way, leave it untouched.
            eprintln!(
                "scan-guards-apply: no pending guards block in {} — left unchanged",
                path.display()
            );
        }
    }
}

/// Resolve the guards body: `-` reads from stdin, anything else is the literal
/// text. Returns `None` when the resolved body is blank (nothing to apply).
fn resolve_guards(guards: &str) -> Option<String> {
    let raw = if guards == "-" {
        let mut buf = String::new();
        if std::io::stdin().read_to_string(&mut buf).is_err() {
            return None;
        }
        buf
    } else {
        guards.to_string()
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Whether `path` is the workspace-root `CLAUDE.md` — i.e. its directory IS the
/// scan `root`. Single-sourced with `scan-guards-list` via [`subproject_of`]: a
/// file is the root unit iff its subproject path (relative to `root`) is empty.
///
/// This deliberately does NOT key off a sibling `mustard.json`: a real nested
/// subproject (e.g. `apps/dashboard`) may ship its own per-package `mustard.json`
/// yet still be a legitimate enrich target. Only the directory == `root`
/// distinguishes the root unit.
fn is_root_claude_md(path: &Path, root: &Path) -> bool {
    path.file_name().is_some_and(|n| n == "CLAUDE.md") && subproject_of(path, root).is_empty()
}

/// Splice the authored `body` into the pending guards block of `content`.
///
/// Locates the span between [`GUARDS_PENDING_OPEN`] and [`GUARDS_CLOSE`] and
/// rewrites it to:
///
/// ```text
/// <!-- mustard:guards -->
/// <!-- facts: ... -->        (preserved verbatim if present)
/// <capped body>
/// <!-- /mustard:guards -->
/// ```
///
/// Every byte before the open marker and after the close marker is preserved.
/// Returns `None` when the pending block is absent (already enriched, or never
/// seeded) so the caller leaves the file untouched.
fn splice(content: &str, body: &str) -> Option<String> {
    let open_at = content.find(GUARDS_PENDING_OPEN)?;
    // The close marker must come after the open marker.
    let close_rel = content[open_at..].find(GUARDS_CLOSE)?;
    let close_at = open_at + close_rel;
    let close_end = close_at + GUARDS_CLOSE.len();

    // The block's interior (between the open-marker line end and the close
    // marker). Preserve any `<!-- facts: ... -->` comment Wave 1 left in here as
    // grounding context for future re-enrich passes.
    let interior = &content[open_at + GUARDS_PENDING_OPEN.len()..close_at];
    let facts_line = interior
        .lines()
        .find(|l| l.trim_start().starts_with("<!-- facts:"))
        .map(str::to_string);

    let capped = cap_lines(body);

    let mut block = String::new();
    block.push_str(GUARDS_DONE_OPEN);
    block.push('\n');
    if let Some(facts) = facts_line {
        block.push_str(facts.trim());
        block.push('\n');
    }
    block.push_str(&capped);
    block.push('\n');
    block.push_str(GUARDS_CLOSE);

    let mut out = String::with_capacity(content.len() + block.len());
    out.push_str(&content[..open_at]);
    out.push_str(&block);
    out.push_str(&content[close_end..]);
    Some(out)
}

/// Enforce the guard-body line cap: keep at most [`MAX_GUARD_LINES`] non-blank
/// lines, warning on stderr when the body is truncated. Blank lines inside the
/// kept range are preserved; trailing blank lines are trimmed.
fn cap_lines(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let non_blank = lines.iter().filter(|l| !l.trim().is_empty()).count();
    if non_blank <= MAX_GUARD_LINES {
        return body.trim_end().to_string();
    }
    eprintln!(
        "scan-guards-apply: guard body has {non_blank} non-blank lines — truncating to {MAX_GUARD_LINES}"
    );
    let mut kept: Vec<&str> = Vec::new();
    let mut seen = 0usize;
    for line in lines {
        if !line.trim().is_empty() {
            if seen >= MAX_GUARD_LINES {
                break;
            }
            seen += 1;
        }
        kept.push(line);
    }
    kept.join("\n").trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A freshly-seeded pending block, as Wave 1 emits it.
    fn pending_doc() -> String {
        format!(
            "# Rt\n\n> Parent: [../CLAUDE.md](../CLAUDE.md)\n\n\
             <!-- mustard:scan-map -->\nTipo: rust\n<!-- /mustard:scan-map -->\n\n\
             ## Guards\n\n{GUARDS_PENDING_OPEN}\n<!-- facts: kind=rust; frameworks=serde, clap -->\n{GUARDS_CLOSE}\n"
        )
    }

    #[test]
    fn scan_guards_apply_splices_non_destructively() {
        let doc = pending_doc();
        let out = splice(&doc, "- DO validate input\n- DON'T panic on IO").expect("pending block present");
        // The authored guards landed inside the block.
        assert!(out.contains("- DO validate input"), "guard 1 missing: {out}");
        assert!(out.contains("- DON'T panic on IO"), "guard 2 missing: {out}");
        // The marker flipped to the non-pending form.
        assert!(out.contains(GUARDS_DONE_OPEN), "done marker missing: {out}");
        assert!(!out.contains(GUARDS_PENDING_OPEN), "pending marker survived: {out}");
        // The facts comment is preserved as grounding context.
        assert!(out.contains("<!-- facts: kind=rust; frameworks=serde, clap -->"), "facts lost: {out}");
        // Everything outside the block is byte-preserved.
        assert!(out.starts_with("# Rt\n\n> Parent:"), "prefix changed: {out}");
        assert!(out.contains("<!-- mustard:scan-map -->\nTipo: rust\n<!-- /mustard:scan-map -->"), "scan-map clobbered: {out}");
        // Exactly one guards block.
        assert_eq!(out.matches(GUARDS_CLOSE).count(), 1, "duplicate close marker: {out}");
    }

    #[test]
    fn stacks_facts_apply_preserves_segment() {
        // The facts line is preserved VERBATIM through the splice — including
        // the `stacks=` segment Wave 1 now emits. If apply ever rebuilt the
        // line field-by-field instead of copying it, the segment (and the
        // grounding it carries for re-enrich passes) would silently degrade.
        let doc = format!(
            "# Web\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n<!-- facts: kind=php; frameworks=laravel/framework; stacks=laravel(0.95),nextjs(0.65) -->\n{GUARDS_CLOSE}\n"
        );
        let out = splice(&doc, "- DO use Eloquent scopes").expect("pending block present");
        assert!(
            out.contains("<!-- facts: kind=php; frameworks=laravel/framework; stacks=laravel(0.95),nextjs(0.65) -->"),
            "facts line degraded by apply: {out}"
        );
        assert!(out.contains(GUARDS_DONE_OPEN), "done marker missing: {out}");
        assert!(out.contains("- DO use Eloquent scopes"), "guard body missing: {out}");
    }

    #[test]
    fn scan_guards_apply_is_idempotent() {
        // After the first apply the block carries the DONE marker, so a second
        // apply finds no pending block and leaves the file untouched.
        let doc = pending_doc();
        let first = splice(&doc, "- DO X").expect("first apply");
        // A re-run no longer matches the pending marker.
        assert!(splice(&first, "- DO Y").is_none(), "second apply must be a no-op: {first}");
    }

    #[test]
    fn scan_guards_apply_caps_lines() {
        let doc = pending_doc();
        // 8 do/don't lines — must be capped to MAX_GUARD_LINES (6).
        let body = (1..=8).map(|i| format!("- DO thing {i}")).collect::<Vec<_>>().join("\n");
        let out = splice(&doc, &body).expect("pending block present");
        assert!(out.contains("- DO thing 6"), "kept line missing: {out}");
        assert!(!out.contains("- DO thing 7"), "line 7 should be truncated: {out}");
        assert!(!out.contains("- DO thing 8"), "line 8 should be truncated: {out}");
    }

    #[test]
    fn scan_guards_apply_no_pending_block_is_none() {
        // A file with no pending marker (already enriched / never seeded) yields
        // None so the caller leaves it untouched.
        let doc = format!("# Sub\n\n## Guards\n\n{GUARDS_DONE_OPEN}\n- DO keep me\n{GUARDS_CLOSE}\n");
        assert!(splice(&doc, "- DO overwrite").is_none());
    }

    #[test]
    fn cap_lines_keeps_short_body_verbatim() {
        let body = "- DO a\n- DO b\n- DON'T c";
        assert_eq!(cap_lines(body), body);
    }

    #[test]
    fn is_root_claude_md_detects_workspace_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // The scan root ships a mustard.json (workspace config) and CLAUDE.md.
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join("CLAUDE.md"), b"# root").unwrap();
        // Root: its directory IS the scan root → refused.
        assert!(is_root_claude_md(&root.join("CLAUDE.md"), root));

        // A nested subproject that ALSO ships its own per-package mustard.json
        // (e.g. apps/dashboard). The OLD sibling-mustard.json heuristic falsely
        // flagged this as the root; the correct rule does NOT, because its
        // directory differs from the scan root.
        let dashboard = root.join("apps").join("dashboard");
        std::fs::create_dir_all(&dashboard).unwrap();
        std::fs::write(dashboard.join("mustard.json"), b"{}").unwrap();
        std::fs::write(dashboard.join("CLAUDE.md"), b"# dashboard").unwrap();
        assert!(
            !is_root_claude_md(&dashboard.join("CLAUDE.md"), root),
            "a nested subproject with its own mustard.json must NOT be treated as root"
        );

        // A plain subproject (no mustard.json beside it) is also not the root.
        let sub = root.join("apps").join("rt");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("CLAUDE.md"), b"# rt").unwrap();
        assert!(!is_root_claude_md(&sub.join("CLAUDE.md"), root));
    }

    /// End-to-end regression for the review defect: a nested subproject that
    /// ships its OWN `mustard.json` AND a pending Guards `CLAUDE.md` under a
    /// scan root must SUCCEED (splice), while the scan-root `CLAUDE.md` must be
    /// refused. The old sibling-`mustard.json` heuristic broke the former.
    #[test]
    fn apply_splices_nested_subproject_with_own_mustard_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Scan root: its own mustard.json + CLAUDE.md (no pending block needed).
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join("CLAUDE.md"), b"# root").unwrap();

        // Nested subproject WITH its own per-package mustard.json AND a pending
        // Guards block — the exact shape the old heuristic falsely refused.
        let dashboard = root.join("apps").join("dashboard");
        std::fs::create_dir_all(&dashboard).unwrap();
        std::fs::write(dashboard.join("mustard.json"), b"{}").unwrap();
        let claude_md = dashboard.join("CLAUDE.md");
        std::fs::write(&claude_md, pending_doc()).unwrap();

        // The scan-root CLAUDE.md is still classified as root → refused.
        assert!(is_root_claude_md(&root.join("CLAUDE.md"), root));
        // The nested subproject is NOT root → eligible for splice.
        assert!(!is_root_claude_md(&claude_md, root));

        // And the splice actually lands (the value path `run` takes after the
        // refusal gate): pending block flips to done with the authored body.
        let content = std::fs::read_to_string(&claude_md).unwrap();
        let out = splice(&content, "- DO validate input").expect("pending block present");
        assert!(out.contains("- DO validate input"), "guard missing: {out}");
        assert!(out.contains(GUARDS_DONE_OPEN), "done marker missing: {out}");
        assert!(!out.contains(GUARDS_PENDING_OPEN), "pending marker survived: {out}");
    }
}
