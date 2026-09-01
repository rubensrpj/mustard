//! Parity ratchet between the published `mustard-rt run` surface and every
//! product caller (templates, CLI sources, installer, packaging, command doc).
//!
//! Complements `run_command_surface.rs` (which locks the clap tree itself):
//!
//! - **FORWARD** — every `mustard-rt run <name>` a product file instructs must
//!   resolve to a registered subcommand, and every long flag typed on it must
//!   be one that command really declares. A template pointing at a name that no
//!   longer exists does not break the build — the command silently VANISHES at
//!   runtime; one typing a flag clap never registered dies with `error:
//!   unexpected argument` and exit 2. This walk turns both into a test failure.
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
        "diagnose-otel",
        "OTEL half of the consolidated doctor report \
         (commands/economy/otel/diagnose.rs); its only prose caller was the \
         `/maint doctor` section, dropped by the four-door surface prune - a \
         telemetry diagnostic with no scripted caller",
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
        "finding-collect",
        "deterministic seeder of meta.json#findings from the reviewer's files \
         and the ac-proof.json removal column \
         (commands/review/finding_collect.rs); consumed IN-PROCESS by the close \
         findings sub-gate (commands/pipeline/close_gates.rs: open_findings), \
         which re-collects on every CLOSE - the CLI face is the standalone \
         collection a reader takes before deciding",
    ),
    (
        "gate-regression-check",
        "regression-gate engine consumed in-process \
         (commands/agent/context_inject.rs build_vocab_matcher; \
         review_spans.rs parses its verdicts); the CLI face has no scripted \
         caller - flagged as dark surface in the F1 LOT C report",
    ),
    (
        "maint-deps",
        "user-invoked per-subproject dependency install \
         (commands/maint/maint_deps.rs); its only prose caller was the `/maint \
         deps` action, dropped by the four-door surface prune - a maintenance \
         escape hatch with no scripted caller",
    ),
    (
        "maint-validate",
        "user-invoked per-subproject build/type-check \
         (commands/maint/maint_validate.rs); its only prose caller was the \
         `/maint validate` action, dropped by the four-door surface prune - a \
         maintenance escape hatch with no scripted caller",
    ),
    (
        "mark-checklist-item",
        "instructed by the close-gate deny remediation \
         (commands/pipeline/close_gates.rs: mark each via mustard-rt run \
         mark-checklist-item)",
    ),
    (
        "mark-finding",
        "instructed by the close-gate deny remediation, once per open finding \
         (commands/pipeline/close_gates.rs: finding_refusal prints the exact \
         mustard-rt run mark-finding line that settles each one)",
    ),
    (
        "metrics",
        "user-invoked pipeline/hook metrics (collect + report faces, \
         commands/economy/); its only prose caller was the `/stats` door, \
         dropped by the four-door surface prune - the dashboard renders the \
         same `.metrics/` corpus through its own readers",
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
        "status",
        "user-invoked consolidated git/pipeline/harness report \
         (commands/pipeline/status.rs); its only prose caller was the \
         `/status` door, dropped by the four-door surface prune - an \
         observability escape hatch with no scripted caller",
    ),
];

/// Declared long flags that NO product prose spells, kept deliberately. Sorted
/// by `(command, flag)`; each justification says why a reader is never left
/// looking for this one.
///
/// The bar is not "it is minor". A flag reachable only by reading `--help` of a
/// command the docs never show is a feature that shipped to nobody, and the
/// honest fixes are to document it or to remove it. A row here says the flag is
/// reachable some OTHER way — it mirrors a documented sibling, it is the escape
/// hatch a refusal message prints, or it exists for a caller that is not prose.
const FLAG_WHITELIST: &[(&str, &str, &str)] = &[
    (
        "ac-negative-check",
        "from",
        "the revision the REMOVAL pass restores the work to; omitted it is the \
         merge base of HEAD and the primary integration base, which is what \
         every documented `--removal` invocation wants",
    ),
    (
        "adapt-cursor",
        "repo",
        "project-root override on a command RUNTIME_WHITELIST already records as \
         callerless - a path argument, not a behaviour",
    ),
    (
        "amend-finalize",
        "session-id",
        "the required argument of a command the SessionEnd hook runs in-process; \
         the CLI face exists for a CRASHED session, and its operator has the \
         session id in front of them",
    ),
    (
        "artifact-update",
        "manifest",
        "manifest path override, defaulting to `apps/cli/templates/.artifacts.json` \
         - the documented invocation is the default",
    ),
    (
        "base-candidates",
        "no-fetch",
        "opt-out of the `git fetch` the default performs; the flag's own help \
         states the reason to leave it alone - the whole point of the menu is \
         that it is true TODAY",
    ),
    (
        "capability",
        "status",
        "frontmatter `status` of a created capability doc; the subcommand help \
         spells the whole `create --slug X --title Y [--status active]` line, and \
         the default is what every caller wants",
    ),
    (
        "claude-dir-prune",
        "repo",
        "project-root override on a command RUNTIME_WHITELIST already records as \
         callerless",
    ),
    (
        "complete-spec",
        "archive-followups",
        "a declared NO-OP retained for compatibility: the single-stage close no \
         longer produces `closed-followup` specs, so there is nothing to sweep. \
         Prose naming it would teach a reader to pass a flag that does nothing",
    ),
    (
        "complete-spec",
        "archive-stale",
        "the same declared no-op as `--archive-followups`, kept for the same \
         compatibility reason",
    ),
    (
        "context-slice",
        "context-claude-md",
        "the slicer's SECOND input path; the CONTEXT.md slice is the documented \
         one and this adds a CLAUDE.md pass after it, described in the command's \
         own help",
    ),
    (
        "diagnose-otel",
        "expect-rows-after",
        "the wait window of a telemetry diagnostic RUNTIME_WHITELIST already \
         records as callerless - documenting the flag ahead of the command would \
         document a road to nowhere",
    ),
    (
        "docs-stale-check",
        "from",
        "narrows the audit to one spec's recorded audits; CLOSE gate 4 runs the \
         whole-repo default in-process",
    ),
    (
        "docs-stale-check",
        "include-nested",
        "opt-in to nested `.claude` installs, with an env twin \
         (`MUSTARD_DOCS_AUDIT_INCLUDE_NESTED`); the default - skip them - is what \
         the CLOSE gate runs",
    ),
    (
        "emit-phase",
        "from",
        "the optional prior phase; its help says it defaults to the spec's last \
         known phase, which is why every instructed invocation omits it",
    ),
    (
        "emit-pipeline",
        "allow-no-qa",
        "the escape hatch of the REVIEW/QA gate, for trusted callers like \
         `qa-run` itself. Prose that advertised it would advertise the way \
         AROUND the gate to exactly the reader the gate is for",
    ),
    (
        "gate-regression-check",
        "moment",
        "the 1/2/3 selector (default 1) of an engine consumed in-process by \
         commands/agent/context_inject.rs; RUNTIME_WHITELIST already records the \
         CLI face as callerless",
    ),
    (
        "gate-regression-check",
        "wave-dir",
        "the `--moment 3` companion on that same callerless CLI face; its help \
         carries the whole contract, exit code included",
    ),
    (
        "git-settle",
        "report",
        "the READING face of the exit ritual, settling nothing. The door prose \
         instructs the ritual; the report is what an operator runs to look first, \
         and the command's help describes it",
    ),
    (
        "mark-checklist-item",
        "cwd",
        "project-root override; the close-gate refusal that instructs this \
         command is read from the project root",
    ),
    (
        "mark-checklist-item",
        "item",
        "the refusal hands it over already filled in - close_gates.rs prints \
         `mark-checklist-item --spec {spec} --item <text>` per unchecked box, so \
         the reader meets the flag at the moment it is needed",
    ),
    (
        "mark-finding",
        "id",
        "same shape: `finding_refusal` prints `mark-finding --spec {spec} --id \
         {id} --to <dest> --reason <why>` once per open finding, with the id \
         already substituted",
    ),
    (
        "pipeline-summary",
        "self-test",
        "a self-check face whose only caller is an acceptance criterion; its help \
         carries the exact `cargo run` line to type",
    ),
    (
        "rehook",
        "repo",
        "project-root override on the harness re-enable door, which `/upsert --on` \
         runs from the project root",
    ),
    (
        "spec-draft",
        "output",
        "output directory override, defaulting to `.claude/spec/{slug}/` - the \
         layout every flow downstream assumes",
    ),
    (
        "spec-draft",
        "signals",
        "an optional free-form comma-separated list embedded in `spec.md` as a \
         comment; nothing reads it back, so there is no behaviour for prose to \
         describe",
    ),
    (
        "statusline",
        "preview",
        "renders every shipped theme on its own labelled line, for a human \
         picking one. `statusline` proper is wired by settings.json and reads its \
         payload from stdin",
    ),
    (
        "unhook",
        "repo",
        "project-root override on the harness disable door, which `/upsert --off` \
         runs from the project root",
    ),
    (
        "work-unit-open",
        "branch",
        "the alternative to the documented `--spec`/`--intent` pair, for a unit \
         whose branch already exists; the flag's help says exactly that",
    ),
    (
        "worktree-gc",
        "age-days",
        "the age threshold (default 7) of a sweep that is dry-run by default; the \
         command's help carries both numbers",
    ),
    (
        "worktree-gc",
        "repo",
        "project-root override; the instructed invocation runs from the project \
         root",
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

/// One `mustard-rt run …` instruction as a product file spells it.
struct RunInvocation {
    /// The registered subcommand name, after the two-token collapse.
    name: String,
    /// Every long flag typed on THAT invocation, without its `--`.
    flags: Vec<String>,
}

/// Extract every `run <name> [--flag …]` instruction reachable through one of
/// the [`CALLER_PREFIXES`], normalizing the two two-token rewrite forms
/// (`metrics wave-status` and `scan spec`, collapsed by `main.rs` argv
/// pre-routing) to their registered single-token names.
fn extract_run_invocations(text: &str) -> Vec<RunInvocation> {
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
            // Where THIS command's arguments begin — after the second token
            // when the two-token form was collapsed, so `scan spec --entity`
            // reads `--entity` as `scan-spec`'s and not as a stray word.
            let mut args_from = end;
            if end < bytes.len() && bytes[end] == b' ' {
                let second_start = end + 1;
                let mut second_end = second_start;
                while second_end < bytes.len() && is_token_byte(bytes[second_end]) {
                    second_end += 1;
                }
                if second_end > second_start && bytes[second_start].is_ascii_lowercase() {
                    match (first, &text[second_start..second_end]) {
                        ("metrics", "wave-status") => {
                            name = "metrics-wave-status".to_string();
                            args_from = second_end;
                        }
                        ("scan", "spec") => {
                            name = "scan-spec".to_string();
                            args_from = second_end;
                        }
                        _ => {}
                    }
                }
            }
            let flags = long_flags_of(&text[args_from..]);
            out.push(RunInvocation { name, flags });
        }
    }
    out
}

/// The long flags of ONE invocation, read from the text that follows its name.
///
/// The sweep stops at the first byte that cannot still belong to the same
/// command — a newline, a closing backtick, a pipe, a chain operator, a
/// redirection — so a second command sharing the line never lends its flags to
/// the first. Every slice boundary lands on an ASCII byte, so prose full of
/// em-dashes is walked without ever cutting a character in half.
fn long_flags_of(rest: &str) -> Vec<String> {
    let bytes = rest.as_bytes();
    let stop = bytes
        .iter()
        .position(|b| matches!(b, b'\n' | b'`' | b'|' | b'&' | b';' | b'<' | b'>'))
        .unwrap_or(bytes.len());
    scan_long_flags(&rest[..stop])
}

/// Every long flag spelled anywhere in a stretch of text.
///
/// Shared by both directions of the flag ratchet, deliberately: FORWARD asks
/// whether a flag typed on an invocation is declared, REVERSE asks whether a
/// declared flag is ever typed, and the two must never disagree about what
/// counts as "typed". A flag opens on a `--` that starts a token and is followed
/// by a lowercase letter, so a markdown `---` fence, an em-dash run and the
/// `--force-with-lease` inside `--no-force-with-lease` are all read the same way
/// here as they are there.
fn scan_long_flags(seg: &str) -> Vec<String> {
    let sb = seg.as_bytes();
    let mut flags = Vec::new();
    let mut i = 0;
    while i + 2 < sb.len() {
        let opens = sb[i] == b'-'
            && sb[i + 1] == b'-'
            && sb[i + 2].is_ascii_lowercase()
            && (i == 0 || !(sb[i - 1].is_ascii_alphanumeric() || sb[i - 1] == b'-'));
        if !opens {
            i += 1;
            continue;
        }
        let flag_start = i + 2;
        let mut flag_end = flag_start;
        while flag_end < sb.len() && is_token_byte(sb[flag_end]) {
            flag_end += 1;
        }
        flags.push(seg[flag_start..flag_end].to_string());
        i = flag_end;
    }
    flags
}

/// Every long flag ONE subcommand declares, minus the two clap generates on its
/// own. `--help` and `--version` are the runtime's, not the product's: no file
/// has to document them and no whitelist should have to excuse them.
fn declared_long_flags(cmd: &Command) -> BTreeSet<&str> {
    cmd.get_arguments()
        .filter_map(clap::Arg::get_long)
        .filter(|f| *f != "help" && *f != "version")
        .collect()
}

/// Every long flag spelled anywhere in the product corpus.
///
/// The whole corpus is read as ONE text, because the question is whether a
/// reader can find the flag at all — not which file happens to carry it.
fn spelled_long_flags(root: &Path) -> BTreeSet<String> {
    forward_corpus(root)
        .iter()
        .flat_map(|p| scan_long_flags(&read_lossy(p)))
        .collect()
}

/// The names half of [`extract_run_invocations`], for the callers that ask only
/// which commands a file instructs.
fn extract_run_names(text: &str) -> Vec<String> {
    extract_run_invocations(text).into_iter().map(|inv| inv.name).collect()
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
/// docs are not a caller. Dashboard backend (`apps/dashboard/server/src`)
/// sources count in full.
fn has_argv_caller(root: &Path, name: &str) -> bool {
    let needle = format!("\"run\", \"{name}\"");
    let own_module = format!("{}.rs", name.replace('-', "_"));

    let mut rt_sources = Vec::new();
    walk_files(&root.join("apps/rt/src"), &mut rt_sources);
    let mut dash_sources = Vec::new();
    walk_files(&root.join("apps/dashboard/server/src"), &mut dash_sources);

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

/// Every long flag a product file types on a `mustard-rt run <name>`
/// instruction must be one that command really declares.
///
/// The NAME half of this ratchet has always been checked; the flag half was
/// blind, and that blindness shipped: a reference `/git` orders read told the
/// agent to run `doctor --only branch-protection`, a flag clap answers with
/// `error: unexpected argument '--only' found` and exit 2. An instruction that
/// dies on its own arguments is exactly as broken as one naming a command that
/// does not exist, and nothing in the repository could tell.
///
/// An unregistered NAME is skipped here — that is the other test's finding, and
/// reporting it twice buries the flag it was asked about.
#[test]
fn forward_every_instructed_flag_is_declared() {
    let root = repo_root();
    let tree = run_command_tree();
    let mut offenders = Vec::new();
    for file in forward_corpus(&root) {
        let text = read_lossy(&file);
        for inv in extract_run_invocations(&text) {
            let Some(cmd) = tree.get_subcommands().find(|c| c.get_name() == inv.name) else {
                continue;
            };
            let declared: BTreeSet<&str> = cmd
                .get_arguments()
                .filter_map(clap::Arg::get_long)
                .chain(["help"])
                .collect();
            for flag in inv.flags {
                if !declared.contains(flag.as_str()) {
                    let shown = file.strip_prefix(&root).unwrap_or(&file);
                    offenders
                        .push(format!("{} -> `run {} --{flag}`", shown.display(), inv.name));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "product files type `mustard-rt run` flags the CLI does not declare - \
         the call aborts with `error: unexpected argument`, exit 2, before doing \
         anything. Fix the file or declare the flag:\n{}",
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

/// Every long flag the binary declares is spelled by some product prose, or
/// carries a justified [`FLAG_WHITELIST`] row.
///
/// The REVERSE of `forward_every_instructed_flag_is_declared`, and the half that
/// was blind. That one asks whether a flag a file TYPES exists; this one asks
/// whether a flag that EXISTS is ever typed. An undocumented flag does not
/// break — it simply never gets used, which is how `--explains-symptom`,
/// `--allow-no-qa` and `--no-fetch` could sit in the surface with no reader able
/// to learn they were there.
///
/// Citation is loose ON PURPOSE: the flag has to appear somewhere in the corpus,
/// not necessarily on its own command's invocation. Whether a flag is typed on
/// the RIGHT command is the forward test's question, and asking it twice would
/// report one defect as two. What this measures is narrower and it is the thing
/// nothing measured: can a reader find out the flag exists at all.
#[test]
fn reverse_every_declared_flag_is_documented() {
    let root = repo_root();
    let spelled = spelled_long_flags(&root);
    let tree = run_command_tree();
    let mut dark = Vec::new();
    for cmd in tree.get_subcommands() {
        let name = cmd.get_name();
        if name == "help" {
            continue;
        }
        for flag in declared_long_flags(cmd) {
            if spelled.contains(flag)
                || FLAG_WHITELIST.iter().any(|(c, f, _)| *c == name && *f == flag)
            {
                continue;
            }
            dark.push(format!("run {name} --{flag}"));
        }
    }
    assert!(
        dark.is_empty(),
        "declared `run` flags no product file ever spells - they ship, and the \
         only way to learn one exists is to read `--help` of a command the docs \
         do not show. Document the flag, drop it, or add a JUSTIFIED \
         FLAG_WHITELIST row:\n{}",
        dark.join("\n")
    );
}

/// The flag whitelist stays sorted, stays real, and stays necessary.
#[test]
fn flag_whitelist_stays_sorted_live_and_not_redundant() {
    let root = repo_root();
    let spelled = spelled_long_flags(&root);
    let tree = run_command_tree();

    for pair in FLAG_WHITELIST.windows(2) {
        assert!(
            (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1),
            "FLAG_WHITELIST must stay sorted: ({}, {}) before ({}, {})",
            pair[0].0,
            pair[0].1,
            pair[1].0,
            pair[1].1
        );
    }
    for (name, flag, justification) in FLAG_WHITELIST {
        let cmd = tree
            .get_subcommands()
            .find(|c| c.get_name() == *name)
            .unwrap_or_else(|| panic!("FLAG_WHITELIST names `run {name}`, which is not registered"));
        assert!(
            declared_long_flags(cmd).contains(*flag),
            "FLAG_WHITELIST names `run {name} --{flag}`, which that command no \
             longer declares - drop the row"
        );
        assert!(
            !justification.trim().is_empty(),
            "FLAG_WHITELIST entry `run {name} --{flag}` carries no justification"
        );
        assert!(
            !spelled.contains(*flag),
            "FLAG_WHITELIST entry `run {name} --{flag}` IS spelled in product \
             prose now - the row is redundant, drop it"
        );
    }
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
