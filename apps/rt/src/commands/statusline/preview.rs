//! `mustard-rt run statusline --preview` — a face que mostra os temas da
//! barra: cada tema numa linha, com o nome dele, sobre um exemplo fixo que não
//! depende do projeto (só os rótulos seguem o idioma dele). Os temas que
//! pedem a fonte especial (Nerd Font, que o `mustard install-nerd-font`
//! instala) saem marcados.

use std::fmt::Write as _;

use super::segment::{
    context_segment, duration_segment, model_segment, mustard_segment, savings_segment, Segment, SegmentKind,
};
use super::theme::{render_line, ThemeId};
use crate::shared::rtk_gain::RtkGain;
use mustard_core::SupportedLocale;

/// O exemplo fixo: a barra aprovada, de uma sessão no levantamento, com os
/// rótulos em `lang`. O tempo, o uso da conversa, a versão do Mustard, a
/// economia do rtk e o modelo saem dos próprios construtores; o projeto, a
/// branch e a spec são forjados, porque os construtores deles leem o estado
/// vivo.
fn synthetic_segments(lang: SupportedLocale) -> Vec<Segment> {
    let payload = serde_json::json!({
        "model": { "display_name": "Opus 5 (1M context)" },
        "cost": { "total_duration_ms": (5 * 60 + 49) * 60_000 },
        "context_window": { "remaining_percentage": 76 },
    });
    let phase = mustard_core::translate("page.phase.survey", lang);
    let mut segs = vec![
        Segment::new(SegmentKind::Module, "portal-florestal-backend"),
        Segment::new(SegmentKind::Git, "\u{2387} feature/pi-kpis-plantio ?1"),
        Segment::new(SegmentKind::Unit, format!("\u{25b8} {phase}")),
    ];
    segs.extend(context_segment(&payload));
    segs.extend(duration_segment(&payload));
    segs.push(mustard_segment());
    segs.extend(savings_segment(Some(&RtkGain { saved: 356_500_000, pct: 64.0 }), lang));
    segs.push(model_segment(&payload));
    segs
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

    /// A prévia acompanha a barra: o exemplo aprovado, com a fase, o uso da
    /// conversa, o tempo em horas, a versão do Mustard e a economia do rtk no
    /// idioma pedido, e sem o custo.
    #[test]
    fn synthetic_segments_follow_the_approved_example() {
        let text = |lang| synthetic_segments(lang).iter().map(|s| s.text.clone()).collect::<Vec<_>>().join("  ");
        let version = mustard_core::harness_version();
        assert_eq!(
            text(SupportedLocale::PtBr),
            format!(
                "portal-florestal-backend  \u{2387} feature/pi-kpis-plantio ?1  \u{25b8} levantamento  \
                 \u{2588}\u{2588}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591} 24%  5h49m  \
                 Mustard {version}  \u{26A1} rtk poupou 64%  Opus 5 (1M context)"
            )
        );
        let english = text(SupportedLocale::EnUs);
        assert!(english.contains("\u{25b8} survey  ") && english.contains("\u{26A1} rtk saved 64%"), "{english}");
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
        let expected: Vec<(&str, bool)> =
            ThemeId::ALL.iter().map(|id| (id.name(), id.theme().requires_nerdfont)).collect();
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
