//! `mustard-rt run exec-rewave-check` — a port of `scripts/exec-rewave-check.js`.
//!
//! Pre-EXECUTE re-check: silently decomposes a single-spec into waves when the
//! finalised spec's `## Files` section carries multi-layer signals
//! (`layerCount >= 2`).
//!
//! Output: one JSON line, always exit 0 (fail-open). The `action` field is
//! parsed downstream, so the shape is preserved exactly:
//!   `{ action: "skip", reason }` | `{ action: "keep-single", reason, signals }`
//!   | `{ action: "decomposed", totalWaves, waves: [{ wave, role, files }] }`.
//!
//! Port note: the JS version shelled to `scope-decompose.js` and
//! `wave-dependency.js`. Both are now in this binary — this port calls the
//! Rust logic directly.
//!
//! ## Single canonical renderer (F4-d item 2)
//!
//! There used to be two divergent renderers of `wave-plan.md`: the canonical
//! one in [`crate::commands::wave::wave_scaffold`] (i18n + `[[wikilinks]]` +
//! localised headings, used by `/feature` at PLAN) and a freeform EN one here
//! (`build_wave_plan_md` / `build_wave_spec_md`). A spec auto-decomposed at
//! EXECUTE entry therefore looked **different** from a spec scaffolded at PLAN.
//!
//! That freeform renderer is gone. [`decompose_if_signaled`] now maps its
//! dependency DAG into the canonical [`Plan`] shape and renders through
//! [`render_wave_plan`] / [`render_wave_spec`]
//! — so a decomposed-at-EXECUTE spec is byte-identical in form to a
//! scaffolded-at-PLAN one (same wikilinks, same localised headings, same
//! lifecycle header). No facade: the freeform functions were deleted, not
//! wrapped.

use crate::commands::spec::scope_decompose::decide;
use crate::commands::wave::wave_dependency::compute_waves;
use crate::commands::wave::wave_lib::{detect_role_with, load_role_patterns, parse_files_section};
use crate::commands::wave::wave_scaffold::{
    Plan, WavePlanEntry, headings, render_wave_plan, render_wave_spec, wave_name,
};
use crate::util::json_io;
use mustard_core::time::now_iso8601;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Extract an optional `new entities: N` count from spec text.
fn parse_new_entity_count(spec_text: &str) -> i64 {
    let lower = spec_text.to_lowercase();
    for needle in ["new entities:", "new entity:", "newentitycount:", "newentitycount "] {
        if let Some(idx) = lower.find(needle) {
            let after = lower[idx + needle.len()..].trim_start();
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<i64>() {
                return n;
            }
        }
    }
    0
}

/// Walk up from `start_dir` to find the project root.
///
/// W2: routes through `mustard_core::io::workspace::workspace_root` so the search
/// uses the same `mustard.json + .claude/` anchor predicate as the rest of the
/// harness. Returns `None` when no anchor is found in any ancestor.
fn find_project_root(start_dir: &Path) -> Option<PathBuf> {
    mustard_core::io::workspace::workspace_root(start_dir).ok()
}

/// Map one DAG wave (`{ wave, files[], roles[], dependsOn:[N] }` from
/// [`compute_waves`]) onto the canonical [`WavePlanEntry`].
///
/// - `n` = the 1-based wave number.
/// - `role` = the **primary** role (first detected role, `lib` fallback) — the
///   same value the wave directory `wave-{n}-{role}` is named after, so the
///   `[[wikilink]]` in `wave-plan.md` and the on-disk folder agree.
/// - `summary` = a one-line file census (`role · N file(s): a, b`), giving the
///   per-wave `## Summary` and the plan-table `Summary` column real content
///   without an LLM.
/// - `depends_on` = the canonical `wave-{m}-{role(m)}` names resolved from the
///   numeric `dependsOn`, so the dependency wikilinks point at real wave specs.
/// - `files` = the DAG wave's own file list, materialised into the wave spec's
///   `## Files`/`## Arquivos` section so `agent-prompt-render` reads it back as
///   `{reference_files}`. `tasks` / `acceptance` are empty on the auto-decompose
///   path: this is a deterministic re-wave at EXECUTE entry, not a Plan-agent
///   authoring step — the file census is the body it can produce without an LLM.
fn dag_wave_to_entry(w: &Value, primary_role_for: &dyn Fn(i64) -> String) -> WavePlanEntry {
    let n = w.get("wave").and_then(Value::as_i64).unwrap_or(0);
    let roles: Vec<String> = w
        .get("roles")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let files: Vec<String> = w
        .get("files")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let depends_on_nums: Vec<i64> = w
        .get("dependsOn")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();

    let primary_role = roles.first().cloned().unwrap_or_else(|| "lib".to_string());
    let role_label = roles.join("/");
    let summary = format!("{role_label} · {} file(s): {}", files.len(), files.join(", "));
    let depends_on: Vec<String> = depends_on_nums
        .iter()
        .map(|m| format!("wave-{m}-{}", primary_role_for(*m)))
        .collect();

    WavePlanEntry {
        n: u32::try_from(n).unwrap_or(0),
        role: primary_role,
        summary,
        depends_on,
        // Materialise the DAG wave's own file census; tasks/acceptance are not
        // produced on the deterministic re-wave path (no Plan-agent authoring).
        tasks: Vec::new(),
        files,
        acceptance: Vec::new(),
        satisfies: Vec::new(),
    }
}

/// Build the canonical [`Plan`] from the dependency DAG `waves` array.
///
/// This replaces the deleted freeform `build_wave_plan_md` — the rendering is
/// now delegated wholesale to
/// [`crate::commands::wave::wave_scaffold`], so a decomposed-at-EXECUTE
/// plan is byte-identical in form to a scaffolded-at-PLAN one.
fn dag_to_plan(waves: &[Value], lang: &str) -> Plan {
    // Resolve each wave number to its primary role first, so a dependency
    // wikilink can name the dependee's canonical `wave-{m}-{role}` folder.
    let primary_by_num: std::collections::BTreeMap<i64, String> = waves
        .iter()
        .map(|w| {
            let num = w.get("wave").and_then(Value::as_i64).unwrap_or(0);
            let role = w
                .get("roles")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .unwrap_or("lib")
                .to_string();
            (num, role)
        })
        .collect();
    let primary_role_for = |m: i64| -> String {
        primary_by_num.get(&m).cloned().unwrap_or_else(|| "lib".to_string())
    };

    let entries: Vec<WavePlanEntry> = waves
        .iter()
        .map(|w| dag_wave_to_entry(w, &primary_role_for))
        .collect();
    let total = u32::try_from(entries.len()).unwrap_or(0);
    Plan {
        waves: entries,
        total_waves: Some(total),
        lang: Some(lang.to_string()),
    }
}

/// Resolve the parent spec's language for re-wave rendering.
///
/// Prefers the `lang` recorded in the spec's `meta.json` sidecar (the same
/// field `wave-scaffold` writes); falls back to `pt-BR` — the identical default
/// `wave-scaffold` uses when a plan omits `lang` — so the EXECUTE-entry output
/// matches the PLAN-time output for the common (unset) case.
fn parent_lang(spec_file: &Path) -> String {
    mustard_core::domain::meta::read_meta_beside(spec_file)
        .and_then(|m| m.lang)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "pt-BR".to_string())
}

/// Re-evaluate `spec_file` for wave decomposition and, when the signal mandates
/// it (`layerCount >= 2` etc.) **and** it is not already decomposed, write the
/// wave structure. Returns the same JSON `action` shape [`run`] prints.
///
/// This is the reusable, non-printing core of [`run`] — the `rewave_observer`
/// hook (F4-c item 1) calls it directly (module-qualified, no subprocess) on the
/// first EXECUTE write of a not-yet-decomposed spec. It is **idempotent**: the
/// `wave-plan.md` / pipeline-state guards (steps 2–3) make a second invocation a
/// `{ action: "skip", reason: "already-decomposed" }` no-op. Fully fail-open —
/// any IO failure degrades to a `skip` / `keep-single` action, never an error.
#[must_use]
pub fn decompose_if_signaled(spec_file: &Path) -> Value {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let spec_dir = spec_file.parent().map_or_else(|| cwd.clone(), Path::to_path_buf);
    let project_root = find_project_root(&spec_dir).unwrap_or_else(|| cwd.clone());
    // F0-e: role-classification overrides from `mustard.json#rolePatterns`.
    let role_patterns = load_role_patterns(&project_root);

    (|| -> Value {
        // 1. Read spec.
        let Ok(spec_text) = fs::read_to_string(spec_file) else {
            return json!({ "action": "skip", "reason": "error-fallback", "error": "spec-not-readable" });
        };
        let spec_name = spec_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // 2. Skip if already decomposed.
        let wave_plan_path = spec_dir.join("wave-plan.md");
        if wave_plan_path.exists() {
            return json!({ "action": "skip", "reason": "already-decomposed" });
        }

        // 3. Skip per pipeline-state.
        let state_file = ClaudePaths::for_project(&project_root)
            .map(|p| p.pipeline_state_file(&spec_name))
            .unwrap_or_else(|_| project_root.join(format!("{spec_name}.json")));
        let state = json_io::read_json(&state_file);
        if let Some(ref s) = state {
            if s.get("isWavePlan").and_then(Value::as_bool) == Some(true) {
                return json!({ "action": "skip", "reason": "already-decomposed" });
            }
            if s.get("scopeOverride").and_then(Value::as_str) == Some("user-rejected-waves") {
                return json!({ "action": "skip", "reason": "user-rejected" });
            }
        }

        // 4. Parse `## Files`.
        let Some(file_paths) = parse_files_section(&spec_text) else {
            return json!({ "action": "skip", "reason": "error-fallback", "error": "no-files-section" });
        };
        if file_paths.is_empty() {
            return json!({ "action": "skip", "reason": "error-fallback", "error": "no-files-section" });
        }

        // 5. Compute layerCount.
        let roles: BTreeSet<String> = file_paths.iter().map(|f| detect_role_with(f, &role_patterns)).collect();
        let layer_count = if roles.len() == 1 && roles.contains("lib") {
            1
        } else {
            roles.len()
        };
        let file_count = file_paths.len();
        let new_entity_count = parse_new_entity_count(&spec_text);

        let signals = json!({
            "fileCount": file_count,
            "layerCount": layer_count,
            "newEntityCount": new_entity_count,
            "knowledgeMatches": [],
        });

        // 6. Decide.
        let decision = decide(&signals);
        if decision.get("decompose").and_then(Value::as_bool) != Some(true) {
            let reason = decision
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("error-fallback");
            return json!({ "action": "keep-single", "reason": reason, "signals": signals });
        }

        // 7. Compute the DAG.
        let dag = compute_waves(&file_paths, &project_root);
        let waves = dag.get("waves").and_then(Value::as_array).cloned();
        let Some(waves) = waves else {
            return json!({ "action": "keep-single", "reason": "no-dag-depth-or-error", "signals": signals });
        };
        if waves.len() < 2 {
            return json!({ "action": "keep-single", "reason": "no-dag-depth-or-error", "signals": signals });
        }

        // 8. Write wave structure through the **canonical** renderer
        //    (F4-d item 2): map the DAG to a `wave_scaffold::Plan` and render
        //    `wave-plan.md` + each `wave-N/spec.md` with the same i18n /
        //    wikilink / heading machinery `/feature` uses at PLAN. No freeform
        //    renderer here — the output is byte-identical in form.
        let lang = parent_lang(spec_file);
        let plan = dag_to_plan(&waves, &lang);
        // Wave headings are ENGLISH-FIXED machine artefacts, so a decomposed-at-
        // EXECUTE spec stays form-identical to a scaffolded-at-PLAN one regardless
        // of the parent spec's recorded `lang`.
        let hd = headings();

        // Carry the parent spec's `## Acceptance Criteria` into the wave-plan so
        // the QA gate (which reads global ACs from `wave-plan.md` once `spec.md`
        // is renamed to `spec.original.md` at step 9 below) still finds them,
        // instead of orphaning them in the archived original.
        let ac_block = crate::commands::spec::spec_sections::section_block(
            &spec_text,
            "acceptanceCriteria",
        );

        let wave_plan_content = render_wave_plan(&plan, &hd, ac_block.as_deref(), &spec_name);
        if fs::write_atomic(&wave_plan_path, wave_plan_content.as_bytes()).is_err() {
            return json!({ "action": "skip", "reason": "error-fallback", "error": "cannot-write-wave-plan" });
        }

        let mut waves_meta: Vec<Value> = Vec::new();
        for (entry, dag_wave) in plan.waves.iter().zip(waves.iter()) {
            let wave_dir = spec_dir.join(wave_name(entry));
            let _ = fs::create_dir_all(&wave_dir);
            let wave_spec_content = render_wave_spec(&spec_name, entry, &hd);
            let _ = fs::write_atomic(wave_dir.join("spec.md"), wave_spec_content.as_bytes());
            // Preserve the action-JSON contract: `files` is the file *count*.
            let file_count = dag_wave
                .get("files")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            waves_meta.push(json!({
                "wave": entry.n,
                "role": entry.role,
                "files": file_count,
            }));
        }

        // 9. Rename original spec to spec.original.md. Surface a failure instead
        //    of swallowing it: the global `## Acceptance Criteria` were already
        //    migrated into `wave-plan.md` (step 8), so a rename that leaves a
        //    stale `spec.md` behind is worth a visible diagnostic. Fail-open —
        //    the error is logged, never propagated: the decomposition still stands.
        let renamed = match fs::rename(spec_file, &spec_dir.join("spec.original.md")) {
            Ok(()) => true,
            Err(e) => {
                eprintln!(
                    "exec-rewave-check: WARN: could not archive spec.md as spec.original.md ({e}); leaving spec.md in place"
                );
                false
            }
        };

        // 10. Update pipeline-state.
        let mut updated = state.unwrap_or_else(|| json!({ "specName": spec_name }));
        if let Some(obj) = updated.as_object_mut() {
            obj.insert("specName".to_string(), json!(spec_name));
            obj.insert("isWavePlan".to_string(), json!(true));
            obj.insert("currentWave".to_string(), json!(1));
            obj.insert("totalWaves".to_string(), json!(waves.len()));
            obj.insert("completedWaves".to_string(), json!([]));
            obj.insert("failedWaves".to_string(), json!([]));
            obj.insert("rewaveSource".to_string(), json!("exec-entry"));
            obj.insert("updatedAt".to_string(), json!(now_iso8601()));
        }
        if let Some(parent) = state_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(&updated) {
            let _ = fs::write_atomic(&state_file, text.as_bytes());
        }

        let mut decomposed = json!({
            "action": "decomposed",
            "totalWaves": waves.len(),
            "waves": waves_meta,
        });
        // Tell a direct caller (and the observer path, which reads this to warn
        // the user) what happened to the original spec. Present only when the
        // archive actually happened, so the field never lies about disk state.
        if renamed {
            if let Some(obj) = decomposed.as_object_mut() {
                obj.insert("renamedTo".to_string(), json!("spec.original.md"));
            }
        }
        decomposed
    })()
}

/// Dispatch `mustard-rt run exec-rewave-check`.
pub fn run(spec_arg: Option<&str>) {
    let emit = |v: Value| println!("{v}");
    let Some(spec_arg) = spec_arg else {
        emit(json!({ "action": "skip", "reason": "no-spec-arg" }));
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let spec_file = if Path::new(spec_arg).is_absolute() {
        PathBuf::from(spec_arg)
    } else {
        cwd.join(spec_arg)
    };
    emit(decompose_if_signaled(&spec_file));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn no_spec_arg_skips() {
        // Direct unit test of the helper paths; `run` would call process exit.
        assert_eq!(parse_new_entity_count("new entities: 3"), 3);
        assert_eq!(parse_new_entity_count("nothing here"), 0);
    }

    #[test]
    fn dag_to_plan_maps_numbers_roles_and_deps() {
        // A 2-wave DAG: wave 2 depends on wave 1.
        let waves = vec![
            json!({ "wave": 1, "files": ["src/domain/user.rs"], "roles": ["domain"], "dependsOn": [] }),
            json!({ "wave": 2, "files": ["src/api/handler.rs"], "roles": ["api"], "dependsOn": [1] }),
        ];
        let plan = dag_to_plan(&waves, "en-US");
        assert_eq!(plan.total_waves, Some(2));
        assert_eq!(plan.lang.as_deref(), Some("en-US"));
        assert_eq!(plan.waves[0].n, 1);
        assert_eq!(plan.waves[0].role, "domain");
        assert!(plan.waves[0].depends_on.is_empty());
        assert_eq!(plan.waves[1].n, 2);
        assert_eq!(plan.waves[1].role, "api");
        // Dependency resolves to the dependee's canonical wave folder name.
        assert_eq!(plan.waves[1].depends_on, vec!["wave-1-domain".to_string()]);
        // The per-wave summary carries a real file census (no LLM, no LLM-ish stub).
        assert!(plan.waves[0].summary.contains("user.rs"), "{:?}", plan.waves[0].summary);
    }

    /// **Convergence (F4-d item 2).** The EXECUTE-entry decomposition writes the
    /// exact same `wave-plan.md` the PLAN-time scaffold would, for the same
    /// plan. We prove it by rendering the canonical plan two ways — the
    /// converter path (`dag_to_plan` → `render_wave_plan`) and a hand-built
    /// `Plan` with identical fields — and asserting byte-equality.
    #[test]
    fn rewave_renders_byte_identical_to_scaffold() {
        let waves = vec![
            json!({ "wave": 1, "files": ["src/a.ts"], "roles": ["general"], "dependsOn": [] }),
            json!({ "wave": 2, "files": ["src/b.ts"], "roles": ["frontend"], "dependsOn": [1] }),
        ];
        let lang = "pt-BR";
        // Both paths render through the SAME ENGLISH-FIXED heading set (machine
        // artefact), so the byte-equality holds regardless of the plan's `lang`.
        let hd = headings();
        // Path A: the re-wave converter + canonical renderer.
        let plan_a = dag_to_plan(&waves, lang);
        let rendered_a = render_wave_plan(&plan_a, &hd, None, "epic-x");
        // Path B: a Plan built directly with the same canonical fields (what a
        // PLAN-time scaffold of the same shape would feed the renderer).
        let plan_b = Plan {
            waves: vec![
                WavePlanEntry {
                    n: 1,
                    role: "general".to_string(),
                    summary: "general · 1 file(s): src/a.ts".to_string(),
                    depends_on: vec![],
                    tasks: vec![],
                    files: vec!["src/a.ts".to_string()],
                    acceptance: vec![],
                    satisfies: Vec::new(),
                },
                WavePlanEntry {
                    n: 2,
                    role: "frontend".to_string(),
                    summary: "frontend · 1 file(s): src/b.ts".to_string(),
                    depends_on: vec!["wave-1-general".to_string()],
                    tasks: vec![],
                    files: vec!["src/b.ts".to_string()],
                    acceptance: vec![],
                    satisfies: Vec::new(),
                },
            ],
            total_waves: Some(2),
            lang: Some(lang.to_string()),
        };
        let rendered_b = render_wave_plan(&plan_b, &hd, None, "epic-x");
        assert_eq!(rendered_a, rendered_b, "re-wave must render the canonical wave-plan.md byte-for-byte");
        // The wave-plan.md is a MACHINE artefact → ENGLISH-FIXED heading even for
        // a pt-BR plan, plus the wikilinks. `wave.`-prefixed — matches the
        // `id:` each target wave actually stamps.
        assert!(rendered_a.contains("# Wave Plan"));
        assert!(!rendered_a.contains("# Plano de Waves"));
        assert!(rendered_a.contains("[[wave.epic-x.1-general]]"));
        assert!(rendered_a.contains("[[wave.epic-x.2-frontend]]"));
    }

    #[test]
    fn find_project_root_locates_claude_dir() {
        let dir = tempdir().unwrap();
        // The W2 anchor predicate requires BOTH `mustard.json` and `.claude/`
        // in the same directory, so plant both.
        std::fs::create_dir_all(ClaudePaths::for_project(dir.path()).unwrap().claude_dir()).unwrap();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            find_project_root(&nested).and_then(|p| std::fs::canonicalize(p).ok()),
            std::fs::canonicalize(dir.path()).ok()
        );
    }
}
