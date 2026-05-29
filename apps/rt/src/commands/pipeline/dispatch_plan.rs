//! `mustard-rt run dispatch-plan` — deterministic wave-routing face.
//!
//! `resume_bootstrap` is the **stage-decision** face (mode / stage / current
//! wave / model); `dispatch-plan` is the **wave-routing** face. It reads a
//! spec's `wave-plan.md`, builds the wave dependency DAG, and emits a
//! deterministic JSON array ordered by dependency level. Each item carries the
//! information the orchestrator needs to dispatch one agent — including a ready
//! `prompt_cmd` (an `agent-prompt-render` invocation) it can run and relay to
//! the `Task` tool.
//!
//! The orchestrator stops deciding the wave order by hand: it iterates the
//! array, runs each `prompt_cmd`, and passes the stdout to `Task`. The "free
//! section" of the orchestrator (interpreting the wave-plan and assembling the
//! dispatch loop) becomes a Rust-owned, LLM-free relay.
//!
//! ## Ordering and `level`
//!
//! Items are sorted by `(level, wave)`. `level` is the topological depth of the
//! wave in the dependency DAG: waves with no dependencies are level 0, a wave
//! that depends on a level-0 wave is level 1, and so on. Two waves that share a
//! level have no dependency between them and may be dispatched together in a
//! single round (one `Task` message with several `<invoke>` blocks).
//!
//! ## Parsers reused (no reimplementation)
//!
//! - The wave-plan table reader recognises the same row shapes that
//!   [`crate::commands::pipeline::resume_bootstrap::extract_wave_model`] and
//!   [`crate::commands::wave::wave_scaffold`] produce (with or without a `Spec`
//!   / `Modelo` column) — column roles are resolved from the header row, not by
//!   fixed index.
//! - Dependency cells are parsed with the single workspace `[[…]]` scanner
//!   ([`mustard_core::io::atomic_md::find_outgoing_links`]).
//! - The per-wave subproject is derived from the wave's `spec.md`
//!   `## Files` / `## Arquivos` section
//!   ([`crate::commands::wave::wave_lib::parse_files_section`]) routed through
//!   the same `apps/<name>` / `packages/<name>` detector that
//!   `dependency-precheck` uses.
//!
//! ## Fail-open contract
//!
//! A missing / unparseable `wave-plan.md`, a non-wave spec, or a single-wave
//! spec all degrade coherently: the process never panics, always exits 0, and
//! emits a (possibly empty or single-item) JSON array. A dependency cycle
//! degrades to source order (every item keeps `level: 0`) rather than dropping
//! waves.

use crate::commands::review::dependency_precheck::detect_subproject;
use crate::commands::wave::wave_lib::parse_files_section;
use crate::shared::context::project_dir;
use mustard_core::io::atomic_md::find_outgoing_links;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs as mfs;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One dispatch item — a single agent the orchestrator will fan out.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DispatchItem {
    /// 1-based wave number (matches the `wave-N-{role}` directory).
    pub wave: u32,
    /// Role token (the `{role}` suffix of `wave-N-{role}`).
    pub role: String,
    /// Subproject path relative to the project root (`apps/<name>` /
    /// `packages/<name>`), or `"."` when the wave's `## Files` do not converge
    /// on a single subproject (fail-open).
    pub subproject: String,
    /// Wave numbers this wave depends on, ascending. Empty when independent.
    #[serde(rename = "depends_on")]
    pub depends_on: Vec<u32>,
    /// Topological depth. Items sharing a level are dispatch-parallel.
    pub level: u32,
    /// Ready `agent-prompt-render` invocation. The orchestrator runs this and
    /// passes the **stdout** (the rendered prompt) to the `Task` tool — it must
    /// NOT treat this string as the prompt itself.
    #[serde(rename = "prompt_cmd")]
    pub prompt_cmd: String,
}

/// A wave row parsed out of `wave-plan.md` (pre-ordering).
#[derive(Debug, Clone)]
struct WaveRow {
    wave: u32,
    role: String,
    depends_on: Vec<u32>,
}

/// Run `mustard-rt run dispatch-plan --spec <slug> [--wave N]`.
///
/// `wave_filter` (the `--wave` flag) restricts the emitted array to that single
/// wave (still carrying its real `depends_on` / `level`), so the orchestrator
/// can re-render one wave's dispatch without recomputing the whole plan.
pub fn run(spec: &str, wave_filter: Option<u32>) {
    let project = PathBuf::from(project_dir());
    let spec_dir = resolve_spec_dir(&project, spec);

    let items = build_plan(&project, &spec_dir, spec, wave_filter);

    let body = serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".to_string());
    println!("{body}");
}

/// Resolve the spec directory through the canonical accessor, fail-open to the
/// unchecked composition (mirrors `resume_bootstrap`).
fn resolve_spec_dir(project: &Path, spec: &str) -> PathBuf {
    ClaudePaths::for_project(project)
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.dir().to_path_buf())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(project).spec_dir().join(spec))
}

/// Assemble the ordered dispatch items for `spec`.
///
/// Pure aside from filesystem reads; extracted so the tests can drive it with a
/// temp spec dir.
fn build_plan(
    project: &Path,
    spec_dir: &Path,
    spec: &str,
    wave_filter: Option<u32>,
) -> Vec<DispatchItem> {
    // 1. Read the wave rows (event-free, FS-first). Plan table preferred; the
    //    `wave-N-{role}/` directories are the fallback when the table is absent.
    let rows = read_wave_rows(spec_dir);
    if rows.is_empty() {
        return Vec::new();
    }

    // 2. Topological level assignment over the wave-number DAG.
    let levels = assign_levels(&rows);

    // 3. Materialise each item, then sort by (level, wave) so independent waves
    //    of the same level group into one dispatch round.
    let mut items: Vec<DispatchItem> = rows
        .iter()
        .map(|row| {
            let subproject = derive_subproject(project, spec_dir, row.wave, &row.role);
            let level = levels.get(&row.wave).copied().unwrap_or(0);
            DispatchItem {
                wave: row.wave,
                role: row.role.clone(),
                subproject: subproject.clone(),
                depends_on: row.depends_on.clone(),
                level,
                prompt_cmd: render_prompt_cmd(spec, row.wave, &row.role, &subproject),
            }
        })
        .collect();

    items.sort_by(|a, b| a.level.cmp(&b.level).then(a.wave.cmp(&b.wave)));

    // 4. `--wave N` slice (still carries the real depends_on/level).
    if let Some(w) = wave_filter {
        items.retain(|it| it.wave == w);
    }
    items
}

/// Build the `agent-prompt-render` invocation for one item. `--mode first` is
/// the dispatch (non-retry) render; the orchestrator swaps to
/// `granular`/`fix-loop` itself on a retry.
fn render_prompt_cmd(spec: &str, wave: u32, role: &str, subproject: &str) -> String {
    format!(
        "mustard-rt run agent-prompt-render --spec {spec} --wave {wave} --role {role} \
         --subproject {subproject} --mode first"
    )
}

// ---------------------------------------------------------------------------
// Wave-plan parsing
// ---------------------------------------------------------------------------

/// Read the spec's wave rows. Prefers the `wave-plan.md` table; falls back to
/// scanning `wave-N-{role}/` directories (chaining each wave on the previous,
/// the same default `plan-from-spec` emits) when the table is absent.
fn read_wave_rows(spec_dir: &Path) -> Vec<WaveRow> {
    let plan_path = spec_dir.join("wave-plan.md");
    if let Ok(text) = mfs::read_to_string(&plan_path) {
        let rows = parse_wave_plan_table(&text);
        if !rows.is_empty() {
            return rows;
        }
    }
    rows_from_fs(spec_dir)
}

/// Parse the wave-plan markdown table into `WaveRow`s.
///
/// The table column order varies across the two renderers (`wave_scaffold`
/// emits `Spec` + no `Modelo`; older plans add a `Modelo` column; the
/// fixture form drops `Spec`). Rather than index by position we read the
/// header row to find which data cell holds the wave number, the role, and the
/// dependency list. Rows whose first cell parses as a wave number drive the
/// result; the `depends_on` cell is parsed via the shared `[[…]]` scanner and
/// normalised to wave numbers.
fn parse_wave_plan_table(text: &str) -> Vec<WaveRow> {
    let mut header: Option<Vec<String>> = None;
    let mut rows: Vec<WaveRow> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells = split_row(trimmed);
        if cells.is_empty() {
            continue;
        }
        // Separator row (`|---|---|`) — skip.
        if cells.iter().all(|c| is_separator_cell(c)) {
            continue;
        }
        // First table row that is not numeric in cell 0 is the header.
        let first_is_num = parse_wave_label(&cells[0]).is_some();
        if header.is_none() && !first_is_num {
            header = Some(cells.iter().map(|c| c.to_ascii_lowercase()).collect());
            continue;
        }
        if !first_is_num {
            continue;
        }
        let Some(wave) = parse_wave_label(&cells[0]) else {
            continue;
        };
        let cols = ColumnMap::from_header(header.as_deref());
        let role = cols
            .role
            .and_then(|i| cells.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "mixed".to_string());
        let depends_on = cols
            .depends_on
            .and_then(|i| cells.get(i))
            .map(|cell| parse_depends_cell(cell, wave))
            .unwrap_or_default();
        rows.push(WaveRow {
            wave,
            role,
            depends_on,
        });
    }
    rows.sort_by_key(|r| r.wave);
    rows.dedup_by_key(|r| r.wave);
    rows
}

/// Resolved data-cell indices for the columns we care about.
struct ColumnMap {
    role: Option<usize>,
    depends_on: Option<usize>,
}

impl ColumnMap {
    /// Map the header cells to column roles. When no header is available
    /// (FS-derived or malformed), fall back to the canonical
    /// `wave_scaffold` layout `| Wave | Spec | Role | Depends on | Summary |`.
    fn from_header(header: Option<&[String]>) -> Self {
        let Some(header) = header else {
            // Canonical scaffold layout indices.
            return ColumnMap {
                role: Some(2),
                depends_on: Some(3),
            };
        };
        let mut role = None;
        let mut depends_on = None;
        for (i, h) in header.iter().enumerate() {
            let h = h.trim();
            if role.is_none() && h == "role" {
                role = Some(i);
            }
            // EN "depends on" / PT "depende de" — match the stem so spacing /
            // accents do not matter.
            if depends_on.is_none() && (h.starts_with("depends") || h.starts_with("depende")) {
                depends_on = Some(i);
            }
        }
        ColumnMap { role, depends_on }
    }
}

/// Split a `| a | b | c |` row into trimmed cell strings (no leading/trailing
/// empties from the bounding pipes).
fn split_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// A markdown table separator cell is dashes only (optionally colon-aligned).
fn is_separator_cell(cell: &str) -> bool {
    let c = cell.trim();
    !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')
}

/// Parse a wave label cell (`1`, `W1`, `w2`) into its number. `None` when it is
/// not a wave label (so header / prose rows are rejected).
fn parse_wave_label(cell: &str) -> Option<u32> {
    let t = cell.trim().trim_start_matches(['W', 'w']).trim();
    t.parse::<u32>().ok()
}

/// Parse the dependency cell into wave numbers.
///
/// Accepts both `[[1]]` (number form) and `[[wave-1-general]]` (name form) via
/// the shared scanner; an em-dash / hyphen / empty cell means "no deps". A
/// self-reference (a wave depending on itself, which the topo pass cannot use)
/// is dropped.
fn parse_depends_cell(cell: &str, self_wave: u32) -> Vec<u32> {
    let trimmed = cell.trim();
    if trimmed.is_empty() || trimmed == "—" || trimmed == "-" {
        return Vec::new();
    }
    let mut out: Vec<u32> = Vec::new();
    for link in find_outgoing_links(trimmed) {
        if let Some(n) = wave_number_from_link(&link) {
            if n != self_wave && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out.sort_unstable();
    out
}

/// Resolve a dependency `[[…]]` token to a wave number. `[[1]]` → 1;
/// `[[wave-1-general]]` → 1; anything else → `None`.
fn wave_number_from_link(link: &str) -> Option<u32> {
    let inner = link.trim();
    if let Ok(n) = inner.parse::<u32>() {
        return Some(n);
    }
    let rest = inner.strip_prefix("wave-")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u32>().ok()
}

/// Fallback: derive wave rows from the `wave-N-{role}/` directories on disk.
/// Each wave is chained on the previous (the `plan-from-spec` default), which
/// keeps a sequential dispatch order when no explicit DAG is declared.
fn rows_from_fs(spec_dir: &Path) -> Vec<WaveRow> {
    let Ok(entries) = mfs::read_dir(spec_dir) else {
        return Vec::new();
    };
    let mut pairs: Vec<(u32, String)> = Vec::new();
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let Some(rest) = entry.file_name.strip_prefix("wave-") else {
            continue;
        };
        let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if digit_end == 0 {
            continue;
        }
        let Ok(n) = rest[..digit_end].parse::<u32>() else {
            continue;
        };
        let role = rest[digit_end..]
            .strip_prefix('-')
            .filter(|r| !r.is_empty())
            .unwrap_or("mixed")
            .to_string();
        pairs.push((n, role));
    }
    pairs.sort_by_key(|p| p.0);
    pairs.dedup_by_key(|p| p.0);
    let mut rows: Vec<WaveRow> = Vec::new();
    let mut prev: Option<u32> = None;
    for (n, role) in pairs {
        let depends_on = prev.map(|p| vec![p]).unwrap_or_default();
        rows.push(WaveRow {
            wave: n,
            role,
            depends_on,
        });
        prev = Some(n);
    }
    rows
}

// ---------------------------------------------------------------------------
// Level assignment (topological depth)
// ---------------------------------------------------------------------------

/// Assign a topological level to each wave: a wave's level is one more than the
/// deepest level among its in-plan dependencies; waves with no in-plan
/// dependency are level 0.
///
/// Robust to a malformed DAG: a dependency on an unknown wave is ignored, and a
/// cycle degrades to level 0 for the stuck nodes (so no wave is dropped —
/// fail-open). Deterministic regardless of input order.
fn assign_levels(rows: &[WaveRow]) -> BTreeMap<u32, u32> {
    let known: std::collections::BTreeSet<u32> = rows.iter().map(|r| r.wave).collect();
    let deps: BTreeMap<u32, Vec<u32>> = rows
        .iter()
        .map(|r| {
            let filtered: Vec<u32> = r
                .depends_on
                .iter()
                .copied()
                .filter(|d| known.contains(d))
                .collect();
            (r.wave, filtered)
        })
        .collect();

    let mut level: BTreeMap<u32, u32> = BTreeMap::new();
    // Iterate to a fixpoint, bounded by the node count so a cycle terminates.
    let max_iters = rows.len() + 1;
    for _ in 0..max_iters {
        let mut changed = false;
        for (&wave, wave_deps) in &deps {
            let new_level = wave_deps
                .iter()
                .filter_map(|d| level.get(d).map(|l| l + 1))
                .max()
                .unwrap_or(0);
            // Only assign when all deps already have a level (otherwise wait a
            // round); a dep with no level yet leaves us at the provisional 0.
            let resolved = wave_deps.iter().all(|d| level.contains_key(d));
            let entry = level.entry(wave).or_insert(0);
            if resolved && *entry != new_level {
                *entry = new_level;
                changed = true;
            } else if !resolved {
                // Provisionally seed at 0 so the node is never dropped.
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    level
}

// ---------------------------------------------------------------------------
// Subproject derivation
// ---------------------------------------------------------------------------

/// Derive the subproject for a wave from its `spec.md` `## Files` section.
///
/// Reuses `parse_files_section` (wave-lib) + `detect_subproject`
/// (dependency-precheck). Returns a project-relative `apps/<name>` /
/// `packages/<name>` string, or `"."` when the files do not converge on one
/// subproject / the wave dir is absent (fail-open).
fn derive_subproject(project: &Path, spec_dir: &Path, wave: u32, role: &str) -> String {
    let wave_spec = wave_spec_path(spec_dir, wave, role);
    let Some(wave_spec) = wave_spec else {
        return ".".to_string();
    };
    let Ok(text) = mfs::read_to_string(&wave_spec) else {
        return ".".to_string();
    };
    let files = parse_files_section(&text).unwrap_or_default();
    if files.is_empty() {
        return ".".to_string();
    }
    match detect_subproject(&files, project) {
        Some(abs) => abs
            .strip_prefix(project)
            .unwrap_or(&abs)
            .to_string_lossy()
            .replace('\\', "/"),
        None => ".".to_string(),
    }
}

/// Resolve the on-disk `wave-{wave}-{role}/spec.md` path. Prefers the exact
/// `{role}` directory; falls back to the first `wave-{wave}-*` directory when
/// the role suffix differs (the plan table role can diverge from the folder).
fn wave_spec_path(spec_dir: &Path, wave: u32, role: &str) -> Option<PathBuf> {
    let exact = spec_dir.join(format!("wave-{wave}-{role}")).join("spec.md");
    if exact.is_file() {
        return Some(exact);
    }
    let entries = mfs::read_dir(spec_dir).ok()?;
    let prefix = format!("wave-{wave}-");
    for entry in entries {
        if entry.is_dir && entry.file_name.starts_with(&prefix) {
            let candidate = entry.path.join("spec.md");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

    #[test]
    fn parse_table_scaffold_form_with_spec_column() {
        // The `wave_scaffold` renderer: `| Wave | Spec | Role | Depends on | Summary |`.
        let plan = "\
# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave-1-general]] | general | — | foundations |
| 2 | [[wave-2-ui]] | ui | [[wave-1-general]] | pieces |
| 3 | [[wave-3-api]] | api | [[wave-2-ui]] | endpoints |
";
        let rows = parse_wave_plan_table(plan);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].wave, 1);
        assert_eq!(rows[0].role, "general");
        assert!(rows[0].depends_on.is_empty());
        assert_eq!(rows[1].role, "ui");
        assert_eq!(rows[1].depends_on, vec![1]);
        assert_eq!(rows[2].depends_on, vec![2]);
    }

    #[test]
    fn parse_table_fixture_form_no_spec_column_number_links() {
        // The dependency_precheck fixture form: `| Wave | Role | Depende de | Summary |`
        // with number-only dependency links `[[1]]`.
        let plan = "\
## Waves Table

| Wave | Role | Depende de | Summary |
|------|------|------------|---------|
| 1 | general | — | foundation |
| 2 | ui | [[1]] | primitives |
| 3 | ui | [[2]] | pages |
";
        let rows = parse_wave_plan_table(plan);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].role, "general");
        assert_eq!(rows[1].depends_on, vec![1]);
        assert_eq!(rows[2].depends_on, vec![2]);
    }

    #[test]
    fn parse_table_with_modelo_column() {
        // Legacy form carrying a `Modelo` column (what extract_wave_model reads).
        let plan = "\
| Wave | Spec | Role | Modelo | Depende de | Resumo |
|------|------|------|--------|------------|--------|
| 1 | [[wave-1-general]] | general | opus | — | foo |
| 2 | [[wave-2-ui]] | ui | sonnet | [[1]] | bar |
";
        let rows = parse_wave_plan_table(plan);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].role, "general");
        assert_eq!(rows[1].role, "ui");
        assert_eq!(rows[1].depends_on, vec![1]);
    }

    #[test]
    fn levels_chain_is_sequential() {
        let rows = vec![
            WaveRow { wave: 1, role: "a".into(), depends_on: vec![] },
            WaveRow { wave: 2, role: "b".into(), depends_on: vec![1] },
            WaveRow { wave: 3, role: "c".into(), depends_on: vec![2] },
        ];
        let levels = assign_levels(&rows);
        assert_eq!(levels[&1], 0);
        assert_eq!(levels[&2], 1);
        assert_eq!(levels[&3], 2);
    }

    #[test]
    fn levels_independent_waves_share_a_level() {
        // 1 has no deps; 2 and 3 both depend only on 1 → both level 1 (parallel).
        let rows = vec![
            WaveRow { wave: 1, role: "a".into(), depends_on: vec![] },
            WaveRow { wave: 2, role: "b".into(), depends_on: vec![1] },
            WaveRow { wave: 3, role: "c".into(), depends_on: vec![1] },
        ];
        let levels = assign_levels(&rows);
        assert_eq!(levels[&1], 0);
        assert_eq!(levels[&2], 1);
        assert_eq!(levels[&3], 1);
    }

    #[test]
    fn levels_cycle_degrades_to_zero_without_dropping() {
        // 1 → 2 → 1 cycle: fail-open keeps both, at level 0.
        let rows = vec![
            WaveRow { wave: 1, role: "a".into(), depends_on: vec![2] },
            WaveRow { wave: 2, role: "b".into(), depends_on: vec![1] },
        ];
        let levels = assign_levels(&rows);
        assert_eq!(levels.len(), 2);
        assert!(levels.contains_key(&1));
        assert!(levels.contains_key(&2));
    }

    #[test]
    fn wave_number_from_link_handles_both_forms() {
        assert_eq!(wave_number_from_link("1"), Some(1));
        assert_eq!(wave_number_from_link("wave-2-general"), Some(2));
        assert_eq!(wave_number_from_link("wave-12-rt"), Some(12));
        assert_eq!(wave_number_from_link("memory/foo"), None);
    }

    #[test]
    fn prompt_cmd_is_a_valid_agent_prompt_render_invocation() {
        let cmd = render_prompt_cmd("2026-05-29-demo", 2, "ui", "apps/dashboard");
        assert!(cmd.starts_with("mustard-rt run agent-prompt-render "));
        assert!(cmd.contains("--spec 2026-05-29-demo"));
        assert!(cmd.contains("--wave 2"));
        assert!(cmd.contains("--role ui"));
        assert!(cmd.contains("--subproject apps/dashboard"));
        assert!(cmd.contains("--mode first"));
    }

    /// End-to-end: a multi-wave spec with a real `wave-plan.md` + wave dirs
    /// yields an ordered array with correct `{wave,role,subproject,depends_on}`.
    #[test]
    fn build_plan_multi_wave_orders_by_dependency() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("demo")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "\
| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave-1-rt]] | rt | — | base |
| 2 | [[wave-2-cli]] | cli | [[wave-1-rt]] | uses base |
",
        )
        .unwrap();
        // Wave 1 declares files under apps/rt; wave 2 under apps/cli.
        std::fs::create_dir_all(spec_dir.join("wave-1-rt")).unwrap();
        std::fs::write(
            spec_dir.join("wave-1-rt").join("spec.md"),
            "# W1\n\n## Files\n- apps/rt/src/foo.rs\n",
        )
        .unwrap();
        std::fs::create_dir_all(spec_dir.join("wave-2-cli")).unwrap();
        std::fs::write(
            spec_dir.join("wave-2-cli").join("spec.md"),
            "# W2\n\n## Files\n- apps/cli/src/bar.rs\n",
        )
        .unwrap();

        let items = build_plan(project, &spec_dir, "demo", None);
        assert_eq!(items.len(), 2);
        // Ordered by (level, wave): wave 1 (level 0) first.
        assert_eq!(items[0].wave, 1);
        assert_eq!(items[0].role, "rt");
        assert_eq!(items[0].subproject, "apps/rt");
        assert_eq!(items[0].level, 0);
        assert!(items[0].depends_on.is_empty());
        assert!(items[0].prompt_cmd.contains("--wave 1"));
        assert!(items[0].prompt_cmd.contains("--subproject apps/rt"));

        assert_eq!(items[1].wave, 2);
        assert_eq!(items[1].role, "cli");
        assert_eq!(items[1].subproject, "apps/cli");
        assert_eq!(items[1].level, 1);
        assert_eq!(items[1].depends_on, vec![1]);
    }

    /// `--wave N` slices to a single item, preserving its real depends_on.
    #[test]
    fn build_plan_wave_filter_slices_to_one() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("demo")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "\
| Wave | Role | Depende de | Summary |
|------|------|------------|---------|
| 1 | a | — | x |
| 2 | b | [[1]] | y |
",
        )
        .unwrap();
        let items = build_plan(project, &spec_dir, "demo", Some(2));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].wave, 2);
        assert_eq!(items[0].depends_on, vec![1]);
    }

    /// Single-wave spec → exactly one item, no deps.
    #[test]
    fn build_plan_single_wave() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("solo")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "\
| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave-1-mixed]] | mixed | — | only |
",
        )
        .unwrap();
        let items = build_plan(project, &spec_dir, "solo", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].wave, 1);
        assert!(items[0].depends_on.is_empty());
        assert_eq!(items[0].level, 0);
    }

    /// Non-wave spec (no plan, no wave dirs) → empty array, fail-open.
    #[test]
    fn build_plan_non_wave_is_empty() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("flat")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Flat\n## Tasks\n- x\n").unwrap();
        let items = build_plan(project, &spec_dir, "flat", None);
        assert!(items.is_empty());
    }

    /// No `wave-plan.md` but `wave-N-{role}/` dirs exist → FS fallback chains
    /// them sequentially.
    #[test]
    fn build_plan_fs_fallback_chains_waves() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("fsfb")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(spec_dir.join("wave-1-rt")).unwrap();
        std::fs::write(spec_dir.join("wave-1-rt").join("spec.md"), "# w1\n").unwrap();
        std::fs::create_dir_all(spec_dir.join("wave-2-cli")).unwrap();
        std::fs::write(spec_dir.join("wave-2-cli").join("spec.md"), "# w2\n").unwrap();

        let items = build_plan(project, &spec_dir, "fsfb", None);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].wave, 1);
        assert!(items[0].depends_on.is_empty());
        assert_eq!(items[1].wave, 2);
        assert_eq!(items[1].depends_on, vec![1]);
        assert_eq!(items[1].level, 1);
    }
}
