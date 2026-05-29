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

use crate::commands::scan::scan_precompute::{
    backup_generated_mds, build_structure_block, build_tooling_block, ensure_notes_md,
    purge_generated_skills,
};
use crate::commands::skill::skill_resolve;
use mustard_core::domain::entity_registry::EntityRegistry;
use mustard_core::time::now_iso8601;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

/// Agent-prompt template embedded at build time. The orchestrator no longer
/// depends on an on-disk copy: a `.claude/scripts/scan/agent-prompt.template.md`
/// file, when present, overrides this; otherwise this baked-in copy is used.
const EMBEDDED_PROMPT_TEMPLATE: &str =
    include_str!("../../../../cli/templates/scripts/scan/agent-prompt.template.md");

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
const ORCH_CLAUDE_TEMPLATE: &str = r"<!-- mustard:generated -->
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
";

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
#[allow(clippy::unnecessary_wraps)] // signature kept for future fallback that may legitimately return None
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
                .map_or("", str::trim);
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
    let (orch_claude, registry) = ClaudePaths::for_project(root)
        .map(|p| (p.claude_md_path(), p.entity_registry_json_path()))
        .unwrap_or_else(|_| (root.join("CLAUDE.md"), root.join("entity-registry.json")));
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
        let sub_paths = ClaudePaths::for_project(&abs_sub).ok();
        let commands_dir = sub_paths
            .as_ref()
            .map(ClaudePaths::commands_dir)
            .unwrap_or_else(|| abs_sub.clone());
        let skills_dir = sub_paths
            .as_ref()
            .map(ClaudePaths::skills_dir)
            .unwrap_or_else(|| abs_sub.clone());
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
///
/// The agents are written **rich**: their `description` is derived from the
/// subproject's stack/role/architecture and its discovered clusters (so the
/// native `subagent_type` router can pick them by signal, not by a generic
/// "role implementation for X" line), and their body carries the subproject's
/// guards, the deterministically-resolved recommended skills, and the
/// pre-mined entity clusters. All of that comes from the same Rust helpers the
/// dispatch renderer uses (`skill_resolve`, `EntityRegistry`/clusters) — no
/// LLM, no facade.
///
/// Idempotent: a non-force run only writes an agent that is absent; a force run
/// regenerates it. Manual (non-`mustard:generated`) agents are never
/// overwritten, even under `--force`.
fn generate_agent_files(
    root: &Path,
    detect: &Value,
    patterns: Option<&Map<String, Value>>,
    force: bool,
    target: Option<&str>,
    result: &mut ScanResult,
) {
    let agents_dir = ClaudePaths::for_project(root)
        .map(|p| p.agents_dir())
        .unwrap_or_else(|_| root.to_path_buf());
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
        let clusters = patterns
            .map(|p| clusters_for_subproject(p, path))
            .unwrap_or_default();
        let guards = read_guards_section(&root.join(path));
        let impl_path = agents_dir.join(format!("{name}-impl.md"));
        let explorer_path = agents_dir.join(format!("{name}-explorer.md"));
        if agent_writable(&impl_path, force)
            && write_safe(
                result,
                root,
                &impl_path,
                &build_impl_agent(root, &title, name, path, role, stack, &clusters, &guards),
            )
        {
            result.generated.push(format!(".claude/agents/{name}-impl.md"));
        }
        if agent_writable(&explorer_path, force)
            && write_safe(
                result,
                root,
                &explorer_path,
                &build_explorer_agent(root, &title, name, path, role, stack, &clusters),
            )
        {
            result.generated.push(format!(".claude/agents/{name}-explorer.md"));
        }
    }
}

/// Whether an agent file may be (re)written: a missing file is always written;
/// an existing file is overwritten only under `--force` AND only when it is a
/// `mustard:generated` artefact (a hand-authored agent is preserved verbatim).
fn agent_writable(agent_path: &Path, force: bool) -> bool {
    if !agent_path.exists() {
        return true;
    }
    if !force {
        return false;
    }
    // Force only regenerates files we own — preserve manual agents.
    read_safe(agent_path).is_some_and(|c| c.contains("<!-- mustard:generated"))
}

/// Read the `## Guards` section body from a subproject's `CLAUDE.md`, capped to
/// a handful of lines so the agent description/body stays compact. Empty when
/// the file or section is absent (fail-open). Mirrors the heading-scan shape of
/// [`crate::commands::agent::agent_prompt_render`]'s `read_guards_block`.
fn read_guards_section(subproject_dir: &Path) -> Vec<String> {
    let Some(text) = read_safe(&subproject_dir.join("CLAUDE.md")) else {
        return Vec::new();
    };
    let mut in_section = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("## ") {
            if in_section {
                break;
            }
            let after = line.trim_start().trim_start_matches('#').trim();
            if after.eq_ignore_ascii_case("Guards") {
                in_section = true;
            }
            continue;
        }
        if in_section {
            let t = line.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
            if out.len() >= 8 {
                break;
            }
        }
    }
    out
}

/// Comma-separated cluster labels for the subproject (deduped, capped) — the
/// routing signal that goes into the agent `description`. Empty when no cluster
/// is tagged.
fn cluster_label_summary(clusters: &[&Value]) -> String {
    let mut labels: Vec<&str> = clusters
        .iter()
        .filter_map(|c| c.get("label").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .collect();
    labels.dedup();
    labels.truncate(6);
    labels.join(", ")
}

/// Resolve the top recommended skills for an agent role+subproject via
/// [`skill_resolve::resolve`] (same deterministic resolver the dispatch
/// renderer uses). The intent is the role label plus the cluster labels and
/// stack, so a fresh registry surfaces the subproject's own conventions.
/// Returns the skill names (capped). Empty when nothing scores.
fn resolve_agent_skills(root: &Path, role: &str, path: &str, stack: &str, clusters: &[&Value]) -> Vec<String> {
    let intent = format!("{role} {stack} {}", cluster_label_summary(clusters));
    let phase = crate::commands::agent::context_inject::role_to_phase(role);
    skill_resolve::resolve(root, &intent, Some(path), Some(phase), 4)
        .into_iter()
        .map(|r| r.name)
        .collect()
}

/// Render the shared `## Pre-mined clusters` + recommended-skills body chunk so
/// both agent templates surface the same deterministic facts.
fn agent_context_body(skills: &[String], clusters: &[&Value], guards: &[String]) -> String {
    let mut out = String::new();
    if !guards.is_empty() {
        out.push_str("## Guards (from CLAUDE.md)\n");
        for g in guards {
            let _ = writeln!(out, "{g}");
        }
        out.push('\n');
    }
    if !skills.is_empty() {
        let _ = writeln!(out, "## Recommended Skills\n{}\n", skills.join(", "));
    }
    let clusters_block = build_clusters_block(clusters);
    if !clusters_block.is_empty() {
        out.push_str(&clusters_block);
        out.push('\n');
    }
    out
}

/// The `<name>-impl.md` agent — written rich and deterministic.
///
/// The `description` is a routing-grade signal (stack + role + clusters) so the
/// native `subagent_type` selector picks this agent for the right subproject,
/// not a generic "role implementation for X". The body carries the subproject's
/// guards, the resolved recommended skills, and the pre-mined entity clusters.
fn build_impl_agent(
    root: &Path,
    title: &str,
    name: &str,
    path: &str,
    role: &str,
    stack: &str,
    clusters: &[&Value],
    guards: &[String],
) -> String {
    let tools = "Read, Write, Edit, Bash, Grep, Glob";
    let cluster_summary = cluster_label_summary(clusters);
    let skills = resolve_agent_skills(root, role, path, stack, clusters);
    let description = build_impl_description(name, role, stack, &cluster_summary);
    let context_body = agent_context_body(&skills, clusters, guards);
    format!(
        "---\nname: {name}-impl\ndescription: {description}\nmodel: sonnet\ntools: [{tools}]\nmemory: project\n---\n<!-- mustard:generated at:{ts} role:{role} -->\n\n\
         # {title} Implementation Agent\n\n\
         > Implements changes in `{path}/` ({stack}). Guards, skills and pre-mined clusters below are authoritative — trust them before re-walking the tree.\n\n\
         ## Mandatory Reads\n1. `{path}/CLAUDE.md` — guards, stack, key paths\n2. `{path}/.claude/commands/guards.md` — DO/DON'T rules\n3. `{path}/.claude/commands/notes.md` — project-specific notes\n\n\
         {context_body}\
         ## Boundary\nRole: {role}. Stack: {stack}. Scope: `{path}/` only.\n\n\
         ## Validation\nRun the build/type-check command listed in `{path}/CLAUDE.md` → Commands. Max 3 attempts, then STOP + report.\n\n\
         ## Return Format\n### Files Modified/Created\n| File | Action |\n|------|--------|\n\n### Build / Type-check\n(paste exact command + result)\n\n### Guards Verified\nTotal: N/total | Violations: V\n",
        ts = now_iso8601()
    )
}

/// Build the routing-grade `description` for the impl agent. Names the
/// subproject, role, stack and (when present) the discovered clusters so the
/// native `subagent_type` selector routes work here by signal.
fn build_impl_description(name: &str, role: &str, stack: &str, cluster_summary: &str) -> String {
    let mut desc = format!(
        "Implementation agent for the {name} subproject ({stack}, {role}). \
         Use when editing or building code under {name}/."
    );
    if !cluster_summary.is_empty() {
        let _ = write!(desc, " Owns these conventions: {cluster_summary}.");
    }
    desc
}

/// The `<name>-explorer.md` agent — written rich and read-only.
fn build_explorer_agent(
    root: &Path,
    title: &str,
    name: &str,
    path: &str,
    role: &str,
    stack: &str,
    clusters: &[&Value],
) -> String {
    let cluster_summary = cluster_label_summary(clusters);
    let skills = resolve_agent_skills(root, "explore", path, stack, clusters);
    let mut description = format!(
        "Read-only exploration agent for the {name} subproject ({stack}). \
         Use when analyzing, auditing, or investigating code under {name}/ without modifying it."
    );
    if !cluster_summary.is_empty() {
        let _ = write!(description, " Knows these conventions: {cluster_summary}.");
    }
    // Explorer body lists clusters + skills but never the guards-as-rules block
    // (it does not write), so only the cluster/skill context is injected.
    let context_body = agent_context_body(&skills, clusters, &[]);
    format!(
        "---\nname: {name}-explorer\ndescription: {description}\nmodel: sonnet\ntools: [Read, Grep, Glob]\nmemory: project\n---\n<!-- mustard:generated at:{ts} role:{role} -->\n\n\
         # {title} Explorer Agent\n\n\
         > Read-only analysis of `{path}/` ({stack}). Patterns, dependencies, architecture, quality evaluation.\n\n\
         ## Mandatory Reads\n1. `{path}/CLAUDE.md` — project rules, guards, stack\n2. `{path}/.claude/commands/guards.md` — DO/DON'T rules\n\n\
         {context_body}\
         ## Boundary\n- **Read-only** — NEVER write, edit, or execute commands\n- Scope: `{path}/` directory only\n- **Budget: ≤20 tool uses total, ≤3 full file reads** — prefer Grep over Read\n\n\
         ## Return Format\n### Findings\n| Severity | File:Line | Detail |\n|----------|-----------|--------|\n",
        ts = now_iso8601()
    )
}

/// Force-mode: clear cluster caches + refresh the registry via the binary.
fn force_refresh(root: &Path, detect: &Value, result: &mut ScanResult) {
    for sub in detect.get("subprojects").and_then(Value::as_array).cloned().unwrap_or_default() {
        let path = sub.get("path").and_then(Value::as_str).unwrap_or("");
        // `.cluster-cache.json` is owned by the agnostic scan cluster-discovery
        // pass and lives inside each subproject's `.claude/`. Not part of the
        // canonical `ClaudePaths` accessors — it is a per-subproject scan
        // artefact, not a per-root catalog entry — so a stable child of the
        // per-subproject `claude_dir()` is the right call.
        let cache = ClaudePaths::for_project(root.join(path))
            .map(|p| p.claude_dir().join(".cluster-cache.json"))
            .unwrap_or_else(|_| root.join(path).join(".cluster-cache.json"));
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

/// Byte cap for the `{{samplesBlock}}` excerpt — keeps the pre-mined sample
/// list compact (≈the dispatch's structure-block budget) so the agent renders
/// facts rather than re-walking the tree.
const SAMPLES_BLOCK_MAX_BYTES: usize = 1200;

/// Pull the `_patterns.{stack}.discovered[]` clusters tagged for `subproject`
/// out of an already-parsed entity-registry document.
///
/// A cluster matches when it has no `subprojectName` (un-scoped) or its
/// `subprojectName` equals / is a path-suffix of the subproject path — mirrors
/// [`EntityRegistry::cluster_labels`]'s scoping. Returns the raw cluster
/// objects (so the caller can read label/suffix/folder/fileCount/samples).
fn clusters_for_subproject<'a>(patterns: &'a Map<String, Value>, subproject: &str) -> Vec<&'a Value> {
    let mut out: Vec<&Value> = Vec::new();
    for body in patterns.values() {
        let Some(arr) = body.get("discovered").and_then(Value::as_array) else {
            continue;
        };
        for cluster in arr {
            // Keep un-scoped clusters and those whose `subprojectName` equals /
            // is a path-suffix of the subproject path.
            let scoped_out = matches!(
                cluster.get("subprojectName").and_then(Value::as_str),
                Some(name) if !subproject.ends_with(name) && name != subproject
            );
            if !scoped_out {
                out.push(cluster);
            }
        }
    }
    out
}

/// Build `{{clustersBlock}}` — a compact `## Pre-mined clusters` table of the
/// pre-discovered structural clusters (label, suffix, folder pattern, file
/// count). Empty when no cluster is tagged for the subproject.
fn build_clusters_block(clusters: &[&Value]) -> String {
    if clusters.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Pre-mined clusters\n");
    out.push_str("Trust these — already discovered by the scan. Do not re-walk the tree.\n\n");
    out.push_str("| Label | Suffix | Folder | Files |\n|---|---|---|---|\n");
    for c in clusters {
        let label = c.get("label").and_then(Value::as_str).unwrap_or("");
        let suffix = c.get("suffix").and_then(Value::as_str).unwrap_or("");
        let folder = c
            .get("folderPattern")
            .or_else(|| c.get("folder"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let count = c.get("fileCount").and_then(Value::as_u64).unwrap_or(0);
        let _ = writeln!(out, "| {label} | {suffix} | {folder} | {count} |");
    }
    out
}

/// Build `{{samplesBlock}}` — the verified sample file paths the scan recorded
/// per cluster, capped at [`SAMPLES_BLOCK_MAX_BYTES`]. Empty when no cluster
/// carries samples.
fn build_samples_block(clusters: &[&Value]) -> String {
    let mut body = String::new();
    for c in clusters {
        let label = c.get("label").and_then(Value::as_str).unwrap_or("");
        let Some(samples) = c.get("samples").and_then(Value::as_array) else {
            continue;
        };
        let paths: Vec<&str> = samples.iter().filter_map(Value::as_str).collect();
        if paths.is_empty() {
            continue;
        }
        let _ = writeln!(body, "- {label}: {}", paths.join(", "));
        if body.len() >= SAMPLES_BLOCK_MAX_BYTES {
            body.push_str("…[truncated sample list]\n");
            break;
        }
    }
    if body.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Verified samples\n");
    out.push_str(&body);
    out
}

/// Render the agent prompt for one subproject from the loaded template.
///
/// `patterns` is the registry's `_patterns` map (when loaded) so the
/// `{{clustersBlock}}` / `{{samplesBlock}}` placeholders are filled with
/// pre-mined facts instead of the empty strings the regression left behind.
fn render_prompt(
    template: &str,
    root: &Path,
    sub: &Value,
    blocks: &std::collections::BTreeMap<String, (String, String)>,
    patterns: Option<&Map<String, Value>>,
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
    let clusters = patterns
        .map(|p| clusters_for_subproject(p, path))
        .unwrap_or_default();
    let clusters_block = build_clusters_block(&clusters);
    let samples_block = build_samples_block(&clusters);
    template
        .replace("{{name}}", name)
        .replace("{{path}}", path)
        .replace("{{absSubprojectPath}}", &abs)
        .replace("{{role}}", role)
        .replace("{{stack}}", stack)
        .replace("{{forceBlock}}", force_block)
        .replace("{{clustersBlock}}", &clusters_block)
        .replace("{{samplesBlock}}", &samples_block)
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
    // Build a `ClaudePaths` handle once for every per-root cache lookup. If
    // `for_project` rejects the input (I1 violation), bail with a structured
    // error in the result rather than do any further IO.
    let Ok(paths) = ClaudePaths::for_project(root) else {
        result.errors.push(format!(
            "orchestrate: ClaudePaths::for_project rejected {} (likely .claude/.claude/ I1 violation)",
            root.display()
        ));
        return result;
    };
    let claude_dir = paths.claude_dir();
    let detect_cache = paths.detect_cache_path();
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
    if force {
        force_refresh(root, &detect, &mut result);
    }

    // Load the entity-registry once — after a force refresh so the rich agents
    // and the dispatch prompt both see the freshest `_patterns.{stack}` clusters.
    // Fail-open: a missing / empty registry yields no patterns and every
    // cluster-driven block stays empty.
    let registry = EntityRegistry::load(root);
    let patterns = registry.patterns();

    // Generate the rich `.claude/agents/{name}-impl|-explorer.md` files. They
    // reuse the same cluster facts as the dispatch prompt so the native
    // `subagent_type` router gets a routing-grade description + a body carrying
    // guards/skills/clusters. Idempotent; preserves manual agents.
    generate_agent_files(root, &detect, patterns, force, target, &mut result);

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
    // into the binary; an on-disk copy overrides it when present. The registry
    // (loaded above) feeds `{{clustersBlock}}` / `{{samplesBlock}}` with the
    // pre-mined `_patterns.{stack}.discovered[]` facts.
    let template_path = claude_dir.join("scripts").join("scan").join("agent-prompt.template.md");
    let template = read_safe(&template_path).unwrap_or_else(|| EMBEDDED_PROMPT_TEMPLATE.to_string());
    for sub in &dispatch {
        let name = sub.get("name").and_then(Value::as_str).unwrap_or("");
        let path = sub.get("path").and_then(Value::as_str).unwrap_or(name);
        let role = sub.get("role").and_then(Value::as_str).unwrap_or("general");
        let stack = sub.get("stackSummary").and_then(Value::as_str).unwrap_or("");
        result.dispatch.push(json!({
            "name": name, "path": path, "role": role, "stackSummary": stack,
            "agentPrompt": render_prompt(&template, root, sub, &blocks, patterns, force),
        }));
    }

    // Persist dispatch state so finalize can verify each subproject.
    // Migrated to `<root>/.claude/.cache/scan-dispatch.json` per the W2 cache reorg.
    let dispatch_state = paths.scan_dispatch_path();
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
    // W2: anchor every per-root cache write at the resolved workspace root
    // (the directory containing `mustard.json + .claude/`) instead of the raw
    // process cwd. Fail strict — a run subcommand cannot do useful work
    // without an anchor.
    let cwd = match crate::shared::context::workspace_root_strict() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("scan-orchestrate: workspace_root resolution failed: {err}");
            std::process::exit(1);
        }
    };
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
        let rendered = render_prompt(EMBEDDED_PROMPT_TEMPLATE, dir.path(), &sub, &blocks, None, false);
        assert!(!rendered.contains("{{"), "unresolved placeholder remains");
        assert!(rendered.contains("api"));
    }

    #[test]
    fn render_prompt_injects_clusters_and_samples_from_patterns() {
        let dir = tempdir().unwrap();
        let sub = json!({ "name": "api", "path": "api", "role": "backend", "stackSummary": "TS" });
        let blocks = std::collections::BTreeMap::new();
        // A `_patterns.{stack}` map shaped like the registry writes it.
        let patterns: Map<String, Value> = serde_json::from_value(json!({
            "typescript": {
                "discovered": [
                    {
                        "label": "Service",
                        "suffix": "Service",
                        "folderPattern": "**/services/",
                        "fileCount": 7,
                        "samples": ["UserService.ts", "OrderService.ts"],
                        "subprojectName": "api"
                    },
                    {
                        "label": "OtherSub",
                        "suffix": "Repo",
                        "fileCount": 4,
                        "samples": ["X.ts"],
                        "subprojectName": "web"
                    }
                ]
            }
        }))
        .unwrap();
        let rendered = render_prompt(
            EMBEDDED_PROMPT_TEMPLATE,
            dir.path(),
            &sub,
            &blocks,
            Some(&patterns),
            false,
        );
        // The api-scoped cluster surfaces; the web-scoped one does not.
        assert!(rendered.contains("Pre-mined clusters"), "clusters block missing: {rendered}");
        assert!(rendered.contains("Service"));
        assert!(rendered.contains("**/services/"));
        assert!(rendered.contains("Verified samples"), "samples block missing");
        assert!(rendered.contains("UserService.ts"));
        assert!(!rendered.contains("OtherSub"), "web-scoped cluster leaked into api prompt");
        assert!(!rendered.contains("{{"), "unresolved placeholder remains");
    }

    #[test]
    fn cluster_and_sample_blocks_empty_without_patterns() {
        assert!(build_clusters_block(&[]).is_empty());
        assert!(build_samples_block(&[]).is_empty());
    }

    #[test]
    fn bootstrap_writes_orch_claude() {
        let dir = tempdir().unwrap();
        let mut result = ScanResult::default();
        bootstrap(dir.path(), &json!({ "subprojects": [] }), false, &mut result);
        let cp = ClaudePaths::for_project(dir.path()).unwrap();
        assert!(cp.claude_md_path().exists());
        assert!(result.generated.iter().any(|g| g == "CLAUDE.md"));
    }

    // -----------------------------------------------------------------------
    // F3-c — rich `.claude/agents/{name}-impl|-explorer.md` generation
    // -----------------------------------------------------------------------

    /// Plant a workspace anchor + a subproject CLAUDE.md with a `## Guards`
    /// section so the rich agent body has something to inject.
    fn anchor_with_guards(dir: &Path, sub: &str) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        std::fs::write(
            dir.join(sub).join("CLAUDE.md"),
            "# Api\n\n## Guards\n- DO validate inputs\n- DON'T leak secrets\n\n## Stack\nrust\n",
        )
        .unwrap();
    }

    /// A `_patterns.{stack}.discovered[]` map with one api-scoped cluster.
    fn api_patterns() -> Map<String, Value> {
        serde_json::from_value(json!({
            "rust": {
                "discovered": [
                    {
                        "label": "Service",
                        "suffix": "Service",
                        "folderPattern": "**/services/",
                        "fileCount": 5,
                        "subprojectName": "api"
                    }
                ]
            }
        }))
        .unwrap()
    }

    #[test]
    fn build_impl_agent_is_rich_not_generic() {
        let dir = tempdir().unwrap();
        anchor_with_guards(dir.path(), "api");
        let patterns = api_patterns();
        let clusters = clusters_for_subproject(&patterns, "api");
        let guards = read_guards_section(&dir.path().join("api"));
        let agent = build_impl_agent(
            dir.path(),
            "Api",
            "api",
            "api",
            "backend",
            "Rust",
            &clusters,
            &guards,
        );
        // Frontmatter: routing-grade description, NOT the old generic line.
        assert!(agent.contains("name: api-impl"));
        assert!(
            !agent.contains("role implementation for"),
            "description must not be the old generic stub"
        );
        assert!(agent.contains("Implementation agent for the api subproject"));
        // The cluster label surfaces in the description (routing signal).
        assert!(agent.contains("Service"), "cluster label missing from description/body");
        // Body carries guards + a pre-mined cluster block.
        assert!(agent.contains("## Guards (from CLAUDE.md)"));
        assert!(agent.contains("DO validate inputs"));
        assert!(agent.contains("## Pre-mined clusters"));
        // Generated-marker is AFTER the closing `---` (never breaks YAML).
        let marker = agent.find("<!-- mustard:generated").unwrap();
        let fm_close = agent.find("\n---\n").unwrap();
        assert!(marker > fm_close, "generated marker must follow frontmatter");
    }

    #[test]
    fn build_explorer_agent_is_read_only_and_rich() {
        let dir = tempdir().unwrap();
        anchor_with_guards(dir.path(), "api");
        let patterns = api_patterns();
        let clusters = clusters_for_subproject(&patterns, "api");
        let agent = build_explorer_agent(dir.path(), "Api", "api", "api", "general", "Rust", &clusters);
        assert!(agent.contains("name: api-explorer"));
        assert!(agent.contains("tools: [Read, Grep, Glob]"), "explorer must stay read-only");
        assert!(
            !agent.contains("## Guards (from CLAUDE.md)"),
            "explorer does not write — no guards-as-rules block"
        );
        // Still carries the pre-mined cluster facts.
        assert!(agent.contains("## Pre-mined clusters"));
        assert!(agent.contains("Read-only exploration agent for the api subproject"));
    }

    #[test]
    fn generate_agent_files_is_idempotent_and_preserves_manual_agents() {
        let dir = tempdir().unwrap();
        anchor_with_guards(dir.path(), "api");
        let detect = json!({
            "subprojects": [{ "name": "api", "path": "api", "role": "backend", "stackSummary": "Rust" }]
        });
        let patterns = api_patterns();

        // First run (non-force): writes both agents.
        let mut r1 = ScanResult::default();
        generate_agent_files(dir.path(), &detect, Some(&patterns), false, None, &mut r1);
        let agents_dir = ClaudePaths::for_project(dir.path()).unwrap().agents_dir();
        let impl_path = agents_dir.join("api-impl.md");
        assert!(impl_path.exists());
        assert!(r1.generated.iter().any(|g| g == ".claude/agents/api-impl.md"));

        // Second run (non-force): the file exists → no rewrite, nothing generated.
        let mut r2 = ScanResult::default();
        generate_agent_files(dir.path(), &detect, Some(&patterns), false, None, &mut r2);
        assert!(
            r2.generated.is_empty(),
            "non-force run must not rewrite an existing agent (idempotent)"
        );

        // A hand-authored agent (no generated marker) is preserved under --force.
        std::fs::write(&impl_path, "---\nname: api-impl\n---\nMANUAL — keep me\n").unwrap();
        let mut r3 = ScanResult::default();
        generate_agent_files(dir.path(), &detect, Some(&patterns), true, None, &mut r3);
        let after = std::fs::read_to_string(&impl_path).unwrap();
        assert!(after.contains("MANUAL — keep me"), "manual agent must survive --force");
        assert!(
            !r3.generated.iter().any(|g| g == ".claude/agents/api-impl.md"),
            "force must not report regenerating a preserved manual agent"
        );

        // A generated agent IS regenerated under --force (idempotent overwrite).
        let mut r4 = ScanResult::default();
        // Restore a generated-marker file first.
        std::fs::write(&impl_path, "<!-- mustard:generated -->\nold body\n").unwrap();
        generate_agent_files(dir.path(), &detect, Some(&patterns), true, None, &mut r4);
        let regen = std::fs::read_to_string(&impl_path).unwrap();
        assert!(regen.contains("Implementation agent"), "generated agent must be refreshed under --force");
        assert!(r4.generated.iter().any(|g| g == ".claude/agents/api-impl.md"));
    }
}
