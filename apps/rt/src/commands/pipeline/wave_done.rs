//! `mustard-rt run wave-done` — composite "finalize a completed wave".
//!
//! Folds the bookkeeping steps the orchestrator did by hand after a wave's
//! agent returned and its work was committed, into ONE call:
//!   0. `wave-reclaim` — fold the wave's commit out of its isolated agent
//!      checkout and onto the work-unit branch. FIRST, and gating: a wave whose
//!      work has not returned is not complete, so a refused fold (a conflict, an
//!      agent checkout holding uncommitted work, nowhere safe to fold to) stops
//!      the composite before the completion event is emitted and answers with
//!      the blocking reason. With isolation off there is no agent checkout and
//!      the step is a clean no-op, so the shared-tree pipeline is unchanged.
//!   1. `emit-pipeline --kind pipeline.wave.complete` — the completion event
//!      plus its side effects (the wave's `spec.md`/`meta.json` → Close/Completed
//!      and the parent's progress bump). Reused **verbatim** through
//!      [`emit_pipeline::run`] so the event, its legacy-alias fan-out, and the
//!      meta sync stay byte-identical to the hand-emitted form.
//!   2. caching the wave's SIGNATURE digest into `wave-{N}-{role}/diff.md` for the next
//!      round's render. This was a shell redirect (`rtk git diff … > diff.md`)
//!      in the orchestrator prose — fragile on Windows (CRLF, and the bash gate
//!      rejects absolute-path redirect targets). Here it is an atomic LF write
//!      through [`fs::write_atomic`], generated with the same fail-open,
//!      rtk-aware git helper the rest of the pipeline uses.
//!
//! Pure consolidation of the emit + cache steps — same event, same meta sync,
//! same path. The cached diff is a deterministic SIGNATURE digest
//! ([`diff_digest`]) rather than a `--stat` line-count. Only the orchestrator's turn count drops
//! (commit + `wave-done`, not commit + emit + redirect) and the redirect footgun
//! disappears. Fail-open: a diff-cache failure never blocks the completion emit
//! (which already ran first).

use std::path::Path;

use mustard_core::domain::model::event::EVENT_PIPELINE_WAVE_COMPLETE;
use mustard_core::io::fs;
use serde_json::{json, Value};

use crate::commands::event::emit_pipeline::{self, EmitPipelineOpts};
use crate::commands::wave::wave_reclaim::{self, WaveReclaimOpts};

/// Run `mustard-rt run wave-done --spec <name> --wave <N> [--duration-ms <ms>]`.
///
/// Reclaims the wave's isolated checkout, then emits `pipeline.wave.complete`
/// (full side effects) and caches the wave diff. Prints a lean JSON
/// confirmation; exits 1 when the reclaim blocked the completion, so a stranded
/// wave cannot be mistaken for a finished one.
pub fn run(spec: &str, wave: u64, duration_ms: Option<u64>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let report = done_at(&cwd, spec, wave, duration_ms);
    let complete = report.get("waveComplete").and_then(Value::as_bool).unwrap_or(false);
    println!("{}", serde_json::to_string(&report).unwrap_or_else(|_| "{}".into()));
    if !complete {
        std::process::exit(1);
    }
}

/// The composite pass — the testable core of [`run`], with the working
/// directory threaded in instead of read from the process.
///
/// Order is the contract: the reclaim gate runs BEFORE the emit, so a refused
/// fold returns without any completion side effect having happened.
fn done_at(cwd: &Path, spec: &str, wave: u64, duration_ms: Option<u64>) -> Value {
    // 0. The way back. A wave whose work is still sitting in an agent checkout
    //    has not returned, and reporting it complete would strand that work.
    let reclaim = wave_reclaim::reclaim_at(&WaveReclaimOpts {
        root: cwd.to_path_buf(),
        spec: spec.to_string(),
        wave,
    });
    if !reclaim.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return json!({ "wave": wave, "waveComplete": false, "reclaim": reclaim });
    }

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
        base: None,
    });

    // 2. Cache the wave diff for the next round's render — fail-open.
    let diff_cached = cache_wave_diff(cwd, spec, wave);

    json!({
        "wave": wave,
        "waveComplete": true,
        "diffCached": diff_cached,
        "reclaimed": reclaim.get("action").cloned().unwrap_or(Value::Null),
    })
}

/// Generate the wave's SIGNATURE digest (added/removed declarations per changed
/// file, via [`super::diff_digest`]) and write it to the wave's `diff.md`,
/// atomically (LF). Returns the repo-relative path written, or `None` when the
/// wave directory cannot be resolved or the write fails — fail-open, the diff
/// cache is render context and never load-bearing.
///
/// `cwd` is threaded in (not read from the environment) so the resolution + write
/// are unit-testable without mutating the process working directory.
fn cache_wave_diff(cwd: &Path, spec: &str, wave: u64) -> Option<String> {
    let wave_dir = emit_pipeline::wave_spec_path(cwd, spec, wave)?;
    // Pilar 3c: a deterministic SIGNATURE digest (added/removed declarations per
    // changed file) instead of the old `git diff --stat` line-count — higher
    // signal for the next wave's implementer/reviewer, "never a file dump". Same
    // fail-open contract: any git error degrades to an empty digest, so `diff.md`
    // is still written (possibly empty) and never load-bearing.
    let digest = super::diff_digest::build_signature_diff(cwd, "HEAD~1", "HEAD");
    let body = format!("{}\n", digest.trim_end());
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
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// AC-6: when the fold-back cannot complete, the wave is NOT reported
    /// complete and the blocking reason NAMES the files — no wave is ever marked
    /// done while its work sits stranded in its own checkout.
    ///
    /// Both sides touch the same line of `a.txt`, so the fold is a real content
    /// conflict. The "no completion was emitted" claim is observed on disk
    /// rather than asserted about control flow: `diff.md` is written by step 2,
    /// AFTER the `pipeline.wave.complete` emit of step 1, so its absence under
    /// the resolved wave directory means execution never reached the emit. (The
    /// emit itself writes through the process-wide project dir, which a unit
    /// test must not repoint — this is the observable that stays honest.)
    #[test]
    fn wave_reclaim_blocks_completion_on_conflict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&main).expect("mkdir");
        git(&main, &["init", "."]);
        git(&main, &["config", "user.email", "t@t"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-b", "dev"]);
        std::fs::write(main.join("mustard.json"), r#"{"git":{"flow":{"*":"dev"}}}"#).expect("cfg");
        std::fs::write(main.join(".gitignore"), ".claude/\n").expect("ignore");
        std::fs::write(main.join("a.txt"), "base\n").expect("seed");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "seed"]);
        git(&main, &["checkout", "-b", "dev_unit"]);
        std::fs::create_dir_all(main.join(".claude/spec/demo/wave-1-rt")).expect("wave dir");
        std::fs::write(main.join(".claude/spec/demo/wave-1-rt/spec.md"), "## Files\n\n- a.txt\n")
            .expect("wave spec");

        // The wave's isolated checkout, cut from the unit's HEAD, rewrites the
        // shared line…
        git(&main, &["worktree", "add", ".claude/worktrees/agent-w1", "-b", "agent-w1"]);
        let wt = main.join(".claude").join("worktrees").join("agent-w1");
        std::fs::write(wt.join("a.txt"), "from the wave\n").expect("wave edit");
        git(&wt, &["add", "-A"]);
        git(&wt, &["commit", "-m", "wave 1 work"]);
        // …while the unit branch moved the same line elsewhere.
        std::fs::write(main.join("a.txt"), "from the unit\n").expect("unit edit");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "unit work"]);

        let v = done_at(&main, "demo", 1, Some(0));
        assert_eq!(v["waveComplete"], json!(false), "{v}");
        assert_eq!(v["reclaim"]["ok"], json!(false), "{v}");
        assert_eq!(v["reclaim"]["reason"], json!("merge-conflict"), "{v}");
        assert_eq!(v["reclaim"]["files"], json!(["a.txt"]), "the conflicting path is named: {v}");

        // The completion never ran: its follow-on artifact was never written.
        assert!(
            !main.join(".claude/spec/demo/wave-1-rt/diff.md").exists(),
            "no completion side effect may happen once the fold is refused"
        );
        // And nothing was destroyed or half-merged.
        assert!(wt.exists(), "the agent checkout is preserved for inspection");
        // Compared line-wise: `merge --abort` restores the file through git's
        // checkout filters, so on a `core.autocrlf` platform the bytes come back
        // CRLF — the claim under test is the CONTENT, not the line ending.
        assert_eq!(
            std::fs::read_to_string(main.join("a.txt")).expect("a.txt").trim_end(),
            "from the unit",
            "the aborted merge left the main checkout exactly as it was"
        );
    }

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
