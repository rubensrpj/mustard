//! `mustard-rt run migrate-to-meta` — one-shot extraction of lifecycle headers
//! into `meta.json` sidecars (Wave 3 of mustard-unification).
//!
//! ## Scope
//!
//! Walks every directory under `<root>` (default `.claude/spec`) recursively,
//! and for each directory that contains at least one `*.md` file with
//! recognisable lifecycle metadata, writes a `meta.json` beside it. The legacy
//! `### Stage:`/`### Outcome:`/`### Phase:`/`### Scope:`/`### Lang:`/
//! `### Checkpoint:`/`### Parent:` headers remain in the `.md` (this subcommand
//! is the **mirror** step; Wave 3 T3.4 removes them in a second pass).
//!
//! ## Safety contract
//!
//! - **Atomic per file.** [`mustard_core::write_meta`] writes via tempfile +
//!   rename; a crash never leaves a half-written `meta.json`.
//! - **Idempotent.** Re-running this subcommand on a tree that already has
//!   `meta.json` produces byte-identical output (the writer normalises the
//!   field order through `Meta`'s declaration order).
//! - **Fail-open per file.** A read/parse failure on one spec increments
//!   `errors` and never aborts the batch.
//! - **Recursive coverage.** The walk descends into sub-wave directories so a
//!   wave-plan epic (`spec.md` + `wave-N-{role}/spec.md` + `wave-plan.md`)
//!   produces a `meta.json` next to each `.md` carrying a lifecycle header.
//!   The `qa/` and `review/` phase dirs are an exception: their `*.md` are
//!   still walked (and counted in `total`), but no `meta.json` is written
//!   beside them — they are pipeline phases, not specs, and carry no
//!   lifecycle (see [`is_phase_dir`]).

use mustard_core::io::fs;
use mustard_core::domain::spec;
use mustard_core::ClaudePaths;
use mustard_core::{Meta, write_meta};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::shared::context::project_dir as env_project_dir;
use mustard_core::time::now_iso8601;

/// Options for `mustard-rt run migrate-to-meta`.
pub struct MigrateToMetaOpts {
    /// Root directory to walk recursively. Defaults to `.claude/spec`.
    pub root: Option<PathBuf>,
    /// When `true`, force-rewrite an existing `meta.json` even if it already
    /// exists. The default is to leave existing sidecars alone (idempotent
    /// content; useful when re-running after a manual edit).
    pub force: bool,
    /// When `true`, after writing `meta.json` also rewrite the `.md` to remove
    /// the legacy `### Stage:` / `### Outcome:` / `### Phase:` / `### Scope:` /
    /// `### Lang:` / `### Checkpoint:` / `### Parent:` / `### Flags:` /
    /// `### Total waves:` lines so the sidecar becomes the sole home of
    /// machine-parseable metadata (Wave 3 T3.4 cleanup pass). Atomic per file.
    /// Idempotent — re-running on already-stripped `.md`s is a no-op.
    pub strip_headers: bool,
}

/// Per-file outcome surfaced in the JSON report.
#[derive(Serialize)]
struct FileRecord {
    path: String,
    action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// Full JSON report emitted to stdout.
#[derive(Serialize)]
struct MigrationReport {
    ran_at: String,
    root: String,
    total: usize,
    written: usize,
    already_present: usize,
    skipped_no_header: usize,
    errors: usize,
    /// Number of `.md` files whose legacy `### Key:` headers were stripped in
    /// this pass (only set when `--strip-headers` was passed).
    headers_stripped: usize,
    files: Vec<FileRecord>,
}

/// Recursively collect every `*.md` under `root`, sorted for stable output.
/// Skips common ignore paths (`node_modules`, `target`, `.git`, build outputs).
fn collect_md(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_md_into(root, &mut out);
    out.sort();
    out
}

/// Names we always skip — these never carry specs and would explode the walk.
fn is_ignored(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | "target" | ".git" | "dist" | "build" | ".next" | ".worktrees"
    )
}

/// D3: `qa/` and `review/` are pipeline *phases*, not specs — they carry no
/// lifecycle, so no `meta.json` sidecar is ever written beside their `*.md`.
/// Returns `true` when `dir`'s own name is `qa` or `review`. The walk still
/// descends into them (their `*.md` are still counted as `total`), but the
/// sidecar write is skipped.
fn is_phase_dir(dir: &Path) -> bool {
    matches!(
        dir.file_name().and_then(|n| n.to_str()),
        Some("qa" | "review")
    )
}

fn collect_md_into(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = &entry.path;
        if entry.is_dir {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if is_ignored(name) {
                continue;
            }
            collect_md_into(path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path.clone());
        }
    }
}

/// Extract a `Meta` from one spec markdown body using the canonical
/// [`mustard_core::domain::spec::header_field`] + [`parse_state`] readers, plus a
/// few sidecar-only fields (`Phase`, `Scope`, `Lang`, `Checkpoint`, `Parent`,
/// `Total waves`).
///
/// Returns `None` when no lifecycle header at all is present (the file is not
/// a spec — likely a README or note).
fn extract_meta(content: &str) -> Option<Meta> {
    let state = spec::parse_state(content);
    let stage = state.as_ref().map(|s| spec::stage_label(s.stage).to_string());
    let outcome = state.as_ref().map(|s| spec::outcome_label(s.outcome).to_string());
    // Carry the qualifier flags (`### Flags:` legacy header → `meta.json#flags`).
    let flags = state
        .as_ref()
        .map(|s| mustard_core::MetaFlags(s.flags.clone()))
        .unwrap_or_default();

    let phase = spec::header_field(content, "Phase");
    let scope = spec::header_field(content, "Scope");
    let lang = spec::header_field(content, "Lang");
    let checkpoint = spec::header_field(content, "Checkpoint");
    let parent = spec::header_field(content, "Parent");
    let total_waves_raw = spec::header_field(content, "Total waves");
    let total_waves: Option<u32> = total_waves_raw
        .as_deref()
        .and_then(|s| s.trim().parse::<u32>().ok());

    // If nothing matched at all the file is not a spec — skip.
    if state.is_none()
        && phase.is_none()
        && scope.is_none()
        && lang.is_none()
        && checkpoint.is_none()
        && parent.is_none()
        && total_waves.is_none()
    {
        return None;
    }

    // The wave-plan markdown is the only file that carries a `### Total waves:`
    // header; treat presence as the `isWavePlan` signal.
    let is_wave_plan = total_waves.map(|_| true);

    Some(Meta {
        stage,
        outcome,
        phase,
        scope,
        lang: lang.map(|l| mustard_core::normalise_lang(&l)),
        checkpoint,
        parent,
        is_wave_plan,
        total_waves,
        flags,
        raw: serde_json::Value::Null,
    })
}

/// Rewrite `content` so every line that matches one of the legacy `### Key:`
/// lifecycle headers (now living in `meta.json`) is removed. Operates on
/// whole lines via [`str::split_inclusive`] so CRLF terminators and accented
/// UTF-8 bodies are preserved byte-for-byte. Collapses adjacent blank lines
/// left behind by the cull. Returns the rewritten body.
///
/// Keys removed: `Stage`, `Outcome`, `Phase`, `Scope`, `Lang`, `Checkpoint`,
/// `Parent`, `Flags`, `Total waves`, and the legacy `Status`. Limited to the
/// header region (everything before the first `## `/```` ``` ````/`~~~`) so a
/// spec that documents the new format in its body is not damaged.
fn strip_header_lines(content: &str) -> String {
    const KEYS: &[&str] = &[
        "Stage", "Outcome", "Phase", "Scope", "Lang", "Checkpoint", "Parent",
        "Flags", "Total waves", "Status",
    ];
    let region = mustard_core::header_region_lines(content);
    let mut out = String::with_capacity(content.len());
    let mut prev_was_blank = false;
    for (idx, line_with_terminator) in content.split_inclusive('\n').enumerate() {
        let in_region = idx < region;
        let trimmed = line_with_terminator.trim_start();
        if in_region {
            let mut matched = false;
            for key in KEYS {
                if let Some(after) = strip_h3_key(trimmed, key) {
                    let _ = after;
                    matched = true;
                    break;
                }
            }
            if matched {
                continue;
            }
        }
        // Collapse runs of blank lines that the removals leave behind.
        let is_blank = line_with_terminator.trim().is_empty();
        if is_blank && prev_was_blank && in_region {
            continue;
        }
        prev_was_blank = is_blank;
        out.push_str(line_with_terminator);
    }
    out
}

/// `### <Key>:` recogniser (case-insensitive on the key). Returns the value
/// substring on a match, `None` otherwise. Restricted to the level-3 ATX form;
/// the bullet `- **Key**:` shape is left untouched — it is body content, not a
/// canonical header.
fn strip_h3_key(line: &str, key: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = t.strip_prefix("###")?;
    let rest = rest.trim_start();
    let want = key.to_ascii_lowercase();
    let lower = rest.to_ascii_lowercase();
    let after_key = lower.strip_prefix(&want)?;
    let after_key_trim = after_key.trim_start();
    let after_colon = after_key_trim.strip_prefix(':')?;
    let value_start = rest.len() - after_colon.len();
    rest.get(value_start..).map(|v| v.trim().to_string())
}

/// `mustard-rt run migrate-to-meta`.
///
/// Walks `<root>` recursively, extracts lifecycle metadata from each `*.md`,
/// and writes `meta.json` beside it. Idempotent: an existing `meta.json` is
/// only overwritten with `--force`.
pub fn run(opts: MigrateToMetaOpts) {
    let cwd = env_project_dir();
    let root = match opts.root.clone() {
        Some(p) => p,
        None => match ClaudePaths::for_project(Path::new(&cwd)) {
            Ok(paths) => paths.spec_dir(),
            Err(_) => {
                // Invalid project root (I1 guard) — emit an empty report and bail.
                let report = MigrationReport {
                    ran_at: now_iso8601(),
                    root: String::new(),
                    total: 0,
                    written: 0,
                    already_present: 0,
                    skipped_no_header: 0,
                    errors: 1,
                    headers_stripped: 0,
                    files: vec![FileRecord {
                        path: cwd.clone(),
                        action: "error",
                        reason: Some("invalid project root (claude-paths guard)".to_string()),
                    }],
                };
                let body = serde_json::to_string_pretty(&report)
                    .unwrap_or_else(|_| json!({"error":"serialize failed"}).to_string());
                println!("{body}");
                return;
            }
        },
    };

    let files = collect_md(&root);

    let mut records: Vec<FileRecord> = Vec::new();
    let mut written = 0usize;
    let mut already_present = 0usize;
    let mut skipped_no_header = 0usize;
    let mut errors = 0usize;
    let mut headers_stripped = 0usize;

    for md_path in &files {
        let rel = md_path.display().to_string();

        // The sidecar always lands as `meta.json` beside the source `.md`.
        let sidecar = match md_path.parent() {
            // D3: never write a sidecar inside a `qa/` or `review/` phase dir.
            Some(parent) if is_phase_dir(parent) => {
                skipped_no_header += 1;
                records.push(FileRecord {
                    path: rel,
                    action: "skipped",
                    reason: Some("phase directory (qa/review) carries no lifecycle".to_string()),
                });
                continue;
            }
            Some(parent) => parent.join("meta.json"),
            None => {
                errors += 1;
                records.push(FileRecord {
                    path: rel,
                    action: "error",
                    reason: Some("no parent directory".to_string()),
                });
                continue;
            }
        };

        let sidecar_present = fs::exists(&sidecar);

        // Read the markdown once — needed for both meta extraction and the
        // optional header-strip pass.
        let content = match fs::read_to_string(md_path) {
            Ok(c) => c,
            Err(e) => {
                errors += 1;
                records.push(FileRecord {
                    path: rel,
                    action: "error",
                    reason: Some(format!("read failed: {e}")),
                });
                continue;
            }
        };

        // ----- Sidecar write (idempotent, --force overrides) -----
        if !sidecar_present || opts.force {
            let Some(meta) = extract_meta(&content) else {
                skipped_no_header += 1;
                records.push(FileRecord {
                    path: rel.clone(),
                    action: "skipped",
                    reason: Some("no lifecycle header".to_string()),
                });
                // Still attempt header strip below if requested — but only on
                // a file that *had* no header there is nothing to strip.
                continue;
            };

            if let Err(e) = write_meta(&sidecar, &meta) {
                errors += 1;
                records.push(FileRecord {
                    path: sidecar.display().to_string(),
                    action: "error",
                    reason: Some(format!("write failed: {e}")),
                });
                continue;
            }
            written += 1;
            records.push(FileRecord {
                path: sidecar.display().to_string(),
                action: "written",
                reason: None,
            });
        } else {
            already_present += 1;
            records.push(FileRecord {
                path: sidecar.display().to_string(),
                action: "already-present",
                reason: None,
            });
        }

        // ----- Header strip (opt-in, T3.4 cleanup pass) -----
        if opts.strip_headers {
            let stripped = strip_header_lines(&content);
            if stripped != content {
                if let Err(e) = fs::write_atomic(md_path, stripped.as_bytes()) {
                    errors += 1;
                    records.push(FileRecord {
                        path: rel.clone(),
                        action: "error",
                        reason: Some(format!("strip-headers write failed: {e}")),
                    });
                    continue;
                }
                headers_stripped += 1;
                records.push(FileRecord {
                    path: rel.clone(),
                    action: "headers-stripped",
                    reason: None,
                });
            }
        }
    }

    let report = MigrationReport {
        ran_at: now_iso8601(),
        root: root.display().to_string(),
        total: files.len(),
        written,
        already_present,
        skipped_no_header,
        errors,
        headers_stripped,
        files: records,
    };

    let body = serde_json::to_string_pretty(&report)
        .unwrap_or_else(|_| json!({"error":"serialize failed"}).to_string());
    println!("{body}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn extracts_canonical_meta_from_new_header() {
        let md = "# Spec X\n### Stage: Execute\n### Outcome: Active\n### Flags: \n\
                  ### Phase: EXECUTE\n### Scope: full\n### Lang: pt-BR\n\
                  ### Checkpoint: 2026-05-24T19:30:00Z\n\nbody\n";
        let m = extract_meta(md).expect("parses");
        assert_eq!(m.stage.as_deref(), Some("Execute"));
        assert_eq!(m.outcome.as_deref(), Some("Active"));
        assert_eq!(m.phase.as_deref(), Some("EXECUTE"));
        assert_eq!(m.scope.as_deref(), Some("full"));
        assert_eq!(m.lang.as_deref(), Some("pt-BR"));
        assert_eq!(m.checkpoint.as_deref(), Some("2026-05-24T19:30:00Z"));
    }

    #[test]
    fn extracts_parent_and_wave_plan_signals() {
        let md = "# Parent\n### Stage: Plan\n### Outcome: Active\n### Flags: \n\
                  ### Scope: full (wave plan)\n### Total waves: 6\n\nbody\n";
        let m = extract_meta(md).expect("parses");
        assert_eq!(m.total_waves, Some(6));
        assert_eq!(m.is_wave_plan, Some(true));

        let child = "# Child\n### Parent: 2026-05-24-parent\n### Stage: Plan\n\
                     ### Outcome: Active\n### Flags: \n\nbody\n";
        let mc = extract_meta(child).expect("parses");
        assert_eq!(mc.parent.as_deref(), Some("2026-05-24-parent"));
    }

    #[test]
    fn lang_short_code_normalises_on_extract() {
        let md = "# S\n### Stage: Plan\n### Outcome: Active\n### Flags: \n\
                  ### Lang: pt\n\nbody\n";
        let m = extract_meta(md).expect("parses");
        assert_eq!(m.lang.as_deref(), Some("pt-BR"));
    }

    #[test]
    fn non_spec_yields_none() {
        let md = "# Just a note\n\nBody without any lifecycle headers.\n";
        assert!(extract_meta(md).is_none());
    }

    #[test]
    fn writes_meta_alongside_spec() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("demo");
        let md_path = spec_dir.join("spec.md");
        write(
            &md_path,
            "# Demo\n### Stage: Plan\n### Outcome: Active\n### Flags: \n\
             ### Lang: pt-BR\n\nbody\n",
        );

        run(MigrateToMetaOpts {
            root: Some(dir.path().to_path_buf()),
            force: false,
            strip_headers: false,
        });

        let sidecar = spec_dir.join("meta.json");
        assert!(sidecar.exists(), "meta.json must be created");
        let body = std::fs::read_to_string(&sidecar).unwrap();
        let meta: Meta = serde_json::from_str(&body).unwrap();
        assert_eq!(meta.stage.as_deref(), Some("Plan"));
        assert_eq!(meta.lang.as_deref(), Some("pt-BR"));
    }

    #[test]
    fn idempotent_without_force() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("demo");
        let md_path = spec_dir.join("spec.md");
        write(
            &md_path,
            "# Demo\n### Stage: Plan\n### Outcome: Active\n### Flags: \n\nbody\n",
        );
        run(MigrateToMetaOpts {
            root: Some(dir.path().to_path_buf()),
            force: false,
            strip_headers: false,
        });
        let sidecar = spec_dir.join("meta.json");
        let first = std::fs::read_to_string(&sidecar).unwrap();

        // Second run: no force, sidecar already exists — must not change it.
        run(MigrateToMetaOpts {
            root: Some(dir.path().to_path_buf()),
            force: false,
            strip_headers: false,
        });
        let second = std::fs::read_to_string(&sidecar).unwrap();
        assert_eq!(first, second, "second run must be byte-identical");
    }

    #[test]
    fn skips_ignored_directories() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("node_modules").join("x");
        write(
            &nested.join("spec.md"),
            "# Stray\n### Stage: Plan\n### Outcome: Active\n### Flags: \n",
        );
        run(MigrateToMetaOpts {
            root: Some(dir.path().to_path_buf()),
            force: false,
            strip_headers: false,
        });
        assert!(!nested.join("meta.json").exists());
    }

    #[test]
    fn covers_wave_plan_layout() {
        // Parent + wave-1/spec.md + wave-2/spec.md + wave-plan.md.
        let dir = tempdir().unwrap();
        let parent = dir.path().join("epic-x");
        write(
            &parent.join("spec.md"),
            "# Epic X\n### Stage: Execute\n### Outcome: Active\n### Flags: \n",
        );
        write(
            &parent.join("wave-plan.md"),
            "# Wave Plan\n### Stage: Plan\n### Outcome: Active\n### Flags: \n\
             ### Total waves: 2\n",
        );
        write(
            &parent.join("wave-1-general").join("spec.md"),
            "# Wave 1\n### Parent: epic-x\n### Stage: Plan\n### Outcome: Active\n### Flags: \n",
        );
        write(
            &parent.join("wave-2-general").join("spec.md"),
            "# Wave 2\n### Parent: epic-x\n### Stage: Plan\n### Outcome: Active\n### Flags: \n",
        );

        run(MigrateToMetaOpts {
            root: Some(dir.path().to_path_buf()),
            force: false,
            strip_headers: false,
        });

        assert!(parent.join("meta.json").exists());
        assert!(parent.join("wave-1-general").join("meta.json").exists());
        assert!(parent.join("wave-2-general").join("meta.json").exists());

        // wave-plan.md and spec.md share a directory; both populate the same
        // `meta.json`. The walker visits them in order — the first writes and
        // subsequent ones short-circuit on `already-present`.
        let plan_meta_body = std::fs::read_to_string(parent.join("meta.json")).unwrap();
        let parsed: Meta = serde_json::from_str(&plan_meta_body).unwrap();
        // The wave-plan and spec.md share `# Epic X` directory; the first md
        // file in alphabetical order is `spec.md` (s after w in collation:
        // wait — 's' before 'w'). So spec.md writes the sidecar.
        assert!(parsed.stage.is_some());
    }

    #[test]
    fn skips_phase_dirs_qa_and_review() {
        // D3: a `qa/spec.md` / `review/spec.md` carrying a (legacy) lifecycle
        // header must NOT get a `meta.json` sidecar — they are phases, not specs.
        let dir = tempdir().unwrap();
        let parent = dir.path().join("epic-x");
        write(
            &parent.join("spec.md"),
            "# Epic X\n### Stage: Execute\n### Outcome: Active\n### Flags: \n",
        );
        write(
            &parent.join("qa").join("spec.md"),
            "# QA\n### Stage: Plan\n### Outcome: Active\n### Flags: \n",
        );
        write(
            &parent.join("review").join("spec.md"),
            "# Review\n### Stage: Plan\n### Outcome: Active\n### Flags: \n",
        );

        run(MigrateToMetaOpts {
            root: Some(dir.path().to_path_buf()),
            force: false,
            strip_headers: false,
        });

        // Root spec gets a sidecar; the two phase dirs do NOT.
        assert!(parent.join("meta.json").exists());
        assert!(!parent.join("qa").join("meta.json").exists());
        assert!(!parent.join("review").join("meta.json").exists());
    }

    #[test]
    fn strip_headers_removes_legacy_lines_after_meta_write() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("demo");
        let md_path = spec_dir.join("spec.md");
        write(
            &md_path,
            "# Demo\n### Stage: Plan\n### Outcome: Active\n### Flags: \n\
             ### Phase: PLAN\n### Scope: full\n### Lang: pt-BR\n\
             ### Checkpoint: 2026-05-24T19:30:00Z\n### Parent: parent-x\n\n\
             ## Tarefas\n\n- [ ] One\n",
        );

        run(MigrateToMetaOpts {
            root: Some(dir.path().to_path_buf()),
            force: false,
            strip_headers: true,
        });

        // meta.json was created.
        let sidecar = spec_dir.join("meta.json");
        assert!(sidecar.exists());

        // Legacy headers gone from the markdown.
        let after = std::fs::read_to_string(&md_path).unwrap();
        for needle in [
            "### Stage:",
            "### Outcome:",
            "### Flags:",
            "### Phase:",
            "### Scope:",
            "### Lang:",
            "### Checkpoint:",
            "### Parent:",
        ] {
            assert!(!after.contains(needle), "{needle} still present:\n{after}");
        }
        // Body preserved.
        assert!(after.contains("## Tarefas"));
        assert!(after.contains("- [ ] One"));
    }

    #[test]
    fn strip_header_lines_is_idempotent() {
        let body = "# Spec\n\n## Body\nplain content\n";
        let stripped = strip_header_lines(body);
        assert_eq!(stripped, body);
    }

    #[test]
    fn strip_header_lines_preserves_body_mentions() {
        // A `### Stage:` mentioned in the body (after `## `) MUST be preserved.
        let body = "# Spec\n### Stage: Plan\n\n## Notes\n### Stage: Example\nbody\n";
        let stripped = strip_header_lines(body);
        // First (header-region) line gone, second (body) line preserved.
        assert!(!stripped.starts_with("# Spec\n### Stage:"));
        assert!(stripped.contains("## Notes\n### Stage: Example"));
    }
}
