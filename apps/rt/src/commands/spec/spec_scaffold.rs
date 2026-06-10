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

use mustard_core::io::fs as mfs;
use mustard_core::domain::meta::{write_meta, Meta, MetaFlags};
use mustard_core::domain::spec::contract::{
    render_checklist_item, SpecInput, CHECKLIST_HEADING, PLAN_DIVIDER, PRD_DIVIDER,
};
use mustard_core::domain::spec;
use mustard_core::{read_meta, Scope, SpecState};
use mustard_core::platform::i18n::{translate, Locale, Tone};
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
        instruction = crate::commands::spec::spec_draft::tone_prompt_instruction(tone),
    );
    // No lifecycle header block — `meta.json` is the single source of every
    // machine-parseable field (stage/outcome/flags/scope/lang/...). `spec.md`
    // is pure PRD/plan narrative.
    body.push('\n');
    body.push_str(PRD_DIVIDER);
    body.push('\n');
    for s in &input.prd_sections {
        // Single-emitter rule (TF 2026-06-10-ac-heading-unico): the AC list
        // block below is the ONLY emitter of the AC heading. The PRD entry
        // stays in `SpecInput` purely for the contract's presence+order check
        // (`check_sections`) — rendering it too duplicated the heading
        // (placeholder body first, real list second), and every
        // `section_block` reader captured the placeholder: a virgin draft
        // failed its own analyze-validation (`unparseable-ac`). Same skip
        // pattern as the wave-plan `tasks` suppression below.
        if s.name.trim().eq_ignore_ascii_case("acceptance-criteria") {
            continue;
        }
        let heading = section_heading_for(&s.name, lang);
        let _ = write!(body, "\n## {heading}\n\n{}\n", s.body);
    }
    let _ = write!(body, "\n## {}\n\n", section_heading_for("acceptance-criteria", lang));
    for ac in &input.acceptance_criteria {
        let _ = write!(
            body,
            "- **{id}** — {stmt}\n  Command: `{cmd}`\n",
            id = ac.id,
            stmt = ac.statement,
            cmd = ac.command
        );
    }
    // A wave-plan *parent* (`total_waves` ≥ 1) is a coordination document: its
    // actionable `## Tarefas` (the agent roadmap) and `## Checklist` (the
    // close-gate's auto-mark target) live in the WAVES, not in the parent. We
    // detect it from the same signal core uses to exempt it from the
    // `ChecklistEmpty` contract rule (`contract.rs::validate`). A non-decomposed
    // Full spec and every Light spec keep BOTH blocks.
    let is_wave_plan = input.total_waves.unwrap_or(0) >= 1;
    if matches!(input.scope, Some(Scope::Full)) {
        body.push('\n');
        body.push_str(PLAN_DIVIDER);
        body.push('\n');
        for s in &input.plan_sections {
            // D1: the wave-plan parent carries no `## Tarefas` — the roadmap
            // belongs to each wave's own spec.md.
            if is_wave_plan && s.name.trim().eq_ignore_ascii_case("tasks") {
                continue;
            }
            let heading = section_heading_for(&s.name, lang);
            let _ = write!(body, "\n## {heading}\n\n{}\n", s.body);
        }
    }
    // Trackable `## Checklist` — emitted for every scope EXCEPT a wave-plan
    // parent, so the close-gate checklist gate is never orphaned. The heading is
    // the EN-only `CHECKLIST_HEADING` (language-agnostic) so the auto-mark hook,
    // `mark-checklist-item`, and close-gate all key off the exact same literal;
    // each line is rendered via `render_checklist_item` into the canonical
    // `- [ ] <label> → <path>` shape those consumers parse. The wave-plan parent
    // is suppressed because its checklist lives in the waves (the close-gate's
    // `find_unmarked_checklist` consolidates the wave checklists in that case).
    if !is_wave_plan {
        let _ = write!(body, "\n## {CHECKLIST_HEADING}\n\n");
        for item in &input.checklist {
            let _ = writeln!(body, "{}", render_checklist_item(item));
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

/// Atomically synchronise the lifecycle metadata to the given [`SpecState`]
/// (`stage` + `outcome` + `flags`) by patching **`meta.json`** — the single
/// source of truth. The `spec.md` narrative is never touched: it carries no
/// lifecycle header.
///
/// Behaviour:
/// - `meta.json` is read (fail-open to a zero-value [`Meta`] when absent),
///   `stage`/`outcome`/`flags`/`checkpoint` are updated, and the document is
///   written back atomically — all other fields are preserved.
/// - `flags` are mapped from the validated [`SpecState`] (its `SpecState::new`
///   invariants — terminal outcome ⇒ Close, `followup_open` ⇒ Close+Active —
///   already hold by construction), so the `meta.json#flags` token array stays
///   the canonical mirror of `SpecState.flags`.
///
/// A missing spec directory is treated as a no-op (the directory is never
/// created; the caller is responsible for directory setup).
///
/// # Errors
///
/// Returns the I/O error encountered, annotated with the offending path.
pub fn sync_status(state: SpecState, spec_path: &Path) -> Result<(), String> {
    // `spec_path` is the path to `spec.md` (or the spec directory — resolve).
    let spec_dir = if spec_path.is_dir() {
        spec_path.to_path_buf()
    } else {
        spec_path.parent().map(Path::to_path_buf).unwrap_or_else(|| spec_path.to_path_buf())
    };

    // Guard: if the spec directory does not exist, skip silently (fail-open).
    if !spec_dir.is_dir() {
        return Ok(());
    }

    // Patch meta.json (preserve all other fields). `meta.json` is the single
    // home of every machine-parseable lifecycle field — `spec.md` is left as
    // pure narrative.
    let meta_path = spec_dir.join("meta.json");
    let mut meta = read_meta(&meta_path).unwrap_or_default();
    meta.stage = Some(spec::stage_label(state.stage).to_string());
    meta.outcome = Some(spec::outcome_label(state.outcome).to_string());
    meta.flags = MetaFlags(state.flags);
    // Checkpoint is updated to "now" so collaborators can detect drift by ts.
    meta.checkpoint = Some(mustard_core::time::now_iso8601());
    write_meta(&meta_path, &meta)
        .map_err(|e| format!("sync_status: write meta.json ({}): {e}", meta_path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers (shared with spec_draft via pub re-export)
// ---------------------------------------------------------------------------

/// Translate a canonical (EN, language-agnostic) section key into the
/// user-facing display heading for the active locale.
///
/// The canonical keys are the kebab-case EN identifiers in
/// [`mustard_core::domain::spec::contract::PRD_SECTIONS`] /
/// [`PLAN_SECTIONS`](mustard_core::domain::spec::contract::PLAN_SECTIONS).
/// The localised heading is the only place the user's natural `language`
/// surfaces in a spec; everything else stays EN. The match is
/// case-insensitive on the key so a `Context`-cased body name still resolves.
/// An unrecognised key passes through unchanged (fail-open).
pub fn section_heading_for(canonical: &str, lang: Locale) -> String {
    let key = match canonical.trim().to_ascii_lowercase().as_str() {
        "context" => "heading.spec.context",
        "users" => "heading.spec.users",
        "metric" => "heading.spec.metric",
        "non-goals" => "heading.spec.non_goals",
        "acceptance-criteria" => "heading.spec.ac",
        "files" => "heading.spec.files",
        "tasks" => "heading.spec.tasks",
        "boundaries" => "heading.spec.limits",
        _ => return canonical.to_string(),
    };
    translate(key, lang).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::meta::Meta;
    use tempfile::tempdir;

    use mustard_core::{Flags, Outcome, Stage};

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
            flags: MetaFlags::default(),
            checklist: Vec::new(),
            raw: serde_json::Value::Null,
        }
    }

    /// Build a validated `SpecState` for the scaffold tests.
    fn st(stage: Stage, outcome: Outcome) -> SpecState {
        SpecState::new(stage, outcome, Flags::default()).expect("legal state")
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
        sync_status(st(Stage::Execute, Outcome::Active), dir.path()).unwrap();
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
    fn sync_status_patches_meta_and_leaves_spec_md_untouched() {
        let dir = tempdir().unwrap();
        // Seed spec.md as pure narrative — no lifecycle header.
        let original = b"# My Spec\n\n## Body\ncontent\n";
        std::fs::write(dir.path().join("spec.md"), original).unwrap();
        // Seed meta.json with Plan/Active.
        write_meta_json(dir.path(), &make_meta("Plan", "Active")).unwrap();

        sync_status(st(Stage::Close, Outcome::Completed), dir.path()).unwrap();

        // spec.md is byte-for-byte unchanged — no header was injected.
        let spec_body = std::fs::read(dir.path().join("spec.md")).unwrap();
        assert_eq!(spec_body, original);

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

        sync_status(st(Stage::Execute, Outcome::Active), dir.path()).unwrap();

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
        let result = sync_status(st(Stage::Plan, Outcome::Active), &ghost);
        assert!(result.is_ok());
        assert!(!ghost.exists());
    }

    /// AC-W1.3 — a wave dir at Plan/Active; after sync_status(Close,
    /// Completed), meta.json carries Close/Completed and spec.md stays narrative.
    #[test]
    fn sync_status_wave_complete() {
        let dir = tempdir().unwrap();
        // Seed wave spec.md as pure narrative.
        std::fs::write(
            dir.path().join("spec.md"),
            b"# Wave 1\n\n## Body\nwork\n",
        )
        .unwrap();
        write_meta_json(dir.path(), &make_meta("Plan", "Active")).unwrap();

        sync_status(st(Stage::Close, Outcome::Completed), dir.path()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(v["stage"], serde_json::json!("Close"));
        assert_eq!(v["outcome"], serde_json::json!("Completed"));
    }

    /// A followup state (Close + Active + followup_open) flows into the
    /// `meta.json#flags` token array via `sync_status`.
    #[test]
    fn sync_status_writes_followup_flag_to_meta() {
        let dir = tempdir().unwrap();
        write_meta_json(dir.path(), &make_meta("Execute", "Active")).unwrap();
        let followup = SpecState::new(
            Stage::Close,
            Outcome::Active,
            Flags { followup_open: true, ..Flags::default() },
        )
        .unwrap();
        sync_status(followup, dir.path()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("meta.json")).unwrap())
                .unwrap();
        assert_eq!(v["stage"], serde_json::json!("Close"));
        assert_eq!(v["flags"], serde_json::json!(["followup_open"]));
    }

    #[test]
    fn section_heading_for_localises() {
        // Canonical EN keys map to the localised display heading.
        assert_eq!(section_heading_for("context", Locale::EnUs), "Context");
        assert_eq!(section_heading_for("context", Locale::PtBr), "Contexto");
        // Case-insensitive on the key.
        assert_eq!(section_heading_for("Context", Locale::EnUs), "Context");
        assert_eq!(section_heading_for("acceptance-criteria", Locale::EnUs), "Acceptance Criteria");
        assert_eq!(section_heading_for("boundaries", Locale::PtBr), "Limites");
        // Unknown keys pass through.
        assert_eq!(section_heading_for("custom", Locale::EnUs), "custom");
    }
}
