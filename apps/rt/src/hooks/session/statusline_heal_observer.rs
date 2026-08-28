//! `statusline_heal_observer` — the `SessionStart` statusline self-heal module.
//!
//! ## Why
//!
//! Up to three copies of this binary can live on one machine: the SYSTEM copy
//! an installer put in `/usr/bin` (or `Program Files`), the PLUGIN copy inside
//! `~/.claude/plugins/cache/…` that Claude Code actually runs its hooks from,
//! and — on a developer's machine — a build sitting in a source clone. The
//! status bar renders `m{stamped}→{current}`, where `current` is the version of
//! whichever copy DRAWS it. So the copy recorded here decides whether the
//! operator can see the plugin fall behind at all.
//!
//! Two wrong answers have shipped. Both are on record, because each looks
//! obviously right until it is measured.
//!
//! **`std::env::current_exe()`** was the first. On the field machine of
//! 2026-08-28 a forgotten build inside a source clone
//! (`C:/atiz/mustard/plugin/bin/mustard-rt.exe`, version 0.1.47) ran once,
//! recorded its own path here, and from then on it WAS the binary the bar
//! started — so it re-recorded that same path, session after session, in a
//! directory no installer can reach. Three reinstalls of the `.exe` changed
//! nothing.
//!
//! **The bare token `mustard-rt`** was the second, on the belief that the
//! plugin prepends its own `bin/` to `PATH`. Measured 2026-08-28: Claude Code
//! APPENDS it — last of 21 entries — so the bare name resolves to the SYSTEM
//! copy. The bar would then report the system's own version, `stamped ==
//! current`, a plain green stamp, and the plugin-vs-system drift that started
//! the incident would never draw again. The bare token also collides with
//! `hook_resolve::rewrite_statusline_value`, which absolutises that exact
//! string because a launcher whose `PATH` omits the install dir loses the bar
//! entirely on Linux — and since `settings.local.json` outranks
//! `settings.json`, this observer would silently undo that fix every session.
//!
//! So the answer is a path, just never one this process INFERS. Claude Code's
//! own plugin registry (`~/.claude/plugins/installed_plugins.json`) records
//! where the plugin lives; it follows the plugin across updates and cannot name
//! a forgotten clone. That is what
//! [`mustard_core::installed_plugin_rt`] reads.
//!
//! ## Behaviour (all fail-open)
//!
//! 0. The registry names no plugin `mustard-rt` → no-op, whatever is recorded.
//!    Writing under that ignorance would be a guess, and a guess is what pinned
//!    the field machine.
//! 1. No `statusLine` key → install the canonical entry for the plugin's copy.
//! 2. `statusLine.command` names `mustard-rt` in any other shape — the bare
//!    token, another copy's path, a stale clone, a quoted path → rewrite it to
//!    the plugin's copy.
//! 3. `statusLine.command` is some other user command (no `mustard-rt` in it)
//!    → never touched.
//!
//! Every other key in the file (e.g. `enabledMcpjsonServers`) is preserved;
//! the merge is non-destructive and idempotent — when the command already
//! matches, nothing is written at all. A corrupt or unreadable file is left
//! alone rather than clobbered.
//!
//! ## Contract shape
//!
//! Pure side effect — no verdict. `StatuslineHealObserver` is an [`Observer`]
//! only: it never panics and never blocks the session.
//!
//! ## Platform note
//!
//! A past incident (SessionStart hang on Windows) was caused by child
//! processes inheriting the hook's stdio pipes. This observer spawns **no**
//! subprocess — pure file IO only. Keep it that way.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// The `SessionStart` statusline self-heal module.
pub struct StatuslineHealObserver;

/// The canonical statusline command for `rt`: forward slashes (project
/// convention for hook-written paths), quoted when the path contains a space so
/// the harness shell does not split it.
fn desired_command(rt: &Path) -> String {
    let rt = rt.to_string_lossy().replace('\\', "/");
    if rt.contains(' ') {
        format!("\"{rt}\" run statusline")
    } else {
        format!("{rt} run statusline")
    }
}

/// The canonical `statusLine` settings object for `rt`.
fn desired_statusline(rt: &Path) -> Value {
    json!({
        "type": "command",
        "command": desired_command(rt),
        "padding": 1,
    })
}

/// Heal `<root>/.claude/settings.local.json` so its `statusLine` points at the
/// plugin's `mustard-rt`. Inner, testable form of the observer — `plugin_rt` is
/// what the registry answered, so the case table can be exercised without a
/// registry on disk. See the module doc for that table. Fail-open at every
/// step: any read, parse, or write failure degrades to a no-op.
pub(crate) fn heal(root: &Path, plugin_rt: Option<&Path>) {
    // Case 0. Nothing the registry can name is nothing this observer may write:
    // the only other source of a path here is inference, which is the defect.
    let Some(plugin_rt) = plugin_rt else {
        return;
    };
    let Ok(paths) = ClaudePaths::for_project(root) else {
        return;
    };
    let settings_path = paths.claude_dir().join("settings.local.json");

    // Read fail-open. A file that exists but cannot be read or parsed as a
    // JSON object is left alone — never clobber what we cannot understand.
    let existing = match fs::read_to_string(&settings_path) {
        Ok(text) => Some(text),
        Err(_) if !fs::exists(&settings_path) => None,
        Err(_) => return,
    };
    let mut obj: Map<String, Value> = match existing.as_deref() {
        None => Map::new(),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => map,
            _ => return,
        },
    };

    let desired_cmd = desired_command(plugin_rt);
    match obj.get("statusLine") {
        // Case 1: no statusLine at all → install the canonical entry.
        None => {}
        Some(entry) => {
            let Some(cmd) = entry.get("command").and_then(Value::as_str) else {
                // A statusLine with no command string is not a shape we own —
                // leave the user's configuration alone.
                return;
            };
            if !cmd.contains("mustard-rt") {
                // Case 3: some other user command — respect the customization.
                return;
            }
            if cmd.replace('\\', "/") == desired_cmd {
                // Already the plugin's copy — idempotent no-op.
                return;
            }
            // Case 2: bare token / another copy / stale clone → rewrite.
        }
    }
    obj.insert("statusLine".to_string(), desired_statusline(plugin_rt));

    // Serialize with the workspace's stable key order (serde_json's default
    // sorted map) + trailing newline, and only write on a real change.
    let Ok(mut text) = serde_json::to_string_pretty(&Value::Object(obj)) else {
        return;
    };
    text.push('\n');
    if existing.as_deref() == Some(text.as_str()) {
        return;
    }
    let _ = fs::write_atomic(&settings_path, text.as_bytes());
}

impl Observer for StatuslineHealObserver {
    /// On `SessionStart`, heal the local statusline setting. Any other
    /// trigger is a no-op. Pure side effect — never panics, never blocks.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::SessionStart) {
            return;
        }
        let root = ctx
            .workspace_root
            .clone()
            .unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        heal(&root, mustard_core::installed_plugin_rt().as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// The plugin copy the registry would name. A real cache path, so a reader
    /// can see at a glance which of the three copies this is.
    fn fake_plugin_rt() -> PathBuf {
        PathBuf::from("/home/dev/.claude/plugins/cache/mustard-local/mustard/0.1.57/bin/mustard-rt")
    }

    fn plugin_command() -> String {
        format!("{} run statusline", fake_plugin_rt().to_string_lossy())
    }

    /// Create `<root>/.claude/` and return the settings.local.json path.
    fn seed_claude(root: &Path) -> PathBuf {
        let dir = root.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.local.json")
    }

    fn read_settings(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn absent_file_is_created_with_statusline_only() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        heal(dir.path(), Some(&fake_plugin_rt()));

        let obj = read_settings(&settings);
        let map = obj.as_object().expect("settings must be a JSON object");
        assert_eq!(map.len(), 1, "only statusLine may be introduced");
        assert_eq!(obj["statusLine"]["command"], plugin_command());
        assert_eq!(obj["statusLine"]["type"], "command");
        assert_eq!(obj["statusLine"]["padding"], 1);
        // Trailing newline, per the write convention.
        assert!(std::fs::read_to_string(&settings).unwrap().ends_with('\n'));
    }

    /// Case 0 — the lock on the rule that this module never guesses. Without
    /// it the obvious "fall back to current_exe" returns, and with it the 2026
    /// -08-28 clone-pinning incident returns too.
    #[test]
    fn no_plugin_in_the_registry_writes_nothing() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());

        heal(dir.path(), None);

        assert!(
            !settings.exists(),
            "with no plugin to name, the heal must write nothing at all"
        );
    }

    /// The measurement of 2026-08-28: Claude Code APPENDS the plugin's `bin/`
    /// to `PATH`, so the bare token resolves to the SYSTEM copy and the bar
    /// stops reporting the plugin. It is a shape to heal AWAY from, not to.
    #[test]
    fn bare_token_is_replaced_by_the_plugin_copy() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        std::fs::write(
            &settings,
            r#"{"statusLine":{"type":"command","command":"mustard-rt run statusline","padding":1}}"#,
        )
        .unwrap();

        heal(dir.path(), Some(&fake_plugin_rt()));

        let obj = read_settings(&settings);
        assert_eq!(obj["statusLine"]["command"], plugin_command());
    }

    #[test]
    fn stale_clone_path_is_replaced_and_other_keys_survive() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        std::fs::write(
            &settings,
            serde_json::to_string_pretty(&json!({
                "enabledMcpjsonServers": ["mustard-memory"],
                "statusLine": {
                    "type": "command",
                    "command": "C:/atiz/mustard/plugin/bin/mustard-rt.exe run statusline",
                    "padding": 1
                }
            }))
            .unwrap(),
        )
        .unwrap();

        heal(dir.path(), Some(&fake_plugin_rt()));

        let obj = read_settings(&settings);
        assert_eq!(obj["statusLine"]["command"], plugin_command());
        // The unrelated key is preserved with its exact value.
        assert_eq!(obj["enabledMcpjsonServers"], json!(["mustard-memory"]));
        assert_eq!(obj.as_object().map(Map::len), Some(2));
    }

    /// The exact shape the field machine of 2026-08-28 was pinned by: a
    /// quoted, backslashed path into a source clone. Quoting must not hide the
    /// `mustard-rt` token from the rewrite.
    #[test]
    fn quoted_windows_path_is_replaced() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        std::fs::write(
            &settings,
            r#"{"statusLine":{"type":"command","command":"\"C:\\Program Files\\Mustard\\mustard-rt.exe\" run statusline","padding":1}}"#,
        )
        .unwrap();

        heal(dir.path(), Some(&fake_plugin_rt()));

        let obj = read_settings(&settings);
        assert_eq!(obj["statusLine"]["command"], plugin_command());
    }

    /// A plugin cache under a Windows profile carries backslashes and can carry
    /// a space; the written command must be forward-slashed and quoted, or the
    /// harness shell splits it at the space.
    #[test]
    fn windows_plugin_path_is_forward_slashed_and_quoted() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        let rt = PathBuf::from("C:\\Users\\Ana Paula\\.claude\\plugins\\bin\\mustard-rt.exe");

        heal(dir.path(), Some(&rt));

        let obj = read_settings(&settings);
        assert_eq!(
            obj["statusLine"]["command"],
            "\"C:/Users/Ana Paula/.claude/plugins/bin/mustard-rt.exe\" run statusline"
        );
    }

    #[test]
    fn custom_non_mustard_statusline_is_untouched() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        let original = r#"{"statusLine":{"type":"command","command":"my-status --fast"}}"#;
        std::fs::write(&settings, original).unwrap();

        heal(dir.path(), Some(&fake_plugin_rt()));

        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            original,
            "a user statusline must be preserved byte-for-byte"
        );
    }

    #[test]
    fn correct_state_writes_nothing() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        // Compact formatting on purpose: a rewrite would re-serialize pretty,
        // so byte-equality proves no write happened — not merely an equal one.
        let original = format!(
            r#"{{"statusLine":{{"command":"{}","padding":1,"type":"command"}}}}"#,
            plugin_command()
        );
        std::fs::write(&settings, &original).unwrap();
        let mtime_before = std::fs::metadata(&settings).unwrap().modified().unwrap();

        heal(dir.path(), Some(&fake_plugin_rt()));

        assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
        let mtime_after = std::fs::metadata(&settings).unwrap().modified().unwrap();
        assert_eq!(mtime_before, mtime_after, "no write may touch the file");
    }

    #[test]
    fn corrupt_json_is_left_alone_without_panic() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        let original = "{not json at all";
        std::fs::write(&settings, original).unwrap();

        heal(dir.path(), Some(&fake_plugin_rt()));

        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            original,
            "a corrupt file must never be clobbered"
        );
    }

    #[test]
    fn missing_claude_dir_does_not_panic() {
        let dir = tempdir().unwrap();
        // No `.claude/` seeded — must not panic (write_atomic creates the
        // parent, which is acceptable; the invariant here is no panic).
        heal(dir.path(), Some(&fake_plugin_rt()));
    }

    // --- observer routing --------------------------------------------------

    fn ctx(dir: &str, trigger: Trigger) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(trigger),
            workspace_root: None,
            inject_only: None,
        }
    }

    #[test]
    fn non_session_start_trigger_is_noop() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        StatuslineHealObserver.observe(
            &HookInput::default(),
            &ctx(dir.path().to_str().unwrap(), Trigger::SessionEnd),
        );
        assert!(!settings.exists(), "SessionEnd must not heal anything");
    }

    /// Routing test, and the one invariant that holds whether or not the
    /// machine running the suite has a plugin registry: the path of the binary
    /// currently executing — here, the test harness — may never be recorded.
    /// Inferring the running executable IS the defect this module exists to
    /// prevent, so asserting its absence is stronger than asserting any
    /// particular value.
    #[test]
    fn session_start_never_records_the_running_binary() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        StatuslineHealObserver.observe(
            &HookInput::default(),
            &ctx(dir.path().to_str().unwrap(), Trigger::SessionStart),
        );

        let Ok(text) = std::fs::read_to_string(&settings) else {
            // No registry on this machine → case 0 → nothing written. Correct.
            return;
        };
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        assert!(
            !exe.is_empty() && !text.contains(&exe),
            "the running binary must never be recorded: {text}"
        );
    }
}
