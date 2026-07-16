//! `grill-capture` — write ONE confirmed glossary term into a `CONTEXT.md`.
//!
//! The other half of the `/feature` ANALYZE glossary loop. `glossary-coverage`
//! reports the weak/missing domain terms (`uncovered`) + the destination
//! (`contextFile`); the orchestrator runs the lightweight inline grill (it asks
//! the user, in chat, for a one-line definition of each uncovered term) and
//! records every CONFIRMED definition here, one call per term. No interrogation
//! lives in this command — it only persists what the grill already settled.
//!
//! Contract:
//! - **Glossary write.** It writes a single term block (`## {Term}\n{definition}`)
//!   into a `CONTEXT.md`; the glossary write itself never touches ADRs or specs.
//! - **Clarify finalize (F6).** With `--finalize`, the command mints
//!   `<spec>/.clarified` for the spec — the marker `approve-spec` requires before
//!   a Full plan may be approved. This is the SINGLE, explicit "clarification
//!   complete" action; a term capture NEVER mints it, and it needs no term, so a
//!   complete-glossary Full spec can still be finalized (killing the old
//!   deadlock). Best-effort and fail-open. The gate itself lives in
//!   `approve-spec`, not here.
//! - **Update, not duplicate.** The target is parsed through the SAME resolver
//!   (`resolve_context_files`, CONTEXT-MAP-aware) and term matcher
//!   (`parse_term_blocks`) the slicer/coverage use, so a term that already has a
//!   block is REPLACED in place (re-grilling a term sharpens its definition); a
//!   new term is appended. The producer and consumer of the glossary cannot
//!   drift.
//! - **Fail-open when absent.** No `--context` resolves to no destination →
//!   `{ok:false, reason:"no-context-target"}`, exit 0. `grill-capture` itself
//!   never blocks; the clarify gate it feeds lives in `scope_guard`.
//!
//! Output (stdout, byte-stable pretty JSON):
//! `{ ok, action, term, contextFile, reason? }` where `action ∈ {appended,
//! updated}`. Always exits 0.

use std::path::{Path, PathBuf};

use mustard_core::io::fs as mfs;
use serde_json::json;

use crate::commands::economy::context_slice::{parse_term_blocks, resolve_context_files};
use crate::shared::context::{
    clarified_marker_path, current_spec, session_id, spec_for_session,
};

/// Resolve the destination `CONTEXT.md` the same way `glossary-coverage` does:
/// the first file that resolved on disk (CONTEXT-MAP expansion included), or the
/// first non-empty requested path when none exists yet (a `missing` glossary
/// still has a concrete destination to create). `None` when no `--context` was
/// given at all.
fn target_file(context: &[String]) -> Option<PathBuf> {
    if let Some(p) = resolve_context_files(context).into_iter().next() {
        return Some(p);
    }
    context
        .iter()
        .find(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// Splice `term`/`definition` into `existing` glossary text. If a block already
/// names `term` (case-insensitive, matched the SAME way the slicer parses
/// blocks), its lines are replaced in place; otherwise the new block is appended
/// after a blank-line separator. Returns the rewritten text + which action ran.
/// Pure — the unit-testable core (no IO).
fn upsert_block(existing: &str, term: &str, definition: &str) -> (String, &'static str) {
    let target = term.trim().to_lowercase();

    // Walk the SAME term-block parse the slicer uses to find an existing block;
    // when found, replace the exact line span its text occupied. Re-parsing
    // rather than a raw string search keeps producer/consumer in lockstep.
    let blocks = parse_term_blocks(existing);
    if let Some(found) = blocks.iter().find(|b| b.term().to_lowercase() == target) {
        // Preserve the EXISTING heading casing (the canonical `## Payable`) —
        // re-grilling sharpens the DEFINITION, it must not silently re-case the
        // term to whatever the caller happened to pass. Replace the exact line
        // span the block occupied, up to (excluding) the next block start.
        let new_block = format!("## {}\n{}", found.term(), definition.trim());
        if let Some(replaced) = replace_span(existing, found.term(), &new_block) {
            return (replaced, "updated");
        }
    }
    let new_block = format!("## {}\n{}", term.trim(), definition.trim());

    // Append. Keep exactly one blank line between the prior content and the new
    // block; a pristine (empty/whitespace) file starts with the block alone.
    let trimmed = existing.trim_end();
    if trimmed.is_empty() {
        (format!("{new_block}\n"), "appended")
    } else {
        (format!("{trimmed}\n\n{new_block}\n"), "appended")
    }
}

/// Replace the line span of the block whose heading term equals `term`
/// (case-insensitive) with `new_block`. The span runs from the matched
/// heading/definition line up to (excluding) the next block-start line — the
/// SAME boundary `parse_term_blocks` splits on. Returns `None` when no heading
/// line matches (the caller then appends).
fn replace_span(existing: &str, term: &str, new_block: &str) -> Option<String> {
    let target = term.to_lowercase();
    let lines: Vec<&str> = existing.lines().collect();
    let is_block_start = |line: &str| block_term(line).is_some();

    let start = lines
        .iter()
        .position(|l| block_term(l).is_some_and(|t| t.to_lowercase() == target))?;
    // The next block start after `start` bounds the span (exclusive).
    let end = lines[start + 1..]
        .iter()
        .position(|l| is_block_start(l))
        .map_or(lines.len(), |off| start + 1 + off);

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    out.extend(lines[..start].iter().map(|s| (*s).to_string()));
    out.push(new_block.to_string());
    out.extend(lines[end..].iter().map(|s| (*s).to_string()));
    let mut joined = out.join("\n");
    if existing.ends_with('\n') {
        joined.push('\n');
    }
    Some(joined)
}

/// The block term a line declares — a `## Heading` / `### Heading` or a
/// `[-*]? **Term** …` definition line. Mirrors the boundary `parse_term_blocks`
/// splits on (a re-implementation kept local because the slicer's `heading_term`
/// / `def_term` are private; both follow the same rule).
fn block_term(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if (2..=3).contains(&hashes)
        && trimmed
            .as_bytes()
            .get(hashes)
            .is_some_and(u8::is_ascii_whitespace)
    {
        let term = trimmed[hashes..].trim();
        if !term.is_empty() {
            return Some(term.to_string());
        }
    }
    // `[-*]? **Term** …` definition line.
    let mut s = line.trim_start();
    if let Some(first) = s.chars().next() {
        if (first == '-' || first == '*') && s[1..].starts_with(char::is_whitespace) {
            s = s[1..].trim_start();
        }
    }
    let after_open = s.strip_prefix("**")?;
    let end = after_open.find("**")?;
    let term = after_open[..end].trim();
    if term.is_empty() {
        None
    } else {
        Some(term.to_string())
    }
}

/// Render the result as byte-stable pretty JSON (deterministic key order).
fn emit(ok: bool, action: &str, term: &str, context_file: &str, reason: Option<&str>) {
    let payload = json!({
        "ok": ok,
        "action": action,
        "term": term,
        "contextFile": context_file,
        "reason": reason,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
    );
}

/// Emit the clarify-finalize result as byte-stable pretty JSON. `spec` is the
/// resolved spec name (deterministic — no volatile path is emitted).
fn emit_finalize(ok: bool, spec: &str, reason: Option<&str>) {
    let payload = json!({
        "ok": ok,
        "action": "clarified",
        "spec": spec,
        "reason": reason,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
    );
}

/// Finalize clarification: mint `<spec>/.clarified` unconditionally — no term
/// required. This is the SINGLE deliberate minter of the marker `approve-spec`
/// requires before a Full plan may be approved (a term capture never mints it),
/// so a complete-glossary Full spec is not deadlocked out of approval.
///
/// The spec is taken from `--spec` when given (the explicit, robust path the
/// Full PLAN flow uses, mirroring `approve-spec`); absent that, it falls back to
/// the session-to-spec binding, then `current_spec`. Fail-open at every step — a
/// resolution or write failure reports `{ok:false, reason}` and still exits 0.
fn run_finalize(spec_arg: &str, root: &Path) {
    let Some(root_str) = root.to_str() else {
        emit_finalize(false, "", Some("bad-root"));
        return;
    };
    let spec = if spec_arg.trim().is_empty() {
        match spec_for_session(root_str, &session_id()).or_else(|| current_spec(root_str)) {
            Some(s) => s,
            None => {
                emit_finalize(false, "", Some("no-active-spec"));
                return;
            }
        }
    } else {
        spec_arg.trim().to_string()
    };
    let Some(marker) = clarified_marker_path(root_str, &spec) else {
        emit_finalize(false, &spec, Some("bad-spec"));
        return;
    };
    if let Some(parent) = marker.parent() {
        let _ = mfs::create_dir_all(parent);
    }
    let body = format!("spec={spec}\nvia=grill-finalize\n");
    if mfs::write_atomic(&marker, body.as_bytes()).is_err() {
        emit_finalize(false, &spec, Some("write-failed"));
        return;
    }
    emit_finalize(true, &spec, None);
}

/// Dispatch `mustard-rt run grill-capture`. Always exits 0 (fail-open).
///
/// Two modes: `--finalize` mints `<spec>/.clarified` (the clarify-finalize — see
/// [`run_finalize`]) and returns; otherwise this is the glossary term capture
/// (`--term` / `--definition` / `--context`). A capture NEVER mints `.clarified`
/// — only the deliberate finalize does.
pub fn run(
    term: &str,
    definition: &str,
    context: &[String],
    spec: &str,
    finalize: bool,
    root: &Path,
) {
    if finalize {
        run_finalize(spec, root);
        return;
    }
    let term_t = term.trim();
    if term_t.is_empty() || definition.trim().is_empty() {
        emit(false, "none", term_t, "", Some("empty-term-or-definition"));
        return;
    }
    let Some(target) = target_file(context) else {
        // No glossary destination → nothing to write. Fail-open: the inline
        // grill simply did not surface anywhere to persist.
        emit(false, "none", term_t, "", Some("no-context-target"));
        return;
    };
    let target_str = target.display().to_string();

    // Read the current glossary (empty when it does not exist yet — a `missing`
    // verdict still gets a fresh file here).
    let existing = std::fs::read_to_string(&target).unwrap_or_default();
    let (updated, action) = upsert_block(&existing, term_t, definition);

    if let Err(e) = mfs::write_atomic(&target, updated.as_bytes()) {
        emit(false, "none", term_t, &target_str, Some(&format!("write-failed: {e}")));
        return;
    }
    emit(true, action, term_t, &target_str, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_a_new_term_to_an_empty_glossary() {
        let (out, action) = upsert_block("", "Payable", "A bill the org owes.");
        assert_eq!(action, "appended");
        assert_eq!(out, "## Payable\nA bill the org owes.\n");
    }

    #[test]
    fn appends_after_existing_content_with_one_blank_line() {
        let existing = "## Tenant\nAn org.\n";
        let (out, action) = upsert_block(existing, "Payable", "A bill owed.");
        assert_eq!(action, "appended");
        assert_eq!(out, "## Tenant\nAn org.\n\n## Payable\nA bill owed.\n");
    }

    #[test]
    fn updates_in_place_when_the_term_already_has_a_block() {
        // Case-insensitive match — `payable` updates the `## Payable` block, and
        // the surrounding blocks survive untouched.
        let existing =
            "## Tenant\nAn org.\n## Payable\nold definition.\n## Ledger\nA log.\n";
        let (out, action) = upsert_block(existing, "payable", "a sharper definition.");
        assert_eq!(action, "updated");
        assert_eq!(
            out,
            "## Tenant\nAn org.\n## Payable\na sharper definition.\n## Ledger\nA log.\n"
        );
    }

    #[test]
    fn update_replaces_a_multi_line_block_body() {
        let existing = "## Payable\nline one.\nline two.\n## Tenant\nAn org.\n";
        let (out, action) = upsert_block(existing, "Payable", "single replacement line.");
        assert_eq!(action, "updated");
        assert_eq!(
            out,
            "## Payable\nsingle replacement line.\n## Tenant\nAn org.\n"
        );
    }

    #[test]
    fn block_term_recognises_headings_and_def_lines() {
        assert_eq!(block_term("## Payable").as_deref(), Some("Payable"));
        assert_eq!(block_term("### Tenant").as_deref(), Some("Tenant"));
        assert_eq!(block_term("- **Ledger** a log").as_deref(), Some("Ledger"));
        assert_eq!(block_term("plain prose"), None);
        assert_eq!(block_term("#NoSpace"), None);
    }

    // ── The clarify-marker mint (integration over a tempdir) ─────────────────

    /// Seed `.claude/spec/<spec>/meta.json` (scope/stage) + a pipeline-state
    /// file so `current_spec` resolves `<spec>` via its FS fallback — the same
    /// no-env-mutation pattern `scope_guard`'s tests use (mutating process env
    /// is `unsafe` under edition 2024).
    fn seed_spec(root: &std::path::Path, spec: &str, scope: &str) {
        let spec_dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            format!(r#"{{"scope":"{scope}","stage":"Plan","outcome":"Active"}}"#),
        )
        .unwrap();
        let states = root.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join(format!("{spec}.json")), "{}").unwrap();
    }

    #[test]
    fn capturing_a_term_does_not_mint_clarified() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full (wave plan)");

        let ctx_file = root.join("CONTEXT.md");
        let context = vec![ctx_file.to_string_lossy().into_owned()];
        run("Payable", "A bill the org owes.", &context, "", false, root);

        // The confirmed term lands in the glossary...
        assert!(
            std::fs::read_to_string(&ctx_file).unwrap().contains("## Payable"),
            "the confirmed term must be persisted to the glossary"
        );
        // ...but a term capture NEVER mints .clarified — only the deliberate
        // finalize does (F6: clarification is explicit, not a capture side effect).
        let marker = clarified_marker_path(root.to_str().unwrap(), "epic").unwrap();
        assert!(!marker.exists(), "a capture must NOT mint .clarified");
    }

    #[test]
    fn finalize_mints_clarified_with_zero_terms() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full (wave plan)");

        // The clarify-finalize takes the spec explicitly and needs NO term — a
        // complete-glossary Full spec can still be marked clarified. This kills
        // the old deadlock where `.clarified` could only be minted by a term
        // capture, so a spec with nothing to grill could never be approved.
        run("", "", &[], "epic", true, root);

        let marker = clarified_marker_path(root.to_str().unwrap(), "epic").unwrap();
        assert!(marker.exists(), "finalize must mint .clarified with zero terms");
    }
}
