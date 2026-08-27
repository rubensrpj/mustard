//! Deterministic plan-staleness check for a spec (spec
//! `melhorias-pagina-specs` item 4). Answers, WITHOUT AI or network, whether a
//! spec's PLAN is still viable: do the files its `## Arquivos` / `## Files`
//! census names still exist, and have any of them changed on disk SINCE the
//! plan was drawn? A plan is `stale` STRICTLY when its code moved — a census
//! file went `missing` or `changed`. Age (`age_days`) is reported as evidence
//! but never, on its own, marks a plan stale.
//!
//! The plan date is the spec's `meta.json#checkpoint` (the last meaningful
//! pipeline event), falling back to the `started_at` the caller passes from the
//! spec card when the sidecar carries no checkpoint. For each census file we
//! compare its git last-commit date (`git log -1 --format=%cI -- <file>`)
//! against the plan date; when git is unavailable we fall back to the file's
//! filesystem mtime. A file that is gone is `missing`; a file committed/touched
//! after the plan is `changed`.
//!
//! FAIL-OPEN CONTRACT (mirrors every dashboard command): a missing spec, an
//! unreadable `spec.md`, or an absent `## Arquivos` section yields
//! `verdict: "unknown"` with a reason — never a panic, never an invented
//! `stale`. The command is registered in `lib.rs` and surfaced through the
//! `dashboardSpecPlanStaleness` binding; it runs ONLY on an explicit
//! "Reanalisar" click (on-demand, never on a query/poll).

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::process_util::no_window_command;

/// Result of [`dashboard_spec_plan_staleness`]. Mirrors the TS `Staleness`
/// shape (`serde(rename_all = "snake_case")`).
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Staleness {
    /// `"stale" | "fresh" | "unknown"`. `unknown` carries a `reason`.
    pub verdict: String,
    /// Whole days between the plan date and now; `0` when the plan date is
    /// unknown.
    pub age_days: i64,
    /// Census files that no longer exist in the repo.
    pub missing: Vec<String>,
    /// Census files modified (git-committed, else mtime) AFTER the plan date.
    pub changed: Vec<String>,
    /// Total files parsed from the census.
    pub total: usize,
    /// Human-readable note when `verdict == "unknown"` (e.g. "sem censo de
    /// arquivos"); empty otherwise.
    pub reason: String,
    /// The plan date the comparison used (ISO-8601), or empty when unknown.
    pub plan_date: String,
}

impl Staleness {
    fn unknown(reason: &str) -> Self {
        Self {
            verdict: "unknown".to_string(),
            reason: reason.to_string(),
            ..Self::default()
        }
    }
}

/// Deterministic plan-staleness evaluation for one spec. `started_at` is the
/// spec card's `started_at` (the caller passes it so we never re-fold the event
/// log here) — used as the plan-date fallback when `meta.json#checkpoint` is
/// absent.
///
/// Always returns `Ok`: a missing spec / census degrades to
/// `verdict: "unknown"`, never an `Err` toast.
pub fn dashboard_spec_plan_staleness(
    repo_path: String,
    spec: String,
    started_at: Option<String>,
) -> Result<Staleness, String> {
    // A panic in the blocking closure degrades to `unknown`, never an Err.
    let result = crate::catch_panic(move || {
        plan_staleness_impl(&repo_path, &spec, started_at.as_deref())
    })
    .unwrap_or_else(|_| Staleness::unknown("falha ao avaliar"));
    Ok(result)
}

/// Synchronous body of [`dashboard_spec_plan_staleness`].
fn plan_staleness_impl(repo_path: &str, spec: &str, started_at: Option<&str>) -> Staleness {
    // Reject obvious traversal — `spec` is a single slug.
    if spec.is_empty() || spec.contains('/') || spec.contains('\\') || spec.contains("..") {
        return Staleness::unknown("nome de spec inválido");
    }
    let base = PathBuf::from(repo_path);
    let spec_dir = base.join(".claude").join("spec").join(spec);
    let spec_md = spec_dir.join("spec.md");
    if !spec_md.is_file() {
        return Staleness::unknown("spec.md inexistente");
    }

    // Plan date: meta.json#checkpoint, else the card's started_at.
    let plan_date_str = read_checkpoint(&spec_md)
        .or_else(|| started_at.map(str::to_string))
        .filter(|s| !s.is_empty());
    let plan_epoch = plan_date_str.as_deref().and_then(parse_iso_to_epoch);

    // Collect the census file paths. For a wave plan we union the parent
    // census with each wave sub-spec's census (the wave specs carry the clean
    // backtick-wrapped paths; the parent census is a coordination document).
    let files = collect_census_files(&spec_dir);
    if files.is_empty() {
        return Staleness::unknown("sem censo de arquivos");
    }

    let mut missing: Vec<String> = Vec::new();
    let mut changed: Vec<String> = Vec::new();
    for rel in &files {
        let abs = base.join(rel);
        if !abs.exists() {
            missing.push(rel.clone());
            continue;
        }
        // Only flag "changed" when we have a plan date to compare against.
        let Some(plan_e) = plan_epoch else { continue };
        if let Some(mod_e) = file_modified_epoch(&base, rel, &abs) {
            if mod_e > plan_e {
                changed.push(rel.clone());
            }
        }
    }

    let now = system_time_epoch(SystemTime::now());
    let age_days = match plan_epoch {
        Some(plan_e) => ((now - plan_e) / 86_400).max(0),
        None => 0,
    };

    // Obsolescence means STRICTLY "the code under the plan changed" — a census
    // file went missing or was modified after the plan date. Age alone is
    // reported as evidence (`age_days`) but never marks a plan stale.
    let stale = !missing.is_empty() || !changed.is_empty();
    Staleness {
        verdict: if stale { "stale" } else { "fresh" }.to_string(),
        age_days,
        missing,
        changed,
        total: files.len(),
        reason: String::new(),
        plan_date: plan_date_str.unwrap_or_default(),
    }
}

/// Read `meta.json#checkpoint` (ISO-8601) beside `spec.md`, via the core
/// lenient reader. `None` when the sidecar is absent or carries no checkpoint.
fn read_checkpoint(spec_md: &Path) -> Option<String> {
    let meta = mustard_core::domain::meta::read_meta_beside(spec_md)?;
    meta.checkpoint.filter(|s| !s.is_empty())
}

/// Collect the deduped union of census file paths for a spec. Reads the spec's
/// own `## Arquivos` / `## Files`, plus — when the dir holds `wave-N-{role}/`
/// sub-specs — each wave sub-spec's census too. Order-stable (first-seen wins).
fn collect_census_files(spec_dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut push_from = |path: &Path| {
        if let Ok(text) = std::fs::read_to_string(path) {
            for f in parse_census_paths(&text) {
                if seen.insert(f.clone()) {
                    out.push(f);
                }
            }
        }
    };

    push_from(&spec_dir.join("spec.md"));

    // Union the wave sub-specs' censuses (a wave plan's parent census is a
    // coordination doc; the wave specs hold the canonical per-wave file lists).
    if let Ok(rd) = std::fs::read_dir(spec_dir) {
        let mut wave_dirs: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("wave-"))
            })
            .collect();
        wave_dirs.sort();
        for dir in wave_dirs {
            push_from(&dir.join("spec.md"));
        }
    }

    out
}

/// Parse the `## Arquivos` / `## Files` section of a spec markdown into the
/// list of file paths it declares. Section runs from the heading to the next
/// `## ` heading or EOF. Each bullet (`- ` / `* `) contributes its first token
/// as a candidate path: surrounding backticks are stripped, and any trailing
/// ` — comment` / ` - comment` / ` (line refs)` is dropped (the parent census
/// of a wave plan annotates each path). A token with no `/` and no recognised
/// file extension is ignored (skips prose bullets like "Onda A — Sidebar").
fn parse_census_paths(text: &str) -> Vec<String> {
    let mut in_section = false;
    let mut in_fence = false;
    let mut out: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed_start = line.trim_start();
        if !in_section {
            if line == "## Arquivos" || line == "## Files" {
                in_section = true;
            }
            continue;
        }
        if !in_fence && line.starts_with("## ") {
            if line == "## Arquivos" || line == "## Files" {
                continue;
            }
            break;
        }
        if trimmed_start.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        // Inside a fenced block, each non-blank non-comment line is a path.
        if in_fence {
            let content = line.trim();
            if content.is_empty() || content.starts_with("//") || content.starts_with('#') {
                continue;
            }
            if let Some(p) = normalise_candidate(content) {
                out.push(p);
            }
            continue;
        }
        // Bullet line outside any fence.
        let rest = trimmed_start
            .strip_prefix("- ")
            .or_else(|| trimmed_start.strip_prefix("* "));
        if let Some(rest) = rest {
            if let Some(p) = extract_path_from_bullet(rest) {
                out.push(p);
            }
        }
    }
    out
}

/// Extract a file path from a bullet body. Takes the first whitespace-delimited
/// token, strips backticks, and validates it looks like a path. Returns `None`
/// for prose bullets (no `/`, no file extension).
fn extract_path_from_bullet(rest: &str) -> Option<String> {
    let first = rest.split_whitespace().next()?;
    normalise_candidate(first)
}

/// Strip backticks/quotes and trailing punctuation from a candidate token and
/// accept it only if it looks like a file path (contains `/` OR has a dotted
/// extension). Returns the cleaned path, or `None` when it is prose.
fn normalise_candidate(token: &str) -> Option<String> {
    let cleaned = token
        .trim()
        .trim_matches(|c| c == '`' || c == '"' || c == '\'')
        .trim_end_matches(|c: char| matches!(c, ',' | ';' | ':' | '.' | ')'))
        .trim();
    if cleaned.is_empty() {
        return None;
    }
    let looks_like_path = cleaned.contains('/')
        || std::path::Path::new(cleaned)
            .extension()
            .is_some_and(|e| !e.is_empty());
    if !looks_like_path {
        return None;
    }
    Some(cleaned.replace('\\', "/"))
}

/// Last-modification epoch (seconds) for a repo file: prefer the git last-commit
/// date (`git log -1 --format=%cI -- <rel>`), fall back to the filesystem mtime
/// when git is unavailable / the file is untracked. `None` when neither is
/// resolvable.
fn file_modified_epoch(base: &Path, rel: &str, abs: &Path) -> Option<i64> {
    if let Some(iso) = git_last_commit_iso(base, rel) {
        if let Some(e) = parse_iso_to_epoch(&iso) {
            return Some(e);
        }
    }
    let meta = std::fs::metadata(abs).ok()?;
    let mtime = meta.modified().ok()?;
    Some(system_time_epoch(mtime))
}

/// `git log -1 --format=%cI -- <rel>` in `base`. `None` on spawn failure,
/// non-zero exit, or empty output (untracked / no history / no git).
fn git_last_commit_iso(base: &Path, rel: &str) -> Option<String> {
    let output = no_window_command("git")
        .args(["log", "-1", "--format=%cI", "--", rel])
        .current_dir(base)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Parse an ISO-8601 timestamp into a Unix epoch (seconds). Handles the
/// `YYYY-MM-DDTHH:MM:SS` core with an optional fractional part and a `Z` /
/// `±HH:MM` offset. Returns `None` for anything it cannot parse — deterministic,
/// no chrono dependency (mirrors the no-extra-crate posture of this crate's
/// other parsers).
fn parse_iso_to_epoch(iso: &str) -> Option<i64> {
    let s = iso.trim();
    let (date, rest) = s.split_once('T').or_else(|| s.split_once(' '))?;
    let mut dp = date.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;

    // Split the time-of-day from any timezone designator.
    let (time_part, offset_secs) = split_offset(rest);
    let mut tp = time_part.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let minute: i64 = tp.next().unwrap_or("0").parse().ok()?;
    // Seconds may carry a fractional part; take the integer head.
    let sec_field = tp.next().unwrap_or("0");
    let second: i64 = sec_field
        .split(['.', ','])
        .next()
        .unwrap_or("0")
        .parse()
        .ok()?;

    let days = days_from_civil(year, month, day);
    let epoch = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_secs;
    Some(epoch)
}

/// Split an ISO time-of-day tail into `(time, offset_seconds)`. A trailing `Z`
/// (or no designator) is UTC (offset 0); a `±HH:MM` shifts accordingly.
fn split_offset(rest: &str) -> (&str, i64) {
    if let Some(stripped) = rest.strip_suffix('Z') {
        return (stripped, 0);
    }
    // Find a `+`/`-` that introduces the offset (after the time digits). The
    // time itself never contains `+`; a `-` only appears in the offset.
    for (i, c) in rest.char_indices() {
        if (c == '+' || c == '-') && i > 0 {
            let (time, off) = rest.split_at(i);
            let sign = if c == '-' { -1 } else { 1 };
            let off = &off[1..]; // skip the sign
            let mut op = off.split(':');
            let oh: i64 = op.next().unwrap_or("0").parse().unwrap_or(0);
            let om: i64 = op.next().unwrap_or("0").parse().unwrap_or(0);
            return (time, sign * (oh * 3_600 + om * 60));
        }
    }
    (rest, 0)
}

/// Days since the Unix epoch (1970-01-01) for a civil date. Howard Hinnant's
/// `days_from_civil` algorithm — exact, branch-free, no external crate.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Seconds since the Unix epoch for a `SystemTime`. Pre-epoch times clamp to 0.
fn system_time_epoch(t: SystemTime) -> i64 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso_handles_z_and_offset() {
        // 2026-06-23T12:24:05Z → known epoch.
        let z = parse_iso_to_epoch("2026-06-23T12:24:05Z").unwrap();
        // Same instant expressed with a +00:00 offset.
        let zero = parse_iso_to_epoch("2026-06-23T12:24:05+00:00").unwrap();
        assert_eq!(z, zero);
        // A +03:00 offset is three hours EARLIER in UTC.
        let plus3 = parse_iso_to_epoch("2026-06-23T12:24:05+03:00").unwrap();
        assert_eq!(plus3, z - 3 * 3_600);
        // Fractional seconds are tolerated (integer head taken).
        let frac = parse_iso_to_epoch("2026-06-23T12:24:05.478Z").unwrap();
        assert_eq!(frac, z);
    }

    #[test]
    fn days_from_civil_matches_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(2000, 1, 1), 10_957);
    }

    #[test]
    fn parse_census_backtick_bullets() {
        let md = "# Spec\n\n## Arquivos\n\n- `apps/web/src/a.tsx`\n- `apps/web/src/b.rs`\n\n## Outra\n- `ignored.ts`\n";
        let files = parse_census_paths(md);
        assert_eq!(files, vec!["apps/web/src/a.tsx", "apps/web/src/b.rs"]);
    }

    #[test]
    fn parse_census_prose_annotations_and_skips_non_paths() {
        // The wave-plan parent census shape: path + ` — comment`, plus prose
        // bullets ("Onda A — Sidebar") that must NOT be taken as paths.
        let md = "## Arquivos\n\nOnda A — Sidebar:\n- apps/dashboard/src/pages/Specs.tsx — tirar bloqueio (420)\n- packages/core/src/lib.rs — algo\n- Onda B é só prosa\n";
        let files = parse_census_paths(md);
        assert_eq!(
            files,
            vec!["apps/dashboard/src/pages/Specs.tsx", "packages/core/src/lib.rs"]
        );
    }

    #[test]
    fn parse_census_fenced_block() {
        let md = "## Files\n\n```\nsrc/main.rs\n// a comment\nsrc/lib.rs\n```\n";
        let files = parse_census_paths(md);
        assert_eq!(files, vec!["src/main.rs", "src/lib.rs"]);
    }

    #[test]
    fn missing_spec_md_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let s = plan_staleness_impl(&dir.path().to_string_lossy(), "no-such-spec", None);
        assert_eq!(s.verdict, "unknown");
        assert_eq!(s.reason, "spec.md inexistente");
    }

    #[test]
    fn no_census_is_unknown_not_stale() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("s1");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Spec\n\nNo census here.\n").unwrap();
        let s = plan_staleness_impl(&dir.path().to_string_lossy(), "s1", None);
        assert_eq!(s.verdict, "unknown");
        assert_eq!(s.reason, "sem censo de arquivos");
    }

    #[test]
    fn missing_file_marks_stale() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("s1");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "## Arquivos\n\n- `src/gone.rs`\n",
        )
        .unwrap();
        let s = plan_staleness_impl(
            &dir.path().to_string_lossy(),
            "s1",
            Some("2020-01-01T00:00:00Z"),
        );
        assert_eq!(s.verdict, "stale");
        assert_eq!(s.missing, vec!["src/gone.rs"]);
        assert_eq!(s.total, 1);
    }

    #[test]
    fn old_plan_with_intact_census_is_fresh_not_stale() {
        // Regression for "age removed from the staleness trigger": a spec whose
        // plan date is years old, but whose only census file still exists and
        // was last modified BEFORE that plan date, must resolve to `fresh` — age
        // alone no longer marks a plan stale (it used to flip to `stale` once
        // `age_days >= STALE_AGE_DAYS`).
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("s1");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "## Arquivos\n\n- `keep.rs`\n").unwrap();

        // Create the census file and backdate its mtime to 2010, well before the
        // plan date below. There is no git in this tempdir, so the impl compares
        // against the filesystem mtime; backdating it keeps the file from being
        // flagged `changed` against an old (2020) plan date.
        let f = std::fs::File::create(dir.path().join("keep.rs")).unwrap();
        // 2010-01-01T00:00:00Z = 1_262_304_000s since epoch.
        let mtime_2010 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_262_304_000);
        f.set_modified(mtime_2010).unwrap();
        drop(f);

        // Plan date in 2020 → already very old (high `age_days`), yet the file's
        // 2010 mtime is before it, so nothing is missing or changed.
        let s = plan_staleness_impl(
            &dir.path().to_string_lossy(),
            "s1",
            Some("2020-01-01T00:00:00Z"),
        );
        assert_eq!(s.verdict, "fresh", "old plan + intact census must be fresh");
        assert!(s.missing.is_empty());
        assert!(s.changed.is_empty());
        // Age is still reported as evidence even though it no longer triggers.
        assert!(s.age_days >= 7, "age is large but does not mark stale");
    }

    #[test]
    fn rejects_traversal_spec_name() {
        let dir = tempfile::tempdir().unwrap();
        let s = plan_staleness_impl(&dir.path().to_string_lossy(), "../evil", None);
        assert_eq!(s.verdict, "unknown");
    }
}
