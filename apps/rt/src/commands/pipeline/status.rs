//! `mustard-rt run status` — project + harness status snapshot.
//!
//! Two modes:
//! - Default (no `--harness`): git branch/modified/last-commit, active vs
//!   orphaned pipelines (via `metrics collect` JSON), last build result, and
//!   entity-registry summary.
//! - `--harness`: reads `<root>/.claude/settings.json`, groups hooks by
//!   lifecycle event, resolves the enforcement mode for each hook via its env
//!   var, and renders a 4-column table.
//!
//! ## Fail-open contract
//!
//! Every IO/parse failure produces a graceful fallback value. The process
//! always exits 0 — a status command must never block work.

use mustard_core::domain::entity_registry::EntityRegistry;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Public options struct
// ---------------------------------------------------------------------------

pub struct StatusOpts {
    pub harness: bool,
    pub format: String,
    pub root: PathBuf,
}

// ---------------------------------------------------------------------------
// Hook description table
// ---------------------------------------------------------------------------

/// Hard-coded human-readable description per hook filename.
fn hook_description(name: &str) -> &'static str {
    match name {
        "bash_guard" => "Blocks dangerous Bash; redirects grep/ls/cat to native tools; rewrites via rtk; commit gate",
        "model_routing_gate" => "Blocks model upgrades vs routing table; downgrades allowed opt-in",
        "tool_use_counter" => "Blocks Explore agents at 15 tool uses (warn at 12)",
        "main_context_counter" => "Enforces L0 delegation; warns/denies un-delegated main-context tool calls",
        "context_budget_gate" => "Blocks Task prompts over per-role budget; advisory over 40% model window",
        "close_gate" => "Closes pipeline only if QA + build pass and checklist complete",
        "entity_registry_gate" => "Blocks /feature, /bugfix if registry missing or version < 3.x",
        "size_gate" => "Warns specs > 500 lines; validates skill YAML frontmatter",
        "path_guard" => "Blocks sensitive-file access; flags edits outside spec boundaries",
        "post_edit" => "Auto-formats by extension; auto-marks Checklist items; guard-verify; pipeline-phase events",
        "session_knowledge_observer" => "Extracts non-obvious decisions to memory_decisions SQLite; friction telemetry",
        "session_start_inject" => "Bootstraps event bus; runs spec-hygiene; injects top-N knowledge patterns",
        "session_cleanup_observer" => "Removes terminal pipeline-states and stale state files",
        "pre_compact_inject" => "Injects working-state snapshot before compaction",
        "prompt_submit_inject" => "Archives pending closed-followup specs on a new pipeline command",
        _ => "(no description)",
    }
}

/// Env var name that controls a given hook's mode.
fn hook_mode_env(name: &str) -> Option<&'static str> {
    match name {
        "bash_guard" => Some("MUSTARD_COMMIT_GATE_MODE"),
        "model_routing_gate" => Some("MUSTARD_MODEL_GATE_MODE"),
        "main_context_counter" => Some("MUSTARD_MAIN_BUDGET_MODE"),
        "context_budget_gate" => Some("CONTEXT_BUDGET_MODE"),
        "close_gate" => Some("MUSTARD_CHECKLIST_GATE_MODE"),
        "entity_registry_gate" => Some("MUSTARD_ENTITY_REGISTRY_GATE_MODE"),
        "size_gate" => Some("MUSTARD_SPEC_SIZE_MODE"),
        "path_guard" => Some("MUSTARD_BOUNDARY_MODE"),
        "post_edit" => Some("MUSTARD_POST_EDIT_MODE"),
        "session_knowledge_observer" => Some("MUSTARD_KNOWLEDGE_MODE"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Harness mode
// ---------------------------------------------------------------------------

/// Read `settings.json`, enumerate hooks, resolve modes from env section.
fn collect_hook_entries(root: &Path) -> Vec<Value> {
    let Ok(paths) = ClaudePaths::for_project(root) else { return Vec::new() };
    let settings_path = paths.settings_json_path();
    let Ok(text) = fs::read_to_string(&settings_path) else { return Vec::new() };
    let settings: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    // Collect env var → value from settings.json["env"]
    let env_map = settings
        .get("env")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let hooks_obj = match settings.get("hooks").and_then(Value::as_object) {
        Some(o) => o.clone(),
        None => return Vec::new(),
    };

    let mut entries: Vec<Value> = Vec::new();

    for (event, event_val) in &hooks_obj {
        let Some(hook_blocks) = event_val.as_array() else { continue };
        for block in hook_blocks {
            let matcher = block
                .get("matcher")
                .and_then(Value::as_str)
                .unwrap_or("*")
                .to_string();

            let Some(inner_hooks) = block.get("hooks").and_then(Value::as_array) else { continue };

            for hook_entry in inner_hooks {
                let command = hook_entry
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                // Extract the last segment of `mustard-rt on <Event>` →
                // that's the event we already have; what we want is a hook
                // name derived from the command string. We use the filename
                // convention: `mustard-rt on PreToolUse` → multiple modules
                // are dispatched. For a single command entry we just use the
                // event + position as identifier, but we also check for
                // explicit hook filenames like `mustard-rt check bash_guard`.
                let hook_name = extract_hook_name(command, event);

                let description = hook_description(&hook_name);
                let mode_env_name = hook_mode_env(&hook_name);
                let mode_str = build_mode_str(&hook_name, mode_env_name, &env_map);

                entries.push(json!({
                    "event": event,
                    "hook": hook_name,
                    "matcher": matcher,
                    "enforces": description,
                    "mode": mode_str,
                }));
            }
        }
    }

    // Sort by event for stable output
    entries.sort_by(|a, b| {
        let ea = a["event"].as_str().unwrap_or("");
        let eb = b["event"].as_str().unwrap_or("");
        ea.cmp(eb)
    });

    entries
}

/// Map an event name to the primary enforcement module name it dispatches.
fn event_to_module(event: &str) -> &'static str {
    match event {
        "PreToolUse" => "bash_guard + model_routing_gate + tool_use_counter + main_context_counter + context_budget_gate + close_gate + path_guard",
        "PostToolUse" => "post_edit + session_knowledge_observer",
        "SessionStart" => "spec_hygiene_observer + session_start_inject",
        "SessionEnd" => "session_cleanup_observer + session_knowledge_observer",
        "PreCompact" => "pre_compact_inject",
        "SubagentStart" => "tool_use_counter + main_context_counter",
        "SubagentStop" => "tool_use_counter + main_context_counter",
        "UserPromptSubmit" => "prompt_submit_inject",
        _ => "(dispatcher)",
    }
}

/// Extract a hook name from the command string and event name.
fn extract_hook_name(command: &str, event: &str) -> String {
    // `mustard-rt check <name>` → use the last token
    if command.contains("check ") {
        if let Some(name) = command.split_whitespace().last() {
            return name.to_string();
        }
    }
    // `mustard-rt on <Event>` → use a descriptive module-list name
    if command.contains(" on ") {
        return event_to_module(event).to_string();
    }
    // Fallback: last whitespace token
    command
        .split_whitespace()
        .last()
        .unwrap_or(event)
        .to_string()
}

/// Build a human-readable mode string, e.g. `"strict (env: MUSTARD_COMMIT_GATE_MODE)"`.
fn build_mode_str(
    _hook_name: &str,
    env_var: Option<&str>,
    env_map: &serde_json::Map<String, Value>,
) -> String {
    if let Some(var) = env_var {
        // Check settings.json env section first, then OS env
        let val = env_map
            .get(var)
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| std::env::var(var).ok())
            .unwrap_or_else(|| "strict".to_string());
        format!("{val} (env: {var})")
    } else {
        "always-on".to_string()
    }
}

// ---------------------------------------------------------------------------
// Default mode: git + pipelines + build + registry
// ---------------------------------------------------------------------------

struct GitStatus {
    branch: String,
    modified: Vec<String>,
    last_commit_hash: String,
    last_commit_subject: String,
}

fn git_status(root: &Path) -> GitStatus {
    let branch = run_git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|| "unknown".to_string());

    let modified: Vec<String> = run_git(root, &["status", "--porcelain"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let log_line =
        run_git(root, &["log", "-1", "--format=%H %s"]).unwrap_or_default();
    let (hash, subject) = log_line
        .split_once(' ')
        .map_or_else(|| (String::new(), String::new()), |(h, s)| (h.to_string(), s.to_string()));

    GitStatus {
        branch,
        modified,
        last_commit_hash: hash.chars().take(12).collect(),
        last_commit_subject: subject,
    }
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "git"])
            .args(args)
            .current_dir(root)
            .output()
            .ok()?
    } else {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .ok()?
    };
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

struct RegistryMeta {
    version: String,
    generated_at: String,
    entity_count: usize,
}

fn registry_meta(root: &Path) -> RegistryMeta {
    let Ok(paths) = ClaudePaths::for_project(root) else {
        return RegistryMeta {
            version: "missing".to_string(),
            generated_at: String::new(),
            entity_count: 0,
        };
    };
    let path = paths.entity_registry_json_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return RegistryMeta {
            version: "missing".to_string(),
            generated_at: String::new(),
            entity_count: 0,
        };
    };
    let Ok(v): Result<Value, _> = serde_json::from_str(&text) else {
        return RegistryMeta {
            version: "parse-error".to_string(),
            generated_at: String::new(),
            entity_count: 0,
        };
    };

    // Queries go through the canonical v4 reader. The prior entity count walked
    // the document root — wrong for v4, where entities live under `e`.
    let registry = EntityRegistry::from_value(v);
    RegistryMeta {
        version: registry.version().unwrap_or("unknown").to_string(),
        generated_at: registry.generated_at().unwrap_or("").to_string(),
        entity_count: registry.entity_count(),
    }
}

struct BuildResult {
    at: String,
    ok: bool,
}

fn last_build(root: &Path) -> Option<BuildResult> {
    let paths = ClaudePaths::for_project(root).ok()?;
    // `.last-build.json` is a legacy direct child of `.claude/` with no typed
    // accessor on `ClaudePaths` — using `claude_dir().join(...)` keeps it
    // routed through the canonical handle without expanding W4 scope.
    let path = paths.claude_dir().join(".last-build.json");
    let text = fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let at = v.get("at").and_then(Value::as_str)?.to_string();
    let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
    Some(BuildResult { at, ok })
}

struct PipelineSummary {
    active: Vec<String>,
    orphaned: Vec<String>,
}

fn pipeline_summary(root: &Path) -> PipelineSummary {
    // Read entity-registry just for the pipeline summary — re-use metrics
    // collect JSON if possible, but fall back to scanning spec directory.
    let Ok(paths) = ClaudePaths::for_project(root) else {
        return PipelineSummary {
            active: Vec::new(),
            orphaned: Vec::new(),
        };
    };
    let spec_root = paths.spec_dir();
    let Ok(entries) = mustard_core::io::fs::read_dir(&spec_root) else {
        return PipelineSummary {
            active: Vec::new(),
            orphaned: Vec::new(),
        };
    };

    let mut active = Vec::new();
    let mut orphaned = Vec::new();

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let spec_md = entry.path.join("spec.md");
        if !spec_md.is_file() {
            continue;
        }
        // Read just the first 512 bytes for the header
        let header_text = {
            use std::io::Read as _;
            let Ok(mut f) = std::fs::File::open(&spec_md) else {
                continue;
            };
            let mut buf = vec![0u8; 512];
            let n = f.read(&mut buf).unwrap_or(0);
            buf.truncate(n);
            String::from_utf8_lossy(&buf).into_owned()
        };

        let outcome_active = header_text.lines().any(|l| {
            l.trim()
                .to_ascii_lowercase()
                .starts_with("### outcome:")
                && l.to_ascii_lowercase().contains("active")
        });
        let stage_ok = header_text.lines().any(|l| {
            let low = l.trim().to_ascii_lowercase();
            low.starts_with("### stage:") && (low.contains("plan") || low.contains("execute"))
        });

        if outcome_active && stage_ok {
            active.push(entry.file_name.clone());
        } else if outcome_active {
            orphaned.push(entry.file_name.clone());
        }
    }

    PipelineSummary { active, orphaned }
}

// ---------------------------------------------------------------------------
// Output renderers
// ---------------------------------------------------------------------------

fn render_default_table(
    git: &GitStatus,
    pipelines: &PipelineSummary,
    build: &Option<BuildResult>,
    registry: &RegistryMeta,
) -> String {
    let mut lines = Vec::new();

    lines.push("## Git\n".to_string());
    lines.push(format!("  Branch   : {}", git.branch));
    lines.push(format!(
        "  Modified : {} file(s)",
        git.modified.len()
    ));
    if !git.last_commit_hash.is_empty() {
        lines.push(format!(
            "  Last     : {} {}",
            git.last_commit_hash, git.last_commit_subject
        ));
    }

    lines.push(String::new());
    lines.push("## Pipelines\n".to_string());
    lines.push(format!("  Active   : {}", pipelines.active.len()));
    for name in &pipelines.active {
        lines.push(format!("    - {name}"));
    }
    if !pipelines.orphaned.is_empty() {
        lines.push(format!("  Orphaned : {}", pipelines.orphaned.len()));
        for name in &pipelines.orphaned {
            lines.push(format!("    - {name}"));
        }
    }

    lines.push(String::new());
    lines.push("## Build\n".to_string());
    match build {
        Some(b) => {
            let status = if b.ok { "pass" } else { "fail" };
            lines.push(format!("  Status   : {status}"));
            lines.push(format!("  At       : {}", b.at));
        }
        None => lines.push("  (no .last-build.json)".to_string()),
    }

    lines.push(String::new());
    lines.push("## Registry\n".to_string());
    lines.push(format!("  Version  : {}", registry.version));
    if !registry.generated_at.is_empty() {
        let short_date: String = registry.generated_at.chars().take(19).collect();
        lines.push(format!("  Generated: {short_date}"));
    }
    lines.push(format!("  Entities : {}", registry.entity_count));

    lines.join("\n")
}

fn render_harness_table(hooks: &[Value]) -> String {
    let mut lines = Vec::new();
    let header = "| Hook             | Matcher               | Enforces                                      | Mode                                       |";
    let sep    = "|------------------|-----------------------|-----------------------------------------------|--------------------------------------------|";

    // Group by event
    let mut events: Vec<String> = Vec::new();
    for h in hooks {
        let ev = h["event"].as_str().unwrap_or("").to_string();
        if !events.contains(&ev) {
            events.push(ev);
        }
    }

    for event in &events {
        lines.push(format!("\n### {event}\n"));
        lines.push(header.to_string());
        lines.push(sep.to_string());
        for h in hooks {
            if h["event"].as_str().unwrap_or("") != event {
                continue;
            }
            let hook = h["hook"].as_str().unwrap_or("");
            let matcher = h["matcher"].as_str().unwrap_or("*");
            let enforces = h["enforces"].as_str().unwrap_or("");
            let mode = h["mode"].as_str().unwrap_or("");

            let hook_col = format!("{hook:<16}");
            let matcher_col = format!("{matcher:<21}");
            // Truncate enforces at 45 chars
            let enforces_short: String = if enforces.chars().count() > 45 {
                let truncated: String = enforces.chars().take(44).collect();
                format!("{truncated}…")
            } else {
                enforces.to_string()
            };
            let enforces_col = format!("{enforces_short:<45}");
            let mode_col = format!("{mode:<42}");

            lines.push(format!("| {hook_col} | {matcher_col} | {enforces_col} | {mode_col} |"));
        }
    }

    lines.join("\n")
}

fn render_default_json(
    git: &GitStatus,
    pipelines: &PipelineSummary,
    build: &Option<BuildResult>,
    registry: &RegistryMeta,
) -> String {
    let doc = json!({
        "git": {
            "branch": git.branch,
            "modified": git.modified,
            "lastCommit": {
                "hash": git.last_commit_hash,
                "subject": git.last_commit_subject,
            }
        },
        "pipelines": {
            "active": pipelines.active,
            "orphaned": pipelines.orphaned,
        },
        "build": match build {
            Some(b) => json!({"at": b.at, "ok": b.ok}),
            None => json!(null),
        },
        "registry": {
            "version": registry.version,
            "generatedAt": registry.generated_at,
            "entities": registry.entity_count,
        }
    });
    serde_json::to_string_pretty(&doc)
        .unwrap_or_else(|_| r#"{"error":"serialize"}"#.to_string())
}

fn render_harness_json(hooks: &[Value]) -> String {
    let doc = json!({ "hooks": hooks });
    serde_json::to_string_pretty(&doc)
        .unwrap_or_else(|_| r#"{"hooks":[]}"#.to_string())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(opts: StatusOpts) {
    let root = &opts.root;

    if opts.harness {
        let hooks = collect_hook_entries(root);
        match opts.format.as_str() {
            "json" => println!("{}", render_harness_json(&hooks)),
            _ => println!("{}", render_harness_table(&hooks)),
        }
    } else {
        let git = git_status(root);
        let pipelines = pipeline_summary(root);
        let build = last_build(root);
        let registry = registry_meta(root);
        match opts.format.as_str() {
            "json" => println!("{}", render_default_json(&git, &pipelines, &build, &registry)),
            _ => println!("{}", render_default_table(&git, &pipelines, &build, &registry)),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_settings(root: &Path, content: &str) {
        let dir = root.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), content).unwrap();
    }

    #[test]
    fn collect_hooks_from_settings_json() {
        let td = tempdir().unwrap();
        write_settings(
            td.path(),
            r#"{
  "env": { "MUSTARD_CHECKLIST_GATE_MODE": "strict" },
  "hooks": {
    "PreToolUse": [
      { "matcher": ".*", "hooks": [{ "type": "command", "command": "rtk mustard-rt on PreToolUse" }] }
    ]
  }
}"#,
        );
        let hooks = collect_hook_entries(td.path());
        assert!(!hooks.is_empty(), "should parse at least one hook entry");
        assert_eq!(hooks[0]["event"], "PreToolUse");
        assert_eq!(hooks[0]["matcher"], ".*");
    }

    #[test]
    fn collect_hooks_missing_settings_returns_empty() {
        let td = tempdir().unwrap();
        let hooks = collect_hook_entries(td.path());
        assert!(hooks.is_empty());
    }

    #[test]
    fn build_mode_str_uses_env_map_value() {
        let mut env_map = serde_json::Map::new();
        env_map.insert(
            "MUSTARD_CHECKLIST_GATE_MODE".to_string(),
            Value::String("warn".to_string()),
        );
        let result = build_mode_str("close_gate", Some("MUSTARD_CHECKLIST_GATE_MODE"), &env_map);
        assert!(result.contains("warn"), "got: {result}");
        assert!(result.contains("MUSTARD_CHECKLIST_GATE_MODE"), "got: {result}");
    }

    #[test]
    fn build_mode_str_defaults_to_strict_when_absent() {
        let env_map = serde_json::Map::new();
        // Use an env var that's unlikely to be set in CI
        let result = build_mode_str(
            "budget",
            Some("MUSTARD_BUDGET_MODE_UNSET_TEST_ZZZ"),
            &env_map,
        );
        assert!(result.contains("strict"), "got: {result}");
    }

    #[test]
    fn render_harness_json_contains_hooks_key() {
        let hooks = vec![json!({"event":"PreToolUse","hook":"bash_guard","matcher":".*","enforces":"x","mode":"strict"})];
        let out = render_harness_json(&hooks);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.get("hooks").is_some());
        assert_eq!(parsed["hooks"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn registry_meta_missing_file_returns_missing() {
        let td = tempdir().unwrap();
        let meta = registry_meta(td.path());
        assert_eq!(meta.version, "missing");
        assert_eq!(meta.entity_count, 0);
    }

    #[test]
    fn registry_meta_parses_version_and_entities() {
        let td = tempdir().unwrap();
        let dir = td.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("entity-registry.json"),
            r#"{"_meta":{"version":"4.0","generatedAt":"2026-05-23T00:00:00Z"},"e":{"User":{},"Post":{}}}"#,
        )
        .unwrap();
        let meta = registry_meta(td.path());
        assert_eq!(meta.version, "4.0");
        assert_eq!(meta.entity_count, 2);
    }
}
