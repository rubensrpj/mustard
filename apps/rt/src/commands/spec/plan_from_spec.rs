//! `mustard-rt run plan-from-spec` — emit a deterministic wave-plan JSON.
//!
//! Part of the deep-refactor W10. Replaces the previous "orchestrator
//! hand-rolls plan.json" step: Rust now owns the canonical shape that
//! [`crate::commands::wave::wave_scaffold`] later materialises into spec files.
//!
//! Inputs (all positional flags):
//!   - `--waves N`         — total wave count (must be >= 1).
//!   - `--roles a,b,c,...` — comma-separated role list. If fewer than N entries
//!     are supplied, the last entry is replicated to fill the remaining slots.
//!     A single role applies to every wave.
//!   - `--lang pt-BR|en-US` — BCP-47 narrative locale (lenient — short codes
//!     are normalised via `mustard_core::normalise_lang`).
//!   - `--summary "..."`   — optional per-wave summary template; empty by default.
//!
//! Output: pretty JSON consumable directly by
//! `wave-scaffold --plan <stdin-or-file>`. The full schema this command emits:
//!
//! ```json
//! {
//!   "waves": [
//!     {
//!       "n": 1,
//!       "role": "backend",
//!       "summary": "…",
//!       "depends_on": ["wave-0-…"],
//!       "tasks": [],
//!       "files": [],
//!       "acceptance": []
//!     }
//!   ],
//!   "total_waves": 1,
//!   "lang": "pt-BR"
//! }
//! ```
//!
//! ### Per-wave body fields
//!
//! `tasks` / `files` / `acceptance` are the materialised work body for each
//! wave. This command emits them **always — even empty** — so the JSON is a
//! self-documenting skeleton: it is the deterministic role/dependency scaffold,
//! and the Plan agent folds the real `tasks` (checklist), `files` (census), and
//! `acceptance` (AC) lines into each entry before handing the plan to
//! `wave-scaffold`. `wave-scaffold` then materialises `## Tasks`/`## Tarefas` +
//! `## Files`/`## Arquivos` into each `wave-N/spec.md` and the AC union into
//! `wave-plan.md` (see [`crate::commands::wave::wave_scaffold`] for the
//! consumer-side contract). The body is therefore never hand-authored after the
//! scaffold — it lives in the plan JSON's per-wave body.
//!
//! Fail-open: invalid args print a usage line on stderr and exit non-zero so
//! the parent pipeline does not silently scaffold an empty plan.

use serde_json::json;
use mustard_core::domain::model::event::ActorKind;
use crate::shared::context;
use crate::shared::events::economy;
use serde::Serialize;

/// One wave entry rendered into the plan JSON.
///
/// `tasks` / `files` / `acceptance` are emitted always (even empty) so the
/// schema is self-documenting: the producer ships the deterministic skeleton
/// and the Plan agent folds the real body lines in before `wave-scaffold`
/// materialises them. The field names match `wave_scaffold::WavePlanEntry`'s
/// `#[serde(default)]` reader exactly.
#[derive(Debug, Serialize)]
struct WaveEntry {
    n: u32,
    role: String,
    summary: String,
    depends_on: Vec<String>,
    tasks: Vec<String>,
    files: Vec<String>,
    acceptance: Vec<String>,
}

/// Plan document. Mirrors the `Plan` struct read by `wave_scaffold` (lenient —
/// extra fields here are ignored downstream, not the other way around).
#[derive(Debug, Serialize)]
struct PlanDoc {
    waves: Vec<WaveEntry>,
    total_waves: u32,
    lang: String,
}

/// Options for `mustard-rt run plan-from-spec`.
#[derive(Debug, Clone)]
pub struct PlanFromSpecOpts {
    /// Total wave count (>= 1).
    pub waves: u32,
    /// Comma-separated role list (e.g. `backend,frontend`).
    pub roles: String,
    /// Narrative locale (`pt-BR` / `en-US`).
    pub lang: String,
    /// Optional summary string applied to every wave.
    pub summary: Option<String>,
}

/// Build the deterministic plan from `opts`. Pure; no IO. Returns `Err` only
/// when the inputs are unusable (zero waves, empty role list).
fn build_plan(opts: &PlanFromSpecOpts) -> Result<PlanDoc, String> {
    if opts.waves == 0 {
        return Err("plan-from-spec: --waves must be >= 1".to_string());
    }
    let roles: Vec<String> = opts
        .roles
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .collect();
    if roles.is_empty() {
        return Err("plan-from-spec: --roles is empty".to_string());
    }

    let summary = opts.summary.clone().unwrap_or_default();
    let mut waves = Vec::with_capacity(opts.waves as usize);
    for i in 0..opts.waves {
        // Replicate the last role when N waves exceed the role list length.
        let role_idx = (i as usize).min(roles.len() - 1);
        let role = roles[role_idx].clone();
        let n = i + 1;
        // Chain: wave N depends on wave N-1's directory name. Wave 1 has no deps.
        let depends_on = if n == 1 {
            Vec::new()
        } else {
            let prev_role_idx = ((i - 1) as usize).min(roles.len() - 1);
            vec![format!("wave-{prev}-{r}", prev = n - 1, r = roles[prev_role_idx])]
        };
        waves.push(WaveEntry {
            n,
            role,
            summary: summary.clone(),
            depends_on,
            // Skeleton body — the Plan agent folds the real checklist / file
            // census / AC into each entry before `wave-scaffold` consumes it.
            tasks: Vec::new(),
            files: Vec::new(),
            acceptance: Vec::new(),
        });
    }
    let lang = mustard_core::normalise_lang(&opts.lang);
    Ok(PlanDoc {
        waves,
        total_waves: opts.waves,
        lang,
    })
}

/// Dispatch `mustard-rt run plan-from-spec`.
pub fn run(opts: PlanFromSpecOpts) {
    let started = std::time::Instant::now();
    let plan = match build_plan(&opts) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let body = serde_json::to_string_pretty(&plan).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    economy::emit_operation(&context::cwd(), ActorKind::Orchestrator, "plan-from-spec", started.elapsed().as_millis() as u64, None, json!({"waves": opts.waves}));
}

/// Telemetry — `pipeline.economy.operation.invoked` for the run.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_plan_minimal_two_waves_two_roles() {
        let plan = build_plan(&PlanFromSpecOpts {
            waves: 2,
            roles: "a,b".to_string(),
            lang: "pt-BR".to_string(),
            summary: None,
        })
        .unwrap();
        assert_eq!(plan.total_waves, 2);
        assert_eq!(plan.waves.len(), 2);
        assert_eq!(plan.waves[0].role, "a");
        assert_eq!(plan.waves[1].role, "b");
        assert!(plan.waves[0].depends_on.is_empty());
        assert_eq!(plan.waves[1].depends_on, vec!["wave-1-a".to_string()]);
        assert_eq!(plan.lang, "pt-BR");
    }

    #[test]
    fn build_plan_role_replication() {
        let plan = build_plan(&PlanFromSpecOpts {
            waves: 3,
            roles: "rt".to_string(),
            lang: "en-US".to_string(),
            summary: Some("baseline".to_string()),
        })
        .unwrap();
        assert_eq!(plan.waves.len(), 3);
        assert!(plan.waves.iter().all(|w| w.role == "rt"));
        assert!(plan.waves.iter().all(|w| w.summary == "baseline"));
        assert_eq!(plan.waves[2].depends_on, vec!["wave-2-rt".to_string()]);
    }

    #[test]
    fn build_plan_rejects_zero_waves() {
        let err = build_plan(&PlanFromSpecOpts {
            waves: 0,
            roles: "a".to_string(),
            lang: "en-US".to_string(),
            summary: None,
        })
        .unwrap_err();
        assert!(err.contains(">= 1"));
    }

    #[test]
    fn plan_json_emits_empty_body_fields_as_self_documenting_skeleton() {
        let plan = build_plan(&PlanFromSpecOpts {
            waves: 1,
            roles: "backend".to_string(),
            lang: "pt-BR".to_string(),
            summary: None,
        })
        .unwrap();
        let json = serde_json::to_string(&plan).unwrap();
        // The body fields are present (even empty) so the schema is
        // self-documenting and `wave-scaffold`'s reader sees the exact keys.
        assert!(json.contains("\"tasks\":[]"), "tasks key emitted: {json}");
        assert!(json.contains("\"files\":[]"), "files key emitted: {json}");
        assert!(json.contains("\"acceptance\":[]"), "acceptance key emitted: {json}");
    }

    #[test]
    fn build_plan_rejects_empty_roles() {
        let err = build_plan(&PlanFromSpecOpts {
            waves: 1,
            roles: ", ,".to_string(),
            lang: "en-US".to_string(),
            summary: None,
        })
        .unwrap_err();
        assert!(err.contains("roles"));
    }
}
