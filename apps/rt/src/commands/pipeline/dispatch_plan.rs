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
//!   [`crate::commands::wave::wave_scaffold`] produces (with or without a `Spec`
//!   column) — column roles are resolved from the header row, not by fixed
//!   index, so an extra legacy column is tolerated.
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
//! emits a (possibly empty or single-item) JSON array. A **non-wave spec with
//! a `spec.md`** (tactical fix / Light scope) emits a one-item plan
//! (`wave: 0`, role `impl`, no `--wave` in the `prompt_cmd` — see
//! [`single_spec_plan`]); a spec with no `spec.md` at all still emits `[]`. A
//! dependency cycle degrades to source order (every item keeps `level: 0`)
//! rather than dropping waves.

use crate::commands::review::dependency_precheck::detect_subproject;
use crate::commands::wave::wave_lib::parse_files_section;
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
    /// The `subagent_type` the orchestrator must pass to `Task` for this role.
    /// Read-only roles resolve to tool-restricted agents (`explore` → `Explore`,
    /// `review`/`qa` → `mustard:mustard-review`, `guards` → `mustard:mustard-guards`);
    /// writing roles stay `general-purpose`. Picked by the tool — never by hand.
    /// See [`crate::commands::agent::agent_prompt_render::recommended_subagent_type`].
    #[serde(rename = "subagent_type")]
    pub subagent_type: String,
    /// `true` when this item's subproject is its OWN nested git repository (a
    /// submodule: `.git` dir/file at its dir) — the git-boundary fact the scan
    /// census records for the matching `projects[]` entry. Threaded so the
    /// rendered implementer prompt can state the boundary (separate commit
    /// history; never bump the superproject gitlink pointer) and the branch gate
    /// can base the work branch on the submodule's own default branch. Omitted
    /// from the JSON when `false` so the dispatch output stays byte-stable for a
    /// non-submodule subproject (the common case).
    #[serde(default, skip_serializing_if = "is_false")]
    pub own_git_root: bool,
}

/// serde `skip_serializing_if` for the additive [`DispatchItem::own_git_root`]:
/// omit the field when `false` so the dispatch-plan JSON is byte-identical to
/// the pre-flag shape for every non-submodule subproject.
fn is_false(b: &bool) -> bool {
    !*b
}

/// The git-boundary FACT for an item's subproject: `true` when the subproject's
/// own directory is a nested git repository root (`.git` dir or pointer file) —
/// the SAME fact the census records for its matching `projects[]` entry (see
/// [`mustard_core::mark_own_git_roots`]), derived here by the single shared
/// probe [`mustard_core::io::workspace::is_git_repo_root`] so dispatch needs no
/// census round-trip through the external scan tool. The superproject root
/// (`"."` / empty) is never a nested boundary. Purely a filesystem probe.
fn subproject_own_git_root(project: &Path, subproject: &str) -> bool {
    let sub = subproject.trim();
    if sub.is_empty() || sub == "." {
        return false;
    }
    mustard_core::io::workspace::is_git_repo_root(&project.join(sub))
}

/// A wave row parsed out of `wave-plan.md` (pre-ordering).
#[derive(Debug, Clone)]
struct WaveRow {
    wave: u32,
    role: String,
    depends_on: Vec<u32>,
}

/// Resolve the spec directory through the canonical accessor, fail-open to the
/// unchecked composition (mirrors `resume_bootstrap`). `pub(crate)` so
/// `wave-advance` resolves the same directory the dispatch plan was built from.
pub(crate) fn resolve_spec_dir(project: &Path, spec: &str) -> PathBuf {
    ClaudePaths::spec_dir_or_unchecked(project, spec)
}

/// Assemble the ordered dispatch items for `spec`.
///
/// Pure aside from filesystem reads; extracted so the tests can drive it with a
/// temp spec dir. `pub(crate)` so `wave-advance` composes the same routing
/// in-process (single DAG source — no subprocess, no reimplementation).
pub(crate) fn build_plan(
    project: &Path,
    spec_dir: &Path,
    spec: &str,
    wave_filter: Option<u32>,
) -> Vec<DispatchItem> {
    // 1. Read the wave rows (event-free, FS-first). Plan table preferred; the
    //    `wave-N-{role}/` directories are the fallback when the table is absent.
    let rows = read_wave_rows(spec_dir);
    if rows.is_empty() {
        // Non-wave spec (no plan table, no wave dirs): degrade to the
        // single-spec one-item plan instead of an empty array, so tactical
        // fixes / Light-scope specs dispatch through the same relay. A spec
        // with no `spec.md` still yields `[]` (the historical contract for an
        // unknown spec). The `--wave` filter applies here too: a non-zero
        // filter against the wave-less item (wave 0) empties the plan.
        let mut items = single_spec_plan(project, spec_dir, spec);
        if let Some(w) = wave_filter {
            items.retain(|it| it.wave == w);
        }
        return items;
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
                subagent_type: crate::commands::agent::agent_prompt_render::recommended_subagent_type(
                    &row.role,
                ),
                own_git_root: subproject_own_git_root(project, &subproject),
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
    // `--emit ref`: the command prints a 2-line stub (the full prompt goes to
    // the spec's `.dispatch/` file) — the orchestrator passes the stub
    // verbatim to `Task` and the PreToolUse hook expands it, so the full
    // text never transits the orchestrator's context.
    format!(
        "mustard-rt run agent-prompt-render --spec {spec} --wave {wave} --role {role} \
         --subproject {subproject} --mode first --emit ref"
    )
}

/// Single-spec (non-wave) fallback: a spec dir carrying a `spec.md` but no
/// `wave-plan.md` / `wave-N-{role}/` dirs (tactical fixes, Light scope) emits
/// a one-item plan.
///
/// Shape decisions:
/// - `wave: 0` — real waves are 1-based, so `0` marks "wave-less" while the
///   field keeps the numeric shape every consumer already parses.
/// - `role: "impl"` — the writing role; `recommended_subagent_type` resolves
///   it to `general-purpose` (never hand-picked).
/// - `prompt_cmd` carries **no** `--wave` flag (`agent-prompt-render` renders
///   the spec's own `spec.md` in that case).
/// - The subproject comes from the spec's `## Files` / `## Arquivos` section
///   through the same `parse_files_section` + `detect_subproject` pair the
///   wave path uses; no convergence → `"."`.
///
/// A missing `spec.md` (unknown spec) still degrades to the empty array — the
/// historical contract is untouched.
fn single_spec_plan(project: &Path, spec_dir: &Path, spec: &str) -> Vec<DispatchItem> {
    let Ok(text) = mfs::read_to_string(spec_dir.join("spec.md")) else {
        return Vec::new();
    };
    let role = "impl";
    let files = parse_files_section(&text).unwrap_or_default();
    let subproject = files_to_subproject(project, &files);
    vec![DispatchItem {
        wave: 0,
        role: role.to_string(),
        subproject: subproject.clone(),
        depends_on: Vec::new(),
        level: 0,
        prompt_cmd: format!(
            "mustard-rt run agent-prompt-render --spec {spec} --role {role} \
             --subproject {subproject} --mode first --emit ref"
        ),
        subagent_type: crate::commands::agent::agent_prompt_render::recommended_subagent_type(
            role,
        ),
        own_git_root: subproject_own_git_root(project, &subproject),
    }]
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
/// The table column order varies across renderers (`wave_scaffold` emits
/// `Spec`; legacy plans add extra columns; the fixture form drops `Spec`).
/// Rather than index by position we read the header row to find which data cell
/// holds the wave number, the role, and the dependency list. Rows whose first
/// cell parses as a wave number drive the result; the `depends_on` cell is
/// parsed via the shared `[[…]]` scanner and normalised to wave numbers.
fn parse_wave_plan_table(text: &str) -> Vec<WaveRow> {
    let mut header: Option<Vec<String>> = None;
    // Phase 1: collect `(wave, role, raw deps cell)`. A dependency cell can name
    // a wave by role (`[[backend]]`), and resolving that needs the full
    // role→wave map — which is not known until every row has been read.
    let mut raw: Vec<(u32, String, String)> = Vec::new();

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
        let deps_cell = cols
            .depends_on
            .and_then(|i| cells.get(i))
            .cloned()
            .unwrap_or_default();
        raw.push((wave, role, deps_cell));
    }
    raw.sort_by_key(|r| r.0);
    raw.dedup_by_key(|r| r.0);

    // Map each role to the wave that introduces it (lowest wave wins for a
    // repeated role), so a role-named dependency cell resolves to a wave number.
    let mut role_to_wave: BTreeMap<String, u32> = BTreeMap::new();
    for (wave, role, _) in &raw {
        role_to_wave.entry(role.clone()).or_insert(*wave);
    }

    // Phase 2: resolve each deps cell now that the role→wave map is complete.
    raw.into_iter()
        .map(|(wave, role, cell)| WaveRow {
            wave,
            depends_on: parse_depends_cell(&cell, wave, &role_to_wave),
            role,
        })
        .collect()
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
            // EN "role" / PT "papel" — without the PT alias a pt-BR wave-plan
            // (header `Papel`) drops every role to the `mixed` fallback, which
            // also breaks role-named dependency resolution below.
            if role.is_none() && (h == "role" || h == "papel") {
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
fn parse_depends_cell(cell: &str, self_wave: u32, role_to_wave: &BTreeMap<String, u32>) -> Vec<u32> {
    let trimmed = cell.trim();
    if trimmed.is_empty() || trimmed == "—" || trimmed == "-" {
        return Vec::new();
    }
    let mut out: Vec<u32> = Vec::new();
    for link in find_outgoing_links(trimmed) {
        if let Some(n) = wave_number_from_link(&link, role_to_wave) {
            if n != self_wave && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out.sort_unstable();
    out
}

/// Resolve a dependency `[[…]]` token to a wave number. `[[1]]` → 1;
/// `[[wave-1-general]]` → 1 (hyphen dir form); `[[wave.<slug>.<N>-<role>]]` → N
/// (the DOTTED wikilink `wave-scaffold` writes, e.g.
/// `[[wave.field-report-fix-package-sialia.2-agents]]`); a bare role token
/// `[[backend]]` → the wave that carries that role (via `role_to_wave`);
/// anything else → `None`.
///
/// The role fallback exists because the Plan agent sometimes authors deps as
/// bare role names instead of the `wave-N-role` dir form `plan-from-spec`
/// emits. Without it, `find_outgoing_links` yields `backend`, this returns
/// `None`, the edge is silently dropped, and the whole DAG flattens to level 0
/// (every wave "dispatch-parallel") — losing real ordering AND the genuine
/// parallelism between truly independent waves. The dotted branch exists for the
/// same failure: `wave-scaffold` writes `wave.<slug>.<N>-<role>`, on which
/// `strip_prefix("wave-")` fails (the head is `wave.`, not `wave-`), so every
/// dotted-dep DAG likewise flattened to level 0 — proven live 2026-07-18 on the
/// field-report-fix-package-sialia pipeline (every wave dispatched in one round).
fn wave_number_from_link(link: &str, role_to_wave: &BTreeMap<String, u32>) -> Option<u32> {
    let inner = link.trim();
    if let Ok(n) = inner.parse::<u32>() {
        return Some(n);
    }
    if let Some(rest) = inner.strip_prefix("wave-") {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = digits.parse::<u32>() {
            return Some(n);
        }
    }
    // Dotted form `wave.<slug>.<N>-<role>`. Split on '.', then read the wave
    // number from the LAST segment (`<N>-<role>`); fall back to the segment
    // right after the `wave` head when it is itself numeric (`wave.<N>` /
    // `wave.<N>.<role>`). Last-segment-first so a slug with leading digits in a
    // middle segment (`wave.2024-redesign.3-agents`) cannot hijack the number.
    if let Some(rest) = inner.strip_prefix("wave.") {
        let segs: Vec<&str> = rest.split('.').collect();
        if let Some(n) = segs.last().and_then(|s| leading_wave_number(s)) {
            return Some(n);
        }
        if let Some(n) = segs.first().and_then(|s| leading_wave_number(s)) {
            return Some(n);
        }
    }
    role_to_wave.get(inner).copied()
}

/// Read the leading wave number from a `<N>-<role>` (or bare `<N>`) segment: the
/// leading ASCII digits, but only when they stand alone or are immediately
/// followed by `-` — so a real `2-agents` label matches while an arbitrary
/// alphanumeric slug segment (`2fa-login`) does NOT masquerade as wave 2.
fn leading_wave_number(seg: &str) -> Option<u32> {
    let digits: String = seg.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    // `digits` are leading ASCII (1 byte each), so this is a valid char boundary.
    let rest = &seg[digits.len()..];
    if rest.is_empty() || rest.starts_with('-') {
        digits.parse::<u32>().ok()
    } else {
        None
    }
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
    files_to_subproject(project, &files)
}

/// Convert a parsed `## Files` list into the project-relative subproject
/// string (`apps/<name>` / `packages/<name>`), or `"."` when the files are
/// empty or do not converge on a single subproject (fail-open). Shared by the
/// per-wave derivation and the single-spec fallback.
fn files_to_subproject(project: &Path, files: &[String]) -> String {
    if files.is_empty() {
        return ".".to_string();
    }
    match detect_subproject(files, project) {
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
    fn parse_table_with_extra_column() {
        // Legacy form carrying an extra column — column roles resolve from the
        // header row, so the extra column is tolerated, not mis-parsed.
        let plan = "\
| Wave | Spec | Role | Notes | Depende de | Resumo |
|------|------|------|-------|------------|--------|
| 1 | [[wave-1-general]] | general | n/a | — | foo |
| 2 | [[wave-2-ui]] | ui | n/a | [[1]] | bar |
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
    fn wave_number_from_link_handles_number_wave_and_role_forms() {
        let roles: BTreeMap<String, u32> =
            [("backend".to_string(), 1u32), ("core".to_string(), 2u32)]
                .into_iter()
                .collect();
        assert_eq!(wave_number_from_link("1", &roles), Some(1));
        assert_eq!(wave_number_from_link("wave-2-general", &roles), Some(2));
        assert_eq!(wave_number_from_link("wave-12-rt", &roles), Some(12));
        // Bare role token resolves via the role→wave map.
        assert_eq!(wave_number_from_link("backend", &roles), Some(1));
        assert_eq!(wave_number_from_link("core", &roles), Some(2));
        // Unknown token (not a number, not wave-N, not a known role) → None.
        assert_eq!(wave_number_from_link("memory/foo", &roles), None);
    }

    /// FINDING #7 (live regression 2026-07-18): `wave-scaffold` writes the DOTTED
    /// dependency wikilink `[[wave.<slug>.<N>-<role>]]`, on which the `wave-`
    /// hyphen branch fails — the edge dropped and the whole DAG flattened to
    /// level 0 (every wave dispatched in one parallel round). The dotted form
    /// must resolve to its `<N>`, and a full table parse over a dotted
    /// Depends-on cell must yield non-flat levels (the dependent wave ≥ 1).
    #[test]
    fn dotted_wikilink_resolves_wave() {
        let roles: BTreeMap<String, u32> = BTreeMap::new();
        // Direct: the dotted token resolves to its `<N>` without any role map.
        assert_eq!(wave_number_from_link("wave.some-slug.2-agents", &roles), Some(2));
        // A slug with leading digits in a MIDDLE segment must not hijack the
        // number — the trailing `<N>-<role>` segment wins (last-segment-first).
        assert_eq!(wave_number_from_link("wave.2024-redesign.3-agents", &roles), Some(3));
        // The `wave.<N>` / `wave.<N>.<role>` shape resolves off the head segment.
        assert_eq!(wave_number_from_link("wave.5.core", &roles), Some(5));

        // Full table parse: a dotted Depends-on cell reconstructs the edge so the
        // dependent wave lands at a non-flat level (>= 1) — not the flattened 0.
        let plan = "\
| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.demo.1-rt]] | rt | — | base |
| 2 | [[wave.demo.2-agents]] | agents | [[wave.demo.1-rt]] | uses base |
";
        let rows = parse_wave_plan_table(plan);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].depends_on.is_empty(), "wave 1 independent");
        assert_eq!(rows[1].depends_on, vec![1], "wave 2 depends on wave 1 via the dotted link");
        let levels = assign_levels(&rows);
        assert_eq!(levels[&1], 0, "wave 1 at level 0");
        assert!(levels[&2] >= 1, "dependent wave must not be flattened to level 0");
    }

    /// Regression for the sialia wave-plan: the Plan agent authored deps as bare
    /// role names (`[[backend]]`/`[[core]]`) instead of the `wave-N-role` form.
    /// They must resolve to wave numbers so the level DAG keeps its real depth —
    /// otherwise every wave flattens to level 0 (all dispatch-parallel).
    #[test]
    fn parse_table_role_named_deps_resolve_to_waves() {
        let plan = "\
| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-backend]] | backend | — | base |
| 2 | [[wave-2-core]] | core | [[backend]] | uses backend |
| 3 | [[wave-3-app-form]] | app-form | [[core]] | uses core |
| 4 | [[wave-4-app-table]] | app-table | — | independent |
";
        let rows = parse_wave_plan_table(plan);
        assert_eq!(rows.len(), 4);
        assert!(rows[0].depends_on.is_empty());
        assert_eq!(rows[1].depends_on, vec![1]); // core ← backend (wave 1)
        assert_eq!(rows[2].depends_on, vec![2]); // app-form ← core (wave 2)
        assert!(rows[3].depends_on.is_empty()); // app-table independent
        // The reconstructed DAG: waves 1 and 4 share level 0 (parallel round 1),
        // wave 2 is level 1, wave 3 is level 2 — exactly the sialia plan's intent.
        let levels = assign_levels(&rows);
        assert_eq!(levels[&1], 0);
        assert_eq!(levels[&4], 0);
        assert_eq!(levels[&2], 1);
        assert_eq!(levels[&3], 2);
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
        // Ref emit: the stub keeps the full prompt out of the orchestrator's
        // context (paid once in the dispatch instead of twice).
        assert!(cmd.contains("--emit ref"));
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

    /// Non-wave spec with a `spec.md` (TF-like / Light scope) → exactly one
    /// `impl` item: wave 0, no deps, subproject inferred from `## Files`, and
    /// a `prompt_cmd` WITHOUT `--wave`.
    #[test]
    fn dispatch_single_spec_tf_like_emits_one_item() {
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
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Flat\n\n## Files\n- apps/rt/src/foo.rs\n- apps/rt/src/bar.rs\n",
        )
        .unwrap();
        let items = build_plan(project, &spec_dir, "flat", None);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.wave, 0, "wave 0 marks the wave-less single spec");
        assert_eq!(item.role, "impl");
        assert_eq!(item.subproject, "apps/rt");
        assert!(item.depends_on.is_empty());
        assert_eq!(item.level, 0);
        assert_eq!(item.subagent_type, "general-purpose");
        assert!(item.prompt_cmd.starts_with("mustard-rt run agent-prompt-render "));
        assert!(item.prompt_cmd.contains("--spec flat"));
        assert!(item.prompt_cmd.contains("--role impl"));
        assert!(item.prompt_cmd.contains("--subproject apps/rt"));
        assert!(item.prompt_cmd.contains("--mode first"));
        assert!(
            !item.prompt_cmd.contains("--wave"),
            "single-spec render must not carry --wave: {}",
            item.prompt_cmd
        );
    }

    /// Single-spec fallback with no `## Files` convergence → subproject `"."`.
    #[test]
    fn dispatch_single_spec_no_files_falls_back_to_dot() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("dotty")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Dotty\n## Tasks\n- x\n").unwrap();
        let items = build_plan(project, &spec_dir, "dotty", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].subproject, ".");
        assert!(items[0].prompt_cmd.contains("--subproject ."));
    }

    /// The wave-plan path is untouched by the single-spec fallback: a spec
    /// WITH a `wave-plan.md` keeps emitting the same multi-item plan (1-based
    /// waves, deps, per-wave `--wave` flags).
    #[test]
    fn dispatch_single_spec_wave_plan_path_unchanged() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("waved")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        // A spec.md ALSO exists — the wave plan must still win.
        std::fs::write(spec_dir.join("spec.md"), "# Waved\n## Files\n- apps/rt/x.rs\n").unwrap();
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
        let items = build_plan(project, &spec_dir, "waved", None);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].wave, 1);
        assert!(items[0].prompt_cmd.contains("--wave 1"));
        assert_eq!(items[1].wave, 2);
        assert_eq!(items[1].depends_on, vec![1]);
        assert!(items[1].prompt_cmd.contains("--wave 2"));
    }

    /// Unknown spec (no dir / no `spec.md`) keeps degrading to `[]` — the
    /// single-spec fallback never invents an item for a spec that does not
    /// exist on disk.
    #[test]
    fn dispatch_single_spec_missing_spec_degrades_empty() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("ghost")
            .unwrap()
            .dir()
            .to_path_buf();
        // Dir absent entirely.
        let items = build_plan(project, &spec_dir, "ghost", None);
        assert!(items.is_empty());
        // Dir present but no spec.md → still empty.
        std::fs::create_dir_all(&spec_dir).unwrap();
        let items = build_plan(project, &spec_dir, "ghost", None);
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

    /// AC-5: the git-boundary fact reaches the dispatch item. A subproject whose
    /// own dir is a nested git root (`.git` FILE — the submodule shape) makes the
    /// item carry `own_git_root: true`; a plain subproject stays false.
    #[test]
    fn dispatch_item_carries_own_git_root_for_nested_repo() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let project = dir.path();

        // A nested submodule: `.git` as a FILE (gitdir pointer) at apps/sub.
        let sub = project.join("apps").join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), b"gitdir: ../../.git/modules/sub\n").unwrap();

        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("bnd")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# B\n\n## Files\n- apps/sub/src/x.rs\n").unwrap();

        let items = build_plan(project, &spec_dir, "bnd", None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].subproject, "apps/sub");
        assert!(items[0].own_git_root, "nested `.git` subproject carries the boundary flag");

        // A plain subproject (no `.git`) does not carry the flag.
        let spec_dir2 = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("plain")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir2).unwrap();
        std::fs::write(spec_dir2.join("spec.md"), "# P\n\n## Files\n- apps/rt/src/y.rs\n").unwrap();
        let items2 = build_plan(project, &spec_dir2, "plain", None);
        assert_eq!(items2.len(), 1);
        assert_eq!(items2[0].subproject, "apps/rt");
        assert!(!items2[0].own_git_root, "a plain subproject has no boundary flag");
    }
}
