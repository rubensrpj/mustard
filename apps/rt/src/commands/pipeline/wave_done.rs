//! `mustard-rt run wave-done` — composite "finalize a completed wave".
//!
//! Folds the bookkeeping steps the orchestrator did by hand after a wave's
//! agent returned and its work was committed, into ONE call:
//!   0. `wave-reclaim` — fold the wave's commit out of its isolated agent
//!      checkout and onto the work-unit branch — the unit being the one the
//!      INVOKING tree sits on, which is why `cwd` is threaded all the way in.
//!      FIRST, and gating: a wave whose work has not returned is not complete,
//!      so a refused fold (a conflict, an agent checkout holding uncommitted
//!      work, nowhere safe to fold to, or checkouts holding work nothing this
//!      wave declares can claim) stops
//!      the composite before the completion event is emitted and answers with
//!      the blocking reason. With isolation off there is no agent checkout and
//!      the step is a clean no-op, so the shared-tree pipeline is unchanged.
//!   1. `emit-pipeline --kind pipeline.wave.complete` — the completion event
//!      plus its side effects (the wave's `spec.md`/`meta.json` → Close/Completed
//!      and the parent's progress bump). Reused **verbatim** through
//!      [`emit_pipeline::run`] so the event, its legacy-alias fan-out, and the
//!      meta sync stay byte-identical to the hand-emitted form.
//!   2. materializing the wave's qualifying lessons as `{spec}/memory/*.md` —
//!      the PROCESS-MEMORY producer. See [`materialize_wave_memory`]; it runs
//!      HERE, at wave close, because closing the SPEC is too late: by then every
//!      following wave has already run without the lesson.
//!   3. caching the wave's SIGNATURE digest into `wave-{N}-{role}/diff.md` for the next
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
//! disappears. Fail-open: neither the memory materialization nor a diff-cache
//! failure can block the completion emit (which already ran first) — only the
//! reclaim gate, step 0, ever refuses to report a wave done, and only because
//! stranded work is a real loss.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::event::EVENT_PIPELINE_WAVE_COMPLETE;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};

use crate::commands::agent::context_inject;
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

    // 2. Materialize this wave's qualifying lessons as spec-memory files, so
    //    the NEXT round's render can pick them up — fail-open.
    let memories = materialize_wave_memory(cwd, spec, wave);

    // 3. Cache the wave diff for the next round's render — fail-open.
    let diff_cached = cache_wave_diff(cwd, spec, wave);

    json!({
        "wave": wave,
        "waveComplete": true,
        "diffCached": diff_cached,
        "memoriesWritten": memories,
        "reclaimed": reclaim.get("action").cloned().unwrap_or(Value::Null),
    })
}

/// Materialize the lessons this wave produced into `{spec}/memory/*.md` — the
/// producer side of the spec-memory channel whose CONSUMER
/// ([`context_inject::resolve_spec_memory`] + the `{cross_wave_memory}`
/// placeholder) has been complete and wired the whole time, reading a directory
/// that no spec in this repository ever had.
///
/// ## Where the lessons come from
///
/// From what actually happened: the `decision` events on the spec's OWN NDJSON
/// log, harvested at `SubagentStop` from a returning agent's `<MEMORY>` block
/// (`hooks::task::subagent_inject::capture_memory_decision`). Nothing is
/// summarised, generated, or inferred — a memory file's body is verbatim what an
/// agent wrote, and its frontmatter names the wave, the run (session), and the
/// timestamp of the event it came from. That traceability is the whole
/// difference from the memory injection this project REMOVED: the defect there
/// was provenance that could not be checked.
///
/// ## Why at wave close and not spec close
///
/// A lesson is only worth materializing if a LATER wave can still act on it.
/// Closing the spec is after the last wave, so the whole point is already gone —
/// in the run that produced this spec, wave 1 learned that the shared git helper
/// trims its whole output (so porcelain cannot be sliced by fixed column) and
/// waves 3, 4 and 5 each wrote Rust without ever seeing it.
///
/// ## Attribution and idempotency
///
/// A decision not yet on disk is attributed to the wave closing NOW — which is
/// the wave that produced it, since this runs at every wave's close. A lesson
/// already materialized is skipped by body comparison, so re-running
/// `wave-done`, or closing a later wave, never re-attributes or duplicates an
/// earlier wave's lesson.
///
/// ## Scope and failure
///
/// Strictly intra-spec: only this spec's own event log is read, and only this
/// spec's `memory/` is written. No global/project memory is produced or injected
/// anywhere — project memory reaches a spec while it is AUTHORED, through the
/// conversation-material channel, not through this code path.
///
/// Fail-open at every step: an unresolvable spec, an unreadable log, a directory
/// that cannot be created, a write that fails — all degrade to fewer (or no)
/// memory files. Completion is gated by the reclaim step because stranded work
/// is a real loss; a missing memory file is not, and must never block a wave
/// from being reported done.
///
/// Returns the repo-relative paths written, for the composite's JSON report.
fn materialize_wave_memory(cwd: &Path, spec: &str, wave: u64) -> Vec<String> {
    let Ok(spec_paths) = ClaudePaths::for_project(cwd).and_then(|p| p.for_spec(spec)) else {
        return Vec::new();
    };
    let memory_dir = spec_paths.dir().join("memory");

    // What this wave's agents actually recorded, in log order, already through
    // the value filter — the producing-side twin of the bar the role contract
    // states at emission. A filter with no input it rejects is decoration.
    let events =
        mustard_core::view::projection::read_harness_events_from_ndjson_dir(&spec_paths.events_dir());
    let candidates: Vec<Lesson> = events
        .iter()
        .filter(|e| e.event == "decision")
        .filter_map(|e| {
            let text = context_inject::normalize_lesson(
                e.payload.get("title").and_then(Value::as_str).unwrap_or_default(),
            );
            context_inject::lesson_qualifies(&text).then(|| Lesson {
                text,
                role: e.payload.get("role").and_then(Value::as_str).unwrap_or_default().to_string(),
                session: e.session_id.clone(),
                recorded: e.ts.clone(),
            })
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut already = recorded_lessons(&memory_dir);
    let mut written: Vec<String> = Vec::new();
    for lesson in candidates {
        if already.contains(&lesson.text) {
            continue;
        }
        if fs::create_dir_all(&memory_dir).is_err() {
            return written;
        }
        let Some(dest) = free_memory_path(&memory_dir, &lesson.text, wave) else {
            continue;
        };
        let body = render_memory_file(&dest, spec, wave, &lesson);
        if fs::write_atomic(&dest, body.as_bytes()).is_err() {
            continue;
        }
        already.insert(lesson.text.clone());
        written.push(
            dest.strip_prefix(cwd)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| dest.to_string_lossy().replace('\\', "/")),
        );
    }
    written
}

/// One captured lesson plus the provenance that makes it checkable: the role
/// that emitted it, the run (session) it belongs to, and when it was recorded.
struct Lesson {
    text: String,
    role: String,
    session: String,
    recorded: String,
}

/// The lesson bodies already on disk under `memory_dir`, normalised for exact
/// comparison. Read from the BODY, not the frontmatter, so the comparison keys
/// on the lesson itself and cannot drift with the header format.
///
/// Fail-open: a missing/unreadable directory yields an empty set, which at worst
/// re-writes a file that already says the same thing.
fn recorded_lessons(memory_dir: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(entries) = fs::read_dir(memory_dir) else {
        return out;
    };
    for entry in entries {
        if entry.is_dir || !entry.file_name.ends_with(".md") {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&entry.path) {
            out.insert(context_inject::normalize_lesson(&memory_body(&text)));
        }
    }
    out
}

/// The body of a memory file — everything after the closing `---` of the
/// frontmatter, trimmed. A file with no frontmatter is its own body.
fn memory_body(text: &str) -> String {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return text.trim().to_string();
    }
    let rest: Vec<&str> = lines.collect();
    match rest.iter().position(|l| l.trim() == "---") {
        Some(end) => rest[end + 1..].join("\n").trim().to_string(),
        None => text.trim().to_string(),
    }
}

/// Resolve a free path for `lesson` under `memory_dir`.
///
/// The stem is deterministic ([`context_inject::memory_file_stem`]), so two
/// different lessons closing in the same wave can collide on it; a numeric
/// suffix disambiguates. A path whose file already holds a DIFFERENT lesson is
/// never overwritten. `None` when every candidate is taken — fail-open, that
/// lesson is simply not materialized.
fn free_memory_path(memory_dir: &Path, lesson: &str, wave: u64) -> Option<PathBuf> {
    let stem = context_inject::memory_file_stem(lesson, wave);
    for n in 1..=9u8 {
        let name = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{n}.md")
        };
        let path = memory_dir.join(&name);
        let Ok(existing) = fs::read_to_string(&path) else {
            return Some(path); // free slot
        };
        // Occupied — reusable only when it already holds THIS lesson (an
        // idempotent re-write); another lesson's file is never clobbered.
        if context_inject::normalize_lesson(&memory_body(&existing)) == lesson {
            return Some(path);
        }
    }
    None
}

/// Render the memory file: frontmatter that NAMES the wave and the run, then the
/// lesson verbatim as the body.
///
/// The shape is dictated by the consumer, not invented here:
/// `context_inject::extract_memory_summary` takes the first body line as the
/// inline summary, and `description_stems` mines the frontmatter `description:`
/// line as a secondary relevance signal — so the lesson appears in both, once as
/// the header field the matcher reads and once as the body the reader reads.
fn render_memory_file(dest: &Path, spec: &str, wave: u64, lesson: &Lesson) -> String {
    let name = dest
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    format!(
        "---\n\
         name: {name}\n\
         description: {text}\n\
         spec: {spec}\n\
         wave: {wave}\n\
         role: {role}\n\
         session: {session}\n\
         recorded: {recorded}\n\
         source: wave-close\n\
         ---\n\n\
         {text}\n",
        text = lesson.text,
        role = lesson.role,
        session = lesson.session,
        recorded = lesson.recorded,
    )
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

    // --- process memory: the producer this channel never had -----------------

    /// Plant a workspace anchor so `ClaudePaths::for_project` accepts the temp
    /// dir, plus the spec directory itself.
    fn anchored_spec(root: &Path, spec: &str) {
        std::fs::create_dir_all(root.join(".claude/spec").join(spec)).expect("spec dir");
        std::fs::write(root.join("mustard.json"), b"{}").expect("anchor");
    }

    /// Append one `decision` event to `spec`'s own NDJSON log — the SAME shape
    /// `hooks::task::subagent_inject::capture_memory_decision` writes in
    /// production, so this exercises the real channel rather than a parallel
    /// fixture format that could silently drift from the real writer.
    fn seed_decision(root: &Path, spec: &str, title: &str, session: &str) {
        use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: mustard_core::time::now_iso8601(),
            session_id: session.to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("subagent_inject".to_string()),
                actor_type: None,
            },
            event: "decision".to_string(),
            payload: json!({ "title": title, "role": "impl", "source": "memory-block" }),
            spec: Some(spec.to_string()),
        };
        let _ = crate::shared::events::route::emit(&root.to_string_lossy(), &event);
    }

    /// Every `.md` file name under the spec's `memory/` directory.
    fn memory_files(root: &Path, spec: &str) -> Vec<String> {
        let dir = root.join(".claude/spec").join(spec).join("memory");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".md"))
            .collect();
        names.sort();
        names
    }

    /// AC-9: a lesson a wave produced survives that wave and reaches the NEXT
    /// round — the whole point of materializing at wave close instead of spec
    /// close.
    ///
    /// The assertion deliberately targets the `## SPEC MEMORY` wikilink, NOT the
    /// lesson text: the same `decision` event also feeds the `## DECISIONS`
    /// block, so asserting on the prose would pass with no memory file written
    /// at all and prove nothing about this wave's work.
    #[test]
    fn wave_lesson_reaches_the_next_round() {
        use crate::commands::agent::render::{render_prompt_at, RenderMode};

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "porcelain-parser-spec";
        anchored_spec(root, spec);
        std::fs::write(
            root.join(".claude/spec").join(spec).join("spec.md"),
            "# T\n## Tasks\n- [ ] make the porcelain parser read the helper output\n",
        )
        .expect("spec.md");

        let lesson = "Chose the porcelain parser over slicing fixed columns \
                      because the shared git helper trims the whole output";
        seed_decision(root, spec, lesson, "run-42");

        let written = materialize_wave_memory(root, spec, 1);
        assert_eq!(written.len(), 1, "one qualifying lesson → one memory file: {written:?}");

        // The file NAMES its wave, and its frontmatter names the run it belongs
        // to — provenance that can be checked, which is precisely what the
        // memory injection removed from this project could not offer.
        let names = memory_files(root, spec);
        assert_eq!(names.len(), 1, "got {names:?}");
        let name = names[0].trim_end_matches(".md").to_string();
        assert!(name.ends_with("-wave1"), "the file names its wave: {name}");
        let body = std::fs::read_to_string(
            root.join(".claude/spec").join(spec).join("memory").join(&names[0]),
        )
        .expect("memory file");
        assert!(body.contains("\nwave: 1\n"), "{body}");
        assert!(body.contains("\nsession: run-42\n"), "the run is named: {body}");
        assert!(body.contains("porcelain parser over slicing"), "{body}");

        // The next round's prompt carries it — through the consumer that was
        // already wired and had never had anything to read.
        let rendered = render_prompt_at(
            root,
            Some(spec),
            Some(2),
            "impl",
            Path::new("."),
            RenderMode::First,
            None,
            None,
            None,
        );
        assert!(rendered.contains("## SPEC MEMORY"), "no spec-memory block: {rendered}");
        assert!(
            rendered.contains(&format!("[[{name}]]")),
            "wave 1's lesson did not reach wave 2's prompt: {rendered}"
        );

        // Closing a later wave neither duplicates the file nor re-attributes it
        // to that later wave — the lesson stays owned by the wave that had it.
        let again = materialize_wave_memory(root, spec, 2);
        assert!(again.is_empty(), "already materialized: {again:?}");
        assert_eq!(memory_files(root, spec), names, "no duplicate, no re-attribution");
    }

    /// AC-10: the value filter has an input it REJECTS. Both directions are
    /// asserted in one test on purpose — a filter that accepts everything and a
    /// filter that rejects everything are equally useless, and only checking one
    /// side cannot tell them apart.
    #[test]
    fn value_filter_rejects_process_residue() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "residue-spec";
        anchored_spec(root, spec);

        // Process residue: a recap of what was done, an interruption, and a
        // file list — the three shapes the emission contract disqualifies.
        seed_decision(
            root,
            spec,
            "Fixed the wave-done regression and updated the tests that covered it",
            "run-1",
        );
        seed_decision(
            root,
            spec,
            "Interrupted before the build finished, so not everything was verified",
            "run-1",
        );
        seed_decision(root, spec, "src/a.rs src/b.rs tests/c.rs tests/d.rs", "run-1");

        assert!(
            materialize_wave_memory(root, spec, 1).is_empty(),
            "process residue must not become durable memory"
        );
        assert!(
            memory_files(root, spec).is_empty(),
            "no memory file at all: {:?}",
            memory_files(root, spec)
        );

        // The other direction, in the SAME log: a real decision does pass, so
        // the emptiness above is the filter working, not the producer being dead.
        seed_decision(
            root,
            spec,
            "Chose wave-close over spec-close for materializing lessons because at spec close \
             every later wave has already run without them",
            "run-1",
        );
        let written = materialize_wave_memory(root, spec, 1);
        assert_eq!(written.len(), 1, "the qualifying lesson survives: {written:?}");
        assert_eq!(memory_files(root, spec).len(), 1);
    }
}
