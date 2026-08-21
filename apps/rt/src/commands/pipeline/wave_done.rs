//! `mustard-rt run wave-done` — composite "finalize a completed wave".
//!
//! Folds the bookkeeping steps the orchestrator did by hand after a wave's
//! agent returned and its work was committed, into ONE call:
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
//!   4. naming the wave's REALITY OBLIGATIONS that nothing it recorded accounts
//!      for. See [`unaccounted_reality_obligations`]; it reports a fact about the
//!      record ("no text this wave left behind names `RO-3.1`"), never a claim
//!      that the duty went unmet — which is why it prints and never blocks.
//!
//! Pure consolidation of the emit + cache steps — same event, same meta sync,
//! same path. The cached diff is a deterministic SIGNATURE digest
//! ([`diff_digest`]) rather than a `--stat` line-count. Only the orchestrator's turn count drops
//! (commit + `wave-done`, not commit + emit + redirect) and the redirect footgun
//! disappears. Fail-open: neither the memory materialization nor a diff-cache
//! failure can block the completion emit (which already ran first).

use std::path::{Path, PathBuf};

use mustard_core::domain::model::event::EVENT_PIPELINE_WAVE_COMPLETE;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};

use crate::commands::agent::context_inject;
use crate::commands::agent::render;
use crate::commands::event::emit_pipeline::{self, EmitPipelineOpts};

/// Run `mustard-rt run wave-done --spec <name> --wave <N> [--duration-ms <ms>]`.
///
/// Emits `pipeline.wave.complete` (full side effects), materializes the wave's
/// lessons, then caches the wave diff. Prints a lean JSON confirmation.
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
        unit_name: None,
        base: None,
        work_kind: None,
    });

    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());

    // 2. Materialize this wave's qualifying lessons as spec-memory files, so
    //    the NEXT round's render can pick them up — fail-open.
    let memories = materialize_wave_memory(&cwd, spec, wave);

    // 3. Cache the wave diff for the next round's render — fail-open.
    let diff_cached = cache_wave_diff(&cwd, spec, wave);

    // 4. Name the reality obligations this wave carried and left without an
    //    account in anything it recorded — fail-open, and a REPORT, never a gate.
    let unaccounted = unaccounted_reality_obligations(&cwd, spec, wave);
    for line in &unaccounted {
        eprintln!("[wave-done] WARN: {line}");
    }

    println!(
        "{}",
        json!({
            "wave": wave,
            "waveComplete": true,
            "diffCached": diff_cached,
            "memoriesWritten": memories,
            "realityUnaccounted": unaccounted,
        })
    );
}

/// The reality obligations this wave was given that nothing it recorded accounts
/// for, each named by its id plus the duty verbatim.
///
/// ## What is actually being checked
///
/// NOT whether the duty was met — no code can know that. What is checked is a
/// fact about the RECORD: the wave declared duty `RO-3.1`, and no text the wave
/// left behind names `RO-3.1`. That is why the field is called *unaccounted* and
/// not *unmet*, and why this is printed rather than enforced: a duty with no
/// account may have been honoured by an agent that forgot to say so, and a gate
/// here would be the harness asserting what it did not verify — the exact habit
/// the spec this ships with exists to remove.
///
/// ## Where an account comes from
///
/// The wave's own recorded return, on the spec's OWN NDJSON log — see
/// [`recorded_return_text`] for the three event kinds that carry an agent's own
/// words and the wave scoping applied to them. The dispatch prompt instructs the
/// agent to account for each duty by its id, and the ids carry the wave number
/// ([`wave_scaffold::parse_reality_obligations`]'s writer twin), so one wave's
/// report can never clear another wave's duty even though the log is per-spec.
/// The wave scoping is the second lock on the same door: an id match inside a
/// SIBLING's return is not even looked at.
///
/// Fail-open at every step: an unresolvable spec or wave, an unreadable wave
/// `spec.md`, or a wave that declared no duty all yield an empty list.
fn unaccounted_reality_obligations(cwd: &Path, spec: &str, wave: u64) -> Vec<String> {
    let Some(wave_dir) = emit_pipeline::wave_spec_path(cwd, spec, wave) else {
        return Vec::new();
    };
    let text = fs::read_to_string(wave_dir.join("spec.md")).unwrap_or_default();
    let duties = crate::commands::wave::wave_scaffold::parse_reality_obligations(&text);
    if duties.is_empty() {
        return Vec::new();
    }
    let Ok(spec_paths) = ClaudePaths::for_project(cwd).and_then(|p| p.for_spec(spec)) else {
        return Vec::new();
    };
    let recorded = recorded_return_text(&spec_paths.events_dir(), wave);
    duties
        .into_iter()
        .filter(|(id, _)| !accounts_for(&recorded, id))
        .map(|(id, duty)| {
            format!(
                "{id} — no account of this duty in anything wave {wave} recorded: \"{duty}\". \
                 Either the world was never checked, or the check was never reported by id"
            )
        })
        .collect()
}

/// `true` when `recorded` accounts for the obligation `id` — the id appears as
/// an IDENTIFIER, not merely as a run of characters.
///
/// A bare `contains` is wrong in the one way that matters here: obligation ids
/// share prefixes (`RO-3.1` is a prefix of `RO-3.10`), so a wave reporting
/// `RO-3.10` also cleared `RO-3.1`, turning an account of one duty into a silent
/// discharge of another. Both sides of a hit must therefore end the identifier:
/// the characters an id is made of are ASCII alphanumerics, `-` and `.`, so a
/// neighbouring one of those means the match landed INSIDE a longer id.
///
/// Case-sensitive on purpose — ids are generated uppercase by
/// [`crate::commands::wave::wave_scaffold`], and folding case here would only
/// widen a test that exists to be narrow.
fn accounts_for(recorded: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let bytes = recorded.as_bytes();
    // Byte-indexed rather than sliced, so a multi-byte neighbour can never split
    // a char boundary. A continuation byte is not ASCII, so it correctly reads
    // as "not part of an id".
    let is_id_char = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'.';
    recorded.match_indices(id).any(|(start, hit)| {
        let end = start + hit.len();
        let left_ok = start == 0 || !is_id_char(bytes[start - 1]);
        let right_ok = end >= bytes.len() || !is_id_char(bytes[end]);
        left_ok && right_ok
    })
}

/// What WAVE `wave` recorded on its way back, concatenated — the substrate an
/// obligation id is looked for in.
///
/// ## The three kinds read
///
/// Only event kinds that carry an agent's OWN words:
///
/// - `agent.return` — the returning report, captured at `SubagentStop` by
///   [`crate::hooks::task::subagent_inject`]. This is where an account of a duty
///   actually lives: ordinary prose in the body of a report. It always names the
///   wave that produced it (the capture records nothing when it cannot), so it
///   never reaches a sibling under the "no wave" allowance below.
/// - `decision` — a harvested `<MEMORY>` block.
/// - `agent.stop` — the dispatcher-side telemetry. Kept for the record it can
///   carry on a SYNCHRONOUS dispatch, but it is not the load-bearing channel: on
///   the background dispatch the wave pipeline uses, its payload is the launch
///   acknowledgement, produced when the child starts and containing no part of
///   the return. Reading only this one was the measured defect — see
///   `capture_return_report`'s own docs.
///
/// The whole payload is serialised rather than one named field, because the
/// report key has moved before and a missed rename would silently turn every
/// duty into an unaccounted one.
///
/// ## The wave scoping
///
/// A round's waves all write to one per-SPEC log, so "what this wave recorded"
/// has to be selected, not assumed. An event is this wave's when it names this
/// wave; an event that names NO wave (`0` — the schema's "outside a wave plan",
/// and the shape every pre-stamp record has) belongs to nobody and stays
/// readable by every wave, since excluding it would silently stop clearing
/// duties that are in fact accounted for. An event naming a DIFFERENT wave is a
/// sibling's record and is never read here.
///
/// Fail-open: an unreadable log yields "".
fn recorded_return_text(events_dir: &Path, wave: u64) -> String {
    mustard_core::view::projection::read_harness_events_from_ndjson_dir(events_dir)
        .iter()
        .filter(|e| {
            e.event == "agent.return" || e.event == "agent.stop" || e.event == "decision"
        })
        .filter(|e| e.wave == 0 || u64::from(e.wave) == wave)
        .map(|e| e.payload.to_string())
        .collect::<Vec<_>>()
        .join("\n")
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
/// A lesson is attributed to the wave RECORDED ON ITS OWN EVENT, never to the
/// wave closing now. The two used to be conflated on the premise that wave-done
/// runs at every wave's close — which is true, but says nothing about ORDER: a
/// round closes several waves, and the first `wave-done` of the round sees every
/// sibling's decision already on the log. It swept them all and stamped each
/// with its own number, so a wave's lesson was filed under a sibling. Measured
/// on the run that produced this spec: two rounds, four memory files, two of
/// them attributed to a wave that did not write them.
///
/// The file's `wave:` field was fixed first, by reading the EVENT. But the sweep
/// itself outlived that fix and kept the same conflation one level up, in the
/// RECORD of the close: closing wave 1 harvested wave 2's lesson and reported
/// the path it wrote under wave 1's `memoriesWritten`. Measured on the run this
/// fix comes from — `wave-done --wave 1` returned
/// `…/memory/shared-proc-…-wave2.md`. A close now takes only what belongs to it:
///
/// - a lesson whose event names THIS wave — its own;
/// - a lesson whose event names NO wave (`0`, the schema's "outside a wave
///   plan") — it belongs to nobody, so no other close will ever come for it, and
///   dropping it here would lose it outright. Its file still says `unknown`; the
///   close reports having written it, not having produced it.
///
/// A lesson naming a DIFFERENT wave is left for that wave's own close, which is
/// the call that gets to report it.
///
/// A lesson already materialized is skipped by body comparison, so re-running
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
/// memory files. A missing memory file must never block a wave from being
/// reported done.
///
/// Returns the repo-relative paths written, for the composite's JSON report.
///
/// `wave` SELECTS which lessons this close takes; it is never STAMPED onto one —
/// the number in a file always comes from the event that carried the lesson.
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
        // This close's own harvest: the lessons this wave emitted, plus the ones
        // no wave claims (see the attribution section above). A sibling's lesson
        // stays for the sibling's close.
        .filter(|e| e.wave == 0 || u64::from(e.wave) == wave)
        .filter_map(|e| {
            let text = context_inject::normalize_lesson(
                e.payload.get("title").and_then(Value::as_str).unwrap_or_default(),
            );
            context_inject::lesson_qualifies(&text).then(|| Lesson {
                text,
                // The wave the EVENT recorded. `0` is the schema's "outside a
                // wave plan" and here means nobody established one, so it
                // becomes `None` and the file will say so.
                wave: (e.wave > 0).then(|| u64::from(e.wave)),
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
        let Some(dest) = free_memory_path(&memory_dir, &lesson.text, lesson.wave) else {
            continue;
        };
        let body = render_memory_file(&dest, spec, &lesson);
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

/// One captured lesson plus the provenance that makes it checkable: the wave
/// that emitted it (`None` when its event recorded none), the role that emitted
/// it, the run (session) it belongs to, and when it was recorded.
struct Lesson {
    text: String,
    wave: Option<u64>,
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
/// The stem is deterministic ([`context_inject::memory_file_stem_for`]), so two
/// different lessons from the same wave can collide on it; a numeric suffix
/// disambiguates. A path whose file already holds a DIFFERENT lesson is never
/// overwritten. `None` when every candidate is taken — fail-open, that lesson is
/// simply not materialized.
///
/// `wave` is the EMITTING wave, `None` when its event recorded none.
fn free_memory_path(memory_dir: &Path, lesson: &str, wave: Option<u64>) -> Option<PathBuf> {
    let stem = context_inject::memory_file_stem_for(lesson, wave);
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
///
/// The `wave:` field is the EMITTING wave and reads `unknown` when its event
/// recorded none. A number here is a claim about who learned this; the closing
/// wave's number would be a claim nothing established.
fn render_memory_file(dest: &Path, spec: &str, lesson: &Lesson) -> String {
    let name = dest
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let wave = lesson
        .wave
        .map_or_else(|| "unknown".to_string(), |w| w.to_string());
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
/// ## Why the range alone is not the wave
///
/// The dispatch loop commits ONCE PER ROUND, not once per wave, so `HEAD~1..HEAD`
/// is the ROUND's commit: every wave that ran in that round would otherwise cache
/// the identical digest. That is not cosmetic — this cache feeds the retry
/// context and the closing summary, so a wave that came back blocked would leak
/// its half-written files into a finished sibling's record. The range stays as it
/// is (it is the right WHEN); the breadth is what gets cut, by the files the wave
/// DECLARED in its own `## Files` section.
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
    let declared = declared_wave_files(&wave_dir);
    let digest = super::diff_digest::build_signature_diff(cwd, "HEAD~1", "HEAD", &declared);
    let body = format!("{}\n", digest.trim_end());
    let dest = wave_dir.join("diff.md");
    fs::write_atomic(&dest, body.as_bytes()).ok()?;
    Some(
        dest.strip_prefix(cwd)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| dest.to_string_lossy().replace('\\', "/")),
    )
}

/// The files this wave declared, read from the wave's OWN `spec.md` through the
/// same `## Files` reader the rest of the pipeline uses
/// ([`render::files_section_paths`] — the reference-files builder and the
/// conversation-material cut read the identical list), so this scope cannot
/// disagree with what the wave's agent was told its boundary was.
///
/// Fail-open: a missing or unreadable wave `spec.md`, or one with no `## Files`
/// section, yields an empty list — and an empty scope means the digest keeps
/// today's un-narrowed behaviour rather than silently caching nothing.
fn declared_wave_files(wave_dir: &Path) -> Vec<String> {
    let text = fs::read_to_string(wave_dir.join("spec.md")).unwrap_or_default();
    render::files_section_paths(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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

    /// Whether `git` is on PATH; mirrors `diff_digest`'s own guard so the
    /// git-backed test degrades to a silent pass where git is unavailable.
    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn git(cwd: &Path, args: &[&str]) {
        let _ = Command::new("git").args(args).current_dir(cwd).output();
    }

    /// A repo plus a spec whose waves 1 and 2 declare DISJOINT files, and a
    /// wave 3 that declares none at all.
    fn round_repo(root: &Path) {
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.email", "t@e.x"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("mustard.json"), b"{}").expect("anchor");

        let spec_dir = root.join(".claude/spec/round-spec");
        for (dir, body) in [
            ("wave-1-impl", "# W1\n\n## Files\n\n- `src/alpha.rs`\n"),
            ("wave-2-impl", "# W2\n\n## Files\n\n- `src/beta.rs`\n"),
            ("wave-3-impl", "# W3\n\n## Tasks\n\n- [ ] no files declared\n"),
        ] {
            std::fs::create_dir_all(spec_dir.join(dir)).expect("wave dir");
            std::fs::write(spec_dir.join(dir).join("spec.md"), body).expect("wave spec");
        }

        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/alpha.rs"), "pub fn alpha_seed() {}\n").expect("alpha");
        std::fs::write(root.join("src/beta.rs"), "pub fn beta_seed() {}\n").expect("beta");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);

        // ONE commit for the whole round — both waves' work lands together,
        // which is exactly the shape the dispatch loop produces.
        std::fs::write(
            root.join("src/alpha.rs"),
            "pub fn alpha_seed() {}\npub fn alpha_added() {}\n",
        )
        .expect("alpha v2");
        std::fs::write(
            root.join("src/beta.rs"),
            "pub fn beta_seed() {}\npub fn beta_added() {}\n",
        )
        .expect("beta v2");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "round 1"]);
    }

    fn cached_diff(root: &Path, wave_dir: &str) -> String {
        std::fs::read_to_string(root.join(".claude/spec/round-spec").join(wave_dir).join("diff.md"))
            .expect("diff.md")
    }

    /// The round shape: two waves committed together must NOT share one diff.
    /// `HEAD~1..HEAD` is the ROUND's commit, so the unscoped digest handed every
    /// wave the same file set — and since this cache feeds the retry context and
    /// the closing summary, a blocked wave's half-written files leaked into a
    /// finished sibling's record. Each wave's cached diff must name only the
    /// files that wave declared.
    #[test]
    fn a_wave_caches_only_its_own_declared_files() {
        if !git_available() {
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        round_repo(root);

        assert!(cache_wave_diff(root, "round-spec", 1).is_some(), "wave 1 cached");
        assert!(cache_wave_diff(root, "round-spec", 2).is_some(), "wave 2 cached");
        let w1 = cached_diff(root, "wave-1-impl");
        let w2 = cached_diff(root, "wave-2-impl");
        // Skip only if git could not produce the range at all (fail-open path,
        // e.g. a sandbox that refuses to commit) — never if the scoping is wrong.
        if w1.trim().is_empty() && w2.trim().is_empty() {
            return;
        }

        assert!(w1.contains("src/alpha.rs"), "wave 1 names its own file: {w1}");
        assert!(w1.contains("alpha_added"), "wave 1 carries its own signature: {w1}");
        assert!(!w1.contains("beta"), "wave 1 leaked its sibling's work: {w1}");

        assert!(w2.contains("src/beta.rs"), "wave 2 names its own file: {w2}");
        assert!(w2.contains("beta_added"), "wave 2 carries its own signature: {w2}");
        assert!(!w2.contains("alpha"), "wave 2 leaked its sibling's work: {w2}");

        // A wave that declared nothing keeps today's behaviour — the whole
        // round — rather than silently caching an empty digest.
        assert!(cache_wave_diff(root, "round-spec", 3).is_some(), "wave 3 cached");
        let w3 = cached_diff(root, "wave-3-impl");
        assert!(
            w3.contains("src/alpha.rs") && w3.contains("src/beta.rs"),
            "an undeclared wave still sees the whole round: {w3}"
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
    fn seed_decision(root: &Path, spec: &str, wave: u32, title: &str, session: &str) {
        use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: mustard_core::time::now_iso8601(),
            session_id: session.to_string(),
            wave,
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
        seed_decision(root, spec, 1, lesson, "run-42");

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

    /// AC-1: a ROUND closes several waves, and each one's memory is written
    /// under the wave that emitted it — none stamped with a sibling's number,
    /// none reported by a sibling's close, and none dropped.
    ///
    /// The measured defect this locks: `wave-done` attributed every pending
    /// lesson to the wave closing NOW. Because a round's waves all close in
    /// sequence, the FIRST close of the round swept its siblings' decisions too
    /// and filed them under its own number. Two rounds of the run that produced
    /// this spec left four memory files, two of them naming a wave that had not
    /// written them.
    ///
    /// So the test drives the closes of a round over a log that already carries
    /// three waves' decisions — exactly the state the first close of a round
    /// sees — and pins BOTH halves: the first close takes only its own (the
    /// half a later fix had to add: the file's `wave:` field was already right
    /// while the close still REPORTED writing a sibling's file), and every
    /// close together loses nothing.
    #[test]
    fn every_wave_keeps_its_own_memory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "one-round-three-waves";
        anchored_spec(root, spec);

        seed_decision(
            root,
            spec,
            3,
            "Chose the porcelain parser over slicing fixed columns because the shared git \
             helper trims the whole output",
            "run-1",
        );
        seed_decision(
            root,
            spec,
            4,
            "Chose an atomic write over a plain write because a mid-write crash corrupts the \
             ledger",
            "run-1",
        );
        seed_decision(
            root,
            spec,
            5,
            "Chose the id match over the substring match because RO-3.10 would otherwise \
             discharge RO-3.1",
            "run-1",
        );

        // The first close of the round. It SEES all three, and it must take only
        // the one wave 3 emitted — a sibling's lesson is the sibling's close to
        // report.
        let first = materialize_wave_memory(root, spec, 3);
        assert_eq!(first.len(), 1, "the first close took a sibling's lesson: {first:?}");
        assert!(first[0].ends_with("-wave3.md"), "and it must be its own: {first:?}");

        // The rest of the round. Together the closes lose nothing.
        let mut written = first;
        written.extend(materialize_wave_memory(root, spec, 4));
        written.extend(materialize_wave_memory(root, spec, 5));
        assert_eq!(written.len(), 3, "none dropped: {written:?}");

        let names = memory_files(root, spec);
        assert_eq!(names.len(), 3, "one file per emitted memory: {names:?}");
        for wave in [3u32, 4, 5] {
            let marker = format!("-wave{wave}.md");
            let name = names
                .iter()
                .find(|n| n.ends_with(&marker))
                .unwrap_or_else(|| panic!("no file for wave {wave}: {names:?}"));
            let body = std::fs::read_to_string(
                root.join(".claude/spec").join(spec).join("memory").join(name),
            )
            .expect("memory file");
            assert!(
                body.contains(&format!("\nwave: {wave}\n")),
                "wave {wave}'s file carries another wave's number: {body}"
            );
        }

        // Re-running any close adds nothing and re-files nothing.
        for wave in [3u64, 4, 5] {
            assert!(materialize_wave_memory(root, spec, wave).is_empty(), "idempotent");
        }
        assert_eq!(memory_files(root, spec), names, "no re-attribution");
    }

    /// The diagnosis of the "five emitted, four written" loss the spec was
    /// written from — measured, not reasoned.
    ///
    /// What the run's own event log shows: five `decision` rows were captured by
    /// `subagent_inject` for that spec, and four memory files exist. The fifth is
    /// the sentence below, verbatim from
    /// `.claude/spec/make-harness-stop-asserting-what/.events/` — the wave-4
    /// lesson the operator later re-entered by hand.
    ///
    /// It was not dropped by the writer. It never reached the writer: the VALUE
    /// FILTER rejected it. The lesson names its alternative ("not
    /// `pipeline.task.dispatch`") but states no consequence of going the other
    /// way, and `lesson_qualifies` requires BOTH clauses. The proof that clause
    /// (b) is the one that failed is the second half of this test: the same
    /// sentence, with a consequence added and nothing else changed, becomes a
    /// file.
    ///
    /// So the loss is a false reject by a filter whose own doc calls itself loose
    /// and says a false reject costs one lesson that stays on the event log —
    /// which is exactly what happened, and how the operator recovered it. Nothing
    /// here is a bug to fix; it is a fact that had to be established rather than
    /// left as "undiagnosed", and this test is what keeps it established.
    #[test]
    fn every_wave_keeps_its_own_memory_and_the_fifth_was_value_filtered_not_dropped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "the-fifth-lesson";
        anchored_spec(root, spec);

        let lost = r#"The trustworthy "was this wave dispatched" signal is `pipeline.wave.start` (emitted by wave-advance itself), not `pipeline.task.dispatch` (orchestrator-relayed and unenforced, per wave-advance's own module docs). Anything asking whether work actually started should key on wave.start first."#;

        assert!(
            !crate::commands::agent::context_inject::lesson_qualifies(lost),
            "the measured fifth lesson must be the value filter's rejection, not the writer's"
        );

        seed_decision(root, spec, 4, lost, "run-measured");
        assert!(
            materialize_wave_memory(root, spec, 4).is_empty(),
            "a lesson the filter rejects must not reach the writer at all"
        );

        // The SAME sentence with a consequence — clause (b) — and nothing else
        // changed. It qualifies, which names the clause that failed above.
        let with_consequence = format!("{lost} Keying on the dispatch event instead reports work that never started.");
        assert!(
            crate::commands::agent::context_inject::lesson_qualifies(&with_consequence),
            "adding the consequence clause must be enough: {with_consequence}"
        );
        seed_decision(root, spec, 4, &with_consequence, "run-measured");
        assert_eq!(
            materialize_wave_memory(root, spec, 4).len(),
            1,
            "the writer drops nothing that clears the filter"
        );
    }

    /// AC-1, through the chain a real run actually walks — no hand-seeded wave
    /// anywhere.
    ///
    /// Why this test exists next to [`every_wave_keeps_its_own_memory`]: that one
    /// writes the `wave` field into the event itself, so it proves the
    /// materializer and nothing about whether a real run can ever produce that
    /// field. Review measured that it could not. The capture read the wave from
    /// `MUSTARD_ACTIVE_WAVE`, which nothing in this repository sets, so every
    /// `decision` row on every real spec log carries `wave: null` and every
    /// memory file a real run wrote said `unknown`. Green test, inert feature.
    ///
    /// So this drives the producers end to end, exactly as the pipeline does:
    ///
    /// 1. `agent-prompt-render --emit ref` writes `wave-{N}-…prompt.md` and hands
    ///    the orchestrator a 2-line stub (here: the stub, written by hand in the
    ///    marker's own format, plus the file the renderer would have written);
    /// 2. the PreToolUse hook expands the stub and STAMPS the wave into the
    ///    prompt it rewrites;
    /// 3. Claude Code persists that prompt verbatim as the child's first
    ///    transcript line (here: written to disk in that shape);
    /// 4. the SubagentStop hook harvests the `<MEMORY>` block and reads the wave
    ///    back off the child's OWN transcript;
    /// 5. `materialize_wave_memory` files each lesson under the wave that emitted
    ///    it.
    ///
    /// Three sibling waves are in flight at once — the round shape that made the
    /// old attribution wrong — and one close sees all three returns.
    #[test]
    fn every_wave_keeps_its_own_memory_through_the_real_dispatch_chain() {
        use crate::commands::agent::agent_prompt_render::PROMPT_REF_MARKER;
        use crate::hooks::task::subagent_inject::SubagentInject;
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "real-chain-three-waves";
        anchored_spec(root, spec);
        let cwd = root.to_string_lossy().to_string();
        // Bind the session→spec the way a live pipeline does, so the capture
        // resolves the spec through its own production lookup. The hook resolves
        // the AMBIENT session id (the value a live hook process carries), so bind
        // that one — binding an invented id would leave the capture spec-less and
        // silently no-op, which is the shape of the very defect being fixed.
        let sid = crate::shared::context::session_id();
        crate::shared::context::bind_session_spec(&cwd, &sid, spec);
        // Belt for a host whose ambient session id is `unknown` (which the
        // binding refuses): the legacy state file `current_spec` reads.
        let states = root.join(".claude/.pipeline-states");
        std::fs::create_dir_all(&states).expect("states dir");
        std::fs::write(states.join(format!("{spec}.json")), b"{}").expect("state file");
        // Fail LOUDLY if the capture would resolve no spec (or another one): a
        // spec-less capture is a silent no-op, and a test that silently captured
        // nothing would assert exactly as much as the inert one it replaces.
        assert_eq!(
            crate::shared::context::spec_for_session(&cwd, &sid)
                .or_else(|| crate::shared::context::current_spec(&cwd))
                .as_deref(),
            Some(spec),
            "the capture's own spec lookup must land on this spec"
        );

        let dispatch_dir = root.join(".claude/spec").join(spec).join(".dispatch");
        std::fs::create_dir_all(&dispatch_dir).expect("dispatch dir");
        let transcripts = root.join("transcripts");
        std::fs::create_dir_all(&transcripts).expect("transcripts dir");

        let lessons = [
            (3u32, "Chose the porcelain parser over slicing fixed columns because the shared git \
                    helper trims the whole output"),
            (4, "Chose an atomic write over a plain write because a mid-write crash corrupts the \
                 ledger"),
            (5, "Chose the id match over the substring match because RO-3.10 would otherwise \
                 discharge RO-3.1"),
        ];

        for (wave, lesson) in lessons {
            // (1) what `--emit ref` leaves on disk, and the stub it returns.
            let rel = format!(".claude/spec/{spec}/.dispatch/wave-{wave}-impl-apps-rt.first.prompt.md");
            std::fs::write(root.join(&rel), "<!-- PREFIX-STABLE -->\n## ROLE\nROLE: impl\n")
                .expect("rendered prompt");
            let stub = format!("{PROMPT_REF_MARKER} {rel}\nDispatch stub — pass verbatim.\n");

            // (2) the PreToolUse hook expands + stamps it.
            let pre = HookInput {
                tool_name: Some("Task".to_string()),
                tool_input: json!({ "prompt": stub, "subagent_type": "impl" }),
                hook_event_name: Some("PreToolUse".to_string()),
                ..HookInput::default()
            };
            let verdict = SubagentInject
                .evaluate(
                    &pre,
                    &Ctx {
                        project_dir: cwd.clone(),
                        trigger: Some(Trigger::PreToolUse),
                        workspace_root: None,
                    },
                )
                .expect("hook must not error");
            let Verdict::Rewrite { tool_input } = verdict else {
                panic!("a ref stub must be rewritten, got {verdict:?}");
            };
            let expanded = tool_input
                .get("prompt")
                .and_then(|v| v.as_str())
                .expect("rewritten prompt")
                .to_string();

            // (3) the child's own transcript — first line is that prompt verbatim.
            let transcript = transcripts.join(format!("agent-wave{wave}.jsonl"));
            let line = json!({
                "type": "user",
                "isSidechain": true,
                "agentId": format!("agent-wave{wave}"),
                "message": { "role": "user", "content": expanded },
            });
            std::fs::write(&transcript, format!("{line}\n")).expect("transcript");

            // (4) the child returns with its lesson; the stop hook captures it.
            let stop = HookInput {
                hook_event_name: Some("SubagentStop".to_string()),
                agent_type: Some("impl".to_string()),
                agent_id: Some(format!("agent-wave{wave}")),
                raw: json!({
                    "agent_id": format!("agent-wave{wave}"),
                    "agent_transcript_path": transcript.to_string_lossy(),
                    "last_assistant_message": format!("Done.\n<MEMORY>{lesson}</MEMORY>"),
                }),
                ..HookInput::default()
            };
            SubagentInject
                .evaluate(
                    &stop,
                    &Ctx {
                        project_dir: cwd.clone(),
                        trigger: Some(Trigger::SubagentStop),
                        workspace_root: None,
                    },
                )
                .expect("hook must not error");
        }

        // (5) each close sees all three returns — the state the first close of a
        //     round sees, and the one that used to stamp its siblings' lessons
        //     with its own number — and takes only its own.
        let mut written = Vec::new();
        for (wave, _) in lessons {
            let mine = materialize_wave_memory(root, spec, u64::from(wave));
            assert_eq!(mine.len(), 1, "wave {wave}'s close took {mine:?}");
            assert!(
                mine[0].ends_with(&format!("-wave{wave}.md")),
                "wave {wave}'s close reported a sibling's file: {mine:?}"
            );
            written.extend(mine);
        }
        assert_eq!(written.len(), 3, "none dropped: {written:?}");

        let names = memory_files(root, spec);
        assert_eq!(names.len(), 3, "one file per emitted memory: {names:?}");
        for (wave, _) in lessons {
            let marker = format!("-wave{wave}.md");
            let name = names
                .iter()
                .find(|n| n.ends_with(&marker))
                .unwrap_or_else(|| panic!("no file for wave {wave}: {names:?}"));
            let body = std::fs::read_to_string(
                root.join(".claude/spec").join(spec).join("memory").join(name),
            )
            .expect("memory file");
            assert!(
                body.contains(&format!("\nwave: {wave}\n")),
                "wave {wave}'s file carries another wave's number: {body}"
            );
        }
        assert!(
            !names.iter().any(|n| n.ends_with("-waveunknown.md")),
            "a wave the dispatch named must never file as unknown: {names:?}"
        );
    }

    /// The other half of AC-1: a decision whose event recorded NO wave is still
    /// materialized — never dropped — and the file says `unknown` instead of
    /// borrowing the closing wave's number. Asserting the drop alone would pass
    /// on a writer that simply refused every unattributed lesson.
    ///
    /// It is also the one lesson a wave-scoped close must still take: no wave
    /// claims it, so no other close will ever come for it.
    #[test]
    fn a_memory_with_no_recorded_wave_says_so() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "unattributed-spec";
        anchored_spec(root, spec);

        seed_decision(
            root,
            spec,
            0,
            "Chose the event's own wave over the closing wave because a round's first close \
             sees every sibling's decision",
            "run-1",
        );

        let written = materialize_wave_memory(root, spec, 1);
        assert_eq!(written.len(), 1, "not dropped: {written:?}");
        let names = memory_files(root, spec);
        assert_eq!(names.len(), 1, "got {names:?}");
        assert!(
            names[0].ends_with("-waveunknown.md"),
            "the name claims a wave nobody established: {}",
            names[0]
        );
        let body =
            std::fs::read_to_string(root.join(".claude/spec").join(spec).join("memory").join(&names[0]))
                .expect("memory file");
        assert!(body.contains("\nwave: unknown\n"), "{body}");
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
            1,
            "Fixed the wave-done regression and updated the tests that covered it",
            "run-1",
        );
        seed_decision(
            root,
            spec,
            1,
            "Interrupted before the build finished, so not everything was verified",
            "run-1",
        );
        seed_decision(root, spec, 1, "src/a.rs src/b.rs tests/c.rs tests/d.rs", "run-1");

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
            1,
            "Chose wave-close over spec-close for materializing lessons because at spec close \
             every later wave has already run without them",
            "run-1",
        );
        let written = materialize_wave_memory(root, spec, 1);
        assert_eq!(written.len(), 1, "the qualifying lesson survives: {written:?}");
        assert_eq!(memory_files(root, spec).len(), 1);
    }

    // --- reality obligations: the duties owed to the world -------------------

    /// Seed an `agent.stop` event carrying a returning agent's report — the same
    /// event `hooks::task::subagent_observer` emits at PostToolUse, so the test
    /// exercises the real channel rather than a fixture shape that could drift.
    fn seed_agent_stop(root: &Path, spec: &str, summary: &str) {
        use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: mustard_core::time::now_iso8601(),
            session_id: "run-7".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("subagent-tracker".to_string()),
                actor_type: None,
            },
            event: "agent.stop".to_string(),
            payload: json!({ "summary": summary }),
            spec: Some(spec.to_string()),
        };
        let _ = crate::shared::events::route::emit(&root.to_string_lossy(), &event);
    }

    /// AC-5: a wave that closes without reporting a duty it was given has that
    /// duty named — by id, with the duty verbatim.
    ///
    /// Two-sided on purpose: the SAME wave carries a second duty the returning
    /// report DOES account for, and that one must not be named. A check that
    /// flags every declared duty is indistinguishable from one that flags none,
    /// and asserting only the flagged side cannot tell them apart.
    #[test]
    fn wave_done_flags_unreported_reality_obligation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "reality-duty-spec";
        anchored_spec(root, spec);
        let wave_dir = root.join(".claude/spec").join(spec).join("wave-3-impl");
        std::fs::create_dir_all(&wave_dir).expect("wave dir");
        std::fs::write(
            wave_dir.join("spec.md"),
            "# W3\n\n## Tasks\n\n- [ ] wire the webhook\n\n## Reality Obligations\n\n\
             - **RO-3.1** — read the provider's official webhook doc for the retry semantics\n\
             - **RO-3.2** — read one stored subscription row and confirm the status column\n",
        )
        .expect("wave spec");

        // The returning agent accounted for the FIRST duty only.
        seed_agent_stop(
            root,
            spec,
            "Wired the webhook. RO-3.1: the official doc says retries are at-least-once.",
        );

        let flagged = unaccounted_reality_obligations(root, spec, 3);
        assert_eq!(flagged.len(), 1, "exactly the unaccounted duty is named: {flagged:?}");
        assert!(flagged[0].starts_with("RO-3.2"), "named by id: {flagged:?}");
        assert!(
            flagged[0].contains("confirm the status column"),
            "the duty rides verbatim so the reader knows what was skipped: {flagged:?}"
        );
        assert!(
            !flagged.iter().any(|f| f.starts_with("RO-3.1")),
            "the accounted duty must not be flagged: {flagged:?}"
        );

        // A wave that declares no duty at all is silent — the report never
        // invents an obligation nobody wrote.
        let plain = root.join(".claude/spec").join(spec).join("wave-4-impl");
        std::fs::create_dir_all(&plain).expect("plain wave dir");
        std::fs::write(plain.join("spec.md"), "# W4\n\n## Tasks\n\n- [ ] plain work\n")
            .expect("plain wave spec");
        assert!(
            unaccounted_reality_obligations(root, spec, 4).is_empty(),
            "a wave with no declared duty is never flagged"
        );
    }

    /// One wave's account must not clear another wave's duty. The ids carry the
    /// wave number precisely because the event log is per-SPEC, not per-wave —
    /// without that, a sibling's report closing a duty here would be the harness
    /// asserting a check that never happened.
    #[test]
    fn a_sibling_waves_report_does_not_clear_this_waves_duty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "reality-crosstalk-spec";
        anchored_spec(root, spec);
        for (n, id) in [(3, "RO-3.1"), (4, "RO-4.1")] {
            let wave_dir = root.join(".claude/spec").join(spec).join(format!("wave-{n}-impl"));
            std::fs::create_dir_all(&wave_dir).expect("wave dir");
            std::fs::write(
                wave_dir.join("spec.md"),
                format!("# W{n}\n\n## Reality Obligations\n\n- **{id}** — check the world\n"),
            )
            .expect("wave spec");
        }
        seed_agent_stop(root, spec, "wave 3 done. RO-3.1: checked, the doc agrees.");

        assert!(
            unaccounted_reality_obligations(root, spec, 3).is_empty(),
            "wave 3's own account clears wave 3's duty"
        );
        let sibling = unaccounted_reality_obligations(root, spec, 4);
        assert_eq!(sibling.len(), 1, "wave 4's duty stays open: {sibling:?}");
        assert!(sibling[0].starts_with("RO-4.1"), "{sibling:?}");
    }

    /// Drive one wave's agent all the way back through the REAL return path: the
    /// PreToolUse expansion stamps the wave into the dispatch prompt, Claude Code
    /// persists that prompt as the first line of the child's own transcript, and
    /// the `SubagentStop` hook harvests the return off it. Returns nothing — what
    /// it leaves behind is on the spec's event log, which is the point.
    ///
    /// Deliberately not a hand-written event: the defect this exercises was that
    /// the channel `wave-done` read could not carry a return at all, and a
    /// fixture-shaped `agent.stop` is exactly what hid it.
    fn child_returns(root: &Path, spec: &str, wave: u32, report: &str) {
        use crate::commands::agent::agent_prompt_render::PROMPT_REF_MARKER;
        use crate::hooks::task::subagent_inject::SubagentInject;
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};

        let cwd = root.to_string_lossy().to_string();
        let dispatch_dir = root.join(".claude/spec").join(spec).join(".dispatch");
        std::fs::create_dir_all(&dispatch_dir).expect("dispatch dir");
        let rel = format!(".claude/spec/{spec}/.dispatch/wave-{wave}-impl.first.prompt.md");
        std::fs::write(root.join(&rel), "<!-- PREFIX-STABLE -->\n## ROLE\nROLE: impl\n")
            .expect("rendered prompt");

        let pre = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({
                "prompt": format!("{PROMPT_REF_MARKER} {rel}\nDispatch stub — pass verbatim.\n"),
                "subagent_type": "impl",
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = |trigger| Ctx {
            project_dir: cwd.clone(),
            trigger: Some(trigger),
            workspace_root: None,
        };
        let verdict = SubagentInject
            .evaluate(&pre, &ctx(Trigger::PreToolUse))
            .expect("hook must not error");
        let Verdict::Rewrite { tool_input } = verdict else {
            panic!("a ref stub must be rewritten, got {verdict:?}");
        };
        let expanded = tool_input
            .get("prompt")
            .and_then(|v| v.as_str())
            .expect("rewritten prompt")
            .to_string();

        let transcripts = root.join("transcripts");
        std::fs::create_dir_all(&transcripts).expect("transcripts dir");
        let transcript = transcripts.join(format!("agent-wave{wave}.jsonl"));
        let line = json!({
            "type": "user",
            "isSidechain": true,
            "agentId": format!("agent-wave{wave}"),
            "message": { "role": "user", "content": expanded },
        });
        std::fs::write(&transcript, format!("{line}\n")).expect("transcript");

        let stop = HookInput {
            hook_event_name: Some("SubagentStop".to_string()),
            agent_type: Some("impl".to_string()),
            agent_id: Some(format!("agent-wave{wave}")),
            raw: json!({
                "agent_id": format!("agent-wave{wave}"),
                "agent_transcript_path": transcript.to_string_lossy(),
                "last_assistant_message": report,
            }),
            ..HookInput::default()
        };
        SubagentInject
            .evaluate(&stop, &ctx(Trigger::SubagentStop))
            .expect("hook must not error");
    }

    /// The record of a wave is the record OF THAT WAVE — on both of its sides.
    ///
    /// Measured on the run this fix comes from, a round that dispatched waves 1
    /// and 2 in parallel:
    ///
    /// 1. `wave-done --wave 1` reported `RO-1.1` unaccounted while the wave-1
    ///    agent's return opened with `RO-1.1 — verified on this install …`. The
    ///    only channel `wave-done` read was `agent.stop`, emitted at
    ///    `PostToolUse(Task)` — which on a background dispatch carries the launch
    ///    acknowledgement, not the return. The account existed and the record
    ///    could not hold it.
    /// 2. The SAME call reported `memoriesWritten:
    ///    [".../memory/shared-proc-…-wave2.md"]` — the WAVE 2 agent's `<MEMORY>`
    ///    block, harvested by wave 1's close because the harvest was spec-scoped.
    ///
    /// So the test runs a parallel round — the exact situation neither defect
    /// survives — and pins both sides. Two-sided on each: a duty the wave did NOT
    /// account for must still be named (or a check that clears everything would
    /// pass), and a duty of THIS wave that only a SIBLING's report mentions must
    /// stay open (or a per-spec read would pass, since ids match across the log).
    #[test]
    fn each_waves_finalisation_reads_only_its_own_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "parallel-round-record";
        anchored_spec(root, spec);
        let cwd = root.to_string_lossy().to_string();
        // Bind the AMBIENT session id, the value a live hook process carries —
        // the capture's own spec lookup keys on it, and an invented id would
        // leave every capture spec-less and silently no-op.
        let sid = crate::shared::context::session_id();
        crate::shared::context::bind_session_spec(&cwd, &sid, spec);
        let states = root.join(".claude/.pipeline-states");
        std::fs::create_dir_all(&states).expect("states dir");
        std::fs::write(states.join(format!("{spec}.json")), b"{}").expect("state file");
        assert_eq!(
            crate::shared::context::spec_for_session(&cwd, &sid)
                .or_else(|| crate::shared::context::current_spec(&cwd))
                .as_deref(),
            Some(spec),
            "the capture's own spec lookup must land on this spec"
        );

        for (wave, duties) in [
            (1u32, "- **RO-1.1** — confirm on this install how a junction is created unelevated\n\
                    - **RO-1.2** — read one existing worktree and confirm its owner pid\n"),
            (2, "- **RO-2.1** — run the collector against a worktree whose owner is gone\n"),
        ] {
            let wave_dir = root.join(".claude/spec").join(spec).join(format!("wave-{wave}-impl"));
            std::fs::create_dir_all(&wave_dir).expect("wave dir");
            std::fs::write(
                wave_dir.join("spec.md"),
                format!("# W{wave}\n\n## Reality Obligations\n\n{duties}"),
            )
            .expect("wave spec");
        }

        // Wave 1 accounts for RO-1.1 only, and leaves its own lesson.
        child_returns(
            root,
            spec,
            1,
            "RO-1.1 — verified on this install, unelevated (IsInRole(Administrator) = False): \
             a directory junction needs no privilege.\n\
             <MEMORY>Chose a directory junction over a symlink on Windows because symlink_dir \
             needs Developer Mode while a junction needs no privilege at all</MEMORY>",
        );
        // Wave 2 accounts for its own duty, leaves its own lesson — and MENTIONS
        // a duty of wave 1's that wave 1 never accounted for. A sibling saying so
        // is not wave 1 checking the world.
        child_returns(
            root,
            spec,
            2,
            "RO-2.1 — ran the collector against an orphan and it removed it. \
             RO-1.2 was handled over in wave 1 as far as I could tell.\n\
             <MEMORY>Chose the owner-pid probe over an age threshold because a degraded probe \
             answers false and must never authorise a removal</MEMORY>",
        );

        // Side one: the account the wave itself gave reaches the wave's record.
        let w1 = unaccounted_reality_obligations(root, spec, 1);
        assert!(
            !w1.iter().any(|f| f.starts_with("RO-1.1")),
            "the wave's own return accounts for RO-1.1: {w1:?}"
        );
        assert_eq!(w1.len(), 1, "exactly the unaccounted duty stays open: {w1:?}");
        assert!(
            w1[0].starts_with("RO-1.2"),
            "a SIBLING's mention of RO-1.2 must not discharge wave 1's duty: {w1:?}"
        );
        assert!(
            unaccounted_reality_obligations(root, spec, 2).is_empty(),
            "wave 2's own return accounts for wave 2's duty"
        );

        // Side two: the harvest. Each close takes only the lesson its own wave
        // emitted — the reported path is the wave's own record, not the round's.
        let m1 = materialize_wave_memory(root, spec, 1);
        assert_eq!(m1.len(), 1, "wave 1's close harvested a sibling's memory: {m1:?}");
        assert!(m1[0].ends_with("-wave1.md"), "and it must be its own: {m1:?}");
        let m2 = materialize_wave_memory(root, spec, 2);
        assert_eq!(m2.len(), 1, "wave 2's own lesson was taken by wave 1: {m2:?}");
        assert!(m2[0].ends_with("-wave2.md"), "and it must be its own: {m2:?}");
    }

    /// AC-2: an account of `RO-3.10` leaves `RO-3.1` unaccounted.
    ///
    /// The match was a bare `contains`, and obligation ids share prefixes by
    /// construction — the tenth duty of a wave spells the first one inside
    /// itself. So a wave that reported ONE duty silently discharged another, and
    /// the report of a duty became the discharge of a duty nobody checked.
    ///
    /// Both directions in one test: the id that WAS accounted for must still
    /// clear, or a matcher that answers "no" to everything would pass.
    #[test]
    fn obligation_match_is_by_id_not_substring() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let spec = "prefix-collision-spec";
        anchored_spec(root, spec);
        let wave_dir = root.join(".claude/spec").join(spec).join("wave-3-impl");
        std::fs::create_dir_all(&wave_dir).expect("wave dir");
        std::fs::write(
            wave_dir.join("spec.md"),
            "# W3\n\n## Reality Obligations\n\n\
             - **RO-3.1** — read the provider's doc for the retry semantics\n\
             - **RO-3.10** — read one stored row and confirm the status column\n",
        )
        .expect("wave spec");

        // The returning agent accounted for the TENTH duty only.
        seed_agent_stop(root, spec, "Done. RO-3.10: the row's status column reads `paid`.");

        let flagged = unaccounted_reality_obligations(root, spec, 3);
        assert_eq!(flagged.len(), 1, "exactly one duty stays open: {flagged:?}");
        assert!(
            flagged[0].starts_with("RO-3.1 "),
            "RO-3.1 was discharged by the substring inside RO-3.10: {flagged:?}"
        );
        assert!(
            !flagged.iter().any(|f| f.starts_with("RO-3.10")),
            "the duty the wave DID account for must not be flagged: {flagged:?}"
        );

        // The boundary rule itself, on both sides of a hit.
        assert!(accounts_for("checked RO-3.1 today", "RO-3.1"));
        assert!(accounts_for("RO-3.1", "RO-3.1"));
        assert!(accounts_for("(RO-3.1)", "RO-3.1"));
        assert!(!accounts_for("checked RO-3.10 today", "RO-3.1"));
        assert!(!accounts_for("checked XRO-3.1 today", "RO-3.1"));
        assert!(!accounts_for("", "RO-3.1"));
        assert!(!accounts_for("RO-3.1", ""));
        // A hit inside a longer id does not hide a real one later in the text.
        assert!(accounts_for("RO-3.10 and also RO-3.1 itself", "RO-3.1"));
        // Multi-byte neighbours are not id characters and never split a slice.
        assert!(accounts_for("verifiquei «RO-3.1» hoje", "RO-3.1"));
    }
}
