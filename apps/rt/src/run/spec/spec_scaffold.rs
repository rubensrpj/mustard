//! Shared scaffold helpers — write `spec.md`, `meta.json`, and atomically
//! synchronise lifecycle headers across both files.
//!
//! Extracted from `spec_draft.rs` so `tactical_fix_create` (and any future
//! subcommand) can produce spec artifacts without duplicating the write logic.
//!
//! ## Public surface
//!
//! | Function | Purpose |
//! |---|---|
//! | `write_spec_md` | Render and write `spec.md` from a [`SpecInput`]. |
//! | `write_meta_json` | Write `meta.json` from a pre-built [`Meta`]. |
//! | `sync_status` | Atomically rewrite lifecycle headers in both files. |

use mustard_core::fs as mfs;
use mustard_core::meta::{write_meta, Meta};
use mustard_core::spec::contract::{SpecInput, PLAN_DIVIDER, PRD_DIVIDER};
use mustard_core::spec;
use mustard_core::{read_meta, Outcome, Scope, SpecState, Stage};
use mustard_core::i18n::{translate, Locale, Tone};
use std::fmt::Write as _;
use std::path::Path;

// ---------------------------------------------------------------------------
// spec.md writer
// ---------------------------------------------------------------------------

/// Render `spec.md` with the canonical layout dividers + sections and write it
/// atomically under `output/spec.md`. Delegates section-heading localisation to
/// `section_heading_for` and uses `translate` for all user-facing copy.
///
/// Caller is responsible for contract-validating `input` before calling; this
/// function is write-only and fails via `Err(String)` on I/O errors.
pub fn write_spec_md(
    output: &Path,
    input: &SpecInput,
    signals: &Option<String>,
    lang: Locale,
    tone: Tone,
) -> Result<(), String> {
    let mut body = String::new();
    let _ = write!(body, "# {}\n\n", input.title);
    // Drafter tone hint — picked up by the LLM that fleshes out section bodies.
    // Hidden in an HTML comment so it never renders in rendered markdown.
    let _ = writeln!(
        body,
        "<!-- drafter:tone={tone} — {instruction} -->",
        tone = tone.as_str(),
        instruction = crate::run::spec::spec_draft::tone_prompt_instruction(tone),
    );
    body.push_str("### Stage: Plan\n### Outcome: Active\n### Flags: \n\n");
    body.push_str(PRD_DIVIDER);
    body.push('\n');
    for s in &input.prd_sections {
        let heading = section_heading_for(&s.name, lang);
        let _ = write!(body, "\n## {heading}\n\n{}\n", s.body);
    }
    let _ = write!(body, "\n## {}\n\n", translate("heading.spec.ac_list", lang));
    for ac in &input.acceptance_criteria {
        let _ = write!(
            body,
            "- **{id}** — {stmt}\n  Command: `{cmd}`\n",
            id = ac.id,
            stmt = ac.statement,
            cmd = ac.command
        );
    }
    if matches!(input.scope, Some(Scope::Full)) {
        body.push('\n');
        body.push_str(PLAN_DIVIDER);
        body.push('\n');
        for s in &input.plan_sections {
            let heading = section_heading_for(&s.name, lang);
            let _ = write!(body, "\n## {heading}\n\n{}\n", s.body);
        }
    }
    if let Some(sigs) = signals {
        if !sigs.trim().is_empty() {
            let _ = write!(body, "\n<!-- signals: {} -->\n", sigs.trim());
        }
    }
    let path = output.join("spec.md");
    mfs::write_atomic(&path, body.as_bytes()).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// meta.json writer
// ---------------------------------------------------------------------------

/// Write a pre-built [`Meta`] document as `meta.json` under `output/`.
/// Atomic — uses [`write_meta`] which writes to a temp file then renames.
pub fn write_meta_json(output: &Path, meta: &Meta) -> Result<(), String> {
    write_meta(&output.join("meta.json"), meta).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// sync_status — atomic two-file header sync
// ---------------------------------------------------------------------------

/// Atomically synchronise the lifecycle headers in **both** `spec.md` and
/// `meta.json` to the given `stage` + `outcome`.
///
/// Behaviour:
/// - `spec.md` is rewritten via [`mustard_core::spec::write_state`], which
///   normalises any legacy `### Status:` / `### Phase:` lines to the canonical
///   `### Stage:` / `### Outcome:` / `### Flags:` triple.
/// - `meta.json` is read (fail-open to a zero-value [`Meta`] when absent),
///   `stage`/`outcome`/`checkpoint` are updated, and the document is written
///   back atomically — all other fields are preserved.
///
/// Either file that does not yet exist is created. A missing spec directory
/// is treated as a no-op (the directory is never created; the caller is
/// responsible for directory setup). Errors for each file are independent:
/// a failed `spec.md` write returns `Err` immediately; a failed `meta.json`
/// write is returned as a second `Err` after the `spec.md` write succeeds.
///
/// # Errors
///
/// Returns the first I/O error encountered, annotated with the offending path.
pub fn sync_status(stage: Stage, outcome: Outcome, spec_path: &Path) -> Result<(), String> {
    // `spec_path` is the path to `spec.md` (or the spec directory — resolve).
    let spec_md = if spec_path.is_dir() {
        spec_path.join("spec.md")
    } else {
        spec_path.to_path_buf()
    };
    let spec_dir = if spec_path.is_dir() {
        spec_path.to_path_buf()
    } else {
        spec_path.parent().map(Path::to_path_buf).unwrap_or_else(|| spec_path.to_path_buf())
    };

    // Guard: if the spec directory does not exist, skip silently (fail-open).
    if !spec_dir.is_dir() {
        return Ok(());
    }

    // Build the SpecState with default (empty) flags.
    let state = SpecState::new(stage, outcome, mustard_core::Flags::default())
        .unwrap_or(SpecState { stage, outcome, flags: mustard_core::Flags::default() });

    // 1. Rewrite spec.md header.
    if spec_md.exists() {
        spec::write_state(&spec_md, &state)
            .map_err(|e| format!("sync_status: write spec.md ({}): {e}", spec_md.display()))?;
    }

    // 2. Patch meta.json (preserve all other fields).
    let meta_path = spec_dir.join("meta.json");
    let mut meta = read_meta(&meta_path).unwrap_or_default();
    meta.stage = Some(spec::stage_label(stage).to_string());
    meta.outcome = Some(spec::outcome_label(outcome).to_string());
    // Checkpoint is updated to "now" so collaborators can detect drift by ts.
    meta.checkpoint = Some(crate::util::now_iso8601());
    write_meta(&meta_path, &meta)
        .map_err(|e| format!("sync_status: write meta.json ({}): {e}", meta_path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers (shared with spec_draft via pub re-export)
// ---------------------------------------------------------------------------

/// Translate a canonical (PT-BR) section name into the user-facing heading
/// for the active locale. Identical to the private helper in `spec_draft` —
/// extracted here to avoid duplication.
pub fn section_heading_for(canonical: &str, lang: Locale) -> String {
    let key = match canonical {
        "Contexto" => "heading.spec.context",
        "Usuários" => "heading.spec.users",
        "Métrica" => "heading.spec.metric",
        "Não-Objetivos" => "heading.spec.non_goals",
        "Critérios de Aceitação" => "heading.spec.ac",
        "Arquivos" => "heading.spec.files",
        "Tarefas" => "heading.spec.tasks",
        "Limites" => "heading.spec.limits",
        _ => return canonical.to_string(),
    };
    translate(key, lang).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::meta::Meta;
    use tempfile::tempdir;

    fn make_meta(stage: &str, outcome: &str) -> Meta {
        Meta {
            stage: Some(stage.to_string()),
            outcome: Some(outcome.to_string()),
            phase: None,
            scope: None,
            lang: None,
            checkpoint: None,
            parent: None,
            is_wave_plan: None,
            total_waves: None,
            raw: serde_json::Value::Null,
        }
    }

    #[test]
    fn write_meta_json_creates_file() {
        let dir = tempdir().unwrap();
        let meta = make_meta("Plan", "Active");
        write_meta_json(dir.path(), &meta).unwrap();
        let path = dir.path().join("meta.json");
        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"stage\""));
        assert!(body.contains("Plan"));
    }

    #[test]
    fn sync_status_creates_meta_when_absent() {
        let dir = tempdir().unwrap();
        // spec.md does not exist — sync_status must not create spec.md
        // (guard: only patches when spec_md exists).
        let spec_md_path = dir.path().join("spec.md");
        sync_status(Stage::Execute, Outcome::Active, dir.path()).unwrap();
        // meta.json was created.
        let meta_path = dir.path().join("meta.json");
        assert!(meta_path.exists());
        // spec.md was NOT created (it didn't exist).
        assert!(!spec_md_path.exists());
        // meta fields correct.
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
        assert_eq!(v["stage"], serde_json::json!("Execute"));
        assert_eq!(v["outcome"], serde_json::json!("Active"));
    }

    #[test]
    fn sync_status_patches_spec_md_and_meta() {
        let dir = tempdir().unwrap();
        // Seed spec.md with legacy header.
        std::fs::write(
            dir.path().join("spec.md"),
            b"# My Spec\n\n### Status: implementing\n\n## Body\ncontent\n",
        )
        .unwrap();
        // Seed meta.json with Plan/Active.
        write_meta_json(dir.path(), &make_meta("Plan", "Active")).unwrap();

        sync_status(Stage::Close, Outcome::Completed, dir.path()).unwrap();

        let spec_body = std::fs::read_to_string(dir.path().join("spec.md")).unwrap();
        assert!(spec_body.contains("### Stage: Close"), "{spec_body}");
        assert!(spec_body.contains("### Outcome: Completed"), "{spec_body}");
        assert!(!spec_body.contains("### Status:"));

        let meta_v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(meta_v["stage"], serde_json::json!("Close"));
        assert_eq!(meta_v["outcome"], serde_json::json!("Completed"));
    }

    #[test]
    fn sync_status_preserves_other_meta_fields() {
        let dir = tempdir().unwrap();
        // Meta with extra fields (scope, lang, total_waves).
        let mut meta = make_meta("Plan", "Active");
        meta.scope = Some("full".to_string());
        meta.lang = Some("pt-BR".to_string());
        meta.total_waves = Some(3);
        write_meta_json(dir.path(), &meta).unwrap();

        sync_status(Stage::Execute, Outcome::Active, dir.path()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(v["scope"], serde_json::json!("full"));
        assert_eq!(v["lang"], serde_json::json!("pt-BR"));
        assert_eq!(v["totalWaves"], serde_json::json!(3));
        assert_eq!(v["stage"], serde_json::json!("Execute"));
    }

    #[test]
    fn sync_status_noop_when_dir_missing() {
        let dir = tempdir().unwrap();
        // Passing a non-existent subdirectory must not panic or create anything.
        let ghost = dir.path().join("ghost");
        let result = sync_status(Stage::Plan, Outcome::Active, &ghost);
        assert!(result.is_ok());
        assert!(!ghost.exists());
    }

    /// AC-W1.3 — a wave dir with Plan/Active headers; after sync_status(Close,
    /// Completed), both spec.md and meta.json carry Close/Completed.
    #[test]
    fn sync_status_wave_complete() {
        let dir = tempdir().unwrap();
        // Seed wave spec.md with Plan/Active.
        std::fs::write(
            dir.path().join("spec.md"),
            b"# Wave 1\n\n### Stage: Plan\n### Outcome: Active\n### Flags: \n\n## Body\nwork\n",
        )
        .unwrap();
        write_meta_json(dir.path(), &make_meta("Plan", "Active")).unwrap();

        sync_status(Stage::Close, Outcome::Completed, dir.path()).unwrap();

        let spec_body = std::fs::read_to_string(dir.path().join("spec.md")).unwrap();
        assert!(spec_body.contains("### Stage: Close"), "{spec_body}");
        assert!(spec_body.contains("### Outcome: Completed"), "{spec_body}");

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(v["stage"], serde_json::json!("Close"));
        assert_eq!(v["outcome"], serde_json::json!("Completed"));
    }

    #[test]
    fn section_heading_for_localises() {
        assert_eq!(section_heading_for("Contexto", Locale::EnUs), "Context");
        assert_eq!(section_heading_for("Contexto", Locale::PtBr), "Contexto");
        // Unknown keys pass through.
        assert_eq!(section_heading_for("Custom", Locale::EnUs), "Custom");
    }
}
