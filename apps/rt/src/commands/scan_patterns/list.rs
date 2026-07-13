//! `scan-patterns-list` — derive the missing pattern-skill *mold* worklist from
//! `grain.model.json` and emit it as a JSON array for the enrich agent.
//!
//! This is the pattern-mold twin of `scan-guards-list`. Where Guards walks the
//! `CLAUDE.md` tree, patterns projects FROM the deterministic model: for each
//! mined role cluster (`roles[]`) with at least [`MIN_CLUSTER`] members, it
//! attributes the cluster to the subproject that owns its `common_dir`, resolves
//! 2-3 real exemplar files (hand-written only — generated/vendored code never
//! teaches convention), and proposes a `{subproject-basename}-{role}-pattern`
//! mold — unless that mold already exists (create-only, so an existing mold is
//! never re-proposed). Capped at [`CAP_PER_SUBPROJECT`] new molds per subproject.
//!
//! Output: a JSON array `[{subproject, label, slug, moldPath, affix, affixKind,
//! declKind, implements, count, exemplars}]` to stdout, sorted by
//! `(subproject, slug)` for byte-stable output. Fail-open: a missing or
//! unparseable model degrades to `[]` and exit 0 — the enrich step then skips
//! silently, exactly like Guards on an empty worklist.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Minimum members a role cluster must have before it earns a mold — a mold
/// teaches a *recurring* convention, and fewer than three files is not yet one.
const MIN_CLUSTER: usize = 3;

/// Hard cap on new molds proposed per subproject per scan. Mirrors the SKILL's
/// "≤4 new molds per subproject per scan" — keeps a first scan of a large repo
/// from fanning out an unbounded agent batch.
const CAP_PER_SUBPROJECT: usize = 4;

/// How many exemplar files a mold candidate carries — enough for the agent to
/// read the shared shape, not so many the dispatch prompt bloats.
const MAX_EXEMPLARS: usize = 3;

/// Minimum exemplar files a cluster must have before it earns a mold. Exemplars
/// are resolved by filename-affix match, so requiring at least two is also the
/// quality bar that separates a real FILE-naming convention (e.g. `*Service.ts`,
/// `*_observer.rs`) from a mere declaration-name affix that happens to recur in
/// a folder — the latter is not a module pattern and must not spawn a mold.
const MIN_EXEMPLARS: usize = 2;

/// Path segments that mark a cluster as test/fixture terrain — a mold teaches
/// PRODUCTION convention, so a cluster whose `common_dir` sits under one of
/// these is discarded. Mirrors the SKILL's `tests/`, `fixtures/`, `__tests__/`,
/// `spec/` set (plus the singular `test`).
const TEST_SEGMENTS: &[&str] = &["tests", "test", "fixtures", "__tests__", "spec", "specs", "__mocks__", "mocks"];

/// The slice of `grain.model.json` this projection reads. Additive `#[serde(default)]`
/// throughout so an older/newer model shape keeps deserialising — the JSON is the
/// contract, not the grain crate's internal types.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Model {
    roles: Vec<Role>,
    modules: Vec<Mod>,
    projects: Vec<Proj>,
}

/// A mined role affix (from `roles[]`) — the "how we write an X" cluster.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Role {
    affix: String,
    /// "suffix" | "prefix".
    kind: String,
    count: usize,
    common_dir: String,
    decl_kind: String,
    implements: Option<String>,
}

/// A source module — used to resolve real exemplar files for a cluster.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Mod {
    path: String,
    /// Machine-written class ("generated" | "vendored" | …); empty = hand-written.
    /// Only hand-written files may serve as exemplars — a mold must teach the
    /// convention humans wrote, never a codegen output.
    file_class: String,
}

/// A workspace subproject (one per build manifest).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Proj {
    name: String,
    dir: String,
}

/// One mold-candidate worklist entry, serialised to the JSON the orchestrator
/// hands (per subproject) to the `mustard-patterns` agent.
#[derive(Serialize)]
struct Candidate {
    /// Subproject directory (forward-slashed, relative to root).
    subproject: String,
    /// Cluster label — the `appliesTo`/`cluster.label` the mold carries (the
    /// role, e.g. `service`).
    label: String,
    /// Mold slug — `{subproject-basename}-{label}`; the skill folder is
    /// `{slug}-pattern`. Matches the existing convention (`scan-stage`,
    /// `rt-inject`).
    slug: String,
    /// Where the agent's SKILL.md is written (`scan-patterns-apply --path`).
    #[serde(rename = "moldPath")]
    mold_path: String,
    affix: String,
    #[serde(rename = "affixKind")]
    affix_kind: String,
    #[serde(rename = "declKind")]
    decl_kind: String,
    implements: Option<String>,
    count: usize,
    /// 1-3 real hand-written files of the cluster the agent must read.
    exemplars: Vec<String>,
}

/// Run `scan-patterns-list`. Prints a JSON array to stdout; exit 0 always.
pub fn run(root: &Path) {
    let candidates = collect(root);
    // `to_string` cannot fail for this shape; fall back to `[]` defensively.
    println!("{}", serde_json::to_string(&candidates).unwrap_or_else(|_| "[]".to_string()));
}

/// The testable core of [`run`]: read the model and derive the capped, sorted
/// mold worklist. Fail-open — any load/parse failure yields an empty worklist.
fn collect(root: &Path) -> Vec<Candidate> {
    let model_path = root.join(".claude").join("grain.model.json");
    let Ok(text) = std::fs::read_to_string(&model_path) else {
        return Vec::new();
    };
    let Ok(model) = serde_json::from_str::<Model>(&text) else {
        return Vec::new();
    };

    // Subprojects with a non-empty dir, longest dir first so `common_dir` is
    // attributed to its most-specific owner (the root unit, dir "", is excluded —
    // molds are never authored for the workspace root).
    let mut projects: Vec<&Proj> = model.projects.iter().filter(|p| !p.dir.is_empty()).collect();
    projects.sort_by(|a, b| b.dir.len().cmp(&a.dir.len()).then(a.dir.cmp(&b.dir)));

    // Module paths sorted once — every exemplar scan reads this in a stable order.
    let mut modules: Vec<&Mod> = model.modules.iter().collect();
    modules.sort_by(|a, b| a.path.cmp(&b.path));

    let mut candidates: Vec<Candidate> = Vec::new();
    for role in &model.roles {
        if role.count < MIN_CLUSTER || role.common_dir.is_empty() {
            continue;
        }
        if under_test(&role.common_dir) {
            continue;
        }
        let Some(project) = owner_of(&role.common_dir, &projects) else {
            continue; // lives outside any named subproject (root-level) — skip.
        };
        let label = slugify(&role.affix);
        if label.is_empty() {
            continue;
        }
        // Lower-kebab the subproject short name too, so a PascalCase C# unit
        // (`DataAccess`) yields a consistent `dataaccess-<role>-pattern` folder.
        let subproj = slugify(basename(&project.dir));
        if subproj.is_empty() {
            continue;
        }
        let slug = format!("{subproj}-{label}");
        let mold_path = format!("{}/.claude/skills/{}-pattern/SKILL.md", project.dir, slug);
        // Create-only: an existing mold (this slug, or any `*-pattern` folder
        // ending in `-{label}-pattern`) is never re-proposed.
        if mold_exists(root, &project.dir, &slug, &label) {
            continue;
        }
        let exemplars = exemplars_for(role, &modules);
        if exemplars.len() < MIN_EXEMPLARS {
            continue; // not a real file-naming convention — nothing teachable here.
        }
        candidates.push(Candidate {
            subproject: project.dir.clone(),
            label,
            slug,
            mold_path,
            affix: role.affix.clone(),
            affix_kind: role.kind.clone(),
            decl_kind: role.decl_kind.clone(),
            implements: role.implements.clone(),
            count: role.count,
            exemplars,
        });
    }

    cap_per_subproject(candidates)
}

/// Enforce [`CAP_PER_SUBPROJECT`]: keep the highest-count clusters per subproject
/// (ties broken by slug), then return the survivors sorted by `(subproject, slug)`
/// for byte-stable output.
fn cap_per_subproject(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    // Rank within each subproject by count desc, then slug, so the cap keeps the
    // strongest conventions.
    candidates.sort_by(|a, b| {
        a.subproject
            .cmp(&b.subproject)
            .then(b.count.cmp(&a.count))
            .then(a.slug.cmp(&b.slug))
    });
    let mut kept: Vec<Candidate> = Vec::new();
    let mut per: BTreeMap<String, usize> = BTreeMap::new();
    for c in candidates {
        let n = per.entry(c.subproject.clone()).or_insert(0);
        if *n >= CAP_PER_SUBPROJECT {
            continue;
        }
        *n += 1;
        kept.push(c);
    }
    // Final byte-stable order for the emitted worklist.
    kept.sort_by(|a, b| a.subproject.cmp(&b.subproject).then(a.slug.cmp(&b.slug)));
    kept
}

/// True when `dir` sits under a conventional test/fixture segment.
fn under_test(dir: &str) -> bool {
    dir.split('/').any(|seg| {
        let seg = seg.to_lowercase();
        TEST_SEGMENTS.contains(&seg.as_str())
    })
}

/// The subproject that owns `common_dir`: the one whose `dir` is the longest
/// prefix of it. `projects` is pre-sorted longest-first, so the first match wins.
fn owner_of<'a>(common_dir: &str, projects: &[&'a Proj]) -> Option<&'a Proj> {
    projects
        .iter()
        .copied()
        .find(|p| common_dir == p.dir || common_dir.starts_with(&format!("{}/", p.dir)))
}

/// The last path segment of a directory (the subproject's short name).
fn basename(dir: &str) -> &str {
    dir.rsplit('/').next().unwrap_or(dir)
}

/// Lowercase `s`, mapping every non-`[a-z0-9]` run to a single `-` and trimming
/// leading/trailing dashes. `"Service" -> "service"`, `"IRepository" -> "irepository"`.
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Whether a mold for this cluster already exists under the subproject's
/// `.claude/skills/`. True when the exact `{slug}-pattern` folder is present, OR
/// any existing `*-pattern` folder ends in `-{label}-pattern` / equals
/// `{label}-pattern` (the same role under a different subproject prefix).
fn mold_exists(root: &Path, subproject: &str, slug: &str, label: &str) -> bool {
    let skills_dir: PathBuf = root.join(subproject).join(".claude").join("skills");
    let exact = format!("{slug}-pattern");
    let by_label_suffix = format!("-{label}-pattern");
    let by_label_exact = format!("{label}-pattern");
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return false; // no skills dir yet → nothing exists.
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with("-pattern") {
            continue;
        }
        if name == exact || name == by_label_exact || name.ends_with(&by_label_suffix) {
            return true;
        }
    }
    false
}

/// Resolve up to [`MAX_EXEMPLARS`] hand-written exemplar files for `role`: the
/// files *directly* in its `common_dir` (grain's role→folder map) whose filename
/// stem carries the affix. The match is deliberately precise — no folder-neighbour
/// fallback — because the exemplars ARE the quality signal: two or more files
/// whose names carry the affix (`UserService.ts`, `OrderService.ts`;
/// `amend_window_inject.rs`) prove a real file-naming convention worth a mold,
/// whereas a declaration-name affix with no matching filenames (a type suffix
/// that merely recurs in a shared folder) resolves too few and is rightly
/// dropped. `modules` is pre-sorted by path, so the pick is stable; generated/
/// vendored code never teaches, so it is skipped.
fn exemplars_for(role: &Role, modules: &[&Mod]) -> Vec<String> {
    let affix = role.affix.to_lowercase();
    modules
        .iter()
        .filter(|m| m.file_class.is_empty()) // hand-written only
        .filter(|m| parent_dir(&m.path) == role.common_dir)
        .filter(|m| matches_affix(stem(&m.path), &affix, &role.kind))
        .map(|m| m.path.clone())
        .take(MAX_EXEMPLARS)
        .collect()
}

/// The directory portion of a path (everything before the last `/`), or `""` for
/// a bare filename.
fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// The filename stem (last path segment, extension stripped), lowercased.
fn stem(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    let base = file.split('.').next().unwrap_or(file);
    base.to_lowercase()
}

/// Whether `stem` carries `affix` in the position `kind` implies. Unknown `kind`
/// falls back to a `contains` test so a cluster is never silently dropped.
fn matches_affix(stem: String, affix: &str, kind: &str) -> bool {
    match kind {
        "suffix" => stem.ends_with(affix),
        "prefix" => stem.starts_with(affix),
        _ => stem.contains(affix),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_model(root: &Path, json: &str) {
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".claude").join("grain.model.json"), json).unwrap();
    }

    #[test]
    fn slugify_normalises() {
        assert_eq!(slugify("Service"), "service");
        assert_eq!(slugify("IRepository"), "irepository");
        assert_eq!(slugify("use"), "use");
        assert_eq!(slugify("__weird__"), "weird");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn under_test_flags_test_terrain() {
        assert!(under_test("apps/api/tests/support"));
        assert!(under_test("pkg/__tests__"));
        assert!(under_test("app/spec/models"));
        assert!(!under_test("apps/api/src/services"));
    }

    #[test]
    fn owner_picks_longest_prefix() {
        let root = Proj { name: "root".into(), dir: "".into() };
        let api = Proj { name: "api".into(), dir: "apps/api".into() };
        let core = Proj { name: "core".into(), dir: "apps/api/core".into() };
        // Root (empty dir) is excluded by `collect`; here we pass only non-empty.
        let mut projects: Vec<&Proj> = vec![&root, &api, &core].into_iter().filter(|p| !p.dir.is_empty()).collect();
        projects.sort_by(|a, b| b.dir.len().cmp(&a.dir.len()));
        assert_eq!(owner_of("apps/api/core/services", &projects).unwrap().dir, "apps/api/core");
        assert_eq!(owner_of("apps/api/services", &projects).unwrap().dir, "apps/api");
        assert!(owner_of("apps/web/services", &projects).is_none());
    }

    #[test]
    fn collect_proposes_mold_for_a_real_cluster() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [{"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services","decl_kind":"class","implements":"BaseService"}],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"},
                {"path":"apps/api/services/README.md"}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "one cluster earns a mold");
        let c = &got[0];
        assert_eq!(c.subproject, "apps/api");
        assert_eq!(c.label, "service");
        assert_eq!(c.slug, "api-service");
        assert_eq!(c.mold_path, "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        assert_eq!(c.implements.as_deref(), Some("BaseService"));
        // Only the two matching hand-written files are exemplars (README excluded).
        assert_eq!(c.exemplars, vec!["apps/api/services/OrderService.ts", "apps/api/services/UserService.ts"]);
    }

    #[test]
    fn collect_skips_below_threshold_and_test_and_generated() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [
                {"affix":"Small","kind":"suffix","count":2,"common_dir":"apps/api/small"},
                {"affix":"Fixture","kind":"suffix","count":9,"common_dir":"apps/api/tests/fixtures"},
                {"affix":"Client","kind":"suffix","count":8,"common_dir":"apps/api/gen"}
              ],
              "modules": [
                {"path":"apps/api/gen/UserClient.ts","file_class":"generated"},
                {"path":"apps/api/gen/OrderClient.ts","file_class":"generated"}
              ]
            }"#,
        );
        let got = collect(root);
        // Small (count<3) skipped; Fixture (under tests/) skipped; Client (only
        // generated modules → no hand-written exemplar) skipped.
        assert!(got.is_empty(), "no production cluster survives: {:?}", got.iter().map(|c| &c.slug).collect::<Vec<_>>());
    }

    #[test]
    fn collect_skips_existing_mold() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [{"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services"}],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"}
              ]
            }"#,
        );
        // A mold for this role already exists (note: different subproject prefix,
        // still matched by the `-service-pattern` suffix rule).
        let existing = root.join("apps/api/.claude/skills/legacy-service-pattern");
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("SKILL.md"), "# existing").unwrap();

        assert!(collect(root).is_empty(), "an existing mold for the role must not be re-proposed");
    }

    #[test]
    fn collect_caps_per_subproject() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Six clusters in one subproject, each with a real exemplar — only the
        // top CAP_PER_SUBPROJECT (by count) survive.
        let mut roles = String::new();
        let mut modules = String::new();
        for (i, n) in [("A", 10), ("B", 9), ("C", 8), ("D", 7), ("E", 6), ("F", 5)] {
            let lower = i.to_lowercase();
            roles.push_str(&format!(
                r#"{{"affix":"{i}","kind":"suffix","count":{n},"common_dir":"apps/api/{lower}"}},"#
            ));
            // two hand-written exemplars whose stems end with the affix (clears MIN_EXEMPLARS)
            modules.push_str(&format!(
                r#"{{"path":"apps/api/{lower}/Thing{i}.ts"}},{{"path":"apps/api/{lower}/Other{i}.ts"}},"#
            ));
        }
        let model = format!(
            r#"{{"projects":[{{"name":"api","dir":"apps/api"}}],"roles":[{}],"modules":[{}]}}"#,
            roles.trim_end_matches(','),
            modules.trim_end_matches(',')
        );
        write_model(root, &model);
        let got = collect(root);
        assert_eq!(got.len(), CAP_PER_SUBPROJECT, "capped to {CAP_PER_SUBPROJECT}: {:?}", got.iter().map(|c| &c.slug).collect::<Vec<_>>());
        // The four highest-count clusters (A,B,C,D) are the survivors.
        let slugs: Vec<&str> = got.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, vec!["api-a", "api-b", "api-c", "api-d"]);
    }

    #[test]
    fn collect_fail_open_on_missing_model() {
        let dir = tempfile::tempdir().unwrap();
        assert!(collect(dir.path()).is_empty(), "missing model → empty worklist, never a panic");
    }

    #[test]
    fn output_is_byte_stable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [
                {"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services"},
                {"affix":"Repository","kind":"suffix","count":4,"common_dir":"apps/api/repos"}
              ],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"},
                {"path":"apps/api/repos/UserRepository.ts"},
                {"path":"apps/api/repos/OrderRepository.ts"}
              ]
            }"#,
        );
        let a = serde_json::to_string(&collect(root)).unwrap();
        let b = serde_json::to_string(&collect(root)).unwrap();
        assert_eq!(a, b, "two runs must produce identical bytes");
        // Sorted by slug: api-repository before api-service.
        assert!(a.find("api-repository").unwrap() < a.find("api-service").unwrap());
    }
}
