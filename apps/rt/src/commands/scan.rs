//! `scan` — mine the workspace into `grain.model.json` via the bundled grain
//! tool. This is THE scan now: it replaces the old in-tree scan engine
//! (miner / ast / vocabulary / cluster discovery / skill+agent generation),
//! which is removed. grain is deterministic and fully
//! language-agnostic; Mustard never reads project source to understand a repo.
//!
//! The model lands at `<root>/.claude/grain.model.json` (the durable product,
//! re-run when the codebase changes). Downstream commands consume it through the
//! [`mustard_core::Scan`] client (`digest --query`, `spec`), never by reading
//! source. No skills, agents, or `.claude/` subproject artifacts are produced.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mustard_core::Scan;
use mustard_core::domain::scan::read_projects;
use serde_json::{json, Value};

use super::scan_claude;

/// Default model location under the project's `.claude/` directory.
fn default_model_path(root: &Path) -> PathBuf {
    root.join(".claude").join("grain.model.json")
}

/// Run `grain scan <root> --out <model>`; print a small JSON result. Fail-open:
/// a spawn/exit error is reported, never panics (matches the other handlers).
///
/// When `full` is `true`, (re)generates a lean CLAUDE.md per subproject after
/// the model is written, regenerating only the machine-owned scan-map block and
/// preserving every curated section verbatim. In the default mode, oversized
/// CLAUDE.md files (> [`scan_claude::CLAUDE_MD_WARN_BYTES`]) are reported in the
/// JSON output and a human-readable warning is printed to stderr.
pub fn run(root: &Path, out: Option<&Path>, full: bool) {
    let model_path = out.map_or_else(|| default_model_path(root), Path::to_path_buf);

    // Snapshot the prior model's purposes BEFORE grain overwrites the file. grain
    // re-mines structure only and writes a purpose-less model, so without this every
    // `/scan` would discard the LLM `purpose` enrichment and re-enrich the whole repo
    // (the `body_hash` incremental in `enrich-purpose` is otherwise defeated).
    let prior_purposes = std::fs::read_to_string(&model_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .map(|old| collect_purposes(&old))
        .unwrap_or_default();

    let scan_result = Scan::locate().scan(root, &model_path);

    let mut result: Value = match &scan_result {
        Ok(()) => json!({ "ok": true, "model": model_path.to_string_lossy() }),
        Err(err) => {
            eprintln!("scan: grain failed: {err}");
            json!({ "ok": false, "error": err.to_string() })
        }
    };

    // Only run the CLAUDE.md pass when grain succeeded (model file is valid).
    if scan_result.is_ok() {
        // Carry preserved purposes into the freshly-mined model, matched by
        // (path, name) so a purpose survives line drift; the carried `body_hash`
        // lets a later `enrich-purpose --render` re-validate and re-enrich only
        // declarations whose body actually changed.
        if !prior_purposes.is_empty() {
            if let Some(mut new_model) = std::fs::read_to_string(&model_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            {
                let restored = restore_purposes(&mut new_model, &prior_purposes);
                if restored > 0 {
                    if let Ok(s) = serde_json::to_string_pretty(&new_model) {
                        if let Err(e) =
                            mustard_core::io::fs::write_atomic(&model_path, format!("{s}\n").as_bytes())
                        {
                            eprintln!("scan: cannot rewrite model with carried purposes: {e}");
                        }
                    }
                    result["purposesCarried"] = json!(restored);
                }
            }
        }

        let projects = read_projects(&model_path);
        let pass = scan_claude::run_pass(root, &projects, full);

        if full {
            result["regenerated"] = json!(pass.regenerated);
            if !pass.over_cap.is_empty() {
                for entry in &pass.over_cap {
                    eprintln!(
                        "scan: CLAUDE.md over hard cap ({} bytes > {} ceiling): {} — not written; trim curated prose",
                        entry.bytes,
                        scan_claude::CLAUDE_MD_HARD_CAP_BYTES,
                        entry.path,
                    );
                }
                let over_cap_json: Vec<Value> = pass.over_cap.iter().map(|e| {
                    json!({ "path": e.path, "bytes": e.bytes })
                }).collect();
                result["over_cap"] = json!(over_cap_json);
                result["ok"] = json!(false);
            }
        } else {
            if !pass.oversized.is_empty() {
                for entry in &pass.oversized {
                    eprintln!(
                        "scan: CLAUDE.md oversized ({} bytes > {} threshold): {} — run with --full to regenerate",
                        entry.bytes,
                        scan_claude::CLAUDE_MD_WARN_BYTES,
                        entry.path,
                    );
                }
            }
            let oversized_json: Vec<Value> = pass.oversized.iter().map(|e| {
                json!({ "path": e.path, "bytes": e.bytes })
            }).collect();
            result["oversized"] = json!(oversized_json);
        }
    }

    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()));
}

/// Prior-model purposes captured for carry-forward into a re-mined model.
#[derive(Default)]
struct CarriedPurposes {
    /// Exact identity `(path, name, line)` → `(purpose, body_hash)`, every
    /// enriched declaration — the precise match for a re-scan that did not move
    /// the code (and the only safe match for method overloads).
    exact: HashMap<(String, String, u64), (String, Option<String>)>,
    /// `(path, name)` → `(purpose, body_hash)` for names UNIQUE within their file
    /// — lets a purpose survive line drift. Ambiguous names (overloads) are
    /// excluded so an overload never inherits a sibling's purpose.
    unique_name: HashMap<(String, String), (String, Option<String>)>,
}

impl CarriedPurposes {
    fn is_empty(&self) -> bool {
        self.exact.is_empty()
    }
}

/// Capture a prior model's enriched declarations for carry-forward.
fn collect_purposes(old: &Value) -> CarriedPurposes {
    let mut exact = HashMap::new();
    let mut counts: HashMap<(String, String), usize> = HashMap::new();
    let mut by_name: HashMap<(String, String), (String, Option<String>)> = HashMap::new();
    let Some(modules) = old.get("modules").and_then(|v| v.as_array()) else {
        return CarriedPurposes::default();
    };
    for module in modules {
        let Some(m_path) = module.get("path").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(decls) = module.get("declarations").and_then(|v| v.as_array()) else {
            continue;
        };
        for decl in decls {
            let Some(purpose) = decl.get("purpose").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(name) = decl.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let line = decl.get("line").and_then(serde_json::Value::as_u64).unwrap_or(0);
            let body_hash = decl.get("body_hash").and_then(|v| v.as_str()).map(str::to_string);
            let val = (purpose.to_string(), body_hash);
            let key = (m_path.to_string(), name.to_string());
            exact.insert((key.0.clone(), key.1.clone(), line), val.clone());
            *counts.entry(key.clone()).or_insert(0) += 1;
            by_name.insert(key, val);
        }
    }
    let unique_name = by_name
        .into_iter()
        .filter(|(k, _)| counts.get(k).copied().unwrap_or(0) == 1)
        .collect();
    CarriedPurposes { exact, unique_name }
}

/// Carry preserved purposes into a freshly-mined model: for every method/function
/// declaration that has no `purpose`, restore the prior `purpose` (+ `body_hash`)
/// — by exact `(path, name, line)` first, then by `(path, name)` when the name is
/// unique in its file (line-drift tolerance). Returns how many were carried. The
/// carried `body_hash` is the safety net: if the body actually changed,
/// `enrich-purpose --render` re-computes a different hash, flags it stale, and
/// re-enriches it, so a stale carry never persists.
fn restore_purposes(new: &mut Value, carried: &CarriedPurposes) -> usize {
    if carried.is_empty() {
        return 0;
    }
    let mut restored = 0usize;
    let Some(modules) = new.get_mut("modules").and_then(|v| v.as_array_mut()) else {
        return 0;
    };
    for module in modules.iter_mut() {
        let m_path = module
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let Some(decls) = module.get_mut("declarations").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for decl in decls.iter_mut() {
            let kind = decl.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if kind != "method" && kind != "function" {
                continue;
            }
            if decl.get("purpose").and_then(|v| v.as_str()).is_some() {
                continue;
            }
            let name = decl
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let line = decl.get("line").and_then(serde_json::Value::as_u64).unwrap_or(0);
            let hit = carried
                .exact
                .get(&(m_path.clone(), name.clone(), line))
                .or_else(|| carried.unique_name.get(&(m_path.clone(), name)));
            let Some((purpose, body_hash)) = hit else {
                continue;
            };
            let (purpose, body_hash) = (purpose.clone(), body_hash.clone());
            let Some(obj) = decl.as_object_mut() else {
                continue;
            };
            obj.insert("purpose".to_string(), Value::String(purpose));
            if let Some(h) = body_hash {
                obj.insert("body_hash".to_string(), Value::String(h));
            }
            restored += 1;
        }
    }
    restored
}

#[cfg(test)]
mod tests {
    use super::{collect_purposes, restore_purposes, CarriedPurposes};
    use serde_json::json;

    #[test]
    fn unique_name_survives_line_drift_overload_needs_exact_line() {
        // Old model: `foo` (unique), `gone` (unique, vanishes), and `over`
        // OVERLOADED at lines 20 and 40 (a god-service signature pattern).
        let old = json!({
            "modules": [{
                "path": "src/a.ts",
                "declarations": [
                    {"kind": "method", "name": "foo", "line": 10,
                     "purpose": "does foo", "body_hash": "h1"},
                    {"kind": "method", "name": "gone", "line": 50,
                     "purpose": "old gone", "body_hash": "h2"},
                    {"kind": "method", "name": "over", "line": 20,
                     "purpose": "over-20", "body_hash": "h20"},
                    {"kind": "method", "name": "over", "line": 40,
                     "purpose": "over-40", "body_hash": "h40"}
                ]
            }]
        });
        let carried = collect_purposes(&old);
        assert_eq!(carried.exact.len(), 4, "all four enriched decls indexed exactly");
        assert_eq!(carried.unique_name.len(), 2, "only foo + gone are unique names");

        // Fresh mine: `foo` drifted (10->14); `over` keeps line 20 but the second
        // overload drifted (40->44); `bar` is net-new. None have a purpose yet.
        let mut new = json!({
            "modules": [{
                "path": "src/a.ts",
                "declarations": [
                    {"kind": "method", "name": "foo", "line": 14},
                    {"kind": "method", "name": "over", "line": 20},
                    {"kind": "method", "name": "over", "line": 44},
                    {"kind": "method", "name": "bar", "line": 60}
                ]
            }]
        });
        let restored = restore_purposes(&mut new, &carried);
        // foo (unique, line-tolerant) + over@20 (exact) carry; over@44 (overload
        // that drifted → ambiguous) and bar (net-new) do not.
        assert_eq!(restored, 2);

        let decls = new["modules"][0]["declarations"].as_array().unwrap();
        assert_eq!(decls[0]["purpose"], "does foo"); // foo, line drift tolerated
        assert_eq!(decls[0]["body_hash"], "h1");
        assert_eq!(decls[1]["purpose"], "over-20"); // overload matched by exact line
        assert_eq!(decls[1]["body_hash"], "h20");
        assert!(decls[2].get("purpose").is_none()); // drifted overload → re-enrich
        assert!(decls[3].get("purpose").is_none()); // net-new
    }

    #[test]
    fn no_prior_model_carries_nothing() {
        let mut new = json!({"modules": [{"path": "a", "declarations": [
            {"kind": "method", "name": "x", "line": 1}
        ]}]});
        assert_eq!(restore_purposes(&mut new, &CarriedPurposes::default()), 0);
    }
}
