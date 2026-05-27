//! `pre_compact` — the `PreCompact` context-snapshot module.
//!
//! ## Scope (b3 Wave 5, session family)
//!
//! Ports `pre-compact.js` **alone** — a single concern with no sibling hook
//! to merge. It triggers on `PreCompact` and builds a compact snapshot of the
//! working state (git branch, uncommitted-changes summary, recent commits,
//! active pipelines, persistent-memory counts, the compaction reason), saves
//! it to `.claude/.compact-state/` for debugging, and surfaces it as
//! `additionalContext` so the agent keeps the state across the compaction.
//!
//! ## Contract shape
//!
//! `pre-compact.js` produced an `additionalContext` payload via `console.log`.
//! Under the consolidated binary that becomes a [`Verdict::Inject`] so the
//! single `emit_outcome` owns the only stdout write. `PreCompact` is a
//! [`Check`].
//!
//! ## Parity note — the "no active pipeline" early exit
//!
//! `pre-compact.js` exits *silently* (no snapshot) when there is no active or
//! implementing pipeline-state. This port reproduces that: it returns
//! `Verdict::Allow` (the silent path) when no pipeline-state file is `active`
//! / `implementing`.

use mustard_core::atomic_md::MarkdownStore;
use mustard_core::error::Error;
use mustard_core::fs;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::ClaudePaths;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::util::now_iso8601;

/// The `PreCompact` context-snapshot module.
pub struct PreCompact;

/// Resolve the project dir for an invocation: the harness `cwd`, else `.`.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Run a git command in `cwd`, returning trimmed stdout, or `None` on failure.
fn git(cwd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// `true` if there is at least one `active` / `implementing` pipeline-state.
/// Mirrors the JS "no active pipeline → exit silently" gate.
fn has_active_pipeline(claude_dir: &Path) -> bool {
    let pipeline_states_dir = claude_dir
        .parent()
        .filter(|_| claude_dir.file_name().and_then(|s| s.to_str()) == Some(".claude"))
        .and_then(|root| ClaudePaths::for_project(root).ok())
        .map(|p| p.pipeline_states_dir());
    let Some(pipeline_states_dir) = pipeline_states_dir else {
        return true;
    };
    let Ok(entries) = fs::read_dir(&pipeline_states_dir) else {
        // No states dir → the JS validation block is skipped entirely (it
        // only early-exits when the dir *exists* with no active state), so a
        // missing dir does NOT silence the snapshot.
        return true;
    };
    for entry in entries {
        if !std::path::Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&entry.path) {
            if let Ok(obj) = serde_json::from_str::<Value>(&text) {
                let status = obj.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if status == "active" || status == "implementing" {
                    return true;
                }
            }
        }
    }
    // The dir exists with no active state. JS collects `stateFiles` then
    // filters to active ones: an empty dir, a dir of non-JSON files, and a
    // dir of JSON states with none active all yield `activeStates.length===0`
    // → `process.exit(0)` (silent, no snapshot). So once the dir exists and no
    // entry was `active`/`implementing`, this is always silent.
    false
}

/// Count memory entries in a `.claude/{knowledge|memory}` directory by scanning
/// `*.md` files via `MarkdownStore::scan_dir`.
///
/// W3B: replaces the `COUNT(*)` SQLite query against `memory_decisions` /
/// `memory_lessons`. An empty or absent directory returns 0 (fail-open).
fn count_memory_dir(cwd: &str, subdir: &str) -> usize {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return 0;
    };
    let dir = paths.claude_dir().join(subdir);
    MarkdownStore::scan_dir(&dir).len()
}

/// Build the snapshot text. Port of the `pre-compact.js` `parts` assembly.
fn build_snapshot(input: &HookInput, cwd: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Git branch.
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|| "unknown".to_string());
    parts.push(format!("Branch: {branch}"));

    // Uncommitted changes.
    if let Some(status) = git(cwd, &["status", "--porcelain"]) {
        let lines: Vec<&str> = status.lines().collect();
        let staged = lines
            .iter()
            .filter(|l| l.starts_with(['M', 'A', 'D', 'R', 'C']))
            .count();
        let modified = lines
            .iter()
            .filter(|l| l.as_bytes().get(1).is_some_and(|&b| b == b'M' || b == b'D'))
            .count();
        let untracked = lines.iter().filter(|l| l.starts_with("??")).count();
        parts.push(format!(
            "Changes: {staged} staged, {modified} modified, {untracked} untracked"
        ));
        let files: Vec<&str> = lines
            .iter()
            .take(20)
            .map(|l| l.get(3..).unwrap_or(""))
            .collect();
        parts.push(format!("Files: {}", files.join(", ")));
    } else {
        parts.push("Working tree: clean".to_string());
    }

    // Recent commits.
    if let Some(log) = git(cwd, &["log", "--oneline", "-3"]) {
        parts.push(format!("Recent commits:\n{log}"));
    }

    // Active pipelines.
    let paths = ClaudePaths::for_project(Path::new(cwd)).ok();
    let states = paths
        .as_ref()
        .map(ClaudePaths::pipeline_states_dir)
        .unwrap_or_else(|| Path::new(cwd).to_path_buf());
    if let Ok(entries) = fs::read_dir(&states) {
        let names: Vec<String> = entries
            .into_iter()
            .filter_map(|e| e.file_name.strip_suffix(".json").map(str::to_string))
            .collect();
        if !names.is_empty() {
            parts.push(format!("Active pipelines: {}", names.join(", ")));
        }
    }

    // Persistent memory — counts from .claude/knowledge/ and .claude/memory/ dirs.
    let knowledge = count_memory_dir(cwd, "knowledge");
    let memory = count_memory_dir(cwd, "memory");
    if knowledge > 0 || memory > 0 {
        parts.push(format!(
            "Persistent memory: {knowledge} knowledge entries, {memory} memory entries"
        ));
    }

    // Compaction reason.
    let reason = input
        .raw
        .get("compact_reason")
        .or_else(|| input.raw.get("trigger"))
        .and_then(|v| v.as_str())
        .unwrap_or("auto");
    parts.push(format!("Compact trigger: {reason}"));

    parts.join("\n")
}

/// Save the snapshot to `.claude/.compact-state/{timestamp}.txt`. Best-effort.
fn save_snapshot(cwd: &str, summary: &str) {
    // `.compact-state/` is not in the documented `ClaudePaths` catalog — it
    // is a transient pre-compact buffer owned by this hook. Anchor it under
    // the canonical `claude_dir()` so no hand-joined `.claude` literal
    // survives in this module, but keep the directory name verbatim.
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let state_dir = paths.claude_dir().join(".compact-state");
    if fs::create_dir_all(&state_dir).is_err() {
        return;
    }
    // The JS filename is `toISOString()` with `:`/`.` replaced by `-`.
    let timestamp = now_iso8601().replace([':', '.'], "-");
    let _ = fs::write_atomic(state_dir.join(format!("{timestamp}.txt")), summary.as_bytes());
}

impl Check for PreCompact {
    /// On `PreCompact`, build and persist a working-state snapshot and inject
    /// it as advisory context. Returns `Verdict::Allow` (the JS silent path)
    /// when no active pipeline exists; any non-`PreCompact` trigger self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreCompact) {
            return Ok(Verdict::Allow);
        }
        let cwd = project_dir(input, ctx);
        let Ok(paths) = ClaudePaths::for_project(Path::new(&cwd)) else {
            return Ok(Verdict::Allow);
        };
        let claude = paths.claude_dir();
        if !has_active_pipeline(&claude) {
            return Ok(Verdict::Allow);
        }
        let summary = build_snapshot(input, &cwd);
        save_snapshot(&cwd, &summary);
        Ok(Verdict::Inject {
            context: format!("[Pre-compact snapshot]\n{summary}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::PreCompact),
            workspace_root: None,
        }
    }

    fn pre_compact_input() -> HookInput {
        HookInput {
            hook_event_name: Some("PreCompact".to_string()),
            ..HookInput::default()
        }
    }

    #[test]
    fn non_pre_compact_trigger_allows() {
        let other = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            PreCompact.evaluate(&pre_compact_input(), &other).unwrap(),
            Verdict::Allow
        );
    }

    #[test]
    fn snapshot_is_injected_when_no_states_dir() {
        // No .pipeline-states dir → the JS validation block is skipped → the
        // snapshot is still produced.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(ClaudePaths::for_project(dir.path()).unwrap().claude_dir()).unwrap();
        let verdict = PreCompact
            .evaluate(&pre_compact_input(), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        match verdict {
            Verdict::Inject { context } => {
                assert!(context.contains("Pre-compact snapshot"));
                assert!(context.contains("Compact trigger:"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    #[test]
    fn no_active_pipeline_is_silent_allow() {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        // A states dir with only a terminal state → silent (Allow).
        std::fs::write(
            paths.pipeline_state_file("done"),
            json!({ "status": "completed" }).to_string(),
        )
        .unwrap();
        assert_eq!(
            PreCompact
                .evaluate(&pre_compact_input(), &ctx(dir.path().to_str().unwrap()))
                .unwrap(),
            Verdict::Allow
        );
    }

    #[test]
    fn empty_states_dir_is_silent_allow() {
        // Dir exists but holds zero .json files → JS `stateFiles=[]` →
        // `activeStates.length===0` → `process.exit(0)`. Parity: no snapshot.
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        assert_eq!(
            PreCompact
                .evaluate(&pre_compact_input(), &ctx(dir.path().to_str().unwrap()))
                .unwrap(),
            Verdict::Allow
        );
    }

    #[test]
    fn active_pipeline_yields_snapshot() {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(
            paths.pipeline_state_file("live"),
            json!({ "status": "implementing" }).to_string(),
        )
        .unwrap();
        let verdict = PreCompact
            .evaluate(&pre_compact_input(), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        match verdict {
            Verdict::Inject { context } => {
                assert!(context.contains("Active pipelines: live"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
        // The snapshot is also persisted to .compact-state/.
        let compact = paths.claude_dir().join(".compact-state");
        let saved = std::fs::read_dir(&compact).unwrap().count();
        assert_eq!(saved, 1, "snapshot file written");
    }

    #[test]
    fn compact_reason_is_carried_into_snapshot() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(ClaudePaths::for_project(dir.path()).unwrap().claude_dir()).unwrap();
        let input = HookInput {
            hook_event_name: Some("PreCompact".to_string()),
            raw: json!({ "compact_reason": "manual" }),
            ..HookInput::default()
        };
        match PreCompact
            .evaluate(&input, &ctx(dir.path().to_str().unwrap()))
            .unwrap()
        {
            Verdict::Inject { context } => {
                assert!(context.contains("Compact trigger: manual"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }
}
