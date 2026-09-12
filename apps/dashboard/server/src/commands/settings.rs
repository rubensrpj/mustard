//! Project-scoped settings commands.
//!
//! The dashboard Settings page lets the user pick the project's text language
//! (`mustard.json` `language.text`); the write is routed through this module
//! so the validation + telemetry contract is shared with every future caller.
//! There is no tone to pick: the voice is one, plain and didactic.
//!
//! ## Layout
//!
//! - [`set_language`] — validate against [`mustard_core::SupportedLocale`], write
//!   `mustard.json` `language.text` via [`ProjectConfig`], emit telemetry.
//! - [`read_settings`] — read it back for the form initial state.
//!
//! ## Fail-open contract
//!
//! - A malformed `mustard.json` is treated as an empty object — the field is
//!   set on a fresh object and the file is rewritten atomically.
//! - Telemetry emission is best-effort; a store failure never propagates.
//! - Path traversal is rejected up front (`repo_path` must be a real dir).

use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::{ProjectConfig, SupportedLocale};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

/// The setting the dashboard reads / writes.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectSettings {
    /// The declared text language, BCP-47 (`pt-BR` / `en-US`). `None` when
    /// the project declared none.
    pub lang: Option<String>,
}

/// Write `mustard.json` `language.text` after validating against
/// [`SupportedLocale::from_str`].
///
/// Rejects the legacy short forms (`pt`/`en`) with a typed error so the
/// dashboard can surface "use pt-BR or en-US" without inferring intent.
/// Emits `pipeline.economy.operation.invoked { operation: "i18n-set-language",
/// duration_ms, tokens_used: 0 }`.
pub fn set_language(repo_path: String, lang: String) -> Result<(), String> {
    let started = Instant::now();
    let locale = SupportedLocale::from_str(&lang).map_err(|e| e.to_string())?;
    let root = repo_root(&repo_path)?;
    let mut config = ProjectConfig::load(&root);
    config.language.text = Some(locale.as_str().to_string());
    config.write(&root).map_err(|e| e.to_string())?;
    emit_i18n_op(&repo_path, "i18n-set-language", started.elapsed().as_millis());
    Ok(())
}

/// Read the declared text language back from `mustard.json`, through
/// [`ProjectConfig::language`], so the Settings page can hydrate its form.
/// Fail-open: a missing or malformed file returns `None` rather than an error.
pub fn read_settings(repo_path: String) -> Result<ProjectSettings, String> {
    let root = repo_root(&repo_path)?;
    let config = ProjectConfig::load(&root);
    Ok(ProjectSettings { lang: config.language().text.map(|lang| lang.as_str().to_string()) })
}

/// Resolve the project root from a dashboard `repo_path`. Rejects empty inputs.
/// The config IO itself is owned by [`ProjectConfig`]; this only guards the
/// argument.
fn repo_root(repo_path: &str) -> Result<PathBuf, String> {
    if repo_path.is_empty() {
        return Err("repo_path is empty".to_string());
    }
    Ok(PathBuf::from(repo_path))
}

/// Emit one `pipeline.economy.operation.invoked` event for an i18n write.
/// Fail-open: a telemetry write failure must never block the settings write.
///
/// The legacy SQLite event-store append route is gone. Settings events land
/// in a project-scoped NDJSON channel at
/// `.claude/.events/dashboard-settings.ndjson` — same atomic-append shape used
/// by every other dashboard emitter. Any IO failure is swallowed so
/// the settings write itself never gets blocked.
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
    let events_dir = Path::new(repo_path).join(".claude").join(".events");
    if std::fs::create_dir_all(&events_dir).is_err() {
        return;
    }
    let path = events_dir.join("dashboard-settings.ndjson");
    if let Ok(line) = serde_json::to_string(&event) {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}
