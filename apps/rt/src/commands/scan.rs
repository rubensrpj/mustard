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

        // Equivalences artifact (additive): project the dictionary the scan
        // tool wrote NEXT TO the model through the local MT sidecar into
        // `grain.equivalences.json` — the PT→EN query-expansion table the
        // `feature` retrieval feeds to `scan rank`. Fail-open by contract: a
        // missing dictionary/translator degrades to `{ok:false, reason}` in
        // the summary and never fails the scan.
        let dict_path = model_path.with_file_name("grain.dictionary.json");
        result["equivalences"] = super::scan_equivalences::generate_at(&dict_path);
    }

    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()));
}
