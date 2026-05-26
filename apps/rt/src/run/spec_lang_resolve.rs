//! `mustard-rt run spec-lang resolve` — resolve a spec's narrative locale.
//!
//! Walks the standard cascade:
//!
//! 1. The spec's `meta.json` sidecar (`lang` field — BCP-47 only).
//! 2. The spec's `### Lang:` header in `spec.md` (legacy fallback).
//! 3. `mustard.json` at the project root (`lang` field).
//! 4. Default — `pt-BR` (per `project_locale_codes`).
//!
//! Output is byte-stable JSON describing the resolved locale and the source it
//! came from, so callers can warn the user when the spec is still on the legacy
//! header form. Fail-open per step — a malformed file degrades to the next
//! source instead of erroring.

use crate::run::env::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::i18n::{project_locale_from_file, Locale};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::{read_meta, spec as spec_io};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Options for `mustard-rt run spec-lang resolve`.
#[derive(Debug, Clone)]
pub struct SpecLangResolveOpts {
    /// Path to a spec directory OR a bare slug under `.claude/spec/`.
    pub spec: String,
}

/// Where the resolved locale came from.
#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum LangSource {
    Meta,
    Header,
    MustardJson,
    Default,
}

/// JSON report shape.
#[derive(Debug, Serialize)]
pub struct SpecLangReport {
    pub spec: String,
    pub lang: String,
    pub source: LangSource,
}

/// Resolve the spec directory: explicit path or `.claude/spec/{slug}/`.
fn spec_dir(cwd: &Path, slug_or_path: &str) -> PathBuf {
    let p = Path::new(slug_or_path);
    if p.is_absolute() && p.is_dir() {
        return p.to_path_buf();
    }
    let direct = cwd.join(slug_or_path);
    if direct.is_dir() {
        return direct;
    }
    cwd.join(".claude").join("spec").join(slug_or_path)
}

/// Read the `lang` field from `meta.json`. Returns `None` on any error
/// (sidecar absent / unparseable / lang absent / lang invalid).
fn lang_from_meta(spec_dir: &Path) -> Option<Locale> {
    let path = spec_dir.join("meta.json");
    let meta = read_meta(&path)?;
    let raw = meta.lang?;
    Locale::from_str(&raw).ok()
}

/// Read the `### Lang:` header from `spec.md`. Returns `None` on any error.
fn lang_from_header(spec_dir: &Path) -> Option<Locale> {
    let body = std::fs::read_to_string(spec_dir.join("spec.md")).ok()?;
    let raw = spec_io::header_field(&body, "Lang")?;
    Locale::from_str(&raw).ok()
}

/// Pure resolver — runs the cascade and returns the (`Locale`, source).
#[must_use]
pub fn resolve(cwd: &Path, slug_or_path: &str) -> (Locale, LangSource) {
    let dir = spec_dir(cwd, slug_or_path);
    if let Some(l) = lang_from_meta(&dir) {
        return (l, LangSource::Meta);
    }
    if let Some(l) = lang_from_header(&dir) {
        return (l, LangSource::Header);
    }
    let mustard_json = cwd.join("mustard.json");
    if mustard_json.exists() {
        let l = project_locale_from_file(&mustard_json);
        // `project_locale_from_file` already fails open to the default. We can't
        // distinguish "found en-US" from "default to pt-BR" without re-reading,
        // so peek manually for the field presence.
        if let Ok(text) = std::fs::read_to_string(&mustard_json) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if v.get("lang").and_then(|v| v.as_str()).is_some() {
                    return (l, LangSource::MustardJson);
                }
            }
        }
    }
    (Locale::default(), LangSource::Default)
}

/// CLI entry.
pub fn run(opts: SpecLangResolveOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (locale, source) = resolve(&cwd, &opts.spec);
    let report = SpecLangReport {
        spec: opts.spec.clone(),
        lang: locale.as_str().to_string(),
        source,
    };
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis(), &opts.spec);
}

fn emit_economy(duration_ms: u128, spec: &str) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec_attr = if spec.is_empty() {
        current_spec(&cwd)
    } else {
        Some(spec.to_string())
    };
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("spec-lang-resolve".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "spec-lang-resolve",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: spec_attr,
    };
    let _ = crate::run::event_route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn meta_lang_wins_over_header() {
        let dir = tempdir().unwrap();
        let spec = dir.path().join(".claude").join("spec").join("demo");
        write(
            &spec.join("meta.json"),
            r#"{"lang":"en-US"}"#,
        );
        write(
            &spec.join("spec.md"),
            "# Demo\n### Lang: pt-BR\n",
        );
        let (l, src) = resolve(dir.path(), "demo");
        assert_eq!(l, Locale::EnUs);
        assert_eq!(src, LangSource::Meta);
    }

    #[test]
    fn header_used_when_meta_absent() {
        let dir = tempdir().unwrap();
        let spec = dir.path().join(".claude").join("spec").join("demo");
        write(
            &spec.join("spec.md"),
            "# Demo\n### Lang: en-US\n",
        );
        let (l, src) = resolve(dir.path(), "demo");
        assert_eq!(l, Locale::EnUs);
        assert_eq!(src, LangSource::Header);
    }

    #[test]
    fn mustard_json_falls_back_when_spec_silent() {
        let dir = tempdir().unwrap();
        let spec = dir.path().join(".claude").join("spec").join("demo");
        std::fs::create_dir_all(&spec).unwrap();
        write(&dir.path().join("mustard.json"), r#"{"lang":"en-US"}"#);
        let (l, src) = resolve(dir.path(), "demo");
        assert_eq!(l, Locale::EnUs);
        assert_eq!(src, LangSource::MustardJson);
    }

    #[test]
    fn default_when_nothing_resolves() {
        let dir = tempdir().unwrap();
        let spec = dir.path().join(".claude").join("spec").join("demo");
        std::fs::create_dir_all(&spec).unwrap();
        let (l, src) = resolve(dir.path(), "demo");
        assert_eq!(l, Locale::default());
        assert_eq!(src, LangSource::Default);
    }

    #[test]
    fn json_shape_is_byte_stable() {
        let report = SpecLangReport {
            spec: "demo".to_string(),
            lang: "pt-BR".to_string(),
            source: LangSource::Default,
        };
        let v = serde_json::to_value(report).unwrap();
        assert_eq!(v["spec"], json!("demo"));
        assert_eq!(v["lang"], json!("pt-BR"));
        assert_eq!(v["source"], json!("default"));
    }
}
