//! `mustard-rt run task-checklist` — domain-specific audit checklist.
//!
//! Ports the `Domain Checklists` table in `task/SKILL.md`. Given a domain
//! token, returns the canonical bullet-list of items a `/mustard:task audit`
//! agent should walk through. Pure transform: no IO, no telemetry beyond the
//! universal economy event.
//!
//! ## Domains
//!
//! | Domain         | Items |
//! |----------------|-------|
//! | `copy`         | Tone consistency, grammar, placeholder text, marketing claims accuracy, CTA clarity |
//! | `design`       | Token usage, component reuse, visual hierarchy, spacing consistency, dark/light parity |
//! | `a11y`         | ARIA labels, contrast ratios, keyboard navigation, screen reader support, focus management |
//! | `i18n`         | Missing keys across locales, hardcoded strings, parameter consistency, pluralization |
//! | `consistency`  | Naming conventions, file structure, pattern adherence across modules |
//! | `api-contract` | DTO completeness, status codes, error response format, endpoint naming, versioning |
//!
//! Unknown domains return the `consistency` list (the documented fallback).

use crate::shared::context::session_id;
use crate::util::now_iso8601;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde::Serialize;
use serde_json::json;

/// Options for `mustard-rt run task-checklist`.
#[derive(Debug, Clone)]
pub struct TaskChecklistOpts {
    pub domain: String,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct ChecklistReport {
    pub domain: String,
    pub items: Vec<String>,
    pub fallback_to_consistency: bool,
}

/// Pure resolver — returns the canonical bullet list for `domain`.
#[must_use]
pub fn checklist_for(domain: &str) -> (Vec<&'static str>, bool) {
    let lc = domain.trim().to_ascii_lowercase();
    match lc.as_str() {
        "copy" => (
            vec![
                "Tone consistency",
                "Grammar",
                "Placeholder text",
                "Marketing claims accuracy",
                "CTA clarity",
            ],
            false,
        ),
        "design" => (
            vec![
                "Token usage",
                "Component reuse",
                "Visual hierarchy",
                "Spacing consistency",
                "Dark/light parity",
            ],
            false,
        ),
        "a11y" | "accessibility" => (
            vec![
                "ARIA labels",
                "Contrast ratios",
                "Keyboard navigation",
                "Screen reader support",
                "Focus management",
            ],
            false,
        ),
        "i18n" => (
            vec![
                "Missing keys across locales",
                "Hardcoded strings",
                "Parameter consistency",
                "Pluralization",
            ],
            false,
        ),
        "consistency" => (
            vec![
                "Naming conventions",
                "File structure",
                "Pattern adherence across modules",
            ],
            false,
        ),
        "api-contract" | "api" => (
            vec![
                "DTO completeness",
                "Status codes",
                "Error response format",
                "Endpoint naming",
                "Versioning",
            ],
            false,
        ),
        _ => (
            vec![
                "Naming conventions",
                "File structure",
                "Pattern adherence across modules",
            ],
            /* fallback = */ true,
        ),
    }
}

/// CLI entry.
pub fn run(opts: TaskChecklistOpts) {
    let started = std::time::Instant::now();
    let (items, fallback) = checklist_for(&opts.domain);
    let report = ChecklistReport {
        domain: opts.domain.clone(),
        items: items.into_iter().map(str::to_string).collect(),
        fallback_to_consistency: fallback,
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
            id: Some("task-checklist".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "task-checklist",
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
    fn copy_domain_returns_five_items() {
        let (items, fb) = checklist_for("copy");
        assert_eq!(items.len(), 5);
        assert!(!fb);
        assert!(items.contains(&"Grammar"));
    }

    #[test]
    fn a11y_alias_matches_accessibility() {
        let (a, _) = checklist_for("a11y");
        let (b, _) = checklist_for("accessibility");
        assert_eq!(a, b);
    }

    #[test]
    fn api_alias_matches_api_contract() {
        let (a, _) = checklist_for("api");
        let (b, _) = checklist_for("api-contract");
        assert_eq!(a, b);
    }

    #[test]
    fn unknown_domain_falls_back_to_consistency_with_flag() {
        let (items, fb) = checklist_for("bogus");
        assert!(fb);
        assert!(items.iter().any(|i| i.contains("Naming")));
    }

    #[test]
    fn case_insensitive() {
        let (a, _) = checklist_for("DESIGN");
        let (b, _) = checklist_for("design");
        assert_eq!(a, b);
    }

    #[test]
    fn json_shape_required_fields() {
        let r = ChecklistReport {
            domain: "copy".to_string(),
            items: vec!["x".to_string()],
            fallback_to_consistency: false,
        };
        let v = serde_json::to_value(r).unwrap();
        assert!(v.get("domain").is_some());
        assert!(v.get("items").unwrap().is_array());
        assert!(v.get("fallback_to_consistency").is_some());
    }
}
