//! Statusline theme model — palette, separator, and the `render_line`
//! function that turns a `&[Segment]` into the final ANSI string.
//!
//! Six themes are shipped as `pub const` items:
//! `DEFAULT`, `MINIMAL`, `TOKYO_NIGHT`, `CATPPUCCIN`, `PASTEL_POWERLINE`,
//! `GRUVBOX_RAINBOW`. Selection is via the `MUSTARD_STATUSLINE_THEME` env var
//! (case-insensitive, hyphens/underscores accepted). Unknown / unset →
//! `Catppuccin`.
//!
//! Powerline themes require a Nerd Font in the terminal — the U+E0B0 right-
//! arrow glyph used for segment transitions renders as tofu otherwise. The
//! escape hatch is `MUSTARD_STATUSLINE_THEME=default`.

use super::segment::{Segment, SegmentKind, SEGMENT_KIND_COUNT};
use std::fmt::Write as _;

/// Env-var read by [`ThemeId::from_env`].
pub const ENV_VAR: &str = "MUSTARD_STATUSLINE_THEME";

/// Hard ANSI reset.
const RESET: &str = "\x1b[0m";
/// Bold modifier.
const BOLD: &str = "\x1b[1m";
/// Dim modifier — used by the pipe separator.
const DIM: &str = "\x1b[2m";
/// Reset background (kept fg) — used at the powerline end-cap.
const BG_RESET: &str = "\x1b[49m";

/// The right-pointing solid triangle from the powerline / Nerd Font set.
pub const POWERLINE_RIGHT: char = '\u{E0B0}';

// ---------------------------------------------------------------------------
// Color
// ---------------------------------------------------------------------------

/// A terminal color. `Ansi` uses the 16-color basic palette (most terminals
/// honor them faithfully — used by `default` / `minimal`); `Rgb` uses 24-bit
/// truecolor — used by the powerline themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Standard ANSI color index 0..=15 (30+n for fg in the 0..=7 range,
    /// 90+n for 8..=15, fallback to 256-color seq for higher).
    Ansi(u8),
    /// 24-bit RGB triple.
    Rgb(u8, u8, u8),
}

impl Color {
    /// ANSI escape sequence that sets the foreground to this color.
    #[must_use]
    pub fn fg_seq(self) -> String {
        match self {
            Color::Ansi(n) if n < 8 => format!("\x1b[3{n}m"),
            Color::Ansi(n) if n < 16 => format!("\x1b[9{}m", n - 8),
            Color::Ansi(n) => format!("\x1b[38;5;{n}m"),
            Color::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        }
    }

    /// ANSI escape sequence that sets the background to this color.
    #[must_use]
    pub fn bg_seq(self) -> String {
        match self {
            Color::Ansi(n) if n < 8 => format!("\x1b[4{n}m"),
            Color::Ansi(n) if n < 16 => format!("\x1b[10{}m", n - 8),
            Color::Ansi(n) => format!("\x1b[48;5;{n}m"),
            Color::Rgb(r, g, b) => format!("\x1b[48;2;{r};{g};{b}m"),
        }
    }
}

// ---------------------------------------------------------------------------
// Style + Theme
// ---------------------------------------------------------------------------

/// Per-segment visual style. `bg = None` means "transparent" — used by the
/// pipe and whitespace separators where every segment shares the terminal
/// background.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub fg: Color,
    pub bg: Option<Color>,
    pub bold: bool,
}

impl Style {
    const fn fg(fg: Color) -> Self {
        Self { fg, bg: None, bold: false }
    }
    const fn fg_bold(fg: Color) -> Self {
        Self { fg, bg: None, bold: true }
    }
    const fn pl(fg: Color, bg: Color) -> Self {
        Self { fg, bg: Some(bg), bold: false }
    }
    const fn pl_bold(fg: Color, bg: Color) -> Self {
        Self { fg, bg: Some(bg), bold: true }
    }
}

/// How segments are joined.
#[derive(Debug, Clone, Copy)]
pub enum Separator {
    /// `" │ "` between segments; theme fg per segment, no bg.
    Pipe,
    /// `"  "` between segments; theme fg per segment, no bg.
    Whitespace,
    /// A Nerd Font glyph; segments get a bg, transitions color the glyph as
    /// `fg = prev.bg`, `bg = next.bg`.
    Powerline { glyph: char },
}

/// A complete statusline theme.
pub struct Theme {
    /// Self-reference used by tests + future introspection (debug logging,
    /// `--print-theme`, etc.). Not read in the live render path, hence the
    /// `allow`.
    #[allow(dead_code)]
    pub id: ThemeId,
    pub separator: Separator,
    /// Styles indexed by `SegmentKind as usize`. Always `SEGMENT_KIND_COUNT`.
    pub styles: [Style; SEGMENT_KIND_COUNT],
    pub requires_nerdfont: bool,
}

impl Theme {
    /// Style for a given segment kind.
    #[must_use]
    pub fn style_for(&self, kind: SegmentKind) -> Style {
        self.styles[kind as usize]
    }
}

// ---------------------------------------------------------------------------
// ThemeId
// ---------------------------------------------------------------------------

/// The complete list of shipped themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    Default,
    Minimal,
    TokyoNight,
    Catppuccin,
    PastelPowerline,
    GruvboxRainbow,
}

impl ThemeId {
    /// Every theme, in preview order.
    pub const ALL: &'static [ThemeId] = &[
        ThemeId::Default,
        ThemeId::Minimal,
        ThemeId::TokyoNight,
        ThemeId::Catppuccin,
        ThemeId::PastelPowerline,
        ThemeId::GruvboxRainbow,
    ];

    /// Human / canonical name used in the env var and in `--preview` output.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ThemeId::Default => "default",
            ThemeId::Minimal => "minimal",
            ThemeId::TokyoNight => "tokyo-night",
            ThemeId::Catppuccin => "catppuccin",
            ThemeId::PastelPowerline => "pastel-powerline",
            ThemeId::GruvboxRainbow => "gruvbox-rainbow",
        }
    }

    /// Parse the env-var value. Trims, lowercases, normalizes `_` to `-`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let norm = raw.trim().to_ascii_lowercase().replace('_', "-");
        match norm.as_str() {
            "default" => Some(ThemeId::Default),
            "minimal" => Some(ThemeId::Minimal),
            "tokyo-night" | "tokyonight" => Some(ThemeId::TokyoNight),
            "catppuccin" | "catppuccin-mocha" | "mocha" => Some(ThemeId::Catppuccin),
            "pastel-powerline" | "pastel" => Some(ThemeId::PastelPowerline),
            "gruvbox-rainbow" | "gruvbox" => Some(ThemeId::GruvboxRainbow),
            _ => None,
        }
    }

    /// Read [`ENV_VAR`] and pick a theme; fall back to [`ThemeId::Catppuccin`]
    /// when unset, empty, or unknown.
    #[must_use]
    pub fn from_env() -> Self {
        std::env::var(ENV_VAR)
            .ok()
            .as_deref()
            .and_then(Self::parse)
            .unwrap_or(ThemeId::Catppuccin)
    }

    /// Resolve to the static `Theme` definition.
    #[must_use]
    pub fn theme(self) -> &'static Theme {
        match self {
            ThemeId::Default => &DEFAULT,
            ThemeId::Minimal => &MINIMAL,
            ThemeId::TokyoNight => &TOKYO_NIGHT,
            ThemeId::Catppuccin => &CATPPUCCIN,
            ThemeId::PastelPowerline => &PASTEL_POWERLINE,
            ThemeId::GruvboxRainbow => &GRUVBOX_RAINBOW,
        }
    }
}

// ---------------------------------------------------------------------------
// render_line — the public driver
// ---------------------------------------------------------------------------

/// Render `segments` according to `theme`. Single returned line, no trailing
/// newline. Per-segment `override_fg` (used by the cost-segment threshold)
/// wins over `theme.style_for(kind).fg`.
#[must_use]
pub fn render_line(theme: &Theme, segments: &[Segment]) -> String {
    if segments.is_empty() {
        return String::new();
    }
    match theme.separator {
        Separator::Pipe => render_pipe(theme, segments),
        Separator::Whitespace => render_whitespace(theme, segments),
        Separator::Powerline { glyph } => render_powerline(theme, segments, glyph),
    }
}

fn effective_fg(style: Style, seg: &Segment) -> Color {
    seg.override_fg.unwrap_or(style.fg)
}

fn wrap_fg(text: &str, fg: Color, bold: bool) -> String {
    let bold_seq = if bold { BOLD } else { "" };
    format!("{}{}{text}{RESET}", bold_seq, fg.fg_seq())
}

fn render_pipe(theme: &Theme, segs: &[Segment]) -> String {
    let sep = format!(" {DIM}\u{2502}{RESET} ");
    segs.iter()
        .map(|s| {
            let style = theme.style_for(s.kind);
            wrap_fg(&s.text, effective_fg(style, s), style.bold)
        })
        .collect::<Vec<_>>()
        .join(&sep)
}

fn render_whitespace(theme: &Theme, segs: &[Segment]) -> String {
    segs.iter()
        .map(|s| {
            let style = theme.style_for(s.kind);
            wrap_fg(&s.text, effective_fg(style, s), style.bold)
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn render_powerline(theme: &Theme, segs: &[Segment], glyph: char) -> String {
    let mut out = String::new();
    let last = segs.len() - 1;
    for (i, seg) in segs.iter().enumerate() {
        let style = theme.style_for(seg.kind);
        // In powerline themes the bg is theme-fixed; honoring `override_fg`
        // would let segment-level threshold colors clash with the palette.
        // The override semantics are "flat themes only" — documented on
        // `Segment::override_fg`.
        let fg = style.fg.fg_seq();
        let bg = style.bg.map(Color::bg_seq).unwrap_or_default();
        let bold = if style.bold { BOLD } else { "" };
        // " text " — pad with spaces so the bg has breathing room
        let _ = write!(out, "{bold}{fg}{bg} {} {RESET}", seg.text);

        // Transition (or end-cap) glyph
        if i < last {
            let next_style = theme.style_for(segs[i + 1].kind);
            let prev_bg_as_fg = style.bg.map(Color::fg_seq).unwrap_or_default();
            let next_bg = next_style.bg.map(Color::bg_seq).unwrap_or_default();
            let _ = write!(out, "{prev_bg_as_fg}{next_bg}{glyph}{RESET}");
        } else {
            // End-cap: the glyph in the last segment's bg color, on the
            // terminal background.
            let prev_bg_as_fg = style.bg.map(Color::fg_seq).unwrap_or_default();
            let _ = write!(out, "{prev_bg_as_fg}{BG_RESET}{glyph}{RESET}");
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Themes — palettes (RGB values sourced from upstream theme catalogs)
// ---------------------------------------------------------------------------

// Each themes block packs styles in the same order as `SegmentKind`:
// Module, Git, Context, Duration, Savings, Diff, Cost, Model, Version, ScanProgress.

/// `default` — pipes, ANSI 8 colors, no bg. Looks like a classic terminal
/// prompt; safe on any terminal.
pub const DEFAULT: Theme = Theme {
    id: ThemeId::Default,
    separator: Separator::Pipe,
    requires_nerdfont: false,
    styles: [
        // Module — bold white
        Style::fg_bold(Color::Ansi(7)),
        // Git — cyan
        Style::fg(Color::Ansi(6)),
        // Context — green (theme default; cost-segment-style override happens
        // at builder time for the % threshold)
        Style::fg(Color::Ansi(2)),
        // Duration — gray (bright black)
        Style::fg(Color::Ansi(8)),
        // Savings — green
        Style::fg(Color::Ansi(2)),
        // Diff — gray (the `+N-N` is its own visual indicator; one color is OK)
        Style::fg(Color::Ansi(8)),
        // Cost — green default (builder overrides for thresholds)
        Style::fg(Color::Ansi(2)),
        // Model — blue
        Style::fg(Color::Ansi(4)),
        // Version — dim white
        Style::fg(Color::Ansi(8)),
        // ScanProgress — yellow (in-flight signal)
        Style::fg(Color::Ansi(3)),
    ],
};

/// `minimal` — same palette as `default` but separator is a double space.
/// Cleanest visual; still no Nerd Font requirement.
pub const MINIMAL: Theme = Theme {
    id: ThemeId::Minimal,
    separator: Separator::Whitespace,
    requires_nerdfont: false,
    styles: DEFAULT.styles,
};

/// `catppuccin` — Mocha palette, powerline. Default theme.
///
/// Palette: Base #1e1e2e, Crust #11111b, Mauve #cba6f7, Sapphire #74c7ec,
/// Green #a6e3a1, Peach #fab387, Yellow #f9e2af, Pink #f5c2e7, Text #cdd6f4.
pub const CATPPUCCIN: Theme = Theme {
    id: ThemeId::Catppuccin,
    separator: Separator::Powerline { glyph: POWERLINE_RIGHT },
    requires_nerdfont: true,
    styles: [
        // Module — base on mauve, bold (the head cap)
        Style::pl_bold(Color::Rgb(0x1e, 0x1e, 0x2e), Color::Rgb(0xcb, 0xa6, 0xf7)),
        // Git — green on crust
        Style::pl(Color::Rgb(0xa6, 0xe3, 0xa1), Color::Rgb(0x18, 0x18, 0x25)),
        // Context — sapphire on crust
        Style::pl(Color::Rgb(0x74, 0xc7, 0xec), Color::Rgb(0x18, 0x18, 0x25)),
        // Duration — text on crust
        Style::pl(Color::Rgb(0xcd, 0xd6, 0xf4), Color::Rgb(0x11, 0x11, 0x1b)),
        // Savings — yellow on crust
        Style::pl(Color::Rgb(0xf9, 0xe2, 0xaf), Color::Rgb(0x18, 0x18, 0x25)),
        // Diff — peach on crust
        Style::pl(Color::Rgb(0xfa, 0xb3, 0x87), Color::Rgb(0x18, 0x18, 0x25)),
        // Cost — green on crust
        Style::pl(Color::Rgb(0xa6, 0xe3, 0xa1), Color::Rgb(0x18, 0x18, 0x25)),
        // Model — base on sapphire (mirrors module cap, balances the line)
        Style::pl_bold(Color::Rgb(0x1e, 0x1e, 0x2e), Color::Rgb(0x74, 0xc7, 0xec)),
        // Version — pink on crust (tail accent)
        Style::pl(Color::Rgb(0xf5, 0xc2, 0xe7), Color::Rgb(0x11, 0x11, 0x1b)),
        // ScanProgress — base on peach, bold (active-work chip)
        Style::pl_bold(Color::Rgb(0x1e, 0x1e, 0x2e), Color::Rgb(0xfa, 0xb3, 0x87)),
    ],
};

/// `tokyo-night` — Tokyo Night palette, powerline. Dark blues, magenta accents.
///
/// Palette: BG #1a1b26, BG-Storm #24283b, FG #c0caf5, Blue #7aa2f7,
/// Cyan #7dcfff, Green #9ece6a, Magenta #bb9af7, Yellow #e0af68, Red #f7768e.
pub const TOKYO_NIGHT: Theme = Theme {
    id: ThemeId::TokyoNight,
    separator: Separator::Powerline { glyph: POWERLINE_RIGHT },
    requires_nerdfont: true,
    styles: [
        // Module — bg on blue, bold (head cap)
        Style::pl_bold(Color::Rgb(0x1a, 0x1b, 0x26), Color::Rgb(0x7a, 0xa2, 0xf7)),
        // Git — green on bg-storm
        Style::pl(Color::Rgb(0x9e, 0xce, 0x6a), Color::Rgb(0x24, 0x28, 0x3b)),
        // Context — cyan on bg-storm
        Style::pl(Color::Rgb(0x7d, 0xcf, 0xff), Color::Rgb(0x24, 0x28, 0x3b)),
        // Duration — fg on bg
        Style::pl(Color::Rgb(0xc0, 0xca, 0xf5), Color::Rgb(0x1a, 0x1b, 0x26)),
        // Savings — yellow on bg-storm
        Style::pl(Color::Rgb(0xe0, 0xaf, 0x68), Color::Rgb(0x24, 0x28, 0x3b)),
        // Diff — magenta on bg-storm
        Style::pl(Color::Rgb(0xbb, 0x9a, 0xf7), Color::Rgb(0x24, 0x28, 0x3b)),
        // Cost — green on bg-storm
        Style::pl(Color::Rgb(0x9e, 0xce, 0x6a), Color::Rgb(0x24, 0x28, 0x3b)),
        // Model — bg on magenta (mirrors module cap on the right side)
        Style::pl_bold(Color::Rgb(0x1a, 0x1b, 0x26), Color::Rgb(0xbb, 0x9a, 0xf7)),
        // Version — fg dim on bg
        Style::pl(Color::Rgb(0x56, 0x5f, 0x89), Color::Rgb(0x1a, 0x1b, 0x26)),
        // ScanProgress — bg on yellow, bold (active-work chip)
        Style::pl_bold(Color::Rgb(0x1a, 0x1b, 0x26), Color::Rgb(0xe0, 0xaf, 0x68)),
    ],
};

/// `pastel-powerline` — pastel rainbow over a dark crust background.
///
/// Bg rotates through pastel hues so the whole line reads as a soft ribbon.
pub const PASTEL_POWERLINE: Theme = Theme {
    id: ThemeId::PastelPowerline,
    separator: Separator::Powerline { glyph: POWERLINE_RIGHT },
    requires_nerdfont: true,
    styles: [
        // Module — crust on pastel pink, bold
        Style::pl_bold(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0xf5, 0xc2, 0xe7)),
        // Git — crust on pastel peach
        Style::pl(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0xfa, 0xb3, 0x87)),
        // Context — crust on pastel yellow
        Style::pl(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0xf9, 0xe2, 0xaf)),
        // Duration — crust on pastel green
        Style::pl(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0xa6, 0xe3, 0xa1)),
        // Savings — crust on pastel teal
        Style::pl(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0x94, 0xe2, 0xd5)),
        // Diff — crust on pastel sapphire
        Style::pl(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0x74, 0xc7, 0xec)),
        // Cost — crust on pastel mauve
        Style::pl(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0xcb, 0xa6, 0xf7)),
        // Model — crust on pastel lavender
        Style::pl_bold(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0xb4, 0xbe, 0xfe)),
        // Version — crust on muted pink
        Style::pl(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0xea, 0x9a, 0x97)),
        // ScanProgress — crust on pastel yellow, bold (active-work chip)
        Style::pl_bold(Color::Rgb(0x11, 0x11, 0x1b), Color::Rgb(0xf9, 0xe2, 0xaf)),
    ],
};

/// `gruvbox-rainbow` — gruvbox earthy palette, powerline rainbow.
///
/// Palette: BG0 #282828, Yellow #d79921, Green #98971a, Aqua #689d6a,
/// Purple #b16286, Orange #d65d0e, Red #cc241d, FG #ebdbb2.
pub const GRUVBOX_RAINBOW: Theme = Theme {
    id: ThemeId::GruvboxRainbow,
    separator: Separator::Powerline { glyph: POWERLINE_RIGHT },
    requires_nerdfont: true,
    styles: [
        // Module — bg on yellow, bold (head)
        Style::pl_bold(Color::Rgb(0x28, 0x28, 0x28), Color::Rgb(0xd7, 0x99, 0x21)),
        // Git — bg on green
        Style::pl(Color::Rgb(0x28, 0x28, 0x28), Color::Rgb(0x98, 0x97, 0x1a)),
        // Context — bg on aqua
        Style::pl(Color::Rgb(0x28, 0x28, 0x28), Color::Rgb(0x68, 0x9d, 0x6a)),
        // Duration — fg on bg0_h
        Style::pl(Color::Rgb(0xeb, 0xdb, 0xb2), Color::Rgb(0x1d, 0x20, 0x21)),
        // Savings — bg on orange
        Style::pl(Color::Rgb(0x28, 0x28, 0x28), Color::Rgb(0xd6, 0x5d, 0x0e)),
        // Diff — bg on purple
        Style::pl(Color::Rgb(0x28, 0x28, 0x28), Color::Rgb(0xb1, 0x62, 0x86)),
        // Cost — bg on aqua-dim
        Style::pl(Color::Rgb(0x28, 0x28, 0x28), Color::Rgb(0x83, 0xa5, 0x98)),
        // Model — bg on red (right-side accent)
        Style::pl_bold(Color::Rgb(0x28, 0x28, 0x28), Color::Rgb(0xcc, 0x24, 0x1d)),
        // Version — fg dim on bg0_h
        Style::pl(Color::Rgb(0xa8, 0x99, 0x84), Color::Rgb(0x1d, 0x20, 0x21)),
        // ScanProgress — bg on orange, bold (active-work chip; distinct from the
        // yellow module head it sits next to)
        Style::pl_bold(Color::Rgb(0x28, 0x28, 0x28), Color::Rgb(0xd6, 0x5d, 0x0e)),
    ],
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::statusline::segment::{Segment, SegmentKind};

    fn seg(kind: SegmentKind, text: &str) -> Segment {
        Segment::new(kind, text)
    }

    #[test]
    fn theme_id_parse_normalizes_input() {
        assert_eq!(ThemeId::parse("default"), Some(ThemeId::Default));
        assert_eq!(ThemeId::parse(" DEFAULT "), Some(ThemeId::Default));
        assert_eq!(ThemeId::parse("Tokyo_Night"), Some(ThemeId::TokyoNight));
        assert_eq!(ThemeId::parse("catppuccin-mocha"), Some(ThemeId::Catppuccin));
        assert_eq!(ThemeId::parse("gruvbox"), Some(ThemeId::GruvboxRainbow));
        assert_eq!(ThemeId::parse(""), None);
        assert_eq!(ThemeId::parse("bogus"), None);
    }

    #[test]
    fn theme_id_all_includes_every_variant() {
        // If someone adds a ThemeId variant they must also add it to ALL,
        // otherwise it won't show up in --preview. Spot-checking length keeps
        // that honest.
        assert_eq!(ThemeId::ALL.len(), 6);
    }

    #[test]
    fn theme_id_theme_resolves_for_every_variant() {
        // Doesn't crash and returns a non-empty palette for each.
        for id in ThemeId::ALL {
            let theme = id.theme();
            assert_eq!(theme.id, *id);
            // Style array is full
            assert_eq!(theme.styles.len(), SEGMENT_KIND_COUNT);
        }
    }

    #[test]
    fn color_fg_seq_matches_ansi_truecolor_shape() {
        assert_eq!(Color::Ansi(6).fg_seq(), "\x1b[36m");
        assert_eq!(Color::Ansi(9).fg_seq(), "\x1b[91m");
        assert_eq!(Color::Rgb(10, 20, 30).fg_seq(), "\x1b[38;2;10;20;30m");
        assert_eq!(Color::Rgb(10, 20, 30).bg_seq(), "\x1b[48;2;10;20;30m");
    }

    #[test]
    fn render_pipe_contains_pipe_separator() {
        let segs = vec![
            seg(SegmentKind::Module, "mustard"),
            seg(SegmentKind::Git, "dev"),
        ];
        let out = render_line(&DEFAULT, &segs);
        assert!(out.contains("mustard"));
        assert!(out.contains("dev"));
        assert!(out.contains("\u{2502}"), "pipe separator missing from {out:?}");
    }

    #[test]
    fn render_whitespace_has_no_pipe() {
        let segs = vec![
            seg(SegmentKind::Module, "mustard"),
            seg(SegmentKind::Git, "dev"),
        ];
        let out = render_line(&MINIMAL, &segs);
        assert!(out.contains("mustard"));
        assert!(out.contains("dev"));
        assert!(!out.contains("\u{2502}"), "minimal must not use pipe");
    }

    #[test]
    fn render_powerline_emits_transition_glyphs() {
        let segs = vec![
            seg(SegmentKind::Module, "mustard"),
            seg(SegmentKind::Git, "dev"),
            seg(SegmentKind::Model, "Opus"),
        ];
        let out = render_line(&CATPPUCCIN, &segs);
        // Three segments → at least 2 transition glyphs + 1 end-cap = 3.
        let glyph_count = out.chars().filter(|c| *c == POWERLINE_RIGHT).count();
        assert!(glyph_count >= 3, "expected >=3 powerline glyphs, got {glyph_count} in {out:?}");
    }

    #[test]
    fn render_powerline_handles_single_segment_with_end_cap() {
        let segs = vec![seg(SegmentKind::Module, "mustard")];
        let out = render_line(&CATPPUCCIN, &segs);
        // Even with one segment, the end-cap glyph appears once.
        let glyph_count = out.chars().filter(|c| *c == POWERLINE_RIGHT).count();
        assert_eq!(glyph_count, 1, "single segment needs exactly one end-cap glyph");
    }

    #[test]
    fn render_line_empty_returns_empty() {
        assert_eq!(render_line(&DEFAULT, &[]), "");
        assert_eq!(render_line(&CATPPUCCIN, &[]), "");
    }

    #[test]
    fn render_honors_segment_override_fg() {
        // override_fg should win over the theme's per-kind fg. Easiest to
        // verify with the default theme where fg sequences are short.
        let mut s = seg(SegmentKind::Cost, "$10.00");
        s.override_fg = Some(Color::Ansi(1)); // red
        let out = render_line(&DEFAULT, &[s]);
        assert!(out.contains("\x1b[31m"), "override fg sequence missing from {out:?}");
    }
}
