//! `mustard-rt run tactical-fix-create` — scaffold a tactical-fix sub-spec.
//!
//! Replaces the steps in `tactical-fix/SKILL.md`. Builds `.claude/spec/<slug>/`
//! containing a `spec.md` with the canonical header (Stage/Outcome/Flags/Scope/
//! Lang/Checkpoint/Parent) and a body skeleton; writes the matching
//! `meta.json` sidecar; finally invokes `spec-link` to record the parent → child
//! edge in the harness event store.
//!
//! Pure-Rust slug derivation: lowercase, strip diacritics (PT), kebab-case,
//! ≤6 words, prefixed by `YYYY-MM-DD` (local). Idempotent on the sidecar — a
//! repeat call against an existing directory aborts with a `dir_exists` error
//! in the JSON rather than overwriting work in flight.

use crate::shared::context::{current_spec, session_id};
use crate::commands::spec::spec_scaffold;
use mustard_core::time::now_iso8601;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs::write_atomic;
use mustard_core::platform::i18n::{slugify, Locale};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::platform::process::rtk_command;
use mustard_core::{read_meta, Meta};
use serde::Serialize;
use serde_json::json;
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
pub struct TacticalFixReport {
    pub parent: String,
    pub slug: String,
    pub spec_dir: String,
    pub spec_md: String,
    pub meta_json: String,
    pub link_emitted: bool,
    pub error: Option<String>,
}

/// Cap the slug at 6 hyphen-separated words.
fn cap_words(slug: &str) -> String {
    slug.split('-')
        .filter(|s| !s.is_empty())
        .take(6)
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
    let dir = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(parent))
        .map(|sp| sp.dir().to_path_buf())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(cwd).spec_dir().join(parent));
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

/// Build the canonical body skeleton.
fn build_body(description: &str, parent: &str, scope: &str, lang: Locale, ts: &str) -> String {
    let (h_context, h_ac, h_files) = match lang {
        Locale::PtBr => ("Contexto", "Critérios de Aceitação", "Arquivos"),
        Locale::EnUs => ("Context", "Acceptance Criteria", "Files"),
    };
    let parent_note = match lang {
        Locale::PtBr => format!("Tactical fix derivado de [[{parent}]]."),
        Locale::EnUs => format!("Tactical fix derived from [[{parent}]]."),
    };
    let lang_code = lang.as_str();
    format!(
        "# Tactical Fix: {description}\n\n\
         ### Stage: Analyze\n\
         ### Outcome: Active\n\
         ### Flags: \n\
         ### Scope: {scope}\n\
         ### Lang: {lang_code}\n\
         ### Checkpoint: {ts}\n\
         ### Parent: {parent}\n\n\
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
    let spec_dir = ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(&slug))
        .map(|sp| sp.dir().to_path_buf())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(cwd).spec_dir().join(&slug));
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
    let body = build_body(&opts.description, &opts.parent, &opts.scope, lang, &ts);
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
        raw: serde_json::Value::Null,
    };
    if let Err(e) = spec_scaffold::write_meta_json(&spec_dir, &meta) {
        report.error = Some(format!("write meta.json failed: {e}"));
        return report;
    }
    // Emit the spec.link event via our own subcommand. Best-effort.
    //
    // Pin the spawned child to the caller's `cwd` + `MUSTARD_WORKSPACE_ROOT`
    // so it writes to the same workspace as the parent — without this,
    // unit tests running under `cargo test -p mustard-rt` would inherit the
    // crate's own `apps/rt/` cwd and leak `apps/rt/.claude/.pipeline-states/`
    // (umbrella AC-G2 regression).
    let link_out = rtk_command(
        "mustard-rt",
        &[
            "run",
            "spec-link",
            "--parent",
            &opts.parent,
            "--child",
            &slug,
            "--reason",
            "tactical-fix",
        ],
    )
    .current_dir(cwd)
    .env("MUSTARD_WORKSPACE_ROOT", cwd)
    .output();
    report.link_emitted = matches!(link_out, Ok(ref o) if o.status.success());
    report
}

/// CLI entry.
pub fn run(opts: TacticalFixOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = create(&cwd, &opts);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis(), &report.slug);
}

fn emit_economy(duration_ms: u128, child_slug: &str) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec_attr = if child_slug.is_empty() {
        current_spec(&cwd)
    } else {
        Some(child_slug.to_string())
    };
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("tactical-fix-create".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "tactical-fix-create",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: spec_attr,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
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
    fn body_includes_canonical_headers_en() {
        let b = build_body("fix x", "epic-y", "light", Locale::EnUs, "2026-05-25T00:00:00Z");
        assert!(b.contains("### Stage: Analyze"));
        assert!(b.contains("### Outcome: Active"));
        assert!(b.contains("### Parent: epic-y"));
        assert!(b.contains("## Context"));
        assert!(b.contains("## Acceptance Criteria"));
    }

    #[test]
    fn body_uses_pt_headers_when_lang_pt() {
        let b = build_body("ajustar", "epic-y", "light", Locale::PtBr, "2026-05-25T00:00:00Z");
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
