//! `mustard-rt run scan-skill-render` — **deterministic** per-cluster SKILL.md
//! generation (F3-a — min-IA / max-Rust).
//!
//! ## What
//!
//! Reads the entity-registry v4 `_patterns.{stack}.discovered[]` clusters and,
//! for every cluster that **qualifies** as a reusable convention, materialises a
//! granular skill under `{subproject}/.claude/skills/{name}/`:
//!
//! - `SKILL.md` — canonical frontmatter ([`mustard_core::domain::skill::frontmatter`])
//!   with **`appliesTo` populated** with the cluster label (this is what unlocks
//!   the `applies_match` arm of the [`crate::commands::skill::skill_resolve`]
//!   scorer), `metadata.generated_by = scan`, derived `tags`, plus a `## Convention`
//!   body (folder / suffix / base-class / interfaces / naming) and a `## Examples`
//!   list built from the cluster `samples[]`. Capped at ≤60 lines so the
//!   `scan-md-validate` size gate passes.
//! - `references/examples.md` — the same samples enumerated with `Ref:` paths so
//!   the existence check in `scan-md-validate` resolves.
//!
//! Everything is **English** (internal artifact) and produced with **no LLM** on
//! the main path — the description is a deterministic stub composed from cluster
//! fields. A documented hook ([`describe_cluster`]) marks where a future,
//! one-shot LLM enrichment could replace the stub without touching the rest of
//! the pipeline.
//!
//! ## Qualification + quantity cap
//!
//! A cluster qualifies when:
//!
//! 1. `fileCount >= MIN_CLUSTER_FILES` (3) — one-off groupings are not
//!    conventions, and
//! 2. its label / suffix is **not noise** ([`is_noise_label`]: `Test`, `Mock`,
//!    `Spec`, `Generated`, `Fixture`, the structural basenames `mod` / `index` /
//!    `main`, …).
//!
//! Qualified clusters are then capped at the **top [`MAX_SKILLS_PER_SUBPROJECT`]
//! (8) by `fileCount`** per subproject so the generator never inflates the
//! orchestrator's skill catalog with the long tail of marginal clusters.
//!
//! ## Idempotence + manual preservation
//!
//! Modelled on [`crate::commands::scan::node_gen`]: generated skills carry the
//! frontmatter `metadata.generated_by: scan` marker **and** the body fence
//! `<!-- mustard:generated -->`. A regeneration pass
//!
//! 1. **rewrites** every qualified cluster's skill (byte-identical on a no-change
//!    run — the render is a pure function of the cluster),
//! 2. **reaps** generated skills whose cluster no longer qualifies, and
//! 3. **never touches** a skill that lacks the marker (hand-authored skills are
//!    owned by the human).
//!
//! ## Fail-open
//!
//! Every filesystem error degrades to a skip; the registry is the source of
//! truth and a skill directory that could not be materialised is never fatal.

use mustard_core::domain::entity_registry::EntityRegistry;
use mustard_core::domain::skill::frontmatter::{parse as parse_fm, SkillSource};
use mustard_core::io::fs as mfs;
use mustard_core::ClaudePaths;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::interpret::slugify;

/// Minimum `fileCount` for a cluster to count as a reusable convention. Below
/// this a grouping is a one-off, not a pattern worth a skill.
pub const MIN_CLUSTER_FILES: u64 = 3;

/// Per-subproject quantity cap — at most this many skills are generated, the
/// top clusters by `fileCount`. Keeps the orchestrator's skill catalog lean
/// (progressive disclosure still applies on top, via `skill-resolve` top-K).
pub const MAX_SKILLS_PER_SUBPROJECT: usize = 8;

/// Body fence marking a SKILL.md as scan-generated — the same sentinel the
/// `scan-md-validate` size/fence gate expects.
pub const GEN_BODY_MARKER: &str = "<!-- mustard:generated -->";

/// Noise suffixes/labels that never become a convention skill, lower-cased.
/// `Test` / `Mock` / `Spec` are test-only; the structural basenames (`mod`,
/// `index`, `main`, …) are language plumbing, not a domain convention.
const NOISE_LABELS: &[&str] = &[
    "test", "tests", "mock", "mocks", "spec", "specs", "generated", "fixture",
    "fixtures", "stub", "stubs", "mod", "index", "main", "lib", "types",
    "constants", "config", "util", "utils", "helper", "helpers",
];

/// The outcome of one render pass — counts for the caller's report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderReport {
    /// Skills written (created or rewritten) this pass.
    pub written: usize,
    /// Stale generated skills reaped this pass.
    pub removed: usize,
    /// Manual (non-generated) skills left untouched.
    pub preserved: usize,
    /// Clusters skipped because they did not qualify (filtered or over the cap).
    pub skipped: usize,
}

/// One skill ready to render — the pure product of a single qualified cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillPlan {
    /// `{sub}-{label-slug}-pattern` skill name (kebab-case).
    name: String,
    /// Subproject the cluster belongs to (relative path component).
    subproject: String,
    /// Cluster label (the raw value — used verbatim in `appliesTo`).
    label: String,
    /// Deterministic description stub.
    description: String,
    /// Derived tags (always includes `add`/`refactor`; never empty).
    tags: Vec<&'static str>,
    /// `## Convention` bullet lines (already formatted, no leading `-`).
    convention: Vec<String>,
    /// Real sample file paths as recorded on the cluster — either a bare
    /// basename or a `folder/file` path relative to the subproject root.
    samples: Vec<String>,
    /// Candidate folders the samples may live under (relative to the subproject
    /// root) — used to resolve a bare-basename sample to a real path.
    folders: Vec<String>,
}

/// CLI entry point — render skills for one subproject or every detected one.
pub fn run(subproject: Option<&str>) {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = render(&root, subproject);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "written": report.written,
            "removed": report.removed,
            "preserved": report.preserved,
            "skipped": report.skipped,
        }))
        .unwrap_or_else(|_| "{}".into())
    );
}

/// Render every qualified cluster's skill. Reads the registry once, groups the
/// `discovered[]` clusters by `subprojectName`, and drives one
/// [`write_subproject_skills`] pass per subproject. Fail-open at every step.
#[must_use]
pub fn render(project_root: &Path, only_subproject: Option<&str>) -> RenderReport {
    let registry = EntityRegistry::load(project_root);
    let plans_by_sub = build_plans(&registry);

    let mut report = RenderReport::default();
    for (sub, plans) in &plans_by_sub {
        if let Some(want) = only_subproject {
            // Match on the subproject path tail (the registry tags clusters with
            // the bare subproject name, callers may pass `apps/<name>`).
            if !(sub == want || want.ends_with(sub.as_str())) {
                continue;
            }
        }
        let sub_report = write_subproject_skills(project_root, sub, plans);
        report.written += sub_report.written;
        report.removed += sub_report.removed;
        report.preserved += sub_report.preserved;
        report.skipped += sub_report.skipped;
    }
    report
}

// ---------------------------------------------------------------------------
// Plan building (pure — no IO)
// ---------------------------------------------------------------------------

/// Group the registry's `discovered[]` clusters by subproject and turn each
/// qualified one into a [`SkillPlan`]. Applies the qualification filter and the
/// per-subproject quantity cap. Pure — no filesystem access.
fn build_plans(registry: &EntityRegistry) -> BTreeMap<String, Vec<SkillPlan>> {
    // 1. Collect every discovered cluster across stacks, keyed by subproject.
    let mut by_sub: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for cluster in discovered_clusters(registry) {
        let sub = cluster
            .get("subprojectName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if sub.is_empty() {
            continue;
        }
        by_sub.entry(sub).or_default().push(cluster.clone());
    }

    // 2. Per subproject: filter to qualified clusters, sort by fileCount desc,
    //    cap at MAX_SKILLS_PER_SUBPROJECT, dedup by skill name.
    let mut out: BTreeMap<String, Vec<SkillPlan>> = BTreeMap::new();
    for (sub, mut clusters) in by_sub {
        clusters.sort_by_key(|c| std::cmp::Reverse(file_count(c)));
        let mut seen_names: BTreeSet<String> = BTreeSet::new();
        let mut plans: Vec<SkillPlan> = Vec::new();
        for cluster in &clusters {
            if plans.len() >= MAX_SKILLS_PER_SUBPROJECT {
                break;
            }
            let Some(plan) = plan_for_cluster(&sub, cluster) else {
                continue;
            };
            if !seen_names.insert(plan.name.clone()) {
                continue;
            }
            plans.push(plan);
        }
        if !plans.is_empty() {
            out.insert(sub, plans);
        }
    }
    out
}

/// Every `_patterns.{stack}.discovered[]` cluster across all stacks.
fn discovered_clusters(registry: &EntityRegistry) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let Some(patterns) = registry.patterns() else {
        return out;
    };
    for body in patterns.values() {
        if let Some(arr) = body.get("discovered").and_then(Value::as_array) {
            out.extend(arr.iter().cloned());
        }
    }
    out
}

fn file_count(c: &Value) -> u64 {
    c.get("fileCount").and_then(Value::as_u64).unwrap_or(0)
}

/// The cluster's primary label — `label`, falling back to `suffix`.
fn cluster_label(c: &Value) -> Option<String> {
    c.get("label")
        .or_else(|| c.get("suffix"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `true` when the (lower-cased) label is structural noise, never a convention.
#[must_use]
pub fn is_noise_label(label: &str) -> bool {
    NOISE_LABELS.contains(&label.to_ascii_lowercase().as_str())
}

/// Build a [`SkillPlan`] for one cluster, or `None` when it does not qualify.
fn plan_for_cluster(sub: &str, cluster: &Value) -> Option<SkillPlan> {
    if file_count(cluster) < MIN_CLUSTER_FILES {
        return None;
    }
    let label = cluster_label(cluster)?;
    if is_noise_label(&label) {
        return None;
    }

    let sub_slug = slugify(short_sub_name(sub));
    let label_slug = slugify(&label);
    let name = format!("{sub_slug}-{label_slug}-pattern");

    // --- Convention bullets — only fields actually present on the cluster. ---
    let mut convention: Vec<String> = Vec::new();
    if let Some(folder) = cluster
        .get("folderPattern")
        .or_else(|| cluster.get("folder"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        convention.push(format!("Folder: `{folder}`"));
    }
    convention.push(format!("Suffix: `{label}`"));
    if let Some(ext) = cluster.get("ext").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        convention.push(format!("Extension: `{ext}`"));
    }
    if let Some(base) = cluster
        .get("commonBaseClass")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        convention.push(format!("Base class: `{base}`"));
    }
    if let Some(ifaces) = cluster.get("commonInterfaces").and_then(Value::as_array) {
        let names: Vec<String> = ifaces
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        if !names.is_empty() {
            convention.push(format!("Interfaces: `{}`", names.join("`, `")));
        }
    }
    if let Some(naming) = cluster
        .get("namingPattern")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        convention.push(format!("Naming: `{naming}`"));
    }
    convention.push(format!("Files: {}", file_count(cluster)));

    // --- Samples (capped to 5 so the SKILL.md + examples.md stay small). ---
    let samples: Vec<String> = cluster
        .get("samples")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .take(5)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // Candidate folders for resolving bare-basename samples to a real path:
    // the `folders[]` array (suffix/filename clusters) or the single `folder`.
    let folders: Vec<String> = match cluster.get("folders").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        None => cluster
            .get("folder")
            .and_then(Value::as_str)
            .map(|f| vec![f.to_string()])
            .unwrap_or_default(),
    };

    Some(SkillPlan {
        name,
        subproject: sub.to_string(),
        description: describe_cluster(&label, file_count(cluster)),
        tags: derive_tags(),
        convention,
        samples,
        folders,
        label,
    })
}

/// The bare subproject name used as the skill-name prefix (the last path
/// segment, so `apps/dashboard` ⇒ `dashboard`).
fn short_sub_name(sub: &str) -> &str {
    sub.rsplit(['/', '\\']).next().filter(|s| !s.is_empty()).unwrap_or(sub)
}

/// Tags every cluster skill carries. The pattern is about adding "one more like
/// these" (`add`) or reshaping existing ones (`refactor`); kept deterministic
/// and stack-agnostic — no per-cluster guessing.
fn derive_tags() -> Vec<&'static str> {
    vec!["add", "refactor"]
}

/// **Deterministic description stub** built from cluster fields — the main-path,
/// no-LLM source of the skill description.
///
/// ## LLM enrichment hook (NOT implemented)
///
/// This is the single, documented seam where a future, opt-in one-shot LLM call
/// could replace the stub with a richer, "pushy" trigger description (per
/// `refs/scan/skill-generation.md`). The contract for that future hook:
/// it MUST stay off the main path (default-OFF, like `interpret::call_model`),
/// MUST be idempotent (cache the result by cluster content-hash so regeneration
/// stays byte-stable), and MUST fall back to this stub on any failure. Nothing
/// here calls a model today.
fn describe_cluster(label: &str, files: u64) -> String {
    format!(
        "Convention for the `{label}` cluster ({files} files in this subproject). \
         Use when adding a new `{label}`, extending the `{label}` set, or refactoring \
         existing `{label}` code to match the established shape."
    )
}

// ---------------------------------------------------------------------------
// Rendering (pure)
// ---------------------------------------------------------------------------

/// Render a [`SkillPlan`] to its byte-stable SKILL.md form.
///
/// `refs` are the samples resolved to repo-root-relative paths that exist on
/// disk (see [`resolve_refs`]); they are listed verbatim so the
/// `scan-md-validate` `Ref:` existence check resolves. Frontmatter key order is
/// fixed (`name`, `description`, `tags`, `appliesTo`, `scope`,
/// `metadata.generated_by`) so a no-change regeneration is byte-identical, and
/// the body carries the `<!-- mustard:generated -->` fence the validator
/// requires. Capped at ≤60 lines.
#[must_use]
fn render_skill_md(plan: &SkillPlan, refs: &[String]) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "name: {}", plan.name);
    let _ = writeln!(out, "description: {}", plan.description);
    let _ = writeln!(out, "tags: [{}]", plan.tags.join(", "));
    // appliesTo POPULATED — this unlocks the scorer's applies_match arm.
    let _ = writeln!(out, "appliesTo: [{}]", plan.label);
    out.push_str("scope: [code-editing]\n");
    out.push_str("metadata:\n");
    let _ = writeln!(out, "  generated_by: {}", SkillSource::Scan.as_str());
    let _ = writeln!(out, "  cluster:\n    label: {}", plan.label);
    out.push_str("---\n\n");
    let _ = writeln!(out, "{GEN_BODY_MARKER}");
    let _ = writeln!(out, "# {} pattern\n", plan.label);
    out.push_str("## Convention\n\n");
    for line in &plan.convention {
        let _ = writeln!(out, "- {line}");
    }
    out.push('\n');
    if !refs.is_empty() {
        out.push_str("## Examples\n\n");
        for r in refs {
            let _ = writeln!(out, "- Ref: {r}");
        }
        out.push('\n');
    }
    out.push_str("## References\n\n");
    out.push_str("See `references/examples.md`.\n");
    out
}

/// Render the companion `references/examples.md`. Carries the generated fence
/// and a `Ref:` line per resolved sample so the validator's existence check
/// resolves.
#[must_use]
fn render_examples_md(plan: &SkillPlan, refs: &[String]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{GEN_BODY_MARKER}");
    let _ = writeln!(out, "# {} — examples in this codebase\n", plan.label);
    if refs.is_empty() {
        out.push_str("_No resolvable samples recorded for this cluster._\n");
    } else {
        for r in refs {
            let _ = writeln!(out, "- Ref: {r}");
        }
        out.push('\n');
    }
    out
}

/// Resolve a plan's samples to **repo-root-relative** paths that exist on disk.
///
/// The validator's `Ref:` existence check joins from the repo root, but cluster
/// samples are either bare basenames or paths relative to the subproject root.
/// For each sample we try `sub_root/sample`, then `sub_root/<folder>/sample` for
/// each candidate folder; the first that exists is emitted relative to
/// `project_root`. Samples that resolve to nothing (stale registry) are skipped
/// silently — matching the skill-generation contract. Output is forward-slashed
/// and de-duplicated, preserving discovery order.
fn resolve_refs(project_root: &Path, sub_root: &Path, plan: &SkillPlan) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for sample in &plan.samples {
        let mut candidates: Vec<PathBuf> = vec![sub_root.join(sample)];
        for folder in &plan.folders {
            candidates.push(sub_root.join(folder).join(sample));
        }
        let Some(hit) = candidates.into_iter().find(|p| p.is_file()) else {
            continue;
        };
        let rel = hit
            .strip_prefix(project_root)
            .unwrap_or(&hit)
            .to_string_lossy()
            .replace('\\', "/");
        if seen.insert(rel.clone()) {
            out.push(rel);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Write / reap / preserve (filesystem)
// ---------------------------------------------------------------------------

/// `true` when a SKILL.md body is scan-generated and therefore owned by this
/// pass. Witnessed by *either* the frontmatter `metadata.generated_by: scan`
/// marker (parsed canonically) or the body fence — a skill lacking both is
/// hand-authored and must never be rewritten or reaped.
#[must_use]
fn is_generated(body: &str) -> bool {
    if body.contains(GEN_BODY_MARKER) {
        return true;
    }
    parse_fm(body)
        .map(|fm| fm.metadata.generated_by == Some(SkillSource::Scan))
        .unwrap_or(false)
}

/// Write every qualified skill for one subproject, reap stale generated skills,
/// preserve manual ones. Returns the per-subproject report.
fn write_subproject_skills(project_root: &Path, sub: &str, plans: &[SkillPlan]) -> RenderReport {
    let mut report = RenderReport::default();
    let sub_root = if sub == "." {
        project_root.to_path_buf()
    } else {
        resolve_sub_root(project_root, sub)
    };
    let Ok(paths) = ClaudePaths::for_project(&sub_root) else {
        return report;
    };
    let skills_dir = paths.skills_dir();
    if mfs::create_dir_all(&skills_dir).is_err() {
        return report;
    }

    // Skill directory names we are about to (re)write — spared from the reaper.
    let mut written_dirs: BTreeSet<String> = BTreeSet::new();

    // 1. Write the current skill set.
    for plan in plans {
        let dir = skills_dir.join(&plan.name);
        if mfs::create_dir_all(&dir).is_err() {
            continue;
        }
        let refs = resolve_refs(project_root, &sub_root, plan);
        let skill_md = render_skill_md(plan, &refs);
        let examples_md = render_examples_md(plan, &refs);
        let wrote_skill = mfs::write_atomic(&dir.join("SKILL.md"), skill_md.as_bytes()).is_ok();
        let _ = mfs::create_dir_all(&dir.join("references"));
        let _ = mfs::write_atomic(
            &dir.join("references").join("examples.md"),
            examples_md.as_bytes(),
        );
        if wrote_skill {
            report.written += 1;
            written_dirs.insert(plan.name.clone());
        }
    }

    // 2. Reap stale generated skills; count + preserve manual ones.
    if let Ok(entries) = mfs::read_dir(&skills_dir) {
        for entry in entries {
            if !entry.is_dir {
                continue;
            }
            let name = entry.file_name.clone();
            if written_dirs.contains(&name) {
                continue;
            }
            let skill_md = entry.path.join("SKILL.md");
            let Ok(body) = mfs::read_to_string(&skill_md) else {
                // No SKILL.md (or unreadable) — not ours to touch.
                continue;
            };
            if is_generated(&body) {
                if mfs::remove_dir_all(&entry.path).is_ok() {
                    report.removed += 1;
                }
            } else {
                report.preserved += 1;
            }
        }
    }

    report
}

/// Resolve a subproject's root directory from its registry name. The registry
/// tags clusters with the bare subproject name; the on-disk layout is
/// `apps/<name>` or `packages/<name>` (the canonical monorepo shape), falling
/// back to `<root>/<name>` then `<root>` when neither exists.
fn resolve_sub_root(project_root: &Path, sub: &str) -> PathBuf {
    for top in ["apps", "packages"] {
        let candidate = project_root.join(top).join(sub);
        if candidate.is_dir() {
            return candidate;
        }
    }
    let direct = project_root.join(sub);
    if direct.is_dir() {
        return direct;
    }
    project_root.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::skill::frontmatter::{parse as parse_fm, validate};
    use serde_json::json;
    use tempfile::tempdir;

    /// Plant a workspace anchor + an `entity-registry.json` with the given body.
    fn seed_registry(root: &Path, doc: Value) {
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        let paths = ClaudePaths::for_project(root).unwrap();
        let pretty = format!("{}\n", serde_json::to_string_pretty(&doc).unwrap());
        std::fs::write(paths.entity_registry_json_path(), pretty).unwrap();
    }

    /// A registry with one qualified cluster (`Service`, 5 files) under the
    /// `api` subproject + one noise cluster (`Test`) + one too-small cluster.
    fn fixture_doc() -> Value {
        json!({
            "_meta": { "version": "4.0", "generated": "2026-05-29" },
            "_patterns": {
                "rust": {
                    "discovered": [
                        {
                            "kind": "suffix-cluster",
                            "label": "Service",
                            "suffix": "Service",
                            "ext": ".rs",
                            "fileCount": 5,
                            "folderPattern": "src/services/",
                            "commonBaseClass": "BaseService",
                            "samples": ["src/services/UserService.rs", "src/services/AuthService.rs"],
                            "subprojectName": "api"
                        },
                        {
                            "kind": "suffix-cluster",
                            "label": "Test",
                            "suffix": "Test",
                            "ext": ".rs",
                            "fileCount": 9,
                            "samples": ["src/UserTest.rs"],
                            "subprojectName": "api"
                        },
                        {
                            "kind": "suffix-cluster",
                            "label": "Widget",
                            "suffix": "Widget",
                            "ext": ".rs",
                            "fileCount": 2,
                            "samples": ["src/Foo.rs"],
                            "subprojectName": "api"
                        }
                    ]
                }
            },
            "_enums": {},
            "e": {}
        })
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn qualification_filters_noise_and_small_clusters() {
        let registry = EntityRegistry::from_value(fixture_doc());
        let plans = build_plans(&registry);
        let api = plans.get("api").expect("api subproject");
        // Only `Service` qualifies: `Test` is noise, `Widget` has <3 files.
        assert_eq!(api.len(), 1, "got {api:?}");
        assert_eq!(api[0].name, "api-service-pattern");
        assert_eq!(api[0].label, "Service");
    }

    #[test]
    fn quantity_cap_keeps_top_n_by_filecount() {
        // 12 distinct qualified clusters → capped at MAX_SKILLS_PER_SUBPROJECT.
        let mut discovered = Vec::new();
        for i in 0..12 {
            discovered.push(json!({
                "label": format!("Kind{i}"),
                "suffix": format!("Kind{i}"),
                "ext": ".rs",
                "fileCount": 3 + i,
                "samples": [format!("src/Kind{i}.rs")],
                "subprojectName": "api"
            }));
        }
        let doc = json!({
            "_meta": { "version": "4.0" },
            "_patterns": { "rust": { "discovered": discovered } },
            "_enums": {}, "e": {}
        });
        let registry = EntityRegistry::from_value(doc);
        let plans = build_plans(&registry);
        let api = plans.get("api").unwrap();
        assert_eq!(api.len(), MAX_SKILLS_PER_SUBPROJECT);
        // The kept set is the highest-fileCount clusters (Kind11..Kind4).
        assert!(api.iter().any(|p| p.label == "Kind11"));
        assert!(!api.iter().any(|p| p.label == "Kind0"));
    }

    /// Plant the `apps/api` layout + the two `Service` sample files so the
    /// `Ref:` resolution finds them on disk.
    fn plant_api_samples(root: &Path) {
        let svc = root.join("apps").join("api").join("src").join("services");
        std::fs::create_dir_all(&svc).unwrap();
        std::fs::write(svc.join("UserService.rs"), "struct UserService;").unwrap();
        std::fs::write(svc.join("AuthService.rs"), "struct AuthService;").unwrap();
    }

    #[test]
    fn renders_skill_md_under_60_lines_with_applies_and_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_registry(root, fixture_doc());
        plant_api_samples(root);

        let report = render(root, None);
        assert_eq!(report.written, 1);

        let skill = root
            .join("apps")
            .join("api")
            .join(".claude")
            .join("skills")
            .join("api-service-pattern")
            .join("SKILL.md");
        let body = read(&skill);
        // Size cap.
        assert!(body.lines().count() <= 60, "SKILL.md must be ≤60 lines");
        // appliesTo populated + generated marker.
        let fm = parse_fm(&body).expect("frontmatter parses");
        assert_eq!(fm.applies_to, vec!["Service"]);
        assert_eq!(fm.metadata.generated_by, Some(SkillSource::Scan));
        assert!(body.contains(GEN_BODY_MARKER));
        // Convention fields surfaced.
        assert!(body.contains("Base class: `BaseService`"));
        assert!(body.contains("src/services/"));
        // Companion examples.md exists with a repo-root-relative Ref line.
        let examples = skill.parent().unwrap().join("references").join("examples.md");
        let ex = read(&examples);
        assert!(
            ex.contains("Ref: apps/api/src/services/UserService.rs"),
            "examples.md must carry a resolvable Ref; got:\n{ex}"
        );
    }

    #[test]
    fn generated_skill_passes_strict_frontmatter() {
        let registry = EntityRegistry::from_value(fixture_doc());
        let plans = build_plans(&registry);
        let plan = &plans["api"][0];
        let body = render_skill_md(plan, &["apps/api/src/services/UserService.rs".to_string()]);
        let fm = parse_fm(&body).unwrap();
        // Strict mode requires tags + scope + metadata.generated_by — all present.
        assert!(validate(&fm, true).is_ok(), "strict validation must pass");
    }

    #[test]
    fn second_render_is_byte_identical() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_registry(root, fixture_doc());
        plant_api_samples(root);

        render(root, None);
        let skills = ClaudePaths::for_project(&root.join("apps").join("api"))
            .unwrap()
            .skills_dir();
        let snapshot = snapshot_dir(&skills);

        render(root, None);
        let snapshot2 = snapshot_dir(&skills);
        assert_eq!(snapshot, snapshot2, "regeneration must be byte-stable");
    }

    #[test]
    fn manual_skill_preserved_and_stale_generated_reaped() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_registry(root, fixture_doc());
        plant_api_samples(root);
        let skills = ClaudePaths::for_project(&root.join("apps").join("api"))
            .unwrap()
            .skills_dir();
        std::fs::create_dir_all(&skills).unwrap();

        // A hand-authored skill (NO generated marker).
        let manual_dir = skills.join("api-hand-written");
        std::fs::create_dir_all(&manual_dir).unwrap();
        let manual = "---\nname: api-hand-written\ndescription: A manual skill that fills at least fifty characters here ok.\ntags: [fix]\n---\nManual body, no marker.\n";
        std::fs::write(manual_dir.join("SKILL.md"), manual).unwrap();

        // A stale generated skill for a cluster the registry no longer has.
        let stale_dir = skills.join("api-ghost-pattern");
        std::fs::create_dir_all(&stale_dir).unwrap();
        let stale = format!(
            "---\nname: api-ghost-pattern\ndescription: Ghost convention filler text long enough to clear fifty chars.\ntags: [add]\nappliesTo: [Ghost]\nscope: [code-editing]\nmetadata:\n  generated_by: scan\n---\n\n{GEN_BODY_MARKER}\n# Ghost\n"
        );
        std::fs::write(stale_dir.join("SKILL.md"), &stale).unwrap();

        let report = render(root, None);
        assert_eq!(report.written, 1, "Service skill (re)written");
        assert_eq!(report.removed, 1, "stale generated ghost reaped");
        assert_eq!(report.preserved, 1, "manual skill counted as preserved");

        // Manual node survives intact, byte-for-byte.
        assert_eq!(read(&manual_dir.join("SKILL.md")), manual);
        // Stale generated skill is gone.
        assert!(!stale_dir.exists());
    }

    fn snapshot_dir(skills: &Path) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        walk(base, &p, out);
                    } else if let Ok(body) = std::fs::read_to_string(&p) {
                        let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                        out.insert(rel, body);
                    }
                }
            }
        }
        walk(skills, skills, &mut out);
        out
    }
}
