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

use serde_json::json;
use mustard_core::domain::model::event::ActorKind;
use crate::shared::context;
use crate::shared::events::economy;
use mustard_core::platform::i18n::SupportedLocale as Locale;
use mustard_core::ProjectConfig;
use mustard_core::ClaudePaths;
use mustard_core::{read_meta, domain::spec as spec_io};
use serde::Serialize;
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
    // Fall back to the canonical `<cwd>/.claude/spec/<slug>/` via ClaudePaths.
    // An I1 guard violation or malformed slug returns an empty PathBuf, which
    // the downstream readers in `run()` handle as "spec absent".
    ClaudePaths::for_project(cwd)
        .ok()
        .and_then(|cp| cp.for_spec(slug_or_path).ok())
        .map(|sp| sp.dir().to_path_buf())
        .unwrap_or_default()
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
    // The mustard.json level contributes only when it actually carries a
    // language key. `ProjectConfig` exposes `spec_lang`/`lang` directly, so we
    // claim the `MustardJson` source exactly when one is present (the cascade's
    // contract) — no second manual read.
    let config = ProjectConfig::load(cwd);
    if config.spec_lang.is_some() || config.lang.is_some() {
        return (config.i18n().lang, LangSource::MustardJson);
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
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "spec-lang-resolve", started.elapsed().as_millis() as u64, Some(opts.spec.as_str()), json!({}));
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
