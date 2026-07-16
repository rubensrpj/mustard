//! Mold provenance — the marker that separates a MACHINE-authored pattern mold
//! (regenerated fresh on every scan) from one a human has touched (preserved
//! forever).
//!
//! `scan-patterns-apply` stamps every mold it writes with a trailing HTML
//! comment carrying the SHA-256 of the exact body it wrote:
//!
//! ```text
//! <!-- mustard:provenance sha256:<64 hex> -->
//! ```
//!
//! On the next scan [`verify`] recomputes the digest over the file WITHOUT the
//! marker line: a match means no human edited the mold since the scan wrote it
//! ([`Provenance::Pristine`] — refresh-eligible), a mismatch means hand
//! maintenance ([`Provenance::Edited`] — preserved), and a missing marker means
//! a legacy or hand-authored skill ([`Provenance::Unmarked`] — preserved).
//! Line endings are folded (`\r\n` → `\n`) on both the stamp and the verify
//! side so a CRLF round-trip through an editor or a git filter never flips a
//! pristine mold to edited.

/// Leading text of the provenance marker line.
const MARKER_PREFIX: &str = "<!-- mustard:provenance sha256:";
/// Trailing text of the provenance marker line.
const MARKER_SUFFIX: &str = " -->";

/// How a mold on disk relates to the scan that (maybe) wrote it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Provenance {
    /// Marker present and the digest matches — machine-authored, untouched.
    Pristine,
    /// Marker present but the digest does not match — hand-edited.
    Edited,
    /// No marker — authored before provenance existed, or by hand.
    Unmarked,
}

/// Normalise a mold body the way `apply` writes it: CRLF folded to LF, any
/// stray provenance lines dropped (a body must never carry its own marker),
/// trailing whitespace trimmed to a single final newline.
pub(crate) fn normalize(body: &str) -> String {
    let unified = body.replace("\r\n", "\n");
    let kept: Vec<&str> = unified.lines().filter(|l| !is_marker(l)).collect();
    format!("{}\n", kept.join("\n").trim_end())
}

/// The marker line for `normalized` (which must already be [`normalize`]d).
pub(crate) fn marker_for(normalized: &str) -> String {
    format!("{MARKER_PREFIX}{}{MARKER_SUFFIX}", digest(normalized))
}

/// Classify a mold file's content. See [`Provenance`].
pub(crate) fn verify(text: &str) -> Provenance {
    let unified = text.replace("\r\n", "\n");
    let Some(expected) = unified.lines().rev().find_map(marker_digest) else {
        return Provenance::Unmarked;
    };
    if digest(&normalize(&unified)) == expected {
        Provenance::Pristine
    } else {
        Provenance::Edited
    }
}

/// Whether `line` is a provenance marker line.
fn is_marker(line: &str) -> bool {
    marker_digest(line).is_some()
}

/// Extract the digest from a marker line; `None` when `line` is not a marker.
fn marker_digest(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix(MARKER_PREFIX)?;
    let hex = rest.strip_suffix(MARKER_SUFFIX)?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex.to_string())
}

/// SHA-256 hex digest of `text` (the crate's dependency-free implementation).
fn digest(text: &str) -> String {
    let mut h = crate::util::sha256::Sha256::new();
    h.update(text.as_bytes());
    h.hex_digest()
}

/// Stamp `body` for writing: the normalised body followed by its marker line.
pub(crate) fn stamp(body: &str) -> String {
    let normalized = normalize(body);
    let marker = marker_for(&normalized);
    format!("{normalized}{marker}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamped_body_verifies_pristine() {
        let out = stamp("# mold\n\nbody text");
        assert!(out.ends_with(" -->\n"), "marker is the last line: {out}");
        assert_eq!(verify(&out), Provenance::Pristine);
    }

    #[test]
    fn edited_body_verifies_edited() {
        let out = stamp("# mold\n\nbody text");
        let touched = out.replace("body text", "body text, tweaked by hand");
        assert_eq!(verify(&touched), Provenance::Edited);
    }

    #[test]
    fn no_marker_verifies_unmarked() {
        assert_eq!(verify("# a hand-authored skill\n"), Provenance::Unmarked);
        assert_eq!(verify(""), Provenance::Unmarked);
    }

    #[test]
    fn crlf_round_trip_stays_pristine() {
        let out = stamp("# mold\n\nbody");
        let crlf = out.replace('\n', "\r\n");
        assert_eq!(verify(&crlf), Provenance::Pristine, "a line-ending filter is not a hand edit");
    }

    #[test]
    fn body_carrying_a_stray_marker_is_scrubbed_before_stamping() {
        let out = stamp("# mold\n<!-- mustard:provenance sha256:0000000000000000000000000000000000000000000000000000000000000000 -->\nbody");
        assert_eq!(out.matches(MARKER_PREFIX).count(), 1, "exactly one marker survives: {out}");
        assert_eq!(verify(&out), Provenance::Pristine);
    }
}
