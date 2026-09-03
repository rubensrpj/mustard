//! `mustard-rt run status` — project + harness status snapshot.
//!
//! Two modes:
//! - Default (no `--harness`): git branch/modified/last-commit, active vs
//!   orphaned pipelines (via `metrics collect` JSON), last build result, and
//!   the repo-model summary (grain.model.json presence + project count).
//! - `--harness`: reads `<root>/.claude/settings.json`, groups hooks by
//!   lifecycle event, resolves the enforcement mode for each hook via its env
//!   var, and renders a 4-column table.
//!
//! ## Fail-open contract
//!
//! Every IO/parse failure produces a graceful fallback value. The process
//! always exits 0 — a status command must never block work.

use crate::shared::context::MarkerProvenance;
use mustard_core::io::fs;
use mustard_core::{ClaudePaths, GateModes};
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
        "bash_command_gate" => "Blocks dangerous Bash; redirects grep/ls/cat to native tools; rewrites via rtk; commit gate",
        "tool_use_counter" => "Blocks Explore agents at 15 tool uses (warn at 12)",
        "main_context_counter" => "Enforces L0 delegation; warns/denies un-delegated main-context tool calls",
        "context_budget_gate" => "Blocks Task prompts over per-role budget; advisory over 40% model window",
        "close_gate" => "Closes pipeline only if QA + build pass and checklist complete",
        "scan_gate" => "Blocks /feature, /bugfix until grain.model.json exists (run `mustard-rt run scan`)",
        "size_gate" => "Warns specs > 500 lines; validates skill YAML frontmatter",
        "boundary_gate" => "Flags edits outside the active spec's declared boundary (sensitive-file denies live in settings permissions.deny)",
        "post_edit" => "Auto-formats by extension; auto-marks Checklist items; guard-verify; pipeline-phase events",
        "session_knowledge_observer" => "Extracts non-obvious decisions to memory_decisions SQLite; friction telemetry",
        "session_start_inject" => "Bootstraps event bus; runs spec-hygiene; injects top-N knowledge patterns",
        "session_cleanup_observer" => "Removes terminal pipeline-states and stale state files",
        "prompt_submit_inject" => "Archives pending closed-followup specs on a new pipeline command",
        _ => "(no description)",
    }
}

/// Env var name that controls a given hook's mode.
///
/// The `--harness` table renders this column as THE KNOB TO SET, so a name
/// here that nothing reads is worse than an empty cell: setting it looks
/// accepted and changes nothing. Two such names sat here — `MUSTARD_POST_EDIT_MODE`
/// and `MUSTARD_KNOWLEDGE_MODE` — and neither was ever read by a hook.
/// `post_edit` now names `MUSTARD_GUARD_GATE_MODE`, which is what its one
/// refusing half really reads (`hooks/write/post_edit.rs`);
/// `session_knowledge_observer` names nothing, because an Observer returns no
/// verdict and so has no enforcement level to set. `gate_table_parity.rs` holds
/// that line: an arm naming a var no env-reading call consults fails the build.
fn hook_mode_env(name: &str) -> Option<&'static str> {
    match name {
        "bash_command_gate" => Some("MUSTARD_COMMIT_GATE_MODE"),
        "main_context_counter" => Some("MUSTARD_MAIN_BUDGET_MODE"),
        "context_budget_gate" => Some("CONTEXT_BUDGET_MODE"),
        "close_gate" => Some("MUSTARD_CHECKLIST_GATE_MODE"),
        // `scan_gate` is always strict (no mode env var).
        "size_gate" => Some("MUSTARD_SPEC_SIZE_MODE"),
        "boundary_gate" => Some("MUSTARD_BOUNDARY_MODE"),
        "post_edit" => Some("MUSTARD_GUARD_GATE_MODE"),
        // `session_knowledge_observer` is an Observer: no verdict, no mode.
        _ => None,
    }
}

/// The `mustard.json#gates` field a hook's mode ALSO resolves from, between the
/// environment and the built-in default.
///
/// The THIRD layer of the cascade, and the one this table used to skip. Four of
/// the seven gates read it — `boundary_gate` and `main_context_counter` through
/// their own `or_else(|| config_override)`, `size_gate` and `close_gate`
/// through `resolve_mode`'s second argument — so a project carrying
/// `{"gates":{"boundary":"strict"}}` with the env var unset had the gate resolve
/// `strict` while this table printed the built-in `warn`. The table named the
/// right knob and the right default and still reported the wrong level, because
/// it stopped one layer short of where the gate stops.
///
/// Keyed by HOOK, because that is what the row is: the reader is looking at a
/// gate, and the question the cell answers is what happens to THAT gate on
/// THIS project. `gate_table_parity.rs` reads the resolver of each mapped env
/// var and fails the build when a gate consults a layer this map does not.
fn hook_config_key(hook: &str) -> Option<&'static str> {
    match hook {
        "boundary_gate" => Some("boundary"),
        "close_gate" => Some("checklist"),
        "main_context_counter" => Some("main_budget"),
        "size_gate" => Some("spec_size"),
        // Every other gate resolves env → built-in default, with no
        // `mustard.json#gates` field of its own.
        _ => None,
    }
}

/// The value one `mustard.json#gates` field carries, by the field name
/// [`hook_config_key`] returns. `None` when the project states nothing there.
///
/// One arm per key [`hook_config_key`] can return, and no more: an arm for a
/// field no row maps is a name nothing renders, and a MISSING arm is worse —
/// the lookup answers `None`, the cell drops to the built-in default, and the
/// layer is skipped without a word. `gate_table_parity.rs` holds both halves.
fn gate_config_value<'a>(gates: &'a GateModes, key: &str) -> Option<&'a str> {
    match key {
        "boundary" => gates.boundary.as_deref(),
        "checklist" => gates.checklist.as_deref(),
        "main_budget" => gates.main_budget.as_deref(),
        "spec_size" => gates.spec_size.as_deref(),
        _ => None,
    }
}

/// The level a gate falls back to when its variable is set NOWHERE — neither in
/// `settings.json#env` nor in the process environment nor in
/// `mustard.json#gates`.
///
/// This used to be the single word `strict`, for every row, and that was the
/// second half of the same lie [`hook_mode_env`] carried: the column named the
/// right knob and reported the wrong level. Four of the seven gates below
/// default to `warn`, `post_edit` among them
/// (`hooks/write/post_edit.rs::parse_guard_gate_mode`), so an operator reading
/// `strict` there believed a Guard violation would be REFUSED when it is
/// merely reported — the most expensive direction for a mistake about a gate
/// to point.
///
/// `gate_table_parity.rs` holds this map to the resolvers themselves: it finds
/// where the runtime reads each name and reads the fallback out of that code,
/// so a resolver that changes its default and a table that does not fails the
/// build.
/// An unknown name answers `warn`, the UNDERSTATING direction: a reader who
/// expects advice and meets a refusal loses one edit; a reader who expects a
/// refusal and gets advice ships the thing the gate was there to stop.
fn hook_default_mode(env_var: &str) -> &'static str {
    match env_var {
        "CONTEXT_BUDGET_MODE" => "strict",
        "MUSTARD_BOUNDARY_MODE" => "warn",
        "MUSTARD_CHECKLIST_GATE_MODE" => "strict",
        "MUSTARD_COMMIT_GATE_MODE" => "warn",
        "MUSTARD_GUARD_GATE_MODE" => "warn",
        "MUSTARD_MAIN_BUDGET_MODE" => "warn",
        "MUSTARD_SPEC_SIZE_MODE" => "strict",
        _ => "warn",
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

    // Layer three of the cascade, loaded once: the gates a gate consults when
    // neither env layer answers. `ProjectConfig::load` fails open to defaults,
    // so an absent or unparseable `mustard.json` simply states nothing.
    let gates = mustard_core::ProjectConfig::load(root).gates;

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
                // explicit hook filenames like `mustard-rt check bash_command_gate`.
                let hook_name = extract_hook_name(command, event);

                let description = hook_description(&hook_name);
                let mode_env_name = hook_mode_env(&hook_name);
                let mode_str = build_mode_str(&hook_name, mode_env_name, &env_map, &gates);

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
        "PreToolUse" => "bash_command_gate + tool_use_counter + main_context_counter + context_budget_gate + close_gate + boundary_gate",
        "PostToolUse" => "post_edit + session_knowledge_observer",
        "SessionStart" => "spec_hygiene_observer + session_start_inject",
        "SessionEnd" => "session_cleanup_observer + session_knowledge_observer",
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

/// Build a human-readable mode string, e.g. `"warn (env: MUSTARD_COMMIT_GATE_MODE)"`.
///
/// The SAME cascade the gate itself walks, in the same order and with the same
/// arithmetic:
///
/// 1. `settings.json#env` — the harness exports that section into the hook's
///    process, so it is this command's read of layer one rather than a layer of
///    its own.
/// 2. the process environment — layer one as the gate sees it.
/// 3. `mustard.json#gates.<field>`, via [`hook_config_key`]. This is the layer
///    the table used to skip, and skipping it printed `warn` at a project whose
///    `boundary_gate` really resolved `strict`.
/// 4. [`hook_default_mode`] — the level the gate's own resolver falls back to,
///    never a blanket `strict`.
///
/// A value that is not one of the three levels falls through to the default
/// WITHOUT consulting the next layer, which is exactly what `resolve_mode` and
/// the two hand-rolled resolvers do: a set-but-unrecognised env var is a typo,
/// and a typo must not silently promote the layer beneath it.
fn build_mode_str(
    hook_name: &str,
    env_var: Option<&str>,
    env_map: &serde_json::Map<String, Value>,
    gates: &GateModes,
) -> String {
    let Some(var) = env_var else { return "always-on".to_string() };
    let stated = env_map
        .get(var)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var(var).ok().filter(|v| !v.trim().is_empty()))
        .or_else(|| {
            hook_config_key(hook_name)
                .and_then(|key| gate_config_value(gates, key))
                .map(str::to_string)
        });
    let val = match stated.unwrap_or_default().to_ascii_lowercase().as_str() {
        "off" => "off".to_string(),
        "warn" => "warn".to_string(),
        "strict" => "strict".to_string(),
        _ => hook_default_mode(var).to_string(),
    };
    format!("{val} (env: {var})")
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

struct ModelMeta {
    version: String,
    generated_at: String,
    entity_count: usize,
}

fn model_meta(root: &Path) -> ModelMeta {
    // The repo model is grain's `.claude/grain.model.json` (produced by
    // `mustard-rt run scan`). Report its presence + project count.
    let model = root.join(".claude").join("grain.model.json");
    if !model.is_file() {
        return ModelMeta {
            version: "missing".to_string(),
            generated_at: String::new(),
            entity_count: 0,
        };
    }
    ModelMeta {
        version: "grain".to_string(),
        generated_at: String::new(),
        entity_count: mustard_core::read_projects(&model).len(),
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

/// An active pipeline's spec name plus the approval provenance its marker holds
/// (the door + instant), or `None` when the spec carries no readable marker.
struct ActiveSpec {
    name: String,
    approval: Option<MarkerProvenance>,
}

struct PipelineSummary {
    active: Vec<ActiveSpec>,
    orphaned: Vec<String>,
}

/// The approval provenance for one active spec, read through the single reader
/// in `context`. `None` for an un-approved spec or an unreadable marker body —
/// on the status line the provenance only decorates; its absence is silence,
/// never an error.
fn approval_provenance(root: &Path, spec: &str) -> Option<MarkerProvenance> {
    let path = crate::shared::context::approval_marker_path(root.to_str()?, spec)?;
    crate::shared::context::read_marker_provenance(&path)
}

fn pipeline_summary(root: &Path) -> PipelineSummary {
    // Re-use the `metrics collect` JSON if possible, but fall back to scanning
    // the spec directory.
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
            active.push(ActiveSpec {
                approval: approval_provenance(root, &entry.file_name),
                name: entry.file_name.clone(),
            });
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
    registry: &ModelMeta,
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
    for spec in &pipelines.active {
        match &spec.approval {
            // Echo the door + instant the marker recorded; a legacy marker with
            // no instant still names its door.
            Some(p) if !p.at.is_empty() => {
                lines.push(format!("    - {} (approved via {} at {})", spec.name, p.via, p.at));
            }
            Some(p) => {
                lines.push(format!("    - {} (approved via {})", spec.name, p.via));
            }
            None => lines.push(format!("    - {}", spec.name)),
        }
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
    registry: &ModelMeta,
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
            "active": pipelines.active.iter().map(|s| {
                // `name` always present; the provenance keys appear only when the
                // marker read back — omitted, never null, on a missing/legacy read.
                let mut obj = serde_json::Map::new();
                obj.insert("name".to_string(), json!(s.name));
                if let Some(p) = &s.approval {
                    obj.insert("approvedVia".to_string(), json!(p.via));
                    if !p.at.is_empty() {
                        obj.insert("approvedAt".to_string(), json!(p.at));
                    }
                }
                Value::Object(obj)
            }).collect::<Vec<_>>(),
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
        let registry = model_meta(root);
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
        let result = build_mode_str(
            "close_gate",
            Some("MUSTARD_CHECKLIST_GATE_MODE"),
            &env_map,
            &GateModes::default(),
        );
        assert!(result.contains("warn"), "got: {result}");
        assert!(result.contains("MUSTARD_CHECKLIST_GATE_MODE"), "got: {result}");
    }

    /// The cell reports what `mustard.json#gates` states when neither env layer
    /// answers — the third layer the gates read and this table used to skip.
    ///
    /// Both directions, so the assertion can fail: the same gate with the field
    /// stated and with it absent must answer differently, and a gate that has
    /// no `gates.*` field of its own must keep answering its built-in default
    /// no matter what the config says.
    #[test]
    fn build_mode_str_reads_the_project_config_layer_the_gates_read() {
        // The env var names here are deliberately ones NOTHING exports. Cargo
        // runs this crate's tests as threads of one process and
        // `hooks/write/boundary_gate.rs` really does `set_var` on the live
        // name, so asserting through `MUSTARD_BOUNDARY_MODE` would measure that
        // neighbour rather than this cascade. The layer under test is keyed by
        // the HOOK, so the knob's name is free to be an unexported one.
        const UNSET: &str = "MUSTARD_BOUNDARY_MODE_UNSET_TEST_ZZZ";
        let env_map = serde_json::Map::new();
        let mut gates = GateModes::default();

        // Absent: the built-in default (`hook_default_mode`'s catch-all `warn`,
        // which is also what `boundary_mode` falls back to).
        let silent = build_mode_str("boundary_gate", Some(UNSET), &env_map, &gates);
        assert!(silent.contains("warn"), "got: {silent}");

        // Stated: what `boundary_mode(gates.boundary.as_deref())` resolves.
        gates.boundary = Some("strict".to_string());
        let stated = build_mode_str("boundary_gate", Some(UNSET), &env_map, &gates);
        assert!(
            stated.contains("strict"),
            "the project states `gates.boundary = strict` and the table still prints \
             the built-in default: {stated}"
        );

        // An unrecognised value is a typo, not a level: the gate falls back to
        // its default, and so must the cell.
        gates.boundary = Some("banana".to_string());
        let typo = build_mode_str("boundary_gate", Some(UNSET), &env_map, &gates);
        assert!(typo.contains("warn"), "got: {typo}");

        // A gate with no `gates.*` field ignores the whole layer.
        gates.boundary = Some("strict".to_string());
        let unmapped = build_mode_str("post_edit", Some(UNSET), &env_map, &gates);
        assert!(unmapped.contains("warn"), "got: {unmapped}");

        // …and a stated value only reaches the cell through the field the HOOK
        // maps: `close_gate` reads `gates.checklist`, never `gates.boundary`.
        let mut crossed = GateModes { boundary: Some("off".to_string()), ..GateModes::default() };
        assert!(build_mode_str("close_gate", Some(UNSET), &env_map, &crossed).contains("warn"));
        crossed.checklist = Some("off".to_string());
        assert!(build_mode_str("close_gate", Some(UNSET), &env_map, &crossed).contains("off"));
    }

    /// An unset knob reports the level its OWN gate falls back to, not a
    /// blanket `strict`.
    ///
    /// Both halves, so the assertion can fail: the same call shape on a gate
    /// that really defaults to `warn` and on one that really defaults to
    /// `strict` must answer differently. A single-word default passed this
    /// test for as long as it existed while telling the operator that a Guard
    /// violation would be REFUSED — `post_edit` reads
    /// `MUSTARD_GUARD_GATE_MODE`, whose resolver falls back to `warn`.
    #[test]
    fn build_mode_str_reports_each_gates_own_default_when_absent() {
        // The map is asked directly: `build_mode_str` consults the process
        // environment too, and a machine that really exports one of these
        // names would make an assertion about the DEFAULT measure something
        // else.
        assert_eq!(hook_default_mode("MUSTARD_GUARD_GATE_MODE"), "warn");
        assert_eq!(hook_default_mode("MUSTARD_COMMIT_GATE_MODE"), "warn");
        assert_eq!(hook_default_mode("MUSTARD_BOUNDARY_MODE"), "warn");
        assert_eq!(hook_default_mode("MUSTARD_MAIN_BUDGET_MODE"), "warn");
        assert_eq!(hook_default_mode("CONTEXT_BUDGET_MODE"), "strict");
        assert_eq!(hook_default_mode("MUSTARD_CHECKLIST_GATE_MODE"), "strict");
        assert_eq!(hook_default_mode("MUSTARD_SPEC_SIZE_MODE"), "strict");

        // …and the rendered cell carries that default, not a blanket `strict`.
        // A name nothing exports, so the process environment cannot answer for
        // it: the fallback is the only thing left to measure.
        let env_map = serde_json::Map::new();
        let unknown = build_mode_str(
            "budget",
            Some("MUSTARD_BUDGET_MODE_UNSET_TEST_ZZZ"),
            &env_map,
            &GateModes::default(),
        );
        assert!(unknown.contains("warn"), "got: {unknown}");
        assert!(unknown.contains("MUSTARD_BUDGET_MODE_UNSET_TEST_ZZZ"), "got: {unknown}");
    }

    #[test]
    fn render_harness_json_contains_hooks_key() {
        let hooks = vec![json!({"event":"PreToolUse","hook":"bash_command_gate","matcher":".*","enforces":"x","mode":"strict"})];
        let out = render_harness_json(&hooks);
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.get("hooks").is_some());
        assert_eq!(parsed["hooks"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn status_shows_approval_provenance() {
        // An active spec whose approval marker records a door + instant surfaces
        // BOTH: the summary carries the typed provenance, and the human table
        // line names the door and the date.
        let td = tempdir().unwrap();
        let root = td.path();
        let spec = "epic";
        let sdir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&sdir).unwrap();
        // The legacy header `pipeline_summary` keys off (outcome + stage).
        std::fs::write(
            sdir.join("spec.md"),
            "### Outcome: Active\n### Stage: Execute\n# Epic\n",
        )
        .unwrap();
        std::fs::write(
            sdir.join(".approved-by-user"),
            crate::shared::context::marker_body(
                spec,
                "AskUserQuestion",
                "s-1",
                "2026-07-24T10:00:00.000Z",
            ),
        )
        .unwrap();

        let summary = pipeline_summary(root);
        let active = summary
            .active
            .iter()
            .find(|a| a.name == spec)
            .expect("the seeded spec is active");
        let prov = active.approval.as_ref().expect("provenance reads back");
        assert_eq!(prov.via, "AskUserQuestion");
        assert_eq!(prov.at, "2026-07-24T10:00:00.000Z");

        let git = GitStatus {
            branch: "b".to_string(),
            modified: Vec::new(),
            last_commit_hash: String::new(),
            last_commit_subject: String::new(),
        };
        let registry = ModelMeta {
            version: "missing".to_string(),
            generated_at: String::new(),
            entity_count: 0,
        };
        let table = render_default_table(&git, &summary, &None, &registry);
        assert!(table.contains("AskUserQuestion"), "table names the door: {table}");
        assert!(table.contains("2026-07-24T10:00:00.000Z"), "table names the date: {table}");
    }

    #[test]
    fn model_meta_missing_file_returns_missing() {
        let td = tempdir().unwrap();
        let meta = model_meta(td.path());
        assert_eq!(meta.version, "missing");
        assert_eq!(meta.entity_count, 0);
    }

    #[test]
    fn model_meta_present_reports_grain() {
        // A present grain.model.json reports version "grain". The project COUNT
        // comes from the scan tool's `facts` command (`read_projects`); that
        // extraction is covered by scan's and mustard-core's own tests, not here
        // — this asserts only the presence branch (no scan binary required).
        let td = tempdir().unwrap();
        let dir = td.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("grain.model.json"), b"{}").unwrap();
        let meta = model_meta(td.path());
        assert_eq!(meta.version, "grain");
    }
}
