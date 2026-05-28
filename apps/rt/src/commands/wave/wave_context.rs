//! `wave_context` — canonical `_context.md` generator for the next wave (N+1).
//!
//! Spec A v4 / Wave 3 — `_context.md` is the input handed to the agent that
//! will execute the upcoming wave. The schema has **5 required sections**;
//! every heading flows through [`mustard_core::i18n::translate`] so the
//! output follows the project locale. AC-A-9 caps the rendered body at
//! **8 000 words** — over the cap, [`build`] returns an explicit
//! [`WaveSummaryError::ContextTooLong`] rather than silently truncating.
//!
//! ## Schema (AC-A-9)
//!
//! ```text
//! ## {heading.context.objective}
//! ## {heading.context.inheritance}
//! ## {heading.context.memory}
//! ## {heading.context.position}
//! ## {heading.context.next_steps_suggestion}
//! ```
//!
//! ## Design
//!
//! - **Hard cap, never silent truncation.** The renderer always emits the
//!   full body; the cap is enforced as a post-render check that yields a
//!   typed error. Callers decide how to react (trim inheritance, drop memory,
//!   surface to the user).
//! - **i18n-pure.** Same contract as
//!   [`crate::commands::wave::wave_summary`] — no hardcoded user-facing strings.
//! - **Idempotent.** Pure on `(input, locale)`. No clock, no env.

use mustard_core::fs as mfs;
use mustard_core::i18n::{translate, SupportedLocale as Locale};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::commands::wave::wave_summary::{WaveSummaryError, WikiLink};

/// Hard cap on the rendered `_context.md` word count — AC-A-9.
pub const CONTEXT_WORD_CAP: usize = 8_000;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One slot in the `## Posição no mapa` section — a wave id + one-line status
/// description. The renderer prints these as a bulleted list; the optional
/// `current` flag emits a leading marker so the agent can locate "you are
/// here" at a glance without parsing wave numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveMapEntry {
    /// Wave directory name (e.g. `wave-3-rt`). Rendered as a wikilink.
    pub wave_id: String,
    /// One-line status / outcome description.
    pub status: String,
    /// `true` when this entry is the upcoming wave (the focus of the
    /// generated `_context.md`). Rendered with a `>` marker.
    pub current: bool,
}

/// Input to [`build`]. Every field is data; headings are catalogue-driven.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveContextInput {
    /// Spec slug under `.claude/spec/`. Used by [`write`] for path resolution.
    pub spec_slug: String,
    /// Target wave directory name — the wave the context is being generated
    /// **for** (typically wave N+1). Used by [`write`].
    pub wave_id: String,
    /// Free-form objective of the upcoming wave (domain content).
    pub objective: String,
    /// Wikilinks to prior `_summary.md` files / memory notes the upcoming
    /// wave inherits.
    pub inheritance: Vec<WikiLink>,
    /// Wikilinks to relevant spec-memory entries (`.claude/spec/<slug>/memory/`).
    pub memory: Vec<WikiLink>,
    /// The wave map — every wave in the spec with its status, with one entry
    /// flagged `current`.
    pub position: Vec<WaveMapEntry>,
    /// Free-form bullets suggesting concrete next-steps for the agent.
    pub next_steps_suggestion: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render the canonical `_context.md` body for the upcoming wave.
///
/// Pure on `(input, locale)` — same input twice produces byte-identical
/// output. Enforces the 8 000-word cap (AC-A-9): when the rendered body
/// exceeds the cap, returns [`WaveSummaryError::ContextTooLong`] carrying
/// the actual word count so the caller can decide how to recover.
///
/// # Errors
///
/// Returns [`WaveSummaryError::ContextTooLong`] when the rendered body
/// exceeds [`CONTEXT_WORD_CAP`] words.
pub fn build(input: &WaveContextInput, locale: Locale) -> Result<String, WaveSummaryError> {
    let body = render_body(input, locale);
    let words = count_words(&body);
    if words > CONTEXT_WORD_CAP {
        return Err(WaveSummaryError::ContextTooLong {
            actual_words: words,
            cap: CONTEXT_WORD_CAP,
        });
    }
    Ok(body)
}

/// Atomically write a rendered `_context.md` body to
/// `{spec_root}/{wave_id}/_context.md`. Returns the resolved path on success.
///
/// # Errors
///
/// Returns [`WaveSummaryError::Io`] when the underlying atomic write fails.
pub fn write(
    spec_root: &Path,
    wave_id: &str,
    content: &str,
) -> Result<PathBuf, WaveSummaryError> {
    let target = spec_root.join(wave_id).join("_context.md");
    mfs::write_atomic(&target, content.as_bytes()).map_err(|e| WaveSummaryError::Io {
        path: target.clone(),
        source: std::io::Error::other(e.to_string()),
    })?;
    Ok(target)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// The unguarded body renderer. Kept separate from [`build`] so cap-violation
/// reporting can quote the actual word count without re-rendering.
fn render_body(input: &WaveContextInput, locale: Locale) -> String {
    let mut body = String::new();

    // 1. Objetivo
    write_heading(&mut body, "heading.context.objective", locale);
    if input.objective.trim().is_empty() {
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
    } else {
        let _ = writeln!(body, "{}", input.objective.trim_end());
    }
    body.push('\n');

    // 2. Herança
    write_heading(&mut body, "heading.context.inheritance", locale);
    write_wikilink_list(&mut body, &input.inheritance, locale);
    body.push('\n');

    // 3. Memória
    write_heading(&mut body, "heading.context.memory", locale);
    write_wikilink_list(&mut body, &input.memory, locale);
    body.push('\n');

    // 4. Posição no mapa
    write_heading(&mut body, "heading.context.position", locale);
    write_wave_map(&mut body, &input.position, locale);
    body.push('\n');

    // 5. Sugestão de próximos passos
    write_heading(&mut body, "heading.context.next_steps_suggestion", locale);
    write_text_bullets(&mut body, &input.next_steps_suggestion, locale);

    body
}

fn write_heading(body: &mut String, key: &str, locale: Locale) {
    let _ = writeln!(body, "## {}", translate(key, locale));
    body.push('\n');
}

fn write_wikilink_list(body: &mut String, links: &[WikiLink], locale: Locale) {
    if links.is_empty() {
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
        return;
    }
    for link in links {
        let _ = writeln!(body, "- {}", link.render());
    }
}

fn write_text_bullets(body: &mut String, items: &[String], locale: Locale) {
    if items.is_empty() {
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
        return;
    }
    for item in items {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _ = writeln!(body, "- {trimmed}");
    }
}

/// Render the wave map. The "current" entry is prefixed with `>` so the
/// agent can locate it without scanning numbers. Structural separator only;
/// no user-facing string.
fn write_wave_map(body: &mut String, entries: &[WaveMapEntry], locale: Locale) {
    if entries.is_empty() {
        let _ = writeln!(body, "{}", translate("placeholder.fill", locale));
        return;
    }
    for entry in entries {
        let marker = if entry.current { "> " } else { "- " };
        let _ = writeln!(
            body,
            "{marker}[[{}]] — {}",
            entry.wave_id,
            entry.status.trim()
        );
    }
}

/// Count whitespace-separated tokens in `text`. The cap is intentionally a
/// *word* count rather than a byte/character count — the agent prompt budget
/// downstream of `_context.md` is measured in tokens, and "words" tracks
/// token volume more closely than raw length across PT-BR and EN-US text.
fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn small_input() -> WaveContextInput {
        WaveContextInput {
            spec_slug: "demo".into(),
            wave_id: "wave-4-rt".into(),
            objective: "Continuar a entrega do gate de regressao.".into(),
            inheritance: vec![WikiLink::new("wave-3-rt/_summary")],
            memory: vec![WikiLink::new("memory/scan-rust-first")],
            position: vec![
                WaveMapEntry {
                    wave_id: "wave-3-rt".into(),
                    status: "Done".into(),
                    current: false,
                },
                WaveMapEntry {
                    wave_id: "wave-4-rt".into(),
                    status: "Active".into(),
                    current: true,
                },
            ],
            next_steps_suggestion: vec!["Capturar snapshot pre".into()],
        }
    }

    #[test]
    fn build_emits_five_required_headings_pt_br() {
        let body = build(&small_input(), Locale::PtBr).expect("within cap");
        for key in [
            "heading.context.objective",
            "heading.context.inheritance",
            "heading.context.memory",
            "heading.context.position",
            "heading.context.next_steps_suggestion",
        ] {
            let expected = format!("## {}", translate(key, Locale::PtBr));
            assert!(body.contains(&expected), "missing heading {expected}\n{body}");
        }
    }

    #[test]
    fn build_emits_five_required_headings_en_us() {
        let body = build(&small_input(), Locale::EnUs).expect("within cap");
        for key in [
            "heading.context.objective",
            "heading.context.inheritance",
            "heading.context.memory",
            "heading.context.position",
            "heading.context.next_steps_suggestion",
        ] {
            let expected = format!("## {}", translate(key, Locale::EnUs));
            assert!(body.contains(&expected), "missing heading {expected}\n{body}");
        }
    }

    #[test]
    fn build_is_idempotent() {
        let input = small_input();
        let a = build(&input, Locale::PtBr).unwrap();
        let b = build(&input, Locale::PtBr).unwrap();
        assert_eq!(a, b);
    }

    /// AC-A-9 — a synthetic 12-wave spec stays under the 8 000-word cap.
    /// We populate every section with bounded amounts of content typical of
    /// the actual harness output (12 prior summaries inherited, 30 memory
    /// notes, full wave map, a handful of next-step suggestions).
    #[test]
    fn test_context_within_8k_words_for_12_wave_spec() {
        let inheritance: Vec<WikiLink> = (0..12)
            .map(|n| WikiLink::new(format!("wave-{n}-rt/_summary")))
            .collect();
        let memory: Vec<WikiLink> = (0..30)
            .map(|n| WikiLink::new(format!("memory/note-{n}")))
            .collect();
        let position: Vec<WaveMapEntry> = (0..13)
            .map(|n| WaveMapEntry {
                wave_id: format!("wave-{n}-rt"),
                status: "Concluded short status line".into(),
                current: n == 12,
            })
            .collect();
        let next_steps_suggestion: Vec<String> = (0..6)
            .map(|n| format!("Capturar snapshot pre wave {n}"))
            .collect();

        let input = WaveContextInput {
            spec_slug: "2026-05-27-mustard-v4-foundation".into(),
            wave_id: "wave-12-rt".into(),
            objective: "Render a canonical context for the upcoming wave."
                .repeat(5),
            inheritance,
            memory,
            position,
            next_steps_suggestion,
        };
        let body = build(&input, Locale::PtBr).expect("within 8000-word cap");
        let words = count_words(&body);
        assert!(
            words <= CONTEXT_WORD_CAP,
            "rendered context body has {words} words; cap is {CONTEXT_WORD_CAP}"
        );
    }

    /// 50-wave spec with very long status lines forces the renderer over the
    /// cap. `build` must surface `ContextTooLong` (no silent truncation).
    #[test]
    fn test_context_too_long_returns_error() {
        // Each wave's status line is intentionally long so the cap is busted
        // by inputs that nominally fit the wave-count budget.
        let big_status: String = std::iter::repeat("regressao detectada palavra ")
            .take(60)
            .collect();
        let position: Vec<WaveMapEntry> = (0..50)
            .map(|n| WaveMapEntry {
                wave_id: format!("wave-{n}-rt"),
                status: big_status.clone(),
                current: n == 49,
            })
            .collect();
        let memory: Vec<WikiLink> = (0..50)
            .map(|n| WikiLink::new(format!("memory/note-very-long-name-{n}")))
            .collect();

        let input = WaveContextInput {
            spec_slug: "stress".into(),
            wave_id: "wave-50-rt".into(),
            objective: "Carry the cap-violation case".into(),
            inheritance: (0..50)
                .map(|n| WikiLink::new(format!("wave-{n}-rt/_summary")))
                .collect(),
            memory,
            position,
            next_steps_suggestion: vec!["next".into(); 50],
        };
        let err = build(&input, Locale::PtBr).expect_err("expected cap violation");
        match err {
            WaveSummaryError::ContextTooLong { actual_words, cap } => {
                assert_eq!(cap, CONTEXT_WORD_CAP);
                assert!(
                    actual_words > CONTEXT_WORD_CAP,
                    "actual_words {actual_words} must exceed cap {cap}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn write_creates_context_at_wave_dir() {
        let dir = tempfile::tempdir().unwrap();
        let written = write(dir.path(), "wave-4-rt", "## Objetivo\n\nbody\n")
            .expect("write succeeds");
        assert_eq!(written, dir.path().join("wave-4-rt").join("_context.md"));
        assert_eq!(
            std::fs::read_to_string(&written).unwrap(),
            "## Objetivo\n\nbody\n"
        );
    }

    #[test]
    fn wave_map_marks_current_entry() {
        let body = build(&small_input(), Locale::PtBr).unwrap();
        // The current entry uses `>` while non-current entries use `-`.
        assert!(body.contains("> [[wave-4-rt]]"));
        assert!(body.contains("- [[wave-3-rt]]"));
    }
}
