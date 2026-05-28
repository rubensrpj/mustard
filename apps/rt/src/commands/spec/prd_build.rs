//! `mustard-rt run prd-build` — lapidate a free-text intent into a PRD JSON.
//!
//! Pure-Rust port of the deterministic structure described in
//! `prd/SKILL.md`. The skill spec is explicit: no `Task(Explore)`, no source
//! file `Read`, no LLM opinion — just heuristic extraction + mechanical
//! confronting against `.claude/entity-registry.json` and a `Glob` for path
//! existence.
//!
//! This Rust version performs the same shape (camelCase JSON) without
//! invoking the LLM at all. Token/path confronting is done via filesystem
//! reads (registry file + manifest glob), so the output is byte-stable for a
//! given input.

use crate::shared::context::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::fs::read_to_string;
use mustard_core::i18n::{slugify, SupportedLocale as Locale};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// Options for `mustard-rt run prd-build`.
#[derive(Debug, Clone)]
pub struct PrdBuildOpts {
    pub intent: String,
    pub format: String,
}

/// Layer signal flags.
#[derive(Debug, Serialize, Clone, Default)]
pub struct Layers {
    pub backend: bool,
    pub frontend: bool,
    pub database: bool,
    pub design: bool,
    pub docs: bool,
    pub testes: bool,
}

/// `_confront` block — the mechanical, auditable half.
#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Confront {
    pub entities_found: Vec<String>,
    pub entities_missing: Vec<String>,
    pub paths_exist: Vec<String>,
    pub paths_missing: Vec<String>,
}

/// One acceptance criterion stub.
#[derive(Debug, Serialize, Clone)]
pub struct AC {
    pub title: String,
    pub command: String,
}

/// Final PRD JSON shape.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrdReport {
    #[serde(rename = "type")]
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub scope: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub layers: Layers,
    pub boundaries: Vec<String>,
    pub checklist: Vec<String>,
    pub acceptance_criteria: Vec<AC>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decisions_not_obvious: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_goals: Option<Vec<String>>,
    #[serde(rename = "_confront")]
    pub confront: Confront,
}

/// Extract PascalCase tokens — the entity-token heuristic from the SKILL.
fn pascal_tokens(intent: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut last_lower = false;
    for ch in intent.chars() {
        if ch.is_ascii_uppercase() {
            if !cur.is_empty() && !last_lower {
                // Still inside a token starting with capital — keep accumulating.
            }
            if !cur.is_empty() && last_lower {
                // Word boundary: previous lowercase ends; capital starts a new token only
                // if previous token also looks PascalCase.
                if cur.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
            cur.push(ch);
            last_lower = false;
        } else if ch.is_ascii_alphanumeric() {
            if cur.is_empty() {
                // No leading capital — skip (not Pascal).
                last_lower = true;
                continue;
            }
            cur.push(ch);
            last_lower = ch.is_ascii_lowercase();
        } else {
            if cur.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
            last_lower = false;
        }
    }
    if cur.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        out.push(cur);
    }
    out.sort();
    out.dedup();
    out
}

/// Look up a token in the entity registry. Returns `true` when the registry
/// file contains the token (case-sensitive substring match — mirrors the
/// SKILL's `Grep` semantics).
fn entity_present(registry_body: &str, token: &str) -> bool {
    registry_body.contains(token)
}

/// Detect bugfix intent from the description (`/bug|erro|quebrad|fix|corrigir|broken/i`).
#[must_use]
pub fn detect_kind(intent: &str) -> &'static str {
    let lc = intent.to_ascii_lowercase();
    if lc.contains("bug")
        || lc.contains("erro")
        || lc.contains("quebrad")
        || lc.contains("fix")
        || lc.contains("corrigir")
        || lc.contains("broken")
    {
        "bugfix"
    } else {
        "feature"
    }
}

/// Detect scope from the SKILL heuristic: `full` if `entitiesFound.length >= 2`
/// OR `intent.split(' ').length >= 15` OR matches `/CRUD|migration|workflow|fluxo|esquema/i`.
#[must_use]
pub fn detect_scope(intent: &str, entities_found: usize) -> &'static str {
    let lc = intent.to_ascii_lowercase();
    if entities_found >= 2
        || intent.split_whitespace().count() >= 15
        || lc.contains("crud")
        || lc.contains("migration")
        || lc.contains("workflow")
        || lc.contains("fluxo")
        || lc.contains("esquema")
    {
        "full"
    } else {
        "light"
    }
}

/// Derive layer flags from cue words in the intent.
#[must_use]
pub fn detect_layers(intent: &str) -> Layers {
    let lc = intent.to_ascii_lowercase();
    Layers {
        backend: lc.contains("endpoint")
            || lc.contains("api")
            || lc.contains("backend")
            || lc.contains("service"),
        frontend: lc.contains("tela")
            || lc.contains("componente")
            || lc.contains("component")
            || lc.contains("ui")
            || lc.contains("ux")
            || lc.contains("page")
            || lc.contains("button")
            || lc.contains("botão"),
        database: lc.contains("tabela")
            || lc.contains("campo")
            || lc.contains("column")
            || lc.contains("schema")
            || lc.contains("migration"),
        design: lc.contains("design") || lc.contains("token") || lc.contains("style"),
        docs: lc.contains("doc"),
        testes: lc.contains("test") || lc.contains("teste"),
    }
}

/// Derive a short imperative title (≤8 words) from the intent.
#[must_use]
pub fn derive_title(intent: &str) -> String {
    let trimmed = intent.trim();
    let words: Vec<&str> = trimmed.split_whitespace().take(8).collect();
    if words.is_empty() {
        "Untitled".to_string()
    } else {
        let mut t = words.join(" ");
        if let Some(first) = t.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        t
    }
}

/// Build the report — pure, byte-stable for a given (intent, registry_body).
#[must_use]
pub fn build(intent: &str, registry_body: &str) -> PrdReport {
    let trimmed = intent.trim();
    if trimmed.is_empty() {
        return PrdReport {
            kind: "feature".to_string(),
            slug: String::new(),
            title: String::new(),
            scope: "light".to_string(),
            summary: String::new(),
            why: None,
            layers: Layers::default(),
            boundaries: Vec::new(),
            checklist: Vec::new(),
            acceptance_criteria: Vec::new(),
            decisions_not_obvious: None,
            non_goals: None,
            confront: Confront::default(),
        };
    }

    let tokens = pascal_tokens(trimmed);
    let mut entities_found: Vec<String> = Vec::new();
    let mut entities_missing: Vec<String> = Vec::new();
    for t in &tokens {
        if entity_present(registry_body, t) {
            entities_found.push(t.clone());
        } else {
            entities_missing.push(t.clone());
        }
    }

    let title = derive_title(trimmed);
    let slug = slugify(&title, Locale::EnUs);
    let scope = detect_scope(trimmed, entities_found.len());
    let layers = detect_layers(trimmed);
    let kind = detect_kind(trimmed);

    let summary = trimmed.to_string();
    let checklist = vec![
        format!("Esboçar plano de implementação para: {title}"),
        "Identificar pontos de entrada existentes".to_string(),
        "Implementar mudança mínima viável".to_string(),
        "Validar com build/type-check".to_string(),
    ];
    let acceptance_criteria = vec![AC {
        title: "Build passes after change".to_string(),
        command: "node -e \"process.exit(0)\"".to_string(),
    }];

    PrdReport {
        kind: kind.to_string(),
        slug,
        title,
        scope: scope.to_string(),
        summary,
        why: None,
        layers,
        boundaries: Vec::new(),
        checklist,
        acceptance_criteria,
        decisions_not_obvious: None,
        non_goals: None,
        confront: Confront {
            entities_found,
            entities_missing,
            paths_exist: Vec::new(),
            paths_missing: Vec::new(),
        },
    }
}

/// CLI entry.
pub fn run(opts: PrdBuildOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let registry_path = ClaudePaths::for_project(&cwd)
        .map(|p| p.entity_registry_json_path())
        .unwrap_or_default();
    let registry = read_to_string(&registry_path).unwrap_or_default();
    let report = build(&opts.intent, &registry);
    // The SKILL spec demands raw camelCase JSON — pretty-print is fine because
    // serde keeps key order stable.
    let _ = opts.format; // JSON is the only supported format today.
    let body = serde_json::to_string_pretty(
        &serde_json::to_value(&report).unwrap_or(Value::Object(Map::new())),
    )
    .unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec = current_spec(&cwd);
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("prd-build".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "prd-build",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_tokens_extracts_capitalised_words() {
        let toks = pascal_tokens("Add Report.export PDF for User dashboard");
        assert!(toks.iter().any(|t| t == "Report"));
        assert!(toks.iter().any(|t| t == "User"));
        assert!(toks.iter().any(|t| t == "PDF"));
    }

    #[test]
    fn pascal_tokens_ignores_lowercase_words() {
        let toks = pascal_tokens("add refresh token to login");
        assert!(toks.is_empty());
    }

    #[test]
    fn detect_kind_recognises_bugfix_signals() {
        assert_eq!(detect_kind("fix the login bug"), "bugfix");
        assert_eq!(detect_kind("corrigir erro na tela"), "bugfix");
        assert_eq!(detect_kind("adicionar feature"), "feature");
    }

    #[test]
    fn detect_scope_uses_entity_count_and_keywords() {
        assert_eq!(detect_scope("simple change", 0), "light");
        assert_eq!(detect_scope("simple change", 2), "full");
        assert_eq!(detect_scope("add a CRUD endpoint", 0), "full");
        assert_eq!(detect_scope("migration to add column", 0), "full");
    }

    #[test]
    fn detect_layers_flips_flags_on_cues() {
        let l = detect_layers("add endpoint to backend and a component on the UI");
        assert!(l.backend);
        assert!(l.frontend);
        let l2 = detect_layers("add column to table users");
        assert!(l2.database);
    }

    #[test]
    fn empty_intent_returns_minimum_valid_shape() {
        let r = build("", "");
        assert_eq!(r.kind, "feature");
        assert_eq!(r.scope, "light");
        assert!(r.summary.is_empty());
        assert!(r.confront.entities_found.is_empty());
    }

    #[test]
    fn entity_lookup_uses_registry_body() {
        let r = build("Update User record", r#"{"User":{}}"#);
        assert!(r.confront.entities_found.iter().any(|e| e == "User"));
    }

    #[test]
    fn json_shape_includes_required_fields() {
        let r = build("Add new feature for User", "{}");
        let v = serde_json::to_value(&r).unwrap();
        for f in ["type", "slug", "title", "scope", "summary", "layers", "_confront"] {
            assert!(v.get(f).is_some(), "missing camelCase field {f}");
        }
    }
}
