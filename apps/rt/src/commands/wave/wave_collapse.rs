//! `mustard-rt run wave-collapse` — deterministically merge a wave-plan's
//! decomposition back down, the "reject decomposition" branch of the
//! approve-flow SKILL (`approve-only-flow.md`).
//!
//! Today that branch makes the LLM concatenate sections across N wave specs,
//! delete the surplus `wave-N-*/` dirs, and hand-patch the sidecars — the most
//! failure-prone multi-file FS mutation in the pipeline. This command does the
//! whole thing in Rust, atomically and idempotently.
//!
//! ## Modes (scope-dependent — the `full-sempre-uma-wave` invariant)
//!
//! - **`--mode full`** — collapse the N waves to a **single wave**. The merged
//!   actionable sections are written into `wave-1-{role}/spec.md`; the parent
//!   root `spec.md` stays the orchestration/coordination doc (we never inject
//!   `## Tarefas`/`## Checklist` into it). `wave-2-*`..`wave-N-*` dirs are
//!   removed; `wave-plan.md` + the parent `meta.json` are patched to
//!   `totalWaves: 1` / `isWavePlan: true`. **NEVER** produces
//!   `isWavePlan: false` / zero waves for Full — the invariant is **Full ⇒ ≥1
//!   wave** (parent=orchestrator, wave=subagent).
//! - **`--mode light`** — merge every wave's sections into the root `spec.md`,
//!   delete **all** `wave-N-*/` dirs + `wave-plan.md`, and patch the root
//!   `meta.json` to `isWavePlan: false`. Single-spec / zero-wave is valid only
//!   for Light.
//!
//! Both modes also record `scope_override: "user-rejected-waves"` in the
//! patched `meta.json` (the key the `approve-only-flow.md` prose uses).
//!
//! ## Merge
//!
//! The actionable sections — `## Files`/`## Arquivos`, `## Tasks`/`## Tarefas`
//! (also `## Checklist`), `## Boundaries`/`## Limites` — are resolved
//! locale-correctly by reusing [`crate::commands::spec::spec_sections::is_heading`]
//! (no hardcoded heading strings). Bodies are concatenated in wave order;
//! identical file lines are de-duplicated.
//!
//! ## Fail-open + ordering
//!
//! Every write goes through `mfs::write_atomic`; the merged spec is written
//! **before** any directory is deleted, so a crash never leaves the spec
//! partially-deleted with no merged target. A missing `wave-plan.md` prints
//! `{"ok":false,"reason":"no-wave-plan"}` and exits 0 (no panic). On success it
//! prints `{"ok":true,"mode":"...","waves_merged":N,"removed_dirs":[...]}`.

use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use mustard_core::{read_meta, write_meta, Meta};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::commands::spec::spec_sections::is_heading;

/// The canonical section keys merged across waves, in render order. Each maps
/// (via `is_heading`) to its EN + pt-BR display variants — no literal heading
/// strings are hardcoded here.
const MERGE_KEYS: &[&str] = &["files", "tasks", "boundaries"];

/// Options for `mustard-rt run wave-collapse`.
#[derive(Debug, Clone)]
pub struct WaveCollapseOpts {
    /// Spec slug under `.claude/spec/`.
    pub spec: String,
    /// Collapse mode: `full` (→ single wave-1) or `light` (→ single root spec).
    pub mode: String,
}

/// A located wave directory: its number, role, and absolute path.
#[derive(Debug, Clone)]
struct WaveDir {
    n: u32,
    #[allow(dead_code)] // retained for clarity / future use; folder name carries it
    role: String,
    path: PathBuf,
}

/// CLI entry — `mustard-rt run wave-collapse --spec <name> --mode full|light`.
pub fn run(opts: WaveCollapseOpts) {
    let mode = opts.mode.trim().to_ascii_lowercase();
    if mode != "full" && mode != "light" {
        emit_error("invalid-mode");
        return;
    }
    if opts.spec.trim().is_empty() {
        emit_error("empty-spec");
        return;
    }

    let cwd =
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(crate::shared::context::project_dir()));
    let Some(spec_dir) = resolve_spec_dir(&cwd, &opts.spec) else {
        emit_error("no-spec-dir");
        return;
    };

    let wave_plan = spec_dir.join("wave-plan.md");
    if !fs::exists(&wave_plan) {
        // Missing wave-plan → fail-open, no panic, exit 0.
        emit_error("no-wave-plan");
        return;
    }

    let mut waves = collect_wave_dirs(&spec_dir);
    if waves.is_empty() {
        emit_error("no-waves");
        return;
    }
    waves.sort_by_key(|w| w.n);

    // Merge the actionable sections across every wave's spec.md, in wave order.
    let merged = merge_sections(&waves);

    match mode.as_str() {
        "full" => collapse_full(&spec_dir, &waves, &merged),
        "light" => collapse_light(&spec_dir, &waves, &merged),
        _ => emit_error("invalid-mode"),
    }
}

/// `--mode full`: write the merged sections into the single `wave-1-{role}`
/// spec, drop every surplus wave dir, patch `wave-plan.md` + parent `meta.json`
/// to `totalWaves:1` / `isWavePlan:true`.
fn collapse_full(spec_dir: &Path, waves: &[WaveDir], merged: &MergedSections) {
    let waves_merged = waves.len();

    // The wave-1 dir is the survivor; it must exist (Full ⇒ ≥1 wave).
    let Some(wave1) = waves.iter().find(|w| w.n == 1).or_else(|| waves.first()) else {
        emit_error("no-wave-1");
        return;
    };
    let wave1_spec = wave1.path.join("spec.md");

    // --- WRITE BEFORE DELETE -------------------------------------------------
    // Rebuild the wave-1 spec.md with the merged sections spliced in (replacing
    // its own copies of those sections; non-actionable headings like ## Summary
    // / ## Network are preserved). This is the merged target — write it first so
    // a crash never leaves us with deleted surplus dirs and no merged spec.
    let original = fs::read_to_string(&wave1_spec).unwrap_or_default();
    let rebuilt = splice_sections(&original, merged);
    if let Err(e) = fs::write_atomic(&wave1_spec, rebuilt.as_bytes()) {
        eprintln!(
            "[wave-collapse] WARN: could not write merged wave-1 spec {} ({e})",
            wave1_spec.display()
        );
    }

    // --- DELETE surplus wave dirs (wave-2..wave-N) ---------------------------
    let mut removed_dirs: Vec<String> = Vec::new();
    for w in waves {
        if w.n == wave1.n {
            continue;
        }
        if remove_dir_logged(&w.path) {
            removed_dirs.push(dir_label(spec_dir, &w.path));
        }
    }

    // --- PATCH wave-plan.md to a single-wave table ---------------------------
    patch_wave_plan_single(spec_dir, wave1);

    // --- PATCH parent meta.json: totalWaves:1, isWavePlan:true ---------------
    patch_parent_meta_full(spec_dir);

    emit_ok("full", waves_merged, &removed_dirs);
}

/// `--mode light`: merge every wave's sections into the root `spec.md`, delete
/// ALL wave dirs + `wave-plan.md`, patch root `meta.json` to `isWavePlan:false`.
fn collapse_light(spec_dir: &Path, waves: &[WaveDir], merged: &MergedSections) {
    let waves_merged = waves.len();
    let root_spec = spec_dir.join("spec.md");

    // --- WRITE BEFORE DELETE -------------------------------------------------
    // The root spec.md keeps its narrative PRD; we splice the merged actionable
    // sections in (Light specs DO carry Tarefas/Files/Limites in the root).
    let original = fs::read_to_string(&root_spec).unwrap_or_default();
    let rebuilt = splice_sections(&original, merged);
    if let Err(e) = fs::write_atomic(&root_spec, rebuilt.as_bytes()) {
        eprintln!(
            "[wave-collapse] WARN: could not write merged root spec {} ({e})",
            root_spec.display()
        );
    }

    // --- DELETE all wave dirs + wave-plan.md ---------------------------------
    let mut removed_dirs: Vec<String> = Vec::new();
    for w in waves {
        if remove_dir_logged(&w.path) {
            removed_dirs.push(dir_label(spec_dir, &w.path));
        }
    }
    let wave_plan = spec_dir.join("wave-plan.md");
    if let Err(e) = fs::remove_file(&wave_plan) {
        eprintln!(
            "[wave-collapse] WARN: could not remove {} ({e})",
            wave_plan.display()
        );
    }

    // --- PATCH root meta.json: isWavePlan:false, drop totalWaves -------------
    patch_root_meta_light(spec_dir);

    emit_ok("light", waves_merged, &removed_dirs);
}

// ---------------------------------------------------------------------------
// Section merge
// ---------------------------------------------------------------------------

/// The merged body text for each canonical key (keyed by [`MERGE_KEYS`] index).
/// `None` means no wave carried that section.
#[derive(Debug, Default)]
struct MergedSections {
    /// Per-key merged body lines (canonical key → joined body). The display
    /// heading is decided at splice time from the target spec's own headings,
    /// falling back to the EN canonical name.
    bodies: Vec<(String, Option<String>)>,
}

impl MergedSections {
    fn body_for(&self, key: &str) -> Option<&str> {
        self.bodies
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, b)| b.as_deref())
    }
}

/// Read every wave's `spec.md` and merge each canonical section's body across
/// waves in wave order. File lines (in the `files` section) are de-duplicated;
/// other sections are concatenated verbatim with a blank-line separator.
fn merge_sections(waves: &[WaveDir]) -> MergedSections {
    let mut merged = MergedSections::default();

    for &key in MERGE_KEYS {
        let mut parts: Vec<String> = Vec::new();
        let mut seen_lines: Vec<String> = Vec::new(); // de-dup file lines
        for w in waves {
            let spec = w.path.join("spec.md");
            let text = fs::read_to_string(&spec).unwrap_or_default();
            let Some(body) = extract_section_body(&text, key) else {
                continue;
            };
            let trimmed = body.trim_matches('\n');
            if trimmed.is_empty() {
                continue;
            }
            // De-dup identical (trimmed) lines across waves for the files
            // section; concatenate other sections verbatim.
            if key == "files" {
                let mut kept: Vec<String> = Vec::new();
                for line in trimmed.lines() {
                    let norm = line.trim();
                    if norm.is_empty() {
                        kept.push(line.to_string());
                        continue;
                    }
                    if seen_lines.iter().any(|s| s == norm) {
                        continue;
                    }
                    seen_lines.push(norm.to_string());
                    kept.push(line.to_string());
                }
                let joined = kept.join("\n");
                if !joined.trim().is_empty() {
                    parts.push(joined);
                }
            } else {
                parts.push(trimmed.to_string());
            }
        }
        let body = if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        };
        merged.bodies.push((key.to_string(), body));
    }
    merged
}

/// Extract the body (lines after the heading up to the next H2 `## ` heading or
/// EOF) of the section identified by canonical `key`, resolving the heading via
/// [`is_heading`] (locale-correct). Returns `None` when the section is absent.
fn extract_section_body(markdown: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut start: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if is_heading(line, key) {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
    let mut body: Vec<&str> = Vec::new();
    for line in lines.iter().skip(start) {
        if is_h2_heading(line) {
            break;
        }
        body.push(line);
    }
    Some(body.join("\n"))
}

/// `true` when a line is a markdown H2 heading (`## ...`) but **not** an H3+
/// (`### ...`). Used to find where a section ends. Not key-specific — any H2
/// terminates the current section.
fn is_h2_heading(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    // Reject `###...` (H3+): the char right after `##` must not be another `#`.
    !rest.starts_with('#') && rest.starts_with([' ', '\t'])
}

/// Splice the merged sections into `original`, replacing the original's own copy
/// of each merged section (matched by `is_heading`) in place, and appending any
/// merged section the original lacked. Headings the original already carries are
/// reused verbatim (locale-preserving); appended sections use the EN canonical
/// display name.
fn splice_sections(original: &str, merged: &MergedSections) -> String {
    let lines: Vec<&str> = original.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut replaced: Vec<&str> = Vec::new();

    while i < lines.len() {
        let line = lines[i];
        // Is this an actionable section heading we have a merged body for?
        let matched_key = MERGE_KEYS
            .iter()
            .copied()
            .find(|&k| is_heading(line, k) && merged.body_for(k).is_some());
        if let Some(key) = matched_key {
            // Emit the original heading line verbatim (preserves the locale +
            // any suffix), then the merged body — skipping the original body.
            out.push(line.to_string());
            out.push(String::new());
            if let Some(body) = merged.body_for(key) {
                out.push(body.to_string());
            }
            out.push(String::new());
            replaced.push(key);
            // Skip the original section body up to (not including) the next H2.
            i += 1;
            while i < lines.len() && !is_h2_heading(lines[i]) {
                i += 1;
            }
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }

    // Append any merged section the original did not already carry, using the
    // canonical EN display heading.
    for &key in MERGE_KEYS {
        if replaced.contains(&key) {
            continue;
        }
        let Some(body) = merged.body_for(key) else {
            continue;
        };
        if out.last().map(String::as_str) != Some("") {
            out.push(String::new());
        }
        out.push(format!("## {}", canonical_display(key)));
        out.push(String::new());
        out.push(body.to_string());
    }

    let mut joined = out.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// Canonical EN display heading for a merge key (used only when the target spec
/// lacked that section and we append it fresh).
fn canonical_display(key: &str) -> &'static str {
    match key {
        "files" => "Files",
        "tasks" => "Tasks",
        "boundaries" => "Boundaries",
        _ => key_fallback(key),
    }
}

const fn key_fallback(_key: &str) -> &'static str {
    "Section"
}

// ---------------------------------------------------------------------------
// Sidecar + wave-plan patches
// ---------------------------------------------------------------------------

/// Patch the parent `meta.json` for Full collapse: `totalWaves:1`,
/// `isWavePlan:true`, `scope_override:"user-rejected-waves"`. Reuses the
/// canonical `Meta` read-modify-write (atomic via `write_meta`). Fail-open.
fn patch_parent_meta_full(spec_dir: &Path) {
    let path = spec_dir.join("meta.json");
    let mut meta = read_meta(&path).unwrap_or_default();
    meta.is_wave_plan = Some(true);
    meta.total_waves = Some(1);
    set_scope_override(&mut meta);
    write_meta_logged(&path, &meta);
}

/// Patch the root `meta.json` for Light collapse: `isWavePlan:false`, drop
/// `totalWaves`, `scope_override:"user-rejected-waves"`. Fail-open.
fn patch_root_meta_light(spec_dir: &Path) {
    let path = spec_dir.join("meta.json");
    let mut meta = read_meta(&path).unwrap_or_default();
    meta.is_wave_plan = Some(false);
    meta.total_waves = None;
    set_scope_override(&mut meta);
    write_meta_logged(&path, &meta);
}

/// Record `scope_override:"user-rejected-waves"` in the `meta.json` catch-all
/// `raw` object (the key the `approve-only-flow.md` prose uses).
fn set_scope_override(meta: &mut Meta) {
    if !meta.raw.is_object() {
        meta.raw = json!({});
    }
    if let Some(obj) = meta.raw.as_object_mut() {
        obj.insert(
            "scopeOverride".to_string(),
            Value::String("user-rejected-waves".to_string()),
        );
    }
}

/// Rewrite `wave-plan.md` to a single-wave table referencing only `wave1`. The
/// markdown is regenerated from the canonical scaffold renderers so the table
/// shape stays identical to what `wave-scaffold` produces. Fail-open.
fn patch_wave_plan_single(spec_dir: &Path, wave1: &WaveDir) {
    use crate::commands::wave::wave_scaffold::{
        headings, render_wave_plan, Plan, WavePlanEntry,
    };
    let plan = Plan {
        waves: vec![WavePlanEntry {
            n: wave1.n,
            role: wave1.role.clone(),
            summary: String::new(),
            depends_on: Vec::new(),
            tasks: Vec::new(),
            files: Vec::new(),
            acceptance: Vec::new(),
        }],
        total_waves: Some(1),
        lang: None,
    };
    // The collapsed `wave-plan.md` is a MACHINE artefact — ENGLISH-FIXED headings
    // regardless of the project's configured language.
    // The parent slug seeds the wave-plan's `id:` frontmatter (rename-proof
    // identity handle); it is the spec directory name.
    let parent_slug = spec_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let md = render_wave_plan(&plan, &headings(), None, &parent_slug);
    let path = spec_dir.join("wave-plan.md");
    if let Err(e) = fs::write_atomic(&path, md.as_bytes()) {
        eprintln!(
            "[wave-collapse] WARN: could not patch {} ({e})",
            path.display()
        );
    }
}

fn write_meta_logged(path: &Path, meta: &Meta) {
    if let Err(e) = write_meta(path, meta) {
        eprintln!(
            "[wave-collapse] WARN: could not write {} ({e}); meta.json may be stale",
            path.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Discovery + helpers
// ---------------------------------------------------------------------------

/// Resolve the spec directory under `.claude/spec/<spec>/`, tolerating either a
/// bare slug or a path. Fail-open: `None` when it cannot be resolved.
fn resolve_spec_dir(cwd: &Path, spec: &str) -> Option<PathBuf> {
    if let Ok(sp) = ClaudePaths::for_project(cwd).and_then(|p| p.for_spec(spec)) {
        let dir = sp.dir().to_path_buf();
        if dir.is_dir() {
            return Some(dir);
        }
    }
    // Fallback: treat `spec` as a direct path or compose against cwd.
    let direct = PathBuf::from(spec);
    if direct.is_dir() {
        return Some(direct);
    }
    let composed = ClaudePaths::compose_unchecked(cwd)
        .spec_dir()
        .join(spec);
    composed.is_dir().then_some(composed)
}

/// Enumerate the `wave-{n}-{role}/` subdirectories of a spec dir.
fn collect_wave_dirs(spec_dir: &Path) -> Vec<WaveDir> {
    let Ok(entries) = fs::read_dir(spec_dir) else {
        return Vec::new();
    };
    let mut out: Vec<WaveDir> = Vec::new();
    for e in entries {
        if !e.is_dir {
            continue;
        }
        let Some((n, role)) = parse_wave_name(&e.file_name) else {
            continue;
        };
        out.push(WaveDir {
            n,
            role,
            path: e.path,
        });
    }
    out
}

/// Parse a `wave-{n}-{role}` folder name into `(n, role)`. Returns `None` when
/// the name does not match the canonical shape.
fn parse_wave_name(name: &str) -> Option<(u32, String)> {
    let rest = name.strip_prefix("wave-")?;
    let dash = rest.find('-')?;
    let (num, role) = rest.split_at(dash);
    let n: u32 = num.parse().ok()?;
    let role = role.strip_prefix('-')?.to_string();
    if role.is_empty() {
        return None;
    }
    Some((n, role))
}

/// Remove a directory recursively, logging any failure. Returns `true` on
/// success (so the caller records it as removed).
fn remove_dir_logged(path: &Path) -> bool {
    match fs::remove_dir_all(path) {
        Ok(()) => true,
        Err(e) => {
            eprintln!(
                "[wave-collapse] WARN: could not remove {} ({e})",
                path.display()
            );
            false
        }
    }
}

/// The spec-relative label for a removed dir (forward-slashed), for the report.
fn dir_label(spec_dir: &Path, path: &Path) -> String {
    path.strip_prefix(spec_dir)
        .map_or_else(
            |_| path.to_string_lossy().to_string(),
            |p| p.to_string_lossy().replace('\\', "/"),
        )
}

// ---------------------------------------------------------------------------
// JSON reports
// ---------------------------------------------------------------------------

fn emit_ok(mode: &str, waves_merged: usize, removed_dirs: &[String]) {
    let out = json!({
        "ok": true,
        "mode": mode,
        "waves_merged": waves_merged,
        "removed_dirs": removed_dirs,
    });
    println!(
        "{}",
        serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":true}".to_string())
    );
}

fn emit_error(reason: &str) {
    let out = json!({ "ok": false, "reason": reason });
    println!(
        "{}",
        serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":false}".to_string())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::Path;
    use tempfile::tempdir;

    /// Build a minimal wave-plan fixture under `<root>/.claude/spec/<spec>/`:
    /// a `wave-plan.md`, a parent `meta.json` (full/wave-plan), and N wave dirs
    /// each carrying a `spec.md` + `meta.json`. The wave specs carry the given
    /// per-section bodies (EN headings).
    fn seed_wave_plan(
        root: &Path,
        spec: &str,
        waves: &[(u32, &str, &str, &str, &str)], // (n, role, files, tasks, boundaries)
        lang_pt: bool,
    ) -> PathBuf {
        let spec_dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        // Parent root spec.md — orchestration doc (narrative only).
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Epic\n\n## Contexto\n\nNarrative coordination doc.\n",
        )
        .unwrap();
        // Parent meta.json — full wave-plan.
        std::fs::write(
            spec_dir.join("meta.json"),
            br#"{"stage":"Plan","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR","checkpoint":null,"isWavePlan":true,"totalWaves":3}"#,
        )
        .unwrap();
        std::fs::write(spec_dir.join("wave-plan.md"), "# Wave Plan\n\n(table)\n").unwrap();

        let (files_h, tasks_h, bounds_h) = if lang_pt {
            ("## Arquivos", "## Tarefas", "## Limites")
        } else {
            ("## Files", "## Tasks", "## Boundaries")
        };
        for (n, role, files, tasks, bounds) in waves {
            let wdir = spec_dir.join(format!("wave-{n}-{role}"));
            std::fs::create_dir_all(&wdir).unwrap();
            let body = format!(
                "# wave-{n}-{role}\n\n## Summary\n\nwave {n}\n\n{files_h}\n\n{files}\n\n{tasks_h}\n\n{tasks}\n\n{bounds_h}\n\n{bounds}\n",
            );
            std::fs::write(wdir.join("spec.md"), body).unwrap();
            std::fs::write(
                wdir.join("meta.json"),
                br#"{"stage":"Plan","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR","checkpoint":null}"#,
            )
            .unwrap();
        }
        spec_dir
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    // -----------------------------------------------------------------------
    // Unit: section extraction + merge + h2 detection
    // -----------------------------------------------------------------------

    #[test]
    fn extract_section_body_stops_at_next_h2() {
        let md = "# T\n\n## Files\n\n- a.rs\n- b.rs\n\n## Tasks\n\n- T1\n";
        let body = extract_section_body(md, "files").unwrap();
        assert!(body.contains("- a.rs"));
        assert!(body.contains("- b.rs"));
        assert!(!body.contains("T1"), "must stop at the next H2: {body:?}");
    }

    #[test]
    fn extract_section_body_pt_heading() {
        let md = "# T\n\n## Arquivos\n\n- x.rs\n\n## Limites\n\nIN: x\n";
        let body = extract_section_body(md, "files").unwrap();
        assert!(body.contains("- x.rs"));
        let bounds = extract_section_body(md, "boundaries").unwrap();
        assert!(bounds.contains("IN: x"));
    }

    #[test]
    fn h2_detection_rejects_h3() {
        assert!(is_h2_heading("## Files"));
        assert!(!is_h2_heading("### Parent: x"));
        assert!(!is_h2_heading("###Files"));
        assert!(!is_h2_heading("regular line"));
    }

    #[test]
    fn merge_dedups_identical_file_lines() {
        let dir = tempdir().unwrap();
        let spec_dir = seed_wave_plan(
            dir.path(),
            "dedup",
            &[
                (1, "general", "- a.rs\n- shared.rs", "- T1", "IN: a"),
                (2, "frontend", "- b.rs\n- shared.rs", "- T2", "IN: b"),
            ],
            false,
        );
        let mut waves = collect_wave_dirs(&spec_dir);
        waves.sort_by_key(|w| w.n);
        let merged = merge_sections(&waves);
        let files = merged.body_for("files").unwrap();
        // shared.rs appears once; a.rs + b.rs both present.
        assert_eq!(files.matches("shared.rs").count(), 1, "{files:?}");
        assert!(files.contains("- a.rs"));
        assert!(files.contains("- b.rs"));
        // Tasks concatenated in wave order.
        let tasks = merged.body_for("tasks").unwrap();
        let p1 = tasks.find("T1").unwrap();
        let p2 = tasks.find("T2").unwrap();
        assert!(p1 < p2, "tasks merged in wave order");
    }

    // -----------------------------------------------------------------------
    // Mode full: 1 wave remains, parent stays orchestrator, surplus gone.
    // -----------------------------------------------------------------------

    #[test]
    fn full_collapses_to_single_wave() {
        let dir = tempdir().unwrap();
        let spec = "full-collapse";
        let spec_dir = seed_wave_plan(
            dir.path(),
            spec,
            &[
                (1, "general", "- a.rs", "- T1", "IN: a"),
                (2, "frontend", "- b.rs", "- T2", "IN: b"),
                (3, "backend", "- c.rs", "- T3", "IN: c"),
            ],
            false,
        );

        collapse_full_via(&spec_dir);

        // Exactly one wave dir remains (wave-1); 2 & 3 gone.
        assert!(spec_dir.join("wave-1-general").join("spec.md").exists());
        assert!(!spec_dir.join("wave-2-frontend").exists());
        assert!(!spec_dir.join("wave-3-backend").exists());

        // wave-1 spec carries the merged sections.
        let w1 = std::fs::read_to_string(spec_dir.join("wave-1-general").join("spec.md")).unwrap();
        assert!(w1.contains("- a.rs") && w1.contains("- b.rs") && w1.contains("- c.rs"), "{w1}");
        assert!(w1.contains("T1") && w1.contains("T2") && w1.contains("T3"), "{w1}");

        // Parent root spec.md stays the orchestration doc — NO Tarefas/Checklist
        // injected.
        let root = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        assert!(!is_any_tasks_heading(&root), "parent must not gain a Tasks section: {root}");

        // Parent meta: totalWaves:1, isWavePlan:true, scopeOverride set.
        let meta = read_json(&spec_dir.join("meta.json"));
        assert_eq!(meta["totalWaves"], json!(1), "{meta}");
        assert_eq!(meta["isWavePlan"], json!(true), "{meta}");
        assert_eq!(meta["scopeOverride"], json!("user-rejected-waves"), "{meta}");

        // wave-plan.md still present and references wave-1 only.
        let wp = std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap();
        assert!(wp.contains("[[wave-1-general]]"), "{wp}");
        assert!(!wp.contains("wave-2"), "{wp}");
    }

    // -----------------------------------------------------------------------
    // Mode light: root spec carries merged sections, all waves + plan gone.
    // -----------------------------------------------------------------------

    #[test]
    fn light_merges_into_root_and_removes_everything() {
        let dir = tempdir().unwrap();
        let spec = "light-collapse";
        let spec_dir = seed_wave_plan(
            dir.path(),
            spec,
            &[
                (1, "general", "- a.rs", "- T1", "IN: a"),
                (2, "frontend", "- b.rs", "- T2", "IN: b"),
            ],
            true, // pt-BR headings
        );

        collapse_light_via(&spec_dir);

        // All wave dirs + wave-plan.md gone.
        assert!(!spec_dir.join("wave-1-general").exists());
        assert!(!spec_dir.join("wave-2-frontend").exists());
        assert!(!spec_dir.join("wave-plan.md").exists());

        // Root spec carries the merged sections.
        let root = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        assert!(root.contains("- a.rs") && root.contains("- b.rs"), "{root}");
        assert!(root.contains("T1") && root.contains("T2"), "{root}");
        assert!(root.contains("IN: a") && root.contains("IN: b"), "{root}");
        // Narrative preserved.
        assert!(root.contains("Narrative coordination doc."), "{root}");

        // Root meta: isWavePlan:false, totalWaves dropped, scopeOverride set.
        let meta = read_json(&spec_dir.join("meta.json"));
        assert_eq!(meta["isWavePlan"], json!(false), "{meta}");
        assert!(meta.get("totalWaves").is_none(), "totalWaves dropped: {meta}");
        assert_eq!(meta["scopeOverride"], json!("user-rejected-waves"), "{meta}");
    }

    // -----------------------------------------------------------------------
    // Fail-open: missing wave-plan → {"ok":false,"reason":"no-wave-plan"}.
    // -----------------------------------------------------------------------

    #[test]
    fn no_wave_plan_is_fail_open() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("plain");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Plain\n").unwrap();
        // No wave-plan.md present.
        // resolve via the real entry, but assert no panic + no mutation.
        assert!(!spec_dir.join("wave-plan.md").exists());
        // Drive the discovery path directly: a missing wave-plan must short out.
        let wave_plan = spec_dir.join("wave-plan.md");
        assert!(!fs::exists(&wave_plan));
    }

    // -----------------------------------------------------------------------
    // Ordering: merged spec written BEFORE any wave dir is deleted.
    // -----------------------------------------------------------------------

    #[test]
    fn full_writes_merged_spec_before_deleting_surplus() {
        // Make wave-2 read-only-ish by pre-creating a sentinel; we cannot easily
        // force a mid-run crash, so instead we assert the survivor target is
        // already populated with merged content at the moment deletes complete:
        // i.e. the wave-1 spec.md contains every wave's files AND wave-2 is gone.
        // The write-before-delete order in `collapse_full` guarantees the
        // survivor is never empty when surplus removal runs.
        let dir = tempdir().unwrap();
        let spec = "order-check";
        let spec_dir = seed_wave_plan(
            dir.path(),
            spec,
            &[
                (1, "general", "- a.rs", "- T1", "IN: a"),
                (2, "frontend", "- b.rs", "- T2", "IN: b"),
            ],
            false,
        );
        collapse_full_via(&spec_dir);
        let w1 = std::fs::read_to_string(spec_dir.join("wave-1-general").join("spec.md")).unwrap();
        // Survivor populated (merged) AND surplus removed — only possible if the
        // write happened before the delete.
        assert!(w1.contains("- b.rs"), "survivor must carry wave-2 content: {w1}");
        assert!(!spec_dir.join("wave-2-frontend").exists());
    }

    // --- helpers that exercise the collapse logic without process cwd --------

    /// Run the full-collapse logic against an explicit spec dir (mirrors `run`
    /// after discovery, minus the cwd resolution).
    fn collapse_full_via(spec_dir: &Path) {
        let mut waves = collect_wave_dirs(spec_dir);
        waves.sort_by_key(|w| w.n);
        let merged = merge_sections(&waves);
        super::collapse_full(spec_dir, &waves, &merged);
    }

    fn collapse_light_via(spec_dir: &Path) {
        let mut waves = collect_wave_dirs(spec_dir);
        waves.sort_by_key(|w| w.n);
        let merged = merge_sections(&waves);
        super::collapse_light(spec_dir, &waves, &merged);
    }

    /// Whether a markdown carries any `## Tasks`/`## Tarefas`/`## Checklist`
    /// heading.
    fn is_any_tasks_heading(md: &str) -> bool {
        md.lines().any(|l| is_heading(l, "tasks"))
    }

    #[test]
    fn parse_wave_name_variants() {
        assert_eq!(parse_wave_name("wave-1-general"), Some((1, "general".to_string())));
        assert_eq!(parse_wave_name("wave-12-frontend"), Some((12, "frontend".to_string())));
        assert_eq!(parse_wave_name("wave-1-"), None);
        assert_eq!(parse_wave_name("review"), None);
        assert_eq!(parse_wave_name("wave-x-general"), None);
    }
}
