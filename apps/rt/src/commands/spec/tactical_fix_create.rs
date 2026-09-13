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
use crate::shared::spec_state::DiskSpecState;
use mustard_core::domain::spec_state::SpecState;
use crate::commands::spec::spec_scaffold;
use mustard_core::time::now_iso8601;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs::write_atomic;
use mustard_core::platform::i18n::{slugify, Locale};
use mustard_core::Meta;
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

/// A spec-mãe pelo nome de uma pasta de spec que existe em `specs`: o nome
/// aparado e sem a barra do fim; a pasta com esse nome, ou, sem ela, a única
/// que só difere em maiúsculas. É o nome da pasta que fica gravado, e é por
/// ele que a testemunha acha o ajuste na branch da mãe. `None` quando
/// nenhuma pasta de spec tem o nome.
fn existing_parent(specs: &Path, given: &str) -> Option<String> {
    let name = given.trim().trim_end_matches(['/', '\\']).trim();
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) {
        return None;
    }
    let folders: Vec<String> = std::fs::read_dir(specs)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|folder| !folder.starts_with('.'))
        .collect();
    if let Some(exact) = folders.iter().find(|folder| *folder == name) {
        return Some(exact.clone());
    }
    let mut same: Vec<&String> = folders.iter().filter(|folder| folder.eq_ignore_ascii_case(name)).collect();
    (same.len() == 1).then(|| same.remove(0).clone())
}

/// Core routine — pure-ish (writes files), returns a report.
fn create(cwd: &Path, opts: &TacticalFixOpts) -> TacticalFixReport {
    // The body headings follow the project's text language, as the parent's do.
    let lang = mustard_core::ProjectConfig::load(cwd).language().text_or_default();
    let today = today_utc();
    let slug = build_slug(&opts.description, lang, &today);
    let spec_dir = ClaudePaths::spec_dir_or_unchecked(cwd, &slug);
    let parent = spec_dir.parent().and_then(|specs| existing_parent(specs, &opts.parent));
    let mut report = TacticalFixReport {
        parent: parent.clone().unwrap_or_else(|| opts.parent.clone()),
        slug: slug.clone(),
        spec_dir: spec_dir.display().to_string(),
        spec_md: spec_dir.join("spec.md").display().to_string(),
        meta_json: spec_dir.join("meta.json").display().to_string(),
        link_emitted: false,
        error: None,
    };
    // Só o nome de uma pasta de spec que existe: a mãe gravada como veio, com
    // espaço, barra ou outra caixa, nunca casaria com a branch dela.
    let Some(parent) = parent else {
        report.error = Some("parent_not_found".to_string());
        return report;
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
    let body = build_body(&opts.description, &parent, lang);
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
        parent: Some(parent.clone()),
        // A tactical fix rides its parent's branch — no base of its own.
        base: None,
        is_wave_plan: None,
        total_waves: None,
        // A freshly created tactical-fix spec carries no qualifier flag.
        flags: mustard_core::MetaFlags::default(),
        // TF checklists stay in the spec markdown (root meta carries none).
        checklist: Vec::new(),
        findings: Vec::new(),
        raw: serde_json::Value::Null,
    };
    if let Err(e) = spec_scaffold::write_meta_json(&spec_dir, &meta) {
        report.error = Some(format!("write meta.json failed: {e}"));
        return report;
    }
    // A spec nasce na fase de plano, pela mesma gravação do `spec-draft`, na
    // branch da spec-mãe, em que o tactical fix vai junto. Ali a escada nomeia
    // a mãe: a testemunha acha o ajuste pela sessão e o aprova, e o portão de
    // escrita continua julgando a mãe.
    let parent_branch = DiskSpecState::new(cwd).state(&parent).and_then(|state| state.branch);
    if let Err(refusal) =
        crate::commands::spec_events::write::record_birth(cwd, &slug, parent_branch.as_deref())
    {
        eprintln!("tactical-fix-create: WARN: {}", refusal.message(lang));
    }
    // Emit the `spec.link` parent → child edge in-process — the retired
    // `spec-link` face used to do this via a child process. Routed with the
    // caller's `cwd`, so unit tests under `cargo test -p mustard-rt` write to
    // their own workspace, never into the repository's own `.claude/`.
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
            "parent": parent,
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

    /// A pasta da spec-mãe `name`, sem mais nada.
    fn parent_folder(root: &Path, name: &str) {
        std::fs::create_dir_all(root.join(".claude").join("spec").join(name)).unwrap();
    }

    #[test]
    fn create_writes_spec_and_meta() {
        let dir = tempdir().unwrap();
        parent_folder(dir.path(), "epic-1");
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
        parent_folder(dir.path(), "epic-1");
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

    /// O tactical fix nasce na fase de plano, na branch da spec-mãe.
    #[test]
    fn the_tactical_fix_is_born_in_plan_on_its_parents_branch() {
        let dir = tempdir().unwrap();
        let parent = mustard_core::io::spec_events::spec_file(dir.path(), "epic-1").unwrap();
        std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
        let running = serde_json::json!({ "phase": "running", "branch": "feature/epic-1" });
        mustard_core::io::spec_events::write(&parent, "state", running.as_object().cloned().unwrap(), &[])
            .unwrap();
        let opts = TacticalFixOpts {
            parent: "epic-1".to_string(),
            description: "Fix null guard".to_string(),
            scope: "light".to_string(),
        };
        let report = create(dir.path(), &opts);
        assert!(report.error.is_none(), "unexpected error: {:?}", report.error);
        let state = DiskSpecState::new(dir.path()).state(&report.slug).expect("the fix is born");
        assert_eq!(state.phase, Some("plan"));
        assert!(!state.approved);
        assert_eq!(state.branch.as_deref(), Some("feature/epic-1"), "it rides its parent's branch");
    }

    /// A mãe é o nome de uma pasta de spec que existe: com barra no fim, com
    /// espaço ou em maiúsculas, fica gravado o nome da pasta; um nome que
    /// nenhuma pasta tem é recusado, e nada é criado.
    #[test]
    fn the_parent_is_the_name_of_an_existing_spec_folder() {
        let dir = tempdir().unwrap();
        parent_folder(dir.path(), "epic-1");
        for (given, description) in [("epic-1/", "Fix one"), (" EPIC-1 ", "Fix two"), ("Epic-1/", "Fix three")] {
            let opts = TacticalFixOpts {
                parent: given.to_string(),
                description: description.to_string(),
                scope: "light".to_string(),
            };
            let report = create(dir.path(), &opts);
            assert!(report.error.is_none(), "{given:?}: {:?}", report.error);
            assert_eq!(report.parent, "epic-1", "{given:?}");
            let meta = mustard_core::read_meta(&dir.path().join(".claude/spec").join(&report.slug).join("meta.json"))
                .expect("the meta");
            assert_eq!(meta.parent.as_deref(), Some("epic-1"), "{given:?}");
        }

        let opts = TacticalFixOpts {
            parent: "epic-2".to_string(),
            description: "Fix four".to_string(),
            scope: "light".to_string(),
        };
        let report = create(dir.path(), &opts);
        assert_eq!(report.error.as_deref(), Some("parent_not_found"));
        assert!(!dir.path().join(".claude/spec").join(&report.slug).exists(), "nothing was created");
    }
}
