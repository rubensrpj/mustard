//! `--emit ref` machinery: the dispatch stub, its deterministic on-disk path,
//! and the FNV-1a key that names spec-less dispatches.
//!
//! `--emit ref` writes the full rendered prompt to a file under `.claude/` and
//! returns a 2-line stub in its place, so the full prompt never transits the
//! orchestrator's context (historically it was paid twice: once as command
//! stdout, once again in the Task dispatch). The `subagent_inject` PreToolUse
//! hook greps the stub for [`PROMPT_REF_MARKER`] and expands it back inside the
//! dispatch.

use super::RenderMode;
use mustard_core::io::fs as mfs;
use std::path::Path;

/// Marker prefix of the stub's first line. `subagent_inject` greps the Task
/// prompt for this exact prefix to locate the file to expand.
pub const PROMPT_REF_MARKER: &str = "MUSTARD-PROMPT-REF:";

/// Write `rendered` to its deterministic dispatch file and return the 2-line
/// stub that stands in for it. Fail-open both ways: an empty render returns
/// the empty string (the historical print-nothing behaviour), and a write
/// failure degrades to the full inline prompt — the dispatch must never be
/// lost to a missing directory or a locked file.
#[allow(clippy::too_many_arguments)] // mirrors render_prompt_at's surface
pub(crate) fn prompt_ref_stub(
    project: &Path,
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    task_filter: Option<&str>,
    task_text: Option<&str>,
    rendered: &str,
) -> String {
    let rel = prompt_ref_rel_path(spec, wave, role, subproject, mode, task_filter, task_text);
    write_prompt_ref(project, &rel, rendered)
}

/// Write `rendered` to project-relative `rel` and return the 2-line dispatch
/// stub that stands in for it. The shared primitive behind every `--emit ref`,
/// so the full prompt never transits the orchestrator's context. Fail-open both
/// ways: an empty render returns the empty string (the historical
/// print-nothing behaviour), and a write failure degrades to the full inline
/// prompt — the dispatch must never be lost to a missing directory or a locked
/// file.
pub(crate) fn write_prompt_ref(project: &Path, rel: &str, rendered: &str) -> String {
    if rendered.is_empty() {
        return String::new();
    }
    if mfs::write_atomic(project.join(rel), rendered.as_bytes()).is_err() {
        eprintln!("--emit ref: WARN: could not write {rel} — falling back to inline prompt");
        return rendered.to_string();
    }
    format!(
        "{PROMPT_REF_MARKER} {rel}\nDispatch stub ({} chars rendered) — pass this stub VERBATIM as the Task prompt; the PreToolUse hook expands it to the full prompt. Subagent fallback: if you are reading this line, the hook did not expand it — Read the file above and follow its content as your prompt.\n",
        rendered.chars().count()
    )
}

/// Compose [`super::render_prompt_at`] + [`prompt_ref_stub`] — the ref-mode miolo
/// reused in-process by `wave-advance`, so its dispatch items carry the cheap
/// stub instead of the full prompt (which the orchestrator would pay once in
/// the command output and again in the Task dispatch).
pub(crate) fn render_prompt_ref_at(
    project: &Path,
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
) -> String {
    let rendered =
        super::render_prompt_at(project, spec, wave, role, subproject, mode, None, None, None);
    prompt_ref_stub(project, spec, wave, role, subproject, mode, None, None, &rendered)
}

/// Deterministic project-relative path (forward slashes — survives Git Bash
/// and stays byte-stable across hosts) for a rendered dispatch prompt.
///
/// - Spec'd renders live beside the spec (cleaned up with it):
///   `.claude/spec/{spec}/.dispatch/wave-{n}-{role}[-{subproject}].{mode}.prompt.md`
///   (`n` = 0 for wave-less renders; the subproject slug disambiguates the
///   per-subproject review round, which renders the same spec/wave/role once
///   per subproject).
/// - Spec-less renders (`/task`, ANALYZE/DIAGNOSE explores) key on an FNV-1a
///   hash of the distinguishing inputs:
///   `.claude/.dispatch/{role}-{hash:016x}.prompt.md`.
fn prompt_ref_rel_path(
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    task_filter: Option<&str>,
    task_text: Option<&str>,
) -> String {
    let mode_tag = match mode {
        RenderMode::First => "first",
        RenderMode::Granular => "granular",
        RenderMode::FixLoop => "fix-loop",
    };
    let sub = subproject.to_string_lossy();
    match spec {
        Some(s) => {
            let sub_slug = path_slug(&sub);
            let sub_part = if sub_slug.is_empty() { String::new() } else { format!("-{sub_slug}") };
            format!(
                ".claude/spec/{s}/.dispatch/wave-{}-{role}{sub_part}.{mode_tag}.prompt.md",
                wave.unwrap_or(0)
            )
        }
        None => {
            let hash = fnv1a64(&[
                role,
                &sub,
                mode_tag,
                task_filter.unwrap_or(""),
                task_text.unwrap_or(""),
            ]);
            format!(".claude/.dispatch/{role}-{hash:016x}.prompt.md")
        }
    }
}

/// Filename-safe slug of a subproject path: alphanumerics and `-` kept,
/// everything else folded to `-`; the root (`.` / empty) yields the empty
/// slug (no suffix).
fn path_slug(path: &str) -> String {
    let slug: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() { String::new() } else { trimmed.to_string() }
}

/// FNV-1a 64-bit over `parts` with a separator fold between them — pure and
/// deterministic (no clock, no randomness), so the same render inputs always
/// map to the same dispatch file.
pub(crate) fn fnv1a64(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for part in parts {
        for b in part.as_bytes() {
            eat(*b);
        }
        eat(0x1f); // unit separator — "ab","c" never collides with "a","bc"
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_ref_rel_paths_are_deterministic_and_collision_free() {
        // Spec'd render: stable name from spec/wave/role/mode; the subproject
        // slug disambiguates the per-subproject review round.
        let p = prompt_ref_rel_path(
            Some("demo"), Some(2), "rt", Path::new("."), RenderMode::First, None, None,
        );
        assert_eq!(p, ".claude/spec/demo/.dispatch/wave-2-rt.first.prompt.md");
        let a = prompt_ref_rel_path(
            Some("demo"), None, "review", Path::new("apps/rt"), RenderMode::First, None, None,
        );
        let b = prompt_ref_rel_path(
            Some("demo"), None, "review", Path::new("apps/cli"), RenderMode::First, None, None,
        );
        assert_eq!(a, ".claude/spec/demo/.dispatch/wave-0-review-apps-rt.first.prompt.md");
        assert_ne!(a, b, "review round across subprojects must not collide");

        // Spec-less render: hashed on the distinguishing inputs — same input
        // → same path (resumable), different task text → different path.
        let x = prompt_ref_rel_path(
            None, None, "explore", Path::new("."), RenderMode::First, None, Some("map the slice"),
        );
        let y = prompt_ref_rel_path(
            None, None, "explore", Path::new("."), RenderMode::First, None, Some("map the slice"),
        );
        let z = prompt_ref_rel_path(
            None, None, "explore", Path::new("."), RenderMode::First, None, Some("other task"),
        );
        assert_eq!(x, y, "deterministic for identical inputs");
        assert_ne!(x, z, "task text distinguishes spec-less dispatches");
        assert!(x.starts_with(".claude/.dispatch/explore-"), "spec-less prefix: {x}");
    }

    #[test]
    fn emit_ref_writes_prompt_file_and_returns_stub() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rendered = "ROLE: impl\nfull rendered body";
        let stub = prompt_ref_stub(
            dir.path(), Some("demo"), Some(1), "rt", Path::new("."), RenderMode::First,
            None, None, rendered,
        );
        let first = stub.lines().next().expect("stub first line");
        let rel = first.strip_prefix(PROMPT_REF_MARKER).expect("marker prefix").trim();
        let on_disk = std::fs::read_to_string(dir.path().join(rel)).expect("stub file");
        assert_eq!(on_disk, rendered, "file holds the full render verbatim");
        assert!(stub.contains("VERBATIM"), "stub instructs verbatim dispatch: {stub}");
        assert!(stub.contains("Read the file"), "stub carries the subagent fallback: {stub}");

        // Empty render → empty stub (the historical print-nothing contract).
        let empty = prompt_ref_stub(
            dir.path(), Some("demo"), Some(1), "rt", Path::new("."), RenderMode::First,
            None, None, "",
        );
        assert!(empty.is_empty(), "empty render must not produce a stub: {empty}");
    }
}
