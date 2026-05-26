//! Content-addressed blob spill for the NDJSON event log.
//!
//! ## Why
//!
//! The W5 NDJSON writer keeps the per-event line small so a `tail`-style live
//! reader (the dashboard) stays under 16 ms per redraw even at 500+ events per
//! wave. Large payloads (Bash transcripts, full file Reads, Task return text)
//! would blow that budget if inlined — they routinely run 5-100 KB.
//!
//! Instead, anything strictly larger than [`SPILL_THRESHOLD_BYTES`] is written
//! once to a content-addressed blob under
//! `<spec_or_session_root>/blobs/{ab}/{sha256}.bin`, and the NDJSON line keeps
//! only the reference (`{"$blob": "<sha256>", "len": <bytes>}`). The address is
//! the SHA-256 of the bytes themselves, so identical payloads (a repeated Read
//! of the same file, a re-run of the same Bash command) share a single blob —
//! no de-dup pass needed.
//!
//! ## Layout
//!
//! ```text
//! .claude/spec/{name}/[wave-N-{role}/]
//! ├── .events/
//! │   └── 1700000000000000-abc-12345.ndjson
//! └── .blobs/
//!     ├── ab/
//!     │   └── abc123…ef.bin
//!     └── cd/
//!         └── cd9876…21.bin
//! ```
//!
//! The two-character prefix keeps any single subdirectory small (capped at 256
//! children) so a `readdir` stays fast even when a wave generates thousands of
//! distinct blobs.
//!
//! ## Fail-open
//!
//! Every error degrades to "inline the payload" — a write that cannot reach the
//! blob directory falls back to keeping the payload on the NDJSON line. The
//! reader sees a normal payload and renders it. Telemetry is never load-bearing.

// W5 follow-up: `maybe_spill` is the active write path (called from
// `event_writer_ndjson::write_event_inner`). `blob_path` is part of the
// reader contract the dashboard's NDJSON tailer consumes — kept public for
// the upcoming `apps/dashboard` reader and exercised by this module's tests,
// so it is annotated locally rather than under a module-wide allow.

use crate::util::sha256::Sha256;
use mustard_core::fs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Payloads strictly larger than this are spilled to a blob. Sized to keep one
/// NDJSON line under a typical 4 KB filesystem block — a `tail -F` consumer
/// then reads one block per event in the steady state.
pub const SPILL_THRESHOLD_BYTES: usize = 4 * 1024;

/// The reference written in place of a spilled payload.
///
/// `$blob` is the SHA-256 hex digest of the original bytes; `len` lets the
/// reader pre-allocate without an extra `stat`. The leading `$` mirrors the
/// MongoDB convention for "this is a reference, not a literal value" — a key
/// that no legitimate event payload will ever choose.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlobRef {
    /// SHA-256 hex digest of the spilled bytes — the blob's address.
    #[serde(rename = "$blob")]
    pub sha256: String,
    /// Byte length of the original payload.
    pub len: usize,
}

/// Outcome of a spill attempt.
#[derive(Debug)]
pub enum SpillOutcome {
    /// The payload was small enough to keep inline — caller embeds it directly.
    Inline,
    /// The payload was spilled to `path`; embed `reference` in the NDJSON line.
    Spilled {
        /// Final on-disk path of the blob. Returned for tests + future
        /// inspection (the production writer only reads `reference`).
        #[allow(dead_code)]
        path: PathBuf,
        /// Reference to embed in the NDJSON line.
        reference: BlobRef,
    },
}

/// Decide whether `payload_bytes` warrants spilling; write the blob when so.
///
/// `root` is the spec or session directory under which `blobs/` lives. The
/// blob is written **atomically** via `fs::write_atomic` so a concurrent
/// reader never sees a half-written file. Writing the same content twice is a
/// no-op — the existence check happens before the write.
///
/// Returns [`SpillOutcome::Inline`] when the payload is below
/// [`SPILL_THRESHOLD_BYTES`] OR any write step failed (fail-open). The caller
/// must then embed the original payload on the NDJSON line.
pub fn maybe_spill(root: &Path, payload_bytes: &[u8]) -> SpillOutcome {
    if payload_bytes.len() <= SPILL_THRESHOLD_BYTES {
        return SpillOutcome::Inline;
    }
    let digest = sha256_hex(payload_bytes);
    let (subdir, file) = split_address(&digest);
    let dir = root.join(".blobs").join(subdir);
    let path = dir.join(format!("{file}.bin"));

    // Idempotent: the address IS the content, so a hit means the bytes are
    // already on disk. No further work required.
    if path.exists() {
        return SpillOutcome::Spilled {
            path,
            reference: BlobRef {
                sha256: digest,
                len: payload_bytes.len(),
            },
        };
    }

    if fs::create_dir_all(&dir).is_err() {
        return SpillOutcome::Inline;
    }
    if fs::write_atomic(&path, payload_bytes).is_err() {
        return SpillOutcome::Inline;
    }
    SpillOutcome::Spilled {
        path,
        reference: BlobRef {
            sha256: digest,
            len: payload_bytes.len(),
        },
    }
}

/// Resolve a blob reference back to its on-disk path. Used by the reader to
/// stream the blob into the dashboard timeline drawer on demand.
///
/// Marked `allow(dead_code)` because the in-tree writer path never resolves a
/// reference — it spills and embeds the address inline. The dashboard reader
/// (in `apps/dashboard/src-tauri`) is the genuine consumer and lives outside
/// this crate. The function is kept public + tested so the reader contract
/// stays a single source of truth (path layout = writer's split_address).
#[allow(dead_code)]
#[must_use]
pub fn blob_path(root: &Path, reference: &BlobRef) -> PathBuf {
    let (subdir, file) = split_address(&reference.sha256);
    root.join(".blobs").join(subdir).join(format!("{file}.bin"))
}

/// Split a hex digest into the first two characters (subdirectory) and the
/// remainder (file stem). Panics on an empty input — guarded by every caller
/// (we only construct addresses from `Sha256`, which always returns 64 chars).
fn split_address(digest: &str) -> (&str, &str) {
    // `unwrap_or` so the function is `#[must_use]`-friendly and never panics;
    // a degenerate input degrades to `("00", digest)` which is still writable.
    let prefix = digest.get(0..2).unwrap_or("00");
    let rest = digest.get(2..).unwrap_or(digest);
    (prefix, rest)
}

/// SHA-256 hex digest of `bytes` — thin wrapper over the in-tree implementation
/// so the rest of this module never imports the hasher directly.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.hex_digest()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn small_payload_stays_inline() {
        let dir = tempdir().unwrap();
        let outcome = maybe_spill(dir.path(), b"tiny payload");
        assert!(matches!(outcome, SpillOutcome::Inline));
        assert!(!dir.path().join(".blobs").exists(), "no .blobs dir for inline");
    }

    #[test]
    fn large_payload_spills_to_addressed_path() {
        let dir = tempdir().unwrap();
        let big = vec![b'a'; SPILL_THRESHOLD_BYTES + 1];
        let outcome = maybe_spill(dir.path(), &big);
        let SpillOutcome::Spilled { path, reference } = outcome else {
            panic!("expected spill");
        };
        assert!(path.exists());
        assert_eq!(reference.len, big.len());
        assert_eq!(reference.sha256.len(), 64, "sha256 hex is 64 chars");
        // Path shape: <root>/.blobs/<2chars>/<62chars>.bin
        let components: Vec<_> = path
            .strip_prefix(dir.path())
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(components[0], ".blobs");
        assert_eq!(components[1].len(), 2);
        assert!(components[2].ends_with(".bin"));
    }

    #[test]
    fn identical_payloads_share_a_blob() {
        let dir = tempdir().unwrap();
        let big = vec![b'x'; SPILL_THRESHOLD_BYTES + 100];
        let first = maybe_spill(dir.path(), &big);
        let second = maybe_spill(dir.path(), &big);
        let SpillOutcome::Spilled { path: p1, reference: r1 } = first else {
            panic!("first spill failed");
        };
        let SpillOutcome::Spilled { path: p2, reference: r2 } = second else {
            panic!("second spill failed");
        };
        assert_eq!(p1, p2, "same content → same path");
        assert_eq!(r1, r2);
    }

    #[test]
    fn blob_path_round_trips_a_reference() {
        let dir = tempdir().unwrap();
        let big = vec![b'z'; SPILL_THRESHOLD_BYTES + 32];
        let SpillOutcome::Spilled { path, reference } = maybe_spill(dir.path(), &big) else {
            panic!("expected spill");
        };
        let resolved = blob_path(dir.path(), &reference);
        assert_eq!(resolved, path);
    }
}
