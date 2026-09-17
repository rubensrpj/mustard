//! `mustard-rt run statusline --preview` — a face que mostra os temas da
//! barra: cada tema numa linha, com o nome dele, sobre um exemplo fixo que não
//! depende do projeto. Os temas que pedem a fonte especial (Nerd Font, que o
//! `mustard install-nerd-font` instala) saem marcados.

use std::fmt::Write as _;

use super::segment::{cost_segment, diff_segment, duration_segment, model_segment, savings_segment, Segment, SegmentKind};
use super::theme::{render_line, ThemeId};

/// Synthetic payload — chosen to exercise every segment that has a
/// reasonable static answer (cost, duration, lines, model). The git, spec
/// and context segments are forged by hand because they read live state.
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
    // Forge the spec segment too — the live builder reads the spec state.
    segs.push(Segment::new(SegmentKind::Unit, "\u{25b8} checkout running 1 de 4 ondas"));

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
    segs
}

/// O texto da face: para cada tema, a linha do nome, marcada quando ele pede
/// a fonte especial, a barra de exemplo nesse tema e uma linha em branco.
fn preview_text() -> String {
    let segs = synthetic_segments();
    // Width the longest name will take, so the previews left-align cleanly.
    let max_name = ThemeId::ALL.iter().map(|id| id.name().len()).max().unwrap_or(0);
    let mut out = String::new();
    for id in ThemeId::ALL {
        let theme = id.theme();
        let nf = if theme.requires_nerdfont { " (Nerd Font)" } else { "" };
        let label = format!("{:width$}", id.name(), width = max_name);
        let _ = writeln!(out, "{label}{nf}:\n  {}\n", render_line(theme, &segs));
    }
    out
}

/// Print one labeled line per shipped theme.
pub fn run() {
    print!("{}", preview_text());
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

    /// A barra tem mais de um tema, alguns pedem a fonte especial e outros
    /// não, e a face da prévia mostra cada um, pelo nome, marcando só os que
    /// pedem a fonte.
    #[test]
    fn the_preview_shows_every_theme_and_marks_the_ones_that_need_the_font() {
        let shown = preview_text();
        let needs_font = ThemeId::ALL.iter().filter(|id| id.theme().requires_nerdfont).count();
        assert!(needs_font > 0 && needs_font < ThemeId::ALL.len(), "some themes need the font and some do not");
        let labels: Vec<(&str, bool)> = shown
            .lines()
            .filter(|line| !line.starts_with(' ') && line.ends_with(':'))
            .map(|line| {
                let label = line.trim_end_matches(':');
                (label.trim_end_matches(" (Nerd Font)").trim_end(), label.ends_with(" (Nerd Font)"))
            })
            .collect();
        let expected: Vec<(&str, bool)> =
            ThemeId::ALL.iter().map(|id| (id.name(), id.theme().requires_nerdfont)).collect();
        assert_eq!(labels, expected, "{shown}");
        assert_eq!(shown.lines().filter(|line| line.starts_with("  ")).count(), ThemeId::ALL.len(), "{shown}");
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
