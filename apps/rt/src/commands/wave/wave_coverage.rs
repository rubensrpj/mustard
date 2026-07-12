//! `wave_coverage` — deterministic execution-coverage guard for one wave.
//!
//! A wave declares the files it will deliver in its `## Files` section, seeded
//! per wave into `meta.json#checklist` (one `{label, path, done:false}` item per
//! file by `wave-scaffold`, see [`super::wave_scaffold`]). The
//! `PostToolUse(Write|Edit)` hook flips an item to `done:true` the moment its
//! file is edited (`post_edit.rs`). So a promised file that was never touched
//! stays `done:false`.
//!
//! [`check`] reads that same sidecar and reports the promised-but-untouched
//! files. It is the deterministic signal behind the wave-completion gate in
//! `wave_complete_observer`: a wave that left promised files untouched is not
//! truly done, regardless of what the agent *claimed* in its report.
//!
//! Mode ([`mode`], env `MUSTARD_WAVE_COVERAGE_MODE`): `strict` (default) |
//! `warn` | `off`. The guard ENFORCES by default: `strict` blocks the
//! `pipeline.wave.complete` emission so the wave reopens on the next
//! `wave-advance` instead of shipping a half-vertical to QA. The env var is
//! only an escape hatch to relax — `warn` surfaces the gap but lets completion
//! proceed, `off` disables the guard — never the primary interface.

use std::path::Path;

use mustard_core::platform::config::Mode;
use mustard_core::read_meta;

/// The coverage verdict for one wave: whether every promised file was touched,
/// plus the paths of any that were not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageVerdict {
    /// `true` when no promised file is left untouched.
    pub ok: bool,
    /// Promised file paths still marked `done:false` (empty when `ok`).
    pub missing: Vec<String>,
}

/// Check a wave directory's execution coverage from its `meta.json#checklist`.
///
/// A wave promises files via file-anchored checklist items (those carrying a
/// `path`); the auto-mark hook flips each to `done:true` when its file is
/// edited. `missing` is the set of file-anchored items still `done:false`.
///
/// Fail-open: a missing / unparseable `meta.json`, or a checklist with no
/// file-anchored items, yields `ok:true` — the guard never invents a gap it
/// cannot prove.
#[must_use]
pub fn check(wave_dir: &Path) -> CoverageVerdict {
    let missing: Vec<String> = read_meta(&wave_dir.join("meta.json"))
        .map(|meta| {
            meta.checklist
                .into_iter()
                .filter(|item| !item.done)
                .filter_map(|item| item.path)
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default();
    CoverageVerdict {
        ok: missing.is_empty(),
        missing,
    }
}

/// Resolve the coverage-guard mode from `MUSTARD_WAVE_COVERAGE_MODE`
/// (`off` | `warn` | `strict`). Default (unset **or** unrecognised)
/// [`Mode::Strict`] — the guard ENFORCES by default: a wave that left promised
/// files untouched does not complete. The env var is only an escape hatch to
/// relax in an edge case (`warn` = surface the gap without blocking, `off` =
/// disable), never the primary interface.
#[must_use]
pub fn mode() -> Mode {
    std::env::var("MUSTARD_WAVE_COVERAGE_MODE")
        .ok()
        .and_then(|raw| Mode::parse(&raw))
        .unwrap_or(Mode::Strict)
}

/// Whether a coverage `verdict` should BLOCK wave completion under `mode`.
///
/// Only [`Mode::Strict`] blocks: a gap stops `pipeline.wave.complete` so the
/// wave reopens. [`Mode::Warn`] and [`Mode::Off`] never block — under `warn`
/// the caller still surfaces the gap, it just does not gate completion. A clean
/// verdict never blocks in any mode. Pure — the policy is testable without
/// touching process env.
#[must_use]
pub fn blocks(verdict: &CoverageVerdict, mode: Mode) -> bool {
    !verdict.ok && matches!(mode, Mode::Strict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Write a `meta.json` with the given file-anchored checklist rows
    /// (`(path, done)`) into `wave_dir`.
    fn write_meta(wave_dir: &Path, rows: &[(&str, bool)]) {
        fs::create_dir_all(wave_dir).unwrap();
        let items: Vec<String> = rows
            .iter()
            .map(|(p, done)| format!(r#"{{"label":"{p}","path":"{p}","done":{done}}}"#))
            .collect();
        let json = format!(r#"{{"checklist":[{}]}}"#, items.join(","));
        fs::write(wave_dir.join("meta.json"), json).unwrap();
    }

    #[test]
    fn coverage_ok_when_all_promised_files_touched() {
        let dir = tempdir().unwrap();
        write_meta(dir.path(), &[("src/a.rs", true), ("src/b.rs", true)]);
        let v = check(dir.path());
        assert!(v.ok, "all done → covered");
        assert!(v.missing.is_empty());
    }

    #[test]
    fn coverage_flags_untouched_promised_files() {
        // The sialia case: some files delivered, others left done:false.
        let dir = tempdir().unwrap();
        write_meta(
            dir.path(),
            &[
                ("core/x.ts", true),
                ("backend/Service.cs", false),
                ("backend/Enum.cs", false),
            ],
        );
        let v = check(dir.path());
        assert!(!v.ok, "untouched promised files → not covered");
        let missing: Vec<&str> = v.missing.iter().map(String::as_str).collect();
        assert_eq!(missing, vec!["backend/Service.cs", "backend/Enum.cs"]);
    }

    #[test]
    fn coverage_fail_open_on_missing_meta() {
        // No meta.json → nothing to enforce → ok (never invent a gap).
        let dir = tempdir().unwrap();
        let v = check(dir.path());
        assert!(v.ok);
        assert!(v.missing.is_empty());
    }

    #[test]
    fn coverage_ignores_label_only_items() {
        // A checklist item with no `path` (a task note, not a file promise) is
        // not a coverage obligation.
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(
            dir.path().join("meta.json"),
            r#"{"checklist":[{"label":"do the thing","done":false},{"label":"src/a.rs","path":"src/a.rs","done":true}]}"#,
        )
        .unwrap();
        let v = check(dir.path());
        assert!(v.ok, "label-only items are not file promises");
    }

    #[test]
    fn mode_parses_known_values() {
        assert!(matches!(Mode::parse("strict"), Some(Mode::Strict)));
        assert!(matches!(Mode::parse("off"), Some(Mode::Off)));
        assert!(matches!(Mode::parse("warn"), Some(Mode::Warn)));
    }
}
