//! `secret_files` — the Rust layer of the secret-file law (`file-guard`).
//!
//! ONE behavior: a `PreToolUse(Read|Write|Edit)` gate that denies access to a
//! sensitive file (`credentials`, `*.pem`, `*.key`, `*.pfx`, `*.p12`,
//! `.git/config`, `id_rsa`, `id_ed25519`). It has no mode — always strict,
//! like `bash-safety`. 1:1 port of the historical `file-guard.js` semantics.
//!
//! The law lives in TWO layers, intentionally redundant:
//!
//! 1. **`settings.json permissions.deny`** — the 24 `Read`/`Edit`/`Write`
//!    globs (8 patterns × 3 tools, both template copies): the config-level
//!    first line, survives `/unhook`.
//! 2. **This residue** — the OLD semantics a permission glob cannot express:
//!    the match is CASE-INSENSITIVE (`Credentials.json`, `KEY.PEM`) and a
//!    SUBSTRING over the FULL forward-slash-normalised path, so a *directory*
//!    component trips it too (`config/credentials/prod.yaml`). Deny globs are
//!    case-sensitive and match path *shapes* (`**/*credentials*` only sees a
//!    basename segment), so they miss both spellings.

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;

/// The secret-file gate.
pub struct SecretFiles;

/// The `file-guard` law: deny a Read/Write/Edit on a sensitive file.
///
/// 1:1 with `file-guard.js`: only `Read`/`Write`/`Edit` tools are inspected;
/// the file path *and* its basename are tested against every pattern. A match
/// → `Deny`; otherwise `None`.
fn file_guard(input: &HookInput) -> Option<Verdict> {
    let tool = input.tool_name.as_deref().unwrap_or_default();
    if !matches!(tool, "Read" | "Write" | "Edit") {
        return None;
    }
    let file_path = input.file_path()?;
    let normalized = file_path.replace('\\', "/");
    let basename = normalized.rsplit('/').next().unwrap_or(&normalized);

    // The full path contains the basename, so one match covers both.
    let pattern = crate::shared::paths::sensitive_pattern(&normalized)?;
    Some(Verdict::Deny {
        reason: format!(
            "[file-guard] Access to sensitive file blocked: {basename}\n\
             Matched pattern: {pattern}"
        ),
    })
}

impl Check for SecretFiles {
    /// Always strict — a sensitive-file match denies, everything else allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        Ok(file_guard(input).unwrap_or(Verdict::Allow))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pre(tool: &str, file_path: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some(tool.to_string()),
            tool_input: json!({ "file_path": file_path }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PreToolUse));
        (input, ctx)
    }

    fn verdict_for(tool: &str, file_path: &str) -> Verdict {
        let (input, ctx) = pre(tool, file_path);
        SecretFiles.evaluate(&input, &ctx).expect("check never errors")
    }

    // --- file-guard parity (restored from the old `path_gate.rs`) -----

    #[test]
    fn file_guard_blocks_pem_key() {
        assert!(verdict_for("Read", "/project/secrets/server.pem").is_blocking());
        assert!(verdict_for("Write", "config/private.key").is_blocking());
    }

    #[test]
    fn file_guard_blocks_credentials() {
        assert!(verdict_for("Read", "/project/.aws/credentials").is_blocking());
    }

    #[test]
    fn file_guard_blocks_git_config_and_ssh_keys() {
        assert!(verdict_for("Edit", "/project/.git/config").is_blocking());
        assert!(verdict_for("Read", "/home/user/.ssh/id_rsa").is_blocking());
        assert!(verdict_for("Read", "/home/user/.ssh/id_ed25519").is_blocking());
    }

    #[test]
    fn file_guard_allows_env_files() {
        // file-guard does NOT block .env (user decision).
        assert_eq!(verdict_for("Read", "/project/.env"), Verdict::Allow);
        assert_eq!(verdict_for("Write", "/project/.env.local"), Verdict::Allow);
    }

    #[test]
    fn file_guard_allows_normal_source() {
        assert_eq!(verdict_for("Edit", "/project/src/main.ts"), Verdict::Allow);
    }

    #[test]
    fn file_guard_ignores_non_file_tools() {
        // Only Read/Write/Edit are inspected.
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "cat server.pem" }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        assert!(file_guard(&input).is_none());
    }

    #[test]
    fn file_guard_blocks_pfx_p12() {
        assert!(verdict_for("Read", "/p/cert.pfx").is_blocking());
        assert!(verdict_for("Read", "/p/cert.p12").is_blocking());
    }

    // --- the OLD semantics the deny globs cannot express ---------------------

    /// PROOF items: case-insensitive and directory-substring spellings — the
    /// ones the 24 `permissions.deny` globs miss (case-sensitive,
    /// basename-shaped) — must block again.
    #[test]
    fn file_guard_blocks_case_and_dir_substring_spellings() {
        assert!(verdict_for("Read", "x/Credentials.json").is_blocking());
        assert!(verdict_for("Read", "certs/KEY.PEM").is_blocking());
        assert!(verdict_for("Write", "config/credentials/prod.yaml").is_blocking());
        assert!(verdict_for("Edit", "backup/ID_RSA.bak").is_blocking());
    }

    /// Only PreToolUse gates; any other trigger self-allows.
    #[test]
    fn non_pre_tool_use_allows() {
        let (input, mut ctx) = pre("Read", "/p/cert.pem");
        ctx.trigger = Some(Trigger::PostToolUse);
        assert_eq!(
            SecretFiles.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }
}
