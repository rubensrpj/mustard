//! `statusline_heal_observer` — the `SessionStart` statusline self-heal module.
//!
//! ## Why
//!
//! The plugin-based install moved the `mustard-rt` binary into the plugin's
//! gitignored `plugin/bin/` directory, so any absolute binary path recorded in
//! settings goes stale whenever the binary moves (and is wrong on every other
//! machine). The law: versioned files never carry a machine-absolute path —
//! per-machine paths live in `.claude/settings.local.json` (gitignored). This
//! observer keeps that file's `statusLine` entry pointing at the *running*
//! binary on every `SessionStart`, healing stale paths without any installer
//! involvement.
//!
//! ## Behaviour (all fail-open)
//!
//! 1. No `statusLine` key → install the canonical
//!    `{"type":"command","command":"<exe> run statusline","padding":1}`.
//! 2. `statusLine.command` references a `mustard-rt` binary that differs from
//!    [`std::env::current_exe`] (stale path, moved binary, bare name) →
//!    rewrite it to the current exe.
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

/// The canonical statusline command for `exe`: forward slashes (project
/// convention for hook-written paths), quoted when the path contains a space
/// so the harness shell does not split it.
fn desired_command(exe: &Path) -> String {
    let exe = exe.to_string_lossy().replace('\\', "/");
    if exe.contains(' ') {
        format!("\"{exe}\" run statusline")
    } else {
        format!("{exe} run statusline")
    }
}

/// The canonical `statusLine` settings object for `exe`.
fn desired_statusline(exe: &Path) -> Value {
    json!({
        "type": "command",
        "command": desired_command(exe),
        "padding": 1,
    })
}

/// Heal `<root>/.claude/settings.local.json` so its `statusLine` points at
/// `current_exe`. Inner, testable form of the observer — see the module doc
/// for the case table. Fail-open at every step: any read, parse, or write
/// failure degrades to a no-op.
pub(crate) fn heal(root: &Path, current_exe: &Path) {
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

    let desired_cmd = desired_command(current_exe);
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
                // Already pointing at the running binary — idempotent no-op.
                return;
            }
            // Case 2: stale / moved / bare mustard-rt reference → rewrite.
        }
    }
    obj.insert("statusLine".to_string(), desired_statusline(current_exe));

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
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        heal(&root, &exe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Create `<root>/.claude/` and return the settings.local.json path.
    fn seed_claude(root: &Path) -> PathBuf {
        let dir = root.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.local.json")
    }

    /// A fake current-exe path. Windows-style separators on purpose: the
    /// backslashes are plain filename bytes on Unix, so the string-level
    /// normalization is exercised identically on both platforms.
    fn fake_exe() -> PathBuf {
        PathBuf::from("C:\\plugins\\mustard\\bin\\mustard-rt.exe")
    }

    fn read_settings(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn absent_file_is_created_with_statusline_only() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        heal(dir.path(), &fake_exe());

        let obj = read_settings(&settings);
        let map = obj.as_object().expect("settings must be a JSON object");
        assert_eq!(map.len(), 1, "only statusLine may be introduced");
        assert_eq!(
            obj["statusLine"]["command"],
            "C:/plugins/mustard/bin/mustard-rt.exe run statusline"
        );
        assert_eq!(obj["statusLine"]["type"], "command");
        assert_eq!(obj["statusLine"]["padding"], 1);
        // Trailing newline, per the write convention.
        assert!(std::fs::read_to_string(&settings).unwrap().ends_with('\n'));
    }

    #[test]
    fn stale_mustard_path_is_rewritten_and_other_keys_survive() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        std::fs::write(
            &settings,
            serde_json::to_string_pretty(&json!({
                "enabledMcpjsonServers": ["mustard-memory"],
                "statusLine": {
                    "type": "command",
                    "command": "C:/Users/ruben/.cargo/bin/mustard-rt.exe run statusline",
                    "padding": 1
                }
            }))
            .unwrap(),
        )
        .unwrap();

        heal(dir.path(), &fake_exe());

        let obj = read_settings(&settings);
        assert_eq!(
            obj["statusLine"]["command"],
            "C:/plugins/mustard/bin/mustard-rt.exe run statusline"
        );
        // The unrelated key is preserved with its exact value.
        assert_eq!(obj["enabledMcpjsonServers"], json!(["mustard-memory"]));
        assert_eq!(obj.as_object().map(Map::len), Some(2));
    }

    #[test]
    fn bare_mustard_rt_command_is_rewritten_to_absolute_exe() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        std::fs::write(
            &settings,
            r#"{"statusLine":{"type":"command","command":"mustard-rt run statusline","padding":1}}"#,
        )
        .unwrap();

        heal(dir.path(), &fake_exe());

        let obj = read_settings(&settings);
        assert_eq!(
            obj["statusLine"]["command"],
            "C:/plugins/mustard/bin/mustard-rt.exe run statusline"
        );
    }

    #[test]
    fn custom_non_mustard_statusline_is_untouched() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        let original = r#"{"statusLine":{"type":"command","command":"my-status --fast"}}"#;
        std::fs::write(&settings, original).unwrap();

        heal(dir.path(), &fake_exe());

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
        let original = r#"{"statusLine":{"command":"C:/plugins/mustard/bin/mustard-rt.exe run statusline","padding":1,"type":"command"}}"#;
        std::fs::write(&settings, original).unwrap();
        let mtime_before = std::fs::metadata(&settings).unwrap().modified().unwrap();

        heal(dir.path(), &fake_exe());

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

        heal(dir.path(), &fake_exe());

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
        heal(dir.path(), &fake_exe());
    }

    // --- observer routing --------------------------------------------------

    fn ctx(dir: &str, trigger: Trigger) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(trigger),
            workspace_root: None,
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

    #[test]
    fn session_start_heals_via_current_exe() {
        let dir = tempdir().unwrap();
        let settings = seed_claude(dir.path());
        StatuslineHealObserver.observe(
            &HookInput::default(),
            &ctx(dir.path().to_str().unwrap(), Trigger::SessionStart),
        );
        // current_exe here is the test binary — the exact path is irrelevant;
        // the shape of the installed entry is what matters.
        let obj = read_settings(&settings);
        let cmd = obj["statusLine"]["command"].as_str().unwrap();
        assert!(cmd.ends_with(" run statusline"), "got: {cmd}");
        assert!(!cmd.contains('\\'), "paths must be forward-slashed: {cmd}");
    }
}
