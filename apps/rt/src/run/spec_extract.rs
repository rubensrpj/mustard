//! `mustard-rt run spec-extract` — a port of `scripts/spec-extract.js`.
//!
//! Cuts a single wave slice (or AC block) from a `spec.md`. Wave N+1 only needs
//! its own section + the previous wave's diff; re-sending the full spec drives
//! prompt bloat.
//!
//! Two spec layouts: `monolithic` (one `spec.md` with `### {Role} Agent
//! (Wave N)` sub-headers — the slice is that section) and `wave-plan` (a
//! `{specName}/wave-N-{role}/spec.md` per wave — each file IS already the
//! slice, handed over whole, never truncated).
//!
//! `--measure` prints a JSON line describing the counterfactual omission.

use crate::run::current_spec;
use crate::run::spec_sections::is_heading;
use crate::util::now_iso8601;
use mustard_core::economy::writer;
use mustard_core::economy::{AgentId, ContextCostFrame, ProjectPath, SpecId, WaveId};
use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::Connection;
use serde_json::{Map, json};
use std::path::{Path, PathBuf};

/// Resolve the harness SQLite path (mirrors `SqliteEventStore::for_project`'s
/// private resolver). Env override `MUSTARD_DB_PATH` wins, else the standard
/// `.claude/.harness/mustard.db` under the project root.
fn economy_db_path(project_dir: &str) -> PathBuf {
    if let Ok(value) = std::env::var("MUSTARD_DB_PATH") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    Path::new(project_dir)
        .join(".claude")
        .join(".harness")
        .join("mustard.db")
}

/// Open a raw [`Connection`] to the harness DB, applying schema/migrations
/// via [`SqliteEventStore::for_project`] first. Returns `None` on any
/// failure — `spec-extract` must remain fail-open even if the store cannot
/// be reached.
fn open_economy_conn(project_dir: &str) -> Option<Connection> {
    let _ = SqliteEventStore::for_project(project_dir).ok()?;
    Connection::open(economy_db_path(project_dir)).ok()
}

/// Record one [`ContextCostFrame`] for the wave-slice the caller just cut.
/// `full_bytes` is the unsliced spec body; `slice_bytes` is what survived;
/// the omission is implied (the dashboard subtracts). Fail-open: every
/// error path is silenced — the JSON `measure` line is still printed and is
/// the contract a script consumer reads.
fn record_extract_frame(
    project_dir: &str,
    full_bytes: usize,
    slice_bytes: usize,
    spec_slug: Option<&str>,
    wave_id: Option<&str>,
) {
    let Some(conn) = open_economy_conn(project_dir) else {
        return;
    };
    let agent = std::env::var("MUSTARD_AGENT_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "spec-extract".to_string());
    let frame = ContextCostFrame {
        ts: now_iso8601(),
        agent_id: AgentId::new(agent),
        wave_id: wave_id.map(WaveId::new),
        spec_id: spec_slug.map(SpecId::new),
        project_path: ProjectPath::new(project_dir),
        prompt_size_bytes: i64::try_from(full_bytes).ok(),
        prefix_stable_bytes: None,
        slice_bytes: i64::try_from(slice_bytes).ok(),
        recipe_bytes: None,
        wave_slice_bytes: i64::try_from(slice_bytes).ok(),
        return_size_bytes: Some(0),
        retry_overhead_bytes: Some(0),
        extra: Map::new(),
    };
    let _ = writer::record_context_cost(&conn, frame);
}

/// Monolithic wave section cap (one section).
const MAX_CHARS: usize = 4000;
/// Advisory soft limit — a per-wave sub-spec is NEVER truncated.
const WAVE_PLAN_SOFT_LIMIT: usize = 50_000;
const TRUNCATE_TAIL: &str = "\n...[truncated]";

/// Detected spec layout.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Monolithic,
    WavePlan,
}

/// Detect spec layout from the path: `.../{specName}/wave-{N}-{role}/spec.md`
/// is `wave-plan`, anything else is `monolithic`.
fn detect_mode(spec_path: &str) -> Mode {
    let norm = spec_path.replace('\\', "/").to_lowercase();
    // `/wave-(\d+)-[^/]+/spec.md$`
    // `/wave-(\d+)-[^/]+/spec.md$`
    if let Some(idx) = norm.rfind("/wave-") {
        let tail = &norm[idx + 6..];
        if let Some(dash) = tail.find('-') {
            let num = &tail[..dash];
            let after = &tail[dash + 1..];
            if !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit())
                && after.ends_with("/spec.md")
            {
                // The `[^/]+` role segment must have no `/` before `/spec.md`.
                let role = &after[..after.len() - "/spec.md".len()];
                if !role.is_empty() && !role.contains('/') {
                    return Mode::WavePlan;
                }
            }
        }
    }
    Mode::Monolithic
}

/// Read a spec file, returning `None` on any error.
fn read_spec(spec_path: &str) -> Option<String> {
    std::fs::read_to_string(spec_path).ok()
}

/// Trim trailing whitespace (`\s+$`).
fn rtrim(s: &str) -> &str {
    s.trim_end()
}

/// Slice from the first line matching `is_h3_match` until the next `### `
/// heading (exclusive).
fn slice_wave_section(text: &str, n: u32) -> Option<String> {
    // Match `^###\s+[^\n]*\(Wave N\)[^\n]*$` case-insensitively.
    let target = format!("(wave {n})");
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines.iter().position(|l| {
        let lower = l.to_lowercase();
        lower.starts_with("###") && {
            let after = lower.trim_start_matches('#');
            after.starts_with([' ', '\t']) && lower.contains(&target)
        }
    })?;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("### ") {
            end = i;
            break;
        }
    }
    Some(rtrim(&lines[start..end].join("\n")).to_string())
}

/// Extract a wave section.
fn extract_wave(spec_path: &str, n: Option<u32>) -> Option<String> {
    let text = read_spec(spec_path)?;
    if detect_mode(spec_path) == Mode::WavePlan {
        return Some(rtrim(&text).to_string());
    }
    let num = n?;
    if num < 1 {
        return None;
    }
    slice_wave_section(&text, num)
}

/// Extract the `## Acceptance Criteria` section until the next `## ` heading.
fn extract_acceptance_criteria(spec_path: &str) -> Option<String> {
    let text = read_spec(spec_path)?;
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines.iter().position(|l| is_heading(l, "acceptanceCriteria"))?;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("## ") {
            end = i;
            break;
        }
    }
    Some(rtrim(&lines[start..end].join("\n")).to_string())
}

/// Measure the counterfactual omission for a wave dispatch.
fn measure(spec_path: &str, n: Option<u32>) -> Option<serde_json::Value> {
    let text = read_spec(spec_path)?;
    let full_bytes = text.len();
    let mode = detect_mode(spec_path);

    if mode == Mode::WavePlan {
        let slice = extract_wave(spec_path, n).unwrap_or_default();
        let slice_bytes = slice.len();
        let wave_dir = Path::new(spec_path).parent();
        let spec_root = wave_dir.and_then(Path::parent);
        let mut wave_plan_md = 0u64;
        let mut sibling_specs = 0u64;
        let mut sibling_count = 0u64;
        if let Some(root) = spec_root {
            let wave_plan = root.join("wave-plan.md");
            if let Ok(meta) = std::fs::metadata(&wave_plan) {
                wave_plan_md = meta.len();
            }
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if !name.starts_with("wave-") {
                        continue;
                    }
                    // `^wave-\d+-`
                    let after = &name[5..];
                    let digits_end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
                    if digits_end == 0 || !after[digits_end..].starts_with('-') {
                        continue;
                    }
                    let sib_dir = entry.path();
                    if Some(sib_dir.as_path()) == wave_dir {
                        continue;
                    }
                    let sib_spec = sib_dir.join("spec.md");
                    if let Ok(meta) = std::fs::metadata(&sib_spec) {
                        sibling_specs += meta.len();
                        sibling_count += 1;
                    }
                }
            }
        }
        let omitted = wave_plan_md + sibling_specs;
        return Some(json!({
            "mode": "wave-plan",
            "full_bytes": full_bytes,
            "slice_bytes": slice_bytes,
            "omitted_bytes": omitted,
            "omitted_detail": {
                "wave_plan_md": wave_plan_md,
                "sibling_specs": sibling_specs,
                "sibling_count": sibling_count,
            },
        }));
    }

    // monolithic
    let slice = extract_wave(spec_path, n).unwrap_or_default();
    let slice_bytes = slice.len();
    let omitted = full_bytes.saturating_sub(slice_bytes);
    Some(json!({
        "mode": "monolithic",
        "full_bytes": full_bytes,
        "slice_bytes": slice_bytes,
        "omitted_bytes": omitted,
        "omitted_detail": { "rest_of_spec": omitted },
    }))
}

/// Cap a string to `limit` chars, appending a truncation tail.
fn cap(s: &str, limit: usize) -> String {
    if s.chars().count() <= limit {
        return s.to_string();
    }
    let keep = limit.saturating_sub(TRUNCATE_TAIL.chars().count());
    let head: String = s.chars().take(keep).collect();
    format!("{head}{TRUNCATE_TAIL}")
}

/// Dispatch `mustard-rt run spec-extract`.
pub fn run(spec: &str, wave: Option<u32>, ac: bool, measure_flag: bool) {
    if !Path::new(spec).exists() {
        eprintln!("[spec-extract] spec not found: {spec}");
        std::process::exit(1);
    }

    if measure_flag {
        match measure(spec, wave) {
            Some(m) => {
                // W2: record the wave-slice into `context_cost_frames` using
                // the same bytes the JSON measure line already reports — that
                // way dashboard queries align with the script's stdout.
                let project_dir = crate::run::env::project_dir();
                let full_bytes =
                    m.get("full_bytes").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let slice_bytes =
                    m.get("slice_bytes").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let spec_slug = current_spec(&project_dir);
                let wave_id_str = wave.map(|n| format!("wave-{n}"));
                record_extract_frame(
                    &project_dir,
                    full_bytes,
                    slice_bytes,
                    spec_slug.as_deref(),
                    wave_id_str.as_deref(),
                );
                println!("{m}");
            }
            None => {
                eprintln!("[spec-extract] could not measure spec");
                std::process::exit(1);
            }
        }
        return;
    }

    let mode = detect_mode(spec);
    let out: Option<String> = if ac {
        let v = extract_acceptance_criteria(spec);
        if v.is_none() {
            eprintln!("[spec-extract] ## Acceptance Criteria section not found");
            std::process::exit(1);
        }
        v
    } else if wave.is_some() || mode == Mode::WavePlan {
        let v = extract_wave(spec, wave);
        if v.is_none() {
            eprintln!(
                "[spec-extract] Wave {} section not found",
                wave.map(|w| w.to_string()).unwrap_or_default()
            );
            std::process::exit(1);
        }
        v
    } else {
        eprintln!("[spec-extract] provide --wave <N> or --ac");
        return;
    };

    let Some(out) = out else { return };
    // W2: record the wave-slice into `context_cost_frames` whenever we cut a
    // wave (the AC branch does not measure a wave so it is skipped here).
    if !ac {
        let project_dir = crate::run::env::project_dir();
        let full_bytes = std::fs::metadata(spec).map(|m| m.len() as usize).unwrap_or(out.len());
        let spec_slug = current_spec(&project_dir);
        let wave_id_str = wave.map(|n| format!("wave-{n}"));
        record_extract_frame(
            &project_dir,
            full_bytes,
            out.len(),
            spec_slug.as_deref(),
            wave_id_str.as_deref(),
        );
    }
    if mode == Mode::WavePlan {
        if out.len() > WAVE_PLAN_SOFT_LIMIT {
            eprintln!(
                "[spec-extract] WARN: wave spec is {} chars (soft limit {WAVE_PLAN_SOFT_LIMIT}) — consider splitting this wave. Emitting whole (not truncated).",
                out.len()
            );
        }
        println!("{out}");
    } else {
        println!("{}", cap(&out, MAX_CHARS));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn detect_mode_recognizes_wave_plan() {
        // Flat layout (wave-2 of 2026-05-21-flatten-spec-layout-and-multi-collab):
        // spec dirs sit directly under `.claude/spec/`, no active/completed
        // buckets. `detect_mode` keys off the `/wave-N-role/spec.md` suffix
        // and is bucket-agnostic by construction.
        assert_eq!(
            detect_mode("/x/spec/foo/wave-2-backend/spec.md"),
            Mode::WavePlan
        );
        assert_eq!(detect_mode("/x/spec/foo/spec.md"), Mode::Monolithic);
    }

    #[test]
    fn extract_wave_slices_monolithic_section() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            "# Spec\n\n### Backend Agent (Wave 1)\ntask one\n\n### Frontend Agent (Wave 2)\ntask two\n"
        )
        .unwrap();
        let slice = extract_wave(path.to_str().unwrap(), Some(1)).unwrap();
        assert!(slice.contains("Wave 1"));
        assert!(slice.contains("task one"));
        assert!(!slice.contains("task two"));
    }

    #[test]
    fn extract_ac_section() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# Spec\n## Files\n- a.ts\n## Acceptance Criteria\n- AC1 runs\n## Tasks\n- [ ] x\n",
        )
        .unwrap();
        let ac = extract_acceptance_criteria(path.to_str().unwrap()).unwrap();
        assert!(ac.contains("AC1 runs"));
        assert!(!ac.contains("Tasks"));
    }

    #[test]
    fn measure_monolithic_reports_omission() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, "# Spec\n### Backend Agent (Wave 1)\nbody\n").unwrap();
        let m = measure(path.to_str().unwrap(), Some(1)).unwrap();
        assert_eq!(m["mode"], json!("monolithic"));
        assert!(m["full_bytes"].as_u64().unwrap() > 0);
    }
}
