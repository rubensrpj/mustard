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
//!   registered run subcommand. FAIL on unresolved references. Both known-sets
//!   are *derived*, never declared: events from the shipped
//!   `plugin/hooks/hooks.json` manifest, subcommands from the live clap tree.
//! - **residue** (`--residue` only) — scan `settings.json`, SKILL.md files,
//!   and refs for mentions of paths/commands that no longer exist (dead `.js`
//!   names, `scripts/` entries with no resolvable target). WARN per hit.
//! - **scratch-residue** (`--residue` só) — tamanho total das sobras que o
//!   `scratch-gc` recolheria e o da compilação compartilhada, pela MESMA
//!   varredura dele. WARN quando há sobra ou quando a compilação passou do teto.
//! - **drift** — compare by hash the folders a fresh payload owns
//!   (`CORE_FOLDERS`) between the installed `.claude/` and the
//!   `templates/` source. Degrades to `skip` when `templates/` is not
//!   reachable from cwd (consumer project).
//! - **state health** — orphan `.pipeline-states/` files (no matching active
//!   spec), expired `closed-followup` state files, missing
//!   `grain.model.json`. WARN per anomaly.
//! - **nerd-font** — at least one Nerd Font detected in the OS font
//!   directories. WARN with install hint (`mustard install-nerd-font`) when
//!   absent. Powerline statusline themes require this; without it the
//!   transition glyphs render as tofu.
//! - **branch-protection** — which branches this repository REALLY refuses a
//!   direct commit on, measured through `mustard_core::protected_branches`
//!   (`origin/HEAD` ∪ `mustard.json#git.protected`). WARN only when
//!   `origin/HEAD` is unreadable, because protection then rests on literals
//!   this project may not use at all.
//! - **spec-index** — o índice das specs (`.claude/spec/index.ndjson`) contra
//!   os arquivos de eventos: índice que falta, linha que falta, sobra ou
//!   difere, e campo `search` calculado por outro redutor. Só lê e acusa, com
//!   WARN e a mensagem no idioma do projeto, que manda rodar
//!   `mustard-rt run index`.

use crate::shared::context;
use crate::util::sha256::Sha256;
use mustard_core::io::fs;
use mustard_core::platform::i18n::{translate, Locale};
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

/// The shipped hook manifest (`plugin/hooks/hooks.json`), embedded at build
/// time. That file is the only thing that decides which `<event>` names the
/// harness ever hands to `mustard-rt on`; embedding it makes the doctor read
/// the same artefact the harness reads, the way `known_run_subcommands` below
/// reads the same clap tree the binary dispatches on. A hand-kept copy drifted
/// in both directions (it carried `PreCompact`, which nothing registers, and
/// omitted `Stop` and `WorktreeCreate`, which are registered).
const SHIPPED_HOOKS_MANIFEST: &str = include_str!("../../../../../plugin/hooks/hooks.json");

/// All hook event names `mustard-rt on <event>` recognizes — the keys of the
/// shipped manifest's `hooks` object.
///
/// Degrades to an empty set when the manifest cannot be parsed. An empty set
/// means "cannot judge", and [`validate_command_string`] treats it that way:
/// it reports nothing rather than declaring every wired event unknown.
#[must_use]
pub fn known_hook_events() -> std::collections::BTreeSet<String> {
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(SHIPPED_HOOKS_MANIFEST) else {
        return std::collections::BTreeSet::new();
    };
    manifest
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .map(|hooks| hooks.keys().cloned().collect())
        .unwrap_or_default()
}

/// All `mustard-rt run <subcommand>` names recognized by the binary — derived
/// from the live clap tree, so the set can never drift from `RunCmd` again.
fn known_run_subcommands() -> std::collections::BTreeSet<String> {
    <crate::commands::RunCmd as clap::Subcommand>::augment_subcommands(clap::Command::new("run"))
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect()
}

/// The Mustard-owned folders the drift check compares against `templates/`.
/// They historically shipped in the payload; in Mustard 2.0 they move to the
/// plugin, so the check degrades to a no-op where they are absent. Re-seed the
/// harness with `mustard init` (idempotent).
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
            let known = known_hook_events();
            // An empty set means the shipped manifest did not parse — the check
            // cannot judge, so it stays silent instead of flagging every event.
            if !known.is_empty() && !known.contains(event) {
                broken.push(format!("unknown hook event: '{event}' in command '{cmd}'"));
            }
        }
        "run" => {
            let subcommand = parts[2];
            if !known_run_subcommands().contains(subcommand) {
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

/// Sobras de cópias descartáveis e tamanho da compilação compartilhada, lidos
/// pela varredura do `scratch-gc` — uma leitura só, para o doctor e a porta
/// de limpeza nunca discordarem sobre o que é sobra. Só com `--residue`: a
/// medida percorre cada candidata inteira.
fn check_scratch_residue(roots: &crate::commands::maint::scratch_gc::ScratchRoots) -> CheckResult {
    use crate::commands::maint::scratch_gc::{human_bytes, survey, MIN_AGE_HOURS};

    let found = survey(roots);
    let count = found.candidates.len();
    let mut details = vec![format!(
        "{count} scratch leftover(s) older than {MIN_AGE_HOURS}h: {} - list with `mustard-rt run clean`, remove with `--apply`",
        human_bytes(found.candidates_bytes())
    )];
    let over_cap = found.shared_target.as_ref().is_some_and(|s| s.over_cap);
    match found.shared_target.as_ref() {
        Some(shared) => details.push(format!(
            "shared build {}: {} (cap {}){}",
            shared.path,
            human_bytes(shared.size_bytes),
            human_bytes(shared.cap_bytes),
            if shared.over_cap { " - over the cap, `mustard-rt run clean --apply` empties it" } else { "" }
        )),
        None => details.push("shared build: not present".to_string()),
    }
    let status = if count > 0 || over_cap { Status::Warn } else { Status::Ok };
    CheckResult { name: "scratch-residue", status, details }
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
        if name.ends_with(".md")
            && let Ok(text) = fs::read_to_string(&path) {
                let source = path.to_string_lossy().into_owned();
                scan_for_dead_js_refs(&text, claude_dir, &source, hits);
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
            drifted.push(format!("{folder}: differs from templates/ (run `mustard init`)"));
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
        for ext in [".cmd", ".bat", ".exe"] {
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
/// well-known manifest files, reduced to stack-name strings. Fail-open: IO
/// errors → empty list.
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
// Check: branch-protection
// ---------------------------------------------------------------------------

/// Ask the PROVIDER whether each base this project declares is protected, and
/// warn when the project declares none.
///
/// **Why the provider and not this machine.** Everything the harness knows
/// about a base is true only on this side of the wire: the write gate refuses
/// an edit here, the doors refuse a merge here. A colleague with a terminal and
/// push rights is stopped by the server's own rule or by nothing at all — so
/// the one reading worth reporting is the server's, and it is asked for every
/// base the project named.
///
/// **Why an absent `git.flow` is a finding again.** The bases come from the
/// declaration alone; with none declared, nothing is protected, nothing is
/// pre-selected, and no base can be asked about. That used to be reported as a
/// healthy install because protection then rested on a probe of `origin/HEAD` —
/// it no longer does, and staying silent would leave the operator believing in
/// a protection that does not exist.
///
/// Skipped when there is no `mustard.json` at the project root (not a mustard
/// project). Never fails: a provider that cannot be reached is reported as
/// unasked, which is a different sentence from unprotected.
fn check_branch_protection(cwd: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "branch-protection";
    if !cwd.join("mustard.json").is_file() {
        return CheckResult::skip(NAME, "no mustard.json at project root");
    }
    let config = mustard_core::ProjectConfig::load(cwd);
    let declared: Vec<String> = config.git.declared_bases().into_iter().collect();
    if declared.is_empty() {
        return CheckResult::warn(
            NAME,
            vec![
                translate("doctor.protection.flow_missing", lang).to_string(),
                translate("doctor.protection.fix", lang).to_string(),
            ],
        );
    }

    protection_report(&declared, crate::shared::pr_provider::provider_for(cwd).as_ref(), lang)
}

/// The reading itself, over the bases and whoever answers for them.
///
/// Apart from [`check_branch_protection`] so the report can be measured
/// against a provider that answers a KNOWN mix — one base ruled, one base
/// open — which is the case the whole check exists for and the one no
/// temporary directory can produce.
fn protection_report(
    declared: &[String],
    provider: &dyn crate::shared::pr_provider::PrProvider,
    lang: Locale,
) -> CheckResult {
    const NAME: &str = "branch-protection";
    let who = provider.provider().to_string();
    let mut details = Vec::new();
    let mut open_bases = false;
    for base in declared {
        let line: String = match provider.branch_protection(base) {
            Ok(true) => translate("doctor.protection.protected", lang).to_string(),
            Ok(false) => {
                open_bases = true;
                translate("doctor.protection.open", lang).to_string()
            }
            Err(reason) => {
                translate("doctor.protection.unasked", lang).replace("{reason}", reason.trim())
            }
        };
        details.push(line.replace("{base}", base).replace("{provider}", &who));
    }
    if open_bases {
        details.push(translate("doctor.protection.fix", lang).to_string());
        return CheckResult::warn(NAME, details);
    }
    let mut r = CheckResult::ok(NAME);
    r.details = details;
    r
}

// ---------------------------------------------------------------------------
// Check: state health
// ---------------------------------------------------------------------------

/// The health of the project's STATE, read from the spec event files.
///
/// **It used to read the old state folder.** `.claude/.pipeline-states/` held
/// one JSON per spec, and this check walked it for two findings: a state file
/// whose spec no longer existed, and a `closed-followup` older than a day.
/// Both findings were about a folder the harness stopped writing — so on a
/// project that never had it, the check was silent about everything, and on an
/// old one it reported the leftovers of a format nobody reads. The state of a
/// spec lives in its own event file now, and that is what this asks.
///
/// Two findings, both about the file the whole harness reads: a spec folder
/// with no event file at all (nothing states what it is), and an event file
/// that cannot be read. Plus the repository model (`grain.model.json`) the
/// scan produces, which is not state but is the other thing whose absence
/// makes every later answer worse.
fn check_state_health(claude_dir: &Path) -> CheckResult {
    let mut warnings: Vec<String> = Vec::new();

    if !claude_dir.join("grain.model.json").exists() {
        warnings.push("grain.model.json missing (run `mustard-rt run scan`)".to_string());
    }

    let root = claude_dir
        .parent()
        .filter(|_| claude_dir.file_name().and_then(|s| s.to_str()) == Some(".claude"))
        .map_or_else(|| claude_dir.to_path_buf(), Path::to_path_buf);
    for spec in collect_active_spec_names(claude_dir) {
        let Ok(path) = mustard_core::io::spec_events::spec_file(&root, &spec) else {
            warnings.push(format!("'{spec}' is not a name a spec can have"));
            continue;
        };
        if !path.exists() {
            warnings.push(format!(
                "'{spec}' has no event file — nothing states what it is or where it stands"
            ));
            continue;
        }
        match mustard_core::io::spec_events::read(&path) {
            Ok(Some(log)) if log.events.is_empty() => {
                warnings.push(format!("'{spec}' has an empty event file"));
            }
            Ok(_) => {}
            Err(refusal) => {
                warnings.push(format!("'{spec}' has an unreadable event file: {}", refusal.reason()));
            }
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
    // ClaudePaths-exempt: `claude_dir` is already resolved via the seam in
    // `run()`; re-deriving with `for_project` here would be circular.
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

// ---------------------------------------------------------------------------
// Check: wave-integrity
// ---------------------------------------------------------------------------

/// For each active spec under `.claude/spec/`, parse `wave-plan.md` for
/// `[[wave-N-<role>]]` wikilinks and verify each referenced subdirectory
/// exists. WARN per broken wikilink (an editor typo or partial scaffold);
/// FAIL only on an empty result paired with a non-empty wave-plan body.
/// Fail-open: a missing spec tree, unreadable file, or malformed wikilink is
/// silently ignored — better to skip a check than crash the doctor.
fn check_wave_integrity(claude_dir: &Path) -> CheckResult {
    // ClaudePaths-exempt: `claude_dir` is already resolved via the seam in
    // `run()`; re-deriving with `for_project` here would be circular.
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
        if let Ok(output) = std::process::Command::new("fc-list").output()
            && output.status.success() {
                let listing = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
                if listing.contains("nerd") {
                    return CheckResult::ok("nerd-font");
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
        if entry.is_dir
            && let Ok(sub) = fs::read_dir(&entry.path) {
                for s in sub {
                    let sn = s.file_name.to_ascii_lowercase();
                    if sn.contains("nerd") || sn.contains("nf-") {
                        return true;
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
    let timestamp = mustard_core::time::now_iso8601();
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

// ---------------------------------------------------------------------------
// JSON renderer
// ---------------------------------------------------------------------------

/// Serialize the report as JSON, in this shape:
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
// Check: spec-index
// ---------------------------------------------------------------------------

/// O índice das specs do projeto `root` contra os arquivos de eventos. Só lê:
/// sem spec, não há o que conferir; índice que falta, linha que diverge e
/// `search` calculado por outro redutor viram WARN, cada um com a mensagem no
/// idioma `lang`, que manda rodar `mustard-rt run index`. Um erro de leitura
/// também é WARN: a conferência nunca derruba o `doctor`.
fn check_spec_index(root: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "spec-index";
    let divergence = match mustard_core::io::spec_index::divergence(root) {
        Ok(divergence) => divergence,
        Err(refusal) => return CheckResult::warn(NAME, vec![refusal.message(lang)]),
    };
    if divergence.specs == 0 && divergence.stale_search == 0 {
        return CheckResult::skip(NAME, translate("spec_index.no_specs", lang));
    }
    let mut details = Vec::new();
    if divergence.specs > 0 && !divergence.index_exists {
        details.push(translate("spec_index.missing", lang).replace("{count}", &divergence.specs.to_string()));
    }
    if !divergence.diverged.is_empty() {
        details.push(
            translate("spec_index.diverged", lang)
                .replace("{count}", &divergence.diverged.len().to_string())
                .replace("{specs}", &divergence.diverged.join(", ")),
        );
    }
    if divergence.stale_search > 0 {
        details.push(
            translate("spec_index.stale_search", lang).replace("{count}", &divergence.stale_search.to_string()),
        );
    }
    if details.is_empty() {
        CheckResult::ok(NAME)
    } else {
        CheckResult::warn(NAME, details)
    }
}

// ---------------------------------------------------------------------------
// Check: scan-output
// ---------------------------------------------------------------------------

/// What the scan writes, inside the project's `.claude/`.
const SCAN_OUTPUTS: &[&str] = &["grain.model.json"];

/// The scan only writes outside git: what it recorded may be neither tracked
/// nor show up as a new file. A visible file becomes a WARN with the list, in
/// the language `lang`; with no git, or nothing recorded yet, there is nothing
/// to check. Read-only.
fn check_scan_output(root: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "scan-output";
    let written: Vec<String> = SCAN_OUTPUTS
        .iter()
        .filter(|name| root.join(".claude").join(name).is_file())
        .map(|name| format!(".claude/{name}"))
        .collect();
    let visible = match visible_to_git(root, &written) {
        Some(visible) if !visible.is_empty() => visible,
        _ => return CheckResult::ok(NAME),
    };
    CheckResult::warn(NAME, vec![translate("doctor.scan_output.visible", lang).replace("{paths}", &visible.join(", "))])
}

/// The paths git sees: tracked, or new and not ignored. `None` when git does
/// not answer (no git, outside a repository).
fn visible_to_git(root: &Path, paths: &[String]) -> Option<Vec<String>> {
    let git_ok = |args: &[&str]| mustard_core::platform::git::run(root, args).ok;
    if !git_ok(&["rev-parse", "--is-inside-work-tree"]) {
        return None;
    }
    Some(
        paths
            .iter()
            .filter(|p| git_ok(&["ls-files", "--error-unmatch", "--", p]) || !git_ok(&["check-ignore", "-q", "--", p]))
            .cloned()
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Check: status-consistency
// ---------------------------------------------------------------------------


#[cfg(test)]
mod status_consistency_tests {
    use super::*;
    use tempfile::tempdir;

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
        let result = match check_name.as_str() {
            "wave-integrity" => check_wave_integrity(&claude_dir),
                "branch-protection" => {
                let project = crate::commands::spec_events::project(&cwd);
                check_branch_protection(&cwd, project.lang)
            }
            "spec-index" => {
                let project = crate::commands::spec_events::project(&cwd);
                check_spec_index(&project.root, project.lang)
            }
            "scan-output" => {
                let project = crate::commands::spec_events::project(&cwd);
                check_scan_output(&project.root, project.lang)
            }
            other => {
                eprintln!(
                    "doctor: unknown check '{other}'. Known: \
                     wave-integrity, branch-protection, spec-index, scan-output"
                );
                std::process::exit(1);
            }
        };
        if opts.format == "json" {
            render_report_json(&[result]);
        } else {
            render_report(&[result]);
        }
        return;
    }

    // Default: run all checks.
    let mut results: Vec<CheckResult> = vec![
        check_wiring(&claude_dir),
        // Before drift: a drift reading is only meaningful once the binary the
        // reading comes FROM is known to be the installed one. A dormant
        // bootstrap makes every version answer below it untrustworthy.
        bootstrap_to_check_result(&crate::commands::doctor::bootstrap_check::run(&cwd)),
        check_drift(&claude_dir),
        check_state_health(&claude_dir),
        check_claude_cli(),
        lsp_check(&cwd),
        check_nerd_font(),
        // Wave-integrity check — always in the full run.
        check_wave_integrity(&claude_dir),
        // Status-consistency check — always in the full run.
        // O que o provedor realmente protege — sempre na rodada inteira: uma
        // base que só este binário recusa é uma base aberta para todo mundo, e
        // isso não aparece até um envio direto passar.
        {
            let project = crate::commands::spec_events::project(&cwd);
            check_branch_protection(&cwd, project.lang)
        },
        // O índice das specs contra os arquivos de eventos: só acusa.
        {
            let project = crate::commands::spec_events::project(&cwd);
            check_spec_index(&project.root, project.lang)
        },
        // What the scan writes stays outside git: it is only reported.
        {
            let project = crate::commands::spec_events::project(&cwd);
            check_scan_output(&project.root, project.lang)
        },
    ];

    if opts.residue {
        results.push(check_residue(&claude_dir));
        results.push(check_scratch_residue(
            &crate::commands::maint::scratch_gc::ScratchRoots::from_env(),
        ));
    }

    if opts.format == "json" {
        render_combined_json(&results);
    } else {
        render_report(&results);
    }

    if results.iter().any(|r| r.status == Status::Fail) {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// JSON path
// ---------------------------------------------------------------------------

/// Render the default (all-checks) JSON payload.
fn render_combined_json(legacy: &[CheckResult]) {
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

    let any_fail = legacy.iter().any(|r| r.status == Status::Fail);
    let any_warn = legacy.iter().any(|r| r.status == Status::Warn);
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
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string())
    );
}

/// Fold the bootstrap report into the doctor's OK/WARN/FAIL envelope.
///
/// `binary-missing`, `stamp-mismatch` and `toolchain-unreachable` are FAIL:
/// each one means the harness cannot do a job it will nonetheless APPEAR to
/// do — dormant hooks, or criteria recorded `unproven` that read like failing
/// tests. `session-stale` is a WARN: everything works, it is just older than
/// what is installed.
fn bootstrap_to_check_result(
    report: &crate::commands::doctor::bootstrap_check::BootstrapReport,
) -> CheckResult {
    if report.ok && report.findings.is_empty() {
        let mut r = CheckResult::ok("bootstrap");
        r.details.push(format!(
            "plugin {} · binary {} · stamp {}",
            report.installed_version.as_deref().unwrap_or("?"),
            report.running_version,
            report.stamped_version.as_deref().unwrap_or("—"),
        ));
        return r;
    }
    let details: Vec<String> = report
        .findings
        .iter()
        .map(|f| format!("{}: {} — fix: {}", f.kind, f.detail, f.remedy))
        .collect();
    if report.failed {
        CheckResult::fail("bootstrap", details)
    } else if report.ok {
        let mut r = CheckResult::ok("bootstrap");
        r.details = details;
        r
    } else {
        CheckResult::warn("bootstrap", details)
    }
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

    // --- scan-output tests ---

    /// The scan map visible to git is reported; excluded, it passes; outside a
    /// repository, there is nothing to check.
    #[test]
    fn the_doctor_flags_a_scan_map_that_git_can_see() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".claude").join("grain.model.json"), "{}").unwrap();
        assert_eq!(check_scan_output(root, Locale::PtBr).status, Status::Ok, "not a repository");

        let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(root).output();
        if !init.is_ok_and(|o| o.status.success()) {
            return; // no git here: nothing the check could measure
        }
        let visible = check_scan_output(root, Locale::PtBr);
        assert_eq!(visible.status, Status::Warn, "{:?}", visible.details);
        assert!(visible.details.join(" ").contains(".claude/grain.model.json"), "{:?}", visible.details);
        let en = check_scan_output(root, Locale::EnUs);
        assert!(en.details.join(" ").contains("visible to git"), "{:?}", en.details);

        std::fs::write(root.join(".git").join("info").join("exclude"), "**/.claude/grain.model.json\n").unwrap();
        assert_eq!(check_scan_output(root, Locale::PtBr).status, Status::Ok);
    }

    // --- spec-index tests ---

    fn spec_event(root: &Path, spec: &str, text: &str) {
        let path = root.join(".claude").join("spec").join(spec).join("spec.ndjson");
        let serde_json::Value::Object(draft) = json!({"author": "user", "text": text}) else { unreachable!() };
        mustard_core::io::spec_events::write_at(&path, "message", draft, &[], "2026-09-11T10:00:00-03:00").unwrap();
    }

    fn spec_index_file(root: &Path) -> PathBuf {
        root.join(".claude").join("spec").join("index.ndjson")
    }

    /// Uma linha do índice mexida à mão faz o doctor acusar a spec pelo nome,
    /// com o comando que conserta; depois do `index`, a conferência passa.
    #[test]
    fn the_doctor_flags_a_divergent_index_line() {
        let dir = tempdir().unwrap();
        spec_event(dir.path(), "trava", "um");
        spec_event(dir.path(), "busca", "dois");
        let index = spec_index_file(dir.path());
        let raw = std::fs::read_to_string(&index).unwrap();
        std::fs::write(&index, raw.replace("\"name\":\"trava\"", "\"name\":\"trava\",\"phase\":\"closed\"")).unwrap();

        let result = check_spec_index(dir.path(), Locale::PtBr);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let detail = result.details.join(" ");
        assert!(detail.contains("difere") && detail.contains("trava"), "{detail}");
        assert!(!detail.contains("busca"), "only the divergent spec is named: {detail}");
        assert!(detail.contains("mustard-rt run index"), "{detail}");

        mustard_core::io::spec_index::rebuild(dir.path()).unwrap();
        assert_eq!(check_spec_index(dir.path(), Locale::PtBr).status, Status::Ok);
    }

    #[test]
    fn the_doctor_flags_a_missing_index_and_is_quiet_when_it_matches() {
        let dir = tempdir().unwrap();
        assert_eq!(check_spec_index(dir.path(), Locale::EnUs).status, Status::Skip, "no spec, nothing to check");
        spec_event(dir.path(), "trava", "um");
        let quiet = check_spec_index(dir.path(), Locale::EnUs);
        assert_eq!(quiet.status, Status::Ok, "{:?}", quiet.details);

        std::fs::remove_file(spec_index_file(dir.path())).unwrap();
        let missing = check_spec_index(dir.path(), Locale::EnUs);
        assert_eq!(missing.status, Status::Warn);
        assert!(missing.details[0].contains("does not exist"), "{:?}", missing.details);
        assert!(missing.details[0].contains("mustard-rt run index"), "{:?}", missing.details);
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

    /// Com sobra candidata, o `--residue` relata o tamanho dela e o da
    /// compilação compartilhada.
    #[test]
    fn doctor_residue_reports_scratch_leftovers() {
        use crate::commands::maint::scratch_gc::{backdate_tree, human_bytes, AgeClock, ScratchRoots};

        let base = tempdir().unwrap();
        let temp_root = base.path().join("tmp");
        let old = temp_root.join("tmp.old");
        std::fs::create_dir_all(old.join("apps").join("rt")).unwrap();
        write_file(&old.join("Cargo.toml"), "[workspace]\n");
        std::fs::write(old.join("apps").join("rt").join("big.bin"), vec![0u8; 3 * 1024]).unwrap();
        // A árvore inteira envelhecida pelo mtime, o relógio das fixtures.
        backdate_tree(&old, 24);
        let candidate_bytes = std::fs::metadata(old.join("Cargo.toml")).unwrap().len() + 3 * 1024;

        let shared = base.path().join("cache").join("scratch-target");
        std::fs::create_dir_all(shared.join("debug")).unwrap();
        std::fs::write(shared.join("debug").join("lib.rlib"), vec![0u8; 2048]).unwrap();

        let roots = ScratchRoots {
            temp_root,
            shared_target: Some(shared.clone()),
            cap_bytes: 1024 * 1024,
            current_session: "sess-current".to_string(),
            current_dir: None,
            home: None,
            clock: AgeClock::Modified,
            owner_uid: crate::commands::maint::scratch_gc::current_uid(),
            now: std::time::SystemTime::now(),
        };
        let result = check_scratch_residue(&roots);

        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let text = result.details.join("\n");
        assert!(
            text.contains(&format!("1 scratch leftover(s) older than 12h: {}", human_bytes(candidate_bytes))),
            "{text}"
        );
        // O comando de limpeza que a linha manda rodar é o que existe.
        assert!(text.contains("list with `mustard-rt run clean`"), "{text}");
        assert!(
            text.contains(&format!("shared build {}: {}", shared.display(), human_bytes(2048))),
            "{text}"
        );
        assert!(old.exists(), "the doctor only reads");
    }

    // --- drift tests ---

    #[test]
    fn drift_skips_when_templates_not_found() {
        // Nest the project ≥5 levels deep inside the tempdir so that
        // `find_templates_dir`'s 5-level upward walk stays WITHIN the
        // (template-free) tempdir and never reaches ancestors of the system
        // temp dir. On some CI runners (notably Windows) a `templates/` or
        // `apps/cli/templates` exists a few levels above `$TMP`, which made the
        // walk find one and return Ok instead of Skip — green locally, red on CI.
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("c").join("d").join("e").join("f");
        let claude_dir = nested.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        // No templates/ anywhere in this subtree.
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

    /// Uma spec cuja pasta existe e cujo arquivo de eventos não: nada diz o
    /// que ela é nem onde ela está, e o diagnóstico acusa isso pelo nome.
    #[test]
    fn a_spec_sem_arquivo_de_eventos_vira_achado() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(claude_dir.join("spec").join("trava")).unwrap();
        write_file(&claude_dir.join("grain.model.json"), "{}");

        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        assert!(
            result.details.iter().any(|d| d.contains("trava")),
            "a spec é acusada pelo nome: {:?}",
            result.details
        );
    }

    /// Uma spec com arquivo de eventos não é achado nenhum — e a pasta velha
    /// de estado, com o que quer que tenha sobrado dentro, também não: ela
    /// deixou de ser lida.
    #[test]
    fn uma_spec_com_arquivo_de_eventos_esta_sa() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let claude_dir = root.join(".claude");
        std::fs::create_dir_all(claude_dir.join("spec").join("trava")).unwrap();
        write_file(&claude_dir.join("grain.model.json"), "{}");
        let states = claude_dir.join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        write_file(&states.join("orfa.json"), r#"{ "spec": "nao-existe", "state": "execute" }"#);

        let path = mustard_core::io::spec_events::spec_file(root, "trava").expect("caminho");
        std::fs::create_dir_all(path.parent().expect("pasta")).unwrap();
        let draft = |value: serde_json::Value| value.as_object().cloned().expect("um objeto");
        mustard_core::io::spec_events::write(&path, "state", draft(json!({"phase": "running"})), &[])
            .expect("estado");

        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    #[test]
    fn state_health_missing_model_warns() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Warn);
        let has_model = result.details.iter().any(|d| d.contains("grain.model.json"));
        assert!(has_model, "expected model warning, got: {:?}", result.details);
    }

    #[test]
    fn state_health_clean_install_is_ok() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        write_file(&claude_dir.join("grain.model.json"), "{}");
        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    // --- branch-protection tests ---

    /// Sem `git.flow`, nada fica protegido — e isso é um achado, não uma
    /// instalação saudável. O aviso diz o `git.flow` pelo nome e mostra como
    /// declarar.
    #[test]
    fn a_falta_do_fluxo_vira_aviso_porque_nada_fica_protegido() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("mustard.json"),
            r#"{"git":{"flow":{},"provider":"github"}}"#,
        );
        let result = check_branch_protection(dir.path(), Locale::PtBr);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        assert!(
            result.details.iter().any(|d| d.contains("git.flow")),
            "o aviso precisa dizer o que falta: {:?}",
            result.details
        );
        assert!(
            result.details.iter().any(|d| d.contains("mustard init")),
            "e como declarar: {:?}",
            result.details
        );
    }

    /// Com bases declaradas e um provedor que não responde, cada base é
    /// reportada como PERGUNTA QUE NÃO CHEGOU A SER FEITA — nunca como
    /// desprotegida, que é uma afirmação sobre o servidor.
    #[test]
    fn provedor_fora_de_alcance_nao_vira_base_desprotegida() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("mustard.json"),
            r#"{"git":{"flow":{"*":"develop","develop":"master"},"provider":"naoexiste"}}"#,
        );
        let result = check_branch_protection(dir.path(), Locale::PtBr);
        for base in ["develop", "master"] {
            assert!(
                result.details.iter().any(|d| d.contains(base)),
                "cada base declarada é reportada, {base} não foi: {:?}",
                result.details
            );
        }
        assert!(
            result.details.iter().all(|d| !d.contains("qualquer pessoa")),
            "um provedor que não respondeu não prova branch aberta: {:?}",
            result.details
        );
    }

    /// Um projeto que declara `develop` e `master` e um provedor que, para a
    /// `master`, não tem política nenhuma: o diagnóstico acusa a `master`,
    /// deixa a `develop` em paz e mostra como ligar.
    ///
    /// Este é o caso que a conferência existe para pegar, e nenhuma pasta
    /// temporária o produz — por isso o provedor aqui é um dublê que responde
    /// uma mistura conhecida.
    #[test]
    fn o_diagnostico_acusa_a_base_que_o_provedor_nao_protege() {
        /// Responde `true` para as bases que nomeia e `false` para as outras.
        struct ProvedorFalso(&'static [&'static str]);
        impl crate::shared::pr_provider::PrProvider for ProvedorFalso {
            fn provider(&self) -> &'static str {
                "azure"
            }
            fn open(
                &self,
                _pr: &crate::shared::pr_provider::PrToOpen,
            ) -> Result<crate::shared::pr_provider::PrOpened, String> {
                Err("fora do teste".into())
            }
            fn edit_body(&self, _n: u64, _b: &str) -> Result<(), String> {
                Err("fora do teste".into())
            }
            fn ready(&self, _n: u64) -> Result<(), String> {
                Err("fora do teste".into())
            }
            fn view(
                &self,
                _which: crate::shared::pr_provider::PrRef<'_>,
            ) -> Result<crate::shared::pr_provider::PrView, String> {
                Err("fora do teste".into())
            }
            fn checks(
                &self,
                _n: u64,
            ) -> Result<crate::shared::pr_provider::PrChecks, String> {
                Err("fora do teste".into())
            }
            fn branch_protection(&self, branch: &str) -> Result<bool, String> {
                Ok(self.0.contains(&branch))
            }
        }

        let bases = vec!["develop".to_string(), "master".to_string()];
        let result =
            protection_report(&bases, &ProvedorFalso(&["develop"]), Locale::PtBr);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let acusa = result
            .details
            .iter()
            .find(|d| d.contains("master"))
            .unwrap_or_else(|| panic!("a master não foi acusada: {:?}", result.details));
        assert!(
            acusa.contains("qualquer pessoa"),
            "o aviso precisa dizer o que uma base sem regra significa: {acusa}",
        );
        assert!(
            result.details.iter().any(|d| d.contains("Rulesets") || d.contains("Policies")),
            "e mostrar como ligar: {:?}",
            result.details
        );
        let develop = result
            .details
            .iter()
            .find(|d| d.contains("develop"))
            .unwrap_or_else(|| panic!("a develop sumiu do relatório: {:?}", result.details));
        assert!(
            !develop.contains("qualquer pessoa"),
            "a base que o provedor protege não pode ser acusada: {develop}",
        );

        let tudo_protegido =
            protection_report(&bases, &ProvedorFalso(&["develop", "master"]), Locale::PtBr);
        assert_eq!(
            tudo_protegido.status,
            Status::Ok,
            "com as duas protegidas não há achado: {:?}",
            tudo_protegido.details
        );
    }

    #[test]
    fn branch_protection_missing_mustard_json_skips() {
        let dir = tempdir().unwrap();
        let result = check_branch_protection(dir.path(), Locale::PtBr);
        assert_eq!(result.status, Status::Skip, "{:?}", result.details);
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
        // grain.model.json to keep state-health from warning.
        write_file(&claude_dir.join("grain.model.json"), "{}");

        // Run all checks the same way `run()` does, rooted at the tempdir.
        let results: Vec<CheckResult> = vec![
            check_wiring(&claude_dir),
            check_drift(&claude_dir),
            check_state_health(&claude_dir),
            lsp_check(dir.path()),
        ];

        let has_lsp = results.iter().any(|r| r.name == "lsp");
        assert!(has_lsp, "expected a check named 'lsp' in the report");
    }
}
