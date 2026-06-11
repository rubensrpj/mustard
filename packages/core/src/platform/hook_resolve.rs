//! Resolve the harness hook commands in a `.claude/settings.json` to an
//! absolute, `PATH`-independent invocation of the installed `mustard-rt`.
//!
//! ## The problem
//!
//! `templates/settings.json` ships every hook as the bare string
//! `rtk mustard-rt on <Event>`. `init`/`update` copy that file verbatim, so the
//! literal command lands in the project's `.claude/settings.json`. Both tokens
//! resolve through `PATH`: `rtk` (the token-economy filter) and `mustard-rt`
//! (the enforcement runtime). When the process that launches Claude Code has a
//! `PATH` that omits the install directory (`~/.cargo/bin`) — the common case
//! for a background / headless launcher — neither token resolves. The harness
//! fails open (the session is not broken) but every hook silently no-ops with
//! `Binary 'mustard-rt' not found on PATH`, disabling Mustard's enforcement for
//! that session.
//!
//! ## The fix (applied at install / rehook time, not in the versioned template)
//!
//! `init`/`update`/`rehook` run *as* — or right next to — the installed
//! `mustard-rt`, so [`std::env::current_exe`] yields the absolute path of the
//! binary that will serve the hooks. After the verbatim copy / snapshot restore
//! we rewrite each hook command to:
//!
//! 1. drop the `rtk` prefix — a hook's stdout is JSON consumed by the harness,
//!    not a human-read terminal, so RTK's filter does nothing useful and only
//!    adds a *second* `PATH` dependency; and
//! 2. replace the bare `mustard-rt` token with the absolute path.
//!
//! The result, e.g. `C:/Users/me/.cargo/bin/mustard-rt.exe on PreToolUse`,
//! resolves with an empty `PATH`. Backslashes are normalised to forward
//! slashes: hooks run through whatever shell Claude Code picked — on Windows
//! frequently Git Bash, which eats unquoted backslashes as escape characters
//! (`C:\Users\…` → `C:Users…`) — and forward slashes survive bash, cmd and
//! PowerShell alike while every Windows API accepts them. The versioned
//! template stays generic; the machine-specific resolution happens at install
//! time — the only moment that knows the machine.
//!
//! Lives in `platform` (not `domain/model/`) because [`rewrite_settings_hooks`]
//! touches disk through the [`crate::io::fs`] seam. The pure string/JSON
//! transform ([`rewrite_command`], [`rewrite_hooks_value`]) is split out so it
//! is unit-testable without a filesystem.
//!
//! Fail-open throughout: an unreadable / non-JSON `settings.json`, a missing
//! `hooks` block, or an unresolvable `current_exe()` all leave the file
//! untouched and return `Ok(0)` rather than erroring — a broken rewrite must
//! never block an install.

use std::path::{Path, PathBuf};

use crate::io::fs as mfs;
use crate::platform::error::{Error, Result};
use serde_json::Value;

/// Rewrite every `mustard-rt` invocation in `<claude_dir>/settings.json` to an
/// **absolute path**, dropping any `rtk` prefix. Covers both the `hooks` block
/// and the top-level `statusLine` command — both ship as bare, PATH-dependent
/// `mustard-rt …` tokens in the template and break identically when the
/// launcher's `PATH` omits the install directory.
///
/// `mustard_rt_exe` is the absolute path of the installed binary — normally the
/// result of [`resolve_mustard_rt`]. Returns the total number of commands
/// rewritten across both surfaces (`0` when there was nothing to do).
///
/// Fail-open: an absent or unreadable `settings.json`, malformed JSON, or no
/// rewritable command all return `Ok(0)` without touching disk.
pub fn rewrite_settings_hooks(claude_dir: &Path, mustard_rt_exe: &Path) -> Result<usize> {
    let settings_path = claude_dir.join("settings.json");
    let Ok(raw) = mfs::read_to_string(&settings_path) else {
        return Ok(0);
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&raw) else {
        return Ok(0);
    };

    // Forward slashes only: the hook command is executed by whatever shell
    // Claude Code picked (on Windows often Git Bash, which strips unquoted
    // backslashes as escape characters), and Windows accepts `/` everywhere.
    let abs = mustard_rt_exe.display().to_string().replace('\\', "/");
    let rewritten = rewrite_hooks_value(&mut root, &abs) + rewrite_statusline_value(&mut root, &abs);
    if rewritten == 0 {
        return Ok(0);
    }

    let mut serialized = serde_json::to_string_pretty(&root)
        .map_err(|e| Error::Parse(format!("serializing settings.json after hook rewrite: {e}")))?;
    serialized.push('\n');
    mfs::write_atomic(&settings_path, serialized.as_bytes())?;
    Ok(rewritten)
}

/// Walk the `hooks` block of a parsed `settings.json` value and rewrite each
/// hook `command` string in place. Returns the count of commands changed.
///
/// Pure (no IO) so it is unit-testable without a filesystem. The shape is the
/// Claude Code hooks schema: `hooks` → event name → array of blocks → each
/// block has a `hooks` array of `{ "type": "command", "command": "..." }`.
pub fn rewrite_hooks_value(root: &mut Value, mustard_rt_abs: &str) -> usize {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut count = 0usize;
    for (_event, blocks) in hooks.iter_mut() {
        let Some(blocks) = blocks.as_array_mut() else { continue };
        for block in blocks.iter_mut() {
            let Some(inner) = block.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            for entry in inner.iter_mut() {
                let Some(command) = entry.get("command").and_then(Value::as_str) else {
                    continue;
                };
                if let Some(next) = rewrite_command(command, mustard_rt_abs) {
                    if next != command {
                        entry["command"] = Value::String(next);
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

/// Rewrite the top-level `statusLine.command` of a parsed `settings.json` value
/// in place. Returns `1` when it was a Mustard command and changed, else `0`.
///
/// Same root cause as the hooks: the template ships
/// `statusLine.command = "mustard-rt run statusline"`, a bare PATH-dependent
/// token, so a launcher whose `PATH` omits the install dir silently loses the
/// status line (observed on Linux). The fix reuses [`rewrite_command`] — the
/// `mustard-rt` token is absolutised (and a stray `rtk` dropped); the
/// `run statusline` tail is preserved verbatim. A non-Mustard or absent
/// `statusLine` is left untouched.
pub fn rewrite_statusline_value(root: &mut Value, mustard_rt_abs: &str) -> usize {
    // Copy the command out first so the immutable borrow of `root` ends before
    // the in-place write below.
    let Some(command) = root
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return 0;
    };
    match rewrite_command(&command, mustard_rt_abs) {
        Some(next) if next != command => {
            if let Some(status) = root.get_mut("statusLine").and_then(Value::as_object_mut) {
                status.insert("command".to_string(), Value::String(next));
                return 1;
            }
            0
        }
        _ => 0,
    }
}

/// Rewrite a single hook command string: drop a leading `rtk` token and replace
/// the bare `mustard-rt` token with `mustard_rt_abs` (quoted when it contains a
/// space — Windows install paths routinely do). Returns `None` when the command
/// is not a Mustard hook invocation (left untouched by the caller).
///
/// Only the first `mustard-rt` token is replaced; the rest of the command
/// (`on PreToolUse`, `check bash_command_gate`, …) is preserved verbatim.
pub fn rewrite_command(command: &str, mustard_rt_abs: &str) -> Option<String> {
    // Peel off the leading executable token, quote-aware: a prior rewrite may
    // have produced a quoted absolute path containing spaces (`"C:\Program
    // Files\…\mustard-rt.exe" on Stop`), which naive whitespace splitting would
    // shred. Returns `(head, rest)` where `rest` is the untouched tail.
    let (head, rest) = split_head(command.trim())?;

    // Drop a leading `rtk` wrapper — the hook output is machine-consumed JSON,
    // so the filter is dead weight and a redundant PATH dependency. The real
    // binary token then follows.
    let (bin, rest) = if head == "rtk" {
        split_head(rest)?
    } else {
        (head, rest)
    };

    // `bin` must denote the `mustard-rt` binary (bare or an already-absolute
    // path from a prior rewrite). Bail otherwise — never touch a command we do
    // not own.
    if !is_mustard_rt_token(bin) {
        return None;
    }

    let quoted = if mustard_rt_abs.contains(' ') {
        format!("\"{mustard_rt_abs}\"")
    } else {
        mustard_rt_abs.to_string()
    };
    let rest = rest.trim();
    let out = if rest.is_empty() {
        quoted
    } else {
        format!("{quoted} {rest}")
    };
    Some(out)
}

/// Split `s` into its first token and the untrimmed remainder. The first token
/// is quote-aware: a leading `"` runs to the next `"` (so a quoted path with
/// spaces is one token, the closing quote included); otherwise the token runs
/// to the first whitespace. Returns `None` for an empty / whitespace-only input.
fn split_head(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    if let Some(after_open) = s.strip_prefix('"') {
        if let Some(close_rel) = after_open.find('"') {
            // Head includes both quotes: `s[0..=close_abs]`.
            let head_end = close_rel + 2; // opening quote + content + closing quote
            return Some((&s[..head_end], &s[head_end..]));
        }
        // Unbalanced quote — treat the whole string as the head.
        return Some((s, ""));
    }
    match s.find(char::is_whitespace) {
        Some(i) => Some((&s[..i], &s[i..])),
        None => Some((s, "")),
    }
}

/// `true` when `token` denotes the `mustard-rt` binary: the bare name, or any
/// path ending in `mustard-rt` / `mustard-rt.exe` (the form left by a previous
/// rewrite — re-running `init`/`rehook` must be idempotent). A leading quote is
/// tolerated so an already-quoted absolute path is recognised.
fn is_mustard_rt_token(token: &str) -> bool {
    let t = token.trim_matches('"');
    let stem = t
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(t)
        .trim_end_matches(".exe");
    stem == "mustard-rt"
}

/// The absolute path of the installed `mustard-rt`, for [`rewrite_settings_hooks`].
///
/// The caller (`mustard init`/`update`, or `mustard-rt run rehook`) is itself an
/// installed binary that sits in the same directory as `mustard-rt`
/// (`~/.cargo/bin`, a Scoop shims dir, the packaged `bin/`, …). We take the
/// directory of [`std::env::current_exe`] and join the platform `mustard-rt`
/// filename. When `current_exe()` *is* `mustard-rt` (the `rehook` case) this
/// round-trips to the same path.
///
/// Returns `None` only when `current_exe()` cannot be resolved at all — the
/// caller then skips the rewrite and the template's bare `mustard-rt` token
/// survives (the pre-fix behavior), so this stays fail-open.
pub fn resolve_mustard_rt() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let filename = if cfg!(windows) { "mustard-rt.exe" } else { "mustard-rt" };
    // Prefer the sibling when it exists (the install layout); otherwise return
    // it anyway — the directory is the install dir and the binary is expected
    // there. An absolute path that does not yet resolve still beats a bare
    // PATH-dependent token.
    Some(dir.join(filename))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drops_rtk_and_absolutizes_mustard_rt() {
        let out =
            rewrite_command("rtk mustard-rt on PreToolUse", "/home/u/.cargo/bin/mustard-rt")
                .expect("a Mustard hook command must rewrite");
        assert_eq!(out, "/home/u/.cargo/bin/mustard-rt on PreToolUse");
    }

    #[test]
    fn quotes_paths_with_spaces() {
        let out = rewrite_command(
            "rtk mustard-rt on SessionStart",
            "C:\\Program Files\\mustard\\mustard-rt.exe",
        )
        .unwrap();
        assert_eq!(
            out,
            "\"C:\\Program Files\\mustard\\mustard-rt.exe\" on SessionStart"
        );
    }

    #[test]
    fn preserves_check_subcommand_args() {
        let out =
            rewrite_command("rtk mustard-rt check bash_command_gate", "/bin/mustard-rt").unwrap();
        assert_eq!(out, "/bin/mustard-rt check bash_command_gate");
    }

    #[test]
    fn works_without_a_leading_rtk() {
        let out = rewrite_command("mustard-rt on PostToolUse", "/bin/mustard-rt").unwrap();
        assert_eq!(out, "/bin/mustard-rt on PostToolUse");
    }

    #[test]
    fn idempotent_on_an_already_absolute_command() {
        let once = rewrite_command("rtk mustard-rt on Stop", "/bin/mustard-rt").unwrap();
        let twice = rewrite_command(&once, "/bin/mustard-rt").unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn idempotent_with_quoted_path() {
        let abs = "C:\\Program Files\\m\\mustard-rt.exe";
        let once = rewrite_command("rtk mustard-rt on Stop", abs).unwrap();
        let twice = rewrite_command(&once, abs).unwrap();
        assert_eq!(once, twice, "re-running must be stable for quoted paths");
    }

    #[test]
    fn leaves_foreign_commands_untouched() {
        assert!(rewrite_command("rtk git status", "/bin/mustard-rt").is_none());
        assert!(rewrite_command("some-other-tool run x", "/bin/mustard-rt").is_none());
    }

    #[test]
    fn walks_the_full_hooks_block() {
        let mut root = json!({
            "hooks": {
                "PreToolUse": [
                    { "matcher": ".*", "hooks": [
                        { "type": "command", "command": "rtk mustard-rt on PreToolUse" }
                    ] }
                ],
                "SessionStart": [
                    { "matcher": "startup", "hooks": [
                        { "type": "command", "command": "rtk mustard-rt on SessionStart" }
                    ] }
                ]
            },
            "statusLine": { "type": "command", "command": "mustard-rt run statusline" }
        });

        let n = rewrite_hooks_value(&mut root, "/abs/mustard-rt");
        assert_eq!(n, 2, "both hook commands rewritten");
        assert_eq!(
            root["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
                .as_str()
                .unwrap(),
            "/abs/mustard-rt on PreToolUse"
        );
        // statusLine is outside `hooks` and left untouched by the hooks-only walk.
        assert_eq!(
            root["statusLine"]["command"].as_str().unwrap(),
            "mustard-rt run statusline"
        );
    }

    #[test]
    fn rewrite_settings_hooks_is_fail_open_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let n = rewrite_settings_hooks(dir.path(), Path::new("/abs/mustard-rt")).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn rewrite_settings_hooks_rewrites_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{ "hooks": { "PreToolUse": [ { "matcher": ".*", "hooks": [ { "type": "command", "command": "rtk mustard-rt on PreToolUse" } ] } ] } }"#,
        )
        .unwrap();

        let n = rewrite_settings_hooks(dir.path(), Path::new("/abs/mustard-rt")).unwrap();
        assert_eq!(n, 1);

        let written = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert!(
            written.contains("/abs/mustard-rt on PreToolUse"),
            "absolute command persisted: {written}"
        );
        assert!(
            !written.contains("rtk mustard-rt"),
            "rtk prefix dropped: {written}"
        );
    }

    #[test]
    fn windows_backslashes_become_forward_slashes_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{ "hooks": { "PreToolUse": [ { "matcher": ".*", "hooks": [ { "type": "command", "command": "rtk mustard-rt on PreToolUse" } ] } ] } }"#,
        )
        .unwrap();

        let n = rewrite_settings_hooks(
            dir.path(),
            Path::new(r"C:\Users\u\.cargo\bin\mustard-rt.exe"),
        )
        .unwrap();
        assert_eq!(n, 1);

        let written = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert!(
            written.contains("C:/Users/u/.cargo/bin/mustard-rt.exe on PreToolUse"),
            "backslashes normalised so Git Bash does not eat them: {written}"
        );
        assert!(
            !written.contains(r"C:\\Users"),
            "no backslash form survives: {written}"
        );
    }

    #[test]
    fn rewrites_statusline_command() {
        let mut root = json!({
            "statusLine": { "type": "command", "command": "mustard-rt run statusline", "padding": 1 }
        });
        let n = rewrite_statusline_value(&mut root, "/home/u/.cargo/bin/mustard-rt");
        assert_eq!(n, 1);
        assert_eq!(
            root["statusLine"]["command"].as_str().unwrap(),
            "/home/u/.cargo/bin/mustard-rt run statusline"
        );
        // Sibling fields of the statusLine object are preserved.
        assert_eq!(root["statusLine"]["type"].as_str().unwrap(), "command");
        assert_eq!(root["statusLine"]["padding"].as_i64().unwrap(), 1);
    }

    #[test]
    fn statusline_absent_or_foreign_is_noop() {
        let mut absent = json!({ "hooks": {} });
        assert_eq!(rewrite_statusline_value(&mut absent, "/bin/mustard-rt"), 0);

        let mut foreign = json!({
            "statusLine": { "type": "command", "command": "starship prompt" }
        });
        assert_eq!(rewrite_statusline_value(&mut foreign, "/bin/mustard-rt"), 0);
        assert_eq!(
            foreign["statusLine"]["command"].as_str().unwrap(),
            "starship prompt",
            "a non-Mustard status line is left untouched"
        );
    }

    #[test]
    fn rewrite_settings_hooks_also_rewrites_statusline() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{ "hooks": { "PreToolUse": [ { "matcher": ".*", "hooks": [ { "type": "command", "command": "rtk mustard-rt on PreToolUse" } ] } ] }, "statusLine": { "type": "command", "command": "mustard-rt run statusline", "padding": 1 } }"#,
        )
        .unwrap();

        let n = rewrite_settings_hooks(dir.path(), Path::new("/abs/mustard-rt")).unwrap();
        assert_eq!(n, 2, "one hook + one statusLine command rewritten");

        let written = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert!(written.contains("/abs/mustard-rt on PreToolUse"), "hook absolutised: {written}");
        assert!(written.contains("/abs/mustard-rt run statusline"), "statusline absolutised: {written}");
        assert!(!written.contains("rtk mustard-rt"), "rtk prefix dropped: {written}");
    }
}
