//! `mustard-rt run upsert` — install or update Mustard in the current project.
//!
//! The plugin's bootstrap door: everything the harness needs in a project —
//! `.claude/settings.json`, the injectable instruction files under
//! `.claude/mustard/`, `.claude/.gitignore`, and the project-root
//! `mustard.json` — is seeded by `mustard_core::upsert_project`, idempotent
//! and always merge-mode (an existing user file is preserved; only what is
//! missing is created or backfilled). The legacy planted-orchestrator
//! footprint is migrated away in the same pass.
//!
//! Output: the serialized `UpsertReport` as pretty JSON — deterministic
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

use mustard_core::InstallMode;
use serde::Serialize;

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
pub(crate) struct PluginRefresh {
    /// [`REFRESHED`] or [`SKIPPED`] — the closed vocabulary of this field.
    pub state: &'static str,
    /// The `{plugin}@{marketplace}` id the refresh acted on. Absent when the
    /// registry named no install to act on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    /// The version the registry records AFTER the refresh — the one a restart
    /// would load. Absent when the refresh did not run, or when the registry
    /// could not be read back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Why the refresh did not run. Present exactly when `state` is
    /// [`SKIPPED`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    /// The restart sentence ([`RESTART_NOTICE`]). Present exactly when `state`
    /// is [`REFRESHED`] — there is nothing to restart FOR when nothing changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,
}

/// The whole answer of `run upsert`: what the engine did to the project, plus
/// what the plugin refresh did.
///
/// The engine's report is flattened, so every key callers already read
/// (`installedBefore`, `created`, `private`, …) keeps its name and its place;
/// `pluginRefresh` is appended after them.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    #[serde(flatten)]
    project: mustard_core::UpsertReport,
    plugin_refresh: PluginRefresh,
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
    let root = PathBuf::from(crate::shared::context::project_dir());

    // Unconditional. The mode is not read from anywhere and not asked for
    // anywhere: a harness that installs itself into someone else's repository
    // is the failure this door exists to make unreachable, and a knob that can
    // reach it is the same failure with an extra step.
    let mode = InstallMode::Private;

    let version = mustard_core::harness_version();
    match mustard_core::upsert_project(&root, Some(&version), mode) {
        Ok(report) => {
            // The refresh is the LAST step, and only on the path where the
            // project was really seeded: a run that wrote nothing has no
            // installation to finish.
            let outcome =
                Report { project: report, plugin_refresh: refresh_plugin(&root) };
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
/// `MUSTARD_CLAUDE_BIN`, mirroring `MUSTARD_RTK_BIN` in the rewrite gate.
fn refresh_plugin(root: &Path) -> PluginRefresh {
    let binary = std::env::var("MUSTARD_CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    let target = claude_config_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join(INSTALLED_PLUGINS)).ok())
        .and_then(|raw| refresh_target(&raw));
    fold_refresh(
        &binary,
        target,
        |command| run_step(command, root),
        mustard_core::installed_harness_version,
    )
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
            return skipped(Some(target.id), format!("`{command}` did not succeed: {reason}"));
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
        ShellOutcome::TimedOut { after } => Err(format!("timeout after {}ms", after.as_millis())),
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
    let flat = raw
        .split_whitespace()
        .map(|token| match token.rsplit_once('/') {
            // A leading `/` (or any embedded one) marks a path; keep the tail.
            Some((_, name)) if token.starts_with('/') && !name.is_empty() => name,
            _ => token,
        })
        .collect::<Vec<_>>()
        .join(" ");
    if flat.is_empty() {
        return "no output".to_string();
    }
    flat.chars().take(REASON_CHARS).collect()
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

    /// AC-1 — a refresh that ran carries BOTH halves of the answer: the version
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

    /// AC-2 — an absent or refusing `claude` leaves the upsert successful and
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

    /// A failed step's output becomes one bounded line — the report stays a
    /// report even when the host is chatty.
    #[test]
    fn excerpt_flattens_and_bounds_child_output() {
        assert_eq!(excerpt("  first line\n  second line \n"), "first line second line");
        assert_eq!(excerpt("   \n\t "), "no output");
        assert_eq!(excerpt(&"x".repeat(1000)).chars().count(), REASON_CHARS);
    }
}
