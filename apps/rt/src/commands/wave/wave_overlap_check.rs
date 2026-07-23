//! `mustard-rt run wave-overlap-check` — advisory PLAN lint for file-scope
//! overlap between dispatch-parallel waves.
//!
//! `dispatch-plan` assigns each wave a topological `level`; waves that share a
//! level have NO dependency between them and are dispatched together in one
//! round (see [`crate::commands::pipeline::dispatch_plan`]). Two such waves that
//! both declare the SAME file in their `## Files` section would put two agents
//! editing that file concurrently with no ordering between them — the "crisp,
//! disjoint boundaries" decomposition discipline exists precisely to prevent
//! this. This audits every dispatch-parallel PAIR and WARNS on a literal file
//! overlap; it NEVER blocks.
//!
//! The signal is OBJECTIVE — a literal set intersection of declared paths, no
//! threshold, no knob. Overlap across DIFFERENT levels is fine (those waves are
//! sequenced by their dependency edge) and is never flagged.
//!
//! Output: one JSON line, mirroring `wave-size-check`'s advisory shape —
//! `{ action, specDir, overlapCount, overlaps: [{ level, waves:[a,b], files:[…] }] }`,
//! or `{ action: "skip", reason }` for the not-applicable cases. Deterministic
//! and byte-stable: overlaps ordered by (level, waveA, waveB), files sorted.
//!
//! Fail-open: a missing spec dir, a non-wave spec, or an unreadable wave spec
//! all degrade to a `skip` / empty audit — never a panic, always exit 0.

use crate::commands::pipeline::dispatch_plan::{build_plan, wave_declared_files};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Dispatch `mustard-rt run wave-overlap-check`.
pub fn run(spec_dir_arg: Option<&str>) {
    let emit = |v: Value| println!("{v}");
    let Some(spec_dir_arg) = spec_dir_arg else {
        emit(json!({ "action": "skip", "reason": "no-spec-dir-arg" }));
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    // Same spec-dir normalisation as the sibling `wave-size-check`: accept a
    // directory, a `…/spec.md` path, or a bare slug, resolved against the
    // `CLAUDE_PROJECT_DIR`-aware project dir.
    let resolved = crate::shared::context::normalise_spec_dir(
        Path::new(&crate::shared::context::project_dir()),
        spec_dir_arg,
    );
    let spec_dir = if resolved.is_absolute() {
        resolved
    } else {
        cwd.join(resolved)
    };
    if !spec_dir.exists() {
        emit(json!({ "action": "skip", "reason": "spec-dir-not-found" }));
        return;
    }
    // Only a wave plan has dispatch-parallel waves; a Light / tactical-fix spec
    // is a single unit and cannot overlap with a sibling.
    if !spec_dir.join("wave-plan.md").exists() {
        emit(json!({ "action": "skip", "reason": "not-a-wave-plan" }));
        return;
    }

    let project_root =
        crate::shared::context::workspace_root_strict().unwrap_or_else(|_| cwd.clone());
    // The spec slug is the spec dir's own name (`.claude/spec/{slug}/`); it only
    // feeds `build_plan`'s prompt render, which this audit discards.
    let spec_slug = spec_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let overlaps = audit_overlaps(&project_root, &spec_dir, &spec_slug);
    emit(json!({
        "action": "audited",
        "specDir": spec_dir.to_string_lossy(),
        "overlapCount": overlaps.len(),
        "overlaps": overlaps,
    }));
}

/// The deterministic list of same-level file overlaps for a wave-plan spec.
///
/// Extracted so a test drives it with a temp spec dir. Reuses
/// [`build_plan`] for the levels — the audit groups on the SAME `level` the
/// dispatcher parallelises on, so it can never disagree with the real dispatch
/// rounds — and [`wave_declared_files`] for each wave's declared paths.
fn audit_overlaps(project_root: &Path, spec_dir: &Path, spec_slug: &str) -> Vec<Value> {
    let items = build_plan(project_root, spec_dir, spec_slug, None);

    // Group each wave's declared-file SET by dispatch level.
    let mut by_level: BTreeMap<u32, Vec<(u32, BTreeSet<String>)>> = BTreeMap::new();
    for item in &items {
        let files: BTreeSet<String> = wave_declared_files(spec_dir, item.wave, &item.role)
            .into_iter()
            .collect();
        by_level.entry(item.level).or_default().push((item.wave, files));
    }

    // Pairwise intersection within each level. `build_plan` returns items sorted
    // by (level, wave), so each level's Vec is wave-ascending and the i<j walk
    // yields (waveA < waveB) pairs in order → the overlaps list is byte-stable.
    let mut overlaps = Vec::new();
    for (level, waves) in &by_level {
        for i in 0..waves.len() {
            for j in (i + 1)..waves.len() {
                let (wave_a, files_a) = &waves[i];
                let (wave_b, files_b) = &waves[j];
                // `BTreeSet::intersection` yields the shared paths already sorted.
                let shared: Vec<String> = files_a.intersection(files_b).cloned().collect();
                if !shared.is_empty() {
                    overlaps.push(json!({
                        "level": level,
                        "waves": [wave_a, wave_b],
                        "files": shared,
                    }));
                }
            }
        }
    }
    overlaps
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::claude_paths::ClaudePaths;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// Anchor a temp dir as a workspace root the ClaudePaths accessor accepts.
    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

    /// Scaffold `.claude/spec/{slug}/` with a wave-plan table + one `spec.md`
    /// per `(folder, files-body)`. Returns the spec dir.
    fn scaffold(project: &Path, slug: &str, plan: &str, waves: &[(&str, &str)]) -> PathBuf {
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec(slug)
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("wave-plan.md"), plan).unwrap();
        for (folder, files) in waves {
            let wdir = spec_dir.join(folder);
            std::fs::create_dir_all(&wdir).unwrap();
            std::fs::write(wdir.join("spec.md"), files).unwrap();
        }
        spec_dir
    }

    /// A three-wave plan where waves 2 and 3 both depend ONLY on wave 1, so they
    /// share a level (dispatch-parallel). Both declare `shared.rs` → one overlap
    /// naming exactly that pair and file.
    #[test]
    fn same_level_overlap_is_flagged() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let plan = "\
| Wave | Role | Depende de | Summary |
|------|------|------------|---------|
| 1 | base | — | foundation |
| 2 | a | [[1]] | first |
| 3 | b | [[1]] | second |
";
        let spec_dir = scaffold(
            dir.path(),
            "ov",
            plan,
            &[
                ("wave-1-base", "## Files\n- apps/rt/src/base.rs\n"),
                ("wave-2-a", "## Files\n- apps/rt/src/shared.rs\n- apps/rt/src/a.rs\n"),
                ("wave-3-b", "## Files\n- apps/rt/src/shared.rs\n- apps/rt/src/b.rs\n"),
            ],
        );
        let overlaps = audit_overlaps(dir.path(), &spec_dir, "ov");
        assert_eq!(overlaps.len(), 1, "one same-level overlapping pair: {overlaps:?}");
        assert_eq!(overlaps[0]["level"], json!(1));
        assert_eq!(overlaps[0]["waves"], json!([2, 3]));
        assert_eq!(overlaps[0]["files"], json!(["apps/rt/src/shared.rs"]));
    }

    /// Wave 2 DEPENDS on wave 1, so they are on different levels: the dependency
    /// sequences them and a shared file is not a conflict → no warning.
    #[test]
    fn different_level_overlap_is_ignored() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let plan = "\
| Wave | Role | Depende de | Summary |
|------|------|------------|---------|
| 1 | a | — | first |
| 2 | b | [[1]] | second |
";
        let spec_dir = scaffold(
            dir.path(),
            "seq",
            plan,
            &[
                ("wave-1-a", "## Files\n- apps/rt/src/shared.rs\n"),
                ("wave-2-b", "## Files\n- apps/rt/src/shared.rs\n"),
            ],
        );
        let overlaps = audit_overlaps(dir.path(), &spec_dir, "seq");
        assert!(overlaps.is_empty(), "sequential waves are never flagged: {overlaps:?}");
    }

    /// Two dispatch-parallel waves with DISJOINT files are clean.
    #[test]
    fn disjoint_same_level_is_clean() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let plan = "\
| Wave | Role | Depende de | Summary |
|------|------|------------|---------|
| 1 | base | — | foundation |
| 2 | a | [[1]] | first |
| 3 | b | [[1]] | second |
";
        let spec_dir = scaffold(
            dir.path(),
            "dj",
            plan,
            &[
                ("wave-1-base", "## Files\n- apps/rt/src/base.rs\n"),
                ("wave-2-a", "## Files\n- apps/rt/src/a.rs\n"),
                ("wave-3-b", "## Files\n- apps/rt/src/b.rs\n"),
            ],
        );
        let overlaps = audit_overlaps(dir.path(), &spec_dir, "dj");
        assert!(overlaps.is_empty(), "disjoint parallel waves are clean: {overlaps:?}");
    }

    /// The separator normalisation makes a Windows-spelled and a Unix-spelled
    /// declaration of the same path compare equal — the overlap still fires, and
    /// the reported path is the forward-slash form.
    #[test]
    fn separator_normalisation_matches_windows_and_unix_paths() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let plan = "\
| Wave | Role | Depende de | Summary |
|------|------|------------|---------|
| 1 | base | — | foundation |
| 2 | a | [[1]] | first |
| 3 | b | [[1]] | second |
";
        let spec_dir = scaffold(
            dir.path(),
            "win",
            plan,
            &[
                ("wave-1-base", "## Files\n- apps/rt/src/base.rs\n"),
                ("wave-2-a", "## Files\n- apps\\rt\\src\\shared.rs\n"),
                ("wave-3-b", "## Files\n- apps/rt/src/shared.rs\n"),
            ],
        );
        let overlaps = audit_overlaps(dir.path(), &spec_dir, "win");
        assert_eq!(overlaps.len(), 1, "separator-normalised overlap: {overlaps:?}");
        assert_eq!(overlaps[0]["files"], json!(["apps/rt/src/shared.rs"]));
    }

    /// Fail-open: a spec that is NOT a wave plan (no `wave-plan.md`) yields no
    /// overlaps — the single-spec dispatch item has no sibling to collide with.
    #[test]
    fn non_wave_spec_has_no_overlaps() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec_dir = ClaudePaths::for_project(dir.path())
            .unwrap()
            .for_spec("flat")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Flat\n\n## Files\n- apps/rt/src/foo.rs\n",
        )
        .unwrap();
        let overlaps = audit_overlaps(dir.path(), &spec_dir, "flat");
        assert!(overlaps.is_empty(), "a single-spec plan cannot overlap: {overlaps:?}");
    }
}
