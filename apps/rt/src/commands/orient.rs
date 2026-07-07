//! `run orient` — the orientation census (the terrain map).
//!
//! ## Why this exists
//!
//! The `/scan` already mines the whole repo into `.claude/grain.model.json`:
//! the subprojects, the kind of each, and the architectural layer of each.
//! Without this projection that map is reduced to three counters and never
//! handed back to the AI, so every request cold-starts with `grep`. This
//! module projects the grain model back into the agent's window as a
//! deterministic, byte-stable census:
//!
//! - **Terrain** (`session_start_inject`, once per session): one line per
//!   subproject (`name · kind · Nf — role`). The kind + file count reuse the
//!   SAME `Project.kind` / `Project.code_files` the subproject-`CLAUDE.md`
//!   footer (`scan_claude`) renders. The role (`papel`) is grain's own
//!   architectural layer from `skeleton[]` (`L0`/`L1`/`L2`), joined by dir —
//!   which also filters the list to the real architectural units (test
//!   fixtures and nested inner crates are excluded, keeping the terrain
//!   ~6 lines).
//!
//! A per-prompt "Level 2" (entrypoint suggestions matched lexically against
//! the user's prompt) used to live here. It was REMOVED after two field
//! sessions measured 1 useful suggestion in 17: prompt words are problem
//! vocabulary, path tokens are code vocabulary, and they only overlap by
//! coincidence. Locating code is on-demand work — `grep` for known literals,
//! the digest (`run feature`) for concepts — never a per-prompt guess.
//!
//! Fail-open throughout: a missing / unreadable / unparseable grain model
//! yields an empty [`Orientation`] (no terrain), so every consumer degrades
//! to nothing rather than erroring. Output is deterministic and byte-stable —
//! everything is sorted, no timestamps leak.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use mustard_core::domain::scan::Project;

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

/// The orientation census projected from the grain model.
#[derive(Debug, Default)]
pub struct Orientation {
    /// One row per architectural subproject, sorted deterministically.
    terrain: Vec<TerrainRow>,
}

/// One terrain line: `name · kind · Nf — role`.
#[derive(Debug)]
struct TerrainRow {
    name: String,
    kind: String,
    code_files: usize,
    /// Architectural layer from `skeleton[]` (`L0`/`L1`/`L2`); empty when the
    /// model carries no skeleton (older scan) and every project is kept.
    role: String,
}

// ===========================================================================
// Core projection.
// ===========================================================================

/// Project `<root>/.claude/grain.model.json` into an [`Orientation`].
///
/// Fail-open: a missing / unreadable / unparseable model returns
/// [`Orientation::default`] (empty). Deterministic: same model ⇒
/// byte-identical projection.
#[must_use]
pub fn compute_orientation(root: &Path) -> Orientation {
    let model_path = root.join(".claude").join("grain.model.json");
    let Ok(text) = std::fs::read_to_string(&model_path) else {
        return Orientation::default();
    };
    let Ok(model) = serde_json::from_str::<RawModel>(&text) else {
        return Orientation::default();
    };

    // The architectural subprojects (skeleton-filtered).
    let skeleton: HashMap<&str, &str> =
        model.skeleton.iter().map(|s| (s.dir.as_str(), s.role.as_str())).collect();
    let skeleton_empty = skeleton.is_empty();

    // Keep the projects grain lists as real architectural units. A project
    // joins the terrain when a `skeleton[]` dir COVERS its dir — exact match
    // or ancestor prefix (an aggregate dir `x/Y` covers its `x/Y/A`, `x/Y/B`
    // child projects; exact-only silently hid a whole layer in the field).
    // A project nested under ANOTHER project's dir stays out — an inner unit
    // belongs to its parent's line. The root's empty dir maps to `(root)`.
    // When the model predates the skeleton field, keep every project (role
    // empty). Purely structural: no language/framework/kind is ever consulted.
    let mut terrain: Vec<TerrainRow> = model
        .projects
        .iter()
        .filter_map(|p| {
            let key = if p.dir.is_empty() { "(root)" } else { p.dir.as_str() };
            if skeleton_empty {
                return Some((p, String::new()));
            }
            let nested_in_project = model.projects.iter().any(|q| {
                !q.dir.is_empty()
                    && q.dir != p.dir
                    && key.starts_with(&format!("{}/", q.dir))
            });
            if nested_in_project {
                return None;
            }
            skeleton
                .iter()
                .filter(|(dir, _)| key == **dir || key.starts_with(&format!("{dir}/")))
                .max_by_key(|(dir, _)| dir.len())
                .map(|(_, role)| (p, (*role).to_string()))
        })
        .map(|(p, role)| TerrainRow {
            name: p.name.clone(),
            kind: p.kind.clone(),
            code_files: p.code_files,
            role,
        })
        .collect();
    // Byte-stable order: by layer (L0→L1→L2), then biggest first, then name.
    terrain.sort_by(|a, b| {
        a.role
            .cmp(&b.role)
            .then(b.code_files.cmp(&a.code_files))
            .then(a.name.cmp(&b.name))
    });

    Orientation { terrain }
}

// ===========================================================================
// Rendering — byte-stable text for both the command and the injects.
// ===========================================================================

/// Header line for the terrain block.
const TERRAIN_HEADER: &str =
    "[Terreno] subprojetos mapeados pelo /scan — leia daqui, não grepe para se orientar:";

/// Render the terrain block (header + one line per subproject), or `None`
/// when there is no terrain (fail-open / empty model).
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

// ===========================================================================
// Command entry — `mustard-rt run orient`.
// ===========================================================================

/// `run orient`: print the terrain. Empty (missing grain model) prints
/// nothing and exits 0 — fail-open, byte-stable.
pub fn run(root: &Path) {
    let orientation = compute_orientation(root);
    if let Some(t) = render_terrain(&orientation) {
        println!("{t}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// A compact, fully-controlled grain model: two architectural subprojects
    /// (one L1 `rt`, one L0 `web`) plus a nested unit and a fixture the skeleton
    /// join must drop.
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
        let o = compute_orientation(&root);
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

    /// The aggregate-dir shape: `skeleton[]` records a parent dir
    /// (`backend/Big`) while `projects[]` records its child projects
    /// (`backend/Big/App`, `backend/Big/Data`) — any multi-project solution/
    /// workspace produces this, whatever the stack. The prefix join must
    /// surface the children with the parent's role — the exact-only join
    /// silently hid an entire layer in the field. A unit nested under
    /// ANOTHER project must still be dropped.
    #[test]
    fn terrain_prefix_join_surfaces_solution_child_projects() {
        const NESTED: &str = r#"{
          "projects": [
            {"name": "web", "dir": "apps/web", "kind": "npm", "code_files": 40},
            {"name": "Big.App", "dir": "backend/Big/App", "kind": "dotnet", "code_files": 700},
            {"name": "Big.Data", "dir": "backend/Big/Data", "kind": "dotnet", "code_files": 300},
            {"name": "inner", "dir": "apps/web/inner", "kind": "npm", "code_files": 5}
          ],
          "skeleton": [
            {"dir": "apps/web", "role": "L0"},
            {"dir": "backend/Big", "role": "L3"}
          ]
        }"#;
        let (_d, root) = seed(NESTED);
        let o = compute_orientation(&root);
        let rendered = render_terrain(&o).expect("terrain");
        insta::assert_snapshot!(rendered, @r###"
        [Terreno] subprojetos mapeados pelo /scan — leia daqui, não grepe para se orientar:
        - web · npm · 40 arquivos — L0
        - Big.App · dotnet · 700 arquivos — L3
        - Big.Data · dotnet · 300 arquivos — L3
        "###);
    }

    #[test]
    fn orient_fail_open() {
        // No grain model on disk → empty projection, nothing rendered. The
        // command prints nothing and exits 0 (parity with the injects).
        let dir = tempdir().unwrap();
        let o = compute_orientation(dir.path());
        assert!(render_terrain(&o).is_none());
        // An unparseable model degrades the same way.
        let (_d, root) = seed("{ not json");
        let o2 = compute_orientation(&root);
        assert!(render_terrain(&o2).is_none());
    }
}
