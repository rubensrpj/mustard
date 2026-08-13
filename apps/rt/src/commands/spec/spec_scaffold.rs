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

/// The unfilled `Control:` marker a fresh draft carries — the `<…>` shape
/// `qa_run::is_skeleton` recognises, so an unanswered control is never mistaken
/// for a control that ran.
///
/// English regardless of the narrative locale: like every `Command:` value, the
/// content is code the orchestrator replaces, not prose a reader consumes.
const AC_CONTROL_SKELETON: &str = "<a command that must be GREEN against the tree as it is today>";

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
    // Leading YAML frontmatter carrying ONLY a stable `id:` — the rename-proof
    // identity handle. `[[spec.{slug}]]` is simultaneously an Obsidian wikilink
    // AND a mustard-resolvable handle (`atomic_md::wikilink::resolve` prefers a
    // frontmatter `id:` over the filename). This is NOT a lifecycle field — the
    // "pure narrative" invariant forbids only `### Stage:`/`### Outcome:`/… which
    // live in `meta.json`; identity is allowed. The `{slug}` is the spec
    // directory name (the parent of `spec.md`). When the output dir has no file
    // name (defensive: never happens for a real spec dir) the block is omitted
    // so the document still parses.
    let slug = output
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if !slug.is_empty() {
        let _ = write!(body, "---\nid: spec.{slug}\n---\n\n");
    }
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
    let ac_total = input.acceptance_criteria.len();
    for (index, ac) in input.acceptance_criteria.iter().enumerate() {
        let _ = write!(
            body,
            "- **{id}** — {stmt}\n  Command: `{cmd}`\n",
            id = ac.id,
            stmt = ac.statement,
            cmd = ac.command
        );
        // The optional `Control:` key, offered on every criterion the negative
        // test will actually judge. It names a command that must come back
        // GREEN against the tree AS IT IS: a red `Command:` proves nothing on
        // its own, because a broken regex, a shell it cannot run under, a
        // missing binary and a quoting error all produce exactly the red an
        // honest criterion produces. A control that must be green TODAY rejects
        // all four with one run, here at PLAN time, where the fix costs one
        // edit.
        //
        // The trailing criterion is skipped through the SAME positional rule
        // the negative test applies (`ac_negative_check::is_exempt`) rather than
        // a second spelling of "which criterion is exempt": it is the
        // build-green safety net, green before the work by design, so it has
        // nothing to control for.
        //
        // The placeholder is ENGLISH regardless of the narrative locale, like
        // every other `Command:` value — the content is code, not prose.
        if !crate::commands::review::ac_negative_check::is_exempt(index, ac_total) {
            let _ = writeln!(body, "  Control: `{AC_CONTROL_SKELETON}`");
        }
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
///
/// One field is CARRIED OVER from whatever sidecar is already there: `base`,
/// the integration base the unit was actually cut from. Every caller here
/// builds its `Meta` from a spec INPUT, which cannot know that — only the cut
/// knows it, and the cut runs first (`spec-draft` cuts the branch before it
/// writes a byte, and the hook gate cuts it earlier still). A plain write would
/// therefore erase the one answer nothing else can reconstruct, since the
/// pending marker that carried it is consumed at the cut. An incoming `base` is
/// never overwritten — a caller that knows is the more recent measurement.
pub fn write_meta_json(output: &Path, meta: &Meta) -> Result<(), String> {
    let path = output.join("meta.json");
    let mut meta = meta.clone();
    if meta.base.is_none() {
        meta.base = read_meta(&path).and_then(|existing| existing.base);
    }
    write_meta(&path, &meta).map_err(|e| e.to_string())
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
            base: None,
            is_wave_plan: None,
            total_waves: None,
            flags: MetaFlags::default(),
            checklist: Vec::new(),
            findings: Vec::new(),
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

    /// `write_spec_md` prepends a leading YAML frontmatter block carrying ONLY a
    /// stable `id: spec.{slug}` — the rename-proof identity handle — while the
    /// body stays pure narrative (no lifecycle `### Stage:`/`### Outcome:`
    /// header). The `{slug}` is the output directory name.
    #[test]
    fn write_spec_md_prepends_id_frontmatter_and_stays_narrative() {
        use mustard_core::domain::spec::contract::SpecInput;
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("my-feature-slug");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let input = SpecInput {
            title: "My Feature".to_string(),
            ..SpecInput::default()
        };
        write_spec_md(&spec_dir, &input, &None, Locale::EnUs, Tone::default())
            .expect("write spec.md");
        let body = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        // Frontmatter is the very first bytes (the resolver requires `---\n`).
        assert!(
            body.starts_with("---\nid: spec.my-feature-slug\n---\n\n"),
            "leading id frontmatter missing:\n{body}"
        );
        // The H1 title still renders (pushed below the frontmatter).
        assert!(body.contains("# My Feature"), "{body}");
        // No lifecycle header leaked in — identity is allowed, lifecycle is not.
        assert!(!body.contains("### Stage:"), "{body}");
        assert!(!body.contains("### Outcome:"), "{body}");
        // Frontmatter carries ONLY `id:` — no other key.
        let fm_end = body.find("\n---\n").expect("frontmatter close");
        let fm = &body["---\n".len()..fm_end];
        assert_eq!(fm.trim(), "id: spec.my-feature-slug", "frontmatter must carry only id:");
    }

    /// A fresh draft OFFERS the `Control:` key on every criterion the negative
    /// test will actually judge — and the SHARED parser reads it back.
    ///
    /// Both halves matter, and the second is why this is one test rather than a
    /// string assertion: a key the drafter emits in a shape `qa_run` does not
    /// parse is a key that ships inert, which is the exact failure mode this
    /// spec exists to close.
    ///
    /// The trailing criterion is skipped through the negative test's own
    /// positional rule: it is the build-green safety net, green before the work
    /// by design, so it has nothing to control for.
    #[test]
    fn a_fresh_draft_offers_the_control_key_on_every_judged_criterion() {
        use crate::commands::review::qa_run::{extract_ac_section, parse_ac_items};
        use mustard_core::domain::spec::contract::{AcceptanceCriterion, SpecInput};
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("control-seed");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let ac = |id: &str, cmd: &str| AcceptanceCriterion {
            id: id.to_string(),
            statement: "when x, then y.".to_string(),
            command: cmd.to_string(),
        };
        let input = SpecInput {
            title: "Seed".to_string(),
            acceptance_criteria: vec![
                ac("AC-1", "cargo test foo"),
                ac("AC-2", "cargo test bar"),
                ac("AC-3", "cargo build"),
            ],
            ..SpecInput::default()
        };
        write_spec_md(&spec_dir, &input, &None, Locale::EnUs, Tone::default())
            .expect("write spec.md");
        let body = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();

        let section = extract_ac_section(&body).expect("the AC section parses");
        let items = parse_ac_items(&section);
        assert_eq!(items.len(), 3, "every criterion still parses: {body}");
        assert_eq!(items[0].command, "cargo test foo", "the command is untouched");
        assert!(
            items[0].control.as_deref().is_some_and(|c| c.starts_with('<')),
            "AC-1 is offered an unfilled control: {:?}",
            items[0].control,
        );
        assert!(items[1].control.is_some(), "AC-2 too: {:?}", items[1].control);
        assert_eq!(
            items[2].control, None,
            "the trailing safety criterion has nothing to control for",
        );
        // And the marker reads as UNFILLED, so nobody mistakes it for a control
        // that ran.
        assert!(
            crate::commands::review::qa_run::is_skeleton(
                items[0].control.as_deref().unwrap_or_default()
            ),
            "the seeded control is a skeleton: {:?}",
            items[0].control,
        );
    }

    /// The dual link: a `[[spec.{slug}]]` reference resolves to the generated
    /// `spec.md` via its frontmatter `id:` (not its filename, which is the
    /// generic `spec.md`). This is the whole point of the convention — the
    /// artifact is addressable by a rename-proof id through the SAME resolver
    /// (`atomic_md::wikilink::resolve`) that Obsidian-style links use.
    #[test]
    fn id_frontmatter_makes_spec_resolvable_by_dual_link() {
        use mustard_core::domain::spec::contract::SpecInput;
        use mustard_core::io::atomic_md::resolve;
        let dir = tempdir().unwrap();
        // The spec lives at `<root>/spec.my-slug/spec.md` (filename is `spec.md`,
        // identity is `spec.my-slug`).
        let spec_dir = dir.path().join("my-slug");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let input = SpecInput {
            title: "Resolvable Spec".to_string(),
            ..SpecInput::default()
        };
        write_spec_md(&spec_dir, &input, &None, Locale::EnUs, Tone::default())
            .expect("write spec.md");

        // `[[spec.my-slug]]` resolves to the spec.md by frontmatter id, even
        // though no file is named `spec.my-slug.md`.
        let resolved = resolve("spec.my-slug", &[dir.path()])
            .expect("dual link must resolve via frontmatter id");
        assert_eq!(resolved, spec_dir.join("spec.md"));
        assert_eq!(resolved.file_name().and_then(|n| n.to_str()), Some("spec.md"));
        // A bogus id does not resolve (no false positive).
        assert!(resolve("spec.not-a-slug", &[dir.path()]).is_none());
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
