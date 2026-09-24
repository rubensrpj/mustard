//! `mustard-rt run upsert` — install or update Mustard in the current project.
//!
//! The plugin's bootstrap door: everything the harness needs in a project —
//! `.claude/settings.local.json`, Mustard's own texts (the session map under
//! `.claude/mustard/`, the two page templates under `.claude/mustard/pages/`
//! and the three agents under `.claude/agents/mustard/`),
//! `.claude/.gitignore`, and the project-root `mustard.json` — is seeded by
//! `mustard_core::upsert_project`, idempotently.
//! The settings file is the LOCAL one because the install is always
//! private-mode (see [`run`]); the shared `.claude/settings.json` is never
//! written here. What the OPERATOR owns is merge-only: an existing
//! `.claude/settings.local.json`, `.claude/.gitignore` or `mustard.json` is
//! preserved, and only what is missing is created or backfilled. Mustard's
//! own texts — `.claude/mustard/session-map.md`,
//! `.claude/mustard/pages/{spec,project}.html` and
//! `.claude/agents/mustard/{wave,review,skill}.md` — are ALWAYS rewritten, in
//! the language of `language.text`: they are the harness's own text, not
//! project configuration, so a copy that diverged is replaced and reported as
//! `Updated`, while a copy already byte-identical to the shipped text is
//! reported as `Preserved` because there was nothing left to write.
//! The local settings gain Mustard's own allow rules — its `mustard-rt run`
//! commands and `ArtifactData`, the tool that writes the database of the pages
//! it publishes — without touching the operator's rules.
//! An older install's map under its former name, `mapa-inicio-sessao.md`,
//! leaves the disk, and every declaration of it in `mustard.json#inject`, in
//! any spelling of the old path, is pointed at the new name with the rest of
//! the file untouched.
//!
//! What an older Mustard left in files that are not its own (the marks in the
//! `CLAUDE.md` files, the seed's lines in the team's `.claude/settings.json`,
//! a planted `.claude/CLAUDE.md`) is its own leftover, and leaves in this same
//! call, with no question: the rules of the Guards go first to the project's
//! pending list (`.claude/pending/ledger.json`), in one item with the text of
//! each rule and the file it left, then the lines leave. They never go to the
//! lesson bank, which stays on this machine and never goes to git: the person
//! turns each rule into a test or drops it. When that item cannot be written,
//! no file of the cleanup changes. `cleanup` lists what left and the files
//! without a mark, which are never touched; `cleaned` says what was done, with
//! the number of the pending item. Nothing is staged or committed.
//!
//! Once the files are written, the same code-tool step `mustard init` runs
//! (`mustard_core::platform::code_tools::ensure_code_tools`) sets up the
//! language-server program and plugin of every language the project involves,
//! so a project that only ever updates gets them too. What the step could not
//! do by itself comes back in `codeToolWarnings`, each sentence naming the
//! command a person runs to finish it; nothing it meets stops the upsert.
//!
//! Output: the serialized [`Report`] as pretty JSON — the engine's
//! `UpsertReport` flattened, with `pluginRefresh` and `codeToolWarnings`
//! appended — deterministic
//! (fixed field order, no timestamps, project-root-relative names only), per
//! the `run`-face byte-stability contract. Fail-open: an engine error is
//! reported as a JSON `{"error": …}` object and the process still exits 0.
//!
//! The footprint is ALWAYS the one that stays invisible to the host
//! repository's git. There is no flag and no mode to choose: an install that
//! versions the harness into a repository is not something this door can be
//! asked for, so no argv, no config and no forgotten default can produce one.
//!
//! The one loud failure: when a private install cannot hide itself in a
//! repository that exists, the engine writes NOTHING and the error is narrated
//! on stderr as well as reported in the JSON. Every other degradation here costs
//! a feature; that one costs the operator's belief that a client's git cannot
//! see the harness, and it is the failure they cannot notice for themselves.
//!
//! # The plugin refresh, and the half of it nobody can automate
//!
//! Seeding the project used to be the whole job, and it left the operator two
//! manual steps: update the plugin, then reload Claude Code. Those two are not
//! the same kind of thing, and treating them as one is what made the door end
//! halfway.
//!
//! UPDATING is a pair of commands the host already publishes —
//! `claude plugin marketplace update <marketplace>` then
//! `claude plugin update <plugin>` — so this command runs them as its last step
//! and reports the version the registry records afterwards.
//!
//! APPLYING is not reachable from here at all. `claude plugin update --help`
//! says `(restart required to apply)`: a session loads its plugin at start and
//! holds it until it ends, because the host owns that decision, not the plugin.
//! So the report carries the sentence instead of a promise — the running session
//! keeps the version it loaded, and only a restart picks up the new one.
//!
//! Every step of the refresh is reported, never fatal: this command's subject is
//! the PROJECT's installation, which does not depend on the state of the plugin.
//! A missing `claude`, a refusal, a stall, or a registry that lists no install of
//! this plugin all become a named `skipped` reason beside a successful upsert.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mustard_core::platform::code_tools::{self, MachineRunner, ToolRunner};
use mustard_core::platform::project_seed::{upsert_project_with, PendingList};
use mustard_core::InstallMode;
use serde::Serialize;

use crate::commands::event::pending::add_for_the_project;
use crate::shared::proc::{run_shell_with_deadline, ShellOutcome};

// The registry's path, the config dir it sits in and the key half this harness
// ships under all come from `mustard_core` — the crate that already reads this
// same file to answer "which version is installed". Copying them here made the
// two drift the day the host moves the file, and this side would degrade to a
// permanent skip nobody could see.
use mustard_core::{claude_config_dir, INSTALLED_PLUGINS, PLUGIN_NAME};

/// How long one refresh step may take. Both steps reach the network (the
/// marketplace update is a git fetch), so an unbounded wait would hang the
/// installation door on a stalled connection.
///
/// Two steps run, so this is HALF the budget the door has. The `/mustard:upsert`
/// prose calls this command from a Bash tool call whose own timeout the host
/// enforces; a per-step ceiling that let the pair outlast it would have the door
/// killed from outside, and then nothing reports at all — the module's own
/// deadline is the only one that can produce a `skipped` a person reads.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(45);

/// The two words `pluginRefresh.state` can carry. Both steps ran and were
/// accepted, or the refresh did not happen and says why.
const REFRESHED: &str = "refreshed";
const SKIPPED: &str = "skipped";

/// The half of "reload the plugin" that no code inside a session can perform.
/// Stated, never promised — see the module header.
const RESTART_NOTICE: &str = "This session keeps running the plugin version it loaded at start. \
     Only restarting Claude Code picks up the refreshed one — an upsert alone does not.";

/// How many characters of a failed step's output the reason carries. Enough to
/// name the refusal, short enough that the report stays a report.
const REASON_CHARS: usize = 300;

/// The plugin the refresh acts on, as Claude Code's registry records it.
///
/// Read rather than assumed: the marketplace half of the key and the install
/// scope are the operator's choices, and a refresh that guessed them would
/// update someone else's install or none at all.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RefreshTarget {
    /// The registry key — `{plugin}@{marketplace}`.
    id: String,
    /// The marketplace half, which `claude plugin marketplace update` names.
    marketplace: String,
    /// The scope the record was installed under (`user`, `project`, …).
    scope: String,
}

/// What the plugin refresh did, as the upsert report carries it.
///
/// Always present, because "the refresh did not run" is an answer the operator
/// needs as much as "it did" — the state before this field existed was a door
/// that neither updated nor said it had not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginRefresh {
    /// [`REFRESHED`] or [`SKIPPED`] — the closed vocabulary of this field.
    state: &'static str,
    /// The `{plugin}@{marketplace}` id the refresh acted on. Absent when the
    /// registry named no install to act on.
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin: Option<String>,
    /// The version the registry records AFTER the refresh — the one a restart
    /// would load. Absent when the refresh did not run, or when the registry
    /// could not be read back.
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    /// Why the refresh did not run. Present exactly when `state` is
    /// [`SKIPPED`].
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<String>,
    /// The restart sentence ([`RESTART_NOTICE`]). Present exactly when `state`
    /// is [`REFRESHED`] — there is nothing to restart FOR when nothing changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    restart: Option<String>,
}

/// The whole answer of `run upsert`: what the engine did to the project, what
/// the plugin refresh did, and what the code-tool step left for a person.
///
/// The engine's report is flattened, so every key callers already read
/// (`installedBefore`, `created`, `private`, …) keeps its name and its place;
/// `pluginRefresh` and `codeToolWarnings` are appended after them.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    #[serde(flatten)]
    project: mustard_core::UpsertReport,
    plugin_refresh: PluginRefresh,
    /// What the code-tool step could not do by itself, one sentence per
    /// warning, in the order the step met them, each naming the command a
    /// person runs to finish it. Always present: empty means every language
    /// the project involves has its program and its plugin, or that it
    /// involves none.
    code_tool_warnings: Vec<String>,
}

/// Execute `mustard-rt run upsert`.
///
/// The `mustard.json#version` stamp is [`mustard_core::harness_version`] —
/// the installed plugin's manifest version when launched by the plugin
/// (`CLAUDE_PLUGIN_ROOT`), the core crate's own version otherwise. The field
/// records "which harness last set this project up"; a legacy 3.1.x CLI stamp
/// reads as drift once and this very command realigns it.
pub fn run() {
    // Workspace-root walk first (an already-installed project resolves to its
    // anchor even from a subdirectory), then `CLAUDE_PROJECT_DIR`, then the
    // process cwd — the fresh-install path, where no anchor exists yet.
    let root = PathBuf::from(crate::shared::context::env::project_dir());

    // The code-tool commands find and spawn their programs against this
    // process's own PATH, the way `mustard init` hands the same step its own.
    let path_env = std::env::var("PATH").unwrap_or_default();
    match upsert(&root, &MachineRunner::new(&path_env), refresh_plugin) {
        Ok(outcome) => {
            let json = serde_json::to_string_pretty(&outcome)
                .unwrap_or_else(|e| format!("{{\"error\": \"serializing report: {e}\"}}"));
            println!("{json}");
        }
        Err(err) => {
            // Fail-open: report the failure as JSON, exit 0 (the run face
            // never signals through the exit code).
            let json = serde_json::json!({ "error": err.to_string() });
            println!("{json}");
            // One failure is not a machine's problem alone. A private install
            // that could not hide leaves the operator believing a client's git
            // cannot see the harness — the one thing they cannot check for
            // themselves — so it is narrated on stderr as well, where a person
            // reads. stdout stays the byte-stable JSON the run face contracts.
            if matches!(err, mustard_core::platform::error::Error::NotHidden(_)) {
                eprintln!(
                    "\n  NOTHING WAS INSTALLED.\n\
                     \n\
                     A private install hides its footprint in this clone's exclude file, and that\n\
                     file could not be used. Every file the install would seed — including the one\n\
                     naming the harness — would have been visible in this repository's `git status`\n\
                     while the report called itself private.\n\
                     \n\
                     Make the exclude file readable and writable (it must be a FILE), then re-run.\n"
                );
            }
        }
    }
}

/// The whole door, short of printing: seed the project, set up the code tools
/// of every language it involves, refresh the plugin.
///
/// `runner` runs the code-tool commands and `refresh` performs the plugin
/// refresh. [`run`] hands both to the machine; the tests hand a fake runner and
/// a canned refresh, so what they drive is the same sequence the command runs.
fn upsert(
    root: &Path,
    runner: &impl ToolRunner,
    refresh: impl FnOnce(&Path) -> PluginRefresh,
) -> mustard_core::platform::error::Result<Report> {
    // Unconditional. The mode is not read from anywhere and not asked for
    // anywhere: a harness that installs itself into someone else's repository
    // is the failure this door exists to make unreachable, and a knob that can
    // reach it is the same failure with an extra step.
    let mode = InstallMode::Private;

    let version = mustard_core::harness_version();
    let project = upsert_project_with(root, Some(&version), mode, &ProjectPending { root })?;

    // Both steps below run only on the path where the project was really
    // seeded: a run that wrote nothing has no installation to finish. The
    // code tools come after the files, so a step that fails or stalls leaves
    // the project already updated, and each failure is a sentence in the
    // report, never an abort.
    let code_tool_warnings =
        code_tools::ensure_code_tools(root, &mustard_core::io::project_map::model_path(root), runner)
            .iter()
            .map(ToString::to_string)
            .collect();

    // The refresh is the LAST step.
    Ok(Report { project, plugin_refresh: refresh(root), code_tool_warnings })
}

/// The project's pending list, the same `.claude/pending/ledger.json` that
/// `run pending` writes, as the cleanup of the install writes into it: one
/// item of the project, with the rules that leave the instruction files.
struct ProjectPending<'a> {
    root: &'a Path,
}

impl PendingList for ProjectPending<'_> {
    fn add(&self, title: &str, detail: &str) -> Result<String, String> {
        add_for_the_project(self.root, title, detail)
    }
}

// ---------------------------------------------------------------------------
// The plugin refresh
// ---------------------------------------------------------------------------

/// Refresh the installed plugin: read the registry, run the two host commands,
/// read the registry back.
///
/// The impure half. Everything that decides anything lives in
/// [`fold_refresh`], which is handed the runner and the version reader, so the
/// tests drive the whole decision without a `claude` on `PATH`.
///
/// The binary name defaults to `claude` and can be pointed elsewhere with
/// `MUSTARD_CLAUDE_BIN`, the way the rtk economy reader
/// (`packages/core/src/domain/economy/sources/rtk.rs`) takes `MUSTARD_RTK_BIN`.
fn refresh_plugin(root: &Path) -> PluginRefresh {
    let binary = std::env::var("MUSTARD_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    let target = claude_config_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join(INSTALLED_PLUGINS)).ok())
        .and_then(|raw| refresh_target(&raw));
    // The version reader is bound to the record this refresh ACTED ON — the
    // same key and the same scope. `installed_harness_version` answers a
    // different question ("what is installed anywhere"), so a leftover copy in
    // another scope would be reported as the result of an update that never
    // touched it.
    let acted_on = target.as_ref().map(|t| (t.id.clone(), t.scope.clone()));
    fold_refresh(&binary, target, |command| run_step(command, root), move || {
        let (id, scope) = acted_on?;
        let raw = std::fs::read_to_string(claude_config_dir()?.join(INSTALLED_PLUGINS)).ok()?;
        installed_version_of(&raw, &id, &scope)
    })
}

/// The version the registry records for ONE record — the `{plugin}@{marketplace}`
/// key under a named scope. `None` when the file, the key, the scope or the
/// field is absent; every one of those is a version this command cannot claim.
fn installed_version_of(raw: &str, id: &str, scope: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_str(raw).ok()?;
    doc.get("plugins")?
        .as_object()?
        .get(id)?
        .as_array()?
        .iter()
        .find(|record| {
            record.get("scope").and_then(serde_json::Value::as_str).unwrap_or("user") == scope
        })
        .and_then(|record| record.get("version").and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

/// The pure half of the refresh: a target, a runner for each step, and the
/// registry read that follows, folded into the reported field.
///
/// The two steps run in order — the marketplace first, because updating the
/// plugin from a clone that never fetched would reinstall the same version —
/// and the first refusal ends the sequence: the second step has nothing new to
/// install once the first did not land.
fn fold_refresh(
    binary: &str,
    target: Option<RefreshTarget>,
    mut step: impl FnMut(&str) -> Result<(), String>,
    installed_after: impl FnOnce() -> Option<String>,
) -> PluginRefresh {
    let Some(target) = target else {
        return skipped(
            None,
            "Claude Code's plugin registry lists no install of this plugin, so there is nothing \
             to update — installing it is a different flow, not this door's job."
                .to_string(),
        );
    };
    // The three values below are spliced into a shell command line. They come
    // from a JSON file this process does not own, so an id shaped like anything
    // other than a plugin id is refused rather than executed.
    if !is_plain_id(&target.id) || !is_plain_id(&target.marketplace) || !is_plain_id(&target.scope) {
        let id = &target.id;
        return skipped(
            None,
            format!(
                "the plugin registry names `{id}` (scope `{}`), which is not the shape of a \
                 plugin id this command will run",
                target.scope
            ),
        );
    }
    let steps = [
        format!("{binary} plugin marketplace update {}", target.marketplace),
        // `--yes` because stdout here is a pipe, not a TTY: the host refuses to
        // prompt when it cannot ask, and without the flag the step always fails.
        //
        // What it auto-accepts, said plainly: a marketplace may declare an
        // install command of its own, and the prompt is where a person would
        // approve running it. Passing `--yes` grants that approval to whatever
        // the marketplace declares. It is bounded by the trust already given —
        // the operator chose this marketplace and installed this plugin from it,
        // and the target comes from THEIR registry, not from us. This project's
        // own marketplace declares no such command (verified), so today the flag
        // only answers a question nobody would otherwise be asked.
        format!("{binary} plugin update {} --scope {} --yes", target.id, target.scope),
    ];
    for command in &steps {
        if let Err(reason) = step(command) {
            // The command is echoed through the same path-stripping the child's
            // own output goes through. `binary` is `claude` on every ordinary
            // run, but `MUSTARD_CLAUDE_BIN` may point anywhere — and a test
            // pointing it at a temporary directory put that directory's random
            // name straight into a `run`-face report the guard asks to stay
            // byte-stable.
            let quoted = excerpt(command);
            return skipped(Some(target.id), format!("`{quoted}` did not succeed: {reason}"));
        }
    }
    PluginRefresh {
        state: REFRESHED,
        plugin: Some(target.id),
        version: installed_after(),
        skipped: None,
        restart: Some(RESTART_NOTICE.to_string()),
    }
}

/// A refresh that did not happen, and the reason a person reads.
fn skipped(plugin: Option<String>, reason: String) -> PluginRefresh {
    PluginRefresh {
        state: SKIPPED,
        plugin,
        version: None,
        skipped: Some(reason),
        // Nothing changed on disk, so there is nothing a restart would apply.
        restart: None,
    }
}

/// Run one refresh step under [`REFRESH_TIMEOUT`]. `Ok` only on exit 0; every
/// other path — an absent binary, a refusal, a stall, a lost child — is an
/// `Err` carrying the excerpt the report names.
///
/// Shares the spawn/drain/deadline machinery with the verify and QA runners
/// ([`run_shell_with_deadline`]), including the concurrent pipe drain that
/// keeps a chatty child from deadlocking on a full OS pipe buffer.
fn run_step(command: &str, cwd: &Path) -> Result<(), String> {
    match run_shell_with_deadline(command, cwd, REFRESH_TIMEOUT) {
        ShellOutcome::Exited { status, stdout, stderr } => {
            if status.success() {
                return Ok(());
            }
            let combined = if stderr.trim().is_empty() { stdout } else { stderr };
            Err(excerpt(&combined))
        }
        // The DEADLINE, not the elapsed time. They differ by a few milliseconds
        // that change on every run, and this line lands in a `run`-face report
        // the guard asks to stay byte-stable. The ceiling is also the useful
        // number: it is the one a reader could raise.
        ShellOutcome::TimedOut { .. } => {
            Err(format!("timed out after {}s", REFRESH_TIMEOUT.as_secs()))
        }
        ShellOutcome::SpawnFailed { error } => Err(error),
    }
}

/// Collapse a child's output into one bounded line for the report, with every
/// absolute path reduced to its file name.
///
/// The `run` face owes byte-stable output, and a child's message routinely
/// carries the machine it ran on: measured here, a missing binary reported
/// `/tmp/tmp.fj9mbSeH9j/bin/nope-claude … not found`, whose middle segment is
/// different on every run. Keeping the NAME keeps the message useful — the
/// reader still learns which program was missing — while the volatile part
/// never reaches the report.
fn excerpt(raw: &str) -> String {
    let flat =
        raw.split_whitespace().map(shorten_paths).collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return "no output".to_string();
    }
    flat.chars().take(REASON_CHARS).collect()
}

/// Characters a child's message glues a path to. Quotes are the common case
/// (`cannot read '/tmp/…'`), but a stack frame writes `(/home/u/x.js:1:1)` and
/// a key-value line writes `installPath=/home/…`, and both would carry the
/// volatile directories straight through a boundary rule that only knew quotes.
const PATH_BOUNDARIES: [char; 8] = ['\'', '"', '`', '(', ')', ',', '=', ':'];

/// Replace every absolute path INSIDE one whitespace-free token with its file
/// name, punctuation and all.
///
/// Splitting on whitespace alone is not enough, and that was a real hole: a
/// child saying `cannot read '/tmp/tmp.YhSlRxVVl6/cache/plugin.json'` puts the
/// whole path in ONE token whose first character is a quote, so a rule keyed on
/// "the token starts with a slash" let it through untouched — into a `run`-face
/// report the guard asks to stay byte-stable. The punctuation in
/// [`PATH_BOUNDARIES`] is a boundary here for exactly that reason.
fn shorten_paths(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut piece = String::new();
    for ch in token.chars() {
        if PATH_BOUNDARIES.contains(&ch) {
            out.push_str(&file_name_if_absolute(&piece));
            piece.clear();
            out.push(ch);
        } else {
            piece.push(ch);
        }
    }
    out.push_str(&file_name_if_absolute(&piece));
    out
}

/// The last segment of an absolute path; anything else unchanged. Keeping the
/// NAME keeps the message useful — the reader still learns which file was
/// meant — while the directories, which differ on every machine and every run,
/// never reach the report.
fn file_name_if_absolute(piece: &str) -> String {
    if piece.starts_with('/') {
        return piece.rsplit('/').next().unwrap_or(piece).to_string();
    }
    piece.to_string()
}

/// Whether a registry-supplied token is a plain id — the only shape spliced
/// into a command line.
fn is_plain_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}

/// The install this refresh should act on, out of Claude Code's registry text.
///
/// The registry keys installs by `{plugin}@{marketplace}` and maps each to an
/// ARRAY — one record per scope. The highest version wins, matching what
/// `mustard_core::installed_harness_version_from` calls "installed": a
/// lower-scoped leftover must not be the one this command updates.
///
/// `None` when the text is not JSON, carries no `plugins` object, or lists no
/// install of this plugin — every one of which means "there is nothing here to
/// update", which is a reported skip and never an error.
fn refresh_target(raw: &str) -> Option<RefreshTarget> {
    let doc: serde_json::Value = serde_json::from_str(raw).ok()?;
    let plugins = doc.get("plugins")?.as_object()?;
    let mut best: Option<(String, RefreshTarget)> = None;
    for (key, records) in plugins {
        let Some((name, marketplace)) = key.split_once('@') else { continue };
        if name != PLUGIN_NAME || marketplace.is_empty() {
            continue;
        }
        for record in records.as_array().into_iter().flatten() {
            let version = record.get("version").and_then(serde_json::Value::as_str).unwrap_or("");
            if version.is_empty() {
                continue;
            }
            let scope =
                record.get("scope").and_then(serde_json::Value::as_str).unwrap_or("user");
            let higher = match &best {
                None => true,
                Some((current, _)) => mustard_core::is_behind(current, version),
            };
            if higher {
                best = Some((
                    version.to_string(),
                    RefreshTarget {
                        id: key.clone(),
                        marketplace: marketplace.to_string(),
                        scope: scope.to_string(),
                    },
                ));
            }
        }
    }
    best.map(|(_, target)| target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry shape Claude Code writes — one `{plugin}@{marketplace}` key
    /// mapping to an array of per-scope records.
    const REGISTRY: &str = r#"{
      "version": 2,
      "plugins": {
        "mustard@mustard-local": [
          { "scope": "user", "version": "0.1.42" }
        ],
        "rust-analyzer-lsp@claude-plugins-official": [
          { "scope": "user", "version": "1.0.0" }
        ]
      }
    }"#;

    /// A path INSIDE quotes is shortened too. The rule keyed on "the token
    /// starts with a slash" let this exact shape through into a `run`-face
    /// report the guard asks to stay byte-stable.
    #[test]
    fn a_quoted_absolute_path_is_shortened_too() {
        let line = excerpt("cannot read '/tmp/tmp.YhSlRxVVl6/cache/plugin.json'");
        assert_eq!(line, "cannot read 'plugin.json'", "the directories must not survive");
        assert!(!line.contains("tmp.YhSlRxVVl6"), "the volatile segment is gone: {line}");
    }

    /// A path glued to punctuation is shortened too — a stack frame and a
    /// key-value line both produce that shape, and neither is bounded by a
    /// quote.
    #[test]
    fn a_path_glued_to_punctuation_is_shortened_too() {
        assert_eq!(excerpt("at (/home/u/app/index.js:1:1)"), "at (index.js:1:1)");
        assert_eq!(excerpt("installPath=/home/u/.claude/x/plugin.json"), "installPath=plugin.json");
    }

    /// …and a plain word with a slash in it is left alone: only ABSOLUTE paths
    /// are volatile, and rewriting a relative one would lose real information.
    #[test]
    fn a_relative_path_is_left_intact() {
        assert_eq!(excerpt("see .claude/grain.model.json"), "see .claude/grain.model.json");
    }

    /// The reported version belongs to the record the refresh ACTED ON, never
    /// to whichever copy happens to be highest.
    ///
    /// A plugin can be installed under more than one scope, and only one of
    /// them was updated. Answering with the maximum across all of them would
    /// report a leftover in another scope as the result of this run — a number
    /// that is true about the machine and false about what just happened.
    #[test]
    fn the_reported_version_is_the_scope_that_was_updated() {
        const TWO_SCOPES: &str = r#"{
          "plugins": {
            "mustard@mustard-local": [
              { "scope": "user", "version": "0.1.43" },
              { "scope": "project", "version": "9.9.9" }
            ]
          }
        }"#;

        assert_eq!(
            installed_version_of(TWO_SCOPES, "mustard@mustard-local", "user").as_deref(),
            Some("0.1.43"),
            "the updated scope answers, not the higher leftover beside it",
        );
        assert_eq!(
            installed_version_of(TWO_SCOPES, "mustard@mustard-local", "absent"),
            None,
            "a scope the registry does not list is a version this command cannot claim",
        );
    }

    /// A refresh that ran carries BOTH halves of the answer: the version
    /// the registry now records, and the sentence saying this session is still
    /// on the one it loaded. Naming the version without the restart would read
    /// as a promise the host does not keep.
    #[test]
    fn a_successful_refresh_names_the_version_and_the_restart() {
        let mut ran: Vec<String> = Vec::new();
        let refresh = fold_refresh(
            "claude",
            refresh_target(REGISTRY),
            |command| {
                ran.push(command.to_string());
                Ok(())
            },
            || Some("0.1.43".to_string()),
        );

        assert_eq!(refresh.state, REFRESHED, "both steps were accepted: {refresh:?}");
        assert_eq!(
            refresh.version.as_deref(),
            Some("0.1.43"),
            "the report must carry the version the registry records after the refresh",
        );
        let restart = refresh.restart.as_deref().unwrap_or_default();
        assert!(
            restart.contains("restarting Claude Code"),
            "the report must say the running session keeps what it loaded: {restart}",
        );
        assert_eq!(refresh.skipped, None, "a refresh that ran has nothing to excuse");
        assert_eq!(refresh.plugin.as_deref(), Some("mustard@mustard-local"));

        // The marketplace is refreshed BEFORE the plugin — updating from a clone
        // that never fetched would reinstall the same version.
        assert_eq!(
            ran,
            vec![
                "claude plugin marketplace update mustard-local".to_string(),
                "claude plugin update mustard@mustard-local --scope user --yes".to_string(),
            ],
            "the two host commands, in order",
        );

        // Negative control: the same fold, with the first step refusing, must
        // NOT produce this state — otherwise every assertion above would pass
        // for a build that ignored its runner entirely.
        let refused = fold_refresh(
            "claude",
            refresh_target(REGISTRY),
            |_| Err("exit 1".to_string()),
            || Some("0.1.43".to_string()),
        );
        assert_eq!(refused.state, SKIPPED, "a refused step cannot report a refresh");
        assert_eq!(refused.version, None, "…and cannot name a resulting version");
    }

    /// An absent or refusing `claude` leaves the upsert successful and
    /// the report explaining itself. Two shapes of unavailable are covered: the
    /// binary that could not be spawned, and a registry that names no install.
    #[test]
    fn an_unavailable_cli_degrades_to_a_reported_skip() {
        // The binary is not there: the first step fails to spawn.
        let missing = fold_refresh(
            "claude",
            refresh_target(REGISTRY),
            |_| Err("No such file or directory (os error 2)".to_string()),
            || panic!("the version must not be read when the refresh did not run"),
        );
        assert_eq!(missing.state, SKIPPED);
        assert_eq!(missing.restart, None, "nothing changed, so nothing needs applying");
        let reason = missing.skipped.as_deref().unwrap_or_default();
        assert!(
            reason.contains("plugin marketplace update") && reason.contains("os error 2"),
            "the reason must name the command and what it answered: {reason}",
        );

        // No install to act on: the registry is readable and lists other
        // plugins, but none of this one.
        let elsewhere = fold_refresh(
            "claude",
            refresh_target(r#"{"plugins": {"other@market": [{"version": "1.0.0"}]}}"#),
            |_| panic!("no step may run when there is no install to update"),
            || panic!("the version must not be read when the refresh did not run"),
        );
        assert_eq!(elsewhere.state, SKIPPED);
        assert!(
            elsewhere.skipped.as_deref().unwrap_or_default().contains("nothing"),
            "the report must say WHY it did not run: {elsewhere:?}",
        );
        assert_eq!(elsewhere.plugin, None, "there was no plugin to name");

        // Negative control: the identical fold with a runner that accepts both
        // steps reports a refresh — so "skipped" above is a decision, not a
        // build that can only ever skip.
        let ok = fold_refresh("claude", refresh_target(REGISTRY), |_| Ok(()), || None);
        assert_eq!(ok.state, REFRESHED);
    }

    /// The target is READ, not assumed: the marketplace half and the scope come
    /// from the operator's own registry, and the highest version wins the way
    /// core's reader defines "installed".
    #[test]
    fn refresh_target_reads_marketplace_and_scope_from_the_registry() {
        let target = refresh_target(REGISTRY).expect("the registry lists this plugin");
        assert_eq!(target.id, "mustard@mustard-local");
        assert_eq!(target.marketplace, "mustard-local");
        assert_eq!(target.scope, "user");

        let two_scopes = r#"{"plugins": {"mustard@mustard": [
            {"scope": "project", "version": "0.1.9"},
            {"scope": "user", "version": "0.1.10"}
        ]}}"#;
        let winner = refresh_target(two_scopes).expect("both records name this plugin");
        assert_eq!(winner.scope, "user", "0.1.10 > 0.1.9 — a dotted compare, not a string one");

        assert_eq!(refresh_target("not json"), None);
        assert_eq!(refresh_target("{}"), None);
        assert_eq!(refresh_target(r#"{"plugins": {"mustard@": [{"version": "1.0.0"}]}}"#), None);
    }

    /// A registry value that is not a plain id never reaches a shell.
    #[test]
    fn a_shell_shaped_id_is_refused_instead_of_run() {
        let hostile = r#"{"plugins": {"mustard@evil; rm -rf /": [{"version": "1.0.0"}]}}"#;
        let refresh = fold_refresh(
            "claude",
            refresh_target(hostile),
            |_| panic!("a hostile id must never be executed"),
            || None,
        );
        assert_eq!(refresh.state, SKIPPED);
        assert!(is_plain_id("mustard@mustard-local"));
        assert!(!is_plain_id("mustard-local; echo hi"));
        assert!(!is_plain_id(""));
    }

    /// The refresh field rides the engine's report without renaming any key it
    /// already published, and the same input serializes byte-identically twice.
    #[test]
    fn the_outcome_appends_the_refresh_without_moving_anything() {
        let outcome = Report {
            project: mustard_core::UpsertReport {
                installed_before: true,
                version: Some("0.1.43".to_string()),
                ..mustard_core::UpsertReport::default()
            },
            plugin_refresh: skipped(None, "no install".to_string()),
            code_tool_warnings: Vec::new(),
        };
        let first = serde_json::to_string_pretty(&outcome).expect("serialize");
        let second = serde_json::to_string_pretty(&outcome).expect("serialize again");
        assert_eq!(first, second, "the run face contracts byte-stable output");

        let value: serde_json::Value = serde_json::from_str(&first).expect("valid JSON");
        assert_eq!(value["installedBefore"], serde_json::json!(true));
        assert_eq!(value["version"], serde_json::json!("0.1.43"));
        assert_eq!(value["pluginRefresh"]["state"], serde_json::json!(SKIPPED));
        assert!(value["pluginRefresh"].get("restart").is_none());
    }

    /// A runner that installs nothing: it writes down each command it is asked
    /// for and answers from memory. A present package manager puts on the PATH
    /// the program it brings, as the machine would; a command line containing
    /// one of `failing` exits with an error. It also notes whether the project
    /// was already seeded when each command came, so the order of the two
    /// steps is observable.
    struct FakeRunner {
        root: PathBuf,
        on_path: std::cell::RefCell<std::collections::BTreeSet<String>>,
        brings: Vec<(&'static str, &'static str)>,
        failing: Vec<&'static str>,
        log: std::cell::RefCell<Vec<String>>,
        seeded_when_called: std::cell::RefCell<Vec<bool>>,
    }

    impl FakeRunner {
        fn new(root: &Path, on_path: &[&str]) -> Self {
            Self {
                root: root.to_path_buf(),
                on_path: std::cell::RefCell::new(on_path.iter().map(|p| (*p).to_string()).collect()),
                brings: Vec::new(),
                failing: Vec::new(),
                log: std::cell::RefCell::new(Vec::new()),
                seeded_when_called: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl ToolRunner for FakeRunner {
        fn on_path(&self, program: &str) -> bool {
            self.on_path.borrow().contains(program)
        }

        fn run(&self, program: &str, args: &[&str]) -> bool {
            let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
            self.log.borrow_mut().push(line.clone());
            self.seeded_when_called.borrow_mut().push(self.root.join("mustard.json").is_file());
            if !self.on_path(program) || self.failing.iter().any(|f| line.contains(f)) {
                return false;
            }
            for (manager, brought) in &self.brings {
                if *manager == program {
                    self.on_path.borrow_mut().insert((*brought).to_string());
                }
            }
            true
        }

        fn found_off_path(&self, _program: &str) -> Option<PathBuf> {
            None
        }
    }

    /// A project in C# and Rust, updated: the files are written first, then the
    /// same code-tool step `mustard init` runs sets up each language. Each one
    /// lands on one side of the line: `csharp-ls` is missing and so is
    /// `dotnet`, so nothing installs and the warning names the command; the
    /// C# plugin install fails and becomes a warning with its command, and the
    /// step moves on; `rust-analyzer` is missing but `rustup` is there, so it
    /// runs. Both warnings reach the report the command prints, and the rest
    /// of the report — the files and the plugin refresh — is still there.
    #[test]
    fn a_atualizacao_roda_a_etapa_das_ferramentas_de_codigo() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("write Cargo.toml");
        std::fs::write(root.join("App.csproj"), "<Project/>\n").expect("write App.csproj");

        let mut runner = FakeRunner::new(root, &["rustup", "claude"]);
        runner.brings.push(("rustup", "rust-analyzer"));
        runner.failing.push("claude plugin install csharp-lsp@claude-plugins-official");

        let outcome = upsert(root, &runner, |_| skipped(None, "no registry in a test".to_string()))
            .expect("a plain project is seeded");

        assert_eq!(
            *runner.log.borrow(),
            vec![
                "claude plugin install csharp-lsp@claude-plugins-official",
                "claude plugin enable csharp-lsp@claude-plugins-official",
                "rustup component add rust-analyzer",
                "claude plugin install rust-analyzer-lsp@claude-plugins-official",
                "claude plugin enable rust-analyzer-lsp@claude-plugins-official",
            ],
            "each language gets its program and its plugin, and the C# failure does not stop Rust",
        );
        assert!(
            runner.seeded_when_called.borrow().iter().all(|seeded| *seeded),
            "every code-tool command runs after the project files are written: {:?}",
            runner.seeded_when_called.borrow(),
        );

        let first = serde_json::to_string_pretty(&outcome).expect("serialize");
        let second = serde_json::to_string_pretty(&outcome).expect("serialize again");
        assert_eq!(first, second, "the run face contracts byte-stable output");
        let value: serde_json::Value = serde_json::from_str(&first).expect("valid JSON");
        assert_eq!(
            value["codeToolWarnings"],
            serde_json::json!([
                "csharp: csharp-ls not found on PATH - install manually: dotnet tool install --global csharp-ls",
                "csharp: could not install the csharp-lsp@claude-plugins-official plugin - run manually: \
                 claude plugin install csharp-lsp@claude-plugins-official",
            ]),
            "each warning reaches the report with the command to run: {first}",
        );
        assert!(
            value["created"].as_array().is_some_and(|c| c.iter().any(|p| p == "mustard.json")),
            "the upsert itself went through: {first}",
        );
        assert_eq!(value["pluginRefresh"]["state"], serde_json::json!(SKIPPED), "{first}");
        assert!(root.join(".claude/settings.local.json").is_file(), "the files are on disk");
    }

    /// The code-tool step runs only once the project was seeded. When the
    /// seeding refuses — a repository whose exclude file cannot be written, so
    /// a private install cannot hide itself — the upsert answers with the error,
    /// and no code-tool command runs at all.
    #[test]
    #[cfg(unix)]
    fn a_atualizacao_roda_a_etapa_das_ferramentas_de_codigo_so_depois_dos_arquivos() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("write Cargo.toml");
        let init = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root)
            .status()
            .expect("run git init");
        assert!(init.success(), "git init");
        // Seal the directory the exclude file lives in: the write fails, while
        // the file itself stays readable to git.
        let info_dir = root.join(".git").join("info");
        std::fs::create_dir_all(&info_dir).expect("the info directory exists");
        if !info_dir.join("exclude").exists() {
            std::fs::write(info_dir.join("exclude"), "").expect("seed an empty exclude file");
        }
        std::fs::set_permissions(&info_dir, std::fs::Permissions::from_mode(0o555)).expect("seal");

        let runner = FakeRunner::new(root, &["rustup", "claude"]);
        let refused = upsert(root, &runner, |_| panic!("no refresh without a seeded project"));

        // Unseal before asserting, so the temp dir can always be removed.
        std::fs::set_permissions(&info_dir, std::fs::Permissions::from_mode(0o755)).expect("unseal");
        assert!(
            matches!(refused, Err(mustard_core::platform::error::Error::NotHidden(_))),
            "the seeding refused: {:?}",
            refused.as_ref().map(|_| "a report"),
        );
        assert!(runner.log.borrow().is_empty(), "no code-tool command may run: {:?}", runner.log.borrow());
        assert!(!root.join("mustard.json").exists(), "nothing was written");
    }

    /// Dois arquivos de instrução com o bloco de regras que um Mustard antigo
    /// escreveu: um é do time, com texto em volta; o outro é só do scan.
    fn lay_out_rules(root: &Path) -> (String, String) {
        let team = "# Api\n\nNossas regras ficam.\n\n## Guards\n\n<!-- mustard:guards -->\n\
                    - Reuse the shared client.\n<!-- /mustard:guards -->\n\nFim do time.\n";
        let only_ours = "@.claude/scan-map.md\n\n# Web\n\n## Guards\n\n<!-- mustard:guards -->\n\
                         - Never block the render.\n<!-- /mustard:guards -->\n";
        for (rel, body) in [("apps/api/CLAUDE.md", team), ("apps/web/CLAUDE.md", only_ours)] {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, body).expect("write the instruction file");
        }
        (team.to_string(), only_ours.to_string())
    }

    fn ledger_items(root: &Path) -> Vec<serde_json::Value> {
        let raw = std::fs::read_to_string(root.join(".claude/pending/ledger.json")).expect("the pending list");
        let ledger: serde_json::Value = serde_json::from_str(&raw).expect("the pending list is JSON");
        ledger["items"].as_array().cloned().unwrap_or_default()
    }

    /// A atualização tira o bloco de regras e cada regra vai, antes, a um
    /// item só da lista de pendências do projeto, com o texto dela e o
    /// arquivo de onde saiu. Nenhuma vai ao banco de lições.
    #[test]
    fn as_regras_do_bloco_vao_a_um_item_da_lista_de_pendencias_e_nao_ao_banco() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        lay_out_rules(root);

        let outcome = upsert(root, &FakeRunner::new(root, &[]), |_| skipped(None, "no registry in a test".to_string()))
            .expect("the project is seeded");
        let done = outcome.project.cleaned.clone().expect("the cleanup ran");
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(done.pending.as_deref(), Some("P-1"), "{done:?}");

        let items = ledger_items(root);
        assert_eq!(items.len(), 1, "um item só para todas as regras: {items:?}");
        assert_eq!(items[0]["id"], "P-1");
        assert_eq!(items[0]["status"], "open");
        let title = items[0]["title"].as_str().expect("title");
        assert!(title.contains("(2)"), "o título conta as regras: {title}");
        let detail = items[0]["detail"].as_str().expect("detail");
        for rule in ["Reuse the shared client. (saiu de apps/api/CLAUDE.md)", "Never block the render. (saiu de apps/web/CLAUDE.md)"] {
            assert!(detail.contains(rule), "cada regra com o arquivo de onde saiu: {rule} em {detail}");
        }

        assert!(!root.join(".claude/spec/lessons.ndjson").exists(), "nada vai ao banco de lições");
        let api = std::fs::read_to_string(root.join("apps/api/CLAUDE.md")).expect("the team file stays");
        assert!(!api.contains("mustard:guards") && !api.contains("Reuse the shared client."), "o bloco saiu: {api}");
        assert!(api.contains("Nossas regras ficam.") && api.contains("Fim do time."), "o texto do time fica: {api}");
        assert!(!root.join("apps/web/CLAUDE.md").exists(), "o arquivo que era só do scan saiu");

        // A segunda atualização não acha regra e não repete o item.
        let again = upsert(root, &FakeRunner::new(root, &[]), |_| skipped(None, "no registry in a test".to_string()))
            .expect("the second run");
        assert!(again.project.cleaned.is_none(), "{:?}", again.project.cleaned);
        assert_eq!(ledger_items(root).len(), 1, "a lista segue com um item");
    }

    /// Sem conseguir gravar a lista de pendências, nenhum arquivo muda: a
    /// regra nunca sai do arquivo sem ficar escrita em algum lugar.
    #[test]
    fn sem_gravar_a_lista_de_pendencias_nenhum_arquivo_muda() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let (team, only_ours) = lay_out_rules(root);
        // A lista não se grava: no lugar do arquivo dela há uma pasta.
        std::fs::create_dir_all(root.join(".claude/pending/ledger.json")).expect("block the pending list");

        let outcome = upsert(root, &FakeRunner::new(root, &[]), |_| skipped(None, "no registry in a test".to_string()))
            .expect("the seeding itself goes through");
        let done = outcome.project.cleaned.clone().expect("the cleanup says what it could not do");
        assert!(done.failed.iter().any(|why| why.starts_with("pending:")), "{done:?}");
        assert!(done.pending.is_none() && done.edited.is_empty() && done.deleted.is_empty(), "{done:?}");

        assert_eq!(std::fs::read_to_string(root.join("apps/api/CLAUDE.md")).expect("api"), team, "o arquivo do time fica igual");
        assert_eq!(std::fs::read_to_string(root.join("apps/web/CLAUDE.md")).expect("web"), only_ours, "o arquivo do scan fica");
        assert!(!root.join(".claude/spec/lessons.ndjson").exists(), "nada vai ao banco de lições");
    }

    /// A failed step's output becomes one bounded line — the report stays a
    /// report even when the host is chatty.
    #[test]
    fn excerpt_flattens_and_bounds_child_output() {
        assert_eq!(excerpt("  first line\n  second line \n"), "first line second line");
        assert_eq!(excerpt("   \n\t "), "no output");
        assert_eq!(excerpt(&"x".repeat(1000)).chars().count(), REASON_CHARS);
    }
}
