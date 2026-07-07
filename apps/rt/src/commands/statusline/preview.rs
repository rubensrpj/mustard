//! `mustard-rt run statusline --preview` — render every shipped theme on its
//! own line, using a synthetic payload. Independent of any project state, so
//! it stays deterministic for screenshots.

use super::segment::{
    cost_segment, diff_segment, duration_segment, model_segment, savings_segment,
    version_segment, Segment, SegmentKind,
};
use super::theme::{render_line, ThemeId};

/// Synthetic payload — chosen to exercise every segment that has a
/// reasonable static answer (cost, duration, lines, version, model). The git
/// + context segments are forged by hand because they read live state.
fn synthetic_segments() -> Vec<Segment> {
    let payload = serde_json::json!({
        "model": { "display_name": "Claude Opus 4.7" },
        "version": "2.1.146",
        "cost": {
            "total_duration_ms": 303 * 60_000 + 27_000,
            "total_lines_added": 7901,
            "total_lines_removed": 1428,
            "total_cost_usd": 0.42,
        },
    });

    let mut segs = vec![Segment::new(SegmentKind::Module, "mustard")];

    // Forge a git segment so preview doesn't depend on whether cwd is a repo.
    segs.push(Segment::new(SegmentKind::Git, "\u{2387} dev_rubens +1"));

    // Forge a context segment — 70% remaining, 60k tokens.
    segs.push(Segment::new(
        SegmentKind::Context,
        format!(
            "{}{} 70% 60k",
            "\u{2588}".repeat(3),
            "\u{2591}".repeat(7)
        ),
    ));

    if let Some(s) = duration_segment(&payload) {
        segs.push(s);
    }
    // RTK savings — forge a representative segment if the real `rtk gain` has
    // nothing locally (CI etc.).
    if let Some(s) = savings_segment() {
        segs.push(s);
    } else {
        segs.push(Segment::new(SegmentKind::Savings, "\u{26A1} 91% 356500k saved"));
    }
    if let Some(s) = diff_segment(&payload) {
        segs.push(s);
    }
    if let Some(s) = cost_segment(&payload) {
        segs.push(s);
    }
    segs.push(model_segment(&payload));
    if let Some(s) = version_segment(&payload) {
        segs.push(s);
    }
    segs
}

/// Print one labeled line per shipped theme.
pub fn run() {
    let segs = synthetic_segments();
    // Width the longest name will take, so the previews left-align cleanly.
    let max_name = ThemeId::ALL.iter().map(|id| id.name().len()).max().unwrap_or(0);
    for id in ThemeId::ALL {
        let theme = id.theme();
        let nf = if theme.requires_nerdfont { " (Nerd Font)" } else { "" };
        let label = format!("{:width$}", id.name(), width = max_name);
        println!("{label}{nf}:");
        println!("  {}", render_line(theme, &segs));
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::statusline::theme::ThemeId;

    #[test]
    fn synthetic_segments_covers_main_kinds() {
        let segs = synthetic_segments();
        let kinds: Vec<_> = segs.iter().map(|s| s.kind).collect();
        // Module + Git + Context + Model are always present
        for required in [
            SegmentKind::Module,
            SegmentKind::Git,
            SegmentKind::Context,
            SegmentKind::Model,
        ] {
            assert!(
                kinds.contains(&required),
                "preview should always include {required:?}, got {kinds:?}"
            );
        }
        // Cost is forged into the synthetic payload → must appear
        assert!(kinds.contains(&SegmentKind::Cost));
    }

    #[test]
    fn each_theme_renders_non_empty_line_for_synthetic_segments() {
        let segs = synthetic_segments();
        for id in ThemeId::ALL {
            let out = render_line(id.theme(), &segs);
            assert!(!out.is_empty(), "{} produced empty output", id.name());
        }
    }
}
