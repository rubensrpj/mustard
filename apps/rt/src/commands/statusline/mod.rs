//! `mustard-rt run statusline` — render the Claude Code status bar.
//!
//! Reads the harness payload JSON from stdin and prints one line on stdout.
//! On any failure (bad JSON, missing fields, panicking I/O) we print
//! `Claude` and exit cleanly — the harness must never see a non-zero exit.
//!
//! Submodules:
//! - [`segment`] — pure data ([`segment::Segment`]) and per-kind builders.
//! - [`theme`]   — palette / separator / `render_line`.
//! - [`preview`] — handler for the `--preview` flag.
//!
//! Theme selection: see [`theme::ENV_VAR`]. Default = `catppuccin` (powerline,
//! requires a Nerd Font). Users without Nerd Font set
//! `MUSTARD_STATUSLINE_THEME=default`.

pub mod cli;

pub mod preview;
pub mod segment;
// `theme` stays crate-internal: the module became `pub` when the `run` CLI
// split moved `StatuslineCmd` into `statusline::cli`, and its public items
// (`ThemeId::theme`, `render_line`, `DEFAULT`) hand out the crate-private
// `Theme` type - capping the module keeps that honest without leaking it.
pub(crate) mod theme;

use segment::{
    context_segment, cost_segment, diff_segment, duration_segment, git_segment,
    inert_segment, model_segment, module_segment, mustard_segment, prune_segment, savings_segment,
    unit_segment,
    version_segment, Segment,
};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use theme::{render_line, ThemeId};

/// Build the ordered segment list from the parsed payload.
///
/// Builders that return `None` (zero duration, no `total_cost_usd`, etc.) are
/// quietly skipped, so the line stays compact when state is sparse.
fn build_segments(data: &Value) -> Vec<Segment> {
    let cwd: PathBuf = data
        .get("workspace")
        .and_then(|w| w.get("current_dir"))
        .or_else(|| data.get("cwd"))
        .and_then(Value::as_str)
        .map_or_else(
            || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            PathBuf::from,
        );

    let mut segs = vec![module_segment(&cwd)];
    if let Some(s) = git_segment(&cwd) {
        segs.push(s);
    }
    if let Some(s) = context_segment(data) {
        segs.push(s);
    }
    if let Some(s) = duration_segment(data) {
        segs.push(s);
    }
    if let Some(s) = savings_segment() {
        segs.push(s);
    }
    if let Some(s) = diff_segment(data) {
        segs.push(s);
    }
    if let Some(s) = cost_segment(data) {
        segs.push(s);
    }
    segs.push(model_segment(data));
    if let Some(s) = version_segment(data) {
        segs.push(s);
    }
    // Mustard's own tail mark: harness version, or the yellow drift hint
    // (`m{stamped}→{current}`) when the project needs `/mustard:upsert`.
    if let Some(s) = mustard_segment(&cwd) {
        segs.push(s);
    }
    // Last, in the order the theme palettes already declare (styles are packed
    // by `SegmentKind`, and a new kind is appended): how many delivered work
    // units still have a live branch. Silent when nothing is owed — the bar
    // only grows when there is something to say.
    if let Some(s) = prune_segment(&cwd) {
        segs.push(s);
    }
    // Where the operator stopped: the unit this session is inside and its
    // stage. The bar named the harness version and nothing else, so a unit
    // parked in PLAN was invisible until a command was typed.
    if let Some(s) = unit_segment(&cwd) {
        segs.push(s);
    }
    // Loudest last, and red: the plugin is installed but switched OFF, so no
    // hook runs at all. That state renders like a healthy one everywhere else.
    if let Some(s) = inert_segment(&cwd) {
        segs.push(s);
    }
    segs
}

/// Which row a segment belongs to.
///
/// The bar carries two unrelated kinds of fact and they were competing for one
/// row: WHERE the work is (folder, branch, which harness, what is owed) and
/// WHAT the session has spent (context, time, savings, diff, cost, model,
/// versions). Measured on a real payload, one row came to 171 visible
/// characters — past the 80–120 a terminal usually gives, so the tail was
/// simply cut off, and the tail is where the harness marks live.
///
/// Splitting by ROLE rather than by width is what makes the result readable:
/// each row answers one question, so the eye learns where to look and the
/// layout does not reshuffle when a number appears or disappears. The branch
/// name — the widest element by far, and variable, since a work-unit branch is
/// `{kind}/{slug}` (or the older `{base}_{slug}`) — gets the first row nearly
/// to itself.
///
/// Claude Code renders one row per printed line (documented), so this is two
/// `println!`s, not a rendering trick.
const fn is_place_row(kind: segment::SegmentKind) -> bool {
    use segment::SegmentKind as K;
    matches!(kind, K::Module | K::Git | K::Mustard | K::Prune)
}

/// Render the statusline from a parsed payload: one row per non-empty group,
/// no trailing newline. A row whose segments are all absent is dropped rather
/// than printed blank — with a sparse payload the bar stays a single line, the
/// shape it had before this split.
fn render(data: &Value) -> Vec<String> {
    let theme = ThemeId::from_env().theme();
    let (place, spend): (Vec<Segment>, Vec<Segment>) =
        build_segments(data).into_iter().partition(|s| is_place_row(s.kind));

    [place, spend]
        .into_iter()
        .filter(|group| !group.is_empty())
        .map(|group| render_line(theme, &group))
        .filter(|line| !line.is_empty())
        .collect()
}

/// Dispatch `mustard-rt run statusline`.
///
/// `preview = true` short-circuits to the [`preview`] handler (no stdin read,
/// no JSON parse). `preview = false` does the live render.
pub fn run(preview: bool) {
    if preview {
        preview::run();
        return;
    }
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        println!("Claude");
        return;
    }
    match serde_json::from_str::<Value>(&buf) {
        Ok(data) => {
            for line in render(&data) {
                println!("{line}");
            }
        }
        Err(_) => println!("Claude"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A full payload renders TWO rows, split by role.
    ///
    /// This replaces a test that pinned the bar to one row. That pin recorded a
    /// 2026-05-21 decision to drop a pipeline BANNER — a second line that
    /// repeated state the bar already showed. What is here now is not that: the
    /// same segments, redistributed, because one row had grown to 171 visible
    /// characters and terminals cut the tail. Claude Code documents one row per
    /// printed line, so two rows is a supported layout, not a workaround.
    #[test]
    fn a_full_payload_renders_two_rows_split_by_role() {
        let data = json!({
            "workspace": { "current_dir": ".", "project_dir": "." },
            "model": { "display_name": "Opus 4.7" },
            "version": "2.1.146",
            "cost": {
                "total_duration_ms": 1000,
                "total_lines_added": 10,
                "total_lines_removed": 2,
                "total_cost_usd": 0.42
            },
            "context_window": {
                "remaining_percentage": 70,
                "total_input_tokens": 50000,
                "total_output_tokens": 10000
            }
        });
        let lines = render(&data);
        assert_eq!(lines.len(), 2, "place row + spend row — got {lines:?}");
        assert!(lines.iter().all(|l| !l.is_empty()), "no blank row is printed: {lines:?}");

        // The split is by ROLE, so each fact lands on exactly one row and the
        // widest element (the branch) is not sharing with the numbers.
        let (place, spend) = (&lines[0], &lines[1]);
        assert!(spend.contains("$0.42"), "cost belongs to the spend row: {spend}");
        assert!(spend.contains("70%"), "context belongs to the spend row: {spend}");
        assert!(!place.contains("$0.42"), "…and never to the place row: {place}");
    }

    /// Every segment kind lands on exactly one row — a kind added later without
    /// a home would silently vanish from the bar, which is the failure mode a
    /// partition invites.
    #[test]
    fn every_rendered_segment_reaches_a_row() {
        let data = json!({
            "workspace": { "current_dir": ".", "project_dir": "." },
            "model": { "display_name": "Opus 4.7" },
            "version": "2.1.146",
            "cost": { "total_cost_usd": 0.42, "total_duration_ms": 1000 }
        });
        let built = build_segments(&data);
        let placed = built.iter().filter(|s| is_place_row(s.kind)).count();
        let spent = built.iter().filter(|s| !is_place_row(s.kind)).count();
        assert_eq!(placed + spent, built.len(), "the partition is total by construction");
        assert!(placed > 0 && spent > 0, "a full payload feeds both rows");
    }

    /// A sparse payload keeps the bar at ONE row: an empty group is dropped, not
    /// printed blank. This is what the split must not cost — a fresh session
    /// with no numbers yet should look exactly as it did before.
    #[test]
    fn render_falls_back_to_one_row_with_minimal_payload() {
        let lines = render(&json!({ "model": { "id": "claude-opus" } }));
        assert!(!lines.is_empty(), "the bar never disappears");
        assert!(lines.iter().all(|l| !l.is_empty()), "no blank row: {lines:?}");
    }

    #[test]
    fn build_segments_includes_cost_when_present() {
        let data = json!({ "cost": { "total_cost_usd": 0.42 } });
        let segs = build_segments(&data);
        assert!(segs.iter().any(|s| s.kind == segment::SegmentKind::Cost));
    }

    #[test]
    fn build_segments_skips_cost_when_zero() {
        let data = json!({ "cost": { "total_cost_usd": 0.0 } });
        let segs = build_segments(&data);
        assert!(!segs.iter().any(|s| s.kind == segment::SegmentKind::Cost));
    }
}
