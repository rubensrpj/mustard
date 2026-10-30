//! A cor da barra de status: um tema só, o Mustard, amarelo e neutro, sem
//! fundo colorido e sem fonte especial. [`render_line`] junta os pedaços com
//! uma barra vertical apagada entre eles.

use super::segment::{SEGMENT_KIND_COUNT, Segment};

/// Hard ANSI reset.
const RESET: &str = "\x1b[0m";
/// Bold modifier.
const BOLD: &str = "\x1b[1m";
/// Dim modifier — used by the pipe separator.
const DIM: &str = "\x1b[2m";

/// Uma cor do terminal, da paleta básica de 16 cores, que todo terminal
/// mostra igual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Standard ANSI color index 0..=15 (30+n for fg in the 0..=7 range,
    /// 90+n for 8..=15, fallback to 256-color seq for higher).
    Ansi(u8),
}

impl Color {
    /// ANSI escape sequence that sets the foreground to this color.
    #[must_use]
    pub(crate) fn fg_seq(self) -> String {
        match self {
            Color::Ansi(n) if n < 8 => format!("\x1b[3{n}m"),
            Color::Ansi(n) if n < 16 => format!("\x1b[9{}m", n - 8),
            Color::Ansi(n) => format!("\x1b[38;5;{n}m"),
        }
    }
}

/// A cor e o negrito de um tipo de pedaço.
#[derive(Debug, Clone, Copy)]
struct Style {
    fg: Color,
    bold: bool,
}

impl Style {
    const fn fg(fg: Color) -> Self {
        Self { fg, bold: false }
    }
    const fn fg_bold(fg: Color) -> Self {
        Self { fg, bold: true }
    }
}

/// O estilo de cada tipo de pedaço, na ordem de `SegmentKind`: o módulo, o
/// git, o contexto, o modelo, a spec e o aviso do Mustard desligado.
const STYLES: [Style; SEGMENT_KIND_COUNT] = [
    Style::fg_bold(Color::Ansi(3)),
    Style::fg(Color::Ansi(7)),
    Style::fg(Color::Ansi(7)),
    Style::fg(Color::Ansi(7)),
    Style::fg_bold(Color::Ansi(3)),
    Style::fg_bold(Color::Ansi(1)),
];

/// A linha da barra, sem quebra no fim. O `override_fg` de um pedaço (o
/// limite do contexto) vence a cor do tipo dele.
#[must_use]
pub fn render_line(segments: &[Segment]) -> String {
    let sep = format!(" {DIM}\u{2502}{RESET} ");
    segments
        .iter()
        .map(|seg| {
            let style = STYLES[seg.kind as usize];
            let bold = if style.bold { BOLD } else { "" };
            let fg = seg.override_fg.unwrap_or(style.fg).fg_seq();
            format!("{bold}{fg}{}{RESET}", seg.text)
        })
        .collect::<Vec<_>>()
        .join(&sep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::statusline::segment::SegmentKind;

    fn seg(kind: SegmentKind, text: &str) -> Segment {
        Segment::new(kind, text)
    }

    #[test]
    fn color_fg_seq_matches_ansi_shape() {
        assert_eq!(Color::Ansi(6).fg_seq(), "\x1b[36m");
        assert_eq!(Color::Ansi(9).fg_seq(), "\x1b[91m");
        assert_eq!(Color::Ansi(42).fg_seq(), "\x1b[38;5;42m");
    }

    #[test]
    fn render_pipe_contains_pipe_separator() {
        let segs = vec![seg(SegmentKind::Module, "mustard"), seg(SegmentKind::Git, "dev")];
        let out = render_line(&segs);
        assert!(out.contains("mustard"));
        assert!(out.contains("dev"));
        assert!(out.contains("\u{2502}"), "pipe separator missing from {out:?}");
    }

    #[test]
    fn render_line_empty_returns_empty() {
        assert_eq!(render_line(&[]), "");
    }

    #[test]
    fn render_honors_segment_override_fg() {
        let mut s = seg(SegmentKind::Context, "90%");
        s.override_fg = Some(Color::Ansi(1)); // red
        let out = render_line(&[s]);
        assert!(out.contains("\x1b[31m"), "override fg sequence missing from {out:?}");
    }
}
