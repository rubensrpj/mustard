//! `mustard-rt run wave-done` — composite "finalize a completed wave".
//!
//! Folds the two bookkeeping steps the orchestrator did by hand after a wave's
//! agent returned and its work was committed, into ONE call:
//!   1. `emit-pipeline --kind pipeline.wave.complete` — the completion event
//!      plus its side effects (the wave's `spec.md`/`meta.json` → Close/Completed
//!      and the parent's progress bump). Reused **verbatim** through
//!      [`emit_pipeline::run`] so the event, its legacy-alias fan-out, and the
//!      meta sync stay byte-identical to the hand-emitted form.
//!   2. caching the wave's diff stat into `wave-{N}-{role}/diff.md` for the next
//!      round's render. This was a shell redirect (`rtk git diff … > diff.md`)
//!      in the orchestrator prose — fragile on Windows (CRLF, and the bash gate
//!      rejects absolute-path redirect targets). Here it is an atomic LF write
//!      through [`fs::write_atomic`], generated with the same fail-open,
//!      rtk-aware git helper the rest of the pipeline uses.
//!
//! Pure consolidation — same event, same meta sync, same diff content (`git diff
//! HEAD~1 HEAD --stat`), same path. Only the orchestrator's turn count drops
//! (commit + `wave-done`, not commit + emit + redirect) and the redirect footgun
//! disappears. Fail-open: a diff-cache failure never blocks the completion emit
//! (which already ran first).

use std::path::Path;

use mustard_core::domain::model::event::EVENT_PIPELINE_WAVE_COMPLETE;
use mustard_core::io::fs;
use mustard_core::platform::process::rtk_command;
use serde_json::json;

use crate::commands::event::emit_pipeline::{self, EmitPipelineOpts};

/// Run `mustard-rt run wave-done --spec <name> --wave <N> [--duration-ms <ms>]`.
///
/// Emits `pipeline.wave.complete` (full side effects) then caches the wave diff.
/// Prints a lean JSON confirmation.
pub fn run(spec: &str, wave: u64, duration_ms: Option<u64>) {
    // 1. Faithful reuse of the wave.complete emit path: event + wave/meta sync +
    //    parent-progress bump. The payload is constructed valid JSON, so none of
    //    emit-pipeline's exit paths (unknown kind / bad JSON / the
    //    pipeline.complete QA gate) can fire for this kind.
    emit_pipeline::run(EmitPipelineOpts {
        kind: EVENT_PIPELINE_WAVE_COMPLETE.to_string(),
        spec: spec.to_string(),
        payload: Some(json!({ "wave": wave, "duration_ms": duration_ms.unwrap_or(0) }).to_string()),
        allow_no_qa: false,
        intent: None,
    });

    // 2. Cache the wave diff for the next round's render — fail-open.
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let diff_cached = cache_wave_diff(&cwd, spec, wave);

    println!(
        "{}",
        json!({ "wave": wave, "waveComplete": true, "diffCached": diff_cached })
    );
}

/// Generate `git diff HEAD~1 HEAD --stat` and write it to the wave's `diff.md`,
/// atomically (LF). Returns the repo-relative path written, or `None` when the
/// wave directory cannot be resolved or the write fails — fail-open, the diff
/// cache is render context and never load-bearing.
///
/// `cwd` is threaded in (not read from the environment) so the resolution + write
/// are unit-testable without mutating the process working directory.
fn cache_wave_diff(cwd: &Path, spec: &str, wave: u64) -> Option<String> {
    let wave_dir = emit_pipeline::wave_spec_path(cwd, spec, wave)?;
    // Same content the orchestrator prose produced (`git diff HEAD~1 HEAD
    // --stat`), via the fail-open rtk-aware helper — no shell redirect, no
    // CRLF/absolute-path footgun. A failed/absent git degrades to an empty stat.
    let stat = rtk_command("git", &["diff", "HEAD~1", "HEAD", "--stat"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let body = format!("{}\n", stat.trim_end());
    let dest = wave_dir.join("diff.md");
    fs::write_atomic(&dest, body.as_bytes()).ok()?;
    Some(
        dest.strip_prefix(cwd)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| dest.to_string_lossy().replace('\\', "/")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The new logic: resolve the `wave-{N}-*` dir and write `diff.md`
    /// atomically. No git repo here, so the diff stat is empty (fail-open) — the
    /// contract under test is the path resolution + the atomic write, not git.
    /// The completion emit itself is covered by `emit_pipeline`'s own tests.
    #[test]
    fn cache_wave_diff_resolves_wave_dir_and_writes_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude/spec/demo-wave/wave-1-impl")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();

        let rel = cache_wave_diff(root, "demo-wave", 1);
        let diff = root.join(".claude/spec/demo-wave/wave-1-impl/diff.md");
        assert!(diff.is_file(), "diff.md written under the resolved wave dir");
        let rel = rel.expect("returns the cached path");
        assert!(rel.contains("wave-1-impl/diff.md"), "relative path points at the wave dir: {rel}");

        // A missing wave dir → None (fail-open), no write, no panic.
        assert!(cache_wave_diff(root, "demo-wave", 9).is_none(), "unresolved wave → None");
        assert!(
            !root.join(".claude/spec/demo-wave/wave-9-impl/diff.md").exists(),
            "no stray write for an unresolved wave"
        );
    }
}
