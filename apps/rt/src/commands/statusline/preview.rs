//! `mustard-rt run statusline --preview` — a face que mostra os temas da
//! barra: cada tema numa linha, com o nome dele, sobre um exemplo fixo que não
//! depende do projeto (só os rótulos seguem o idioma dele). Os temas que
//! pedem a fonte especial (Nerd Font, que o `mustard install-nerd-font`
//! instala) saem marcados.

use std::fmt::Write as _;

use super::segment::{Segment, SegmentKind, context_segment, model_segment};
use super::theme::{ThemeId, render_line};
use mustard_core::SupportedLocale;

/// Preview the same compact shape the live statusline uses.
fn synthetic_segments(lang: SupportedLocale) -> Vec<Segment> {
    let payload = serde_json::json!({"model":{"display_name":"Sonnet 5.5"},"context_window":{"remaining_percentage":76}});
    let phase = mustard_core::translate("page.phase.running", lang);
    let mut segments = vec![
        Segment::new(SegmentKind::Unit, format!("pi-cardapio-original · {phase}")),
        model_segment(&payload),
    ];
    segments.extend(context_segment(&payload, lang));
    segments
}

/// O texto da face: para cada tema, a linha do nome, marcada quando ele pede
/// a fonte especial, a barra de exemplo nesse tema e uma linha em branco.
fn preview_text(lang: SupportedLocale) -> String {
    let segs = synthetic_segments(lang);
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

/// Print one labeled line per shipped theme, with the labels in the language
/// of the project in the current directory.
pub fn run() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let lang = mustard_core::ProjectConfig::load(&cwd).language().text_or_default();
    print!("{}", preview_text(lang));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::statusline::theme::ThemeId;

    #[test]
    fn preview_matches_the_compact_live_statusline() {
        for (lang, context) in [(SupportedLocale::PtBr, "contexto 24%"), (SupportedLocale::EnUs, "context 24%")] {
            let segments = synthetic_segments(lang);
            assert_eq!(segments.len(), 3);
            let text = segments.iter().map(|segment| segment.text.clone()).collect::<Vec<_>>().join(" ");
            assert!(text.contains(context) && text.contains("pi-cardapio-original") && text.contains("Sonnet 5.5"));
            for absent in ["Mustard 0.", "vs média", "rtk poupou", "5h49m"] {
                assert!(!text.contains(absent), "{text}");
            }
        }
    }

    /// A barra tem mais de um tema, alguns pedem a fonte especial e outros
    /// não, e a face da prévia mostra cada um, pelo nome, marcando só os que
    /// pedem a fonte.
    #[test]
    fn the_preview_shows_every_theme_and_marks_the_ones_that_need_the_font() {
        let shown = preview_text(SupportedLocale::PtBr);
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
        let expected: Vec<(&str, bool)> = ThemeId::ALL.iter().map(|id| (id.name(), id.theme().requires_nerdfont)).collect();
        assert_eq!(labels, expected, "{shown}");
        assert_eq!(shown.lines().filter(|line| line.starts_with("  ")).count(), ThemeId::ALL.len(), "{shown}");
    }

    #[test]
    fn each_theme_renders_non_empty_line_for_synthetic_segments() {
        let segs = synthetic_segments(SupportedLocale::PtBr);
        for id in ThemeId::ALL {
            let out = render_line(id.theme(), &segs);
            assert!(!out.is_empty(), "{} produced empty output", id.name());
        }
    }
}
