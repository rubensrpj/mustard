//! Parity ratchet between the published `mustard-rt run` surface and every
//! product caller (templates, CLI sources, installer, packaging, command doc).
//!
//! Complements `run_command_surface.rs` (which locks the clap tree itself):
//!
//! - **FORWARD** — every `mustard-rt run <name>` a product file instructs must
//!   resolve to a registered subcommand. A template pointing at a name that no
//!   longer exists does not break the build — the command silently VANISHES at
//!   runtime. This walk turns that into a test failure.
//! - **REVERSE** — every registered subcommand must have at least one static
//!   product caller (prose instruction or spawned argv), or a justified entry
//!   in [`RUNTIME_WHITELIST`]. A command nobody calls is dark surface: it
//!   ships, it bit-rots, and nothing notices.
//!
//! Deterministic: walks the repo tree only (sorted), no network, no env vars.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Command, Subcommand};
use mustard_rt::commands::RunCmd;

/// Registered commands with no static product caller that are still shipped
/// deliberately. Each justification cites where the runtime caller or the
/// instructing surface actually lives. A name with NO honest justification
/// must NOT be parked here — remove the registration instead. Kept sorted.
const RUNTIME_WHITELIST: &[(&str, &str)] = &[
    (
        "adapt-cursor",
        "user-invoked .cursorrules generator (commands/maint/adapt_cursor.rs); its \n         only prose caller was the pre-2.0 `init --cursor` hint, dropped by the \n         thin-init rewrite; a maintenance escape hatch with no scripted caller",
    ),
    (
        "amend-finalize",
        "SessionEnd finalizes the amend window in-process \
         (hooks/session/session_cleanup_observer.rs); the CLI face is the \
         documented manual re-run for a crashed session",
    ),
    (
        "claude-dir-prune",
        "user-invoked .claude/ drift audit (commands/maint/claude_dir_prune.rs \
         module doc); maintenance escape hatch with no scripted caller",
    ),
    (
        "dependency-precheck",
        "EXECUTE pre-gate the orchestrator runs from the bare-name instruction \
         in commands/mustard/feature/SKILL.md section 3 (never spelled with \
         the mustard-rt prefix there)",
    ),
    (
        "docs-stale-check",
        "CLOSE gate 4 - run in-process by close-orchestrate and named (with \
         --skip-docs) in commands/mustard/close/SKILL.md; the CLI face is the \
         standalone re-run",
    ),
    (
        "exec-rewave-check",
        "EXECUTE pre-gate named bare in commands/mustard/feature/SKILL.md \
         section 3 dispatch chain",
    ),
    (
        "gate-regression-check",
        "regression-gate engine consumed in-process \
         (commands/agent/context_inject.rs build_vocab_matcher; \
         review_spans.rs parses its verdicts); the CLI face has no scripted \
         caller - flagged as dark surface in the F1 LOT C report",
    ),
    (
        "mark-checklist-item",
        "instructed by the close-gate deny remediation \
         (commands/pipeline/close_gates.rs: mark each via mustard-rt run \
         mark-checklist-item)",
    ),
    (
        "metrics-wave-status",
        "user-facing wave telemetry; main.rs keeps the two-token rewrite \
         (metrics wave-status) for human invocation - its dashboard spawn was \
         removed in the 2.0 dashboard cut (flagged in the F1 LOT C report)",
    ),
    (
        "pipeline-summary",
        "CLOSE gate 5 (advisory) - run in-process by close-orchestrate and \
         named in commands/mustard/close/SKILL.md step 7",
    ),
    (
        "rebuild-specs",
        "manual repair tool: regenerates the committed .summary.json sidecars \
         (commands/spec/rebuild_specs.rs module doc); user-invoked only \
         (flagged in the F1 LOT C report)",
    ),
    (
        "review-dispatch",
        "built to replace the review SKILL's imperative steps, but the SKILL \
         still calls review-prefetch/diff-context directly - unadopted \
         (flagged as dark surface in the F1 LOT C report)",
    ),
    (
        "security-scan",
        "secret/permission scanner with an exit-code contract \
         (commands/review/security_scan.rs, JS-era port); no product caller \
         since scripts/ was retired (flagged as dark surface in the F1 LOT C \
         report)",
    ),
    (
        "worktree-gc",
        "SessionStart probe runs it in-process (hooks/session/\
         session_start_inject.rs -> worktree_gc::session_start_probe) and the \
         command is the probe's remediation",
    ),
];

/// Caller spellings that precede a `run <name>` instruction in product files.
/// `$RtExe` is `install.ps1`'s handle for the freshly built `mustard-rt.exe`.
const CALLER_PREFIXES: &[&str] = &["mustard-rt run ", "mustard-rt.exe run ", "$RtExe run "];

/// The repo root, resolved from this crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Build the `run` command tree exactly as `main.rs` hands it to clap.
fn run_command_tree() -> Command {
    let mut cmd = RunCmd::augment_subcommands(Command::new("run"));
    cmd.build();
    cmd
}

/// Every declared `run` subcommand name (clap's auto `help` excluded), sorted.
fn surface_names() -> Vec<String> {
    let cmd = run_command_tree();
    let mut names: Vec<String> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .filter(|n| n != "help")
        .collect();
    names.sort_unstable();
    names
}

/// Recursively collect files under `dir` in a deterministic (sorted) order.
fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "node_modules" || name == "target" || name == ".git" {
                continue;
            }
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Read a file as lossy UTF-8; unreadable files degrade to an empty string.
fn read_lossy(path: &Path) -> String {
    fs::read(path).map_or_else(|_| String::new(), |b| String::from_utf8_lossy(&b).into_owned())
}

fn has_extension(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.contains(&e))
}

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
}

/// Extract every `run <name>` instruction reachable through one of the
/// [`CALLER_PREFIXES`], normalizing the two two-token rewrite forms
/// (`metrics wave-status` and `scan spec`, collapsed by `main.rs` argv
/// pre-routing) to their registered single-token names.
fn extract_run_names(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for prefix in CALLER_PREFIXES {
        let mut from = 0;
        while let Some(pos) = text[from..].find(prefix) {
            let start = from + pos + prefix.len();
            from = start;
            let mut end = start;
            while end < bytes.len() && is_token_byte(bytes[end]) {
                end += 1;
            }
            if end == start || !bytes[start].is_ascii_lowercase() {
                continue;
            }
            let first = &text[start..end];
            let mut name = first.to_string();
            if end < bytes.len() && bytes[end] == b' ' {
                let second_start = end + 1;
                let mut second_end = second_start;
                while second_end < bytes.len() && is_token_byte(bytes[second_end]) {
                    second_end += 1;
                }
                if second_end > second_start && bytes[second_start].is_ascii_lowercase() {
                    match (first, &text[second_start..second_end]) {
                        ("metrics", "wave-status") => name = "metrics-wave-status".to_string(),
                        ("scan", "spec") => name = "scan-spec".to_string(),
                        _ => {}
                    }
                }
            }
            out.push(name);
        }
    }
    out
}

/// The files whose `run <name>` instructions the FORWARD check validates.
fn forward_corpus(root: &Path) -> Vec<PathBuf> {
    let mut files = reverse_prose_corpus(root);
    walk_files(&root.join("packaging"), &mut files);
    files.push(root.join("MUSTARD-COMMANDS.md"));
    files
}

/// The prose half of the REVERSE caller corpus: templates (md/json, which
/// includes the settings.json seed), the CLI sources, and the installer.
fn reverse_prose_corpus(root: &Path) -> Vec<PathBuf> {
    let templates = root.join("apps/cli/templates");
    assert!(templates.is_dir(), "templates dir missing at {}", templates.display());
    let mut files = Vec::new();
    walk_files(&templates, &mut files);
    // The harness seeds (settings.json — whose permissions/statusLine name
    // `mustard-rt run` commands — and the injectable instruction files) moved
    // to `packages/core/templates/`, compiled into the binaries via
    // `include_str!`. They are product callers all the same.
    let core_templates = root.join("packages/core/templates");
    assert!(
        core_templates.is_dir(),
        "core seed dir missing at {}",
        core_templates.display()
    );
    walk_files(&core_templates, &mut files);
    files.retain(|p| has_extension(p, &["md", "json"]));

    // Mustard 2.0: the command/skill/ref callers moved from `apps/cli/templates`
    // into the `plugin/` tree (init ships them via the plugin, not a copy). Walk
    // it too so those `mustard-rt run <name>` instructions still count as product
    // callers — otherwise every plugin-hosted command reads as dark surface.
    let plugin = root.join("plugin");
    if plugin.is_dir() {
        let mut plugin_files = Vec::new();
        walk_files(&plugin, &mut plugin_files);
        plugin_files.retain(|p| has_extension(p, &["md", "json"]));
        files.extend(plugin_files);
    }

    let mut cli_sources = Vec::new();
    walk_files(&root.join("apps/cli/src"), &mut cli_sources);
    cli_sources.retain(|p| has_extension(p, &["rs"]));
    files.extend(cli_sources);
    let installer = root.join("install.ps1");
    assert!(installer.is_file(), "install.ps1 missing at {}", installer.display());
    files.push(installer);
    files
}

/// Collapse all whitespace runs to single spaces so multi-line argv arrays
/// (a quoted "run" and the quoted name split across lines) match their
/// single-line spelling.
fn squash_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `true` when some Rust source spawns `mustard-rt` with `["run", "<name>"]`.
///
/// rt sources exclude the registration/list surfaces (`cli.rs` family files,
/// `doctor.rs` known-list) and the command's own module — a command's own
/// docs are not a caller. Dashboard (`src-tauri`) sources count in full.
fn has_argv_caller(root: &Path, name: &str) -> bool {
    let needle = format!("\"run\", \"{name}\"");
    let own_module = format!("{}.rs", name.replace('-', "_"));

    let mut rt_sources = Vec::new();
    walk_files(&root.join("apps/rt/src"), &mut rt_sources);
    let mut dash_sources = Vec::new();
    walk_files(&root.join("apps/dashboard/src-tauri/src"), &mut dash_sources);

    let excluded = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "cli.rs" || n == "doctor.rs" || n == own_module)
    };
    rt_sources
        .iter()
        .filter(|p| has_extension(p, &["rs"]) && !excluded(p))
        .chain(dash_sources.iter().filter(|p| has_extension(p, &["rs"])))
        .any(|p| squash_whitespace(&read_lossy(p)).contains(&needle))
}

#[test]
fn forward_every_instructed_run_name_is_registered() {
    let root = repo_root();
    let registered: BTreeSet<String> = surface_names().into_iter().collect();
    let mut offenders = Vec::new();
    for file in forward_corpus(&root) {
        let text = read_lossy(&file);
        for name in extract_run_names(&text) {
            if !registered.contains(&name) {
                let shown = file.strip_prefix(&root).unwrap_or(&file);
                offenders.push(format!("{} -> `run {name}`", shown.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "product files instruct `mustard-rt run` names the CLI does not \
         register - the call dies silently at runtime. Fix the file or \
         register the command:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn reverse_every_registered_name_has_a_caller_or_a_justification() {
    let root = repo_root();
    let instructed: BTreeSet<String> = reverse_prose_corpus(&root)
        .iter()
        .flat_map(|p| extract_run_names(&read_lossy(p)))
        .collect();
    let mut dark = Vec::new();
    for name in surface_names() {
        let whitelisted = RUNTIME_WHITELIST.iter().any(|(n, _)| *n == name);
        if whitelisted || instructed.contains(&name) || has_argv_caller(&root, &name) {
            continue;
        }
        dark.push(name);
    }
    assert!(
        dark.is_empty(),
        "registered `run` subcommands with no product caller (templates, CLI \
         sources, installer, settings template, rt/dashboard argv spawns) and \
         no RUNTIME_WHITELIST justification - dark surface. Wire a caller, \
         add a JUSTIFIED whitelist entry, or remove the registration:\n{}",
        dark.join("\n")
    );
}

#[test]
fn runtime_whitelist_stays_sorted_live_and_not_redundant() {
    for pair in RUNTIME_WHITELIST.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "RUNTIME_WHITELIST must stay sorted: {} before {}",
            pair[0].0,
            pair[1].0
        );
    }
    let registered: BTreeSet<String> = surface_names().into_iter().collect();
    let root = repo_root();
    let instructed: BTreeSet<String> = reverse_prose_corpus(&root)
        .iter()
        .flat_map(|p| extract_run_names(&read_lossy(p)))
        .collect();
    for (name, justification) in RUNTIME_WHITELIST {
        assert!(
            registered.contains(*name),
            "RUNTIME_WHITELIST entry {name} is not a registered subcommand - drop the row"
        );
        assert!(
            !justification.trim().is_empty(),
            "RUNTIME_WHITELIST entry {name} carries no justification"
        );
        assert!(
            !(instructed.contains(*name) || has_argv_caller(&root, name)),
            "RUNTIME_WHITELIST entry {name} now has a static product caller - \
             the row is redundant, drop it"
        );
    }
}
