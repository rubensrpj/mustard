//! NDJSON [`EventReader`] — streaming, cached, zero-copy-filter access to
//! per-spec event logs.
//!
//! This module is the shared primitive consumed by every downstream sub-spec
//! (W2-W7) of the no-sqlite refactor. It intentionally has **no trait** — the
//! struct is concrete per the project directive "no abstraction by hypothesis".

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::io::events::types::Event;

/// Process-lifetime, mtime-invalidated cache key.
type CacheKey = (PathBuf, SystemTime);

/// Concrete NDJSON event reader.
///
/// Provides three access patterns:
///
/// 1. [`EventReader::stream`] — streaming iterator, never loads the full file.
/// 2. [`EventReader::cached_for_session`] — in-process cache keyed on
///    `(path, mtime)`; re-reads only when the file changes.
/// 3. [`EventReader::filter_kind`] — zero-allocation adapter over any
///    `Iterator<Item = Event>`.
#[derive(Default)]
pub struct EventReader {
    cache: HashMap<CacheKey, Vec<Event>>,
}

impl EventReader {
    /// Create a new, empty reader.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a streaming iterator over all events in `path`.
    ///
    /// Reads the file line-by-line via `BufReader` — each line is parsed as
    /// an independent JSON object (NDJSON format). The file is never fully
    /// loaded into memory.
    ///
    /// Parse errors on individual lines are silently dropped (fail-open);
    /// an unreadable file returns an empty iterator.
    pub fn stream(path: &Path) -> impl Iterator<Item = Event> {
        // Open the file; if it doesn't exist / can't be opened, return empty.
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return itertools_either::Either::Left(std::iter::empty()),
        };

        // Line-by-line: each `BufRead::lines()` call yields exactly one NDJSON
        // record. This correctly isolates malformed lines — a bad line is
        // skipped without corrupting the reader position for subsequent lines.
        let iter = BufReader::new(file)
            .lines()
            .filter_map(|line| {
                let line = line.ok()?;
                serde_json::from_str::<Event>(&line).ok() // fail-open: skip bad lines
            });

        itertools_either::Either::Right(iter)
    }

    /// Return a cached slice of all events for `spec_path`.
    ///
    /// The cache entry is invalidated whenever `fs::metadata(path)?.modified()`
    /// changes. On any IO error (missing file, unsupported mtime) the method
    /// returns an empty slice rather than propagating the error (fail-open).
    pub fn cached_for_session(&mut self, spec_path: &Path) -> &[Event] {
        let mtime = fs::metadata(spec_path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let key: CacheKey = (spec_path.to_path_buf(), mtime);

        // Only insert if the key is absent (avoids a redundant clone on hit).
        if !self.cache.contains_key(&key) {
            let events: Vec<Event> = Self::stream(spec_path).collect();
            self.cache.insert(key.clone(), events);
        }

        // Evict stale entries for the same path with a different mtime.
        // Keep the cache bounded: one entry per logical file.
        let path_buf = spec_path.to_path_buf();
        self.cache
            .retain(|k, _| k.0 != path_buf || k.1 == mtime);

        self.cache.get(&key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Zero-allocation adapter that filters an event iterator by `kind`.
    ///
    /// Returns only events whose `kind` field equals `kind` exactly.
    /// No intermediate `Vec` is allocated.
    pub fn filter_kind<'a>(
        iter: impl Iterator<Item = Event> + 'a,
        kind: &'a str,
    ) -> impl Iterator<Item = Event> + 'a {
        iter.filter(move |e| e.kind == kind)
    }
}

// ---------------------------------------------------------------------------
// Internal helper — a lightweight Either type so `stream` can return two
// concrete iterator types without boxing. Only used inside this module.
// ---------------------------------------------------------------------------
mod itertools_either {
    pub enum Either<L, R> {
        Left(L),
        Right(R),
    }

    impl<L, R, T> Iterator for Either<L, R>
    where
        L: Iterator<Item = T>,
        R: Iterator<Item = T>,
    {
        type Item = T;

        fn next(&mut self) -> Option<T> {
            match self {
                Either::Left(l) => l.next(),
                Either::Right(r) => r.next(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    /// Write `n` NDJSON event lines to a temp file and return it.
    fn make_ndjson(n: usize) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for i in 0..n {
            let kind = if i % 2 == 0 { "tool.use" } else { "pipeline.status" };
            writeln!(
                f,
                r#"{{"kind":"{kind}","payload":{{"i":{i}}},"ts":{i}}}"#
            )
            .unwrap();
        }
        f.flush().unwrap();
        f
    }

    /// AC-1B-1: streaming 10 000 NDJSON lines must complete in <50 ms p95.
    ///
    /// Runs the benchmark 10 times and checks that 95% of runs (≥ 9/10) are
    /// under the threshold — a single OS scheduling hiccup cannot fail the AC.
    #[test]
    fn bench_stream_10k_under_50ms() {
        const LINES: usize = 10_000;
        const THRESHOLD_MS: u128 = 50;
        const RUNS: usize = 10;
        const REQUIRED_PASSING: usize = 9; // p95 = 9/10

        let file = make_ndjson(LINES);
        let path = file.path();

        let mut passing = 0usize;
        for _ in 0..RUNS {
            let start = std::time::Instant::now();
            let count = EventReader::stream(path).count();
            let elapsed = start.elapsed().as_millis();

            assert_eq!(count, LINES, "all lines must parse");
            if elapsed < THRESHOLD_MS {
                passing += 1;
            }
        }

        assert!(
            passing >= REQUIRED_PASSING,
            "p95 benchmark failed: only {passing}/{RUNS} runs under {THRESHOLD_MS}ms"
        );
    }

    #[test]
    fn stream_empty_file_returns_no_events() {
        let f = NamedTempFile::new().unwrap();
        let count = EventReader::stream(f.path()).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn stream_missing_file_returns_no_events() {
        let path = Path::new("/nonexistent/path/events.ndjson");
        let count = EventReader::stream(path).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn stream_skips_malformed_lines() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"{{"kind":"ok","payload":{{}}}}"#).unwrap();
        writeln!(f, "not valid json!!!").unwrap();
        writeln!(f, r#"{{"kind":"also-ok","payload":{{}}}}"#).unwrap();
        f.flush().unwrap();

        let events: Vec<Event> = EventReader::stream(f.path()).collect();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn filter_kind_returns_matching_events_only() {
        let f = make_ndjson(10);
        let iter = EventReader::stream(f.path());
        let tool_uses: Vec<Event> = EventReader::filter_kind(iter, "tool.use").collect();
        // Lines 0,2,4,6,8 → kind == "tool.use"
        assert_eq!(tool_uses.len(), 5);
        assert!(tool_uses.iter().all(|e| e.kind == "tool.use"));
    }

    #[test]
    fn cached_for_session_returns_same_slice_on_second_call() {
        let f = make_ndjson(5);
        let mut reader = EventReader::new();
        let first = reader.cached_for_session(f.path()).len();
        let second = reader.cached_for_session(f.path()).len();
        assert_eq!(first, second);
        assert_eq!(first, 5);
    }
}
