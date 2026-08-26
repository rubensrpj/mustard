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
//! **Dropping (`--drop --reason "<why>"`):** the third position. An item let
//! go on purpose is recorded as a decision — `dropped: "<reason>"` in the
//! sidecar, `- [~] … — dropped: <reason>` in the markdown — and a
//! `checklist.item.dropped` event carries the reason to the NDJSON sink. The
//! reason is mandatory: `--drop` without one is a bad-argument exit, so a
//! decision can never be recorded mutely. Dropping is terminal in both
//! directions: this command never marks a dropped item done, and never
//! re-opens it.
//!
//! Output (stdout): one line — `marked` | `already-marked` | `dropped` |
//! `already-dropped` | `error: <reason>`.
//! Exit codes: 0 success/no-op, 1 not-found/no-section/not-located, 2 bad args.

use mustard_core::domain::model::event::{
    Actor, ActorKind, ChecklistItemDroppedPayload, ChecklistItemMarkedPayload,
    EVENT_CHECKLIST_ITEM_DROPPED, EVENT_CHECKLIST_ITEM_MARKED, HarnessEvent, SCHEMA_VERSION,
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

/// Emit the `checklist.item.dropped` harness event — the decision half of the
/// pair. Best-effort telemetry, fail-open like [`emit_item_marked`]. It is a
/// separate event on purpose: a consumer counting `checklist.item.marked`
/// must not see a drop and report progress that never happened.
fn emit_item_dropped(
    project_dir: &str,
    actor_id: &str,
    spec: &str,
    wave: u32,
    item: &ChecklistItem,
    reason: &str,
) {
    let payload = ChecklistItemDroppedPayload {
        spec: spec.to_string(),
        wave,
        item: item.label.clone(),
        path: item.path.clone(),
        reason: reason.to_string(),
    };
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        // The router resolves the real session id (env / newest session dir).
        session_id: "unknown".to_string(),
        wave,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some(actor_id.to_string()),
            actor_type: None,
        },
        event: EVENT_CHECKLIST_ITEM_DROPPED.to_string(),
        payload: serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
        spec: Some(spec.to_string()),
    };
    let _ = crate::shared::events::route::emit(project_dir, &event);
}

/// `true` when a checklist item matches the `--item` needle: a substring of
/// the label (the historical markdown contract), or a normalised exact /
/// segment-suffix / basename match against the item's path anchor.
fn item_matches(item: &ChecklistItem, needle: &str) -> bool {
    if needle.trim().is_empty() {
        return false;
    }
    // The operator's needle is usually LONGER than what the item records: the
    // close gate reports the task SENTENCE from `wave-*/spec.md`, while the
    // sidecar stores a short `label` and an optional `path`. So containment is
    // tried both ways — but never rawly.
    //
    // **Every containment goes through the token boundary.** A raw
    // `needle.contains(label)` was tried first and review caught it: it ran
    // before the boundary rules and returned early, so a sentence about
    // `src/x.rs.bak` marked the item for `src/x.rs`. Marking the WRONG item is
    // the failure this function exists to avoid — the operator sees a refusal,
    // never a silent mismarking.
    if item.label.contains(needle) {
        return true;
    }
    if contains_as_token(needle, &item.label) {
        return true;
    }
    let Some(path) = item.path.as_deref() else {
        return false;
    };
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let n = needle.replace('\\', "/").to_ascii_lowercase();
    if p == n || p.ends_with(&format!("/{n}")) || p.rsplit('/').next() == n.rsplit('/').next() {
        return true;
    }
    // The sentence names the file either in full — "criar apps/rt/…/x.rs no
    // molde de …" — or by its BASENAME alone: "em session_start_inject.rs:
    // quando source é compact…". Seven of fifteen real items used the short
    // spelling. Both go through the same boundary.
    if contains_as_token(&n, &p) {
        return true;
    }
    match p.rsplit('/').next().filter(|b| !b.is_empty()) {
        Some(base) => contains_as_token(&n, base),
        None => false,
    }
}

/// Does `haystack` contain `needle` as a WHOLE token?
///
/// A filename is a token, not a substring: `x.rs` must not match a sentence
/// about `prefix_x.rs` or `x.rs.bak`, because marking the wrong checklist item
/// is worse than refusing — a refusal is visible, a wrong mark is not.
///
/// The two sides are deliberately asymmetric. A character before the match that
/// could continue a name (`_`, `-`, alphanumeric, `/`) closes it; so does one
/// after. A dot AFTER also closes it, because `x.rs.bak` is a different file —
/// but a dot BEFORE is ordinary punctuation and does not.
///
/// An empty needle matches nothing: it would otherwise match every item.
fn contains_as_token(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    if needle.is_empty() {
        return false;
    }
    haystack.match_indices(needle).any(|(i, _)| {
        let before_ok = i == 0
            || !haystack[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'));
        let after = i + needle.len();
        let after_ok = after >= haystack.len()
            || !haystack[after..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'));
        before_ok && after_ok
    })
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

/// What the caller asked the marker to do with the located item.
#[derive(Debug, Clone, Copy)]
enum Move<'a> {
    /// Flip an open item to done.
    Mark,
    /// Record an open item as dropped on purpose, with the stated reason.
    /// There is no reason-less variant — the type refuses a mute drop.
    Drop(&'a str),
}

/// Outcome of the meta-first attempt.
#[derive(Debug)]
enum MetaMark {
    /// An open item matched and was flipped done (event emitted).
    Marked,
    /// The only matches were already done — idempotent no-op.
    AlreadyMarked,
    /// An open item matched and was recorded as dropped (event emitted).
    Dropped,
    /// The only matches were already dropped — idempotent no-op, and the
    /// refusal that keeps a decision from being turned back into a task.
    AlreadyDropped,
    /// A match was found but the move was refused, or the sidecar write failed.
    Error(String),
}

/// Apply `mv` to `checklist[idx]` in `dir`'s sidecar, persist atomically, and
/// emit the matching event. The caller has verified `idx` is in bounds and the
/// item is open.
fn apply_and_emit(
    cwd: &Path,
    dir: &Path,
    meta: &mut Meta,
    idx: usize,
    mv: Move<'_>,
) -> Result<(), String> {
    match mv {
        Move::Mark => meta.checklist[idx].done = true,
        Move::Drop(reason) => meta.checklist[idx].dropped = Some(reason.to_string()),
    }
    write_meta(&dir.join("meta.json"), meta)
        .map_err(|e| format!("cannot write meta.json: {e}"))?;
    let (slug, wave) = spec_and_wave_of(&dir.join("spec.md"));
    let project = cwd.to_string_lossy();
    match mv {
        Move::Mark => emit_item_marked(
            &project,
            ActorKind::Cli,
            "mark-checklist-item",
            &slug,
            wave,
            &meta.checklist[idx],
        ),
        Move::Drop(reason) => emit_item_dropped(
            &project,
            "mark-checklist-item",
            &slug,
            wave,
            &meta.checklist[idx],
            reason,
        ),
    }
    Ok(())
}

/// Try the meta-first move across the spec's own dir + its wave subdirs.
/// Returns `None` when no sidecar checklist carried a match at all — the
/// caller then falls back to the legacy markdown `## Checklist` path.
///
/// Only an OPEN item is ever moved. A dropped item is invisible to both moves:
/// marking it would resurrect a decision as progress, and dropping it twice
/// would overwrite the first reason.
fn try_move_in_metas(
    cwd: &Path,
    spec_dir: &Path,
    needle: &str,
    mv: Move<'_>,
) -> Option<MetaMark> {
    let mut already_done = false;
    let mut already_dropped = false;
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
            .position(|it| it.is_open() && item_matches(it, needle))
        {
            return Some(match (apply_and_emit(cwd, &dir, &mut meta, i, mv), mv) {
                (Ok(()), Move::Mark) => MetaMark::Marked,
                (Ok(()), Move::Drop(_)) => MetaMark::Dropped,
                (Err(e), _) => MetaMark::Error(e),
            });
        }
        for it in &meta.checklist {
            if !item_matches(it, needle) {
                continue;
            }
            already_dropped = already_dropped || it.is_dropped();
            already_done = already_done || (it.done && !it.is_dropped());
        }
    }
    match mv {
        // A mark request satisfied by an already-done item is the historical
        // idempotent no-op; when the only match is a DROPPED item, say so
        // instead of flipping it — that is the resurrection this refuses.
        Move::Mark if already_done => Some(MetaMark::AlreadyMarked),
        Move::Mark if already_dropped => Some(MetaMark::AlreadyDropped),
        Move::Drop(_) if already_dropped => Some(MetaMark::AlreadyDropped),
        // Dropping finished work would rewrite history: the work exists.
        Move::Drop(_) if already_done => Some(MetaMark::Error(format!(
            "cannot drop an item that is already done: {needle}"
        ))),
        _ => None,
    }
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

/// Parsed checkbox line: `(prefix, state, gap, text)`. `state` is one of
/// `' '` (open), `'x'`/`'X'` (done), `'~'` (dropped on purpose — the text then
/// carries the ` — dropped: <reason>` tail).
struct Checkbox<'a> {
    prefix: &'a str,
    state: char,
    gap: &'a str,
    text: &'a str,
}

impl Checkbox<'_> {
    /// `true` when the line records work someone let go on purpose.
    const fn is_dropped(&self) -> bool {
        self.state == '~'
    }

    /// `true` when the line records finished work.
    const fn is_done(&self) -> bool {
        self.state == 'x' || self.state == 'X'
    }
}

/// Parse a `^(\s*-\s+)\[([ xX~])\](\s+)(.*)$` checkbox line.
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
    if !matches!(state, ' ' | 'x' | 'X' | '~') {
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

/// Resolve the requested move from the two drop arguments, refusing a drop
/// that states no reason (exit 2, before anything is read or written).
///
/// This is where "cannot be written without a stated reason" is enforced for
/// the caller: a `--drop` with a blank reason never reaches the sidecar, and
/// a `--reason` without `--drop` is a mistake worth naming rather than
/// silently ignoring.
fn resolve_move(drop: bool, reason: Option<&str>) -> Move<'_> {
    let stated = reason.map(str::trim).filter(|r| !r.is_empty());
    match (drop, stated) {
        (true, Some(r)) => Move::Drop(r),
        (true, None) => die(
            2,
            "--drop requires --reason \"<why>\": a dropped item is a decision, \
             and a decision without a stated reason is indistinguishable from \
             a forgotten task",
        ),
        (false, Some(_)) => die(2, "--reason is only meaningful with --drop"),
        (false, None) => Move::Mark,
    }
}

/// Dispatch `mustard-rt run mark-checklist-item`.
pub fn run(
    spec: Option<&str>,
    item: Option<&str>,
    line: Option<usize>,
    cwd_arg: Option<&str>,
    drop: bool,
    reason: Option<&str>,
) {
    let Some(spec) = spec else {
        die(2, "--spec is required");
    };
    if item.is_none() && line.is_none() {
        die(2, "either --item or --line is required");
    }
    if item.is_some() && line.is_some() {
        die(2, "--item and --line are mutually exclusive");
    }
    let mv = resolve_move(drop, reason);

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
                let target = &meta.checklist[n - 1];
                // Terminal positions answer without moving: a dropped item is
                // never re-opened, and finished work is never dropped.
                if target.is_dropped() {
                    println!("already-dropped");
                    std::process::exit(0);
                }
                if target.done {
                    match mv {
                        Move::Mark => {
                            println!("already-marked");
                            std::process::exit(0);
                        }
                        Move::Drop(_) => die(
                            1,
                            &format!("cannot drop an item that is already done: --line {n}"),
                        ),
                    }
                }
                match (apply_and_emit(&cwd, &spec_dir, &mut meta, n - 1, mv), mv) {
                    (Ok(()), Move::Mark) => {
                        println!("marked");
                        std::process::exit(0);
                    }
                    (Ok(()), Move::Drop(_)) => {
                        println!("dropped");
                        std::process::exit(0);
                    }
                    (Err(e), _) => die(1, &e),
                }
            }
        } else if let Some(outcome) = try_move_in_metas(&cwd, &spec_dir, item.unwrap_or(""), mv) {
            match outcome {
                MetaMark::Marked => {
                    println!("marked");
                    std::process::exit(0);
                }
                MetaMark::AlreadyMarked => {
                    println!("already-marked");
                    std::process::exit(0);
                }
                MetaMark::Dropped => {
                    println!("dropped");
                    std::process::exit(0);
                }
                MetaMark::AlreadyDropped => {
                    println!("already-dropped");
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
        // Reached only after the wave sidecars were searched and nothing
        // matched, so blaming the missing section describes the LAST thing
        // tried rather than what actually failed. A wave-plan spec never has
        // this section — its items live in the waves — and a reader sent to
        // look for it finds nothing and no next step.
        die(
            1,
            &format!(
                "no checklist item matches {:?}. The spec's own `## Checklist` section is \
                 absent (normal for a wave plan — the items live in each `wave-*/meta.json`), \
                 and no wave sidecar carries a matching OPEN item either. Check the wording \
                 against `mustard-rt run wave-tree --spec <spec>`, or pass `--line <n>` when \
                 the item is in this file",
                item.unwrap_or(""),
            ),
        );
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
                // No OPEN match. Before refusing, answer from the terminal
                // positions — but never by moving one: a `- [~]` line is a
                // decision, and turning it back into a task is the very thing
                // this command must not do.
                for line in lines.iter().take(end).skip(start) {
                    let Some(cb) = parse_checkbox(line) else { continue };
                    if !cb.text.contains(item) {
                        continue;
                    }
                    if cb.is_dropped() {
                        println!("already-dropped");
                        std::process::exit(0);
                    }
                    if cb.is_done() {
                        match mv {
                            Move::Mark => {
                                println!("already-marked");
                                std::process::exit(0);
                            }
                            Move::Drop(_) => die(
                                1,
                                &format!("cannot drop an item that is already done: {item}"),
                            ),
                        }
                    }
                }
                die(1, &format!("no `- [ ]` item matching: {item}"));
            }
        }
    };

    let new_line = {
        let Some(cb) = parse_checkbox(&lines[target_idx]) else { die(1, "target line is not a checkbox") };
        if cb.is_dropped() {
            println!("already-dropped");
            std::process::exit(0);
        }
        match mv {
            Move::Mark => {
                if cb.is_done() {
                    println!("already-marked");
                    std::process::exit(0);
                }
                format!("{}[x]{}{}", cb.prefix, cb.gap, cb.text)
            }
            Move::Drop(reason) => {
                if cb.is_done() {
                    die(1, "cannot drop an item that is already done");
                }
                // `[~]` plus the reason on the same line: the record a reader
                // months later needs in order to tell a decision from a task
                // nobody got to.
                format!(
                    "{}[~]{}{} — dropped: {}",
                    cb.prefix,
                    cb.gap,
                    cb.text,
                    one_line(reason)
                )
            }
        }
    };
    lines[target_idx] = new_line;

    if let Err(e) = fs::write_atomic(&spec_path, lines.join("\n").as_bytes()) {
        die(1, &format!("cannot write spec: {e}"));
    }
    match mv {
        Move::Mark => println!("marked"),
        Move::Drop(_) => println!("dropped"),
    }
}

/// Collapse newlines/tabs in the stated reason so one item stays one markdown
/// line (the section is parsed line-by-line by three consumers).
fn one_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
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
            dropped: None,
        };
        assert!(item_matches(&it, "handler.rs"), "basename");
        assert!(item_matches(&it, "api/handler.rs"), "segment suffix");
        assert!(item_matches(&it, "src/api/handler.rs"), "exact path");
        assert!(item_matches(&it, "handler"), "label substring");
        assert!(!item_matches(&it, "other.rs"));
    }

    /// The needle the operator actually has is the close gate's SENTENCE, and
    /// it is longer than the label — the reverse of what the matcher assumed.
    ///
    /// Measured in the field: 15 items, all implemented, could not be marked
    /// through the only door that marks them. Each wave records an item twice —
    /// a short `label` (the file path) in `meta.json`, and the task sentence in
    /// that wave's `spec.md` — and the gate reports the sentence.
    #[test]
    fn the_gates_own_sentence_marks_the_item_it_names() {
        let it = ChecklistItem {
            label: "apps/rt/src/hooks/session/session_start_inject.rs".to_string(),
            path: Some("apps/rt/src/hooks/session/session_start_inject.rs".to_string()),
            done: false,
            dropped: None,
        };
        // The full path inside a longer sentence.
        assert!(item_matches(
            &it,
            "criar apps/rt/src/hooks/session/session_start_inject.rs no molde do vizinho",
        ));
        // The BASENAME alone — how 7 of those 15 items were written.
        assert!(item_matches(
            &it,
            "em session_start_inject.rs: quando source é compact, reentregar a família",
        ));

        // A basename must be a whole token. `x.rs` matching a sentence about
        // `prefix_x.rs` would mark the WRONG item, and a checklist marked wrong
        // is worse than one that refuses.
        let short = ChecklistItem {
            label: "src/x.rs".to_string(),
            path: Some("src/x.rs".to_string()),
            done: false,
            dropped: None,
        };
        assert!(!item_matches(&short, "em prefix_x.rs: outra tarefa"), "prefixed");
        assert!(!item_matches(&short, "em x.rs.bak: outra tarefa"), "suffixed");
        assert!(item_matches(&short, "em x.rs: a tarefa certa"), "whole token");

        // The FULL-PATH spelling goes through the same boundary. The first
        // version checked `needle.contains(label)` before any boundary rule and
        // returned early, so this exact sentence marked the wrong item — and
        // the test above never caught it, because `em x.rs.bak` does not
        // contain `src/x.rs` (found in review).
        assert!(
            !item_matches(&short, "em src/x.rs.bak: outra tarefa"),
            "the full path must not match a longer filename either",
        );
        assert!(
            !item_matches(&short, "em src/x.rs_old: outra tarefa"),
            "nor a name that merely starts with it",
        );
        assert!(item_matches(&short, "criar src/x.rs no molde do vizinho"), "full path, whole token");

        // A PROSE label is not a path. `ChecklistItem::label` is documented as
        // the human-readable task label, so a short one used to match any
        // sentence containing that word — `teste` marking an item because the
        // operator wrote "adicionar o teste de regressão em outro.rs". The
        // boundary makes it a whole word, and a refusal beats a wrong mark.
        let prose = ChecklistItem {
            label: "teste".to_string(),
            path: None,
            done: false,
            dropped: None,
        };
        assert!(item_matches(&prose, "rodar o teste de novo"), "whole word matches");
        assert!(!item_matches(&prose, "revisar o testes-antigos do modulo"), "not a fragment");

        // An empty needle matches nothing — it would otherwise mark the first
        // open item of whichever wave is read first.
        assert!(!item_matches(&it, ""));
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

        let outcome = try_move_in_metas(project.path(), &spec_dir, "handler.rs", Move::Mark);
        assert!(matches!(outcome, Some(MetaMark::Marked)), "first call marks");
        let meta = read_meta(&wave_dir.join("meta.json")).unwrap();
        assert!(meta.checklist[0].done, "done flipped in the wave sidecar");

        // Idempotent: a second call is a no-op `already-marked`.
        let again = try_move_in_metas(project.path(), &spec_dir, "handler.rs", Move::Mark);
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
        assert!(try_move_in_metas(project.path(), &spec_dir, "a", Move::Mark).is_none());
    }

    // --- the third position: dropped on purpose (AC-8) ----------------------

    /// Seed a wave-plan spec with one open checklist item; returns
    /// `(project, spec_dir, wave_dir)`.
    fn seed_wave_checklist(items_json: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
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
            format!(
                r#"{{"stage":"Execute","outcome":"Active","parent":"demo","checklist":{items_json}}}"#
            ),
        )
        .unwrap();
        (project, spec_dir, wave_dir)
    }

    /// AC-8 — a checklist item dropped on purpose WITH a stated reason is
    /// recorded as a decision (sidecar + `checklist.item.dropped` event) and
    /// stays distinct from an unchecked item: it is not open work, it is not
    /// done, and no later mark turns it back into either.
    #[test]
    fn checklist_records_dropped_with_reason() {
        let (project, spec_dir, wave_dir) = seed_wave_checklist(
            r#"[{"label":"src/api/handler.rs","path":"src/api/handler.rs","done":false}]"#,
        );

        // The drop carries a reason, and the reason IS the record.
        let outcome = try_move_in_metas(
            project.path(),
            &spec_dir,
            "handler.rs",
            Move::Drop("the endpoint moved to the gateway spec"),
        );
        assert!(matches!(outcome, Some(MetaMark::Dropped)), "the drop is recorded");

        let meta = read_meta(&wave_dir.join("meta.json")).unwrap();
        let item = &meta.checklist[0];
        assert_eq!(item.drop_reason(), Some("the endpoint moved to the gateway spec"));
        // Distinct from an unchecked item AND from a done one.
        assert!(!item.is_open(), "a dropped item is not pending work");
        assert!(!item.done, "dropping never claims the work was done");

        // The decision landed in the event stream as its OWN kind, carrying
        // the reason — a consumer counting `checklist.item.marked` must not
        // see it and report progress nobody made.
        let sp = ClaudePaths::for_project(project.path()).unwrap().for_spec("demo").unwrap();
        let mut dropped_lines = 0;
        let mut marked_lines = 0;
        for f in std::fs::read_dir(sp.events_dir()).unwrap() {
            let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
            for l in body.lines() {
                if l.contains("\"event\":\"checklist.item.dropped\"") {
                    assert!(l.contains("the endpoint moved to the gateway spec"), "{l}");
                    assert!(l.contains("\"wave\":1"), "{l}");
                    dropped_lines += 1;
                }
                marked_lines += usize::from(l.contains("\"event\":\"checklist.item.marked\""));
            }
        }
        assert_eq!(dropped_lines, 1, "exactly one checklist.item.dropped line");
        assert_eq!(marked_lines, 0, "a drop is never reported as a mark");

        // Idempotent, and terminal: dropping again is a no-op, and MARKING it
        // refuses instead of resurrecting the decision as progress.
        assert!(matches!(
            try_move_in_metas(project.path(), &spec_dir, "handler.rs", Move::Drop("again")),
            Some(MetaMark::AlreadyDropped)
        ));
        assert!(matches!(
            try_move_in_metas(project.path(), &spec_dir, "handler.rs", Move::Mark),
            Some(MetaMark::AlreadyDropped)
        ));
        let after = read_meta(&wave_dir.join("meta.json")).unwrap();
        assert!(!after.checklist[0].done, "the mark did not flip a dropped item");
        assert_eq!(
            after.checklist[0].drop_reason(),
            Some("the endpoint moved to the gateway spec"),
            "the first reason was not overwritten"
        );
    }

    /// Finished work cannot be re-labelled a decision, and a drop still
    /// refuses what it cannot find — the marker's refusal stays honest.
    #[test]
    fn drop_refuses_done_items_and_missing_items() {
        let (project, spec_dir, _wave) = seed_wave_checklist(
            r#"[{"label":"src/api/handler.rs","path":"src/api/handler.rs","done":true}]"#,
        );
        match try_move_in_metas(project.path(), &spec_dir, "handler.rs", Move::Drop("late")) {
            Some(MetaMark::Error(e)) => assert!(e.contains("already done"), "{e}"),
            other => panic!("expected a refusal for a done item, got {other:?}"),
        }
        // Nothing matches at all → `None`: the caller falls through to the
        // markdown pass, which dies rather than inventing an item.
        assert!(
            try_move_in_metas(project.path(), &spec_dir, "nowhere.rs", Move::Drop("x")).is_none()
        );
    }

    /// `resolve_move` is the gate that makes a reason mandatory: a bare
    /// `--drop` never produces a `Move::Drop`.
    #[test]
    fn a_drop_without_a_reason_is_not_a_move() {
        assert!(matches!(resolve_move(false, None), Move::Mark));
        assert!(matches!(resolve_move(true, Some("out of scope")), Move::Drop("out of scope")));
        // Trimmed: the stored reason never carries the caller's whitespace.
        assert!(matches!(resolve_move(true, Some("  spaced  ")), Move::Drop("spaced")));
    }

    /// The legacy markdown path gets the same third position: `- [~]` with
    /// the reason on the line, and it is not an unchecked item afterwards.
    #[test]
    fn markdown_dropped_line_parses_as_its_own_state() {
        let (_d, path) = write_spec(
            "## Checklist\n- [ ] alpha\n- [~] beta — dropped: folded into alpha\n",
        );
        let raw = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = raw.split('\n').collect();
        let (start, end) = find_checklist_section(&lines).unwrap();

        let open = parse_checkbox(lines[start]).unwrap();
        assert!(!open.is_dropped() && !open.is_done());

        let dropped = parse_checkbox(lines[start + 1]).unwrap();
        assert!(dropped.is_dropped(), "`[~]` is its own state, not a checked box");
        assert!(!dropped.is_done(), "dropped is not done");
        assert!(dropped.text.contains("dropped: folded into alpha"), "{}", dropped.text);

        // The mark pass scans for `state == ' '` only, so the dropped line is
        // invisible to it — exactly one open item in the section.
        let open_count = lines[start..end]
            .iter()
            .filter(|l| parse_checkbox(l).is_some_and(|cb| cb.state == ' '))
            .count();
        assert_eq!(open_count, 1);
    }

    #[test]
    fn one_line_folds_a_multiline_reason() {
        assert_eq!(one_line("moved to\n  the gateway\tspec"), "moved to the gateway spec");
    }
}
