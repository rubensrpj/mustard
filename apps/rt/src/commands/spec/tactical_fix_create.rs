//! `mustard-rt run tactical-fix-create` — scaffold a tactical-fix sub-spec.
//!
//! Replaces the steps in `tactical-fix/SKILL.md`. Builds `.claude/spec/<slug>/`
//! containing a `spec.md` body skeleton (pure narrative — no lifecycle header)
//! and the matching `meta.json` sidecar that carries every machine-parseable
//! field (stage/outcome/scope/lang/checkpoint/parent); finally emits the
//! `spec.link` parent → child edge in-process into the harness event store.
//!
//! Pure-Rust slug derivation: lowercase, strip diacritics (PT), kebab-case,
//! ≤6 words, prefixed by `YYYY-MM-DD` (local). Idempotent on the sidecar — a
//! repeat call against an existing directory aborts with a `dir_exists` error
//! in the JSON rather than overwriting work in flight.

use serde_json::json;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use crate::shared::context;
use crate::shared::events::economy;
use crate::commands::spec::spec_scaffold;
use mustard_core::time::now_iso8601;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs::write_atomic;
use mustard_core::platform::i18n::{slugify, Locale};
use mustard_core::{read_meta, Meta};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run tactical-fix-create`.
#[derive(Debug, Clone)]
pub struct TacticalFixOpts {
    pub parent: String,
    pub description: String,
    pub scope: String,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub(crate) struct TacticalFixReport {
    pub parent: String,
    pub slug: String,
    pub spec_dir: String,
    pub spec_md: String,
    pub meta_json: String,
    pub link_emitted: bool,
    pub error: Option<String>,
}

/// Max number of words kept in a tactical-fix slug (keeps slugs short).
const SLUG_MAX_TOKENS: usize = 4;

/// Cap the slug at [`SLUG_MAX_TOKENS`] hyphen-separated words.
fn cap_words(slug: &str) -> String {
    slug.split('-')
        .filter(|s| !s.is_empty())
        .take(SLUG_MAX_TOKENS)
        .collect::<Vec<_>>()
        .join("-")
}

/// Build the date-prefixed slug.
fn build_slug(description: &str, lang: Locale, today: &str) -> String {
    let body = cap_words(&slugify(description, lang));
    if body.is_empty() {
        format!("{today}-tactical-fix")
    } else {
        format!("{today}-{body}")
    }
}

/// Read the parent's locale to inherit the body headings. Falls back to PT-BR.
fn parent_lang(cwd: &Path, parent: &str) -> Locale {
    let dir = ClaudePaths::spec_dir_or_unchecked(cwd, parent);
    if let Some(meta) = read_meta(&dir.join("meta.json")) {
        if let Some(raw) = meta.lang {
            if let Ok(l) = raw.parse::<Locale>() {
                return l;
            }
        }
    }
    Locale::default()
}

/// Today as YYYY-MM-DD (UTC — tests run in any timezone).
fn today_utc() -> String {
    let now = now_iso8601();
    now.chars().take(10).collect()
}

/// Build the canonical body skeleton. Lifecycle metadata (stage / outcome /
/// scope / lang / checkpoint / parent) lives only in the `meta.json` sidecar;
/// the markdown is pure narrative. The parent is still surfaced as a body link
/// in the context note so a human reader sees the lineage.
fn build_body(description: &str, parent: &str, lang: Locale) -> String {
    let (h_context, h_ac, h_files) = match lang {
        Locale::PtBr => ("Contexto", "Critérios de Aceitação", "Arquivos"),
        Locale::EnUs => ("Context", "Acceptance Criteria", "Files"),
    };
    let parent_note = match lang {
        Locale::PtBr => format!("Tactical fix derivado de [[{parent}]]."),
        Locale::EnUs => format!("Tactical fix derived from [[{parent}]]."),
    };
    format!(
        "# Tactical Fix: {description}\n\n\
         ## {h_context}\n\n\
         {parent_note}\n\n\
         ## {h_ac}\n\n\
         <!-- 1-3 binary, executable AC, cross-shell -->\n\n\
         ## {h_files}\n\n\
         <!-- Paths intentionally touched -->\n"
    )
}

/// Core routine — pure-ish (writes files), returns a report.
fn create(cwd: &Path, opts: &TacticalFixOpts) -> TacticalFixReport {
    let lang = parent_lang(cwd, &opts.parent);
    let today = today_utc();
    let slug = build_slug(&opts.description, lang, &today);
    let spec_dir = ClaudePaths::spec_dir_or_unchecked(cwd, &slug);
    let mut report = TacticalFixReport {
        parent: opts.parent.clone(),
        slug: slug.clone(),
        spec_dir: spec_dir.display().to_string(),
        spec_md: spec_dir.join("spec.md").display().to_string(),
        meta_json: spec_dir.join("meta.json").display().to_string(),
        link_emitted: false,
        error: None,
    };
    if spec_dir.exists() {
        report.error = Some("dir_exists".to_string());
        return report;
    }
    if let Err(e) = std::fs::create_dir_all(&spec_dir) {
        report.error = Some(format!("create_dir failed: {e}"));
        return report;
    }
    let ts = now_iso8601();
    let body = build_body(&opts.description, &opts.parent, lang);
    let spec_path = spec_dir.join("spec.md");
    if let Err(e) = write_atomic(&spec_path, body.as_bytes()) {
        report.error = Some(format!("write spec.md failed: {e}"));
        return report;
    }
    let meta = Meta {
        stage: Some("Analyze".to_string()),
        outcome: Some("Active".to_string()),
        phase: None,
        scope: Some(opts.scope.clone()),
        lang: Some(lang.as_str().to_string()),
        checkpoint: Some(ts.clone()),
        parent: Some(opts.parent.clone()),
        is_wave_plan: None,
        total_waves: None,
        // A freshly created tactical-fix spec carries no qualifier flag.
        flags: mustard_core::MetaFlags::default(),
        // TF checklists stay in the spec markdown (root meta carries none).
        checklist: Vec::new(),
        raw: serde_json::Value::Null,
    };
    if let Err(e) = spec_scaffold::write_meta_json(&spec_dir, &meta) {
        report.error = Some(format!("write meta.json failed: {e}"));
        return report;
    }
    // Emit the `spec.link` parent → child edge in-process — the retired
    // `spec-link` face used to do this via a child process. Routed with the
    // caller's `cwd`, so unit tests under `cargo test -p mustard-rt` write to
    // their own workspace (umbrella AC-G2 regression).
    let link_ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.clone(),
        session_id: context::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("tactical-fix-create".to_string()),
            actor_type: None,
        },
        event: "spec.link".to_string(),
        payload: json!({
            "parent": opts.parent,
            "child": slug,
            "reason": "tactical-fix",
        }),
        spec: Some(slug.clone()),
    };
    report.link_emitted =
        crate::shared::events::route::emit(cwd.to_string_lossy().as_ref(), &link_ev);
    report
}

/// CLI entry.
pub fn run(opts: TacticalFixOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = create(&cwd, &opts);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "tactical-fix-create", started.elapsed().as_millis() as u64, Some(report.slug.as_str()), json!({}));
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn slug_caps_at_six_words_with_date_prefix() {
        let s = build_slug(
            "a very long description that has many words indeed",
            Locale::EnUs,
            "2026-05-25",
        );
        assert!(s.starts_with("2026-05-25-"));
        let tail: Vec<&str> = s["2026-05-25-".len()..].split('-').collect();
        assert!(tail.len() <= 6);
    }

    #[test]
    fn body_has_no_lifecycle_header_en() {
        let b = build_body("fix x", "epic-y", Locale::EnUs);
        // Lifecycle metadata lives only in meta.json — never in the markdown.
        assert!(!b.contains("### Stage:"));
        assert!(!b.contains("### Outcome:"));
        assert!(!b.contains("### Parent:"));
        // The body still surfaces the parent as a narrative link + EN headings.
        assert!(b.contains("[[epic-y]]"));
        assert!(b.contains("## Context"));
        assert!(b.contains("## Acceptance Criteria"));
    }

    #[test]
    fn body_uses_pt_headings_when_lang_pt() {
        let b = build_body("ajustar", "epic-y", Locale::PtBr);
        assert!(!b.contains("### Stage:"));
        assert!(b.contains("## Contexto"));
        assert!(b.contains("## Critérios de Aceitação"));
        assert!(b.contains("## Arquivos"));
    }

    #[test]
    fn create_writes_spec_and_meta() {
        let dir = tempdir().unwrap();
        let opts = TacticalFixOpts {
            parent: "epic-1".to_string(),
            description: "Fix null guard".to_string(),
            scope: "light".to_string(),
        };
        let report = create(dir.path(), &opts);
        assert!(report.error.is_none(), "unexpected error: {:?}", report.error);
        let spec_dir = dir.path().join(".claude/spec").join(&report.slug);
        assert!(spec_dir.join("spec.md").exists());
        assert!(spec_dir.join("meta.json").exists());
    }

    #[test]
    fn create_aborts_when_dir_exists() {
        let dir = tempdir().unwrap();
        let opts = TacticalFixOpts {
            parent: "epic-1".to_string(),
            description: "Fix one thing".to_string(),
            scope: "light".to_string(),
        };
        let r1 = create(dir.path(), &opts);
        assert!(r1.error.is_none());
        let r2 = create(dir.path(), &opts);
        assert_eq!(r2.error.as_deref(), Some("dir_exists"));
    }
}
