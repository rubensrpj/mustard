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
        base: None,
    });

    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());

    // 2. Materialize this wave's qualifying lessons as spec-memory files, so
    //    the NEXT round's render can pick them up — fail-open.
    let memories = materialize_wave_memory(&cwd, spec);

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
/// The wave's own recorded return: the `agent.stop` telemetry (whose payload
/// carries the returning agent's report) and the `decision` events harvested
/// from its `<MEMORY>` blocks, both on the spec's OWN NDJSON log. No new channel
/// and no new flag — the dispatch prompt instructs the agent to account for each
/// duty by its id, and the ids carry the wave number
/// ([`wave_scaffold::parse_reality_obligations`]'s writer twin), so one wave's
/// report can never clear another wave's duty even though the log is per-spec.
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
    let recorded = recorded_return_text(&spec_paths.events_dir());
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

/// Everything the waves of this spec recorded on their way back, concatenated —
/// the substrate an obligation id is looked for in.
///
/// Only the two event kinds that carry an agent's OWN words are read: `agent.stop`
/// (the returning report) and `decision` (a harvested `<MEMORY>` block). The
/// whole payload is serialised rather than one named field, because the report
/// key has moved before and a missed rename would silently turn every duty into
/// an unaccounted one. Fail-open: an unreadable log yields "".
fn recorded_return_text(events_dir: &Path) -> String {
    mustard_core::view::projection::read_harness_events_from_ndjson_dir(events_dir)
        .iter()
        .filter(|e| e.event == "agent.stop" || e.event == "decision")
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
/// So the sweep stays — whichever `wave-done` gets there first materializes
/// everything pending, which is what keeps a lesson from being lost when its own
/// wave's close already ran — but the WAVE in the file comes from the event.
/// When the event carries none, the file says `unknown` rather than borrowing
/// the closing wave's number: the harness must not assert an attribution it
/// never established.
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
/// Takes no wave: the closing wave is exactly the number this must NOT use.
fn materialize_wave_memory(cwd: &Path, spec: &str) -> Vec<String> {
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

        let written = materialize_wave_memory(root, spec);
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
        let again = materialize_wave_memory(root, spec);
        assert!(again.is_empty(), "already materialized: {again:?}");
        assert_eq!(memory_files(root, spec), names, "no duplicate, no re-attribution");
    }

    /// AC-1: a ROUND closes several waves, and each one's memory is written
    /// under the wave that emitted it — none stamped with a sibling's number,
    /// and none dropped.
    ///
    /// The measured defect this locks: `wave-done` attributed every pending
    /// lesson to the wave closing NOW. Because a round's waves all close in
    /// sequence, the FIRST close of the round swept its siblings' decisions too
    /// and filed them under its own number. Two rounds of the run that produced
    /// this spec left four memory files, two of them naming a wave that had not
    /// written them.
    ///
    /// So the test drives ONE close over a log that already carries three waves'
    /// decisions — exactly the state the first close of a round sees.
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

        // The first close of the round. It sees all three, and it must not keep
        // any of them for itself.
        let written = materialize_wave_memory(root, spec);
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

        // The closes of the round's other waves add nothing and re-file nothing.
        assert!(materialize_wave_memory(root, spec).is_empty(), "idempotent");
        assert_eq!(memory_files(root, spec), names, "no re-attribution");
    }

    /// The other half of AC-1: a decision whose event recorded NO wave is still
    /// materialized — never dropped — and the file says `unknown` instead of
    /// borrowing the closing wave's number. Asserting the drop alone would pass
    /// on a writer that simply refused every unattributed lesson.
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

        let written = materialize_wave_memory(root, spec);
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
            materialize_wave_memory(root, spec).is_empty(),
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
        let written = materialize_wave_memory(root, spec);
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
