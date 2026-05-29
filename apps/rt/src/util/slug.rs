//! Content-addressed slug for memory / knowledge markdown filenames.
//!
//! Before this module, `fnv1a8` + `slug_for` were copy-pasted verbatim into
//! three modules (`commands::knowledge::memory`,
//! `commands::knowledge::memory_ingest`, and the `hooks::observe::memory_promote_observer`
//! observer). This is the single home; every call site uses [`slug_for`]
//! directly. `fnv1a8` stays private — it is only ever the hash suffix of a slug.

/// Compute a short FNV-1a hash of `s` (8 hex chars). Slug suffix only — not
/// security-relevant.
fn fnv1a8(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:016x}", h).chars().take(8).collect()
}

/// Slug shape: `{compact_ts}-{hash8}` — filename-safe, deterministic per
/// `(timestamp, content)` pair. `captured_at` is reduced to its alphanumeric
/// characters; `content` is hashed with [`fnv1a8`].
#[must_use]
pub fn slug_for(captured_at: &str, content: &str) -> String {
    let ts_compact: String = captured_at
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    format!("{ts_compact}-{}", fnv1a8(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_deterministic() {
        let a = slug_for("2026-05-27T00:00:00.000Z", "hello");
        let b = slug_for("2026-05-27T00:00:00.000Z", "hello");
        assert_eq!(a, b);
        let c = slug_for("2026-05-27T00:00:00.000Z", "world");
        assert_ne!(a, c);
    }

    #[test]
    fn slug_shape_is_compact_ts_and_hash8() {
        let s = slug_for("2026-05-27T00:00:00.000Z", "hello");
        let (ts, hash) = s.split_once('-').expect("has separator");
        // Non-alphanumerics (`-`, `:`, `.`) are stripped from the timestamp.
        assert_eq!(ts, "20260527T000000000Z");
        assert_eq!(hash.len(), 8);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
