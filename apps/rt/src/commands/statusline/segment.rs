//! Statusline segments — pure data ([`Segment`]) plus the per-kind builders
//! that turn the harness JSON payload into segments. Themes (in `theme.rs`)
//! own all color/separator decisions; this module only produces *text*.
//!
//! The one exception is [`Segment::override_fg`], used by [`cost_segment`]
//! when the per-segment threshold (green / yellow / red) needs to override
//! the theme default. Theme renderers honor it.

use super::theme::Color;
use crate::commands::economy::rtk_gain::get_rtk_gain;
use crate::shared::branch_state::{awaiting_prune, LocalOnlyPr};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

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
    Mustard = 9,
    Prune = 10,
    /// The work unit this session is inside, and its stage.
    Unit = 11,
    /// The plugin is installed but switched off, so no hook runs.
    Inert = 12,
}

/// Count of kinds — keep in sync with the last variant.
pub const SEGMENT_KIND_COUNT: usize = 13;

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

/// Mustard harness segment. Aligned project → `m{version}` (the running
/// harness). Drifted stamp (older install, or unstamped) → `m{stamped}⇡{current}`
/// in yellow — the visual "update available" hint; `/mustard:upsert` realigns.
/// `None` when the project carries no `mustard.json` (Mustard not installed —
/// the line stays quiet).
#[must_use]
pub fn mustard_segment(cwd: &Path) -> Option<Segment> {
    if !mustard_core::ProjectConfig::exists(cwd) {
        return None;
    }
    let stamped = mustard_core::ProjectConfig::load(cwd).version;
    let current = mustard_core::harness_version();
    Some(match stamped {
        Some(s) if s == current => Segment::new(SegmentKind::Mustard, format!("m{current}")),
        other => {
            let from = other.unwrap_or_else(|| "?".to_string());
            let mut seg = Segment::new(SegmentKind::Mustard, format!("m{from}\u{2191}{current}"));
            seg.override_fg = Some(Color::Ansi(3));
            seg
        }
    })
}

/// `▸ {slug} {STAGE}` — the work unit this session is inside, and where it is.
///
/// The operator who reopens a terminal should not have to type a command to
/// learn where they stopped. Before this segment the bar named the harness
/// version and nothing else, so a unit parked in PLAN was invisible until
/// `/mustard:spec` was run.
///
/// Reads the per-session active-spec marker ([`current_spec`]) and that spec's
/// `meta.json`, both of which the pipeline already maintains — a status bar
/// redrawn every turn must not enumerate the spec tree. `None` when the project
/// is not a Mustard install or no unit is active.
#[must_use]
pub fn unit_segment(cwd: &Path) -> Option<Segment> {
    if !mustard_core::ProjectConfig::exists(cwd) {
        return None;
    }
    let slug = crate::shared::context::current_spec(&cwd.to_string_lossy())
        .filter(|s| !s.is_empty())?;
    // The stage is a convenience, not the point: an unreadable or half-written
    // `meta.json` still leaves the unit NAMED, which is the whole job here.
    let stage = std::fs::read_to_string(cwd.join(".claude/spec").join(&slug).join("meta.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|m| m.get("phase").and_then(Value::as_str).map(str::to_string));
    let text = match stage {
        Some(phase) => format!("\u{25b8} {slug} {phase}"),
        None => format!("\u{25b8} {slug}"),
    };
    Some(Segment::new(SegmentKind::Unit, text))
}

/// Is the Mustard plugin listed in `settings` and switched OFF?
///
/// `Some(true)` disabled, `Some(false)` enabled, `None` when the question
/// cannot be answered — no file, unparseable, or the plugin unlisted (a source
/// checkout with no plugin install is not a defect). The marketplace suffix
/// varies by install (`mustard@mustard-local`, `mustard@mustard`, …), so the
/// name before the `@` is what decides.
///
/// Shared with the doctor's `inject-delivery` check, which reports the same
/// state as a FAIL: one reader, so the bar and the diagnosis cannot disagree.
#[must_use]
pub fn plugin_switched_off(settings: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(settings).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    json.get("enabledPlugins")?
        .as_object()?
        .iter()
        .find(|(key, _)| key.split('@').next() == Some("mustard"))
        .map(|(_, value)| value.as_bool() == Some(false))
}

/// `⨯ harness inerte` — the plugin is installed and switched OFF.
///
/// With the plugin disabled no hook runs at all: no router, no gates. Measured
/// in the field 2026-08-25, that state is indistinguishable from a working
/// harness — the bar rendered normally while nothing was enforced, and three
/// attempts were spent discovering it by error. Red, because it is not
/// something owed; it is the harness not running.
///
/// `None` when the switch cannot be read (no settings file, unreadable, the
/// plugin unlisted) or when it is enabled. Never claims health it did not
/// measure: an unanswerable question renders nothing.
#[must_use]
pub fn inert_segment(cwd: &Path) -> Option<Segment> {
    if !mustard_core::ProjectConfig::exists(cwd) {
        return None;
    }
    // Through `claude_config_dir()`, which honours `CLAUDE_CONFIG_DIR`. A
    // hardcoded `$HOME/.claude` left this flag silent for an operator who moved
    // their config: the bar looked healthy while no hook ran, which is the very
    // state it exists to show (found in review).
    let settings = mustard_core::platform::harness::claude_config_dir()?.join("settings.json");
    if plugin_switched_off(&settings) != Some(true) {
        return None;
    }
    let lang = mustard_core::ProjectConfig::load(cwd).i18n().lang;
    let label = mustard_core::translate("statusline.harness.inert", lang);
    let mut seg = Segment::new(SegmentKind::Inert, format!("\u{2a2f} {label}"));
    seg.override_fg = Some(Color::Ansi(1));
    Some(seg)
}

// ---------------------------------------------------------------------------
// Pending-prune segment
// ---------------------------------------------------------------------------

/// Where the measured count is memoised, under the project's harness dir.
const PRUNE_CACHE_FILE: &str = ".prune-count";

/// How long a measured count stays fresh.
///
/// The bar is redrawn on every turn and the measurement costs a handful of git
/// invocations (one ref sweep plus one ancestry read per base), so the answer
/// is memoised for a short window: long enough that a burst of turns measures
/// once, short enough that a unit the user just pruned leaves the bar within a
/// turn or two. Mirrors the on-disk, mtime-driven window the Stop observer's
/// anti-spam marker already uses — an in-process memo would be worthless here,
/// since each render is its own process.
const PRUNE_CACHE_SECS: u64 = 30;

/// `✂ N a podar` — how many delivered work units still have a live branch.
///
/// `None` when the project is not a Mustard install (the bar stays quiet, like
/// [`mustard_segment`]) or when nothing is owed. The count comes from the ONE
/// classifier ([`awaiting_prune`]) with the lookup that asks no provider
/// ([`LocalOnlyPr`]): a status bar must not open a network connection per
/// branch, so it counts only merges LOCAL ancestry proves. It can therefore
/// under-report and never over-report — `mustard-rt run git-settle --report` is
/// the face that also asks the provider.
#[must_use]
pub fn prune_segment(cwd: &Path) -> Option<Segment> {
    if !mustard_core::ProjectConfig::exists(cwd) {
        return None;
    }
    let count = pending_prune_count(cwd);
    if count == 0 {
        return None;
    }
    let lang = mustard_core::ProjectConfig::load(cwd).i18n().lang;
    let label = mustard_core::translate("statusline.prune.label", lang);
    let mut seg = Segment::new(SegmentKind::Prune, format!("\u{2702} {count} {label}"));
    // Yellow: something is owed, nothing is wrong.
    seg.override_fg = Some(Color::Ansi(3));
    Some(seg)
}

/// The count, served from the short-lived cache when it is still fresh and
/// re-measured otherwise. Fail-open at every step: an unreadable cache
/// re-measures, an unwritable one simply measures again next render.
fn pending_prune_count(cwd: &Path) -> usize {
    let cache = ClaudePaths::for_project(cwd)
        .ok()
        .map(|paths| paths.harness_dir().join(PRUNE_CACHE_FILE));
    if let Some(path) = cache.as_deref() {
        if let Some(fresh) = cached_count(path) {
            return fresh;
        }
    }
    let measured = measure_pending_prune(cwd);
    if let Some(path) = cache.as_deref() {
        store_count(path, measured);
    }
    measured
}

/// The cached count when the file was written inside [`PRUNE_CACHE_SECS`];
/// `None` when it is absent, stale, unreadable, or written in the future
/// (clock skew re-measures rather than trusting an impossible mtime).
fn cached_count(path: &Path) -> Option<usize> {
    let written = fs::modified(path).ok()?;
    let age = SystemTime::now().duration_since(written).ok()?;
    if age > Duration::from_secs(PRUNE_CACHE_SECS) {
        return None;
    }
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Persist the count for the next renders (best-effort).
fn store_count(path: &Path, count: usize) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write_atomic(path, count.to_string().as_bytes());
}

/// Measure the count from git — the uncached path.
fn measure_pending_prune(cwd: &Path) -> usize {
    let config = mustard_core::ProjectConfig::load(cwd);
    let flow = crate::shared::work_kind::BaseFlow::of(&config.git);
    let git_read = |args: &[&str]| git(cwd, args);
    awaiting_prune(&git_read, &LocalOnlyPr, &flow).len()
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

    /// AC-7 — the bar names the active unit and its stage, and says so when the
    /// harness is inert.
    ///
    /// Both halves answer the same question: what does the operator see without
    /// typing anything? A unit parked in PLAN was invisible, and a switched-off
    /// plugin rendered exactly like a working one.
    #[test]
    fn statusline_names_the_active_unit_and_flags_an_inert_harness() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // `current_spec` memoises per PROCESS — right for a hook, which is one
        // invocation, but it means each state below needs its own root: asking
        // about a root before seeding it would cache the empty answer.
        let bare = tempfile::tempdir().unwrap();
        assert!(unit_segment(bare.path()).is_none(), "not a Mustard project: quiet");
        assert!(inert_segment(bare.path()).is_none());

        let idle = tempfile::tempdir().unwrap();
        std::fs::write(idle.path().join("mustard.json"), r#"{"version":"1.0.0"}"#).unwrap();
        assert!(unit_segment(idle.path()).is_none(), "no active unit must render nothing");

        std::fs::write(root.join("mustard.json"), r#"{"version":"1.0.0","specLang":"pt-BR"}"#).unwrap();
        // A unit in PLAN: named, with its stage. The active unit is the newest
        // pipeline-state file — the same source every other consumer reads.
        let spec = root.join(".claude/spec/roteador-didatico");
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("meta.json"), r#"{"stage":"Plan","phase":"PLAN"}"#).unwrap();
        let states = root.join(".claude/.pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join("roteador-didatico.json"), "{}").unwrap();

        let seg = unit_segment(root).expect("an active unit must reach the bar");
        assert!(seg.text.contains("roteador-didatico"), "the unit is unnamed: {}", seg.text);
        assert!(seg.text.contains("PLAN"), "the stage is missing: {}", seg.text);

        // An unreadable meta still leaves the unit NAMED — that is the job.
        std::fs::write(spec.join("meta.json"), "{ not json").unwrap();
        let seg = unit_segment(root).expect("a broken meta must not hide the unit");
        assert!(seg.text.contains("roteador-didatico"));
    }

    /// The inert flag reads the plugin switch, and never claims health it could
    /// not measure: an absent or unlisted switch answers "cannot tell".
    ///
    /// Exercises the decision directly rather than through `$HOME`: a test that
    /// mutates process-wide environment races every other test in the binary,
    /// and the thing worth pinning here is the verdict, not the path lookup.
    #[test]
    fn the_inert_flag_reads_the_plugin_switch_and_stays_silent_when_unanswerable() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let verdict = |body: Option<&str>| {
            match body {
                Some(b) => std::fs::write(&settings, b).unwrap(),
                None => {
                    let _ = std::fs::remove_file(&settings);
                }
            }
            plugin_switched_off(&settings)
        };

        assert_eq!(verdict(None), None, "no settings file: unanswerable, not green");
        assert_eq!(verdict(Some("{ not json")), None, "unparseable: unanswerable");
        assert_eq!(verdict(Some(r#"{"enabledPlugins":{}}"#)), None, "unlisted: unanswerable");
        assert_eq!(
            verdict(Some(r#"{"enabledPlugins":{"mustard@mustard-local":true}}"#)),
            Some(false),
            "an enabled plugin is measured as running",
        );
        // The marketplace suffix varies by install, so the name before `@` decides.
        assert_eq!(
            verdict(Some(r#"{"enabledPlugins":{"other@x":true,"mustard@whatever":false}}"#)),
            Some(true),
            "a disabled plugin is measured whatever marketplace it came from",
        );
    }

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

    /// A project that never installed Mustard is not nagged — the bar stays
    /// exactly as long as it was, and no git sweep is even attempted.
    #[test]
    fn prune_segment_silent_without_a_mustard_project() {
        let td = tempfile::tempdir().expect("tempdir");
        assert!(prune_segment(td.path()).is_none());
    }

    /// The short window: a just-written count is served, a backdated one is
    /// refused so the next render measures again. Without the refusal the bar
    /// would keep advertising units the user already pruned.
    #[test]
    fn prune_count_cache_serves_fresh_and_refuses_stale() {
        let td = tempfile::tempdir().expect("tempdir");
        let path = td.path().join(".prune-count");
        store_count(&path, 3);
        assert_eq!(cached_count(&path), Some(3), "a just-written count is fresh");

        let stale = SystemTime::now() - Duration::from_secs(PRUNE_CACHE_SECS + 5);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(stale)
            .expect("backdate");
        assert_eq!(cached_count(&path), None, "past the window the cache stops answering");
        assert_eq!(cached_count(&td.path().join("never-written")), None, "a miss is a miss");
    }

    /// **AC-8.** With units owed, the bar SAYS SO: the count, in the language
    /// the project configured, derived from the project's OWN bases.
    ///
    /// The agnosticism half is two-sided against the production region only
    /// (this test's own fixture necessarily spells a base): the code reads the
    /// project's own bases, and carries no base spelling of its own. The first
    /// assertion alone would pass in a file that also hardcoded one.
    #[test]
    fn statusline_names_units_awaiting_prune() {
        let td = tempfile::tempdir().expect("tempdir");
        let root = td.path();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("git");
        };
        run(&["init", "."]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        run(&["config", "commit.gpgsign", "false"]);
        run(&["checkout", "-b", "dev"]);
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("config");
        run(&["add", "-A"]);
        run(&["commit", "-m", "seed"]);

        // Nothing delivered yet — the bar stays exactly as long as it was.
        assert!(prune_segment(root).is_none(), "nothing owed, nothing said");
        // That render MEASURED zero and cached it. Inside the window the next
        // render is served from the cache by design, so the count below has to
        // be a fresh measurement to mean anything — drop the memo, exactly as
        // the window expiring would.
        if let Ok(paths) = ClaudePaths::for_project(root) {
            let _ = std::fs::remove_file(paths.harness_dir().join(PRUNE_CACHE_FILE));
        }

        // A delivered unit: merged into its base, branch alive on both sides.
        run(&["checkout", "-b", "dev_delivered"]);
        std::fs::write(root.join("work.txt"), "w").expect("file");
        run(&["add", "-A"]);
        run(&["commit", "-m", "work"]);
        run(&["checkout", "dev"]);
        run(&["merge", "--no-ff", "dev_delivered", "-m", "merge dev_delivered"]);
        run(&["update-ref", "refs/remotes/origin/dev_delivered", "refs/heads/dev_delivered"]);

        let seg =
            prune_segment(root).expect("a delivered unit whose branch survives must be announced");
        let lang = mustard_core::ProjectConfig::load(root).i18n().lang;
        let label = mustard_core::translate("statusline.prune.label", lang);
        assert_eq!(seg.kind, SegmentKind::Prune);
        assert!(seg.text.contains('1'), "the bar states the count: {}", seg.text);
        assert!(
            seg.text.contains(label),
            "…worded from the catalogue in the configured language: {}",
            seg.text
        );

        // Agnosticism, over the production region only.
        let src = include_str!("segment.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or_default();
        assert!(!production.is_empty(), "the production region must still be readable here");
        assert!(
            production.contains("BaseFlow::of"),
            "the bases must come from the project's own config — `BaseFlow` derives \
             every one of them from `git.flow` and spells none",
        );
        for spelling in
            [["\"de", "v\""].concat(), ["\"mai", "n\""].concat(), ["\"mast", "er\""].concat()]
        {
            assert!(!production.contains(&spelling), "a base name is spelled in the code: {spelling}");
        }
    }

    #[test]
    fn version_segment_prepends_v() {
        let s = version_segment(&json!({ "version": "2.1.146" })).unwrap();
        assert_eq!(s.text, "v2.1.146");
        assert!(version_segment(&json!({})).is_none());
    }

}
