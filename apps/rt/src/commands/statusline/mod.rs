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
    model_segment, module_segment, mustard_segment, savings_segment, version_segment,
    Segment,
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
    // (`m{stamped}↑{current}`) when the project needs `/mustard:upsert`.
    if let Some(s) = mustard_segment(&cwd) {
        segs.push(s);
    }
    segs
}

/// Render the statusline from a parsed payload. Single line, no newline.
fn render(data: &Value) -> Vec<String> {
    let theme = ThemeId::from_env().theme();
    let segs = build_segments(data);
    vec![render_line(theme, &segs)]
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

    /// Locks down: the statusline must never emit a second line. The pipeline
    /// banner was removed for good (see 2026-05-21 conversation); this test
    /// catches any future attempt to add another `println!` in `run`.
    #[test]
    fn render_never_emits_pipeline_second_line() {
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
        assert_eq!(lines.len(), 1, "statusline must be a single line — got {lines:?}");
    }

    #[test]
    fn render_falls_back_to_module_only_with_minimal_payload() {
        let lines = render(&json!({ "model": { "id": "claude-opus" } }));
        assert_eq!(lines.len(), 1);
        // First line should at least contain SOME printable text (the module
        // name pulled from cwd).
        assert!(!lines[0].is_empty());
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
