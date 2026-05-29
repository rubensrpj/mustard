//! Generic AI-enrichment block for scan-generated `.md` artifacts.
//!
//! Every generated `.md` may carry ONE enrich block — a region whose
//! *boundaries* the deterministic generator owns but whose *inner* prose is
//! written by the optional AI enrich step and PRESERVED across re-scans:
//!
//! ```text
//! <!-- mustard:enrich hash=ab12cd34 -->
//! ## Purpose
//!
//! …AI-written prose…
//! <!-- /mustard:enrich -->
//! ```
//!
//! The `hash` fingerprints the deterministic skeleton the prose was written for.
//! On regeneration the generator emits a fresh skeleton containing a *pending*
//! block stamped with the CURRENT hash; [`merge`] then restores the previous
//! prose IFF the previous block's hash matches — otherwise the prose was written
//! for a now-stale skeleton and is left pending so the next enrich pass rewrites
//! it. This keeps the artifact idempotent: same skeleton + same preserved prose
//! ⇒ byte-identical output, and a changed skeleton transparently invalidates
//! stale prose instead of silently keeping it.

use mustard_core::io::fs as mfs;
use sha2::{Digest, Sha256};
use std::path::Path;

const START_PREFIX: &str = "<!-- mustard:enrich hash=";
const START_SUFFIX: &str = " -->";
const END: &str = "<!-- /mustard:enrich -->";

/// The placeholder body a freshly-generated (not-yet-enriched) block carries.
pub const PENDING: &str = "_(pending `/scan` enrich)_";

/// 12-hex-char fingerprint of the deterministic skeleton the prose belongs to.
/// Short on purpose — it only needs to detect change, not resist collision.
#[must_use]
pub fn fingerprint(skeleton: &str) -> String {
    let mut h = Sha256::new();
    h.update(skeleton.as_bytes());
    let digest = h.finalize();
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Render a pending (un-enriched) block stamped with `hash`.
#[must_use]
pub fn pending_block(hash: &str) -> String {
    format!("{START_PREFIX}{hash}{START_SUFFIX}\n{PENDING}\n{END}")
}

/// Render an enriched block carrying `prose`, stamped with `hash`.
#[must_use]
pub fn enriched_block(hash: &str, prose: &str) -> String {
    format!("{START_PREFIX}{hash}{START_SUFFIX}\n{}\n{END}", prose.trim())
}

/// Extract `(hash, inner)` of the enrich block in `content`, if present. `inner`
/// is the text between the markers, trimmed of the surrounding newlines.
#[must_use]
pub fn extract(content: &str) -> Option<(String, String)> {
    let start = content.find(START_PREFIX)?;
    let hash_from = start + START_PREFIX.len();
    let hash_len = content[hash_from..].find(START_SUFFIX)?;
    let hash = content[hash_from..hash_from + hash_len].to_string();
    let inner_from = hash_from + hash_len + START_SUFFIX.len();
    let inner_len = content[inner_from..].find(END)?;
    let inner = content[inner_from..inner_from + inner_len].trim().to_string();
    Some((hash, inner))
}

/// `true` when `inner` is the pending placeholder (no real prose yet).
#[must_use]
pub fn is_pending(inner: &str) -> bool {
    inner.trim() == PENDING || inner.trim().is_empty()
}

/// Merge a freshly-generated `skeleton` (which already contains a pending block
/// stamped with `hash`) against the `previous` file content: when `previous`
/// carries a non-pending block whose hash matches `hash`, restore that prose;
/// otherwise return `skeleton` unchanged (block stays pending).
#[must_use]
pub fn merge(skeleton: &str, previous: Option<&str>, hash: &str) -> String {
    let Some(previous) = previous else {
        return skeleton.to_string();
    };
    let Some((prev_hash, prev_inner)) = extract(previous) else {
        return skeleton.to_string();
    };
    if prev_hash != hash || is_pending(&prev_inner) {
        return skeleton.to_string();
    }
    // Restore the preserved prose into the skeleton's block.
    replace_block(skeleton, &enriched_block(hash, &prev_inner))
}

/// Position just past the newline ending the first `# ` heading line, if any.
fn after_first_h1(s: &str) -> Option<usize> {
    let line_start = if s.starts_with("# ") { 0 } else { s.find("\n# ")? + 1 };
    let nl = s[line_start..].find('\n')? + line_start + 1;
    Some(nl)
}

/// Insert a pending enrich block into a freshly-generated `skeleton` at the
/// canonical spot — just after the first H1 heading, or prepended when the
/// skeleton has none. The block's hash fingerprints the whole skeleton, so any
/// change to the deterministic body invalidates a previously-written prose.
#[must_use]
pub fn insert(skeleton: &str) -> String {
    let hash = fingerprint(skeleton);
    match after_first_h1(skeleton) {
        Some(at) => format!("{}\n{}\n{}", &skeleton[..at], pending_block(&hash), &skeleton[at..]),
        None => format!("{}\n\n{skeleton}", pending_block(&hash)),
    }
}

/// Write `content` (already carrying a pending block from [`insert`]) to `path`,
/// preserving a previously-enriched block whose hash still matches. Returns
/// whether the write succeeded; fail-open at the IO layer.
#[must_use]
pub fn write_preserving(path: &Path, content: &str) -> bool {
    let hash = extract(content).map_or_else(String::new, |(h, _)| h);
    let previous = mfs::read_to_string(path).ok();
    let merged = merge(content, previous.as_deref(), &hash);
    mfs::write_atomic(path, merged.as_bytes()).is_ok()
}

/// Generate-and-write a scan artifact so it carries a preserved enrich block:
/// inserts a pending block into the `skeleton`, then writes it to `path` while
/// restoring any matching prose already on disk. The one call every `.md`
/// generator uses in place of a raw write. Returns whether the write succeeded.
#[must_use]
pub fn write_enrichable(path: &Path, skeleton: &str) -> bool {
    write_preserving(path, &insert(skeleton))
}

/// Replace the whole enrich block in `content` with `replacement`. Returns
/// `content` unchanged when no block is present.
#[must_use]
fn replace_block(content: &str, replacement: &str) -> String {
    let Some(start) = content.find(START_PREFIX) else {
        return content.to_string();
    };
    let Some(end_rel) = content[start..].find(END) else {
        return content.to_string();
    };
    let end = start + end_rel + END.len();
    format!("{}{replacement}{}", &content[..start], &content[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_round_trips() {
        let block = pending_block("abc123");
        let (hash, inner) = extract(&block).unwrap();
        assert_eq!(hash, "abc123");
        assert!(is_pending(&inner));
    }

    #[test]
    fn enriched_round_trips() {
        let block = enriched_block("abc123", "## Purpose\n\nDoes a thing.");
        let (hash, inner) = extract(&block).unwrap();
        assert_eq!(hash, "abc123");
        assert!(!is_pending(&inner));
        assert!(inner.contains("Does a thing."));
    }

    #[test]
    fn merge_preserves_matching_hash() {
        let skeleton = format!("# T\n\n{}\n\n## Convention\n", pending_block("h1"));
        let previous = format!("# T\n\n{}\n\n## Convention\n", enriched_block("h1", "## Purpose\n\nKept."));
        let merged = merge(&skeleton, Some(&previous), "h1");
        assert!(merged.contains("Kept."), "matching-hash prose must be preserved:\n{merged}");
    }

    #[test]
    fn merge_drops_stale_hash() {
        // The skeleton changed → new hash `h2`; the previous prose was for `h1`.
        let skeleton = format!("# T\n\n{}\n\n## Convention\n", pending_block("h2"));
        let previous = format!("# T\n\n{}\n", enriched_block("h1", "## Purpose\n\nStale."));
        let merged = merge(&skeleton, Some(&previous), "h2");
        assert!(!merged.contains("Stale."), "stale prose must be dropped:\n{merged}");
        assert!(extract(&merged).is_some_and(|(_, i)| is_pending(&i)));
    }

    #[test]
    fn merge_no_previous_is_skeleton() {
        let skeleton = format!("# T\n\n{}\n", pending_block("h1"));
        assert_eq!(merge(&skeleton, None, "h1"), skeleton);
    }

    #[test]
    fn fingerprint_is_stable_and_sensitive() {
        assert_eq!(fingerprint("abc"), fingerprint("abc"));
        assert_ne!(fingerprint("abc"), fingerprint("abd"));
        assert_eq!(fingerprint("abc").len(), 12);
    }
}
