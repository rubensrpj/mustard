//! `mustard-rt run mark-checklist-item` — a port of
//! `scripts/mark-checklist-item.js`, extended for the events-first checklist.
//!
//! **Meta-first:** the canonical home of trackable items is the
//! `meta.json#checklist` sidecar (seeded per wave by `wave-scaffold`). The
//! item is located by `--item` against the label / path / basename across the
//! resolved spec's own sidecar plus every `wave-N-*` subdir sidecar, flipped
//! to `done: true` (idempotent), and a `checklist.item.marked` event is
//! emitted to the per-spec NDJSON sink. The markdown `## Checklist` section
//! remains the legacy fallback for un-migrated specs.
//!
//! Output (stdout): one line — `marked` | `already-marked` | `error: <reason>`.
//! Exit codes: 0 success/no-op, 1 not-found/no-section/not-located, 2 bad args.

use mustard_core::domain::model::event::{
    Actor, ActorKind, ChecklistItemMarkedPayload, EVENT_CHECKLIST_ITEM_MARKED, HarnessEvent,
    SCHEMA_VERSION,
};
use mustard_core::domain::spec::contract::ChecklistItem;
use mustard_core::io::fs;
use mustard_core::time::now_iso8601;
use mustard_core::{ClaudePaths, Meta, read_meta, write_meta};
use std::path::{Path, PathBuf};

/// Print `error: <msg>` and exit with `code`.
fn die(code: i32, msg: &str) -> ! {
    println!("error: {msg}");
    std::process::exit(code);
}

/// Resolve a spec argument to a `spec.md` path. Accepts an absolute `.md`
/// path, an absolute directory (e.g. a `wave-N-{role}` dir), a bare slug under
/// `.claude/spec/`, or a directory relative to `--cwd` / the process cwd.
fn resolve_spec_path(spec: &str, cwd: &Path) -> Option<PathBuf> {
    let p = Path::new(spec);
    if p.is_absolute() {
        if p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("md")) && p.exists() {
            return Some(p.to_path_buf());
        }
        let as_dir = p.join("spec.md");
        if as_dir.exists() {
            return Some(as_dir);
        }
    }
    if let Ok(paths) = ClaudePaths::for_project(cwd) {
        if let Ok(spec_paths) = paths.for_spec(spec) {
            let flat = spec_paths.spec_md_path();
            if flat.exists() {
                return Some(flat);
            }
        }
    }
    // Relative directory — against the resolved cwd first, then the process cwd
    // (the historical behaviour).
    let from_cwd = cwd.join(spec).join("spec.md");
    if from_cwd.exists() {
        return Some(from_cwd);
    }
    let as_dir = Path::new(spec).join("spec.md");
    if as_dir.exists() {
        return Some(as_dir);
    }
    None
}

// ---------------------------------------------------------------------------
// Meta-first marking — `meta.json#checklist` is the canonical home
// ---------------------------------------------------------------------------

/// Parse a `wave-{n}-{role}` directory name into its wave number.
pub(crate) fn wave_number_of(dir_name: &str) -> Option<u32> {
    dir_name.strip_prefix("wave-")?.split('-').next()?.parse::<u32>().ok()
}

/// Spec slug + wave number for a `spec.md` (or `meta.json` sibling) path. A
/// wave directory (`wave-{n}-{role}/`) attributes to its PARENT slug with the
/// parsed wave number; a top-level spec dir attributes to itself with wave 0.
fn spec_and_wave_of(spec_path: &Path) -> (String, u32) {
    let dir = spec_path.parent();
    let dir_name = dir
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if let Some(n) = wave_number_of(dir_name) {
        let parent = dir
            .and_then(|d| d.parent())
            .and_then(|d| d.file_name())
            .and_then(|nm| nm.to_str())
            .unwrap_or(dir_name);
        (parent.to_string(), n)
    } else {
        (dir_name.to_string(), 0)
    }
}

/// Emit the `checklist.item.marked` harness event to the per-spec NDJSON sink.
/// Best-effort telemetry — never affects the caller's outcome (fail-open).
/// Shared with the `checklist-auto-mark` hook (`hooks/write/post_edit.rs`).
pub(crate) fn emit_item_marked(
    project_dir: &str,
    actor_kind: ActorKind,
    actor_id: &str,
    spec: &str,
    wave: u32,
    item: &ChecklistItem,
) {
    let payload = ChecklistItemMarkedPayload {
        spec: spec.to_string(),
        wave,
        item: item.label.clone(),
        path: item.path.clone(),
    };
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        // The router resolves the real session id (env / newest session dir).
        session_id: "unknown".to_string(),
        wave,
        actor: Actor {
            kind: actor_kind,
            id: Some(actor_id.to_string()),
            actor_type: None,
        },
        event: EVENT_CHECKLIST_ITEM_MARKED.to_string(),
        payload: serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(project_dir, &event);
}

/// `true` when a checklist item matches the `--item` needle: a substring of
/// the label (the historical markdown contract), or a normalised exact /
/// segment-suffix / basename match against the item's path anchor.
fn item_matches(item: &ChecklistItem, needle: &str) -> bool {
    if item.label.contains(needle) {
        return true;
    }
    let Some(path) = item.path.as_deref() else {
        return false;
    };
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let n = needle.replace('\\', "/").to_ascii_lowercase();
    if n.is_empty() {
        return false;
    }
    p == n || p.ends_with(&format!("/{n}")) || p.rsplit('/').next() == n.rsplit('/').next()
}

/// The candidate meta-bearing dirs for marking: the spec's own dir first, then
/// every `wave-N-*` subdir (sorted) — given a wave-plan PARENT, the item lives
/// in one of the waves' sidecars.
fn meta_candidate_dirs(spec_dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![spec_dir.to_path_buf()];
    if let Ok(entries) = fs::read_dir(spec_dir) {
        let mut waves: Vec<PathBuf> = entries
            .into_iter()
            .filter(|e| e.path.is_dir() && e.file_name.starts_with("wave-"))
            .map(|e| e.path)
            .collect();
        waves.sort();
        out.extend(waves);
    }
    out
}

/// Outcome of the meta-first marking attempt.
enum MetaMark {
    /// An un-done item matched and was flipped (event emitted).
    Marked,
    /// The only matches were already done — idempotent no-op.
    AlreadyMarked,
    /// A match was found but the sidecar write failed.
    Error(String),
}

/// Flip `checklist[idx]` to done in `dir`'s sidecar, persist atomically, and
/// emit `checklist.item.marked`. The caller has verified `idx` is in bounds
/// and the item is not yet done.
fn flip_and_emit(cwd: &Path, dir: &Path, meta: &mut Meta, idx: usize) -> Result<(), String> {
    meta.checklist[idx].done = true;
    write_meta(&dir.join("meta.json"), meta)
        .map_err(|e| format!("cannot write meta.json: {e}"))?;
    let (slug, wave) = spec_and_wave_of(&dir.join("spec.md"));
    emit_item_marked(
        &cwd.to_string_lossy(),
        ActorKind::Cli,
        "mark-checklist-item",
        &slug,
        wave,
        &meta.checklist[idx],
    );
    Ok(())
}

/// Try the meta-first marking across the spec's own dir + its wave subdirs.
/// Returns `None` when no sidecar checklist carried a match at all — the
/// caller then falls back to the legacy markdown `## Checklist` path.
fn try_mark_in_metas(cwd: &Path, spec_dir: &Path, needle: &str) -> Option<MetaMark> {
    let mut already = false;
    for dir in meta_candidate_dirs(spec_dir) {
        let Some(mut meta) = read_meta(&dir.join("meta.json")) else {
            continue;
        };
        if meta.checklist.is_empty() {
            continue;
        }
        if let Some(i) = meta
            .checklist
            .iter()
            .position(|it| !it.done && item_matches(it, needle))
        {
            return Some(match flip_and_emit(cwd, &dir, &mut meta, i) {
                Ok(()) => MetaMark::Marked,
                Err(e) => MetaMark::Error(e),
            });
        }
        already = already || meta.checklist.iter().any(|it| it.done && item_matches(it, needle));
    }
    already.then_some(MetaMark::AlreadyMarked)
}

/// Locate the `## Checklist` section. Returns `(start_idx, end_idx)` where
/// `start_idx` is the first body line after the header and `end_idx` is the
/// next `## ` header (exclusive) or end-of-file.
fn find_checklist_section(lines: &[&str]) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| {
        // `^##\s+Checklist\b`
        l.strip_prefix("##")
            .is_some_and(|r| {
                let t = r.trim_start_matches([' ', '\t']);
                t.len() != r.len()
                    && {
                        let lower = t.to_lowercase();
                        lower.strip_prefix("checklist").is_some_and(|tail| {
                            tail.chars()
                                .next()
                                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
                        })
                    }
            })
    })? + 1;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start) {
        // `^##\s`
        if l.strip_prefix("##").is_some_and(|r| r.starts_with([' ', '\t'])) {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// Parsed checkbox line: `(prefix, state, gap, text)`.
struct Checkbox<'a> {
    prefix: &'a str,
    state: char,
    gap: &'a str,
    text: &'a str,
}

/// Parse a `^(\s*-\s+)\[([ xX])\](\s+)(.*)$` checkbox line.
fn parse_checkbox(line: &str) -> Option<Checkbox<'_>> {
    let trimmed_start = line.len() - line.trim_start().len();
    let after_ws = &line[trimmed_start..];
    let rest = after_ws.strip_prefix('-')?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let dash_gap_len = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let prefix_end = trimmed_start + 1 + dash_gap_len;
    let body = &line[prefix_end..];
    let inner = body.strip_prefix('[')?;
    let state = inner.chars().next()?;
    if !matches!(state, ' ' | 'x' | 'X') {
        return None;
    }
    let after_state = &inner[state.len_utf8()..];
    let after_bracket = after_state.strip_prefix(']')?;
    if after_bracket.is_empty() || !after_bracket.starts_with([' ', '\t']) {
        return None;
    }
    let gap_len = after_bracket.len() - after_bracket.trim_start_matches([' ', '\t']).len();
    let text = &after_bracket[gap_len..];
    Some(Checkbox {
        prefix: &line[..prefix_end],
        state,
        gap: &after_bracket[..gap_len],
        text,
    })
}

/// Dispatch `mustard-rt run mark-checklist-item`.
pub fn run(spec: Option<&str>, item: Option<&str>, line: Option<usize>, cwd_arg: Option<&str>) {
    let Some(spec) = spec else {
        die(2, "--spec is required");
    };
    if item.is_none() && line.is_none() {
        die(2, "either --item or --line is required");
    }
    if item.is_some() && line.is_some() {
        die(2, "--item and --line are mutually exclusive");
    }

    let cwd = cwd_arg
        .map_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), PathBuf::from);
    let Some(spec_path) = resolve_spec_path(spec, &cwd) else {
        die(1, &format!("spec not found: {spec}"));
    };

    // Meta-first: the `meta.json#checklist` sidecar is the canonical home of
    // trackable items. Only when no sidecar checklist carries the item does
    // the legacy markdown `## Checklist` path below run.
    if let Some(spec_dir) = spec_path.parent().map(Path::to_path_buf) {
        if let Some(n) = line {
            // `--line N` indexes the resolved spec's OWN meta checklist
            // (1-based) — a consolidated index across waves would be ambiguous.
            if let Some(mut meta) =
                read_meta(&spec_dir.join("meta.json")).filter(|m| !m.checklist.is_empty())
            {
                if n == 0 || n > meta.checklist.len() {
                    die(
                        1,
                        &format!(
                            "--line {n} is outside the meta checklist (1-{})",
                            meta.checklist.len()
                        ),
                    );
                }
                if meta.checklist[n - 1].done {
                    println!("already-marked");
                    std::process::exit(0);
                }
                match flip_and_emit(&cwd, &spec_dir, &mut meta, n - 1) {
                    Ok(()) => {
                        println!("marked");
                        std::process::exit(0);
                    }
                    Err(e) => die(1, &e),
                }
            }
        } else if let Some(outcome) = try_mark_in_metas(&cwd, &spec_dir, item.unwrap_or("")) {
            match outcome {
                MetaMark::Marked => {
                    println!("marked");
                    std::process::exit(0);
                }
                MetaMark::AlreadyMarked => {
                    println!("already-marked");
                    std::process::exit(0);
                }
                MetaMark::Error(e) => die(1, &e),
            }
        }
    }

    let raw = match fs::read_to_string(&spec_path) {
        Ok(r) => r,
        Err(e) => die(1, &format!("cannot read spec: {e}")),
    };
    let mut lines: Vec<String> = raw.split('\n').map(String::from).collect();
    let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let Some((start, end)) = find_checklist_section(&line_refs) else {
        die(1, "no `## Checklist` section in spec");
    };

    let target_idx: usize = if let Some(n) = line {
        let idx = n.wrapping_sub(1);
        if n == 0 || idx < start || idx >= end {
            die(
                1,
                &format!(
                    "--line {n} is outside the Checklist section (lines {}-{end})",
                    start + 1
                ),
            );
        }
        if parse_checkbox(&lines[idx]).is_none() {
            die(1, &format!("--line {n} is not a checkbox"));
        }
        idx
    } else {
        let item = item.unwrap_or("");
        let mut found: Option<usize> = None;
        for (i, line) in lines.iter().enumerate().take(end).skip(start) {
            if let Some(cb) = parse_checkbox(line) {
                if cb.state == ' ' && cb.text.contains(item) {
                    found = Some(i);
                    break;
                }
            }
        }
        match found {
            Some(i) => i,
            None => {
                // Idempotency: was the only match already `[x]`?
                for line in lines.iter().take(end).skip(start) {
                    if let Some(cb) = parse_checkbox(line) {
                        if (cb.state == 'x' || cb.state == 'X') && cb.text.contains(item) {
                            println!("already-marked");
                            std::process::exit(0);
                        }
                    }
                }
                die(1, &format!("no `- [ ]` item matching: {item}"));
            }
        }
    };

    let new_line = {
        let Some(cb) = parse_checkbox(&lines[target_idx]) else { die(1, "target line is not a checkbox") };
        if cb.state == 'x' || cb.state == 'X' {
            println!("already-marked");
            std::process::exit(0);
        }
        format!("{}[x]{}{}", cb.prefix, cb.gap, cb.text)
    };
    lines[target_idx] = new_line;

    if let Err(e) = fs::write_atomic(&spec_path, lines.join("\n").as_bytes()) {
        die(1, &format!("cannot write spec: {e}"));
    }
    println!("marked");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_spec(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    #[test]
    fn parses_checkbox_lines() {
        let cb = parse_checkbox("  - [ ] do the thing").unwrap();
        assert_eq!(cb.state, ' ');
        assert_eq!(cb.text, "do the thing");
        assert!(parse_checkbox("- not a checkbox").is_none());
    }

    #[test]
    fn finds_checklist_section() {
        let lines = vec!["# Spec", "## Checklist", "- [ ] a", "## Next"];
        let (start, end) = find_checklist_section(&lines).unwrap();
        assert_eq!((start, end), (2, 3));
    }

    #[test]
    fn marks_item_by_substring() {
        let (_d, path) = write_spec("## Checklist\n- [ ] alpha\n- [ ] beta\n");
        let mut lines: Vec<String> =
            std::fs::read_to_string(&path).unwrap().split('\n').map(String::from).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (start, end) = find_checklist_section(&refs).unwrap();
        let mut idx = None;
        for i in start..end {
            if let Some(cb) = parse_checkbox(&lines[i]) {
                if cb.state == ' ' && cb.text.contains("beta") {
                    idx = Some(i);
                }
            }
        }
        let i = idx.unwrap();
        let cb = parse_checkbox(&lines[i]).unwrap();
        lines[i] = format!("{}[x]{}{}", cb.prefix, cb.gap, cb.text);
        assert_eq!(lines[i], "- [x] beta");
    }

    // --- meta-first marking (checklist-progresso-por-onda W2) ---------------

    #[test]
    fn item_matches_label_path_and_basename() {
        let it = ChecklistItem {
            label: "src/api/handler.rs".to_string(),
            path: Some("src/api/handler.rs".to_string()),
            done: false,
        };
        assert!(item_matches(&it, "handler.rs"), "basename");
        assert!(item_matches(&it, "api/handler.rs"), "segment suffix");
        assert!(item_matches(&it, "src/api/handler.rs"), "exact path");
        assert!(item_matches(&it, "handler"), "label substring");
        assert!(!item_matches(&it, "other.rs"));
    }

    #[test]
    fn spec_and_wave_attribution() {
        let wave = Path::new("/p/.claude/spec/demo/wave-3-rt/spec.md");
        assert_eq!(spec_and_wave_of(wave), ("demo".to_string(), 3));
        let top = Path::new("/p/.claude/spec/demo/spec.md");
        assert_eq!(spec_and_wave_of(top), ("demo".to_string(), 0));
        assert_eq!(wave_number_of("wave-12-frontend"), Some(12));
        assert_eq!(wave_number_of("not-a-wave"), None);
    }

    /// Meta-first end-to-end: a wave-plan PARENT slug locates the item inside
    /// the WAVE's `meta.json#checklist`, flips it (idempotently) and emits the
    /// `checklist.item.marked` NDJSON event under the spec's `.events/` sink.
    #[test]
    fn marks_wave_meta_item_and_emits_event() {
        let project = tempdir().unwrap();
        let paths = ClaudePaths::for_project(project.path()).unwrap();
        let sp = paths.for_spec("demo").unwrap();
        let spec_dir = sp.dir().to_path_buf();
        let wave_dir = spec_dir.join("wave-1-rt");
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Demo\n").unwrap();
        std::fs::write(wave_dir.join("spec.md"), "# wave-1-rt\n").unwrap();
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","parent":"demo","checklist":[{"label":"src/api/handler.rs","path":"src/api/handler.rs","done":false}]}"#,
        )
        .unwrap();

        let outcome = try_mark_in_metas(project.path(), &spec_dir, "handler.rs");
        assert!(matches!(outcome, Some(MetaMark::Marked)), "first call marks");
        let meta = read_meta(&wave_dir.join("meta.json")).unwrap();
        assert!(meta.checklist[0].done, "done flipped in the wave sidecar");

        // Idempotent: a second call is a no-op `already-marked`.
        let again = try_mark_in_metas(project.path(), &spec_dir, "handler.rs");
        assert!(matches!(again, Some(MetaMark::AlreadyMarked)));

        // The NDJSON event landed under the spec's events sink with wave=1.
        let events_dir = sp.events_dir();
        assert!(events_dir.exists(), "events dir must exist after the emit");
        let mut found = false;
        for f in std::fs::read_dir(&events_dir).unwrap() {
            let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
            for l in body.lines() {
                if l.contains("\"event\":\"checklist.item.marked\"") {
                    assert!(l.contains("src/api/handler.rs"), "{l}");
                    found = true;
                }
            }
        }
        assert!(found, "checklist.item.marked NDJSON line must be present");
    }

    /// No sidecar checklist anywhere → `None` (the legacy markdown fallback
    /// stays reachable for un-migrated specs).
    #[test]
    fn meta_mark_falls_through_without_sidecar_checklist() {
        let project = tempdir().unwrap();
        let spec_dir = project.path().join(".claude").join("spec").join("legacy");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# L\n\n## Checklist\n- [ ] a\n").unwrap();
        // A sidecar WITHOUT a checklist does not capture the marking.
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active"}"#,
        )
        .unwrap();
        assert!(try_mark_in_metas(project.path(), &spec_dir, "a").is_none());
    }
}
