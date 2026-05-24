//! Spec-scoped Tauri commands.
//!
//! Wave 3 of `mustard-unification` — the sidecar `meta.json` is the canonical
//! home for machine-parseable lifecycle metadata (`stage`, `outcome`, `phase`,
//! `scope`, `lang`, `checkpoint`, `parent`, `isWavePlan`, `totalWaves`). This
//! module exposes a single Tauri command that reads it and emits a
//! `pipeline.economy.operation.invoked` event so `/economia` can prove that
//! the sidecar is the path being used (instead of the legacy `.md` parser).
//!
//! ## Fail-open contract
//!
//! - A missing / unreadable / malformed `meta.json` yields the default `Meta`
//!   (all fields `None`).
//! - The telemetry emit is best-effort: a store-open or append failure is
//!   logged on stderr and never propagated to the caller.
//! - Path traversal is rejected up front (the spec name must be a single
//!   directory component).

use crate::db;
use crate::fs;
use mustard_core::Meta;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::store::event_store::EventSink;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Read the `meta.json` sidecar next to a spec's `spec.md` and return its
/// fields to the dashboard. Tries the flat-layout path
/// (`.claude/spec/{spec_name}/meta.json`) first; if absent, probes one level
/// deeper for a wave-N child (`.claude/spec/{parent}/{spec_name}/meta.json`).
///
/// Emits `pipeline.economy.operation.invoked { operation: "meta-sidecar-read",
/// duration_ms }` on every call. Fail-open: a missing sidecar returns the
/// default `Meta` (all `None`); a write failure on the telemetry emit is
/// swallowed.
#[tauri::command]
pub fn read_spec_meta(repo_path: String, spec_name: String) -> Result<Meta, String> {
    // Reject traversal — `spec_name` is a single directory name, not a path.
    if spec_name.is_empty()
        || spec_name.contains('/')
        || spec_name.contains('\\')
        || spec_name.contains("..")
    {
        return Err(format!("invalid spec name: {}", spec_name));
    }

    let started = Instant::now();
    let base = PathBuf::from(&repo_path).join(".claude").join("spec");

    // 1. Standalone spec — flat layout: .claude/spec/{spec_name}/meta.json
    let direct = base.join(&spec_name).join("meta.json");
    let meta = mustard_core::read_meta(&direct).or_else(|| {
        // 2. Wave-plan child: scan one level for {parent}/{spec_name}/meta.json
        if let Ok(rd) = fs::read_dir(&base) {
            for entry in rd {
                if !entry.is_dir {
                    continue;
                }
                let candidate = entry.path.join(&spec_name).join("meta.json");
                if let Some(m) = mustard_core::read_meta(&candidate) {
                    return Some(m);
                }
            }
        }
        None
    });

    let duration_ms = started.elapsed().as_millis();
    emit_meta_sidecar_read(&repo_path, &spec_name, duration_ms);

    Ok(meta.unwrap_or_default())
}

/// Append `pipeline.economy.operation.invoked` for one `read_spec_meta` call.
/// Fail-open: any store error is silently swallowed.
fn emit_meta_sidecar_read(repo_path: &str, spec_name: &str, duration_ms: u128) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        session_id: String::new(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("read-spec-meta".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: serde_json::json!({
            "operation": "meta-sidecar-read",
            "duration_ms": duration_ms,
            "tokens_used": 0,
            "spec": spec_name,
        }),
        spec: Some(spec_name.to_string()),
    };
    let repo = Path::new(repo_path);
    // Fail-open: a telemetry write failure must never break the read path.
    let _ = db::with_store(repo, |store| store.append(&event).map_err(|e| e.to_string()));
}
