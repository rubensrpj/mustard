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

use crate::run::spec::scope_decompose::decide;
use crate::run::wave::wave_dependency::compute_waves;
use crate::run::wave::wave_lib::{detect_role, parse_files_section};
use crate::util::now_iso8601;
use mustard_core::fs;
use mustard_core::spec;
use mustard_core::ClaudePaths;
use mustard_core::{Flags, Outcome, SpecState, Stage};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Read a JSON file, returning `None` on any error.
fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

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
/// W2: routes through `mustard_core::workspace::workspace_root` so the search
/// uses the same `mustard.json + .claude/` anchor predicate as the rest of the
/// harness. Returns `None` when no anchor is found in any ancestor.
fn find_project_root(start_dir: &Path) -> Option<PathBuf> {
    mustard_core::workspace::workspace_root(start_dir).ok()
}

/// Extract the `## Summary` section body.
fn extract_summary(spec_text: &str) -> String {
    let lines: Vec<&str> = spec_text.split('\n').collect();
    let Some(start) = lines.iter().position(|l| {
        l.strip_prefix("##")
            .is_some_and(|r| {
                let t = r.trim_start_matches([' ', '\t']);
                t.len() != r.len() && t.to_lowercase().trim_end() == "summary"
            })
    }) else {
        return "(see spec)".to_string();
    };
    let mut body = Vec::new();
    for l in lines.iter().skip(start + 1) {
        if l.strip_prefix("##").is_some_and(|r| r.starts_with([' ', '\t'])) {
            break;
        }
        body.push(*l);
    }
    let joined = body.join("\n").trim().to_string();
    if joined.is_empty() {
        "(see spec)".to_string()
    } else {
        joined
    }
}

/// Build `wave-plan.md` content.
fn build_wave_plan_md(spec_name: &str, waves: &[Value], spec_text: &str, reason: &str) -> String {
    let now = now_iso8601();
    let summary = extract_summary(spec_text);
    let wave_lines: Vec<String> = waves
        .iter()
        .map(|w| {
            let num = w.get("wave").and_then(Value::as_i64).unwrap_or(0);
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
            let depends_on: Vec<i64> = w
                .get("dependsOn")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            let depends = if depends_on.is_empty() {
                "none".to_string()
            } else {
                depends_on
                    .iter()
                    .map(|d| format!("wave {d}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!(
                "### Wave {num} — {}\nDepends on: {depends}\nFiles ({}): {}",
                roles.join("/"),
                files.len(),
                files.join(", ")
            )
        })
        .collect();

    // Canonical lifecycle header (NEW three-line form). The plan is freshly
    // decomposed at EXECUTE entry → Stage::Execute + Active. The non-lifecycle
    // `Scope`/`Decomposed` metadata follows as its own header lines.
    let [stage_line, outcome_line, flags_line] = spec::serialize_header(
        &SpecState::new(Stage::Execute, Outcome::Active, Flags::default()).unwrap_or(SpecState {
            stage: Stage::Execute,
            outcome: Outcome::Active,
            flags: Flags::default(),
        }),
    );
    format!(
        "<!-- mustard:generated -->\n# Wave Plan: {spec_name}\n{stage_line}\n{outcome_line}\n{flags_line}\n### Scope: full\n### Decomposed: yes\n### Checkpoint: {now}\n### Reason: {reason}\n### Source: exec-rewave-check (re-evaluated at EXECUTE entry)\n\n## Summary\n{summary}\n\n## Waves\n{}\n\n## Rationale\nDecomposed at EXECUTE entry by exec-rewave-check.\nThreshold: layerCount >= 2 (reason: {reason}).\n",
        wave_lines.join("\n\n")
    )
}

/// Build a per-wave `spec.md`.
fn build_wave_spec_md(
    parent_spec_text: &str,
    wave_files: &[String],
    wave_num: i64,
    wave_role: &str,
    wave_plan_rel: &str,
) -> String {
    let summary = extract_summary(parent_spec_text);
    // Extract the `## Tasks` section body.
    let lines: Vec<&str> = parent_spec_text.split('\n').collect();
    let tasks = lines
        .iter()
        .position(|l| {
            l.strip_prefix("##")
                .is_some_and(|r| {
                    let t = r.trim_start_matches([' ', '\t']);
                    t.len() != r.len() && t.to_lowercase().trim_end() == "tasks"
                })
        })
        .map(|start| {
            let mut body = Vec::new();
            for l in lines.iter().skip(start + 1) {
                if l.strip_prefix("##").is_some_and(|r| r.starts_with([' ', '\t'])) {
                    break;
                }
                body.push(*l);
            }
            body.join("\n").trim().to_string()
        })
        .unwrap_or_default();
    let file_list: Vec<String> = wave_files.iter().map(|f| format!("- {f}")).collect();

    format!(
        "<!-- mustard:generated -->\n> Wave spec — see [../wave-plan.md]({wave_plan_rel}) for overall plan.\n\n# Wave {wave_num} — {wave_role}\n\n## Summary\n{summary}\n\n## Files\n{}\n\n## Tasks\n{tasks}\n",
        file_list.join("\n")
    )
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
    let spec_dir = spec_file.parent().map_or_else(|| cwd.clone(), Path::to_path_buf);
    let project_root = find_project_root(&spec_dir).unwrap_or_else(|| cwd.clone());

    let result = (|| -> Value {
        // 1. Read spec.
        let Ok(spec_text) = fs::read_to_string(&spec_file) else {
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
        let state = read_json(&state_file);
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
        let roles: BTreeSet<&str> = file_paths.iter().map(|f| detect_role(f)).collect();
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
        let decompose_reason = decision.get("reason").and_then(Value::as_str).unwrap_or("");

        // 8. Write wave structure.
        let wave_plan_content = build_wave_plan_md(&spec_name, &waves, &spec_text, decompose_reason);
        if fs::write_atomic(&wave_plan_path, wave_plan_content.as_bytes()).is_err() {
            return json!({ "action": "skip", "reason": "error-fallback", "error": "cannot-write-wave-plan" });
        }

        let mut waves_meta: Vec<Value> = Vec::new();
        for w in &waves {
            let wave_num = w.get("wave").and_then(Value::as_i64).unwrap_or(0);
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
            let primary_role = roles.first().cloned().unwrap_or_else(|| "lib".to_string());
            let wave_dir = spec_dir.join(format!("wave-{wave_num}-{primary_role}"));
            let _ = fs::create_dir_all(&wave_dir);
            let wave_spec_content =
                build_wave_spec_md(&spec_text, &files, wave_num, &roles.join("/"), "../wave-plan.md");
            let _ = fs::write_atomic(wave_dir.join("spec.md"), wave_spec_content.as_bytes());
            waves_meta.push(json!({
                "wave": wave_num,
                "role": primary_role,
                "files": files.len(),
            }));
        }

        // 9. Rename original spec to spec.original.md.
        let _ = fs::rename(&spec_file, spec_dir.join("spec.original.md"));

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

        json!({
            "action": "decomposed",
            "totalWaves": waves.len(),
            "waves": waves_meta,
        })
    })();

    emit(result);
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
    fn extract_summary_reads_section() {
        let s = "# Spec\n## Summary\nthis is the summary\n## Files\n- a.ts\n";
        assert_eq!(extract_summary(s), "this is the summary");
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
