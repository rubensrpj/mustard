//! Concept-node graph: parse, validate, render the MOC (Wave 3 — project-profiler).
//!
//! Walks `.claude/graph/` collecting every concept-node markdown file, parses
//! its frontmatter `id` plus inline `[[id]]` edges, and produces three
//! artifacts: an in-memory adjacency map, an `id → path` lookup table, and an
//! `index.md` Map-Of-Content (MOC) listing every node grouped by kind. The
//! same pass also surfaces validation warnings — `[[id]]` edges pointing at
//! ids the index does not know about (orphan edges) and cycles in the
//! adjacency graph (which the validator records as cut, never panics).
//!
//! The Wave 4 resolver consumes the adjacency + id table to walk the graph
//! and assemble per-agent context; this module is the build/validate face,
//! exposed both as a library (for `sync_registry` / future enrichment passes
//! to call) and as the `mustard-rt run graph-index` subcommand.
//!
//! ## Fail-open
//!
//! A missing `.claude/graph/` tree degrades to an empty index — never an
//! error. A malformed frontmatter (`id` missing) skips that file with a
//! warning rather than aborting the build. The validator records warnings;
//! it never returns `Err`.

use mustard_core::fs as mfs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The output of one graph-index build pass.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GraphIndex {
    /// `id → relative path under .claude/graph/`. Byte-stable ordering.
    pub nodes: BTreeMap<String, String>,
    /// Adjacency map: `id → outbound edge ids` (in source order).
    pub edges: BTreeMap<String, Vec<String>>,
    /// Validation warnings — orphans + cycles. Each entry is a single line.
    pub warnings: Vec<String>,
    /// Skill files whose frontmatter was extended with an `aliases:` entry.
    pub aliased_skills: Vec<String>,
}

/// Recursively collect every `.md` file under `dir`, sorted by relative path.
/// Hidden directories (those starting with `.`) are *not* skipped — the vault
/// itself lives under `.claude/graph/`, so dot-prefix is normal here.
fn collect_markdown(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = mfs::read_dir(&d) else {
            continue;
        };
        for entry in entries {
            if entry.is_dir {
                stack.push(entry.path.clone());
                continue;
            }
            if !entry.file_name.ends_with(".md") {
                continue;
            }
            let rel = entry
                .path
                .strip_prefix(dir)
                .unwrap_or(&entry.path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((entry.path, rel));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Extract the frontmatter `id:` value (if present) from a markdown body.
/// Returns `None` when the file has no `---` frontmatter or no `id:` key.
fn parse_frontmatter_id(content: &str) -> Option<String> {
    let stripped = content.strip_prefix("---\n")?;
    let end = stripped.find("\n---")?;
    let block = &stripped[..end];
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("id:") {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Extract the frontmatter `kind:` value, defaulting to `"node"` when absent.
fn parse_frontmatter_kind(content: &str) -> String {
    let Some(stripped) = content.strip_prefix("---\n") else {
        return "node".to_string();
    };
    let Some(end) = stripped.find("\n---") else {
        return "node".to_string();
    };
    let block = &stripped[..end];
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("kind:") {
            let value = rest.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    "node".to_string()
}

/// Single-pass scan of `content` for `[[id]]` occurrences. Mirrors the
/// `wikilink::extract_links` token shape (`[a-zA-Z0-9_\-\.]+`) but expanded
/// to accept the `.` separator that namespaced concept-ids use.
#[must_use]
pub fn extract_edges(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    while i < len {
        if bytes[i] == b'[' && i + 1 < len && bytes[i + 1] == b'[' {
            let start = i + 2;
            let mut j = start;
            while j < len {
                let c = bytes[j];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' {
                    j += 1;
                    continue;
                }
                break;
            }
            if j > start && j + 1 < len && bytes[j] == b']' && bytes[j + 1] == b']' {
                if let Ok(name) = std::str::from_utf8(&bytes[start..j]) {
                    out.push(name.to_string());
                }
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Detect cycles in the adjacency map. Returns the ids that participate in a
/// cycle (one entry per back-edge target — duplicates collapsed). Cycles are
/// reported as warnings; the graph is never mutated.
fn detect_cycles(edges: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut cycles: BTreeSet<String> = BTreeSet::new();
    for start in edges.keys() {
        // Iterative DFS with a per-walk `on_stack` set. Each `start` runs its
        // own DFS so we do not need a global `visited` set (the cost is
        // bounded — graphs are small, sub-linear in entity count).
        let mut stack: Vec<(String, usize)> = vec![(start.clone(), 0)];
        let mut on_stack: BTreeSet<String> = BTreeSet::new();
        on_stack.insert(start.clone());
        while let Some((node, idx)) = stack.last().cloned() {
            let next = edges.get(&node).and_then(|v| v.get(idx)).cloned();
            match next {
                Some(neighbor) => {
                    let last_mut = stack
                        .last_mut()
                        .expect("stack non-empty inside cycle DFS");
                    last_mut.1 += 1;
                    if on_stack.contains(&neighbor) {
                        cycles.insert(neighbor);
                        continue;
                    }
                    on_stack.insert(neighbor.clone());
                    stack.push((neighbor, 0));
                }
                None => {
                    on_stack.remove(&node);
                    stack.pop();
                }
            }
        }
    }
    cycles.into_iter().collect()
}

/// Build the graph index from `<project_root>/.claude/graph/`. A missing
/// directory degrades to an empty [`GraphIndex`] (no warnings, no panic).
#[must_use]
pub fn build_index(project_root: &Path) -> GraphIndex {
    let graph_dir = project_root.join(".claude").join("graph");
    let mut index = GraphIndex::default();
    // Ensure the vault directory exists so the MOC is always materialised.
    // A missing directory is the cold-start case; subsequent runs over an
    // existing tree behave identically.
    if !graph_dir.exists() && mfs::create_dir_all(&graph_dir).is_err() {
        return index;
    }

    let mut id_to_kind: BTreeMap<String, String> = BTreeMap::new();
    for (abs, rel) in collect_markdown(&graph_dir) {
        // Skip the MOC itself — it has no `id:` and is regenerated each run.
        if rel == "index.md" {
            continue;
        }
        let Ok(content) = mfs::read_to_string(&abs) else {
            continue;
        };
        let Some(id) = parse_frontmatter_id(&content) else {
            index
                .warnings
                .push(format!("warning: {rel} has no frontmatter id — skipped"));
            continue;
        };
        if index.nodes.contains_key(&id) {
            index.warnings.push(format!(
                "warning: duplicate id {id} (second occurrence in {rel})"
            ));
            continue;
        }
        let kind = parse_frontmatter_kind(&content);
        let raw_edges = extract_edges(&content);
        index.edges.insert(id.clone(), raw_edges);
        id_to_kind.insert(id.clone(), kind);
        index.nodes.insert(id, rel);
    }

    // Orphan detection — `[[id]]` edge whose target is not in the index.
    for (from, neighbors) in &index.edges {
        for to in neighbors {
            if !index.nodes.contains_key(to) {
                index
                    .warnings
                    .push(format!("warning: orphan edge {from} -> {to}"));
            }
        }
    }

    // Cycle detection — reported as warnings; the adjacency is left intact.
    for cyc in detect_cycles(&index.edges) {
        index
            .warnings
            .push(format!("warning: cycle includes {cyc}"));
    }

    // Inject `aliases:[id]` into matching `SKILL.md` files (best-effort).
    let aliased = inject_skill_aliases(project_root, &index.nodes);
    index.aliased_skills = aliased;

    // Render the MOC. Failures here are silent — the in-memory index still
    // wins; the caller can inspect `warnings` for the failure.
    let moc = render_moc(&index.nodes, &id_to_kind);
    let moc_path = graph_dir.join("index.md");
    if mfs::write_atomic(&moc_path, moc.as_bytes()).is_err() {
        index
            .warnings
            .push(format!("warning: failed to write {}", moc_path.display()));
    }

    index
}

/// Render the MOC markdown: nodes grouped by `kind`, sorted by id.
fn render_moc(nodes: &BTreeMap<String, String>, id_to_kind: &BTreeMap<String, String>) -> String {
    let mut by_kind: BTreeMap<String, Vec<(&String, &String)>> = BTreeMap::new();
    for (id, rel) in nodes {
        let kind = id_to_kind
            .get(id)
            .cloned()
            .unwrap_or_else(|| "node".to_string());
        by_kind.entry(kind).or_default().push((id, rel));
    }
    let mut out = String::new();
    out.push_str("# Map of Content\n\n");
    let _ = writeln!(out, "Total nodes: **{}**\n", nodes.len());
    if nodes.is_empty() {
        out.push_str("_Empty graph — no concept-nodes yet._\n");
        return out;
    }
    for (kind, mut rows) in by_kind {
        let _ = writeln!(out, "## {kind}\n");
        rows.sort_by(|a, b| a.0.cmp(b.0));
        for (id, rel) in rows {
            let _ = writeln!(out, "- [{id}]({rel})");
        }
        out.push('\n');
    }
    out
}

/// Inject `aliases:[id]` into every `.claude/skills/*/SKILL.md` whose
/// directory name maps to a known skill-kind id. Idempotent: re-running on
/// a SKILL.md that already carries the alias is a no-op.
///
/// The skill id convention mirrors the concept-node convention:
/// `{sub}.skill.{slug}` where `sub = "_root"` for root-level skills and
/// `slug = directory name`.
fn inject_skill_aliases(project_root: &Path, nodes: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let skills_dir = project_root.join(".claude").join("skills");
    let Ok(entries) = mfs::read_dir(&skills_dir) else {
        return out;
    };
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let skill_md = entry.path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        let Ok(body) = mfs::read_to_string(&skill_md) else {
            continue;
        };
        let alias_id = format!("_root.skill.{}", super::interpret::slugify(&entry.file_name));
        // Only inject when the skill id is part of the live graph — keeps the
        // alias surface in sync with what the resolver can dereference.
        if !nodes.contains_key(&alias_id) {
            continue;
        }
        if let Some(updated) = ensure_alias_in_frontmatter(&body, &alias_id) {
            if mfs::write_atomic(&skill_md, updated.as_bytes()).is_ok() {
                out.push(alias_id);
            }
        }
    }
    out
}

/// Append `aliases:[<id>]` to a SKILL.md frontmatter when missing. Returns
/// `None` when the alias is already present (the caller skips the write).
fn ensure_alias_in_frontmatter(body: &str, id: &str) -> Option<String> {
    let stripped = body.strip_prefix("---\n")?;
    let end = stripped.find("\n---")?;
    let (fm, rest) = stripped.split_at(end);
    // Already aliased? Bail (idempotent).
    for line in fm.lines() {
        if let Some(after) = line.strip_prefix("aliases:") {
            if after.contains(id) {
                return None;
            }
        }
    }
    let alias_line = format!("aliases: [{id}]\n");
    let new_fm = if fm.ends_with('\n') {
        format!("{fm}{alias_line}")
    } else {
        format!("{fm}\n{alias_line}")
    };
    Some(format!("---\n{new_fm}{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn extract_edges_recognises_namespaced_ids() {
        let body = "see [[apps-rt.entity.user]] and [[apps-rt.enum.role]]\n[[bare-name]]";
        let edges = extract_edges(body);
        assert_eq!(
            edges,
            vec![
                "apps-rt.entity.user".to_string(),
                "apps-rt.enum.role".to_string(),
                "bare-name".to_string(),
            ]
        );
    }

    #[test]
    fn parse_frontmatter_id_handles_minimal_block() {
        let body = "---\nid: foo.entity.bar\nkind: entity\n---\nbody";
        assert_eq!(parse_frontmatter_id(body).as_deref(), Some("foo.entity.bar"));
        assert_eq!(parse_frontmatter_kind(body), "entity");
        assert!(parse_frontmatter_id("no frontmatter").is_none());
    }

    /// AC-2: id→path resolves every edge; missing targets become warnings;
    /// cycles never panic (recorded as a warning).
    #[test]
    fn graph_validation() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let graph_dir = root.join(".claude").join("graph");

        // a → b → c (clean)
        write(
            &graph_dir.join("foo.entity.a.md"),
            "---\nid: foo.entity.a\nkind: entity\n---\n# A\n[[foo.entity.b]]",
        );
        write(
            &graph_dir.join("foo.entity.b.md"),
            "---\nid: foo.entity.b\nkind: entity\n---\n# B\n[[foo.entity.c]]",
        );
        write(
            &graph_dir.join("foo.entity.c.md"),
            "---\nid: foo.entity.c\nkind: entity\n---\n# C\n",
        );
        // orphan edge target.
        write(
            &graph_dir.join("foo.entity.d.md"),
            "---\nid: foo.entity.d\nkind: entity\n---\n# D\n[[foo.entity.missing]]",
        );
        // cycle: e → f → e
        write(
            &graph_dir.join("foo.entity.e.md"),
            "---\nid: foo.entity.e\nkind: entity\n---\n# E\n[[foo.entity.f]]",
        );
        write(
            &graph_dir.join("foo.entity.f.md"),
            "---\nid: foo.entity.f\nkind: entity\n---\n# F\n[[foo.entity.e]]",
        );

        let index = build_index(root);
        assert_eq!(index.nodes.len(), 6, "every well-formed node indexed");
        assert!(root.join(".claude/graph/index.md").exists(), "MOC written");

        // Edge id-table coverage: every non-orphan edge target is in the table.
        for (from, neighbors) in &index.edges {
            for to in neighbors {
                if to == "foo.entity.missing" {
                    continue;
                }
                assert!(
                    index.nodes.contains_key(to),
                    "edge {from} -> {to} must resolve"
                );
            }
        }

        let has_orphan = index
            .warnings
            .iter()
            .any(|w| w.contains("orphan edge") && w.contains("foo.entity.missing"));
        assert!(has_orphan, "orphan must surface as a warning");
        let has_cycle = index
            .warnings
            .iter()
            .any(|w| w.contains("cycle includes"));
        assert!(has_cycle, "cycle must surface as a warning");
    }

    /// AC-3: ids must be unique — a duplicate id surfaces as a warning rather
    /// than overwriting the first entry.
    #[test]
    fn graph_ids_unique() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let graph_dir = root.join(".claude").join("graph");

        write(
            &graph_dir.join("foo.entity.a.md"),
            "---\nid: foo.entity.a\nkind: entity\n---\n# A\n",
        );
        // Duplicate id under a different filename.
        write(
            &graph_dir.join("foo.entity.a.dup.md"),
            "---\nid: foo.entity.a\nkind: entity\n---\n# A again\n",
        );
        // Distinct id.
        write(
            &graph_dir.join("foo.entity.b.md"),
            "---\nid: foo.entity.b\nkind: entity\n---\n# B\n",
        );

        let index = build_index(root);
        // Each unique id appears exactly once.
        let ids: BTreeSet<&String> = index.nodes.keys().collect();
        assert_eq!(ids.len(), index.nodes.len());
        assert_eq!(index.nodes.len(), 2, "duplicate id is skipped, not appended");
        let has_dup_warning = index
            .warnings
            .iter()
            .any(|w| w.contains("duplicate id foo.entity.a"));
        assert!(has_dup_warning, "duplicate id must surface as a warning");
    }

    #[test]
    fn skill_alias_injection_is_idempotent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Skill on disk.
        let skill = root
            .join(".claude")
            .join("skills")
            .join("my-skill")
            .join("SKILL.md");
        write(
            &skill,
            "---\nname: my-skill\ndescription: \"x\"\n---\nbody",
        );
        // Graph node mirrors the skill id convention.
        let graph_node = root
            .join(".claude")
            .join("graph")
            .join("_root.skill.my-skill.md");
        write(
            &graph_node,
            "---\nid: _root.skill.my-skill\nkind: skill\n---\n# my-skill\n",
        );

        let first = build_index(root);
        assert_eq!(first.aliased_skills, vec!["_root.skill.my-skill".to_string()]);
        let after_first = std::fs::read_to_string(&skill).unwrap();
        assert!(after_first.contains("aliases: [_root.skill.my-skill]"));

        // Second run is a no-op — the alias is already there.
        let second = build_index(root);
        assert!(second.aliased_skills.is_empty());
        let after_second = std::fs::read_to_string(&skill).unwrap();
        assert_eq!(after_first, after_second, "second run must not rewrite");
    }
}
