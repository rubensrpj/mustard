//! `mustard-rt run scan-orchestrate` — a port of `scripts/scan/orchestrate.js`.
//!
//! Pre-dispatch orchestration for `/scan`. Replaces the prose protocol the LLM
//! orchestrator used to follow step-by-step: all mechanical work happens here,
//! and the LLM only consumes the JSON output to dispatch Task agents.
//!
//! Contract: stdout is the JSON result; the process always exits `0`
//! (fail-open) — per-step errors are reported inside the JSON `errors[]`.
//!
//! Port note: the JS shelled to `sync-detect.js` / `sync-registry.js`. Those
//! are now `mustard-rt run` subcommands; this port spawns `current_exe()`
//! (the same binary) rather than a separate `bun` process — no Node/Bun in the
//! loop. The agent-prompt template is embedded in the binary via
//! `include_str!`; an on-disk `.claude/scripts/scan/agent-prompt.template.md`
//! (present in projects built by `mustard init`) acts as an optional override.

use crate::run::scan_precompute::{
    backup_generated_mds, build_structure_block, build_tooling_block, ensure_notes_md,
    purge_generated_skills,
};
use crate::util::now_iso8601;
use mustard_core::fs;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Agent-prompt template embedded at build time. The orchestrator no longer
/// depends on an on-disk copy: a `.claude/scripts/scan/agent-prompt.template.md`
/// file, when present, overrides this; otherwise this baked-in copy is used.
const EMBEDDED_PROMPT_TEMPLATE: &str =
    include_str!("../../../cli/templates/scripts/scan/agent-prompt.template.md");

/// The orchestration result accumulator — JSON-shaped exactly as the JS.
#[derive(Default)]
struct ScanResult {
    force: bool,
    target: Option<String>,
    fast_path: bool,
    dispatch: Vec<Value>,
    skipped: Vec<Value>,
    generated: Vec<String>,
    cleanup: Vec<String>,
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl ScanResult {
    fn to_json(&self) -> Value {
        json!({
            "force": self.force,
            "target": self.target,
            "fastPath": self.fast_path,
            "dispatch": self.dispatch,
            "skipped": self.skipped,
            "generated": self.generated,
            "cleanup": self.cleanup,
            "errors": self.errors,
            "warnings": self.warnings,
        })
    }
}

/// The orchestrator `.claude/CLAUDE.md` template (always regenerated).
const ORCH_CLAUDE_TEMPLATE: &str = r#"<!-- mustard:generated -->
# Orchestrator Rules

## Role
You are the orchestrator. Coordinate pipelines and route intent. Delegate non-trivial code work via Task — do trivial work directly to avoid pointless overhead.

## Intent Routing

| Intent | Signals | Action |
|--------|---------|--------|
| Feature | create, add, new entity, new CRUD, implement | Pipeline Feature |
| Enhancement | improve, adjust, change, add field/column, optimize, update | Pipeline Feature |
| Bugfix | error, bug, not working, broken, fix, correct | Pipeline Bugfix |
| Analyze | analyze, audit, evaluate, check, compare, inspect, assess | Direct Grep/Glob OR Task(Explore) if >3 places to search |
| Simple | config tweak, single-line edit, rename one file, version bump | Direct (no Task) |

Any change that touches production code (schema, API, UI) → Pipeline Feature.

## When to delegate via Task (L0)

**MUST delegate (always Task):**
- Pipeline phases EXECUTE (any scope) and PLAN (Full scope)
- Exploration touching >3 files or >2 directories
- New code generation across multiple files
- Refactor crossing ≥3 files
- Any agent-typed work (general-purpose, Plan, Explore)

**MAY work directly in parent (no Task overhead):**
- Read a single file to answer a question
- Edit ≤2 specific files already identified
- Bash status/version/list commands (git status, ls, npm ls)
- Single Grep/Glob to locate a symbol
- Vibe/Spike/Prototype mode

**Why:** Parent context grows with every direct tool call. When it bloats, hooks force retries and pipelines degrade. Tasks isolate work in fresh sub-contexts. Health metric: aim for ≥50% of code actions delegated when pipelines are active.

## Full Reference
Rules, pipeline, naming: `.claude/pipeline-config.md`
"#;

/// The empty entity-registry v4.0 skeleton.
const EMPTY_REGISTRY: &str = r#"{
  "_meta": {
    "version": "4.0"
  },
  "_patterns": {},
  "_enums": {},
  "e": {}
}"#;

/// Read a file as a string, `None` on any error.
fn read_safe(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok()
}

/// Read + parse JSON, `None` on any error.
fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&read_safe(p)?).ok()
}

/// Write a file, creating parent directories. Records a write error.
fn write_safe(result: &mut ScanResult, root: &Path, p: &Path, content: &str) -> bool {
    if fs::write_atomic(p, content.as_bytes()).is_err() {
        result.errors.push(format!("write {}: failed", rel_posix(root, p)));
        return false;
    }
    true
}

/// POSIX relative path of `p` against `root`.
fn rel_posix(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Run `mustard-rt run sync-detect` and parse its JSON. Fallback to a
/// CLAUDE.md scan when the binary path cannot be resolved.
fn run_detect(root: &Path, result: &mut ScanResult) -> Option<Value> {
    if let Ok(exe) = std::env::current_exe() {
        let output = Command::new(&exe)
            .args(["run", "sync-detect"])
            .current_dir(root)
            .output();
        if let Ok(out) = output {
            if let Ok(text) = String::from_utf8(out.stdout) {
                if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                    return Some(parsed);
                }
            }
        }
    }
    result
        .warnings
        .push("sync-detect unavailable — using CLAUDE.md fallback detection".to_string());
    Some(fallback_detect(root))
}

/// Parse the root CLAUDE.md `## Project Structure` table for subprojects.
fn fallback_detect(root: &Path) -> Value {
    let mut subprojects: Vec<Value> = Vec::new();
    if let Some(content) = read_safe(&root.join("CLAUDE.md")) {
        let mut in_table = false;
        for line in content.lines() {
            if line.contains("## Project Structure") {
                in_table = true;
                continue;
            }
            if in_table && line.starts_with("##") && !line.contains("## Project Structure") {
                break;
            }
            if !in_table {
                continue;
            }
            let trimmed = line.trim_start();
            if !trimmed.starts_with('|') {
                continue;
            }
            let cell = trimmed
                .trim_start_matches('|')
                .split('|')
                .next()
                .map(str::trim)
                .unwrap_or("");
            if cell.is_empty()
                || cell == "-"
                || cell.starts_with('(')
                || cell.starts_with('[')
                || cell.starts_with("Subproject")
                || cell.chars().all(|c| c == '-' || c == ' ')
            {
                continue;
            }
            if root.join(cell).join("CLAUDE.md").exists() {
                subprojects.push(json!({
                    "name": cell, "path": cell, "role": "general", "stackSummary": "",
                }));
            }
        }
    }
    json!({ "subprojects": subprojects, "sourceHashes": {} })
}

/// Classify each subproject into dispatch / skip by hash + git-dirty.
fn classify(detect: &Value, old_cache: Option<&Value>, force: bool, target: Option<&str>) -> (Vec<Value>, Vec<Value>) {
    let old_hashes = old_cache
        .and_then(|c| c.get("sourceHashes"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let new_hashes = detect.get("sourceHashes").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut dispatch = Vec::new();
    let mut skipped = Vec::new();
    for sub in detect.get("subprojects").and_then(Value::as_array).cloned().unwrap_or_default() {
        let name = sub.get("name").and_then(Value::as_str).unwrap_or("");
        let path = sub.get("path").and_then(Value::as_str).unwrap_or(name);
        // `target` is a user-facing label — accept either the name or the path.
        if let Some(t) = target {
            if name != t && path != t {
                continue;
            }
        }
        // Hashes are keyed by `path` (collision-safe); `name` only labels.
        let old_hash = old_hashes.get(path).and_then(Value::as_str);
        let new_hash = new_hashes.get(path).and_then(Value::as_str);
        let hash_changed = old_hash.is_none() || old_hash != new_hash;
        let dirty = sub.get("gitDirty").and_then(Value::as_bool).unwrap_or(false);
        if force || hash_changed || dirty {
            dispatch.push(sub);
        } else {
            skipped.push(json!({ "name": name, "reason": "hash unchanged, no git dirty" }));
        }
    }
    (dispatch, skipped)
}

/// Bootstrap foundational files when missing (fast-path skips when present).
fn bootstrap(root: &Path, detect: &Value, force: bool, result: &mut ScanResult) {
    let root_claude = root.join("CLAUDE.md");
    let orch_claude = root.join(".claude").join("CLAUDE.md");
    let registry = root.join(".claude").join("entity-registry.json");
    let have_root = root_claude.exists();
    let have_registry = registry.exists();

    if !force && have_root && have_registry {
        result.fast_path = true;
        return;
    }
    if write_safe(result, root, &orch_claude, ORCH_CLAUDE_TEMPLATE) {
        result.generated.push(".claude/CLAUDE.md".to_string());
    }
    if !have_root {
        let project_name = root.file_name().and_then(|n| n.to_str()).unwrap_or("project");
        let rows: String = detect
            .get("subprojects")
            .and_then(Value::as_array)
            .map(|subs| {
                subs.iter()
                    .map(|s| {
                        let name = s.get("name").and_then(Value::as_str).unwrap_or("");
                        let stack = s.get("stackSummary").and_then(Value::as_str).filter(|x| !x.is_empty()).unwrap_or("-");
                        let path = s.get("path").and_then(Value::as_str).unwrap_or(name);
                        format!("| {name} | {stack} | - | [{name}](./{path}/CLAUDE.md) |")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| "| (none detected) | - | - | - |".to_string());
        let body = format!(
            "# {project_name} - Project Context\n\n\
             > Framework rules: See [.claude/CLAUDE.md](./.claude/CLAUDE.md)\n\n\
             ## Project Structure\n\n\
             | Subproject | Technology | Port | CLAUDE.md |\n\
             |------------|------------|------|-----------|\n\
             {rows}\n\n\
             ## Entity Registry\n\n\
             **CRITICAL:** Before searching for ANY entity, read `.claude/entity-registry.json` first.\n\n\
             ## Ignore Paths\n\n\
             Never search in:\n\
             - `node_modules/`, `.next/`, `bin/`, `obj/`, `dist/`, `migrations/`\n"
        );
        if write_safe(result, root, &root_claude, &body) {
            result.generated.push("CLAUDE.md".to_string());
        }
    }
    if !have_registry && write_safe(result, root, &registry, EMPTY_REGISTRY) {
        result.generated.push(".claude/entity-registry.json".to_string());
    }
    for sub in detect.get("subprojects").and_then(Value::as_array).cloned().unwrap_or_default() {
        let name = sub.get("name").and_then(Value::as_str).unwrap_or("");
        let path = sub.get("path").and_then(Value::as_str).unwrap_or(name);
        let sub_claude = root.join(path).join("CLAUDE.md");
        if !sub_claude.exists() {
            let title = title_case(name);
            let stack = sub.get("stackSummary").and_then(Value::as_str).filter(|s| !s.is_empty());
            let body = format!(
                "# {title}\n\n\
                 > Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)\n\n\
                 ## Stack\n\n{}\n\n## Commands\n\n(populated by /scan)\n\n## Guards\n\n(populated by /scan)\n",
                stack.unwrap_or("(detected on next /scan)")
            );
            if write_safe(result, root, &sub_claude, &body) {
                result.generated.push(format!("{path}/CLAUDE.md"));
            }
        }
    }
}

/// Title-case the first letter of `s`.
fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Per-subproject pre-computation: backup, notes.md, tooling + structure blocks.
fn precompute(
    root: &Path,
    detect: &Value,
    dispatched: &[String],
    force: bool,
    result: &mut ScanResult,
) -> std::collections::BTreeMap<String, (String, String)> {
    let mut blocks = std::collections::BTreeMap::new();
    for sub in detect.get("subprojects").and_then(Value::as_array).cloned().unwrap_or_default() {
        let name = sub.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let path = sub.get("path").and_then(Value::as_str).unwrap_or(&name).to_string();
        // `dispatched` is path-keyed (collision-safe) — see `classify`.
        if !dispatched.contains(&path) {
            continue;
        }
        let abs_sub = root.join(&path);
        let commands_dir = abs_sub.join(".claude").join("commands");
        let skills_dir = abs_sub.join(".claude").join("skills");
        if force {
            for n in purge_generated_skills(&skills_dir) {
                result.cleanup.push(format!("{path}/.claude/skills/{n}"));
            }
            for n in backup_generated_mds(&commands_dir) {
                result.cleanup.push(format!("{path}/.claude/commands/{n} → _backup/{n}"));
            }
        }
        let role = sub.get("role").and_then(Value::as_str).unwrap_or("general");
        if ensure_notes_md(&commands_dir, &name, role) {
            result.generated.push(format!("{path}/.claude/commands/notes.md"));
        }
        let stack = sub.get("stackSummary").and_then(Value::as_str).unwrap_or("");
        blocks.insert(
            path,
            (build_tooling_block(&abs_sub, stack), build_structure_block(&abs_sub)),
        );
    }
    blocks
}

/// Generate per-subproject impl + explorer agent files.
fn generate_agent_files(root: &Path, detect: &Value, force: bool, target: Option<&str>, result: &mut ScanResult) {
    let agents_dir = root.join(".claude").join("agents");
    for sub in detect.get("subprojects").and_then(Value::as_array).cloned().unwrap_or_default() {
        let name = sub.get("name").and_then(Value::as_str).unwrap_or("");
        let path = sub.get("path").and_then(Value::as_str).unwrap_or(name);
        if let Some(t) = target {
            if name != t && path != t {
                continue;
            }
        }
        let role = sub.get("role").and_then(Value::as_str).unwrap_or("general");
        let stack = sub.get("stackSummary").and_then(Value::as_str).unwrap_or("auto-detected");
        let title = title_case(name);
        let impl_path = agents_dir.join(format!("{name}-impl.md"));
        let explorer_path = agents_dir.join(format!("{name}-explorer.md"));
        if (force || !impl_path.exists())
            && write_safe(result, root, &impl_path, &build_impl_agent(&title, name, path, role, stack))
        {
            result.generated.push(format!(".claude/agents/{name}-impl.md"));
        }
        if (force || !explorer_path.exists())
            && write_safe(result, root, &explorer_path, &build_explorer_agent(&title, name, path, role))
        {
            result.generated.push(format!(".claude/agents/{name}-explorer.md"));
        }
    }
}

/// The `<name>-impl.md` agent template.
fn build_impl_agent(title: &str, name: &str, path: &str, role: &str, stack: &str) -> String {
    let tools = "Read, Write, Edit, Bash, Grep, Glob";
    format!(
        "---\nname: {name}-impl\ndescription: {role} implementation for {name}. Reads {name}/CLAUDE.md for guards.\nmodel: sonnet\ntools: [{tools}]\nmemory: project\n---\n<!-- mustard:generated -->\n\n\
         # {title} Implementation Agent\n\n\
         ## Mandatory Reads\n1. `{path}/CLAUDE.md` — guards, stack, key paths\n2. `{path}/.claude/commands/guards.md` — DO/DON'T rules\n3. `{path}/.claude/commands/notes.md` — project-specific notes\n\n\
         ## Boundary\nRole: {role}. Stack: {stack}.\n\n\
         ## Validation\nRun the build/type-check command listed in `{path}/CLAUDE.md` → Commands.\n\n\
         ## Return Format\n### Files Modified/Created\n| File | Action |\n|------|--------|\n\n### Build / Type-check\n{{output}}\n\n### Guards Verified\nTotal: {{n}}/{{total}} | Violations: {{v}}\n"
    )
}

/// The `<name>-explorer.md` agent template.
fn build_explorer_agent(title: &str, name: &str, path: &str, role: &str) -> String {
    format!(
        "---\nname: {name}-explorer\ndescription: Read-only exploration agent for {name} codebase analysis and investigation.\nmodel: haiku\ntools: [Read, Grep, Glob]\nmemory: project\n---\n<!-- mustard:generated at:{} role:{role} -->\n\n\
         # {title} Explorer Agent\n\n\
         > Read-only analysis of {name} codebase. Patterns, dependencies, architecture, quality evaluation.\n\n\
         ## Mandatory Reads\n1. `{path}/CLAUDE.md` — project rules, guards, stack\n2. `{path}/.claude/commands/guards.md` — DO/DON'T rules\n\n\
         ## Boundary\n- **Read-only** — NEVER write, edit, or execute commands\n- Scope: `{path}/` directory only\n- **Budget: ≤20 tool uses total, ≤3 full file reads** — prefer Grep over Read\n\n\
         ## Return Format\n### Findings\n| Severity | File:Line | Detail |\n|----------|-----------|--------|\n",
        now_iso8601()
    )
}

/// Force-mode: clear cluster caches + refresh the registry via the binary.
fn force_refresh(root: &Path, detect: &Value, result: &mut ScanResult) {
    for sub in detect.get("subprojects").and_then(Value::as_array).cloned().unwrap_or_default() {
        let path = sub.get("path").and_then(Value::as_str).unwrap_or("");
        let cache = root.join(path).join(".claude").join(".cluster-cache.json");
        if fs::exists(&cache) && fs::remove_file(&cache).is_ok() {
            result.cleanup.push(rel_posix(root, &cache));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let status = Command::new(&exe)
            .args(["run", "sync-registry", "--force"])
            .current_dir(root)
            .output();
        match status {
            Ok(out) if out.status.success() => {
                result.generated.push(".claude/entity-registry.json".to_string());
            }
            Ok(out) => {
                let code = out.status.code().unwrap_or(-1);
                result
                    .warnings
                    .push(format!("forceRefreshRegistry: sync-registry exit {code} — brief may be stale"));
            }
            Err(e) => result.warnings.push(format!("forceRefreshRegistry: {e}")),
        }
    }
}

/// Render the agent prompt for one subproject from the loaded template.
fn render_prompt(
    template: &str,
    root: &Path,
    sub: &Value,
    blocks: &std::collections::BTreeMap<String, (String, String)>,
    force: bool,
) -> String {
    let name = sub.get("name").and_then(Value::as_str).unwrap_or("");
    let path = sub.get("path").and_then(Value::as_str).unwrap_or(name);
    let role = sub.get("role").and_then(Value::as_str).unwrap_or("general");
    let stack = sub.get("stackSummary").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("(unknown)");
    let abs = rel_posix(Path::new(""), &root.join(path));
    let force_block = if force {
        "FORCE MODE: orchestrate already purged mustard:generated skills/commands and refreshed backups. Skip cleanup — proceed straight to source analysis."
    } else {
        ""
    };
    let (tooling, structure) = blocks.get(path).cloned().unwrap_or_default();
    template
        .replace("{{name}}", name)
        .replace("{{path}}", path)
        .replace("{{absSubprojectPath}}", &abs)
        .replace("{{role}}", role)
        .replace("{{stack}}", stack)
        .replace("{{forceBlock}}", force_block)
        .replace("{{clustersBlock}}", "")
        .replace("{{samplesBlock}}", "")
        .replace("{{budgetBlock}}", "")
        .replace("{{evidenceBlock}}", "")
        .replace("{{toolingBlock}}", &tooling)
        .replace("{{structureBlock}}", &structure)
}

/// Run the orchestration. Separate from [`run`] so tests can drive it.
fn orchestrate(root: &Path, force: bool, target: Option<&str>) -> ScanResult {
    let mut result = ScanResult {
        force,
        target: target.map(str::to_string),
        ..ScanResult::default()
    };
    let claude_dir = root.join(".claude");
    let detect_cache = claude_dir.join(".detect-cache.json");
    let old_cache = read_json(&detect_cache);

    let Some(detect) = run_detect(root, &mut result) else {
        result.errors.push("detect: no subprojects discovered".to_string());
        return result;
    };
    if detect.get("subprojects").and_then(Value::as_array).is_none() {
        result.errors.push("detect: no subprojects discovered (sync-detect output invalid)".to_string());
        return result;
    }

    bootstrap(root, &detect, force, &mut result);
    generate_agent_files(root, &detect, force, target, &mut result);
    if force {
        force_refresh(root, &detect, &mut result);
    }

    let (dispatch, skipped) = classify(&detect, old_cache.as_ref(), force, target);
    result.skipped = skipped;
    let dispatched_paths: Vec<String> = dispatch
        .iter()
        .filter_map(|s| {
            s.get("path")
                .or_else(|| s.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    let blocks = precompute(root, &detect, &dispatched_paths, force, &mut result);

    // Render the agent prompt for each dispatch target. The template is baked
    // into the binary; an on-disk copy overrides it when present.
    let template_path = claude_dir.join("scripts").join("scan").join("agent-prompt.template.md");
    let template = read_safe(&template_path).unwrap_or_else(|| EMBEDDED_PROMPT_TEMPLATE.to_string());
    for sub in &dispatch {
        let name = sub.get("name").and_then(Value::as_str).unwrap_or("");
        let path = sub.get("path").and_then(Value::as_str).unwrap_or(name);
        let role = sub.get("role").and_then(Value::as_str).unwrap_or("general");
        let stack = sub.get("stackSummary").and_then(Value::as_str).unwrap_or("");
        result.dispatch.push(json!({
            "name": name, "path": path, "role": role, "stackSummary": stack,
            "agentPrompt": render_prompt(&template, root, sub, &blocks, force),
        }));
    }

    // Persist dispatch state so finalize can verify each subproject.
    let dispatch_state = claude_dir.join(".scan-dispatch.json");
    let lite: Vec<Value> = dispatch
        .iter()
        .map(|s| {
            let name = s.get("name").and_then(Value::as_str).unwrap_or("");
            let path = s.get("path").and_then(Value::as_str).unwrap_or(name);
            json!({
                "name": name, "path": path,
                "absSubprojectPath": rel_posix(Path::new(""), &root.join(path)),
            })
        })
        .collect();
    let state = json!({ "ts": now_iso8601(), "dispatch": lite });
    let _ = write_safe(
        &mut result,
        root,
        &dispatch_state,
        &(serde_json::to_string_pretty(&state).unwrap_or_default() + "\n"),
    );

    // Refresh the detect cache: the next non-force scan compares its
    // path-keyed `sourceHashes` against this snapshot to skip unchanged
    // subprojects. Nothing else writes this file, so without this the
    // hash-skip would run against frozen data.
    if let Value::Object(mut cache) = detect.clone() {
        cache.insert("lastScan".to_string(), json!(now_iso8601()));
        let _ = write_safe(
            &mut result,
            root,
            &detect_cache,
            &(serde_json::to_string_pretty(&Value::Object(cache)).unwrap_or_default() + "\n"),
        );
    }

    if let Some(warnings) = detect.get("warnings").and_then(Value::as_array) {
        for w in warnings {
            if let Some(s) = w.as_str() {
                result.warnings.push(s.to_string());
            }
        }
    }
    result
}

/// Dispatch `mustard-rt run scan-orchestrate`.
pub fn run(force: bool, target: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = orchestrate(&cwd, force, target);
    println!(
        "{}",
        serde_json::to_string_pretty(&result.to_json()).unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fallback_detect_parses_structure_table() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("CLAUDE.md"),
            "# Root\n\n## Project Structure\n\n| Subproject | Tech |\n|---|---|\n| api | TS |\n\n## Next\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::write(dir.path().join("api").join("CLAUDE.md"), "# api").unwrap();
        let detect = fallback_detect(dir.path());
        let subs = detect["subprojects"].as_array().unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0]["name"], json!("api"));
    }

    #[test]
    fn classify_skips_unchanged_hash() {
        let detect = json!({
            "subprojects": [{ "name": "api", "path": "api" }],
            "sourceHashes": { "api": "abc" },
        });
        let cache = json!({ "sourceHashes": { "api": "abc" } });
        let (dispatch, skipped) = classify(&detect, Some(&cache), false, None);
        assert!(dispatch.is_empty());
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn classify_dispatches_on_force() {
        let detect = json!({
            "subprojects": [{ "name": "api", "path": "api" }],
            "sourceHashes": { "api": "abc" },
        });
        let cache = json!({ "sourceHashes": { "api": "abc" } });
        let (dispatch, _) = classify(&detect, Some(&cache), true, None);
        assert_eq!(dispatch.len(), 1);
    }

    #[test]
    fn embedded_template_renders_without_on_disk_file() {
        // The baked-in template must carry placeholders and resolve them all,
        // so a scan never falls back to an empty dispatch list.
        assert!(EMBEDDED_PROMPT_TEMPLATE.contains("{{name}}"));
        let dir = tempdir().unwrap();
        let sub = json!({ "name": "api", "path": "api", "role": "backend", "stackSummary": "TS" });
        let blocks = std::collections::BTreeMap::new();
        let rendered = render_prompt(EMBEDDED_PROMPT_TEMPLATE, dir.path(), &sub, &blocks, false);
        assert!(!rendered.contains("{{"), "unresolved placeholder remains");
        assert!(rendered.contains("api"));
    }

    #[test]
    fn bootstrap_writes_orch_claude() {
        let dir = tempdir().unwrap();
        let mut result = ScanResult::default();
        bootstrap(dir.path(), &json!({ "subprojects": [] }), false, &mut result);
        assert!(dir.path().join(".claude").join("CLAUDE.md").exists());
        assert!(result.generated.iter().any(|g| g == "CLAUDE.md"));
    }
}
