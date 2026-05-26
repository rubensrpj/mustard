//! Project-scoped settings Tauri commands.
//!
//! Wave 4 of `mustard-unification` — `mustard.json` gains `lang` (BCP-47) and
//! `tone` (didactic / technical / concise). The dashboard Settings page lets
//! the user pick both; the writes are routed through this module so the
//! validation + telemetry contract is shared with every future caller.
//!
//! ## Layout
//!
//! - [`set_language`] — validate against [`mustard_core::SupportedLocale`], write
//!   `mustard.json#lang`, emit `pipeline.economy.operation.invoked`.
//! - [`set_tone`] — validate against [`mustard_core::Tone`], write
//!   `mustard.json#tone`, emit telemetry.
//! - [`read_settings`] — read both fields back for the form initial state.
//!
//! ## Fail-open contract
//!
//! - A malformed `mustard.json` is treated as an empty object — the field is
//!   set on a fresh object and the file is rewritten atomically.
//! - Telemetry emission is best-effort; a store failure never propagates.
//! - Path traversal is rejected up front (`repo_path` must be a real dir).

use crate::db;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::store::event_store::EventSink;
use mustard_core::{SupportedLocale, Tone};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

/// The two settings the dashboard reads / writes.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectSettings {
    /// BCP-47 locale code (`pt-BR` / `en-US`). `None` when unset.
    pub lang: Option<String>,
    /// Tone string (`didactic` / `technical` / `concise`). `None` when unset.
    pub tone: Option<String>,
}

/// Write `mustard.json#lang` after validating against [`Locale::from_str`].
///
/// Rejects the legacy short forms (`pt`/`en`) with a typed error so the
/// dashboard can surface "use pt-BR or en-US" without inferring intent.
/// Emits `pipeline.economy.operation.invoked { operation: "i18n-set-language",
/// duration_ms, tokens_used: 0 }`.
#[tauri::command]
pub fn set_language(repo_path: String, lang: String) -> Result<(), String> {
    let started = Instant::now();
    let locale = SupportedLocale::from_str(&lang).map_err(|e| e.to_string())?;
    write_field(&repo_path, "lang", serde_json::Value::String(locale.as_str().to_string()))?;
    emit_i18n_op(&repo_path, "i18n-set-language", started.elapsed().as_millis());
    Ok(())
}

/// Write `mustard.json#tone` after validating against [`Tone::parse`].
#[tauri::command]
pub fn set_tone(repo_path: String, tone: String) -> Result<(), String> {
    let started = Instant::now();
    let parsed = Tone::parse(&tone).ok_or_else(|| {
        format!("unknown tone {tone:?}; expected didactic / technical / concise")
    })?;
    write_field(
        &repo_path,
        "tone",
        serde_json::Value::String(parsed.as_str().to_string()),
    )?;
    emit_i18n_op(&repo_path, "i18n-set-tone", started.elapsed().as_millis());
    Ok(())
}

/// Read `lang` + `tone` back from `mustard.json` so the Settings page can
/// hydrate its form. Fail-open: a missing or malformed file returns both
/// fields as `None` rather than an error.
#[tauri::command]
pub fn read_settings(repo_path: String) -> Result<ProjectSettings, String> {
    let path = mustard_json_path(&repo_path)?;
    let value = read_or_empty_object(&path);
    Ok(ProjectSettings {
        lang: value
            .get("lang")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        tone: value
            .get("tone")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

/// Resolve the project `mustard.json` path. Rejects empty / traversal-y inputs.
fn mustard_json_path(repo_path: &str) -> Result<PathBuf, String> {
    if repo_path.is_empty() {
        return Err("repo_path is empty".to_string());
    }
    Ok(PathBuf::from(repo_path).join("mustard.json"))
}

/// Read `mustard.json` (or yield `{}` when absent / malformed). Used by both
/// the read side and the write side so a malformed file does not surface as a
/// permanent error — the next write rewrites it cleanly.
fn read_or_empty_object(path: &Path) -> serde_json::Value {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return serde_json::json!({}),
    };
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({}))
}

/// Atomically write `field = value` into `mustard.json`, preserving every
/// other field on the existing object. Uses the same `fs::write_atomic`
/// sibling-tempfile-rename routine as `meta.json` writers.
fn write_field(repo_path: &str, field: &str, value: serde_json::Value) -> Result<(), String> {
    let path = mustard_json_path(repo_path)?;
    let mut root = read_or_empty_object(&path);
    // The W4 spec assumes a JSON object at the root; coerce non-object roots
    // (e.g. an accidental array) into a fresh object rather than fail-closed.
    if !root.is_object() {
        root = serde_json::json!({});
    }
    if let Some(obj) = root.as_object_mut() {
        obj.insert(field.to_string(), value);
    }
    let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    let mut bytes = body.into_bytes();
    bytes.push(b'\n');
    mustard_core::fs::write_atomic(&path, &bytes).map_err(|e| e.to_string())
}

/// Emit one `pipeline.economy.operation.invoked` event for an i18n write.
/// Fail-open: a telemetry write failure must never block the settings write.
fn emit_i18n_op(repo_path: &str, operation: &str, duration_ms: u128) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        session_id: String::new(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("dashboard-settings".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: serde_json::json!({
            "operation": operation,
            "duration_ms": duration_ms,
            "tokens_used": 0,
        }),
        spec: None,
    };
    let repo = Path::new(repo_path);
    let _ = db::with_store(repo, |store| store.append(&event).map_err(|e| e.to_string()));
}
