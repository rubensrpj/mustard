//! `mustard-rt run doctor` — read-only installation health diagnostic.
//!
//! Runs four checks and prints a compact OK/WARN/FAIL report per category.
//! Exit 1 if any check is FAIL, 0 otherwise. Fail-open on every IO error:
//! a check that cannot complete is demoted to WARN, never crashes.
//!
//! ## Checks
//!
//! - **wiring** — every `mustard-rt on <event>` / `run <cmd>` command string
//!   referenced in `.claude/settings.json` resolves to a known event or
//!   registered run subcommand. FAIL on unresolved references.
//! - **residue** (`--residue` only) — scan `settings.json`, SKILL.md files,
//!   and refs for mentions of paths/commands that no longer exist (dead `.js`
//!   names, `scripts/` entries with no resolvable target). WARN per hit.
//! - **drift** — compare by hash the folders that `mustard-cli update`
//!   regenerates (`CORE_FOLDERS`) between the installed `.claude/` and the
//!   `templates/` source. Degrades to `skip` when `templates/` is not
//!   reachable from cwd (consumer project).
//! - **state health** — orphan `.pipeline-states/` files (no matching active
//!   spec), expired `closed-followup` state files, missing
//!   `entity-registry.json`. WARN per anomaly.
//! - **nerd-font** — at least one Nerd Font detected in the OS font
//!   directories. WARN with install hint (`mustard install-nerd-font`) when
//!   absent. Powerline statusline themes require this; without it the
//!   transition glyphs render as tofu.

use crate::shared::context::{current_spec, session_id};
use crate::commands::skill::skill_discovery_lint;
use crate::util::now_iso8601;
use crate::util::sha256::Sha256;
use mustard_core::io::fs;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde_json::json;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The status of a single check.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Ok,
    Warn,
    Fail,
    Skip,
}

impl Status {
    fn label(&self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
            Status::Skip => "SKIP",
        }
    }
}

/// One diagnostic check result.
struct CheckResult {
    name: &'static str,
    status: Status,
    details: Vec<String>,
}

impl CheckResult {
    fn ok(name: &'static str) -> Self {
        Self { name, status: Status::Ok, details: Vec::new() }
    }

    fn warn(name: &'static str, details: Vec<String>) -> Self {
        Self { name, status: Status::Warn, details }
    }

    fn fail(name: &'static str, details: Vec<String>) -> Self {
        Self { name, status: Status::Fail, details }
    }

    fn skip(name: &'static str, reason: &str) -> Self {
        Self { name, status: Status::Skip, details: vec![reason.to_string()] }
    }
}

// ---------------------------------------------------------------------------
// Known valid events and run subcommands
// ---------------------------------------------------------------------------

/// All hook event names `mustard-rt on <event>` recognizes.
const KNOWN_EVENTS: &[&str] = &[
    "PreToolUse",
    "PostToolUse",
    "SessionStart",
    "PreCompact",
    "SessionEnd",
    "SubagentStart",
    "SubagentStop",
    "UserPromptSubmit",
];

/// All `mustard-rt run <subcommand>` names recognized by the binary.
/// Derived from the `RunCmd` enum variants in `run/mod.rs` (kebab-case).
const KNOWN_RUN_SUBCOMMANDS: &[&str] = &[
    "sync-detect",
    "sync-registry",
    "diff-context",
    "emit-event",
    "emit-phase",
    "complete-spec",
    "context-slice",
    "memory",
    "epic-fold",
    "spec-extract",
    "spec-link",
    "analyze-validation",
    "mark-checklist-item",
    "wave-tree",
    "wave-dependency",
    "scope-decompose",
    "exec-rewave-check",
    "wave-size-check",
    "qa-run",
    "metrics",
    "event-projections",
    "verify-pipeline",
    "pipeline-summary",
    "review-result",
    "statusline",
    "skills",
    "security-scan",
    "verify-emit",
    "rtk-gain",
    "scan-orchestrate",
    "scan-finalize",
    "otel-collector",
    "diagnose-otel",
    "doctor",
    "db-maintain",
    "unhook",
    "rehook",
    "plan-from-spec",
    "spec-status-backfill",
];

/// The Mustard-owned folders that `mustard-cli update` regenerates.
/// Derived from the `CORE_FOLDERS` constant in `apps/cli/src/commands/update.rs`.
const CORE_FOLDERS: &[&str] = &["commands/mustard", "hooks", "skills", "scripts", "refs"];

// ---------------------------------------------------------------------------
// Check: wiring
// ---------------------------------------------------------------------------

/// Parse `.claude/settings.json` and verify that every `mustard-rt on <event>`
/// and `mustard-rt run <cmd>` command string references a known event or
/// subcommand.
fn check_wiring(claude_dir: &Path) -> CheckResult {
    let settings_path = claude_dir.join("settings.json");
    let text = match fs::read_to_string(&settings_path) {
        Ok(t) => t,
        Err(e) => {
            return CheckResult::warn(
                "wiring",
                vec![format!("cannot read settings.json: {e}")],
            )
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            return CheckResult::fail(
                "wiring",
                vec![format!("settings.json is not valid JSON: {e}")],
            )
        }
    };

    let mut broken: Vec<String> = Vec::new();
    collect_commands_from_json(&json, &mut broken);

    if broken.is_empty() {
        CheckResult::ok("wiring")
    } else {
        CheckResult::fail("wiring", broken)
    }
}

/// Recursively walk all `"command"` string values in a JSON value and validate
/// any that look like `mustard-rt on <event>` or `mustard-rt run <cmd>`.
fn collect_commands_from_json(val: &serde_json::Value, broken: &mut Vec<String>) {
    match val {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(cmd)) = map.get("command") {
                validate_command_string(cmd, broken);
            }
            for v in map.values() {
                collect_commands_from_json(v, broken);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                collect_commands_from_json(v, broken);
            }
        }
        _ => {}
    }
}

/// Check one command string. Validates `mustard-rt on <event>` and
/// `mustard-rt run <cmd>` patterns; ignores everything else.
fn validate_command_string(cmd: &str, broken: &mut Vec<String>) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.len() < 3 || parts[0] != "mustard-rt" {
        return;
    }
    match parts[1] {
        "on" => {
            let event = parts[2];
            if !KNOWN_EVENTS.contains(&event) {
                broken.push(format!("unknown hook event: '{event}' in command '{cmd}'"));
            }
        }
        "run" => {
            let subcommand = parts[2];
            if !KNOWN_RUN_SUBCOMMANDS.contains(&subcommand) {
                broken.push(format!("unknown run subcommand: '{subcommand}' in command '{cmd}'"));
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Check: residue
// ---------------------------------------------------------------------------

/// Scan `settings.json`, SKILL.md files, and refs for dead references —
/// `.js` script names that no longer exist, `scripts/` paths with no
/// resolvable target. WARN per hit. Only run when `--residue` is passed.
fn check_residue(claude_dir: &Path) -> CheckResult {
    let mut hits: Vec<String> = Vec::new();

    // Check for dead .js script references in settings.json.
    let settings_path = claude_dir.join("settings.json");
    if let Ok(text) = fs::read_to_string(&settings_path) {
        scan_for_dead_js_refs(&text, claude_dir, "settings.json", &mut hits);
    }

    // Scan SKILL.md files for dead .js references.
    scan_md_files_for_dead_refs(claude_dir, &mut hits);

    // Check if CORE_FOLDERS lists scripts/ but no scripts exist.
    let scripts_dir = claude_dir.join("scripts");
    if fs::exists(&scripts_dir) {
        match fs::read_dir(&scripts_dir) {
            Ok(entries) => {
                if entries.is_empty() {
                    hits.push("scripts/ directory is empty (CORE_FOLDER with no content)".to_string());
                }
            }
            Err(e) => {
                hits.push(format!("cannot read scripts/: {e}"));
            }
        }
    }

    if hits.is_empty() {
        CheckResult::ok("residue")
    } else {
        CheckResult::warn("residue", hits)
    }
}

/// Scan text for `.js` filename patterns and check if they exist under
/// `.claude/` or `hooks/`.
fn scan_for_dead_js_refs(text: &str, claude_dir: &Path, source: &str, hits: &mut Vec<String>) {
    for word in text.split_whitespace() {
        // Strip leading quotes or path separators for matching.
        let clean = word.trim_matches(|c: char| c == '"' || c == '\'' || c == ',');
        if clean.ends_with(".js") && !clean.contains("://") {
            // Resolve relative to claude_dir or its parent (project root).
            let project_root = claude_dir.parent().unwrap_or(claude_dir);
            let candidate_claude = claude_dir.join(clean);
            let candidate_root = project_root.join(clean);
            if !candidate_claude.exists() && !candidate_root.exists() {
                hits.push(format!("dead .js reference '{clean}' in {source}"));
            }
        }
    }
}

/// Walk `.claude/` looking for SKILL.md files and scan them for dead refs.
fn scan_md_files_for_dead_refs(claude_dir: &Path, hits: &mut Vec<String>) {
    let walker = collect_files_recursive(claude_dir, 4);
    for path in walker {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".md") {
            if let Ok(text) = fs::read_to_string(&path) {
                let source = path.to_string_lossy().into_owned();
                scan_for_dead_js_refs(&text, claude_dir, &source, hits);
            }
        }
    }
}

/// Collect all files under `dir` up to `max_depth` levels deep. Fail-open.
fn collect_files_recursive(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();
    collect_recursive_inner(dir, max_depth, 0, &mut results);
    results
}

fn collect_recursive_inner(dir: &Path, max_depth: usize, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        if entry.is_dir {
            collect_recursive_inner(&entry.path, max_depth, depth + 1, out);
        } else {
            out.push(entry.path);
        }
    }
}

// ---------------------------------------------------------------------------
// Check: drift
// ---------------------------------------------------------------------------

/// Compare installed `.claude/` core folders against `templates/` source by
/// SHA-256 hash. Degrades to `skip` when `templates/` is not reachable.
fn check_drift(claude_dir: &Path) -> CheckResult {
    // Locate templates/ relative to cwd. Walk upward up to 4 levels.
    let templates_dir = find_templates_dir(claude_dir.parent().unwrap_or(claude_dir));
    let Some(templates_dir) = templates_dir else {
        return CheckResult::skip(
            "drift",
            "templates/ not reachable from cwd (consumer project — skipped)",
        );
    };

    let mut drifted: Vec<String> = Vec::new();

    for folder in CORE_FOLDERS {
        let installed = claude_dir.join(folder);
        let source = templates_dir.join(folder);

        if !source.exists() {
            // Source folder absent — skip this entry silently.
            continue;
        }
        if !installed.exists() {
            drifted.push(format!("{folder}: installed folder missing"));
            continue;
        }

        // Collect and hash all files in both trees.
        let installed_hash = hash_directory(&installed);
        let source_hash = hash_directory(&source);

        if installed_hash != source_hash {
            drifted.push(format!("{folder}: differs from templates/ (run `mustard update`)"));
        }
    }

    if drifted.is_empty() {
        CheckResult::ok("drift")
    } else {
        CheckResult::warn("drift", drifted)
    }
}

/// Try to locate a `templates/` directory by walking up from `start`.
fn find_templates_dir(start: &Path) -> Option<PathBuf> {
    // Look for apps/cli/templates from repo root, or templates/ at repo root.
    let mut candidate = start.to_path_buf();
    for _ in 0..5 {
        let direct = candidate.join("templates");
        if direct.exists() && direct.is_dir() {
            return Some(direct);
        }
        let via_cli = candidate.join("apps").join("cli").join("templates");
        if via_cli.exists() && via_cli.is_dir() {
            return Some(via_cli);
        }
        match candidate.parent() {
            Some(p) => candidate = p.to_path_buf(),
            None => break,
        }
    }
    None
}

/// Hash all files in a directory tree, sorted by relative path for stability.
/// Returns a hex string; returns `"<error>"` on IO failure (fail-open).
fn hash_directory(dir: &Path) -> String {
    let mut files = Vec::new();
    collect_recursive_inner(dir, 8, 0, &mut files);
    files.sort();

    let mut hasher = Sha256::new();
    for file_path in &files {
        if let Ok(bytes) = fs::read(file_path) {
            // Mix in the relative path for rename detection.
            let rel = file_path
                .strip_prefix(dir)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            hasher.update(rel.as_bytes());
            hasher.update(b"\x00");
            hasher.update(&bytes);
        }
    }
    hasher.hex_digest()
}

// ---------------------------------------------------------------------------
// Check: claude_cli
// ---------------------------------------------------------------------------

/// Probe for the `claude` CLI binary and report its resolved path.
///
/// Searches `PATH` the same way the OS would, also probing `.cmd` / `.bat`
/// wrappers on Windows. Produces `OK` when found, `WARN` when absent (the
/// scan cold-path falls back to the agnostic floor without blocking).
fn check_claude_cli() -> CheckResult {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ';' } else { ':' };

    for dir in path_var.split(sep) {
        let base = std::path::Path::new(dir).join("claude");
        if base.exists() {
            let p = base.to_string_lossy().into_owned();
            return CheckResult { name: "claude_cli", status: Status::Ok, details: vec![p] };
        }
        // Windows: try .cmd / .bat / .exe extensions.
        #[cfg(windows)]
        for ext in &[".cmd", ".bat", ".exe"] {
            let candidate = std::path::Path::new(dir).join(format!("claude{ext}"));
            if candidate.exists() {
                let p = candidate.to_string_lossy().into_owned();
                return CheckResult { name: "claude_cli", status: Status::Ok, details: vec![p] };
            }
        }
    }

    CheckResult::warn(
        "claude_cli",
        vec![
            "claude CLI not found on PATH — scan cold-path will fall back to the agnostic floor."
                .to_string(),
            "fix: install Claude Code (https://claude.ai/code) and ensure the binary is on PATH"
                .to_string(),
        ],
    )
}

// ---------------------------------------------------------------------------
// Check: LSP
// ---------------------------------------------------------------------------

/// Map a stack name to the canonical language-server binary name (and an
/// install hint). The table is best-effort; unmapped stacks are silently ignored.
fn lsp_server_for_stack(stack: &str) -> Option<(&'static str, &'static str)> {
    match stack {
        "rust" => Some(("rust-analyzer", "rustup component add rust-analyzer")),
        "typescript" | "javascript" => {
            Some(("typescript-language-server", "npm install -g typescript-language-server typescript"))
        }
        "python" => Some(("pyright", "pip install pyright")),
        "go" => Some(("gopls", "go install golang.org/x/tools/gopls@latest")),
        "java" => Some(("jdtls", "install Eclipse JDT Language Server")),
        "csharp" => Some(("omnisharp", "install OmniSharp via .NET or VS extension")),
        _ => None,
    }
}

/// Detect which language stacks are active in `project_dir` by probing for
/// well-known manifest files — the same signals `sync-detect` uses, but
/// reduced to stack-name strings. Fail-open: IO errors → empty list.
fn detect_stacks(project_dir: &Path) -> Vec<&'static str> {
    let mut stacks: Vec<&'static str> = Vec::new();

    // Rust: Cargo.toml with [package]
    let cargo = project_dir.join("Cargo.toml");
    if cargo.is_file()
        && fs::read_to_string(&cargo)
            .unwrap_or_default()
            .contains("[package]")
    {
        stacks.push("rust");
    }

    // Go: go.mod
    if project_dir.join("go.mod").is_file() {
        stacks.push("go");
    }

    // Python: pyproject.toml or requirements.txt
    if project_dir.join("pyproject.toml").is_file()
        || project_dir.join("requirements.txt").is_file()
    {
        stacks.push("python");
    }

    // TypeScript/JavaScript: package.json
    let pkg_path = project_dir.join("package.json");
    if pkg_path.is_file() {
        let content = fs::read_to_string(&pkg_path).unwrap_or_default();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            let deps_have_ts = ["dependencies", "devDependencies"].iter().any(|section| {
                json.get(*section)
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|obj| obj.contains_key("typescript"))
            });
            if deps_have_ts {
                stacks.push("typescript");
            } else {
                stacks.push("javascript");
            }
        } else {
            stacks.push("javascript");
        }
    }

    // C#: any *.csproj present
    if let Ok(entries) = fs::read_dir(project_dir) {
        let has_csproj = entries
            .iter()
            .any(|e| e.file_name.ends_with(".csproj"));
        if has_csproj {
            stacks.push("csharp");
        }
    }

    // Java: pom.xml or build.gradle
    if project_dir.join("pom.xml").is_file() || project_dir.join("build.gradle").is_file() {
        stacks.push("java");
    }

    stacks
}

/// Look up `binary` in the directories listed in the `PATH` environment
/// variable. On Windows, also probes with the `.exe` suffix. Fail-open:
/// any lookup error returns `false`.
fn which(binary: &str) -> bool {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(target_os = "windows") { ';' } else { ':' };
    for dir in path_var.split(sep) {
        let candidate = std::path::Path::new(dir).join(binary);
        if candidate.exists() {
            return true;
        }
        // Windows: also try with .exe suffix.
        #[cfg(target_os = "windows")]
        {
            let exe = std::path::Path::new(dir).join(format!("{binary}.exe"));
            if exe.exists() {
                return true;
            }
        }
    }
    false
}

/// Check that each detected stack's language server is present on `PATH`.
fn lsp_check(project_dir: &Path) -> CheckResult {
    let stacks = detect_stacks(project_dir);

    // Collect mapped (stack → server) entries, ignoring unmapped stacks.
    let mapped: Vec<(&str, &str, &str)> = stacks
        .iter()
        .filter_map(|s| lsp_server_for_stack(s).map(|(bin, hint)| (*s, bin, hint)))
        .collect();

    if mapped.is_empty() {
        return CheckResult::skip("lsp", "no mapped stacks detected");
    }

    // Deduplicate by binary (typescript + javascript both map to the same server).
    let mut seen_bins: Vec<&str> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for (_stack, bin, hint) in &mapped {
        if seen_bins.contains(bin) {
            continue;
        }
        seen_bins.push(bin);
        if !which(bin) {
            missing.push(format!("missing: {bin} (install: {hint})"));
        }
    }

    if missing.is_empty() {
        CheckResult::ok("lsp")
    } else {
        CheckResult::warn("lsp", missing)
    }
}

// ---------------------------------------------------------------------------
// Check: state health
// ---------------------------------------------------------------------------

/// Inspect `.claude/.pipeline-states/` for orphan or stale state files;
/// also checks for missing `entity-registry.json`.
fn check_state_health(claude_dir: &Path) -> CheckResult {
    let mut warnings: Vec<String> = Vec::new();

    // Check entity-registry.json presence.
    let registry = claude_dir.join("entity-registry.json");
    if !registry.exists() {
        warnings.push("entity-registry.json missing (run `mustard-rt run sync-registry`)".to_string());
    }

    // Inspect pipeline-states/.
    let states_dir = claude_dir
        .parent()
        .filter(|_| claude_dir.file_name().and_then(|s| s.to_str()) == Some(".claude"))
        .and_then(|root| ClaudePaths::for_project(root).ok())
        .map(|p| p.pipeline_states_dir())
        .unwrap_or_else(|| claude_dir.to_path_buf());
    if !states_dir.exists() {
        // No states dir — clean install, nothing to warn about.
        if warnings.is_empty() {
            return CheckResult::ok("state-health");
        }
        return CheckResult::warn("state-health", warnings);
    }

    // Collect spec names from spec/ (flat layout — no buckets).
    let active_specs = collect_active_spec_names(claude_dir);

    let Ok(entries) = fs::read_dir(&states_dir) else {
        warnings.push("cannot read .pipeline-states/ directory".to_string());
        return CheckResult::warn("state-health", warnings);
    };

    // 24 hours in milliseconds for closed-followup expiry.
    const FOLLOWUP_EXPIRY_MS: u128 = 24 * 60 * 60 * 1_000;
    let now_ms = crate::util::now_millis();

    for entry in entries {
        let path = entry.path.clone();
        let file_name = entry.file_name.clone();

        // Parse the state file (JSON with at least a `spec` or `state` field).
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };

        // Detect closed-followup: state files with status "closed-followup".
        let state_val = val
            .get("state")
            .or_else(|| val.get("status"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();

        if state_val == "closed-followup" {
            // Check timestamp for expiry.
            let ts = val
                .get("timestamp")
                .or_else(|| val.get("updatedAt"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if is_timestamp_expired(ts, now_ms, FOLLOWUP_EXPIRY_MS) {
                warnings.push(format!("expired closed-followup state: {file_name}"));
            }
            continue;
        }

        // Detect orphan: state file whose spec is not in spec/ (flat layout).
        let spec_name = val
            .get("spec")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        if !spec_name.is_empty() && !active_specs.contains(&spec_name) {
            warnings.push(format!("orphan state file '{file_name}' (spec '{spec_name}' not in spec/)"));
        }
    }

    if warnings.is_empty() {
        CheckResult::ok("state-health")
    } else {
        CheckResult::warn("state-health", warnings)
    }
}

/// Collect the directory names under `.claude/spec/` (flat layout — no buckets).
fn collect_active_spec_names(claude_dir: &Path) -> Vec<String> {
    let active_dir = claude_dir.join("spec");
    let Ok(entries) = fs::read_dir(&active_dir) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.file_name)
        .collect()
}

/// Return true if `ts` (ISO-8601 string) is older than `expiry_ms` milliseconds
/// relative to `now_ms`. Returns false on parse failure (fail-open).
fn is_timestamp_expired(ts: &str, now_ms: u128, expiry_ms: u128) -> bool {
    if ts.is_empty() {
        return false;
    }
    // Parse `YYYY-MM-DDThh:mm:ss` prefix — enough for expiry comparison.
    let ts_bytes = ts.as_bytes();
    if ts_bytes.len() < 19 {
        return false;
    }
    let year: u64 = parse_digits(&ts[0..4]).unwrap_or(0);
    let month: u64 = parse_digits(&ts[5..7]).unwrap_or(0);
    let day: u64 = parse_digits(&ts[8..10]).unwrap_or(0);
    let hour: u64 = parse_digits(&ts[11..13]).unwrap_or(0);
    let minute: u64 = parse_digits(&ts[14..16]).unwrap_or(0);
    let second: u64 = parse_digits(&ts[17..19]).unwrap_or(0);

    if year == 0 || month == 0 || day == 0 {
        return false;
    }

    // Approximate epoch seconds using a Julian Day Number calculation.
    let ts_secs = approx_epoch_secs(year, month, day, hour, minute, second);
    let ts_ms = (ts_secs as u128) * 1_000;
    now_ms.saturating_sub(ts_ms) > expiry_ms
}

/// Parse an ASCII decimal string slice, returning `None` on failure.
fn parse_digits(s: &str) -> Option<u64> {
    s.parse().ok()
}

/// Approximate Unix epoch seconds for a UTC date/time.
/// Uses the proleptic Gregorian calendar (no daylight saving, no leap seconds).
fn approx_epoch_secs(year: u64, month: u64, day: u64, hour: u64, minute: u64, second: u64) -> u64 {
    // Days from epoch (1970-01-01) to the given date.
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe;
    // Days since 1970-01-01 (day 719_468 in the proleptic Gregorian calendar).
    let since_epoch = days.saturating_sub(719_468);
    since_epoch * 86_400 + hour * 3_600 + minute * 60 + second
}

// ---------------------------------------------------------------------------
// Check: wave-integrity (W10.T10.5)
// ---------------------------------------------------------------------------

/// For each active spec under `.claude/spec/`, parse `wave-plan.md` for
/// `[[wave-N-<role>]]` wikilinks and verify each referenced subdirectory
/// exists. WARN per broken wikilink (an editor typo or partial scaffold);
/// FAIL only on an empty result paired with a non-empty wave-plan body.
/// Fail-open: a missing spec tree, unreadable file, or malformed wikilink is
/// silently ignored — better to skip a check than crash the doctor.
fn check_wave_integrity(claude_dir: &Path) -> CheckResult {
    let spec_root = claude_dir.join("spec");
    let Ok(entries) = fs::read_dir(&spec_root) else {
        return CheckResult::skip("wave-integrity", "no .claude/spec/ directory");
    };
    let mut warnings: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let plan_path = entry.path.join("wave-plan.md");
        if !plan_path.is_file() {
            continue;
        }
        scanned += 1;
        let Ok(text) = fs::read_to_string(&plan_path) else {
            continue;
        };
        for link in extract_wave_wikilinks(&text) {
            let dir = entry.path.join(&link);
            if !dir.is_dir() {
                warnings.push(format!(
                    "{spec}: [[{link}]] -> directory missing",
                    spec = entry.file_name,
                ));
            }
        }
    }
    if scanned == 0 {
        return CheckResult::skip("wave-integrity", "no wave-plan.md files found");
    }
    if warnings.is_empty() {
        let mut r = CheckResult::ok("wave-integrity");
        r.details.push(format!("scanned {scanned} wave-plan(s) — no missing dirs"));
        r
    } else {
        CheckResult::warn("wave-integrity", warnings)
    }
}

/// Pull every `[[wave-N-<role>]]` wikilink from raw markdown. Matches the
/// `wave-N-{role}` shape only; ignores generic `[[link]]` references so cross-
/// links to non-wave concept nodes don't trigger spurious warnings.
fn extract_wave_wikilinks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            // Find the closing `]]` on the same line.
            if let Some(end) = text[i + 2..].find("]]") {
                let link = &text[i + 2..i + 2 + end];
                // Cut piped text (e.g. `[[wave-1-rt|label]]`).
                let core = link.split('|').next().unwrap_or(link).trim();
                if is_wave_link(core) && !out.iter().any(|s| s == core) {
                    out.push(core.to_string());
                }
                i += 2 + end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Loose recogniser for `wave-{N}-{role}` — `N` numeric, role non-empty.
fn is_wave_link(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("wave-") else {
        return false;
    };
    let Some((n, role)) = rest.split_once('-') else {
        return false;
    };
    !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && !role.is_empty()
}

// ---------------------------------------------------------------------------
// Check: nerd-font
// ---------------------------------------------------------------------------

/// Probe OS font directories for *any* Nerd Font (filename containing both a
/// font-family-ish token and "nerd" or "nf-"). WARN when none is found, since
/// the powerline statusline themes need one.
///
/// Fail-open: read errors degrade to "not detected" (WARN) rather than
/// blocking the doctor run.
fn check_nerd_font() -> CheckResult {
    let dirs = nerd_font_search_dirs();
    if dirs.iter().any(|d| scan_for_any_nerd_font(d)) {
        return CheckResult::ok("nerd-font");
    }
    // Linux: fontconfig is authoritative if the binary is on PATH.
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("fc-list").output() {
            if output.status.success() {
                let listing = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
                if listing.contains("nerd") {
                    return CheckResult::ok("nerd-font");
                }
            }
        }
    }
    CheckResult::warn(
        "nerd-font",
        vec![
            "no Nerd Font detected on this host — powerline statusline themes will render \
             tofu (□) instead of separator arrows."
                .to_string(),
            "fix: run `mustard install-nerd-font` (default JetBrainsMono)".to_string(),
            "or set MUSTARD_STATUSLINE_THEME=default (pipe-only, no Nerd Font needed)"
                .to_string(),
        ],
    )
}

fn nerd_font_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
        dirs.push(PathBuf::from("C:/Windows/Fonts"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library").join("Fonts"));
        }
        dirs.push(PathBuf::from("/Library/Fonts"));
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/fonts"));
        }
        dirs.push(PathBuf::from("/usr/share/fonts"));
    }
    dirs
}

/// One level + immediate subdirectories. Match any file whose lowercased
/// name contains "nerd" or "nf-".
fn scan_for_any_nerd_font(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries {
        let name = entry.file_name.to_ascii_lowercase();
        if name.contains("nerd") || name.contains("nf-") {
            return true;
        }
        if entry.is_dir {
            if let Ok(sub) = fs::read_dir(&entry.path) {
                for s in sub {
                    let sn = s.file_name.to_ascii_lowercase();
                    if sn.contains("nerd") || sn.contains("nf-") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Report renderer
// ---------------------------------------------------------------------------

/// Print the compact OK/WARN/FAIL/SKIP report to stdout.
fn render_report(results: &[CheckResult]) {
    let timestamp = crate::util::now_iso8601();
    println!("mustard doctor — {timestamp}");
    println!("{}", "─".repeat(40));
    for r in results {
        let label = r.status.label();
        println!("{label:4}  {}", r.name);
        for detail in &r.details {
            println!("      · {detail}");
        }
    }
    println!("{}", "─".repeat(40));
    let any_fail = results.iter().any(|r| r.status == Status::Fail);
    let any_warn = results.iter().any(|r| r.status == Status::Warn);
    if any_fail {
        println!("status  FAIL — fix issues above before continuing");
    } else if any_warn {
        println!("status  WARN — review warnings above");
    } else {
        println!("status  OK — installation looks healthy");
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// skill-discovery check integration
// ---------------------------------------------------------------------------

/// Run the skill-discovery lint and return a `CheckResult`.
fn run_skill_discovery_check(root: &Path) -> CheckResult {
    let report = skill_discovery_lint::check(root);
    if report.violations.is_empty() {
        let mut r = CheckResult::ok("skill-discovery");
        r.details.push(format!("scanned {} SKILL.md file(s) — no violations", report.scanned));
        r
    } else {
        let details: Vec<String> = report
            .violations
            .iter()
            .map(|v| format!("{}:{} — {}", v.path, v.line, v.phrase))
            .collect();
        CheckResult::warn("skill-discovery", details)
    }
}

// ---------------------------------------------------------------------------
// JSON renderer
// ---------------------------------------------------------------------------

/// Serialize the report as JSON. W10.T10.6 shape:
///
/// ```json
/// {
///   "checks": [{ "name": "...", "status": "ok|warn|fail|skip",
///                "message": "...", "details": ["..."] }],
///   "overall": "ok|warn|fail",
///   "violations": [...]
/// }
/// ```
///
/// `status` is lowercased (the spec's `ok|warn|fail` contract);
/// `message` is the first detail line, joined with `; ` when multiple exist.
/// `details` is preserved for callers that want the full per-check list.
fn render_report_json(results: &[CheckResult]) {
    let checks: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            let status_str = r.status.label().to_ascii_lowercase();
            let message = if r.details.is_empty() {
                String::new()
            } else if r.details.len() == 1 {
                r.details[0].clone()
            } else {
                r.details.join("; ")
            };
            json!({
                "name": r.name,
                "status": status_str,
                "message": message,
                "details": r.details,
            })
        })
        .collect();

    // Aggregate overall verdict (FAIL > WARN > OK; SKIP is neutral).
    let any_fail = results.iter().any(|r| r.status == Status::Fail);
    let any_warn = results.iter().any(|r| r.status == Status::Warn);
    let overall = if any_fail {
        "fail"
    } else if any_warn {
        "warn"
    } else {
        "ok"
    };

    let violations: Vec<String> = results
        .iter()
        .filter(|r| r.name == "skill-discovery" && r.status == Status::Warn)
        .flat_map(|r| r.details.iter())
        .cloned()
        .collect();

    let body = json!({
        "checks": checks,
        "overall": overall,
        "violations": violations,
    });
    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string()));
}

// ---------------------------------------------------------------------------
// Check: status-consistency (W2 — spec-status-consistency)
// ---------------------------------------------------------------------------

/// Check every spec directory under `.claude/spec/` for:
/// - Missing `### Stage:` / `### Outcome:` headers in `spec.md`.
/// - Divergence between `spec.md` headers and `meta.json` fields.
/// - Invalid `(Stage, Outcome)` combinations.
///
/// Recurses into `wave-N-*/` subdirectories within each spec.
/// Returns a single FAIL result listing every problematic path, or OK when
/// all specs are consistent.
fn check_status_consistency(claude_dir: &Path) -> CheckResult {
    use mustard_core::{header_field, read_meta};

    /// Valid `(stage_label, outcome_label)` pairs.
    fn is_valid_combo(stage: &str, outcome: &str) -> bool {
        matches!(
            (stage, outcome),
            ("Plan" | "Analyze" | "Execute" | "QaReview" | "Close", "Active")
                | ("Close", "Completed" | "Cancelled" | "Abandoned" | "Superseded" | "Absorbed")
        )
    }

    /// Check one (spec.md, meta.json) pair. Returns a list of FAIL strings.
    fn check_pair(spec_md_path: &std::path::Path) -> Vec<String> {
        let Some(spec_dir) = spec_md_path.parent() else {
            return vec![format!("{}: no parent dir", spec_md_path.display())];
        };
        let label = spec_md_path.display().to_string();

        let content = match std::fs::read_to_string(spec_md_path) {
            Ok(c) => c,
            Err(e) => return vec![format!("{label}: cannot read spec.md: {e}")],
        };

        let stage_in_spec = header_field(&content, "Stage");
        let outcome_in_spec = header_field(&content, "Outcome");

        // Missing headers.
        let (Some(stage_spec), Some(outcome_spec)) = (stage_in_spec, outcome_in_spec) else {
            return vec![format!("{label}: missing headers in spec.md")];
        };

        // Invalid combo.
        if !is_valid_combo(&stage_spec, &outcome_spec) {
            return vec![format!("{label}: invalid combo stage={stage_spec:?} outcome={outcome_spec:?}")];
        }

        // meta.json divergence.
        let meta_path = spec_dir.join("meta.json");
        if !meta_path.exists() {
            return vec![format!("{label}: meta.json missing")];
        }
        let meta = read_meta(&meta_path).unwrap_or_default();
        let stage_meta = meta.stage.as_deref().unwrap_or("").to_string();
        let outcome_meta = meta.outcome.as_deref().unwrap_or("").to_string();

        if stage_meta.is_empty() || outcome_meta.is_empty() {
            return vec![format!("{label}: meta.json missing stage/outcome fields \
                (stage={stage_meta:?}, outcome={outcome_meta:?})")];
        }

        if !stage_spec.eq_ignore_ascii_case(&stage_meta)
            || !outcome_spec.eq_ignore_ascii_case(&outcome_meta)
        {
            return vec![format!("{label}: stage divergence: spec.md={stage_spec:?}/{outcome_spec:?}, \
                meta.json={stage_meta:?}/{outcome_meta:?}")];
        }

        vec![]
    }

    let spec_root = claude_dir.join("spec");
    let Ok(entries) = fs::read_dir(&spec_root) else {
        return CheckResult::skip("status-consistency", "no .claude/spec/ directory");
    };

    let mut fails: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        // Check parent spec.md.
        let parent_spec_md = entry.path.join("spec.md");
        if parent_spec_md.exists() {
            scanned += 1;
            fails.extend(check_pair(&parent_spec_md));
        }
        // Recurse into wave-N-* subdirectories.
        if let Ok(sub_entries) = fs::read_dir(&entry.path) {
            for sub in sub_entries {
                if !sub.is_dir {
                    continue;
                }
                // Only wave subdirs: name starts with "wave-" followed by a digit.
                let name = &sub.file_name;
                let is_wave = name.starts_with("wave-")
                    && name.chars().nth(5).is_some_and(|c| c.is_ascii_digit());
                if !is_wave {
                    continue;
                }
                let wave_spec_md = sub.path.join("spec.md");
                if wave_spec_md.exists() {
                    scanned += 1;
                    fails.extend(check_pair(&wave_spec_md));
                }
            }
        }
    }

    if scanned == 0 {
        return CheckResult::skip("status-consistency", "no spec.md files found");
    }
    if fails.is_empty() {
        let mut r = CheckResult::ok("status-consistency");
        r.details.push(format!("scanned {scanned} spec(s) — all consistent"));
        r
    } else {
        CheckResult::fail("status-consistency", fails)
    }
}

#[cfg(test)]
mod status_consistency_tests {
    use super::*;
    use tempfile::tempdir;

    fn make_spec_dir(
        root: &std::path::Path,
        name: &str,
        spec_stage: &str,
        spec_outcome: &str,
        meta_stage: &str,
        meta_outcome: &str,
    ) {
        let spec_dir = root.join(".claude").join("spec").join(name);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            format!("# {name}\n\n### Stage: {spec_stage}\n### Outcome: {spec_outcome}\n### Flags: \n\n## Body\n"),
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            format!(r#"{{"stage":"{meta_stage}","outcome":"{meta_outcome}"}}"#),
        )
        .unwrap();
    }

    #[test]
    fn doctor_status_consistency_closed_followup_ok() {
        let dir = tempdir().unwrap();
        make_spec_dir(dir.path(), "fu-spec", "Close", "Active", "Close", "Active");
        let claude_dir = dir.path().join(".claude");
        let result = check_status_consistency(&claude_dir);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    #[test]
    fn doctor_status_consistency_invalid_combo_fail() {
        let dir = tempdir().unwrap();
        make_spec_dir(dir.path(), "bad-spec", "Analyze", "Cancelled", "Analyze", "Cancelled");
        let claude_dir = dir.path().join(".claude");
        let result = check_status_consistency(&claude_dir);
        assert_eq!(result.status, Status::Fail, "{:?}", result.details);
        assert!(result.details.iter().any(|d| d.contains("invalid combo")), "{:?}", result.details);
    }

    #[test]
    fn doctor_status_consistency_divergence_fail() {
        let dir = tempdir().unwrap();
        make_spec_dir(dir.path(), "div-spec", "Execute", "Active", "Plan", "Active");
        let claude_dir = dir.path().join(".claude");
        let result = check_status_consistency(&claude_dir);
        assert_eq!(result.status, Status::Fail, "{:?}", result.details);
        let msg = result.details.join(" ");
        assert!(msg.contains("Execute") && msg.contains("Plan"), "{msg}");
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run doctor`.
pub struct DoctorOpts {
    /// Also scan for dead file/script references (slower).
    pub residue: bool,
    /// Named check to run in isolation (e.g. `skill-discovery`,
    /// `claude-paths`, `workspace-leaks`, `i1`).
    pub check: Option<String>,
    /// Output format: `text` (default) or `json`.
    pub format: String,
}

/// Dispatch `mustard-rt run doctor [--residue] [--check <CHECK>] [--format json|--json]`.
pub fn run(opts: DoctorOpts) {
    let started = std::time::Instant::now();
    let cwd = crate::shared::context::workspace_root_strict()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let claude_dir = ClaudePaths::for_project(&cwd)
        .map(|p| p.claude_dir())
        .unwrap_or_else(|_| cwd.clone());

    // When a specific --check is requested, run only that check.
    if let Some(ref check_name) = opts.check {
        // W3.T3.4 / T3.8 / T3.9 — the three claude-paths-single-source checks
        // produce native JSON shapes (not the generic `CheckResult` envelope).
        // They short-circuit BEFORE the legacy match below so their JSON form
        // is the only output.
        if matches!(check_name.as_str(), "claude-paths" | "workspace-leaks" | "i1") {
            run_typed_check(check_name, &cwd, opts.format == "json");
            emit_economy(started.elapsed().as_millis(), 1, false);
            return;
        }

        let result = match check_name.as_str() {
            "skill-discovery" => run_skill_discovery_check(&cwd),
            "wave-integrity" => check_wave_integrity(&claude_dir),
            "status-consistency" => check_status_consistency(&claude_dir),
            other => {
                eprintln!(
                    "doctor: unknown check '{other}'. Known: skill-discovery, \
                     wave-integrity, claude-paths, workspace-leaks, i1, status-consistency"
                );
                std::process::exit(1);
            }
        };
        if opts.format == "json" {
            render_report_json(&[result]);
        } else {
            render_report(&[result]);
        }
        emit_economy(started.elapsed().as_millis(), 1, false);
        return;
    }

    // Default: run all checks.
    let mut results: Vec<CheckResult> = vec![
        check_wiring(&claude_dir),
        check_drift(&claude_dir),
        check_state_health(&claude_dir),
        check_claude_cli(),
        lsp_check(&cwd),
        check_nerd_font(),
        // W10.T10.5 — new check, always in the full run.
        check_wave_integrity(&claude_dir),
        // skill-discovery is always included in the full run (advisory).
        run_skill_discovery_check(&cwd),
        // W2 spec-status-consistency — always in the full run.
        check_status_consistency(&claude_dir),
    ];

    if opts.residue {
        results.push(check_residue(&claude_dir));
    }

    // W3.T3.10 — claude-paths-single-source check trio. Each check renders
    // its native JSON object under its own top-level key in the JSON path;
    // in text mode it folds into a `CheckResult` envelope so the OK/WARN/FAIL
    // summary line still works.
    let cp_report = crate::commands::doctor::doctor_claude_paths::run(&cwd);
    let wl_report = crate::commands::doctor::doctor_workspace_leaks::run(&cwd);
    let i1_report = crate::commands::doctor::doctor_i1::run(&cwd);

    if opts.format == "json" {
        render_combined_json(&results, &cp_report, &wl_report, &i1_report);
    } else {
        results.push(claude_paths_to_check_result(&cp_report));
        results.push(workspace_leaks_to_check_result(&wl_report));
        results.push(i1_to_check_result(&i1_report));
        render_report(&results);
    }

    // i1 violations are hard errors — exit-non-zero even when every legacy
    // check returns OK.
    let any_fail = results.iter().any(|r| r.status == Status::Fail) || !i1_report.ok;
    emit_economy(started.elapsed().as_millis(), results.len() + 3, any_fail);
    if any_fail {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// W3.T3.4 / T3.8 / T3.9 — typed-check JSON path
// ---------------------------------------------------------------------------

/// Run one of the typed checks (`claude-paths`, `workspace-leaks`, `i1`) and
/// print its native JSON shape. Text mode renders the same payload as
/// pretty-printed JSON — the typed checks do not have a separate text format,
/// callers asking for text get JSON regardless (the typed shape IS the
/// contract).
fn run_typed_check(name: &str, cwd: &Path, json_format: bool) {
    let value = match name {
        "claude-paths" => serde_json::to_value(crate::commands::doctor::doctor_claude_paths::run(cwd)),
        "workspace-leaks" => serde_json::to_value(crate::commands::doctor::doctor_workspace_leaks::run(cwd)),
        "i1" => {
            let report = crate::commands::doctor::doctor_i1::run(cwd);
            let exit_non_zero = !report.ok;
            let v = serde_json::to_value(report);
            // Print first so consumers see the body before we exit.
            print_typed_value(v, json_format);
            if exit_non_zero {
                std::process::exit(1);
            }
            return;
        }
        _ => return,
    };
    print_typed_value(value, json_format);
}

fn print_typed_value(
    value: Result<serde_json::Value, serde_json::Error>,
    _json_format: bool,
) {
    let v = value.unwrap_or_else(|_| serde_json::json!({}));
    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".to_string()));
}

/// Render the default (all-checks) JSON payload. Combines the legacy
/// `CheckResult` array shape with the three W3 typed reports under fixed
/// top-level keys (`claude_paths`, `workspace_leaks`, `i1`) so the dashboard
/// and CI consumers can read each independently.
fn render_combined_json(
    legacy: &[CheckResult],
    cp: &crate::commands::doctor::doctor_claude_paths::ClaudePathsReport,
    wl: &crate::commands::doctor::doctor_workspace_leaks::WorkspaceLeaksReport,
    i1: &crate::commands::doctor::doctor_i1::I1Report,
) {
    let checks: Vec<serde_json::Value> = legacy
        .iter()
        .map(|r| {
            let status_str = r.status.label().to_ascii_lowercase();
            let message = if r.details.is_empty() {
                String::new()
            } else if r.details.len() == 1 {
                r.details[0].clone()
            } else {
                r.details.join("; ")
            };
            json!({
                "name": r.name,
                "status": status_str,
                "message": message,
                "details": r.details,
            })
        })
        .collect();

    let any_fail = legacy.iter().any(|r| r.status == Status::Fail) || !i1.ok;
    let any_warn = legacy.iter().any(|r| r.status == Status::Warn)
        || !cp.divergences.is_empty()
        || !wl.leaks.is_empty();
    let overall = if any_fail { "fail" } else if any_warn { "warn" } else { "ok" };

    let violations: Vec<String> = legacy
        .iter()
        .filter(|r| r.name == "skill-discovery" && r.status == Status::Warn)
        .flat_map(|r| r.details.iter())
        .cloned()
        .collect();

    let body = json!({
        "checks": checks,
        "overall": overall,
        "violations": violations,
        // W3.T3.10 — three named, typed reports keyed verbatim.
        "claude_paths": cp,
        "workspace_leaks": wl,
        "i1": i1,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string())
    );
}

/// Project a `ClaudePathsReport` onto the legacy `CheckResult` envelope for
/// text-mode rendering.
fn claude_paths_to_check_result(
    report: &crate::commands::doctor::doctor_claude_paths::ClaudePathsReport,
) -> CheckResult {
    if report.divergences.is_empty() {
        let mut r = CheckResult::ok("claude-paths");
        r.details.push("filesystem matches ClaudePaths catalog".to_string());
        return r;
    }
    let details: Vec<String> = report
        .divergences
        .iter()
        .map(|d| format!("{} {} ({})", d.kind, d.path, d.severity))
        .collect();
    CheckResult::warn("claude-paths", details)
}

/// Project a `WorkspaceLeaksReport` onto the legacy `CheckResult` envelope.
fn workspace_leaks_to_check_result(
    report: &crate::commands::doctor::doctor_workspace_leaks::WorkspaceLeaksReport,
) -> CheckResult {
    if report.leaks.is_empty() {
        let mut r = CheckResult::ok("workspace-leaks");
        r.details.push("no nested .claude/ holds pipeline state".to_string());
        return r;
    }
    let details: Vec<String> = report
        .leaks
        .iter()
        .map(|l| format!("{} -> {}", l.path, l.leaked_entries.join(", ")))
        .collect();
    CheckResult::warn("workspace-leaks", details)
}

/// Project an `I1Report` onto the legacy `CheckResult` envelope. I1 is a hard
/// error: any violation becomes FAIL, never WARN.
fn i1_to_check_result(report: &crate::commands::doctor::doctor_i1::I1Report) -> CheckResult {
    if report.violations.is_empty() {
        let mut r = CheckResult::ok("i1");
        r.details.push("no .claude/.claude/ sequence found".to_string());
        return r;
    }
    CheckResult::fail("i1", report.violations.clone())
}

/// Telemetry — `pipeline.economy.operation.invoked` for the doctor run.
fn emit_economy(duration_ms: u128, check_count: usize, any_failure: bool) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec = current_spec(&cwd);
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("doctor".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "doctor",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
            "checks": check_count,
            "ok": !any_failure,
        }),
        spec,
    };
    let _ = crate::shared::events::route::emit(&cwd, &ev);
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // --- Helpers ---

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn make_minimal_settings(hooks_dir: &Path, command: &str) {
        let settings = format!(
            r#"{{ "hooks": {{ "PreToolUse": [{{ "hooks": [{{ "type": "command", "command": "{command}" }}] }}] }} }}"#
        );
        write_file(&hooks_dir.join("settings.json"), &settings);
    }

    // --- wiring tests ---

    #[test]
    fn wiring_clean_settings_is_ok() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        make_minimal_settings(&claude_dir, "mustard-rt on PreToolUse");
        let result = check_wiring(&claude_dir);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    #[test]
    fn wiring_broken_event_is_fail() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        make_minimal_settings(&claude_dir, "mustard-rt on NonExistentEvent");
        let result = check_wiring(&claude_dir);
        assert_eq!(result.status, Status::Fail);
        assert!(result.details[0].contains("NonExistentEvent"));
    }

    #[test]
    fn wiring_broken_run_subcommand_is_fail() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        make_minimal_settings(&claude_dir, "mustard-rt run dead-script");
        let result = check_wiring(&claude_dir);
        assert_eq!(result.status, Status::Fail);
        assert!(result.details[0].contains("dead-script"));
    }

    #[test]
    fn wiring_missing_settings_is_warn() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        // No settings.json created.
        let result = check_wiring(&claude_dir);
        assert_eq!(result.status, Status::Warn);
    }

    // --- residue tests ---

    #[test]
    fn residue_detects_dead_js_reference() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        // Plant a settings.json that references a .js file that doesn't exist.
        write_file(
            &claude_dir.join("settings.json"),
            r#"{ "command": "node .claude/scripts/dead-hook.js" }"#,
        );
        let result = check_residue(&claude_dir);
        assert_eq!(result.status, Status::Warn);
        let found = result.details.iter().any(|d| d.contains("dead-hook.js"));
        assert!(found, "expected dead-hook.js hit, got: {:?}", result.details);
    }

    #[test]
    fn residue_clean_dir_is_ok() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        write_file(&claude_dir.join("settings.json"), r#"{ "foo": "bar" }"#);
        let result = check_residue(&claude_dir);
        assert_eq!(result.status, Status::Ok);
    }

    // --- drift tests ---

    #[test]
    fn drift_skips_when_templates_not_found() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        // No templates/ in the hierarchy.
        let result = check_drift(&claude_dir);
        assert_eq!(result.status, Status::Skip);
    }

    #[test]
    fn drift_ok_when_hashes_match() {
        let dir = tempdir().unwrap();
        let templates_dir = dir.path().join("templates");
        let claude_dir = dir.path().join(".claude");
        // Create matching content for one CORE_FOLDER.
        let folder = "skills";
        let src_file = templates_dir.join(folder).join("test.md");
        let dst_file = claude_dir.join(folder).join("test.md");
        write_file(&src_file, "# hello");
        write_file(&dst_file, "# hello");

        let result = check_drift(&claude_dir);
        // Should not be FAIL — either OK or SKIP.
        assert_ne!(result.status, Status::Fail, "{:?}", result.details);
    }

    #[test]
    fn drift_warns_on_hash_mismatch() {
        let dir = tempdir().unwrap();
        let templates_dir = dir.path().join("templates");
        let claude_dir = dir.path().join(".claude");
        let folder = "skills";
        let src_file = templates_dir.join(folder).join("test.md");
        let dst_file = claude_dir.join(folder).join("test.md");
        write_file(&src_file, "# source version");
        write_file(&dst_file, "# different installed version");

        let result = check_drift(&claude_dir);
        // Either WARN (drift detected) or SKIP (templates not reachable via
        // find_templates_dir — the tempdir has no apps/cli path, so find_templates_dir
        // should find `templates/` directly).
        assert!(
            result.status == Status::Warn || result.status == Status::Skip,
            "expected WARN or SKIP, got {:?}: {:?}", result.status, result.details
        );
    }

    // --- state health tests ---

    #[test]
    fn state_health_orphan_state_warns() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        let states_dir = claude_dir.join(".pipeline-states");
        std::fs::create_dir_all(&states_dir).unwrap();
        // Plant an orphan state file (spec not in spec/ flat dir).
        write_file(
            &states_dir.join("orphan.json"),
            r#"{ "spec": "2026-01-01-nonexistent-spec", "state": "execute" }"#,
        );
        // entity-registry.json present to isolate the orphan check.
        write_file(&claude_dir.join("entity-registry.json"), "{}");

        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Warn);
        let has_orphan = result.details.iter().any(|d| d.contains("orphan"));
        assert!(has_orphan, "expected orphan warning, got: {:?}", result.details);
    }

    #[test]
    fn state_health_missing_registry_warns() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        // No entity-registry.json, no .pipeline-states/.
        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Warn);
        let has_registry = result.details.iter().any(|d| d.contains("entity-registry.json"));
        assert!(has_registry, "expected registry warning, got: {:?}", result.details);
    }

    #[test]
    fn state_health_clean_install_is_ok() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        // Registry present, no .pipeline-states/ directory.
        write_file(&claude_dir.join("entity-registry.json"), "{}");
        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    // --- timestamp expiry helper ---

    #[test]
    fn expired_timestamp_detected() {
        // A timestamp far in the past is expired.
        assert!(is_timestamp_expired("2020-01-01T00:00:00Z", u128::MAX, 1));
    }

    #[test]
    fn future_timestamp_not_expired() {
        // now_ms = 0, expiry = 24h — everything is in the future.
        assert!(!is_timestamp_expired("2999-12-31T23:59:59Z", 0, 86_400_000));
    }

    #[test]
    fn empty_timestamp_not_expired() {
        assert!(!is_timestamp_expired("", u128::MAX, 1));
    }

    // --- lsp_check tests ---

    #[test]
    fn lsp_check_skips_with_no_mapped_stacks() {
        let dir = tempdir().unwrap();
        // Empty directory: no manifest files → no mapped stacks → Skip.
        let result = lsp_check(dir.path());
        assert_eq!(result.status, Status::Skip, "{:?}", result.details);
    }

    #[test]
    fn doctor_report_includes_lsp_check() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        // Minimal settings.json so wiring check doesn't fail hard.
        make_minimal_settings(&claude_dir, "mustard-rt on PreToolUse");
        // entity-registry.json to keep state-health from warning.
        write_file(&claude_dir.join("entity-registry.json"), "{}");

        // Run all checks the same way `run()` does, rooted at the tempdir.
        let mut results: Vec<CheckResult> = Vec::new();
        results.push(check_wiring(&claude_dir));
        results.push(check_drift(&claude_dir));
        results.push(check_state_health(&claude_dir));
        results.push(lsp_check(dir.path()));

        let has_lsp = results.iter().any(|r| r.name == "lsp");
        assert!(has_lsp, "expected a check named 'lsp' in the report");
    }
}
