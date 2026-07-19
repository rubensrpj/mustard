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
use mustard_core::domain::scan::{mark_own_git_roots, read_projects};
use serde_json::{json, Value};

use super::scan_claude;

/// Default model location under the project's `.claude/` directory.
fn default_model_path(root: &Path) -> PathBuf {
    root.join(".claude").join("grain.model.json")
}

/// Run `grain scan <root> --out <model>`; print a small JSON result. Fail-open:
/// a spawn/exit error is reported, never panics (matches the other handlers).
///
/// When `full` is `true`, (re)generates the mustard-owned
/// `.claude/scan-map.md` per subproject after the model is written, and keeps
/// the project's CLAUDE.md footprint minimal (import line + legacy-block
/// migration + Guards seed + breadcrumb heal — never measured, never
/// refused). The hard cap guards only the machine map (runaway generator).
pub fn run(root: &Path, out: Option<&Path>, full: bool) {
    let model_path = out.map_or_else(|| default_model_path(root), Path::to_path_buf);

    // Preflight BEFORE the miner: an unpopulated submodule is indistinguishable
    // from an absent subtree once the walk runs — it visits the directory, finds
    // nothing, and mines a model missing that whole subproject with no error and
    // no coverage entry. Refusing here keeps the PREVIOUS (complete) model on
    // disk, which is strictly better than replacing it with a hollow one.
    let hollow = hollow_submodules(root);
    if !hollow.is_empty() {
        for path in &hollow {
            eprintln!(
                "scan: submodule `{path}` is declared in .gitmodules but its directory is EMPTY — \
                 the model would silently omit that entire subproject."
            );
        }
        eprintln!(
            "scan: refusing to mine a hollow model (the existing one is left untouched). \
             Populate with `git submodule update --init --recursive`, then re-run."
        );
        let result = json!({
            "ok": false,
            "reason": "hollow-submodules",
            "empty_submodules": hollow,
        });
        println!("{}", serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()));
        return;
    }

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
        let mut projects = read_projects(&model_path);
        // The grain miner is git-blind; stamp the git-boundary FACT onto the
        // census here (a `.git` dir/file at each subproject's dir) so the
        // subproject list carries "this is its own repo" for every downstream
        // consumer (dispatch / prompt render / branch gate re-derive it the
        // same way from the same helper). See `mark_own_git_roots`.
        mark_own_git_roots(root, &mut projects);
        let pass = scan_claude::run_pass(root, &projects, full);

        if full {
            result["regenerated"] = json!(pass.regenerated);
            if !pass.over_cap.is_empty() {
                for entry in &pass.over_cap {
                    eprintln!(
                        "scan: scan-map over hard cap ({} bytes > {} ceiling): {} — not written; runaway machine map",
                        entry.bytes,
                        scan_claude::SCAN_MAP_HARD_CAP_BYTES,
                        entry.path,
                    );
                }
                let over_cap_json: Vec<Value> = pass.over_cap.iter().map(|e| {
                    json!({ "path": e.path, "bytes": e.bytes })
                }).collect();
                result["over_cap"] = json!(over_cap_json);
                result["ok"] = json!(false);
            }
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

/// Submodule paths declared in `.gitmodules` whose working directory holds no
/// files — checked out out of band or never populated.
///
/// The declaration is the evidence: `.gitmodules` says a subtree belongs here,
/// so an empty directory is a hole, not an absence. Nothing else can tell the
/// difference — git metadata is invisible to the miner, and the walk only sees
/// files. Parsing is deliberately dumb (the `path =` entries, nothing else): a
/// `.gitmodules` we cannot read yields nothing to complain about, which is the
/// fail-open default for a repo that has no submodules at all.
fn hollow_submodules(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(".gitmodules")) else {
        return Vec::new(); // no submodules declared — nothing to check.
    };
    let mut out: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("path")?.trim_start().strip_prefix('='))
        .map(|p| p.trim().replace('\\', "/"))
        .filter(|p| !p.is_empty())
        .filter(|p| {
            // Absent or empty — both mean "not checked out here".
            std::fs::read_dir(root.join(p)).map_or(true, |mut it| it.next().is_none())
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, body).expect("write");
    }

    #[test]
    fn a_repo_without_submodules_has_nothing_to_report() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(hollow_submodules(dir.path()).is_empty(), "no .gitmodules → fail-open");
    }

    #[test]
    fn a_declared_but_unpopulated_submodule_is_reported() {
        // The shape that costs a scan run: `.gitmodules` promises the subtree,
        // the directory is there, and it is empty. Only the declaration and the
        // emptiness matter — never what the subtree is written in.
        let dir = tempfile::tempdir().expect("tempdir");
        write(&dir.path().join(".gitmodules"), "[submodule \"sub\"]\n\tpath = sub\n\turl = u\n");
        std::fs::create_dir_all(dir.path().join("sub")).expect("empty dir");
        assert_eq!(hollow_submodules(dir.path()), vec!["sub".to_string()]);

        // A missing directory is the same hole.
        let gone = tempfile::tempdir().expect("tempdir");
        write(&gone.path().join(".gitmodules"), "[submodule \"sub\"]\n\tpath = sub\n");
        assert_eq!(hollow_submodules(gone.path()), vec!["sub".to_string()]);
    }

    #[test]
    fn any_file_at_all_counts_as_populated() {
        // The check is emptiness, not content: the miner decides what is source,
        // and this preflight stays blind to language, extension and layout.
        let dir = tempfile::tempdir().expect("tempdir");
        write(&dir.path().join(".gitmodules"), "[submodule \"sub\"]\n\tpath = sub\n\turl = u\n");
        write(&dir.path().join("sub").join("anything"), "x");
        assert!(hollow_submodules(dir.path()).is_empty(), "checked out → silent");
    }

    #[test]
    fn every_declared_path_is_checked_not_just_the_first() {
        // A superproject with several submodules must not hide the second hole
        // behind the first populated one.
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            &dir.path().join(".gitmodules"),
            "[submodule \"a\"]\n\tpath = vendor/a\n[submodule \"b\"]\n\tpath = vendor/b\n",
        );
        write(&dir.path().join("vendor").join("a").join("anything"), "x");
        std::fs::create_dir_all(dir.path().join("vendor").join("b")).expect("empty dir");
        assert_eq!(hollow_submodules(dir.path()), vec!["vendor/b".to_string()]);
    }
}
