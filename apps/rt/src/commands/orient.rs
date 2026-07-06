//! `run orient` — the orientation census (the two-level terrain map).
//!
//! ## Why this exists
//!
//! The `/scan` already mines the whole repo into `.claude/grain.model.json`:
//! the subprojects, the kind of each, and — the valuable part — the exemplar
//! files by role (`role` + `start_line`). Today that map is reduced to three
//! counters and never handed back to the AI, so every request cold-starts with
//! `grep`. This module projects the grain model back into the agent's window as
//! a deterministic, byte-stable census, in two levels:
//!
//! - **Level 1 — Terrain** (`session_start_inject`, once per session): one line
//!   per subproject (`name · kind · Nf — role`). The kind + file count reuse the
//!   SAME `Project.kind` / `Project.code_files` the subproject-`CLAUDE.md` footer
//!   (`scan_claude`) renders. The role (`papel`) is grain's own architectural
//!   layer from `skeleton[]` (`L0`/`L1`/`L2`), joined by dir — which also filters
//!   the list to the real architectural units (test fixtures and nested inner
//!   crates are excluded, keeping the terrain ~6 lines).
//! - **Level 2 — Entrypoints** (`prompt_submit_inject`, per request, gated):
//!   given an `intent`, the subproject(s) matched by lexical overlap
//!   (intent tokens × the project's name/dir/domain-entities), each with its
//!   exemplar files by role as `path:line` (NEVER the snippet). Hard cap: ≤2
//!   subprojects × ≤4 roles.
//!
//! Fail-open throughout: a missing / unreadable / unparseable grain model yields
//! an empty [`Orientation`] (no terrain, no entrypoints), so every consumer
//! degrades to nothing rather than erroring. Output is deterministic and
//! byte-stable — repo-relative paths pass through verbatim, everything is
//! sorted, no timestamps leak.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use mustard_core::domain::scan::Project;

/// Hard cap on the number of subprojects Level 2 surfaces for one intent.
const MAX_ENTRY_PROJECTS: usize = 2;
/// Hard cap on the number of roles (exemplar `path:line` lines) per subproject.
const MAX_ENTRY_ROLES: usize = 4;

// ===========================================================================
// Grain model — the minimal read-only view this module deserializes.
// ===========================================================================

/// The slice of `grain.model.json` the census reads. Only the fields consumed
/// are declared; every extra field grain writes is ignored (lenient serde), so
/// the scan tool stays the single owner of the model format. No `snippet` field
/// is declared anywhere in this view, so the census can never leak a code body.
#[derive(Debug, Default, Deserialize)]
struct RawModel {
    #[serde(default)]
    projects: Vec<Project>,
    #[serde(default)]
    skeleton: Vec<SkeletonEntry>,
    #[serde(default)]
    modules: Vec<Module>,
}

/// One `modules[]` row — a source file in the repo inventory with its declared
/// symbols. Level 2 ranks concept-relevant entrypoints over this full file
/// inventory. No `snippet` field exists on a module, so a code body can never
/// leak.
#[derive(Debug, Deserialize)]
struct Module {
    #[serde(default)]
    path: String,
    #[serde(default)]
    declarations: Vec<Decl>,
}

/// One `modules[].declarations[]` row — a declared symbol and its line. Used to
/// pick a file's primary entrypoint (its earliest declaration: line + name).
#[derive(Debug, Deserialize)]
struct Decl {
    #[serde(default)]
    name: String,
    #[serde(default)]
    line: usize,
}

/// One `skeleton[]` row — grain's per-dir architectural layer (`L0`/`L1`/`L2`).
#[derive(Debug, Deserialize)]
struct SkeletonEntry {
    #[serde(default)]
    dir: String,
    #[serde(default)]
    role: String,
}

// ===========================================================================
// Orientation — the computed, render-ready projection.
// ===========================================================================

/// The two-level orientation census projected from the grain model.
#[derive(Debug, Default)]
pub struct Orientation {
    /// The request that produced the entrypoints (echoed into their header).
    intent: Option<String>,
    /// Level 1 — one row per architectural subproject, sorted deterministically.
    terrain: Vec<TerrainRow>,
    /// Level 2 — matched subproject(s) with their exemplar `path:line` by role.
    entrypoints: Vec<EntryGroup>,
}

/// One Level-1 line: `name · kind · Nf — role`.
#[derive(Debug)]
struct TerrainRow {
    name: String,
    kind: String,
    code_files: usize,
    /// Architectural layer from `skeleton[]` (`L0`/`L1`/`L2`); empty when the
    /// model carries no skeleton (older scan) and every project is kept.
    role: String,
}

/// One Level-2 group: a matched subproject and its exemplar entrypoints.
#[derive(Debug)]
struct EntryGroup {
    dir: String,
    entries: Vec<Entry>,
}

/// One Level-2 line: `path:line — role` (no snippet, ever).
#[derive(Debug)]
struct Entry {
    path: String,
    line: usize,
    role: String,
}

// ===========================================================================
// Core projection.
// ===========================================================================

/// Project `<root>/.claude/grain.model.json` into an [`Orientation`].
///
/// With `intent = None` only the terrain (Level 1) is computed. With an intent
/// the entrypoints (Level 2) are matched too. Fail-open: a missing / unreadable
/// / unparseable model returns [`Orientation::default`] (empty). Deterministic:
/// same model + same intent ⇒ byte-identical projection.
#[must_use]
pub fn compute_orientation(root: &Path, intent: Option<&str>) -> Orientation {
    let model_path = root.join(".claude").join("grain.model.json");
    let Ok(text) = std::fs::read_to_string(&model_path) else {
        return Orientation::default();
    };
    let Ok(model) = serde_json::from_str::<RawModel>(&text) else {
        return Orientation::default();
    };

    // --- Level 1: the architectural subprojects (skeleton-filtered) ---------
    let skeleton: HashMap<&str, &str> =
        model.skeleton.iter().map(|s| (s.dir.as_str(), s.role.as_str())).collect();
    let skeleton_empty = skeleton.is_empty();

    // Keep the projects grain lists as real architectural units: those present
    // in `skeleton[]` (joined by dir; the root's empty dir maps to `(root)`).
    // When the model predates the skeleton field, keep every project (role empty).
    let mut kept: Vec<(&Project, String)> = Vec::new();
    for p in &model.projects {
        let key = if p.dir.is_empty() { "(root)" } else { p.dir.as_str() };
        match skeleton.get(key) {
            Some(role) => kept.push((p, (*role).to_string())),
            None if skeleton_empty => kept.push((p, String::new())),
            None => {}
        }
    }

    let mut terrain: Vec<TerrainRow> = kept
        .iter()
        .map(|(p, role)| TerrainRow {
            name: p.name.clone(),
            kind: p.kind.clone(),
            code_files: p.code_files,
            role: role.clone(),
        })
        .collect();
    // Byte-stable order: by layer (L0→L1→L2), then biggest first, then name.
    terrain.sort_by(|a, b| {
        a.role
            .cmp(&b.role)
            .then(b.code_files.cmp(&a.code_files))
            .then(a.name.cmp(&b.name))
    });

    // --- Level 2: concept-relevant entrypoints (repo-wide, grouped) ---------
    let entrypoints = match intent {
        Some(text) if !text.trim().is_empty() => {
            match_entrypoints(text, &kept, &model.modules)
        }
        _ => Vec::new(),
    };

    Orientation {
        intent: intent.map(str::to_string),
        terrain,
        entrypoints,
    }
}

/// Rank the whole module inventory by the intent's *concept* tokens and group
/// the top hits by owning subproject. Repo-wide by design: a request names a
/// concept (`close gate`, `scope classify`), not usually a subproject, so the
/// concept — not a subproject-name match — drives the pick. Steps:
///
/// 1. `concept` = intent tokens minus every subproject's name/dir tokens (they
///    say WHERE, not WHAT; a bare `rt` should not rank files). Empty ⇒ nothing.
/// 2. Score each non-test module by the SUMMED corpus rarity (`N / df`) of the
///    concept tokens in its PATH. A rare token (`inject`) outweighs a common one
///    (`src`, or a subproject name that survived), so glue never wins — a
///    corpus-derived stopword suppression, never a hand-curated list.
/// 3. Rank (score desc, path asc), group by owning subproject, cap at
///    [`MAX_ENTRY_PROJECTS`] groups × [`MAX_ENTRY_ROLES`] files. No fallback:
///    zero concept matches ⇒ no entrypoints (the terrain + digest cover it),
///    never a pretend-relevant structural dump.
fn match_entrypoints(
    intent: &str,
    kept: &[(&Project, String)],
    modules: &[Module],
) -> Vec<EntryGroup> {
    let intent_tokens: HashSet<String> = tokenize(intent).into_iter().collect();
    let subproject_tokens: HashSet<String> =
        kept.iter().flat_map(|(p, _)| project_lexicon(p)).collect();
    let concept: HashSet<String> =
        intent_tokens.difference(&subproject_tokens).cloned().collect();
    if concept.is_empty() {
        return Vec::new();
    }

    // Non-test modules, keyed by PATH tokens — a file's path is its precise
    // signature (`*_inject.rs`, `telemetry.rs`, `scope_classify.rs`). Declaration
    // names are deliberately NOT folded in: big-declaration files (`main.rs`,
    // dispatchers) match concept tokens incidentally and drown the real target;
    // a concept in a symbol but not the path (or a PT→EN vocabulary gap) is the
    // digest's job, which the template routes to.
    let pool: Vec<(&Module, HashSet<String>)> = modules
        .iter()
        .filter(|m| !mustard_core::domain::ast::is_test_path(&m.path))
        .map(|m| (m, tokenize(&m.path).into_iter().collect()))
        .collect();
    let n = pool.len();
    if n == 0 {
        return Vec::new();
    }

    // Document frequency of each concept token across the corpus.
    let mut df: HashMap<&String, usize> = HashMap::new();
    for (_, tokens) in &pool {
        for t in &concept {
            if tokens.contains(t) {
                *df.entry(t).or_insert(0) += 1;
            }
        }
    }

    // Score, keep positives, rank deterministically.
    let mut ranked: Vec<(usize, &Module)> = pool
        .iter()
        .filter_map(|(m, tokens)| {
            let score: usize = concept
                .iter()
                .filter(|t| tokens.contains(*t))
                .map(|t| n / df.get(t).copied().unwrap_or(1).max(1))
                .sum();
            (score > 0).then_some((score, *m))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));

    // Group the ranked files by owning subproject, in rank order, honouring the
    // per-group and group-count caps.
    let mut groups: Vec<EntryGroup> = Vec::new();
    for (_, m) in ranked {
        let Some(dir) = owning_dir(&m.path, kept) else {
            continue;
        };
        if let Some(g) = groups.iter_mut().find(|g| g.dir == dir) {
            if g.entries.len() < MAX_ENTRY_ROLES {
                g.entries.push(entry_of(m));
            }
        } else if groups.len() < MAX_ENTRY_PROJECTS {
            groups.push(EntryGroup { dir, entries: vec![entry_of(m)] });
        }
        if groups.len() == MAX_ENTRY_PROJECTS
            && groups.iter().all(|g| g.entries.len() >= MAX_ENTRY_ROLES)
        {
            break;
        }
    }
    groups
}

/// The dir of the kept subproject that owns `path` — the longest project dir
/// that is a path-prefix of it. `None` for a repo-root file (no kept subproject
/// contains it).
fn owning_dir(path: &str, kept: &[(&Project, String)]) -> Option<String> {
    kept.iter()
        .map(|(p, _)| &p.dir)
        .filter(|dir| {
            !dir.is_empty() && (path == dir.as_str() || path.starts_with(&format!("{dir}/")))
        })
        .max_by_key(|dir| dir.len())
        .cloned()
}

/// Build a Level-2 [`Entry`] from a module: its primary entrypoint is the file's
/// earliest declaration (its line + symbol name); a declaration-less module
/// degrades to line 1 with no hint.
fn entry_of(m: &Module) -> Entry {
    let primary = m.declarations.iter().min_by_key(|d| d.line);
    Entry {
        path: m.path.clone(),
        line: primary.map_or(1, |d| d.line.max(1)),
        role: primary.map(|d| d.name.clone()).unwrap_or_default(),
    }
}

/// A subproject's identifying tokens: its name tokens and its dir-leaf tokens.
/// These say WHERE work lands, not WHAT it is, so [`match_entrypoints`] subtracts
/// the union of them from the intent before ranking — a bare `rt` or `dashboard`
/// must not rank files, only concept words should.
fn project_lexicon(p: &Project) -> HashSet<String> {
    let mut lex: HashSet<String> = HashSet::new();
    lex.extend(tokenize(&p.name));
    if let Some(leaf) = p.dir.rsplit('/').next() {
        lex.extend(tokenize(leaf));
    }
    lex
}

/// Split a string into lowercased alphanumeric tokens of length ≥2. Grammatical
/// connectors (`no`, `de`, …) survive tokenisation but never collide with the
/// code identifiers a project lexicon is built from, so no stoplist is needed.
fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(str::to_ascii_lowercase)
        .collect()
}

// ===========================================================================
// Rendering — byte-stable text for both the command and the injects.
// ===========================================================================

/// Header line for the Level-1 terrain block.
const TERRAIN_HEADER: &str =
    "[Terreno] subprojetos mapeados pelo /scan — leia daqui, não grepe para se orientar:";

/// Render the Level-1 terrain block (header + one line per subproject), or
/// `None` when there is no terrain (fail-open / empty model).
#[must_use]
pub fn render_terrain(o: &Orientation) -> Option<String> {
    if o.terrain.is_empty() {
        return None;
    }
    let mut out = String::from(TERRAIN_HEADER);
    for row in &o.terrain {
        out.push_str("\n- ");
        out.push_str(&row.name);
        out.push_str(" · ");
        out.push_str(&row.kind);
        out.push_str(&format!(" · {} arquivos", row.code_files));
        if !row.role.is_empty() {
            out.push_str(" — ");
            out.push_str(&row.role);
        }
    }
    Some(out)
}

/// Render the Level-2 entrypoints block (header echoing the intent + one
/// `path:line — role` per exemplar, grouped by subproject), or `None` when no
/// subproject matched the intent.
#[must_use]
pub fn render_entrypoints(o: &Orientation) -> Option<String> {
    if o.entrypoints.is_empty() {
        return None;
    }
    let intent = o.intent.as_deref().unwrap_or_default();
    let mut out = format!(
        "[Pontos de entrada] para \"{intent}\" — arquivos-exemplar relevantes (leia estes, não redescubra):"
    );
    for group in &o.entrypoints {
        out.push('\n');
        out.push_str(&group.dir);
        out.push(':');
        for e in &group.entries {
            out.push_str(&format!("\n- {}:{} — {}", e.path, e.line, e.role));
        }
    }
    Some(out)
}

// ===========================================================================
// Command entry — `mustard-rt run orient [--intent <str>]`.
// ===========================================================================

/// `run orient`: print the terrain, plus the intent-matched entrypoints when an
/// `--intent` is supplied. Empty (missing grain / no match) prints nothing and
/// exits 0 — fail-open, byte-stable.
pub fn run(intent: Option<&str>, root: &Path) {
    let orientation = compute_orientation(root, intent);
    let mut blocks: Vec<String> = Vec::new();
    if let Some(t) = render_terrain(&orientation) {
        blocks.push(t);
    }
    if let Some(e) = render_entrypoints(&orientation) {
        blocks.push(e);
    }
    if !blocks.is_empty() {
        println!("{}", blocks.join("\n\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// A compact, fully-controlled grain model: two architectural subprojects
    /// (one L1 `rt`, one L0 `web`) plus a nested unit and a fixture the skeleton
    /// join must drop. `modules[]` is the concept-ranking inventory: five
    /// `*_inject.rs` under `apps/rt`, one under `apps/web`, two unrelated `rt`
    /// files (no concept match), and one test file (`is_test_path` must exclude).
    const FIXTURE: &str = r#"{
      "projects": [
        {"name": "rt", "dir": "apps/rt", "kind": "cargo", "code_files": 232},
        {"name": "web", "dir": "apps/web", "kind": "npm", "code_files": 40},
        {"name": "inner", "dir": "apps/rt/inner", "kind": "cargo", "code_files": 5},
        {"name": "fixture", "dir": "apps/scan/tests/fixtures/x", "kind": "go", "code_files": 2},
        {"name": "(root)", "dir": "", "kind": "npm", "code_files": 12}
      ],
      "skeleton": [
        {"dir": "apps/rt", "role": "L1"},
        {"dir": "apps/web", "role": "L0"},
        {"dir": "(root)", "role": "L2"}
      ],
      "modules": [
        {"path": "apps/rt/src/commands/agent/context_inject.rs", "declarations": [{"name": "ContextInject", "line": 8}]},
        {"path": "apps/rt/src/hooks/observe/amend_window_inject.rs", "declarations": [{"name": "AmendInject", "line": 12}]},
        {"path": "apps/rt/src/hooks/session/prompt_submit_inject.rs", "declarations": [{"name": "PromptSubmitInject", "line": 45}, {"name": "run", "line": 60}]},
        {"path": "apps/rt/src/hooks/session/session_start_inject.rs", "declarations": [{"name": "SessionStartInject", "line": 82}]},
        {"path": "apps/rt/src/hooks/task/subagent_inject.rs", "declarations": [{"name": "SubagentInject", "line": 20}]},
        {"path": "apps/rt/src/commands/scan_claude.rs", "declarations": [{"name": "ScanClaude", "line": 10}]},
        {"path": "apps/rt/src/main.rs", "declarations": [{"name": "main", "line": 1}]},
        {"path": "apps/web/src/inject_widget.tsx", "declarations": [{"name": "InjectWidget", "line": 3}]},
        {"path": "apps/rt/tests/hook_it.rs", "declarations": [{"name": "it_works", "line": 1}]}
      ]
    }"#;

    /// Write [`FIXTURE`] into `<root>/.claude/grain.model.json` and return the
    /// tempdir + its path.
    fn seed(model: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("grain.model.json"), model).unwrap();
        let root = dir.path().to_path_buf();
        (dir, root)
    }

    #[test]
    fn terrain_lists_one_line_per_architectural_subproject() {
        let (_d, root) = seed(FIXTURE);
        let o = compute_orientation(&root, None);
        // The nested inner crate and the test fixture are dropped by the
        // skeleton join; rt / web / (root) survive.
        let rendered = render_terrain(&o).expect("terrain");
        insta::assert_snapshot!(rendered, @r###"
        [Terreno] subprojetos mapeados pelo /scan — leia daqui, não grepe para se orientar:
        - web · npm · 40 arquivos — L0
        - rt · cargo · 232 arquivos — L1
        - (root) · npm · 12 arquivos — L2
        "###);
    }

    #[test]
    fn entrypoints_rank_concept_across_subprojects_capped_and_test_free() {
        let (_d, root) = seed(FIXTURE);
        let o = compute_orientation(&root, Some("adicionar hook de inject no rt"));
        let rendered = render_entrypoints(&o).expect("entrypoints");
        // The concept token "inject" (subproject names `rt`/`web` subtracted, glue
        // `de`/`no` matching no path) selects every `*_inject.rs` across BOTH
        // subprojects. The unrelated rt files (scan_claude / main) are dropped; the
        // test file (`hook_it.rs`) is excluded by is_test_path; the 5th rt inject
        // (subagent) is dropped by the ≤4-per-group cap. No snippet can appear.
        assert!(!rendered.contains("scan_claude"));
        assert!(!rendered.contains("main.rs"));
        assert!(!rendered.contains("hook_it"));
        assert!(!rendered.contains("subagent"));
        insta::assert_snapshot!(rendered, @r###"
        [Pontos de entrada] para "adicionar hook de inject no rt" — arquivos-exemplar relevantes (leia estes, não redescubra):
        apps/rt:
        - apps/rt/src/commands/agent/context_inject.rs:8 — ContextInject
        - apps/rt/src/hooks/observe/amend_window_inject.rs:12 — AmendInject
        - apps/rt/src/hooks/session/prompt_submit_inject.rs:45 — PromptSubmitInject
        - apps/rt/src/hooks/session/session_start_inject.rs:82 — SessionStartInject
        apps/web:
        - apps/web/src/inject_widget.tsx:3 — InjectWidget
        "###);
    }

    #[test]
    fn no_concept_match_yields_no_entrypoints() {
        let (_d, root) = seed(FIXTURE);
        // "rt" is only a subproject name (subtracted) and "revisar" maps to no
        // path, so Level 2 stays silent — the terrain + digest cover it, never a
        // pretend-relevant structural dump.
        let o = compute_orientation(&root, Some("revisar o rt"));
        assert!(render_entrypoints(&o).is_none());
    }

    #[test]
    fn no_intent_yields_no_entrypoints() {
        let (_d, root) = seed(FIXTURE);
        let o = compute_orientation(&root, None);
        assert!(render_entrypoints(&o).is_none());
    }

    #[test]
    fn orient_fail_open() {
        // No grain model on disk → empty projection, nothing rendered. The
        // command prints nothing and exits 0 (parity with the injects).
        let dir = tempdir().unwrap();
        let o = compute_orientation(dir.path(), Some("anything"));
        assert!(render_terrain(&o).is_none());
        assert!(render_entrypoints(&o).is_none());
        // An unparseable model degrades the same way.
        let (_d, root) = seed("{ not json");
        let o2 = compute_orientation(&root, Some("rt"));
        assert!(render_terrain(&o2).is_none());
    }

    #[test]
    fn entrypoints_respect_caps() {
        // "inject" matches six modules across rt (5) and web (1); caps hold at
        // ≤2 groups and ≤4 files each.
        let (_d, root) = seed(FIXTURE);
        let o = compute_orientation(&root, Some("inject"));
        assert_eq!(o.entrypoints.len(), MAX_ENTRY_PROJECTS);
        for g in &o.entrypoints {
            assert!(g.entries.len() <= MAX_ENTRY_ROLES);
        }
    }
}
