//! `mustard-rt run i18n translate-heading` — translate a markdown heading line.
//!
//! Maps a `## Heading` (or `### Heading`) string into the user's target locale
//! using the canonical heading map in [`mustard_core::i18n`]. The intent is to
//! keep tooling that rewrites spec sections (close-summary, wave scaffolding,
//! resume bootstrap) idiom-agnostic — they call this one subcommand instead of
//! carrying their own bilingual lookup tables.
//!
//! ## Heading recognition
//!
//! Strips a leading `#` run + spaces and looks the bare label up against the
//! `heading.spec.*` and `heading.memory.*` keys exposed by [`mustard_core::i18n::translate`].
//! Recognises both directions (PT→EN and EN→PT). Unknown labels round-trip
//! unchanged (fail-open).

use crate::shared::context::session_id;
use crate::util::now_iso8601;
use mustard_core::i18n::{translate, SupportedLocale as Locale};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde::Serialize;
use serde_json::json;
use std::str::FromStr;

/// Options for `mustard-rt run i18n translate-heading`.
#[derive(Debug, Clone)]
pub struct TranslateHeadingOpts {
    pub from: String,
    pub to_lang: String,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct TranslateReport {
    pub from: String,
    pub to_lang: String,
    pub to: String,
    pub matched_key: Option<String>,
}

/// All canonical spec/memory heading keys we know how to translate.
const HEADING_KEYS: &[&str] = &[
    "heading.spec.context",
    "heading.spec.users",
    "heading.spec.metric",
    "heading.spec.non_goals",
    "heading.spec.ac",
    "heading.spec.ac_list",
    "heading.spec.tasks",
    "heading.spec.files",
    "heading.spec.limits",
    "heading.spec.summary",
    "heading.memory.origin",
    "heading.memory.applies_to",
    "heading.memory.status",
    "heading.memory.related",
    "heading.memory.principles",
];

/// Strip leading `#` characters + a single space so `"## Tarefas"` → `("##", "Tarefas")`.
fn split_heading(line: &str) -> (String, String) {
    let trimmed = line.trim_start();
    let hashes: String = trimmed.chars().take_while(|c| *c == '#').collect();
    let rest = &trimmed[hashes.len()..];
    let body = rest.trim_start();
    (hashes, body.to_string())
}

/// Pure transform: given the raw heading line and target locale, return the
/// translated heading line + the matched key (if any).
#[must_use]
pub fn translate_heading(raw: &str, target: Locale) -> (String, Option<String>) {
    let (hashes, label) = split_heading(raw);
    let label_lc = label.trim().to_lowercase();
    // Try every known key against both locales — the matching one tells us
    // which canonical heading the input refers to.
    for key in HEADING_KEYS {
        let pt = translate(key, Locale::PtBr).to_lowercase();
        let en = translate(key, Locale::EnUs).to_lowercase();
        if label_lc == pt || label_lc == en {
            let new_label = translate(key, target);
            let prefix = if hashes.is_empty() {
                String::new()
            } else {
                format!("{hashes} ")
            };
            return (format!("{prefix}{new_label}"), Some((*key).to_string()));
        }
    }
    // Unknown — round-trip unchanged.
    (raw.to_string(), None)
}

/// CLI entry.
pub fn run(opts: TranslateHeadingOpts) {
    let started = std::time::Instant::now();
    let target = Locale::from_str(&opts.to_lang).unwrap_or_default();
    let (translated, matched) = translate_heading(&opts.from, target);
    let report = TranslateReport {
        from: opts.from.clone(),
        to_lang: target.as_str().to_string(),
        to: translated,
        matched_key: matched,
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("i18n-translate-heading".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "i18n-translate-heading",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: None,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_pt_to_en_preserving_hash_level() {
        let (out, key) = translate_heading("## Tarefas", Locale::EnUs);
        assert_eq!(out, "## Tasks");
        assert_eq!(key.as_deref(), Some("heading.spec.tasks"));
    }

    #[test]
    fn translates_en_to_pt() {
        let (out, key) = translate_heading("## Files", Locale::PtBr);
        assert_eq!(out, "## Arquivos");
        assert_eq!(key.as_deref(), Some("heading.spec.files"));
    }

    #[test]
    fn three_hash_heading_round_trips() {
        let (out, _) = translate_heading("### Context", Locale::PtBr);
        assert_eq!(out, "### Contexto");
    }

    #[test]
    fn unknown_heading_round_trips_unchanged() {
        let (out, key) = translate_heading("## Bogus", Locale::PtBr);
        assert_eq!(out, "## Bogus");
        assert!(key.is_none());
    }

    #[test]
    fn empty_string_round_trips() {
        let (out, _) = translate_heading("", Locale::EnUs);
        assert_eq!(out, "");
    }

    #[test]
    fn json_shape_includes_required_fields() {
        let report = TranslateReport {
            from: "## Tarefas".to_string(),
            to_lang: "en-US".to_string(),
            to: "## Tasks".to_string(),
            matched_key: Some("heading.spec.tasks".to_string()),
        };
        let v = serde_json::to_value(report).unwrap();
        assert!(v.get("from").is_some());
        assert!(v.get("to_lang").is_some());
        assert!(v.get("to").is_some());
        assert!(v.get("matched_key").is_some());
    }
}
