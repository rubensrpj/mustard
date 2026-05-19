//! `mustard-rt run statusline` — a port of `scripts/statusline.js`.
//!
//! Renders the Claude Code status bar. Unlike every other `run` subcommand
//! this one *does* read JSON from stdin — the harness pipes the statusline
//! payload (`workspace`, `model`, `cost`, `context_window`, …) on stdin and
//! expects up to three lines on stdout. A parse failure prints `Claude`
//! (the JS fallback) and exits clean.
//!
//! Port note: the JS git/RTK temp-file caches are dropped — they shaved a
//! `git`/`rtk` spawn off repeated renders, but a native binary spawns fast
//! enough that the extra filesystem coupling is not worth carrying. Git status
//! and RTK gain are queried directly each render (still fail-open).

use crate::run::rtk_gain::get_rtk_gain;
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// ANSI escape colours — the JS `C` table.
mod c {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";
    pub const GRAY: &str = "\x1b[90m";
    pub const BRIGHT_RED: &str = "\x1b[91m";
}

/// Terminal pipeline statuses (JS `TERMINAL_STATUSES`).
const TERMINAL: &[&str] = &["implemented", "completed", "validated", "cancelled"];

/// Run a `git` subcommand in `cwd`, returning trimmed stdout or `None`.
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
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Build the git segment — branch + staged/modified/untracked counts.
fn git_segment(cwd: &Path) -> Option<String> {
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let porcelain = git(cwd, &["status", "--porcelain"]).unwrap_or_default();
    let (mut staged, mut modified, mut untracked) = (0, 0, 0);
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
        status.push_str(&format!("{}+{staged}", c::GREEN));
    }
    if modified > 0 {
        status.push_str(&format!("{}~{modified}", c::YELLOW));
    }
    if untracked > 0 {
        status.push_str(&format!("{}?{untracked}", c::RED));
    }
    let status_str = if status.is_empty() {
        format!(" {}\u{2713}{}", c::GREEN, c::RESET)
    } else {
        format!(" {status}{}", c::RESET)
    };
    Some(format!("{}\u{2387} {branch}{}{status_str}", c::CYAN, c::RESET))
}

/// Build the context-window segment — a 10-cell bar plus a token count.
fn context_segment(data: &Value) -> Option<String> {
    let ctx = data.get("context_window")?;
    let rem = ctx.get("remaining_percentage")?.as_f64()?;
    let pct = rem.round() as i64;
    let exceeds = data.get("exceeds_200k_tokens") == Some(&Value::Bool(true));
    let color = if exceeds || pct < 20 {
        c::BRIGHT_RED
    } else if pct < 40 {
        c::RED
    } else if pct < 60 {
        c::YELLOW
    } else {
        c::GREEN
    };
    let bar_len = 10i64;
    let used = (((100 - pct) as f64 / 100.0) * bar_len as f64).round() as i64;
    let used = used.clamp(0, bar_len);
    let bar = format!(
        "{color}{}{}{}{}",
        "\u{2588}".repeat(used as usize),
        c::DIM,
        "\u{2591}".repeat((bar_len - used) as usize),
        c::RESET
    );
    let in_tok = ctx.get("total_input_tokens").and_then(Value::as_i64).unwrap_or(0);
    let out_tok = ctx.get("total_output_tokens").and_then(Value::as_i64).unwrap_or(0);
    let total_k = (in_tok + out_tok) / 1000;
    let mut s = format!("{bar} {color}{pct}%{} {}{total_k}k{}", c::RESET, c::GRAY, c::RESET);
    if exceeds {
        s.push_str(&format!(" {}{}\u{26A0} >200k{}", c::BRIGHT_RED, c::BOLD, c::RESET));
    }
    Some(s)
}

/// Build the duration segment.
fn duration_segment(data: &Value) -> Option<String> {
    let dur_ms = data.get("cost")?.get("total_duration_ms")?.as_i64()?;
    if dur_ms <= 0 {
        return None;
    }
    let m = dur_ms / 60_000;
    let s = (dur_ms % 60_000) / 1000;
    let t = if m > 0 {
        if s > 0 {
            format!("{m}m{s}s")
        } else {
            format!("{m}m")
        }
    } else {
        format!("{s}s")
    };
    Some(format!("{}{t}{}", c::GRAY, c::RESET))
}

/// Build the RTK token-economy segment.
fn rtk_segment() -> Option<String> {
    let gain = get_rtk_gain()?;
    if gain.saved <= 0 && gain.pct <= 0.0 {
        return None;
    }
    let saved_k = (gain.saved as f64 / 1000.0).round() as i64;
    let pct = gain.pct.round() as i64;
    let color = if pct > 50 {
        c::GREEN
    } else if pct > 20 {
        c::YELLOW
    } else {
        c::GRAY
    };
    Some(format!("{color}\u{26A1} {pct}%{} {}{saved_k}k saved{}", c::RESET, c::GRAY, c::RESET))
}

/// Whether any non-terminal pipeline-state file exists under `dir`.
fn pipeline_segment(dir: &Path) -> Option<String> {
    let states_dir = dir.join(".claude").join(".pipeline-states");
    let mut pipelines: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&states_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                if let Ok(state) = serde_json::from_str::<Value>(&text) {
                    let status = state.get("status").and_then(Value::as_str).unwrap_or("");
                    if !TERMINAL.contains(&status) {
                        pipelines.push(state);
                    }
                }
            }
        }
    }
    let most_recent = pipelines.into_iter().max_by(|a, b| {
        let ta = a.get("updatedAt").and_then(Value::as_str).unwrap_or("");
        let tb = b.get("updatedAt").and_then(Value::as_str).unwrap_or("");
        ta.cmp(tb)
    })?;
    let spec = most_recent
        .get("spec")
        .or_else(|| most_recent.get("feature"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    let phase = most_recent
        .get("phase")
        .map(|p| p.to_string())
        .unwrap_or_else(|| "?".to_string());
    let phase_name = most_recent
        .get("phaseName")
        .and_then(Value::as_str)
        .unwrap_or("");
    Some(format!(
        "{}{spec}{} {}P{phase} {phase_name}{}",
        c::CYAN, c::RESET, c::YELLOW, c::RESET
    ))
}

/// Render the statusline from a parsed payload.
fn render(data: &Value) -> Vec<String> {
    let sep = format!(" {}\u{2502}{} ", c::DIM, c::RESET);
    let cwd = data
        .get("workspace")
        .and_then(|w| w.get("current_dir"))
        .or_else(|| data.get("cwd"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let project_dir = data
        .get("workspace")
        .and_then(|w| w.get("project_dir"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.clone());

    let mut line1: Vec<String> = Vec::new();
    // Module name.
    let module = cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "?".to_string());
    line1.push(format!("{}{}{module}{}", c::BOLD, c::WHITE, c::RESET));
    if let Some(g) = git_segment(&cwd) {
        line1.push(g);
    }
    if let Some(ctx) = context_segment(data) {
        line1.push(ctx);
    }
    if let Some(d) = duration_segment(data) {
        line1.push(d);
    }
    if let Some(r) = rtk_segment() {
        line1.push(r);
    }
    // Lines +/-.
    let la = data.get("cost").and_then(|c| c.get("total_lines_added")).and_then(Value::as_i64).unwrap_or(0);
    let lr = data.get("cost").and_then(|c| c.get("total_lines_removed")).and_then(Value::as_i64).unwrap_or(0);
    if la > 0 || lr > 0 {
        let mut parts = String::new();
        if la > 0 {
            parts.push_str(&format!("{}+{la}{}", c::GREEN, c::RESET));
        }
        if lr > 0 {
            parts.push_str(&format!("{}-{lr}{}", c::RED, c::RESET));
        }
        line1.push(parts);
    }
    // Model.
    let raw_model = data
        .get("model")
        .and_then(|m| m.get("display_name").or_else(|| m.get("id")))
        .and_then(Value::as_str)
        .unwrap_or("Claude");
    let model_short = raw_model
        .strip_prefix("Claude ")
        .or_else(|| raw_model.strip_prefix("claude-"))
        .unwrap_or(raw_model);
    line1.push(format!("{}{model_short}{}", c::BLUE, c::RESET));
    if let Some(v) = data.get("version").and_then(Value::as_str) {
        line1.push(format!("{}v{v}{}", c::DIM, c::RESET));
    }

    let mut out = vec![line1.join(&sep)];
    if let Some(p) = pipeline_segment(&project_dir) {
        out.push(p);
    }
    out
}

/// Dispatch `mustard-rt run statusline`. Reads the payload from stdin.
pub fn run() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn context_segment_renders_bar() {
        let data = json!({
            "context_window": {
                "remaining_percentage": 70,
                "total_input_tokens": 50000,
                "total_output_tokens": 10000,
            }
        });
        let seg = context_segment(&data).unwrap();
        assert!(seg.contains("70%"));
        assert!(seg.contains("60k"));
    }

    #[test]
    fn duration_segment_formats_minutes() {
        let data = json!({ "cost": { "total_duration_ms": 125_000 } });
        let seg = duration_segment(&data).unwrap();
        assert!(seg.contains("2m5s"));
    }

    #[test]
    fn render_falls_back_to_module_only() {
        let lines = render(&json!({ "model": { "id": "claude-opus" } }));
        assert!(!lines.is_empty());
    }

    #[test]
    fn duration_segment_none_when_zero() {
        assert!(duration_segment(&json!({ "cost": { "total_duration_ms": 0 } })).is_none());
    }
}
