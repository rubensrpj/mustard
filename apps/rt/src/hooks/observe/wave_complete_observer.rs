//! `wave_complete_observer` — auto-emit `pipeline.wave.complete` (F4-c item 2).
//!
//! ## Decision 6 — auto-abertura por tipo (wave advance is *structural* → automatic)
//!
//! When the subagent that executed a wave returns (`SubagentStop`) and the wave
//! is **deterministically complete**, this observer auto-emits the
//! `pipeline.wave.complete` event so the parent's progress (`currentWave` /
//! `completedWaves`) advances without the LLM having to call `emit-pipeline`.
//! The emission re-uses the existing canonical emitter
//! [`crate::commands::event::emit_pipeline::run`] (module-qualified — no
//! facade), which already syncs the wave's `spec.md` + `meta.json` headers and
//! bumps the parent's progress fields.
//!
//! ## "Deterministically complete" — the source of truth
//!
//! A wave is complete when its `_review-spans.md` ledger
//! ([`crate::commands::review::review_spans`]) records **≥1 returned child** and
//! **no red verdict** (consolidation Allowed). The span ledger is the same
//! deterministic source the W5 regression gate and `close_orchestrate` already
//! read; a red entry means a child failed the behaviour gate, so the wave is
//! *not* done and no completion is emitted. The active wave is resolved from the
//! `MUSTARD_ACTIVE_SPEC` + `MUSTARD_ACTIVE_WAVE` env vars (same lookup
//! [`crate::hooks::task::subagent_inject`] uses for its span eval).
//!
//! ## Idempotency
//!
//! Before emitting, the observer scans the spec's per-spec NDJSON `.events/`
//! dir for an existing `pipeline.wave.complete` whose payload `wave` equals this
//! wave's number. If one is present the emit is skipped — a second `SubagentStop`
//! for the same wave is a no-op, so the parent's `completedWaves` never double
//! counts.
//!
//! ## Role — observer, fail-open, NEVER denies
//!
//! Pure [`Observer`]: returns `()`, cannot block. Every IO step degrades to a
//! no-op. `MUSTARD_WAVE_COMPLETE_OBSERVER_MODE=off` disables it; any other value
//! (default) is `on`. No `deny`/`strict` mode — wave advance is bookkeeping.

use crate::commands::review::review_spans::{self, ConsolidationCheck};
use crate::shared::events::economy;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::domain::model::event::ActorKind;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The auto wave-complete observer.
pub struct WaveCompleteObserver;

/// Whether the observer is enabled. `off` (case-insensitive) disables it; any
/// other value — including unset — is `on`.
fn is_off() -> bool {
    std::env::var("MUSTARD_WAVE_COMPLETE_OBSERVER_MODE")
        .unwrap_or_default()
        .eq_ignore_ascii_case("off")
}

/// Resolve `(spec, wave_dir, wave_number)` for the active wave, or `None` when
/// no wave is active or the directory is missing. Reads `MUSTARD_ACTIVE_SPEC` +
/// `MUSTARD_ACTIVE_WAVE`. Pure trigger predicate — no side effects.
fn active_wave(cwd: &str) -> Option<(String, PathBuf, u32)> {
    let spec = std::env::var("MUSTARD_ACTIVE_SPEC").ok().filter(|s| !s.is_empty())?;
    let wave = std::env::var("MUSTARD_ACTIVE_WAVE").ok().filter(|s| !s.is_empty())?;
    let claude = ClaudePaths::for_project(Path::new(cwd)).ok()?;
    let spec_paths = claude.for_spec(&spec).ok()?;
    // The wave env var carries either the bare number ("5") or the slug
    // ("wave-5-rt"). Resolve to the on-disk wave directory, then parse the
    // number back out of its name so the emitted payload carries an integer.
    let wave_dir = resolve_wave_dir(spec_paths.dir(), &wave)?;
    let number = wave_number_from_dir(&wave_dir)?;
    Some((spec, wave_dir, number))
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

/// `true` when the wave is deterministically complete: ≥1 child returned and
/// the consolidation check is Allowed (no red verdict on the ledger). A wave
/// whose ledger is empty (no child ever returned) is *not* complete.
fn wave_is_complete(wave_dir: &Path) -> bool {
    let returned = review_spans::read_entries(wave_dir);
    if returned.is_empty() {
        return false;
    }
    matches!(review_spans::check_consolidation(wave_dir), ConsolidationCheck::Allowed)
}

/// `true` when a `pipeline.wave.complete` event for `wave` already exists in the
/// spec's per-spec NDJSON `.events/` dir — the idempotency guard.
fn already_emitted(cwd: &str, spec: &str, wave: u32) -> bool {
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
        e.event == "pipeline.wave.complete"
            && e.payload.get("wave").and_then(Value::as_u64) == Some(u64::from(wave))
    })
}

impl Observer for WaveCompleteObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if is_off() {
            return;
        }
        if ctx.trigger != Some(Trigger::SubagentStop) {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        let Some((spec, wave_dir, wave)) = active_wave(&cwd) else {
            return;
        };
        if !wave_is_complete(&wave_dir) {
            return;
        }
        // Idempotency: never emit the same wave's completion twice.
        if already_emitted(&cwd, &spec, wave) {
            return;
        }
        // Re-use the canonical emitter (module-qualified, no facade). It routes
        // the event, syncs the wave headers, and bumps the parent progress.
        crate::commands::event::emit_pipeline::run(
            crate::commands::event::emit_pipeline::EmitPipelineOpts {
                kind: "pipeline.wave.complete".to_string(),
                spec: spec.clone(),
                payload: Some(json!({ "wave": wave }).to_string()),
                allow_no_qa: false,
                intent: None,
            },
        );
        economy::emit(
            &cwd,
            ActorKind::Hook,
            "wave_complete_observer",
            "pipeline.economy.operation.invoked",
            None,
            json!({ "operation": "wave_complete_observer.emit", "wave": wave, "duration_ms": 0, "tokens_used": 0 }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::review::review_spans::{VerdictEntry, VERDICT_GREEN, VERDICT_RED};
    use tempfile::tempdir;

    fn green(child: &str) -> VerdictEntry {
        VerdictEntry {
            verdict: VERDICT_GREEN.to_string(),
            child_id: child.to_string(),
            iso_ts: "2026-05-28T00:00:00Z".to_string(),
            signal_count: 0,
            first_message: String::new(),
        }
    }

    fn red(child: &str) -> VerdictEntry {
        VerdictEntry {
            verdict: VERDICT_RED.to_string(),
            child_id: child.to_string(),
            iso_ts: "2026-05-28T00:00:00Z".to_string(),
            signal_count: 1,
            first_message: "stub".to_string(),
        }
    }

    #[test]
    fn empty_ledger_is_not_complete() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-3-rt");
        std::fs::create_dir_all(&wave).unwrap();
        assert!(!wave_is_complete(&wave));
    }

    #[test]
    fn clean_ledger_with_a_child_is_complete() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-3-rt");
        review_spans::append_verdict(&wave, &green("c1")).unwrap();
        assert!(wave_is_complete(&wave));
    }

    #[test]
    fn red_ledger_is_not_complete() {
        let dir = tempdir().unwrap();
        let wave = dir.path().join("wave-3-rt");
        review_spans::append_verdict(&wave, &green("c1")).unwrap();
        review_spans::append_verdict(&wave, &red("c2")).unwrap();
        assert!(!wave_is_complete(&wave));
    }

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
        // Bare number → resolves the `wave-4-*` dir.
        assert_eq!(
            resolve_wave_dir(spec_dir, "4"),
            Some(spec_dir.join("wave-4-core"))
        );
        // Full slug → resolves directly.
        assert_eq!(
            resolve_wave_dir(spec_dir, "wave-4-core"),
            Some(spec_dir.join("wave-4-core"))
        );
        // Unknown → None.
        assert_eq!(resolve_wave_dir(spec_dir, "9"), None);
    }

    #[test]
    fn already_emitted_detects_prior_wave_complete() {
        use crate::shared::events::writer_ndjson::write_event;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let project = dir.path();
        // No event yet → false.
        assert!(!already_emitted(project.to_str().unwrap(), "specC", 2));
        // Seed a pipeline.wave.complete for wave 2.
        let _ = write_event(
            project, Some("specC"), None, "s", "pipeline.wave.complete", "pipeline",
            Some(0), Some("s"), Some("test"), None, &json!({ "wave": 2 }),
        );
        assert!(already_emitted(project.to_str().unwrap(), "specC", 2));
        // A different wave number is still unseen.
        assert!(!already_emitted(project.to_str().unwrap(), "specC", 3));
    }

    #[test]
    fn observer_no_ops_without_active_wave_env() {
        // No MUSTARD_ACTIVE_SPEC/WAVE → active_wave None → observe is a no-op.
        let dir = tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::SubagentStop),
            workspace_root: None,
        };
        let input = HookInput {
            hook_event_name: Some("SubagentStop".to_string()),
            ..HookInput::default()
        };
        WaveCompleteObserver.observe(&input, &ctx); // must not panic
    }
}
