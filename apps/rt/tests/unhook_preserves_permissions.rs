// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Regression test for the harness kill-switch pair.
//!
//! `unhook` used to rename `.claude/settings.json` out of the way. That file
//! carries no `hooks` block — Mustard registers its hooks through
//! `plugin/hooks/hooks.json` — so the rename left every hook firing while
//! deleting what the file *does* carry: the developer's `permissions.deny`
//! safety net, `permissions.allow`, `statusLine` and the telemetry `env` block.
//!
//! These two tests pin both halves of the fix end-to-end through the public
//! command entry points: disabling must set `disableAllHooks` and keep the rest
//! of the file; restoring must remove exactly that key and keep the rest.

use std::path::Path;

use mustard_rt::commands::maint::rehook::{self, RehookOpts};
use mustard_rt::commands::maint::unhook::{self, UnhookOpts};
use serde_json::Value;

/// A settings.json shaped like the one Mustard installs: a deny list the
/// developer relies on, an allow list, a status line, and no `hooks` block.
const SEED: &str = r#"{
  "cleanupPeriodDays": 30,
  "permissions": {
    "allow": ["Read", "Edit", "Write"],
    "deny": [
      "Bash(rm -rf:*)",
      "Bash(git push --force:*)",
      "Read(**/*.pem)"
    ]
  },
  "statusLine": {
    "type": "command",
    "command": "mustard-rt run statusline",
    "padding": 1
  }
}
"#;

/// Write `SEED` (optionally already flagged) to `<project>/.claude/settings.json`.
fn seed_settings(project: &Path, disabled: bool) {
    let claude = project.join(".claude");
    std::fs::create_dir_all(&claude).unwrap();
    let mut root: Value = serde_json::from_str(SEED).unwrap();
    if disabled {
        root.as_object_mut()
            .unwrap()
            .insert("disableAllHooks".to_string(), Value::Bool(true));
    }
    std::fs::write(
        claude.join("settings.json"),
        serde_json::to_string_pretty(&root).unwrap(),
    )
    .unwrap();
}

/// Parse `<project>/.claude/settings.json`, asserting it is still there.
fn read_settings(project: &Path) -> Value {
    let path = project.join(".claude").join("settings.json");
    assert!(
        path.exists(),
        "settings.json must stay in place at {}",
        path.display()
    );
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// Assert the safety net and the status line came through untouched.
fn assert_safety_net_intact(root: &Value) {
    let deny = root["permissions"]["deny"]
        .as_array()
        .expect("permissions.deny survives the kill-switch");
    assert_eq!(deny.len(), 3, "every deny rule survives: {deny:?}");
    for rule in ["Bash(rm -rf:*)", "Bash(git push --force:*)", "Read(**/*.pem)"] {
        assert!(
            deny.iter().any(|v| v.as_str() == Some(rule)),
            "deny rule {rule} survives: {deny:?}"
        );
    }
    assert_eq!(
        root["permissions"]["allow"].as_array().map(Vec::len),
        Some(3),
        "permissions.allow survives"
    );
    assert_eq!(
        root["statusLine"]["command"].as_str(),
        Some("mustard-rt run statusline"),
        "statusLine survives"
    );
    assert_eq!(
        root["statusLine"]["padding"].as_i64(),
        Some(1),
        "statusLine siblings survive"
    );
    assert_eq!(
        root["cleanupPeriodDays"].as_i64(),
        Some(30),
        "unrelated top-level keys survive"
    );
}

#[test]
fn unhook_disables_hooks_without_dropping_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    seed_settings(project, /* disabled = */ false);
    // Volatile state the switch is still expected to wipe.
    std::fs::create_dir_all(project.join(".claude").join(".agent-state")).unwrap();

    unhook::run(UnhookOpts {
        repo: Some(project.to_path_buf()),
        scope: "this".to_string(),
        confirm: false,
    });

    let root = read_settings(project);
    assert_eq!(
        root["disableAllHooks"].as_bool(),
        Some(true),
        "the kill-switch must actually silence the hooks"
    );
    assert_safety_net_intact(&root);
    assert!(
        !project.join(".claude").join(".agent-state").exists(),
        "volatile harness state is still wiped"
    );
    // The rename is gone: nothing may be left behind under a .disabled name.
    let strays: Vec<String> = std::fs::read_dir(project.join(".claude"))
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("settings.json.disabled"))
        .collect();
    assert!(strays.is_empty(), "no snapshot is minted any more: {strays:?}");
}

#[test]
fn rehook_reenables_hooks_without_dropping_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    seed_settings(project, /* disabled = */ true);

    rehook::run(RehookOpts {
        repo: Some(project.to_path_buf()),
        scope: "this".to_string(),
        confirm: false,
    });

    let root = read_settings(project);
    assert!(
        root.get("disableAllHooks").is_none(),
        "the flag must be removed, not set to false: {root}"
    );
    assert_safety_net_intact(&root);
}
