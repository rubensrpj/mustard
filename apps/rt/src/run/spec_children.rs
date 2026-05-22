//! `mustard-rt run spec-children --parent <slug>` — UNION of sub-specs
//! discovered via `spec.link` events (SQLite) and via filesystem header
//! parsing (`### Parent: <slug>`).
//!
//! Why a UNION
//! -----------
//!
//! The dashboard's `spec_children_v2` Tauri command resolves sub-specs by
//! calling [`mustard_core::SqliteSpecReader::children_of`], which only sees
//! `spec.link` events in the local SQLite store. A sub-spec created via
//! `/mustard:tactical-fix` that declares `### Parent: <slug>` in its header
//! but whose `spec.link` event never landed in *this* developer's store
//! (typical after a `git pull` from a teammate) becomes invisible. The
//! filesystem header is the cross-developer canon — versioned in git, durable
//! across `pull`. SQLite remains the richest data source (timestamps, reason).
//!
//! Resolution policy: when both sources agree on a child slug, SQLite wins
//! (status / timestamps / reason are preserved); the entry is annotated
//! `source = Both`. Header-only entries default to `status = "unknown"`
//! unless the spec header itself declares `### Status: <X>`. Event-only
//! entries are reported as before with `source = Event`. Output is sorted
//! by slug for byte-stability.
//!
//! Fail-open: any I/O or SQLite failure is silently downgraded to an empty
//! contribution from that side. The subcommand always emits valid JSON.

use mustard_core::fs;
use mustard_core::spec;
use mustard_core::{SpecChild, SpecReader, SpecStatus, SqliteSpecReader, WaveView};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Which source identified this child.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChildSource {
    /// Discovered only via a `spec.link` event in the SQLite store.
    Event,
    /// Discovered only via `### Parent:` in the child's `spec.md` header.
    Header,
    /// Discovered via both sources.
    Both,
}

/// One sub-spec linked to a parent — UNION row used by the dashboard.
#[derive(Serialize, Debug, Clone)]
pub struct ChildEntry {
    /// Child spec slug (the directory name under `.claude/spec/`).
    pub spec: String,
    /// Lifecycle status in kebab-case (matches the on-disk `### Status:`
    /// spelling, e.g. `planning`, `implementing`, `completed`). Falls back
    /// to `"unknown"` for header-only entries whose header has no status.
    pub status: String,
    /// First-event timestamp (ISO-8601), when known from the SQLite side.
    pub started_at: Option<String>,
    /// Terminal-event timestamp (ISO-8601), when known from the SQLite side.
    pub completed_at: Option<String>,
    /// Free-form `spec.link` payload reason (e.g. `"tactical-fix"`), when
    /// known from the SQLite side.
    pub reason: Option<String>,
    /// Which source(s) produced this entry.
    pub source: ChildSource,
    /// Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs-polish`):
    /// the wave of the parent whose execution window contains this child's
    /// `started_at`. `None` when the child has no `started_at` (header-only)
    /// or its start does not fall inside any wave range. The dashboard
    /// renders sub-specs nested under their owning wave row; rows with
    /// `wave == None` go to a "Sem onda correlacionada" bucket.
    pub wave: Option<u32>,
}

/// Render a [`SpecStatus`] as its kebab-case spelling — delegates to the
/// canonical [`SpecStatus::as_kebab`] (single source of truth).
fn spec_status_to_kebab(status: SpecStatus) -> String {
    status.as_kebab().to_string()
}

/// Convert a [`SpecChild`] (SQLite side) into our UNION row, tagged `Event`
/// by default. The `source` may be promoted to `Both` later if filesystem
/// scan also matches. `wave` is filled later by [`correlate_waves`].
fn child_from_event(child: &SpecChild) -> ChildEntry {
    ChildEntry {
        spec: child.spec.clone(),
        status: spec_status_to_kebab(child.status),
        started_at: child.started_at.clone(),
        completed_at: child.completed_at.clone(),
        reason: child.reason.clone(),
        source: ChildSource::Event,
        wave: None,
    }
}

/// Strip surrounding `[[ ]]` from a wikilink target. Leaves any other text
/// untouched. Whitespace inside the brackets is trimmed.
fn strip_wikilink(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")) {
        inner.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Parse the `### Parent:` link and the lifecycle status out of a `spec.md`'s
/// leading window. Returns `(parent_slug, status_kebab_opt)` when a parent
/// header is found, else `None`.
///
/// The parent slug is normalised (surrounding `[[wikilink]]` brackets are
/// stripped). The status is resolved through the canonical
/// [`mustard_core::spec`] parser — so the new `### Stage:`/`### Outcome:`
/// header *and* every legacy `### Status:` shape are understood — and projected
/// to the kebab-case status word the dashboard's sub-spec rows expect. A spec
/// with a `### Parent:` but no lifecycle header surfaces `status = None`
/// (callers default it to `"unknown"`).
fn parse_header_window(window: &str) -> Option<(String, Option<String>)> {
    // `### Parent:` is not part of the lifecycle-header domain — read it via the
    // shared header-region-scoped accessor so prose mentions never match.
    let parent = spec::header_field(window, "Parent")
        .map(|raw| strip_wikilink(&raw))
        .filter(|s| !s.is_empty())?;
    // Lifecycle status: canonical parse → projected status word.
    let status = spec::parse_state(window).map(|st| spec::status_word(&st).to_string());
    Some((parent, status))
}

/// Read at most the first `cap` bytes of a file as UTF-8 (lossy on invalid
/// sequences). Returns `None` on any I/O failure.
fn read_header_window(path: &Path, cap: usize) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; cap];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Scan `<project>/.claude/spec/*/spec.md`, returning every child slug whose
/// header declares `### Parent: <parent>` (raw or wikilinked). The returned
/// [`ChildEntry`] rows are tagged [`ChildSource::Header`] and carry the
/// status read from the header when present (else `"unknown"`).
///
/// Fail-open: missing `.claude/spec/` directory yields an empty result; per-
/// file I/O errors are silently skipped.
fn scan_filesystem(project: &Path, parent: &str) -> Vec<ChildEntry> {
    let spec_root = project.join(".claude").join("spec");
    let Ok(entries) = fs::read_dir(&spec_root) else {
        return Vec::new();
    };
    let mut out: Vec<ChildEntry> = Vec::new();
    // Cap at 4 KiB — header section is always near the top of a spec.md.
    const HEADER_CAP: usize = 4096;
    for entry in entries {
        let dir_path = &entry.path;
        if !entry.is_dir {
            continue;
        }
        let spec_md = dir_path.join("spec.md");
        if !spec_md.is_file() {
            continue;
        }
        let Some(window) = read_header_window(&spec_md, HEADER_CAP) else {
            continue;
        };
        let Some((found_parent, status_opt)) = parse_header_window(&window) else {
            continue;
        };
        if found_parent != parent {
            continue;
        }
        let slug = entry.file_name.clone();
        out.push(ChildEntry {
            spec: slug,
            status: status_opt.unwrap_or_else(|| "unknown".to_string()),
            started_at: None,
            completed_at: None,
            reason: None,
            source: ChildSource::Header,
            wave: None,
        });
    }
    out
}

/// Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs-polish`) —
/// correlate every child's `started_at` against the parent's wave timeline.
///
/// Reads `pipeline.wave.complete` (and the implicit `pipeline.task.dispatch`
/// for `started_at`) via [`SqliteSpecReader::waves`]. A child is attributed
/// to wave `N` when its `started_at` falls inside `[wave.started_at,
/// wave.completed_at]`. Children without a `started_at` (header-only rows)
/// keep `wave = None`; children whose `started_at` precedes the first wave
/// or follows the last wave's completion keep `wave = None` too (callers
/// render those in the "Sem onda" bucket).
///
/// Fail-open: a missing event store or a wave row without a usable timestamp
/// silently degrades to "no correlation" for that child.
fn correlate_waves(project: &Path, parent: &str, entries: &mut [ChildEntry]) {
    // Build `wave -> (start_ms, end_ms)` from the parent's wave timeline.
    let Ok(reader) = SqliteSpecReader::for_project(project) else {
        return;
    };
    let waves: Vec<WaveView> = reader.waves(parent).unwrap_or_default();
    if waves.is_empty() {
        return;
    }
    let mut ranges: Vec<(u32, i64, i64)> = waves
        .iter()
        .filter_map(|w| {
            let start = w.started_at.as_deref().and_then(parse_iso_ms)?;
            // Open-ended (in-progress) waves: treat `completed_at` as i64::MAX
            // so any child started after `start` is attributed to the wave.
            // This is the desired UX during EXECUTE — a tactical-fix created
            // inside the running wave shouldn't fall into the "no correlation"
            // bucket just because the wave hasn't closed yet.
            let end = w
                .completed_at
                .as_deref()
                .and_then(parse_iso_ms)
                .unwrap_or(i64::MAX);
            Some((w.wave, start, end))
        })
        .collect();
    if ranges.is_empty() {
        return;
    }
    ranges.sort_by_key(|t| t.1);

    for entry in entries.iter_mut() {
        let Some(start_iso) = entry.started_at.as_deref() else {
            continue;
        };
        let Some(child_start_ms) = parse_iso_ms(start_iso) else {
            continue;
        };
        // Walk ranges in chronological order; first hit wins.
        for (wave, start_ms, end_ms) in &ranges {
            if child_start_ms >= *start_ms && child_start_ms <= *end_ms {
                entry.wave = Some(*wave);
                break;
            }
        }
    }
}

/// Parse an ISO-8601 UTC timestamp into a milliseconds-since-epoch i64. Fail-
/// open: returns `None` for any unparseable input. Delegates to the shared
/// helper in `complete_spec` so the parser stays in one place.
fn parse_iso_ms(ts: &str) -> Option<i64> {
    crate::run::complete_spec::parse_iso_millis(ts)
}

/// Compute the UNION of sub-specs for `parent`.
///
/// 1. **Set A (events):** [`SqliteSpecReader::children_of`] — exact same call
///    `spec_children_v2` made historically. Fail-open: a missing or locked
///    store contributes an empty Set A.
/// 2. **Set B (headers):** filesystem scan of
///    `<project>/.claude/spec/*/spec.md` for `### Parent: <parent>`.
/// 3. **Merge:** Set A entries keep their richer SQLite metadata; when a
///    Set A slug also appears in Set B, the entry is promoted to
///    [`ChildSource::Both`]. Set B entries that have no Set A counterpart
///    are appended with [`ChildSource::Header`].
///
/// Output is sorted by slug ascending for byte-stability.
#[must_use]
pub fn list_children(project: &Path, parent: &str) -> Vec<ChildEntry> {
    if parent.is_empty() {
        return Vec::new();
    }

    // -- Set A: events -------------------------------------------------------
    let mut by_slug: HashMap<String, ChildEntry> = HashMap::new();
    if let Ok(reader) = SqliteSpecReader::for_project(project) {
        if let Ok(children) = reader.children_of(parent) {
            for child in &children {
                let entry = child_from_event(child);
                by_slug.insert(entry.spec.clone(), entry);
            }
        }
    }

    // -- Set B: filesystem headers ------------------------------------------
    let header_entries = scan_filesystem(project, parent);
    for header_entry in header_entries {
        if let Some(existing) = by_slug.get_mut(&header_entry.spec) {
            // Both sources agree — keep SQLite data, promote the tag.
            existing.source = ChildSource::Both;
        } else {
            by_slug.insert(header_entry.spec.clone(), header_entry);
        }
    }

    let mut out: Vec<ChildEntry> = by_slug.into_values().collect();
    out.sort_by(|a, b| a.spec.cmp(&b.spec));

    // Wave 2 (spec 2026-05-21-dashboard-spec-tabs-polish): attribute each
    // child to a wave of the parent. Failures here are silently absorbed —
    // every row keeps `wave = None` and the dashboard renders the "Sem
    // onda" bucket.
    correlate_waves(project, parent, &mut out);

    out
}

/// Dispatch `mustard-rt run spec-children --parent <slug>`. Emits the
/// resulting `Vec<ChildEntry>` as JSON to stdout. Fail-open: any error path
/// degrades to `[]` and exit `0`.
pub fn run(parent: Option<&str>) {
    let Some(parent) = parent else {
        eprintln!("Usage: mustard-rt run spec-children --parent <slug>");
        println!("[]");
        return;
    };
    let project = PathBuf::from(crate::run::env::project_dir());
    let entries = list_children(&project, parent);
    match serde_json::to_string(&entries) {
        Ok(text) => println!("{text}"),
        Err(_) => println!("[]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_spec(project: &Path, slug: &str, body: &str) {
        let spec_dir = project.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), body).unwrap();
    }

    #[test]
    fn union_returns_header_only_when_no_event() {
        let td = tempdir().unwrap();
        write_spec(
            td.path(),
            "child-a",
            "# Child A\n\n### Parent: parent-x\n### Status: completed\n",
        );
        let result = list_children(td.path(), "parent-x");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].spec, "child-a");
        assert_eq!(result[0].source, ChildSource::Header);
        assert_eq!(result[0].status, "completed");
        assert!(result[0].started_at.is_none());
        assert!(result[0].completed_at.is_none());
        assert!(result[0].reason.is_none());
        // Header-only rows have no `started_at`, so wave correlation
        // is impossible — they always land in the "no wave" bucket.
        assert!(result[0].wave.is_none());
    }

    #[test]
    fn union_skips_unrelated_parents() {
        let td = tempdir().unwrap();
        write_spec(
            td.path(),
            "child-other",
            "# Child Other\n\n### Parent: parent-y\n",
        );
        let result = list_children(td.path(), "parent-x");
        assert!(result.is_empty(), "expected no entries for parent-x");
    }

    #[test]
    fn union_accepts_wikilinked_parent() {
        let td = tempdir().unwrap();
        write_spec(
            td.path(),
            "child-b",
            "# Child B\n\n### Parent: [[parent-x]]\n### Status: planning\n",
        );
        let result = list_children(td.path(), "parent-x");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].spec, "child-b");
        assert_eq!(result[0].source, ChildSource::Header);
        assert_eq!(result[0].status, "planning");
    }

    #[test]
    fn union_defaults_status_to_unknown_when_header_missing_status() {
        let td = tempdir().unwrap();
        write_spec(
            td.path(),
            "child-c",
            "# Child C\n\n### Parent: parent-x\n",
        );
        let result = list_children(td.path(), "parent-x");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "unknown");
    }

    #[test]
    fn union_sorts_entries_by_slug() {
        let td = tempdir().unwrap();
        write_spec(td.path(), "z-child", "### Parent: p\n");
        write_spec(td.path(), "a-child", "### Parent: p\n");
        write_spec(td.path(), "m-child", "### Parent: p\n");
        let result = list_children(td.path(), "p");
        let slugs: Vec<&str> = result.iter().map(|e| e.spec.as_str()).collect();
        assert_eq!(slugs, vec!["a-child", "m-child", "z-child"]);
    }

    #[test]
    fn union_empty_parent_returns_empty_vec() {
        let td = tempdir().unwrap();
        write_spec(td.path(), "child", "### Parent: anything\n");
        let result = list_children(td.path(), "");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_header_window_strips_wikilink() {
        let window = "# Title\n\n### Parent: [[my-parent]]\n### Status: draft\n";
        let parsed = parse_header_window(window).expect("should parse");
        assert_eq!(parsed.0, "my-parent");
        // `draft` maps to `planning` via `SpecStatus::parse`.
        assert_eq!(parsed.1.as_deref(), Some("planning"));
    }

    #[test]
    fn parse_header_window_unknown_status_degrades_to_none() {
        // An unrecognised lifecycle token yields no canonical state, so the
        // status is `None` (callers default it to `"unknown"`). The canonical
        // parser does not preserve arbitrary free-text status words — the
        // status vocabulary is the closed Stage/Outcome/Flags set.
        let window = "### Parent: p\n### Status: weird-status\n";
        let parsed = parse_header_window(window).expect("should parse");
        assert_eq!(parsed.0, "p");
        assert_eq!(parsed.1, None);
    }

    #[test]
    fn parse_header_window_returns_none_without_parent() {
        let window = "# Top-level spec\n\n### Status: planning\n";
        assert!(parse_header_window(window).is_none());
    }

    // -----------------------------------------------------------------------
    // Wave 2 — sub-spec correlation against parent wave windows.
    //
    // Seeds the parent's `pipeline.task.dispatch` (provides `started_at`) +
    // `pipeline.wave.complete` (closes the window) events, then directly
    // populates the children's `started_at` to drive `correlate_waves`.
    // We bypass `children_of` (which requires a `spec.link` event) by feeding
    // entries directly to `correlate_waves` — that's the unit we care about.
    // -----------------------------------------------------------------------

    use mustard_core::model::event::{
        Actor, ActorKind, EVENT_PIPELINE_TASK_DISPATCH, EVENT_PIPELINE_WAVE_COMPLETE,
        HarnessEvent, SCHEMA_VERSION,
    };
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use serde_json::json;

    fn seed_event(
        store: &SqliteEventStore,
        spec: &str,
        ts: &str,
        kind: &str,
        payload: serde_json::Value,
    ) {
        let ev = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.into(),
            session_id: "s-test".into(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("test".into()),
                actor_type: None,
            },
            event: kind.into(),
            payload,
            spec: Some(spec.into()),
        };
        store.append(&ev).unwrap();
    }

    /// Build the store at the canonical path under the project dir so
    /// `SqliteSpecReader::for_project` and `correlate_waves` both find it.
    /// `for_project` resolves to `{project}/.claude/.harness/mustard.db` and
    /// auto-creates the parent directories on open.
    fn open_store_for(project: &Path) -> SqliteEventStore {
        SqliteEventStore::for_project(project).unwrap()
    }

    #[test]
    fn correlate_waves_attributes_child_to_matching_wave_range() {
        let td = tempdir().unwrap();
        let parent = "parent-x";
        let store = open_store_for(td.path());

        // Wave 1: 10:00 → 10:05.
        seed_event(
            &store,
            parent,
            "2026-05-21T10:00:00.000Z",
            EVENT_PIPELINE_TASK_DISPATCH,
            json!({ "wave": 1, "name": "w1", "role": "impl" }),
        );
        seed_event(
            &store,
            parent,
            "2026-05-21T10:05:00.000Z",
            EVENT_PIPELINE_WAVE_COMPLETE,
            json!({ "wave": 1 }),
        );
        // Wave 2: 10:10 → 10:20.
        seed_event(
            &store,
            parent,
            "2026-05-21T10:10:00.000Z",
            EVENT_PIPELINE_TASK_DISPATCH,
            json!({ "wave": 2, "name": "w2", "role": "impl" }),
        );
        seed_event(
            &store,
            parent,
            "2026-05-21T10:20:00.000Z",
            EVENT_PIPELINE_WAVE_COMPLETE,
            json!({ "wave": 2 }),
        );

        let mut entries = vec![
            // Child A started at 10:03 — falls inside wave 1.
            ChildEntry {
                spec: "child-in-w1".into(),
                status: "completed".into(),
                started_at: Some("2026-05-21T10:03:00.000Z".into()),
                completed_at: None,
                reason: None,
                source: ChildSource::Event,
                wave: None,
            },
            // Child B started at 10:15 — falls inside wave 2.
            ChildEntry {
                spec: "child-in-w2".into(),
                status: "completed".into(),
                started_at: Some("2026-05-21T10:15:00.000Z".into()),
                completed_at: None,
                reason: None,
                source: ChildSource::Event,
                wave: None,
            },
            // Child C started at 11:00 — outside all wave windows.
            ChildEntry {
                spec: "child-orphan".into(),
                status: "completed".into(),
                started_at: Some("2026-05-21T11:00:00.000Z".into()),
                completed_at: None,
                reason: None,
                source: ChildSource::Event,
                wave: None,
            },
        ];
        correlate_waves(td.path(), parent, &mut entries);

        let by_slug: HashMap<&str, Option<u32>> = entries
            .iter()
            .map(|e| (e.spec.as_str(), e.wave))
            .collect();
        assert_eq!(by_slug["child-in-w1"], Some(1));
        assert_eq!(by_slug["child-in-w2"], Some(2));
        assert_eq!(by_slug["child-orphan"], None);
    }

    #[test]
    fn correlate_waves_open_range_attributes_late_child_to_in_progress_wave() {
        // A wave with `pipeline.task.dispatch` but no `wave.complete` is
        // in-progress; its window stays open (`end = i64::MAX`) so any child
        // started after dispatch attributes to it.
        let td = tempdir().unwrap();
        let parent = "parent-y";
        let store = open_store_for(td.path());
        seed_event(
            &store,
            parent,
            "2026-05-21T10:00:00.000Z",
            EVENT_PIPELINE_TASK_DISPATCH,
            json!({ "wave": 3, "name": "ongoing", "role": "impl" }),
        );

        let mut entries = vec![ChildEntry {
            spec: "tactical-fix".into(),
            status: "planning".into(),
            started_at: Some("2026-05-21T10:30:00.000Z".into()),
            completed_at: None,
            reason: None,
            source: ChildSource::Event,
            wave: None,
        }];
        correlate_waves(td.path(), parent, &mut entries);
        assert_eq!(entries[0].wave, Some(3));
    }

    #[test]
    fn correlate_waves_noop_without_started_at() {
        // Header-only rows have `started_at = None` — correlation must leave
        // their `wave` as `None`.
        let td = tempdir().unwrap();
        let parent = "parent-z";
        let store = open_store_for(td.path());
        seed_event(
            &store,
            parent,
            "2026-05-21T10:00:00.000Z",
            EVENT_PIPELINE_TASK_DISPATCH,
            json!({ "wave": 1, "name": "w1" }),
        );
        seed_event(
            &store,
            parent,
            "2026-05-21T10:05:00.000Z",
            EVENT_PIPELINE_WAVE_COMPLETE,
            json!({ "wave": 1 }),
        );

        let mut entries = vec![ChildEntry {
            spec: "header-only".into(),
            status: "unknown".into(),
            started_at: None,
            completed_at: None,
            reason: None,
            source: ChildSource::Header,
            wave: None,
        }];
        correlate_waves(td.path(), parent, &mut entries);
        assert!(entries[0].wave.is_none());
    }
}
