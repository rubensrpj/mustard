//! `mustard-rt run recipe-match` — match a recipe by entity + operation.
//!
//! Walks `.claude/recipes/` (flat or one-level nested under a subproject dir),
//! matches recipes by `operation`, and emits the matched recipe as pretty JSON
//! on stdout. Exits 0 silently when there is no match or no recipes directory.
//!
//! ## Path resolution
//!
//! Recipes today are **scan-derived** (project-profiler W3): every
//! `files[].path` is a real on-disk path inside the recipe's subproject — no
//! `{Entity}` / `{subproject}` placeholders to substitute. The legacy
//! convention-name lookup (`backend`/`frontend`/`admin`) and the
//! `to_pascal_case` placeholder rewriter were removed in the
//! template-agnostic-audit (Wave 4): they baked CRUD/web archetype names into
//! the runtime, which contradicts Mustard's agnostic principle. Resolution is
//! now pass-through — the recipe's `path` is the resolved path. A `pattern`
//! fallback is retained for one release in case any in-flight recipe still
//! carries the legacy key; it is emitted verbatim (no substitution).
//!
//! ## Wave 2 — economia-didatica-e-economias-reais
//!
//! When a recipe matches, this subcommand *also* persists one
//! [`SavingsSource::RecipeInjection`] row into the W1 `savings_records` table:
//! every character of skeleton we hand the agent is a character the model did
//! not have to derive, so we book the proxy via
//! [`mustard_core::domain::economy::writer::injection_savings_tokens`] (the public
//! `chars / 4` helper merged in W1). Persistence is a strict side-effect — the
//! stdout JSON shape is unchanged and any DB failure degrades to an
//! `eprintln!`, mirroring how [`super::rtk_gain::run`] handles its own
//! best-effort writes.

use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::domain::economy::{
    self,
    model::{SavingsRecord, SavingsSource},
    scope::{ProjectPath, SpecId},
};
use mustard_core::io::fs;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json, Map, Value};
use std::path::Path;

use crate::shared::context::current_spec;
use crate::shared::events::route;
use mustard_core::time::now_iso8601;

/// Dispatch `mustard-rt run recipe-match`.
pub fn run(entity: Option<&str>, operation: Option<&str>, subproject: Option<&str>) {
    let _ = subproject; // kept for CLI compat; resolution is now pass-through
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let (Some(entity), Some(operation)) = (entity, operation) else {
        return; // exit 0 silently
    };

    let Ok(paths) = ClaudePaths::for_project(&cwd) else { return; };
    let recipes_dir = paths.claude_dir().join("recipes");
    if !recipes_dir.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(&recipes_dir) else {
        return;
    };

    let operation_lower = operation.to_lowercase();
    let mut json_files: Vec<std::path::PathBuf> = entries
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.path)
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    json_files.sort();

    let mut matched: Option<Value> = None;
    for file in json_files {
        let Ok(raw) = fs::read_to_string(&file) else {
            continue;
        };
        let Ok(recipe) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(operations) = recipe.get("operations").and_then(Value::as_array) else {
            continue;
        };
        let op_matches = operations
            .iter()
            .filter_map(Value::as_str)
            .any(|op| op.to_lowercase() == operation_lower);
        if !op_matches {
            continue;
        }
        // `requires_entity` with no entity → skip; entity is always present here.
        matched = Some(recipe);
        break;
    }

    let Some(matched) = matched else {
        return; // no match — exit 0 silently
    };

    // Wave 2 (economia-didatica-e-economias-reais): book the proxy "tokens the
    // agent would have spent deriving this skeleton" before printing the legacy
    // JSON. The injection is what saved them, so the matched recipe value (the
    // very payload the agent receives in its prompt) is the load-bearing input
    // to `injection_savings_tokens`. Strict side-effect: any failure here is
    // `eprintln!` + continue — the stdout emission below is the contract.
    persist_injection_savings(&matched, &cwd);

    // Wave 4 (project-profiler): delegate the convention-closure resolution
    // to `context-resolve`. The recipe's entity + operation make a natural
    // scope; the resolver walks the concept-node graph and returns the
    // minimum closure of conventions to inject alongside the skeleton. The
    // stdout JSON of `recipe-match` stays byte-stable (the resolver's
    // closure rides on stderr as a debug line — pipeline tooling parses
    // stdout only). Fail-open: no graph / no nodes ⇒ silent skip.
    delegate_to_resolver(entity, operation, &cwd);

    let resolved_files: Vec<Value> = matched
        .get("files")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|f| {
                    // Scan-derived recipes carry a real `path`; the legacy
                    // `pattern` key is honoured verbatim as a one-release
                    // fallback for in-flight recipes (no substitution).
                    let path = f
                        .get("path")
                        .and_then(Value::as_str)
                        .or_else(|| f.get("pattern").and_then(Value::as_str))
                        .unwrap_or("");
                    json!({
                        "resolved_path": path,
                        "action": f.get("action").cloned().unwrap_or(Value::Null),
                        "hint": f.get("hint").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let output = json!({
        "recipe": matched.get("name").cloned().unwrap_or(Value::Null),
        "entity": entity,
        "operation": operation,
        "description": matched.get("description").and_then(Value::as_str).unwrap_or(""),
        "files": resolved_files,
        "checklist": matched
            .get("checklist")
            .filter(|v| v.is_array())
            .cloned()
            .unwrap_or_else(|| json!([])),
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string())
    );
}

/// Persist one [`SavingsSource::RecipeInjection`] row for the matched recipe.
///
/// Fail-open at every step:
///
/// 1. `serde_json::to_string` on the recipe value — empty string on failure,
///    which still yields `0` tokens (the floor) instead of aborting.
/// 2. `economy::store::open_for` — `eprintln!` + skip if the DB cannot be
///    opened (a fresh project with no `.claude/.harness/` yet, or the file
///    locked by a sibling writer).
/// 3. `economy::writer::record_savings` — `eprintln!` only; we never propagate
///    the error to the stdout caller.
///
/// Idempotence: the caller only invokes this when `matched.is_some()`, so we
/// write exactly one row per successful invocation. A null or empty skeleton
/// floors at `0` tokens but the row still lands — preserving the "we tried
/// to help here" signal the dashboard wants to count.
fn persist_injection_savings(matched: &Value, cwd: &Path) {
    let skeleton = serde_json::to_string(matched).unwrap_or_default();
    let tokens = economy::writer::injection_savings_tokens(&skeleton);

    let cwd_str = cwd.to_string_lossy().into_owned();
    let spec_id = current_spec(&cwd_str).map(SpecId::new);

    let record = SavingsRecord {
        ts: now_iso8601(),
        source: SavingsSource::RecipeInjection,
        tokens_saved: tokens,
        model_target: None,
        project_path: ProjectPath::new(cwd),
        spec_id,
        wave_id: None,
        agent_id: None,
        extra: Map::new(),
    };

    let (event_name, payload) = economy::writer::savings_event(&record);
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: "unknown".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("recipe-match".to_string()),
            actor_type: None,
        },
        event: event_name,
        payload,
        spec: current_spec(&cwd_str),
    };
    let _ = route::emit(&cwd_str, &event);
}

/// Wave-4 delegation: route the recipe match through the unified
/// `context-resolve` walk. The resolver is a pure function of the on-disk
/// vault — no network, no SQLite — so the cost is bounded by the size of
/// `.claude/graph/`. Output rides on stderr as a one-line summary; stdout
/// stays byte-stable for the legacy parser.
fn delegate_to_resolver(entity: &str, operation: &str, cwd: &Path) {
    let scope = crate::commands::scan::resolve::ResolveScope {
        entities: vec![entity.to_string()],
        operation: Some(operation.to_string()),
        ..crate::commands::scan::resolve::ResolveScope::default()
    };
    let out = crate::commands::scan::resolve::resolve_closure(cwd, &scope);
    if !out.closure.is_empty() {
        eprintln!(
            "recipe_match: context-resolve closure={} dist=[0..{}] truncated={}",
            out.closure.len(),
            out.closure.iter().map(|n| n.distance).max().unwrap_or(0),
            out.truncated,
        );
    }
}
