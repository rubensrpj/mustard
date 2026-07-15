//! `knowledge_store` — the single on-disk owner of [`Knowledge`] records.
//!
//! ## Responsibility
//!
//! [`KnowledgeStore`] is the **one** module that reads and writes
//! [`Knowledge`] markdown files. It owns the disk layout and the
//! frontmatter+body fencing, delegating the actual byte-write to the canonical
//! atomic primitive [`crate::io::fs::write_atomic`] and the frontmatter parse
//! to the shared [`crate::io::atomic_md::frontmatter`] parser. The
//! [`Knowledge`] type itself (in `domain/model/`) stays pure — it knows how to
//! turn into `(frontmatter, body)` but never touches disk.
//!
//! ## Layout (the deterministic decision)
//!
//! A store is rooted at the project's `.claude/` directory and lays records out
//! onto the **pre-existing legacy directories** so the five legacy *readers*
//! find the unified *writes* with **no migration**:
//!
//! ```text
//! <.claude>/
//! ├── memory/agent/{slug}.md       ← Kind::Summary  (any scope)
//! ├── memory/decisions/{slug}.md   ← Kind::Decision
//! ├── memory/lessons/{slug}.md     ← Kind::Lesson
//! ├── knowledge/{slug}.md          ← Kind::Principle | Reference, Scope::Global
//! └── spec/{spec}/memory/{slug}.md ← Kind::Principle | Reference, Scope::Spec/Wave
//! ```
//!
//! - The directory is chosen by `(kind, scope)` — see [`legacy_subdir`]. This is
//!   the exact tree the legacy writers used, so a reader scanning
//!   `.claude/memory/agent/` (or `knowledge/`, …) keeps working unchanged.
//! - The filename is the content-addressed [`Knowledge::slug`], so writing the
//!   same logical record twice is idempotent (same path) and two distinct
//!   records never collide.
//!
//! ## Backward-compatible frontmatter (the no-break contract)
//!
//! [`Knowledge::to_markdown`] emits the *canonical* key set (`kind`, `scope`,
//! `label`, `captured_at`, `confidence`, `status`, …). The legacy readers,
//! however, read **legacy** keys that differ per directory:
//!
//! | dir                  | reader expects                          |
//! |----------------------|-----------------------------------------|
//! | `memory/agent/`      | `summary`, `at`, `last_used`, `session_id` |
//! | `knowledge/`         | `name`, `description`                    |
//!
//! So on the way to disk the store **augments** the canonical frontmatter with
//! the legacy aliases for the target directory (see [`augment_legacy_aliases`]).
//! The canonical keys are kept too — [`Knowledge::from_markdown`] still recovers
//! a faithful record, and the aliases are pure addition (the readers ignore
//! unknown keys). The store is the right home for this: directory layout and
//! legacy compatibility are an `io` concern, never a `domain/model/` one (which
//! stays pure).
//!
//! ## Fail-open
//!
//! - [`KnowledgeStore::read_all`] never panics: an unreadable file, a missing
//!   directory, or malformed frontmatter degrades that record out of the result
//!   set; everything readable still comes back. Order is deterministic (sorted
//!   by path) so callers and snapshot tests are stable.
//! - [`KnowledgeStore::write`] is the single quality gate on capture: a
//!   non-[substantive](crate::domain::model::knowledge::Knowledge::is_substantive)
//!   record (empty body, placeholder summary, context echo) is silently skipped
//!   (`Ok(None)`); a real record is written (`Ok(Some(path))`). The only error
//!   surface is a genuine IO failure from the atomic write. No `unwrap`/`expect`
//!   outside tests.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::domain::model::knowledge::{Kind, Knowledge, Scope};
use crate::io::atomic_md::frontmatter;
use crate::io::fs;
use crate::platform::error::Result;

/// The on-disk owner of [`Knowledge`] records, rooted at one directory.
///
/// Cheap to construct and clone — it holds only the root path. Build it over
/// whatever directory the caller wants the store to live in (the wiring of
/// *which* directory that is belongs to the `rt` layer, not here).
#[derive(Debug, Clone)]
pub struct KnowledgeStore {
    root: PathBuf,
}

impl KnowledgeStore {
    /// Open a store rooted at `root`. The directory is **not** created here;
    /// it is materialised lazily on the first [`write`](Self::write).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The store's root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The legacy directory a record lives in, resolved by `(kind, scope)` and
    /// rooted at the store's `.claude/` root — see [`legacy_subdir`].
    #[must_use]
    fn dir_for(&self, k: &Knowledge) -> PathBuf {
        self.root.join(legacy_subdir(k))
    }

    /// The canonical on-disk path for `k`: `<root>/<legacy-subdir>/<slug>.md`.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn path_for(&self, k: &Knowledge) -> PathBuf {
        self.dir_for(k).join(format!("{}.md", k.slug()))
    }

    /// Write `k` atomically to its legacy directory, creating it if needed.
    ///
    /// This is the **single quality gate** on knowledge capture: a record that
    /// is not [substantive](Knowledge::is_substantive) (empty body, placeholder
    /// summary like `"interrupted mid-task"`, or pure context echo) is **not
    /// written** and the call returns `Ok(None)`. Garbage is rejected at the one
    /// entry point — no caller duplicates this check. A substantive record is
    /// written and the call returns `Ok(Some(path))`.
    ///
    /// The on-disk frontmatter is the canonical [`Knowledge::to_markdown`] set
    /// **plus** the legacy aliases the directory's readers expect (see
    /// [`augment_legacy_aliases`]), so the existing readers keep working with no
    /// migration.
    ///
    /// Idempotent for a given record: the content-addressed slug means the same
    /// logical record always lands at the same path.
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::platform::error::Error::Io) when the directory cannot
    /// be created or the atomic write fails. A skipped (non-substantive) record
    /// is **not** an error — it is `Ok(None)`.
    pub fn write(&self, k: &Knowledge) -> Result<Option<PathBuf>> {
        // The one quality gate: reject measured noise before it ever hits disk.
        if !k.is_substantive() {
            return Ok(None);
        }
        let dir = self.dir_for(k);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.md", k.slug()));
        let (mut fm, body) = k.to_markdown();
        augment_legacy_aliases(k, &mut fm);
        let text = render_markdown(&fm, &body);
        fs::write_atomic(&path, text.as_bytes())?;
        Ok(Some(path))
    }

    /// Read every record in the store, optionally restricted to one [`Scope`].
    ///
    /// Fail-open: unreadable or unparseable files are skipped, never fatal. The
    /// result is sorted by source path so the order is deterministic across
    /// runs and platforms.
    ///
    /// When `scope_filter` is `Some`, only records whose [`Knowledge::scope`]
    /// **equals** the filter are returned (exact match — a `Spec{demo}` filter
    /// does not pull in `Wave{demo,1}` records; pass the precise reach you want,
    /// or `None` for everything).
    ///
    /// ## Quality gate on the read side (same criterion as [`write`])
    ///
    /// A record that is not
    /// [substantive](Knowledge::is_substantive) is **skipped** — the store never
    /// surfaces noise, not even the legacy garbage already sitting on disk. This
    /// is the *same single criterion* the [`write`](Self::write) gate consults,
    /// applied at the one read entry point, so every reader ([`recall`] and the
    /// render included) inherits the filter without re-implementing it. The
    /// name-addressed per-spec memory (`spec/{spec}/memory`) is hidden here too
    /// rather than destroyed; [`knowledge prune`](crate) only deletes the four
    /// content-addressed store dirs.
    ///
    /// [`write`]: Self::write
    /// [`recall`]: crate
    #[must_use]
    pub fn read_all(&self, scope_filter: Option<&Scope>) -> Vec<Knowledge> {
        // Read ONLY the knowledge-store directories, never the whole `.claude/`
        // tree: config/docs (`CLAUDE.md`, `refs/`, `skills/`, `commands/`, …)
        // are not knowledge and must never leak into recall. The set of dirs is
        // the single source of truth in [`knowledge_dirs`].
        let mut paths: Vec<PathBuf> = Vec::new();
        for dir in knowledge_dirs(&self.root) {
            paths.extend(md_files_in(&dir));
        }
        // Deterministic order regardless of filesystem enumeration order.
        paths.sort();

        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            let Some(k) = read_one(&path) else { continue };
            // The one quality gate, on the read side: never surface noise — not
            // even the legacy garbage already on disk. Same `is_substantive`
            // criterion as the write gate; one rule, both ends (SOLID).
            if !k.is_substantive() {
                continue;
            }
            if let Some(want) = scope_filter {
                if &k.scope != want {
                    continue;
                }
            }
            out.push(k);
        }
        out
    }

    /// Read the single record at `path`, parsing its frontmatter + body into a
    /// [`Knowledge`].
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`](crate::platform::error::Error::NotFound) when the file
    /// is absent, [`Error::Io`](crate::platform::error::Error::Io) on a genuine
    /// read failure. A file with no frontmatter still parses (every field falls
    /// back to its default via [`Knowledge::from_markdown`]).
    pub fn read(&self, path: &Path) -> Result<Knowledge> {
        let text = fs::read_to_string(path)?;
        Ok(parse_text(&text))
    }

}

// ---------------------------------------------------------------------------
// Legacy layout + frontmatter compatibility (the no-migration contract)
// ---------------------------------------------------------------------------

/// The four **fixed, content-addressed** store sub-directories (path segments
/// relative to the `.claude/` root). This is the single source of truth for the
/// store's fixed dirs — both the read scope ([`knowledge_dirs`]) and the prune
/// sweep ([`crate::io::knowledge_store`] consumers) derive from it rather than
/// re-listing the dirs. The fifth, name-addressed `spec/{spec}/memory` store is
/// deliberately **not** here: it is globbed at read time and never swept by
/// prune, so it stays out of the fixed set.
pub(crate) const STORE_DIRS: [&[&str]; 4] = [
    &["memory", "agent"],
    &["memory", "decisions"],
    &["memory", "lessons"],
    &["knowledge"],
];

/// The `.claude/`-relative subdirectory a record lives in, resolved by
/// `(kind, scope)` so the legacy readers find unified writes unchanged:
///
/// - [`Kind::Summary`] → `memory/agent` (the agent-summary store; scope is
///   carried *inside* the frontmatter, so a Wave- or Spec-scoped summary still
///   lands here — the path never encodes reach).
/// - [`Kind::Decision`] → `memory/decisions`; [`Kind::Lesson`] → `memory/lessons`.
/// - [`Kind::Principle`] / [`Kind::Reference`]: `Scope::Spec`/`Scope::Wave` →
///   `spec/{spec}/memory` (per-spec memory); `Scope::Global` → `knowledge`.
///
/// A Principle/Reference whose spec slug is empty degrades to `knowledge`
/// rather than producing a `spec//memory` path (fail-safe).
#[must_use]
fn legacy_subdir(k: &Knowledge) -> PathBuf {
    let p = |s: &str| PathBuf::from(s);
    match k.kind {
        Kind::Summary => p("memory").join("agent"),
        Kind::Decision => p("memory").join("decisions"),
        Kind::Lesson => p("memory").join("lessons"),
        Kind::Principle | Kind::Reference => match k.scope.spec() {
            Some(spec) if !spec.is_empty() => p("spec").join(spec).join("memory"),
            _ => p("knowledge"),
        },
    }
}

/// Augment the canonical frontmatter `fm` with the **legacy alias keys** the
/// readers of this record's directory expect. Pure addition — every canonical
/// key stays, so [`Knowledge::from_markdown`] still round-trips; the aliases
/// only widen the map for readers that branch on the old names.
///
/// - `memory/agent/` readers (`memory search`/`list`, `memory_promote_observer`)
///   read `summary`, `at`, `last_used`, `session_id` → mirror from
///   `label` / `captured_at` / `session`.
/// - `knowledge/` readers (`session_start_inject`, `memory list`) read `name`
///   and `description` → mirror from `label` (the searchable headline) and the
///   body summary (`content` first line) respectively.
///
/// Decisions/lessons need no alias: their only reader key (`captured_at`) is
/// already canonical.
fn augment_legacy_aliases(k: &Knowledge, fm: &mut serde_json::Map<String, Value>) {
    match k.kind {
        Kind::Summary => {
            fm.insert("summary".into(), Value::String(k.label.clone()));
            fm.insert("at".into(), Value::String(k.origin.captured_at.clone()));
            fm.insert(
                "last_used".into(),
                Value::String(k.origin.captured_at.clone()),
            );
            if let Some(session) = &k.origin.session {
                fm.insert("session_id".into(), Value::String(session.clone()));
            }
        }
        Kind::Principle | Kind::Reference if k.scope.spec().is_none() => {
            // `knowledge/` (Global) — the session/list readers key on these.
            fm.insert("name".into(), Value::String(k.label.clone()));
            fm.insert("description".into(), Value::String(k.label.clone()));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Internals (free functions — fail-open, no panics)
// ---------------------------------------------------------------------------

/// Render a frontmatter object + body to a fenced markdown string.
///
/// Mirrors the writer in [`crate::io::atomic_md::store::MarkdownDoc::to_markdown`]
/// (same `key: value` shape, same array rendering) so the store's output is
/// readable by the shared frontmatter parser on the way back in. Key order is
/// the caller's (we never reorder), so byte-stability is inherited from
/// [`Knowledge::to_markdown`]'s fixed ordering.
fn render_markdown(fm: &serde_json::Map<String, serde_json::Value>, body: &str) -> String {
    use serde_json::Value;
    let mut out = String::from("---\n");
    for (k, v) in fm {
        let val_str = match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            Value::Array(arr) => {
                let joined: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect();
                format!("[{}]", joined.join(", "))
            }
            other => other.to_string(),
        };
        out.push_str(k);
        out.push_str(": ");
        out.push_str(&val_str);
        out.push('\n');
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

/// Parse raw file text (frontmatter fence + body) into a [`Knowledge`].
fn parse_text(text: &str) -> Knowledge {
    let (fm, body) = frontmatter::parse(text);
    let map = fm
        .as_ref()
        .and_then(frontmatter::Frontmatter::as_object)
        .cloned()
        .unwrap_or_default();
    Knowledge::from_markdown(&map, body)
}

/// Read + parse one record, returning `None` on any IO/parse failure
/// (fail-open).
fn read_one(path: &Path) -> Option<Knowledge> {
    let text = fs::read_to_string(path).ok()?;
    Some(parse_text(&text))
}

/// The directories that **are** the knowledge store, rooted at `root` (the
/// project's `.claude/`). This is the single source of truth for "which dirs
/// hold knowledge" on the **read** side — it mirrors the write-side layout in
/// [`legacy_subdir`] so the two never diverge (SOLID):
///
/// - the four fixed, content-addressed dirs ([`STORE_DIRS`]): `memory/agent`,
///   `memory/decisions`, `memory/lessons`, `knowledge`;
/// - plus every `spec/{spec}/memory` (the name-addressed per-spec store), found
///   by a **one-level** glob over `spec/` so cross-spec memory is recalled.
///
/// Order is deterministic: the four fixed dirs first (in declaration order),
/// then the per-spec memory dirs sorted by spec slug. **Nothing else** under
/// `.claude/` is included — `refs/`, `skills/`, `commands/`, `CLAUDE.md`,
/// `context/`, `.dispatch/`, `.session/` are config/docs, not knowledge, and
/// must never reach recall.
#[must_use]
fn knowledge_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = STORE_DIRS
        .iter()
        .map(|segs| {
            let mut d = root.to_path_buf();
            for seg in *segs {
                d.push(seg);
            }
            d
        })
        .collect();

    // One-level glob over `spec/`: each `spec/{spec}/memory`. A missing `spec/`
    // dir degrades to no entries (fail-open). Sort the spec dirs for a stable,
    // enumeration-independent order.
    let spec_root = root.join("spec");
    let mut spec_mem: Vec<PathBuf> = match fs::read_dir(&spec_root) {
        Ok(entries) => entries
            .into_iter()
            .filter(|e| e.is_dir)
            .map(|e| e.path.join("memory"))
            .collect(),
        Err(_) => Vec::new(),
    };
    spec_mem.sort();
    dirs.extend(spec_mem);
    dirs
}

/// The immediate `*.md` files of `dir` (non-recursive — store dirs are flat).
/// A missing/unreadable directory yields an empty vector (fail-open).
fn md_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.path)
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::knowledge::{Kind, Origin, Status};
    use tempfile::tempdir;

    fn sample(kind: Kind, scope: Scope, label: &str, content: &str) -> Knowledge {
        // Keep `origin.spec`/`origin.wave` consistent with the scope: the
        // markdown format stores spec/wave once (derived from the scope) and
        // recovers `origin` from those same keys, so a self-consistent record
        // is the precondition for a lossless disk round-trip.
        let origin = Origin {
            spec: scope.spec().map(str::to_string),
            wave: scope.wave(),
            captured_at: "2026-06-15T00:00:00.000Z".into(),
            ..Origin::default()
        };
        Knowledge {
            kind,
            scope,
            label: label.into(),
            content: content.into(),
            origin,
            confidence: 0.6,
            status: Status::Active,
        }
    }

    /// Write a record that is expected to be substantive, asserting it was not
    /// skipped, and return the path it landed at.
    fn write_ok(store: &KnowledgeStore, k: &Knowledge) -> PathBuf {
        store
            .write(k)
            .expect("write must not IO-fail")
            .expect("substantive record must be written, not skipped")
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        let k = sample(
            Kind::Decision,
            Scope::Spec { spec: "demo".into() },
            "use markdown",
            "chose markdown over sqlite",
        );
        let path = write_ok(&store, &k);
        assert!(path.exists());
        // File landed under the legacy decisions dir (decision → memory/decisions).
        assert!(path.starts_with(dir.path().join("memory").join("decisions")));

        let back = store.read(&path).unwrap();
        assert_eq!(back, k, "store write→read must round-trip the record");
    }

    #[test]
    fn kind_scope_maps_onto_legacy_dirs() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        let cases: Vec<(Knowledge, PathBuf)> = vec![
            (
                sample(Kind::Summary, Scope::Wave { spec: "s".into(), wave: 1 }, "a", "b"),
                dir.path().join("memory").join("agent"),
            ),
            (
                sample(Kind::Decision, Scope::Global, "a", "b"),
                dir.path().join("memory").join("decisions"),
            ),
            (
                sample(Kind::Lesson, Scope::Global, "a", "b"),
                dir.path().join("memory").join("lessons"),
            ),
            (
                sample(Kind::Principle, Scope::Global, "a", "b"),
                dir.path().join("knowledge"),
            ),
            (
                sample(Kind::Reference, Scope::Spec { spec: "demo".into() }, "a", "b"),
                dir.path().join("spec").join("demo").join("memory"),
            ),
        ];
        for (k, want_dir) in cases {
            let p = write_ok(&store, &k);
            assert!(p.starts_with(&want_dir), "{:?} not under {:?}", p, want_dir);
        }
    }

    #[test]
    fn summary_write_carries_legacy_agent_aliases() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        let mut k = sample(
            Kind::Summary,
            Scope::Wave { spec: "demo".into(), wave: 1 },
            "delivered the thing",
            "details",
        );
        k.origin.session = Some("sess-1".into());
        let path = write_ok(&store, &k);
        let text = std::fs::read_to_string(&path).unwrap();
        // Legacy readers (memory search/promote) key on these, not on `label`.
        assert!(text.contains("summary: delivered the thing"), "{text}");
        assert!(text.contains("at: 2026-06-15T00:00:00.000Z"), "{text}");
        assert!(text.contains("last_used: 2026-06-15T00:00:00.000Z"), "{text}");
        assert!(text.contains("session_id: sess-1"), "{text}");
        // Canonical keys are still present → from_markdown round-trips.
        let back = store.read(&path).unwrap();
        assert_eq!(back, k);
    }

    #[test]
    fn global_principle_write_carries_name_and_description() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        let k = sample(Kind::Principle, Scope::Global, "fail-open", "hooks never abort");
        let path = write_ok(&store, &k);
        let text = std::fs::read_to_string(&path).unwrap();
        // `.claude/knowledge/` readers (session_start_inject) read name/description.
        assert!(text.contains("name: fail-open"), "{text}");
        assert!(text.contains("description: fail-open"), "{text}");
    }

    #[test]
    fn write_is_idempotent_same_path() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        let k = sample(Kind::Lesson, Scope::Global, "x", "y");
        let p1 = write_ok(&store, &k);
        let p2 = write_ok(&store, &k);
        assert_eq!(p1, p2, "same record → same content-addressed path");
        assert_eq!(store.read_all(None).len(), 1, "no duplicate file written");
    }

    #[test]
    fn read_all_is_empty_and_fail_open_on_missing_root() {
        let dir = tempdir().unwrap();
        // Point at a child that does not exist — must degrade to empty, no panic.
        let store = KnowledgeStore::new(dir.path().join("does-not-exist"));
        assert!(store.read_all(None).is_empty());
    }

    #[test]
    fn read_all_returns_every_written_record_deterministically() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        write_ok(&store, &sample(Kind::Decision, Scope::Global, "d", "dd"));
        write_ok(
            &store,
            &sample(
                Kind::Summary,
                Scope::Wave { spec: "s".into(), wave: 1 },
                "su",
                "sudd",
            ),
        );
        write_ok(
            &store,
            &sample(
                Kind::Principle,
                Scope::Spec { spec: "s".into() },
                "p",
                "pp",
            ),
        );

        let first = store.read_all(None);
        let second = store.read_all(None);
        assert_eq!(first.len(), 3);
        assert_eq!(
            first, second,
            "read_all order must be deterministic across calls"
        );
    }

    #[test]
    fn read_all_filters_by_scope_exactly() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        write_ok(&store, &sample(Kind::Lesson, Scope::Global, "g", "gg"));
        write_ok(
            &store,
            &sample(
                Kind::Summary,
                Scope::Spec { spec: "demo".into() },
                "s",
                "ss",
            ),
        );
        write_ok(
            &store,
            &sample(
                Kind::Summary,
                Scope::Wave { spec: "demo".into(), wave: 2 },
                "w",
                "ww",
            ),
        );

        let only_spec = store.read_all(Some(&Scope::Spec { spec: "demo".into() }));
        assert_eq!(only_spec.len(), 1, "exact-scope filter: Spec only");
        assert_eq!(only_spec[0].label, "s");

        let only_wave =
            store.read_all(Some(&Scope::Wave { spec: "demo".into(), wave: 2 }));
        assert_eq!(only_wave.len(), 1);
        assert_eq!(only_wave[0].label, "w");

        let only_global = store.read_all(Some(&Scope::Global));
        assert_eq!(only_global.len(), 1);
        assert_eq!(only_global[0].label, "g");
    }

    #[test]
    fn read_all_skips_unparseable_but_keeps_the_rest() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        write_ok(&store, &sample(Kind::Decision, Scope::Global, "ok", "body"));
        // Plant a junk .md with no frontmatter — it parses (defaults) rather
        // than panicking; and a non-md file that must be ignored entirely.
        let junk_dir = dir.path().join("memory").join("decisions");
        std::fs::write(junk_dir.join("not-markdown.txt"), b"ignored").unwrap();
        // A garbage .md still yields a (default) record — fail-open means
        // degrade, and a frontmatter-less file is valid input.
        let all = store.read_all(None);
        assert!(
            all.iter().any(|k| k.label == "ok"),
            "the good record survives"
        );
    }

    #[test]
    fn read_all_filters_out_non_substantive_legacy_files() {
        // A garbage record already on disk (written before the write-gate
        // existed, or by a legacy writer) must be hidden by the read gate too —
        // the same `is_substantive` criterion, applied on the way out.
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        write_ok(&store, &sample(Kind::Decision, Scope::Global, "real call", "chose markdown"));
        // Plant the exact sialia junk directly on disk (bypassing the write
        // gate): empty body + "interrupted mid-task" summary in the agent dir.
        let agent_dir = dir.path().join("memory").join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("junk.md"),
            b"---\nsummary: interrupted mid-task\nat: 2026-06-15T00:00:00.000Z\n---\n",
        )
        .unwrap();

        let all = store.read_all(None);
        assert_eq!(all.len(), 1, "the legacy junk is filtered out on read");
        assert_eq!(all[0].label, "real call");
    }

    #[test]
    fn read_all_reads_only_knowledge_dirs_not_the_whole_claude_tree() {
        // The bug: `read_all` used to walk the ENTIRE `.claude/` tree for `*.md`,
        // so config/docs (`CLAUDE.md`, `refs/`, `skills/`, …) leaked into recall
        // as if they were knowledge. The fix scopes reads to the store dirs only.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let store = KnowledgeStore::new(root);

        // A REAL record inside a knowledge dir — must appear.
        write_ok(
            &store,
            &sample(Kind::Decision, Scope::Global, "real decision", "chose markdown over sqlite"),
        );
        // A per-spec memory record (name-addressed store) — must appear too, so
        // cross-spec recall works.
        write_ok(
            &store,
            &sample(
                Kind::Reference,
                Scope::Spec { spec: "demo".into() },
                "spec ref",
                "a substantive per-spec note that recall should reach",
            ),
        );

        // Plant config/docs `*.md` with REAL bodies OUTSIDE every knowledge dir.
        // Each is substantive (so the is_substantive gate would NOT filter them);
        // only the directory scope keeps them out.
        let plant = |segs: &[&str], name: &str, body: &str| {
            let mut d = root.to_path_buf();
            for s in segs {
                d.push(s);
            }
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(name), body.as_bytes()).unwrap();
        };
        plant(&[], "CLAUDE.md", "---\nname: orchestrator\n---\nYou are the orchestrator. Coordinate pipelines and route intent across the project.");
        plant(&["refs"], "canonical-phases.md", "---\nname: phases\n---\nANALYZE PLAN EXECUTE REVIEW QA CLOSE are the canonical pipeline phases.");
        plant(&["skills", "feature"], "SKILL.md", "---\nname: feature\n---\nThe feature pipeline mines the repo into a deterministic grain model the agents consume.");
        plant(&["commands"], "guards.md", "---\nname: guards\n---\nGuards are do/don't lines grounded in the subproject conventions for each project.");
        plant(&["context"], "ctx.md", "---\nname: ctx\n---\nSubstantive context document body that is decidedly not project knowledge.");

        let all = store.read_all(None);
        let labels: Vec<&str> = all.iter().map(|k| k.label.as_str()).collect();
        // The two real knowledge records appear...
        assert!(labels.contains(&"real decision"), "in-store decision must appear: {labels:?}");
        assert!(labels.contains(&"spec ref"), "per-spec memory must appear (cross-spec recall): {labels:?}");
        // ...and NOTHING from config/docs dirs leaks in.
        assert_eq!(all.len(), 2, "only the two real records — no config/docs leak: {labels:?}");
        for label in &labels {
            assert!(
                !matches!(*label, "orchestrator" | "phases" | "feature" | "guards" | "ctx"),
                "config/docs leaked into read_all: {label}"
            );
        }
    }

    #[test]
    fn knowledge_dirs_is_the_fixed_four_plus_per_spec_memory() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Materialise two spec dirs so the one-level glob has something to find.
        std::fs::create_dir_all(root.join("spec").join("alpha").join("memory")).unwrap();
        std::fs::create_dir_all(root.join("spec").join("beta").join("memory")).unwrap();

        let dirs = knowledge_dirs(root);
        let rel: Vec<String> = dirs
            .iter()
            .map(|d| {
                d.strip_prefix(root)
                    .unwrap_or(d)
                    .components()
                    .filter_map(|c| c.as_os_str().to_str())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();
        // Fixed four first (declaration order), then per-spec memory sorted by spec.
        assert_eq!(
            rel,
            vec![
                "memory/agent",
                "memory/decisions",
                "memory/lessons",
                "knowledge",
                "spec/alpha/memory",
                "spec/beta/memory",
            ],
            "deterministic store-dir set"
        );
    }

    #[test]
    fn path_for_matches_write_destination() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        let k = sample(Kind::Reference, Scope::Global, "r", "rr");
        let predicted = store.path_for(&k);
        let written = write_ok(&store, &k);
        assert_eq!(predicted, written);
    }

    #[test]
    fn non_substantive_record_is_skipped_not_written() {
        let dir = tempdir().unwrap();
        let store = KnowledgeStore::new(dir.path());
        // The exact sialia case: empty body + "interrupted mid-task" summary.
        let junk = sample(
            Kind::Summary,
            Scope::Wave { spec: "demo".into(), wave: 1 },
            "interrupted mid-task",
            "",
        );
        let written = store.write(&junk).expect("write must not IO-fail");
        assert!(written.is_none(), "non-substantive record must return Ok(None)");
        assert!(
            store.read_all(None).is_empty(),
            "nothing must hit disk for a skipped record"
        );
        assert!(
            !store.path_for(&junk).exists(),
            "no file at the predicted path"
        );
    }
}
