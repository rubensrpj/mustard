//! `mustard-rt run spec-clear` — sweep terminal, idle spec directories (W5.T5.5).
//!
//! ## Why
//!
//! `.claude/spec/` is a flat list of every spec the project ever ran
//! (per `feedback_no_size_variants` + the W3 sidecar layout). With 55+ closed
//! specs in a mature monorepo, the spec list page accumulates noise:
//!
//! - Every spec whose `meta.json` reports `stage=close` + `outcome=completed`
//!   is "done" but still on disk.
//! - Each holds NDJSON events + (optionally) blob spills (W5.T5.1).
//! - The dashboard tailer + spec discovery have to walk them anyway.
//!
//! `spec-clear` finds those terminal specs whose **most recent event** is
//! older than `--age-days N` (default 15) and either reports or removes them.
//!
//! ## Algorithm
//!
//! 1. Glob `.claude/spec/*/spec.md`.
//! 2. For each, [`meta::read_meta_beside`] the sidecar (W3 guaranteed every
//!    spec has one).
//! 3. Skip when `stage != Close` OR `outcome != Completed` — preserves active
//!    or follow-up work.
//! 4. Walk the spec dir recursively, find the most recent mtime under
//!    `events/` (any depth — covers wave-N subdirs).
//! 5. Compare against the cutoff (`now - age_days * 86400s`).
//! 6. In `--dry-run` (default): emit one table line per match.
//!    In `--apply`: `fs::remove_dir_all` the spec directory and emit an event.
//!
//! ## Flags
//!
//! - `--dry-run` (default) — preview only, no filesystem mutation.
//! - `--apply` — perform the deletion. Required to mutate.
//! - `--all` — operate on every terminal spec regardless of age.
//! - `--name <slug>` — restrict to one spec.
//! - `--age-days <N>` — override the 15-day default.
//!
//! ## Output
//!
//! Byte-stable pretty JSON:
//!
//! ```json
//! {
//!   "candidates": [
//!     { "spec": "auth", "age_days": 30, "last_event_at": "...", "action": "remove" }
//!   ],
//!   "removed":  ["auth", ...],
//!   "kept":     [{ "spec": "billing", "age_days": 4, "reason": "below threshold" }],
//!   "skipped":  [{ "spec": "still-running", "reason": "active stage" }],
//!   "errors":   [{ "spec": "...", "error": "..." }]
//! }
//! ```
//!
//! Fail-open: a single bad spec degrades to an `errors[]` entry; the sweep
//! continues.

use crate::run::env::project_dir;
use crate::util::now_iso8601;
use mustard_core::fs;
use mustard_core::meta::read_meta_beside;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Default sweep horizon (15 days) — older terminal specs become candidates.
pub const DEFAULT_AGE_DAYS: u32 = 15;

/// Options for `mustard-rt run spec-clear`.
pub struct SpecClearOpts {
    /// Project root override. Defaults to the current working directory.
    pub repo: Option<PathBuf>,
    /// Age threshold in whole days; specs idle longer than this are eligible.
    pub age_days: u32,
    /// When `true`, `fs::remove_dir_all` runs. Default is preview.
    pub apply: bool,
    /// When `true`, ignore the age threshold (sweep every terminal spec).
    pub all: bool,
    /// When `Some`, restrict to a single spec slug.
    pub name: Option<String>,
}

#[derive(Serialize)]
struct Candidate {
    spec: String,
    age_days: Option<u64>,
    last_event_at: Option<String>,
    /// `"remove"` (in `--apply`) or `"would-remove"` (dry-run).
    action: &'static str,
}

#[derive(Serialize)]
struct KeptEntry {
    spec: String,
    age_days: Option<u64>,
    reason: &'static str,
}

#[derive(Serialize)]
struct SkippedEntry {
    spec: String,
    reason: &'static str,
}

#[derive(Serialize)]
struct ErrorEntry {
    spec: String,
    error: String,
}

#[derive(Serialize)]
struct Report {
    candidates: Vec<Candidate>,
    removed: Vec<String>,
    kept: Vec<KeptEntry>,
    skipped: Vec<SkippedEntry>,
    errors: Vec<ErrorEntry>,
    dry_run: bool,
    age_days: u32,
}

/// Entry point — invoked from `RunCmd::SpecClear` dispatch.
pub fn run(opts: SpecClearOpts) {
    let repo = opts
        .repo
        .clone()
        .unwrap_or_else(|| PathBuf::from(project_dir()));
    let report = collect_and_act(&repo, &opts);

    // Best-effort emit a summary event so `/economia` sees the operation.
    emit_summary(&repo, &report);

    let out = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{out}");
}

/// Walk `.claude/spec/*/spec.md`, classify each into a report bucket, and
/// (optionally) remove the candidates.
fn collect_and_act(repo: &Path, opts: &SpecClearOpts) -> Report {
    let Ok(cp) = ClaudePaths::for_project(repo) else {
        return Report {
            candidates: Vec::new(),
            removed: Vec::new(),
            kept: Vec::new(),
            skipped: Vec::new(),
            errors: Vec::new(),
            dry_run: !opts.apply,
            age_days: opts.age_days,
        };
    };
    let spec_root = cp.spec_dir();
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0u128, |d| d.as_millis()) as i64;
    let cutoff_ms = now_ms - i64::from(opts.age_days) * 86_400_000;

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut kept: Vec<KeptEntry> = Vec::new();
    let mut skipped: Vec<SkippedEntry> = Vec::new();
    let mut errors: Vec<ErrorEntry> = Vec::new();

    let Ok(entries) = fs::read_dir(&spec_root) else {
        // Fail-open: a missing spec root yields an empty report.
        return Report {
            candidates,
            removed,
            kept,
            skipped,
            errors,
            dry_run: !opts.apply,
            age_days: opts.age_days,
        };
    };

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let path = entry.path.clone();
        let slug = entry.file_name.clone();

        // --name restriction.
        if let Some(want) = opts.name.as_deref() {
            if want != slug {
                continue;
            }
        }

        let spec_md = path.join("spec.md");
        if !spec_md.exists() {
            skipped.push(SkippedEntry {
                spec: slug.clone(),
                reason: "no spec.md",
            });
            continue;
        }

        // W3 guarantees a meta.json sidecar.
        let Some(meta) = read_meta_beside(&spec_md) else {
            skipped.push(SkippedEntry {
                spec: slug.clone(),
                reason: "missing meta.json",
            });
            continue;
        };

        let stage = meta.stage.as_deref().unwrap_or("").to_ascii_lowercase();
        let outcome = meta.outcome.as_deref().unwrap_or("").to_ascii_lowercase();
        if stage != "close" || outcome != "completed" {
            skipped.push(SkippedEntry {
                spec: slug.clone(),
                reason: "not terminal (stage!=close or outcome!=completed)",
            });
            continue;
        }

        // Find most-recent mtime under any `events/` subdir (recursive).
        let last_mtime_ms = walk_for_last_event_mtime(&path);
        let age_days = last_mtime_ms.map(|ms| {
            let age_ms = (now_ms - ms).max(0);
            (age_ms / 86_400_000) as u64
        });

        let eligible_by_age = match last_mtime_ms {
            Some(ms) => opts.all || ms <= cutoff_ms,
            None => opts.all, // No events found — only eligible with --all.
        };

        if !eligible_by_age {
            kept.push(KeptEntry {
                spec: slug.clone(),
                age_days,
                reason: "below threshold",
            });
            continue;
        }

        let last_event_at = last_mtime_ms.map(iso_from_epoch_ms);
        candidates.push(Candidate {
            spec: slug.clone(),
            age_days,
            last_event_at,
            action: if opts.apply { "remove" } else { "would-remove" },
        });

        if opts.apply {
            match fs::remove_dir_all(&path) {
                Ok(()) => removed.push(slug.clone()),
                Err(e) => errors.push(ErrorEntry {
                    spec: slug.clone(),
                    error: format!("remove failed: {e}"),
                }),
            }
        }
    }

    Report {
        candidates,
        removed,
        kept,
        skipped,
        errors,
        dry_run: !opts.apply,
        age_days: opts.age_days,
    }
}

/// Walk `dir` recursively, returning the most-recent mtime (epoch ms) of any
/// regular file under any `events/` subdirectory at any depth. Returns `None`
/// when no event file is found.
fn walk_for_last_event_mtime(dir: &Path) -> Option<i64> {
    let mut latest: Option<i64> = None;
    walk_inner(dir, false, &mut latest);
    latest
}

/// `inside_events` tracks whether we've already entered an `events/` subdir;
/// once true, every regular file is a candidate.
fn walk_inner(dir: &Path, inside_events: bool, latest: &mut Option<i64>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.path.clone();
        let name = &entry.file_name;
        if entry.is_dir {
            let now_inside = inside_events || name == "events";
            walk_inner(&path, now_inside, latest);
        } else if inside_events {
            if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(m) = meta.modified() {
                    if let Ok(dur) = m.duration_since(SystemTime::UNIX_EPOCH) {
                        let ms = dur.as_millis() as i64;
                        if latest.is_none_or(|cur| ms > cur) {
                            *latest = Some(ms);
                        }
                    }
                }
            }
        }
    }
}

/// Render epoch ms to ISO-8601 (`YYYY-MM-DDThh:mm:ss.sssZ`). Hand-rolled to
/// stay dependency-free (matches `util::now_iso8601`).
fn iso_from_epoch_ms(ms: i64) -> String {
    let secs = (ms / 1000).max(0) as u64;
    let millis = (ms % 1000).max(0) as u32;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

/// Best-effort emit a `spec.clear.run` summary event into the harness store.
/// Fail-open — the report is the user-facing artifact.
fn emit_summary(repo: &Path, report: &Report) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: String::new(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("spec-clear".to_string()),
            actor_type: None,
        },
        event: "spec.clear.run".to_string(),
        payload: json!({
            "candidates": report.candidates.len(),
            "removed": report.removed.len(),
            "kept": report.kept.len(),
            "skipped": report.skipped.len(),
            "errors": report.errors.len(),
            "dry_run": report.dry_run,
            "age_days": report.age_days,
        }),
        spec: None,
    };
    // `spec.clear.run` is non-pipeline → NDJSON via the W5 router (lands in
    // the session-fallback dir since the event carries no spec attribution).
    let _ = crate::run::event_route::emit(repo.to_string_lossy().as_ref(), &event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::meta::{write_meta, Meta};
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_spec(repo: &Path, slug: &str, stage: &str, outcome: &str) {
        let dir = repo.join(".claude").join("spec").join(slug);
        fs::create_dir_all(&dir).unwrap();
        let mut f = File::create(dir.join("spec.md")).unwrap();
        writeln!(f, "# {slug}").unwrap();
        let meta = Meta::new(Some(stage), Some(outcome), None, None, None, None, None);
        write_meta(&dir.join("meta.json"), &meta).unwrap();
    }

    fn write_event(repo: &Path, slug: &str, file: &str) {
        let dir = repo.join(".claude").join("spec").join(slug).join(".events");
        fs::create_dir_all(&dir).unwrap();
        let mut f = File::create(dir.join(file)).unwrap();
        writeln!(f, r#"{{"event":"x"}}"#).unwrap();
    }

    #[test]
    fn skips_active_specs() {
        let dir = tempdir().unwrap();
        write_spec(dir.path(), "active-one", "Execute", "Active");
        write_event(dir.path(), "active-one", "fresh.ndjson");

        let report = collect_and_act(
            dir.path(),
            &SpecClearOpts {
                repo: None,
                age_days: 0,
                apply: true,
                all: true,
                name: None,
            },
        );
        assert!(report.removed.is_empty(), "active spec must not be removed");
        assert!(report.skipped.iter().any(|s| s.spec == "active-one"));
        // Original spec dir should still exist.
        assert!(dir
            .path()
            .join(".claude/spec/active-one")
            .exists());
    }

    #[test]
    fn dry_run_does_not_mutate() {
        let dir = tempdir().unwrap();
        write_spec(dir.path(), "old-one", "Close", "Completed");
        write_event(dir.path(), "old-one", "ancient.ndjson");

        let report = collect_and_act(
            dir.path(),
            &SpecClearOpts {
                repo: None,
                age_days: 0,
                apply: false,
                all: true,
                name: None,
            },
        );
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.candidates[0].action, "would-remove");
        assert!(report.removed.is_empty());
        assert!(dir.path().join(".claude/spec/old-one").exists());
    }

    #[test]
    fn apply_removes_terminal_idle_specs() {
        let dir = tempdir().unwrap();
        write_spec(dir.path(), "done-one", "Close", "Completed");
        write_event(dir.path(), "done-one", "stale.ndjson");

        let report = collect_and_act(
            dir.path(),
            &SpecClearOpts {
                repo: None,
                age_days: 0,
                apply: true,
                all: true,
                name: None,
            },
        );
        assert_eq!(report.removed, vec!["done-one".to_string()]);
        assert!(!dir.path().join(".claude/spec/done-one").exists());
    }

    #[test]
    fn name_restricts_to_one_spec() {
        let dir = tempdir().unwrap();
        write_spec(dir.path(), "a", "Close", "Completed");
        write_spec(dir.path(), "b", "Close", "Completed");
        write_event(dir.path(), "a", "a.ndjson");
        write_event(dir.path(), "b", "b.ndjson");

        let report = collect_and_act(
            dir.path(),
            &SpecClearOpts {
                repo: None,
                age_days: 0,
                apply: true,
                all: true,
                name: Some("a".to_string()),
            },
        );
        assert_eq!(report.removed, vec!["a".to_string()]);
        assert!(dir.path().join(".claude/spec/b").exists());
    }

    #[test]
    fn missing_meta_is_skipped_not_removed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/spec/no-meta");
        fs::create_dir_all(&path).unwrap();
        File::create(path.join("spec.md")).unwrap();

        let report = collect_and_act(
            dir.path(),
            &SpecClearOpts {
                repo: None,
                age_days: 0,
                apply: true,
                all: true,
                name: None,
            },
        );
        assert!(report.removed.is_empty());
        assert!(report.skipped.iter().any(|s| s.spec == "no-meta"));
        assert!(path.exists());
    }
}
