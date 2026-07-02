//! `wave_start_observer` — auto-emit `pipeline.wave.start` (DEFECT 2, 2026-06-05).
//!
//! The symmetric counterpart to [`crate::hooks::observe::wave_complete_observer`].
//! Where that observer fires on `SubagentStop` and emits
//! `pipeline.wave.complete` when the wave is deterministically done, this one
//! fires on `SubagentStart` (the first child of a wave begins executing) and
//! emits a single `pipeline.wave.start` so the dashboard can mark the wave
//! `InProgress` from an explicit signal — not by inferring it from a
//! `pipeline.task.dispatch`.
//!
//! ## Why a dedicated event (not `pipeline.task.dispatch`)
//!
//! `dispatch-plan` is a pure JSON-emitting face (byte-stable, snapshot-gated):
//! it does NOT actually dispatch, so there is no code-side point inside it to
//! emit a runtime event. The real "wave started" moment is when the harness
//! starts the wave's subagent — exactly the `SubagentStart` trigger this
//! observer hooks. A `pipeline.task.dispatch` is orchestrator-authored and not
//! reliably emitted for every wave; an explicit `pipeline.wave.start` keyed off
//! the same `MUSTARD_ACTIVE_SPEC` + `MUSTARD_ACTIVE_WAVE` env vars the
//! completion observer uses makes the `in_progress = started-without-complete`
//! derivation precise.
//!
//! ## Idempotency
//!
//! A wave can fan out several parallel children, each raising `SubagentStart`.
//! Before emitting, the observer scans the spec's per-spec NDJSON `.events/`
//! dir for an existing `pipeline.wave.start` whose payload `wave` equals this
//! wave's number; if present, the emit is skipped. A wave that already emitted
//! `pipeline.wave.complete` is also skipped — a late `SubagentStart` must not
//! resurrect a finished wave as in-progress.
//!
//! ## Role — observer, fail-open, NEVER denies
//!
//! Pure [`Observer`]: returns `()`, cannot block. Every IO step degrades to a
//! no-op. `MUSTARD_WAVE_START_OBSERVER_MODE=off` disables it; any other value
//! (default) is `on`. No `deny`/`strict` mode — wave bookkeeping never blocks.

use crate::shared::events::economy;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::domain::model::event::{ActorKind, EVENT_PIPELINE_WAVE_START};
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The auto wave-start observer.
pub struct WaveStartObserver;

/// Whether the observer is enabled. `off` (case-insensitive) disables it; any
/// other value — including unset — is `on`.
fn is_off() -> bool {
    std::env::var("MUSTARD_WAVE_START_OBSERVER_MODE")
        .unwrap_or_default()
        .eq_ignore_ascii_case("off")
}

/// Resolve `(spec, wave_number)` for the active wave, or `None` when no wave is
/// active or its directory is missing. Reads `MUSTARD_ACTIVE_SPEC` +
/// `MUSTARD_ACTIVE_WAVE`. Pure trigger predicate — no side effects.
fn active_wave(cwd: &str) -> Option<(String, u32)> {
    let spec = std::env::var("MUSTARD_ACTIVE_SPEC").ok().filter(|s| !s.is_empty())?;
    let wave = std::env::var("MUSTARD_ACTIVE_WAVE").ok().filter(|s| !s.is_empty())?;
    let claude = ClaudePaths::for_project(Path::new(cwd)).ok()?;
    let spec_paths = claude.for_spec(&spec).ok()?;
    // The wave env var carries either the bare number ("5") or the slug
    // ("wave-5-rt"). Resolve to the on-disk wave directory, then parse the
    // number back out of its name so the emitted payload carries an integer.
    let wave_dir = resolve_wave_dir(spec_paths.dir(), &wave)?;
    let number = wave_number_from_dir(&wave_dir)?;
    Some((spec, number))
}

/// Find the `wave-{n}(-role)?` directory under `spec_dir` matching `wave`
/// (a bare number or a full slug). `None` when nothing matches on disk.
fn resolve_wave_dir(spec_dir: &Path, wave: &str) -> Option<PathBuf> {
    // Full slug as-is.
    let direct = spec_dir.join(wave);
    if direct.is_dir() {
        return Some(direct);
    }
    let prefix_exact = format!("wave-{wave}");
    let prefix_role = format!("wave-{wave}-");
    let entries = std::fs::read_dir(spec_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
        if name_str == prefix_exact || name_str.starts_with(&prefix_role) {
            let p = spec_dir.join(name_str);
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

/// Parse the wave number out of a `wave-{n}(-role)?` directory name.
fn wave_number_from_dir(wave_dir: &Path) -> Option<u32> {
    let name = wave_dir.file_name()?.to_str()?;
    let rest = name.strip_prefix("wave-")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u32>().ok()
}

/// `true` when a `pipeline.wave.start` (or already a `pipeline.wave.complete`)
/// for `wave` exists in the spec's per-spec NDJSON `.events/` dir. A wave that
/// has already started — or already finished — must not emit `start` again.
fn already_signalled(cwd: &str, spec: &str, wave: u32) -> bool {
    let events_dir = ClaudePaths::for_project(Path::new(cwd))
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())
        .unwrap_or_else(|| {
            ClaudePaths::compose_unchecked(Path::new(cwd))
                .spec_dir()
                .join(spec)
                .join(".events")
        });
    read_harness_events_from_ndjson_dir(&events_dir).iter().any(|e| {
        (e.event == EVENT_PIPELINE_WAVE_START || e.event == "pipeline.wave.complete")
            && e.payload.get("wave").and_then(Value::as_u64) == Some(u64::from(wave))
    })
}

impl Observer for WaveStartObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if is_off() {
            return;
        }
        if ctx.trigger != Some(Trigger::SubagentStart) {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        let Some((spec, wave)) = active_wave(&cwd) else {
            return;
        };
        // Idempotency: a wave starts exactly once, even with parallel children.
        if already_signalled(&cwd, &spec, wave) {
            return;
        }
        // Re-use the canonical emitter (module-qualified, no facade). It routes
        // the `pipeline.wave.start` event into the per-spec NDJSON sink.
        crate::commands::event::emit_pipeline::run(
            crate::commands::event::emit_pipeline::EmitPipelineOpts {
                kind: EVENT_PIPELINE_WAVE_START.to_string(),
                spec: spec.clone(),
                payload: Some(json!({ "wave": wave }).to_string()),
                allow_no_qa: false,
                intent: None,
                base: None,
            },
        );
        economy::emit(
            &cwd,
            ActorKind::Hook,
            "wave_start_observer",
            "pipeline.economy.operation.invoked",
            None,
            json!({ "operation": "wave_start_observer.emit", "wave": wave, "duration_ms": 0, "tokens_used": 0 }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn wave_number_parses_from_slug() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-12-rt");
        std::fs::create_dir_all(&wave).unwrap();
        assert_eq!(wave_number_from_dir(&wave), Some(12));
        let bare = dir.path().join("wave-7");
        std::fs::create_dir_all(&bare).unwrap();
        assert_eq!(wave_number_from_dir(&bare), Some(7));
    }

    #[test]
    fn resolve_wave_dir_matches_number_and_slug() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::create_dir_all(spec_dir.join("wave-4-core")).unwrap();
        assert_eq!(resolve_wave_dir(spec_dir, "4"), Some(spec_dir.join("wave-4-core")));
        assert_eq!(
            resolve_wave_dir(spec_dir, "wave-4-core"),
            Some(spec_dir.join("wave-4-core"))
        );
        assert_eq!(resolve_wave_dir(spec_dir, "9"), None);
    }

    #[test]
    fn already_signalled_detects_prior_start_and_complete() {
        use crate::shared::events::writer_ndjson::write_event;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let project = dir.path();
        let proj = project.to_str().unwrap();
        // No event yet → false.
        assert!(!already_signalled(proj, "specS", 2));
        // A prior wave.start for wave 2 → true (don't re-emit).
        let _ = write_event(
            project, Some("specS"), None, "s", "pipeline.wave.start", "pipeline",
            Some(0), Some("s"), Some("test"), None, &json!({ "wave": 2 }),
        );
        assert!(already_signalled(proj, "specS", 2));
        // A different wave is still unseen.
        assert!(!already_signalled(proj, "specS", 3));
        // A wave.complete for wave 3 also suppresses a late start (no resurrect).
        let _ = write_event(
            project, Some("specS"), None, "s", "pipeline.wave.complete", "pipeline",
            Some(0), Some("s"), Some("test"), None, &json!({ "wave": 3 }),
        );
        assert!(already_signalled(proj, "specS", 3));
    }

    #[test]
    fn observer_no_ops_without_active_wave_env() {
        // No MUSTARD_ACTIVE_SPEC/WAVE → active_wave None → observe is a no-op.
        let dir = tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::SubagentStart),
            workspace_root: None,
        };
        let input = HookInput {
            hook_event_name: Some("SubagentStart".to_string()),
            ..HookInput::default()
        };
        WaveStartObserver.observe(&input, &ctx); // must not panic
    }
}
