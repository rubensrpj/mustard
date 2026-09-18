//! A ligação dos ganchos: cada comando `mustard-rt on <evento>` e
//! `mustard-rt run <comando>` que o `settings.json` cita tem de existir. Os
//! dois conjuntos de nomes válidos são lidos do que o binário entrega — os
//! eventos, do manifesto de ganchos embutido, que a porta publica; os
//! comandos, da árvore do clap — e nunca de uma lista mantida à mão.

use std::path::Path;

use mustard_core::io::fs;

use super::{known_hook_events, CheckResult};

/// All `mustard-rt run <subcommand>` names recognized by the binary — derived
/// from the live clap tree, so the set can never drift from `RunCmd` again.
fn known_run_subcommands() -> std::collections::BTreeSet<String> {
    <crate::commands::RunCmd as clap::Subcommand>::augment_subcommands(clap::Command::new("run"))
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect()
}

/// Parse `.claude/settings.json` and verify that every `mustard-rt on <event>`
/// and `mustard-rt run <cmd>` command string references a known event or
/// subcommand.
pub(super) fn check_wiring(claude_dir: &Path) -> CheckResult {
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::super::Status;
    use super::*;
    use crate::commands::doctor::doctor::tests::*;

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
}
