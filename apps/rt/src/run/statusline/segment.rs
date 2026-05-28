//! Statusline segments — pure data ([`Segment`]) plus the per-kind builders
//! that turn the harness JSON payload into segments. Themes (in `theme.rs`)
//! own all color/separator decisions; this module only produces *text*.
//!
//! The one exception is [`Segment::override_fg`], used by [`cost_segment`]
//! when the per-segment threshold (green / yellow / red) needs to override
//! the theme default. Theme renderers honor it.

use super::theme::Color;
use crate::run::economy::rtk_gain::get_rtk_gain;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// All segment kinds the statusline knows how to render. New kinds must be
/// appended (themes index a `[Style; SEGMENT_KIND_COUNT]` by `kind as usize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SegmentKind {
    Module = 0,
    Git = 1,
    Context = 2,
    Duration = 3,
    Savings = 4,
    Diff = 5,
    Cost = 6,
    Model = 7,
    Version = 8,
}

/// Count of kinds — keep in sync with the last variant.
pub const SEGMENT_KIND_COUNT: usize = 9;

/// A single line element with no theme coupling. Builders return
/// `Option<Segment>` so a missing payload field omits the segment cleanly.
#[derive(Debug, Clone)]
pub struct Segment {
    pub kind: SegmentKind,
    pub text: String,
    /// Per-render fg override — used by `cost_segment` and `context_segment`
    /// for threshold coloring. **Honored only by flat separators**
    /// (`Pipe` / `Whitespace`). Powerline themes ignore it so the palette
    /// stays harmonic; the override clashing with a fixed bg looks worse than
    /// the missing signal.
    pub override_fg: Option<Color>,
}

impl Segment {
    #[must_use]
    pub fn new(kind: SegmentKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            override_fg: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Builders — one per kind. Each is `pub` so `preview.rs` can build a
// synthetic line. The orchestration that picks which builders to call lives
// in `mod.rs`.
// ---------------------------------------------------------------------------

/// `cwd` basename. Falls back to `"?"` if cwd has no file name.
#[must_use]
pub fn module_segment(cwd: &Path) -> Segment {
    let module = cwd
        .file_name()
        .map_or_else(|| "?".to_string(), |n| n.to_string_lossy().to_string());
    Segment::new(SegmentKind::Module, module)
}

/// `⎇ branch +N~N?N` or `⎇ branch ✓`. Returns `None` when `cwd` is not a git
/// repository or the `git` binary is unavailable.
#[must_use]
pub fn git_segment(cwd: &Path) -> Option<Segment> {
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let porcelain = git(cwd, &["status", "--porcelain"]).unwrap_or_default();
    let (mut staged, mut modified, mut untracked) = (0u32, 0u32, 0u32);
    for line in porcelain.lines() {
        if line.starts_with("??") {
            untracked += 1;
        } else {
            let mut chars = line.chars();
            let x = chars.next().unwrap_or(' ');
            let y = chars.next().unwrap_or(' ');
            if matches!(x, 'M' | 'A' | 'D' | 'R' | 'C') {
                staged += 1;
            }
            if matches!(y, 'M' | 'D') {
                modified += 1;
            }
        }
    }
    let mut status = String::new();
    if staged > 0 {
        let _ = write!(status, "+{staged}");
    }
    if modified > 0 {
        let _ = write!(status, "~{modified}");
    }
    if untracked > 0 {
        let _ = write!(status, "?{untracked}");
    }
    let suffix = if status.is_empty() {
        " \u{2713}".to_string()
    } else {
        format!(" {status}")
    };
    Some(Segment::new(
        SegmentKind::Git,
        format!("\u{2387} {branch}{suffix}"),
    ))
}

/// 10-cell bar + `NN%` + token count (`NNNk`). Returns `None` when the
/// `context_window.remaining_percentage` field is missing.
#[must_use]
pub fn context_segment(data: &Value) -> Option<Segment> {
    let ctx = data.get("context_window")?;
    let rem = ctx.get("remaining_percentage")?.as_f64()?;
    let pct = rem.round() as i64;
    let bar_len = 10i64;
    let used = (((100 - pct) as f64 / 100.0) * bar_len as f64).round() as i64;
    let used = used.clamp(0, bar_len);
    let bar = format!(
        "{}{}",
        "\u{2588}".repeat(used as usize),
        "\u{2591}".repeat((bar_len - used) as usize),
    );
    let in_tok = ctx
        .get("total_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let out_tok = ctx
        .get("total_output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total_k = (in_tok + out_tok) / 1000;
    let exceeds = data.get("exceeds_200k_tokens") == Some(&Value::Bool(true));
    let warn = if exceeds { " \u{26A0}>200k" } else { "" };
    let mut s = Segment::new(SegmentKind::Context, format!("{bar} {pct}% {total_k}k{warn}"));
    // Threshold-driven fg override: red <20% or exceeds 200k, yellow <40%
    if exceeds || pct < 20 {
        s.override_fg = Some(Color::Ansi(9)); // bright red
    } else if pct < 40 {
        s.override_fg = Some(Color::Ansi(1)); // red
    } else if pct < 60 {
        s.override_fg = Some(Color::Ansi(3)); // yellow
    }
    Some(s)
}

/// `Nm Ns` or `Ns`. Returns `None` when duration is zero/missing.
#[must_use]
pub fn duration_segment(data: &Value) -> Option<Segment> {
    let dur_ms = data.get("cost")?.get("total_duration_ms")?.as_i64()?;
    if dur_ms <= 0 {
        return None;
    }
    let m = dur_ms / 60_000;
    let s = (dur_ms % 60_000) / 1000;
    let text = if m > 0 {
        if s > 0 {
            format!("{m}m{s}s")
        } else {
            format!("{m}m")
        }
    } else {
        format!("{s}s")
    };
    Some(Segment::new(SegmentKind::Duration, text))
}

/// `⚡ NN% NNNk saved`. Returns `None` when RTK has nothing to report.
#[must_use]
pub fn savings_segment() -> Option<Segment> {
    let gain = get_rtk_gain()?;
    if gain.saved <= 0 && gain.pct <= 0.0 {
        return None;
    }
    let saved_k = (gain.saved as f64 / 1000.0).round() as i64;
    let pct = gain.pct.round() as i64;
    Some(Segment::new(
        SegmentKind::Savings,
        format!("\u{26A1} {pct}% {saved_k}k saved"),
    ))
}

/// `+N-N`. Returns `None` when both numbers are zero.
#[must_use]
pub fn diff_segment(data: &Value) -> Option<Segment> {
    let la = data
        .get("cost")
        .and_then(|c| c.get("total_lines_added"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let lr = data
        .get("cost")
        .and_then(|c| c.get("total_lines_removed"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if la == 0 && lr == 0 {
        return None;
    }
    let mut parts = String::new();
    if la > 0 {
        let _ = write!(parts, "+{la}");
    }
    if lr > 0 {
        let _ = write!(parts, "-{lr}");
    }
    Some(Segment::new(SegmentKind::Diff, parts))
}

/// `$0.42` etc. Returns `None` when the cost field is missing or zero.
/// Threshold override on fg: green <$1, yellow <$5, red >=$5.
#[must_use]
pub fn cost_segment(data: &Value) -> Option<Segment> {
    let usd = data
        .get("cost")
        .and_then(|c| c.get("total_cost_usd"))
        .and_then(Value::as_f64)?;
    if usd <= 0.0 {
        return None;
    }
    let text = format!("${usd:.2}");
    let mut s = Segment::new(SegmentKind::Cost, text);
    s.override_fg = Some(if usd >= 5.0 {
        Color::Ansi(1) // red
    } else if usd >= 1.0 {
        Color::Ansi(3) // yellow
    } else {
        Color::Ansi(2) // green
    });
    Some(s)
}

/// `Opus 4.7` etc. Strips the `Claude ` / `claude-` prefix to keep the line
/// tight.
#[must_use]
pub fn model_segment(data: &Value) -> Segment {
    let raw = data
        .get("model")
        .and_then(|m| m.get("display_name").or_else(|| m.get("id")))
        .and_then(Value::as_str)
        .unwrap_or("Claude");
    let short = raw
        .strip_prefix("Claude ")
        .or_else(|| raw.strip_prefix("claude-"))
        .unwrap_or(raw);
    Segment::new(SegmentKind::Model, short.to_string())
}

/// `vX.Y.Z`. Returns `None` when version is missing.
#[must_use]
pub fn version_segment(data: &Value) -> Option<Segment> {
    let v = data.get("version").and_then(Value::as_str)?;
    Some(Segment::new(SegmentKind::Version, format!("v{v}")))
}

// ---------------------------------------------------------------------------
// git helper — local to this module
// ---------------------------------------------------------------------------

fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn module_segment_uses_cwd_basename() {
        let m = module_segment(Path::new("/tmp/foo-project"));
        assert_eq!(m.text, "foo-project");
        assert_eq!(m.kind, SegmentKind::Module);
    }

    #[test]
    fn context_segment_renders_bar() {
        let seg = context_segment(&json!({
            "context_window": {
                "remaining_percentage": 70,
                "total_input_tokens": 50000,
                "total_output_tokens": 10000,
            }
        }))
        .unwrap();
        assert!(seg.text.contains("70%"));
        assert!(seg.text.contains("60k"));
        // 70% is above all thresholds → no override
        assert!(seg.override_fg.is_none());
    }

    #[test]
    fn context_segment_low_pct_overrides_fg_red() {
        let seg = context_segment(&json!({
            "context_window": { "remaining_percentage": 10 }
        }))
        .unwrap();
        assert!(seg.override_fg.is_some());
    }

    #[test]
    fn duration_segment_formats_minutes() {
        let seg = duration_segment(&json!({ "cost": { "total_duration_ms": 125_000 } })).unwrap();
        assert_eq!(seg.text, "2m5s");
    }

    #[test]
    fn duration_segment_none_when_zero() {
        assert!(duration_segment(&json!({ "cost": { "total_duration_ms": 0 } })).is_none());
    }

    #[test]
    fn diff_segment_omits_when_both_zero() {
        assert!(diff_segment(&json!({ "cost": {} })).is_none());
        let seg = diff_segment(&json!({
            "cost": { "total_lines_added": 100, "total_lines_removed": 5 }
        }))
        .unwrap();
        assert_eq!(seg.text, "+100-5");
    }

    #[test]
    fn cost_segment_threshold_green_yellow_red() {
        let s50c = cost_segment(&json!({ "cost": { "total_cost_usd": 0.50 } })).unwrap();
        assert_eq!(s50c.text, "$0.50");
        // green = Ansi(2)
        assert!(matches!(s50c.override_fg, Some(Color::Ansi(2))));

        let s3 = cost_segment(&json!({ "cost": { "total_cost_usd": 3.00 } })).unwrap();
        assert_eq!(s3.text, "$3.00");
        // yellow = Ansi(3)
        assert!(matches!(s3.override_fg, Some(Color::Ansi(3))));

        let s12 = cost_segment(&json!({ "cost": { "total_cost_usd": 12.5 } })).unwrap();
        assert_eq!(s12.text, "$12.50");
        // red = Ansi(1)
        assert!(matches!(s12.override_fg, Some(Color::Ansi(1))));
    }

    #[test]
    fn cost_segment_none_when_missing_or_zero() {
        assert!(cost_segment(&json!({})).is_none());
        assert!(cost_segment(&json!({ "cost": {} })).is_none());
        assert!(cost_segment(&json!({ "cost": { "total_cost_usd": 0.0 } })).is_none());
    }

    #[test]
    fn model_segment_strips_prefixes() {
        let s = model_segment(&json!({ "model": { "display_name": "Claude Opus 4.7" } }));
        assert_eq!(s.text, "Opus 4.7");
        let s = model_segment(&json!({ "model": { "id": "claude-sonnet-4-6" } }));
        assert_eq!(s.text, "sonnet-4-6");
        // Fallback when both are absent
        let s = model_segment(&json!({}));
        assert_eq!(s.text, "Claude");
    }

    #[test]
    fn version_segment_prepends_v() {
        let s = version_segment(&json!({ "version": "2.1.146" })).unwrap();
        assert_eq!(s.text, "v2.1.146");
        assert!(version_segment(&json!({})).is_none());
    }
}
