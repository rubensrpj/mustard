//! `mustard-rt on WorktreeCreate` — the isolation-event handler whose contract
//! is UNLIKE every other hook: when configured it REPLACES Claude Code's
//! native `git worktree add`, stdout must carry the created path (never the
//! `hookSpecificOutput` JSON), and a non-zero exit ABORTS the creation with
//! stderr shown to the user. That abort-by-exit is the one deliberate
//! exception to the crate's exit-0 fail-open rule — it IS this event's
//! protocol, exactly like a `Deny` is a gate's.
//!
//! The engine lives in [`crate::commands::work_unit_open::hook_create`] (same
//! motor as `mustard-rt run work-unit-open`): `{base}_{slug}` names cut from a
//! fresh `origin/{base}`; non-unit names (`agent-*`, desktop) replicate the
//! native default cut so background isolation never breaks.

use mustard_core::domain::model::contract::HookInput;
use std::path::PathBuf;

/// Handle one `WorktreeCreate` invocation and exit per the event contract.
pub fn run(input: &HookInput) -> ! {
    let Some(requested) = input.worktree_path.as_deref().map(str::trim).filter(|p| !p.is_empty())
    else {
        eprintln!("WorktreeCreate: input sem worktree_path");
        std::process::exit(1);
    };
    let cwd = input
        .cwd
        .clone()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    match crate::commands::work_unit_open::hook_create(requested, &cwd) {
        Ok(path) => {
            println!("{path}");
            std::process::exit(0);
        }
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}
