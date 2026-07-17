//! `scan-patterns-list` — derive the missing pattern-skill *mold* worklist from
//! `grain.model.json` and emit it as a JSON array for the enrich agent.
//!
//! This is the pattern-mold twin of `scan-guards-list`. Where Guards walks the
//! `CLAUDE.md` tree, patterns projects FROM the deterministic model: for each
//! mined role cluster (`roles[]`) with at least [`MIN_CLUSTER`] members, it
//! attributes the cluster to the subproject that owns its `common_dir`, resolves
//! 2-3 real exemplar files (hand-written only — generated/vendored code never
//! teaches convention), and proposes a `{subproject-basename}-{role}-pattern`
//! mold. Machine-authored molds stay FRESH: an existing mold whose
//! [`super::provenance`] marker verifies is re-proposed as `mode: "refresh"`
//! on every scan; a hand-edited or unmarked mold is preserved and never
//! re-proposed, and a slug recorded in `.claude/scan-declined.json`
//! ([`super::decline`]) is skipped entirely. Uncapped — every cluster clearing
//! the quality bars is proposed.
//!
//! Output: a JSON array `[{subproject, label, slug, mode, moldPath, affix,
//! affixKind, declKind, implements, count, exemplars}]` to stdout, sorted by
//! `(subproject, slug)` for byte-stable output. Fail-open: a missing or
//! unparseable model degrades to `[]` and exit 0 — the enrich step then skips
//! silently, exactly like Guards on an empty worklist.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Minimum members a role cluster must have before it earns a mold — a mold
/// teaches a *recurring* convention, and fewer than three files is not yet one.
const MIN_CLUSTER: usize = 3;

/// How many exemplar files a mold candidate carries — enough for the agent to
/// read the shared shape, not so many the dispatch prompt bloats.
const MAX_EXEMPLARS: usize = 3;

/// Minimum exemplar files a cluster must have before it earns a mold — fewer
/// than two files cannot show a *recurring* shape, whichever signature resolved
/// them ([`exemplars_for`]: filename affix, declaration affix, or — for a
/// `folder` role, whose affix is absent from every name by construction —
/// residence in the role's home).
///
/// This is a LIVENESS bar, not a naming bar: it proves the cluster still has
/// hand-written files worth reading. Whether their shape is worth TEACHING is
/// the enrich agent's judgment ([`super::decline`]), never this constant's.
/// (It once claimed to resolve exemplars by filename ONLY, and that a
/// declaration affix "must not spawn a mold" — while the fallback right below
/// exists precisely to let it. Both halves of that claim were false.)
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
    /// The miner's per-entity SLICES — role sets that recur together across
    /// entities. Read only to RANK the worklist ([`recurrence_by_role`]), never
    /// to filter it: belonging to a slice proves a role is useful, but NOT
    /// belonging proves nothing, because a cross-cutting convention (a
    /// `middleware`, a `store`) is real and teachable yet exists once, not once
    /// per entity. The signal is asymmetric; a filter would treat it as
    /// symmetric. Measured on a real workspace: filtering on it would have
    /// killed 11 of 38 live molds.
    conventions: Vec<Convention>,
}

/// One mined convention slice — only the role membership and how many entities
/// repeat it matter here.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Convention {
    roles: Vec<String>,
    optional_roles: Vec<String>,
    /// How many entities repeat this slice — the rank signal.
    recurrence: usize,
}

/// A mined role affix (from `roles[]`) — the "how we write an X" cluster.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Role {
    affix: String,
    /// How the affix attaches, verbatim from the miner — FOUR values, not two:
    /// `"suffix"` / `"prefix"` (the affix lives in the symbol NAME), `"nested"`
    /// (a bare recurring declaration inside a role folder), and `"folder"` (the
    /// affix IS the folder name, so by construction NOTHING in the filename
    /// carries it). A consumer MUST branch on all four: this doc once read
    /// `"suffix" | "prefix"`, [`matches_affix`] was written to match it, and
    /// every `folder` cluster was silently discarded for years — the doc drift
    /// WAS the defect. A kind minted after this code degrades to a permissive
    /// test rather than dropping the cluster.
    kind: String,
    count: usize,
    common_dir: String,
    /// EVERY recurring home of the role (miner's `dirs`, additive — empty on
    /// older models). A convention spread across parents keeps them all;
    /// resolving against `common_dir` alone loses every exemplar outside the
    /// densest one.
    dirs: Vec<String>,
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
    /// The module's mined declarations — the fallback exemplar signal for
    /// clusters whose convention lives in DECLARATION names, not filenames
    /// (`configs/contracts.ts` declaring `contractsConfig`).
    declarations: Vec<Decl>,
}

/// One mined declaration (only the name matters here).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Decl {
    name: String,
}

/// A workspace subproject (one per build manifest).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Proj {
    name: String,
    dir: String,
}

/// One mold-candidate worklist entry, serialised to the JSON the orchestrator
/// hands (per subproject) to the `mustard-patterns` agent. Crate-visible so
/// `agent-prompt-render --role patterns` can materialise the SAME worklist
/// into the dispatched `## TASK` (single source — never a re-derivation).
#[derive(Serialize)]
pub(crate) struct Candidate {
    /// Subproject directory (forward-slashed, relative to root).
    pub(crate) subproject: String,
    /// Cluster label — the `appliesTo`/`cluster.label` the mold carries (the
    /// role, e.g. `service`).
    pub(crate) label: String,
    /// Mold slug — `{subproject-basename}-{label}`; the skill folder is
    /// `{slug}-pattern`. Matches the existing convention (`scan-stage`,
    /// `rt-inject`).
    pub(crate) slug: String,
    /// Where the agent's SKILL.md is written (`scan-patterns-apply --path`).
    #[serde(rename = "moldPath")]
    pub(crate) mold_path: String,
    pub(crate) affix: String,
    #[serde(rename = "affixKind")]
    pub(crate) affix_kind: String,
    #[serde(rename = "declKind")]
    pub(crate) decl_kind: String,
    pub(crate) implements: Option<String>,
    pub(crate) count: usize,
    /// 1-3 real hand-written files of the cluster the agent must read.
    pub(crate) exemplars: Vec<String>,
    /// Slice recurrence ([`recurrence_by_role`]) — READ ORDER only, never a
    /// gate. Not serialised: it steers which candidate the agent reads first
    /// and is not part of the worklist contract.
    #[serde(skip)]
    pub(crate) rank: usize,
}

/// One role the worklist did NOT propose, with the reason it was dropped —
/// the `--rejected` diagnostic ([`collect_rejected`]).
#[derive(Serialize)]
pub(crate) struct Rejection {
    pub(crate) affix: String,
    pub(crate) kind: String,
    pub(crate) count: usize,
    #[serde(rename = "commonDir")]
    pub(crate) common_dir: String,
    /// Owning subproject dir, or empty when the role has no named owner.
    pub(crate) subproject: String,
    /// Closed vocabulary — one per drop point in [`collect_inner`].
    pub(crate) reason: &'static str,
}

/// Run `scan-patterns-list`. Prints a JSON array to stdout; exit 0 always.
/// `rejected` swaps the payload for the drop diagnostic; the default output is
/// byte-identical to before the flag existed (the `/scan` flow consumes it).
pub fn run(root: &Path, rejected: bool) {
    // `to_string` cannot fail for these shapes; fall back to `[]` defensively.
    let json = if rejected {
        serde_json::to_string(&collect_rejected(root))
    } else {
        serde_json::to_string(&collect(root))
    };
    println!("{}", json.unwrap_or_else(|_| "[]".to_string()));
}

/// The `--rejected` twin of [`collect`]: every role the worklist dropped, with
/// its reason. Sorted by `(reason, affix)` for byte-stable output.
///
/// Exists because [`collect_inner`]'s drop points are otherwise SILENT — a role
/// that does not become a candidate simply never appears, with no log and no
/// count. That is exactly how a whole family of folder-borne conventions stayed
/// dead without anyone noticing: the only way to learn why a project has no mold
/// for X was to re-read this source file. Measured on a real workspace: 34% of
/// eligible roles were dropped mutely.
pub(crate) fn collect_rejected(root: &Path) -> Vec<Rejection> {
    let mut rejections = collect_inner(root).1;
    rejections.sort_by(|a, b| a.reason.cmp(b.reason).then(a.affix.cmp(&b.affix)));
    rejections
}

/// The testable core of [`run`]: read the model and derive the sorted mold
/// worklist. Fail-open — any load/parse failure yields an empty worklist.
/// Crate-visible because `agent-prompt-render --role patterns` reuses it
/// in-process to embed the per-subproject worklist in the dispatch prompt.
pub(crate) fn collect(root: &Path) -> Vec<Candidate> {
    collect_inner(root).0
}

/// The shared engine of [`collect`] and [`collect_rejected`]: one pass that
/// yields the proposed candidates AND the roles it dropped with their reason.
/// Both faces derive from the SAME walk — a diagnostic that re-implemented the
/// funnel could disagree with the funnel, which would defeat its purpose.
fn collect_inner(root: &Path) -> (Vec<Candidate>, Vec<Rejection>) {
    let mut rejected: Vec<Rejection> = Vec::new();
    let model_path = root.join(".claude").join("grain.model.json");
    let Ok(text) = std::fs::read_to_string(&model_path) else {
        return (Vec::new(), rejected);
    };
    let Ok(model) = serde_json::from_str::<Model>(&text) else {
        return (Vec::new(), rejected);
    };

    // Subprojects with a non-empty dir, longest dir first so `common_dir` is
    // attributed to its most-specific owner (the root unit, dir "", is excluded —
    // molds are never authored for the workspace root).
    let mut projects: Vec<&Proj> = model.projects.iter().filter(|p| !p.dir.is_empty()).collect();
    projects.sort_by(|a, b| b.dir.len().cmp(&a.dir.len()).then(a.dir.cmp(&b.dir)));

    // Module paths sorted once — every exemplar scan reads this in a stable order.
    let mut modules: Vec<&Mod> = model.modules.iter().collect();
    modules.sort_by(|a, b| a.path.cmp(&b.path));

    // Slugs the enrich agent already refused with a recorded reason — a dead
    // candidate must not burn a dispatch on every scan.
    let declined = super::decline::declined(root);
    // Read ORDER only — never a gate. See `recurrence_by_role`.
    let rank_of = recurrence_by_role(&model.conventions);

    // Record a drop with its reason, so `--rejected` can answer "why is there no
    // mold for X?" without anyone re-reading this funnel.
    let mut drop = |role: &Role, subproject: &str, reason: &'static str| {
        rejected.push(Rejection {
            affix: role.affix.clone(),
            kind: role.kind.clone(),
            count: role.count,
            common_dir: role.common_dir.clone(),
            subproject: subproject.to_string(),
            reason,
        });
    };

    let mut candidates: Vec<Candidate> = Vec::new();
    for role in &model.roles {
        if role.common_dir.is_empty() {
            drop(role, "", "no_common_dir");
            continue;
        }
        if role.count < MIN_CLUSTER {
            drop(role, "", "below_cluster_min");
            continue;
        }
        if under_test(&role.common_dir) {
            drop(role, "", "test_terrain");
            continue;
        }
        let Some(project) = owner_of(&role.common_dir, &projects) else {
            drop(role, "", "no_owner"); // root-level: outside any named subproject.
            continue;
        };
        let label = slugify(&role.affix);
        if label.is_empty() {
            drop(role, &project.dir, "empty_label");
            continue;
        }
        // Lower-kebab the subproject short name too, so a PascalCase C# unit
        // (`DataAccess`) yields a consistent `dataaccess-<role>-pattern` folder.
        let subproj = slugify(basename(&project.dir));
        if subproj.is_empty() {
            drop(role, &project.dir, "empty_label");
            continue;
        }
        let slug = format!("{subproj}-{label}");
        if declined.contains_key(&slug) {
            drop(role, &project.dir, "declined"); // agent refused, with a recorded reason.
            continue;
        }
        let mold_path = format!("{}/.claude/skills/{}-pattern/SKILL.md", project.dir, slug);
        // Any surviving mold (this slug, or another `*-{label}-pattern` claiming
        // the role) is preserved — the sweep already deleted the mustard-
        // generated ones, so whatever remains is hand-authored/adopted.
        if mold_present(root, &project.dir, &slug, &label) {
            drop(role, &project.dir, "mold_exists");
            continue;
        }
        // Every home of the role that belongs to THIS subproject and is not
        // test terrain — exemplars may live in any of them.
        let homes: Vec<&str> = std::iter::once(role.common_dir.as_str())
            .chain(role.dirs.iter().map(String::as_str))
            .filter(|d| !under_test(d))
            .filter(|d| *d == project.dir || d.starts_with(&format!("{}/", project.dir)))
            .collect();
        let exemplars = exemplars_for(role, &modules, &homes);
        if exemplars.len() < MIN_EXEMPLARS {
            drop(role, &project.dir, "no_exemplars"); // nothing hand-written left to read.
            continue;
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
            rank: rank_of.get(&role.affix.to_lowercase()).copied().unwrap_or(0),
        });
    }

    // Byte-stable order, now rank-first: within a subproject the agent reads the
    // strongest convention first (highest slice recurrence), and an unranked role
    // reads last WITHOUT being excluded. Slug breaks ties, so the order stays
    // total and deterministic.
    candidates.sort_by(|a, b| {
        a.subproject.cmp(&b.subproject).then(b.rank.cmp(&a.rank)).then(a.slug.cmp(&b.slug))
    });
    (candidates, rejected)
}

/// Highest slice recurrence per role affix (lowercased), from the miner's
/// `conventions[]`: how many entities repeat the slice this role belongs to.
/// Absent role → 0.
///
/// RANKING ONLY. A role scoring 0 still earns its candidacy and stays in the
/// worklist; it just reads last. The temptation to drop it was measured and
/// refuted: `conventions[]` mines PER-ENTITY slices, so a cross-cutting role
/// (`middleware`, `store`, `filter`) scores 0 while being a perfectly real
/// convention — on a real workspace, filtering on this signal would have killed
/// 11 of 38 live molds. Separating a useful orphan from a useless one requires
/// READING the files, which is the enrich agent's job, not this function's.
fn recurrence_by_role(conventions: &[Convention]) -> HashMap<String, usize> {
    let mut best: HashMap<String, usize> = HashMap::new();
    for conv in conventions {
        for affix in conv.roles.iter().chain(conv.optional_roles.iter()) {
            let key = affix.to_lowercase();
            let slot = best.entry(key).or_insert(0);
            *slot = (*slot).max(conv.recurrence);
        }
    }
    best
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

/// Whether ANY mold already claims this cluster under the subproject's
/// `.claude/skills/` — the exact `{slug}-pattern` folder, or another
/// `*-pattern` folder ending in `-{label}-pattern` / equal to `{label}-pattern`
/// (the same role under a different subproject prefix). Post-sweep, a surviving
/// mold is hand-authored/adopted (`source: manual`); the mustard-generated ones
/// were already deleted, so presence alone means "preserve, do not re-author".
fn mold_present(root: &Path, subproject: &str, slug: &str, label: &str) -> bool {
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
/// files *directly* in its `common_dir` (grain's role→folder map) that carry
/// the affix. Two accepted signatures, tried in order:
///
/// 1. **Filename affix** — the file's stem carries it (`UserService.ts`,
///    `OrderService.ts`): the classic file-naming convention.
/// 2. **Declaration affix** (fallback, when 1 resolves fewer than
///    [`MIN_EXEMPLARS`]) — the file DECLARES a symbol carrying it
///    (`configs/contracts.ts` declaring `contractsConfig`; `_components/form/`
///    files declaring `ClientForm`). The role was mined from declarations, so
///    resolving exemplars by the same signal is symmetric — without this, a
///    real per-entity convention whose filenames vary never earns a mold (the
///    defect that hid the entity-config/entity-form conventions in field
///    validation).
///
/// In both signatures the LOCATION bar is the same: the file must sit directly
/// in ONE OF the role's homes (`homes`: its `common_dir` plus every recurring
/// `dirs` entry the caller kept) — each expanded by [`dir_matches`] when the
/// miner generalized it (`Modules/v1/<Name>s/Services`); without that
/// expansion every feature-folder convention (the whole .NET `Modules/{F}/…`
/// backend) is silently discarded, and without the extra homes a convention
/// spread across parents loses every exemplar outside the densest one.
/// `modules` is pre-sorted by path, so the pick is stable; generated/vendored
/// code never teaches, so it is skipped.
fn exemplars_for(role: &Role, modules: &[&Mod], homes: &[&str]) -> Vec<String> {
    let affix = role.affix.to_lowercase();
    let located: Vec<&&Mod> = modules
        .iter()
        .filter(|m| m.file_class.is_empty()) // hand-written only
        // Test terrain is excluded by HOME (`collect`), deliberately NOT by
        // re-scanning the file's own path. Tempting as that re-check looks —
        // the home is abstracted, so `.../<Name>s/…` could seem to hide a
        // `Tests` segment — it is wrong: `abstract_entity` only replaces the
        // segment that IS the entity, so a wildcarded `Tests`/`specs` names a
        // DOMAIN entity, not a fixture dir. A segment that is genuinely test
        // terrain is not the entity, stays literal in the home, and `under_test`
        // already drops it there. Re-checking the module path instead punishes
        // real production files for the entity they belong to: it silently
        // killed the `Tab` cluster, whose exemplars live under the `specs`
        // FEATURE (`features/specs/SpecTabBar/`).
        .filter(|m| homes.iter().any(|h| dir_matches(parent_dir(&m.path), h)))
        .collect();

    let by_filename: Vec<String> = located
        .iter()
        .filter(|m| matches_affix(stem(&m.path), &affix, &role.kind))
        .map(|m| m.path.clone())
        .take(MAX_EXEMPLARS)
        .collect();
    if by_filename.len() >= MIN_EXEMPLARS {
        return by_filename;
    }

    // Fallback: the convention lives in the declaration names.
    located
        .iter()
        .filter(|m| {
            m.declarations
                .iter()
                .any(|d| matches_affix(d.name.to_lowercase(), &affix, &role.kind))
        })
        .map(|m| m.path.clone())
        .take(MAX_EXEMPLARS)
        .collect()
}

/// Whether `actual` (a real parent dir) matches `pattern` (a role `common_dir`
/// that may carry generalized `<…>` placeholder segments). Segment-for-segment:
/// same segment count, and a `pattern` segment containing `<` is a wildcard
/// (matches any real segment); every other segment must be equal. No regex —
/// the placeholder always occupies a whole segment (`<Name>s`, `<name>`), so a
/// segment-level wildcard is exact for the miner's output and cheap.
fn dir_matches(actual: &str, pattern: &str) -> bool {
    if !pattern.contains('<') {
        return actual == pattern; // fast path: literal common_dir
    }
    let a: Vec<&str> = actual.split('/').collect();
    let p: Vec<&str> = pattern.split('/').collect();
    if a.len() != p.len() {
        return false;
    }
    a.iter().zip(p.iter()).all(|(seg, pat)| pat.contains('<') || seg == pat)
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

/// Whether `stem` carries `affix` in the position `kind` implies — one arm per
/// signal the miner mints, so `_` means only "a kind minted after this code".
///
/// `folder` is the arm this function was missing, and its absence silently
/// discarded EVERY folder-borne convention: a folder role's affix is the
/// DIRECTORY name, which by construction is absent from the filename — were it
/// present the miner would have mined it as a `suffix`. Testing the name asks a
/// question that cannot be answered yes, so both exemplar passes resolved 0 and
/// the cluster died at [`MIN_EXEMPLARS`]. Survival was a naming lottery: a role
/// folder whose files redundantly echo it (`EndPoints/BankEndPoints.cs`) passed
/// the `contains` test by luck, while `GraphQL/BankQueryResolver.cs` and
/// `_components/bank-form.tsx` — identical in kind — did not.
///
/// For a folder role RESIDENCE IS THE CONVENTION, and [`exemplars_for`] already
/// proved it against `homes` using the very predicate the miner used to assign
/// the role. So there is nothing left to test here: the name check was not just
/// wrong, it was redundant.
fn matches_affix(stem: String, affix: &str, kind: &str) -> bool {
    match kind {
        "suffix" => stem.ends_with(affix),
        "prefix" => stem.starts_with(affix),
        // A bare recurring declaration inside a role folder: the affix IS the
        // whole name, but the filename may differ (`[id]/route.ts` declaring
        // `PUT`), so keep the permissive test the declaration pass relies on.
        "nested" => stem.contains(affix),
        "folder" => true,
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
    fn collect_emits_every_cluster_uncapped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Six clusters in one subproject, each with real exemplars — ALL are
        // proposed (no per-subproject cap; the quality bars are the only filter).
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
        let slugs: Vec<&str> = got.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, vec!["api-a", "api-b", "api-c", "api-d", "api-e", "api-f"], "every cluster survives, sorted");
    }

    #[test]
    fn collect_preserves_any_surviving_mold() {
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
        // Any mold surviving the sweep (hand-authored/adopted) is preserved —
        // the list never re-authors over an existing folder. (The generated
        // ones were already deleted by the sweep before list ran.)
        let mold_dir = root.join("apps/api/.claude/skills/api-service-pattern");
        std::fs::create_dir_all(&mold_dir).unwrap();
        std::fs::write(mold_dir.join("SKILL.md"), "---\nname: api-service-pattern\nsource: manual\n---\nhand\n").unwrap();
        assert!(collect(root).is_empty(), "a surviving mold is never re-proposed");
    }

    #[test]
    fn dir_matches_expands_generalized_segments() {
        // Literal fast path.
        assert!(dir_matches("app/services", "app/services"));
        assert!(!dir_matches("app/services", "app/repos"));
        // Placeholder segment is a wildcard; literal segments must still match.
        assert!(dir_matches("app/modules/contracts/services", "app/modules/<Name>s/services"));
        assert!(dir_matches("app/modules/partners/services", "app/modules/<Name>s/services"));
        assert!(!dir_matches("app/modules/contracts/repos", "app/modules/<Name>s/services"), "trailing literal differs");
        // Segment count must match.
        assert!(!dir_matches("app/modules/contracts/sub/services", "app/modules/<Name>s/services"));
    }

    #[test]
    fn collect_resolves_exemplars_across_all_role_homes() {
        // A convention SPREAD across parents: the miner's `common_dir` points
        // at the densest home (`app/configs`, 1 file) but the real per-entity
        // convention lives in the OTHER home (`(dashboard)/<name>s/config.tsx`,
        // one per entity). With `dirs` carried in the model, the resolver must
        // find the exemplars there — the exact shape that kept entity-config
        // underivable in field validation.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"app","dir":"app"}],
              "roles": [{"affix":"Config","kind":"suffix","count":49,"common_dir":"app/configs","dirs":["app/(dashboard)/<name>s"]}],
              "modules": [
                {"path":"app/configs/entity-picker-configs.ts","declarations":[{"kind":"const","name":"entityPickerRegistry"}]},
                {"path":"app/(dashboard)/contracts/config.tsx"},
                {"path":"app/(dashboard)/clients/config.tsx"}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "multi-home cluster earns a mold");
        assert_eq!(got[0].slug, "app-config");
        assert_eq!(
            got[0].exemplars,
            vec!["app/(dashboard)/clients/config.tsx", "app/(dashboard)/contracts/config.tsx"],
            "exemplars resolve in the non-dominant home"
        );
    }

    #[test]
    fn collect_resolves_exemplars_by_declaration_affix() {
        // The convention lives in DECLARATION names, not filenames: a flat
        // `configs/` folder whose files are named by entity (`contracts.ts`)
        // but each declares a `*Config` symbol. The filename pass finds 0; the
        // declaration fallback must resolve them — this exact shape (49-file
        // Config cluster) was silently dropped in field validation.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"app","dir":"app"}],
              "roles": [{"affix":"Config","kind":"suffix","count":49,"common_dir":"app/configs"}],
              "modules": [
                {"path":"app/configs/contracts.ts","declarations":[{"kind":"const","name":"contractsConfig"}]},
                {"path":"app/configs/clients.ts","declarations":[{"kind":"const","name":"clientsConfig"}]},
                {"path":"app/configs/index.ts","declarations":[{"kind":"const","name":"registry"}]}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "declaration-affix cluster earns a mold");
        assert_eq!(got[0].slug, "app-config");
        // Only the files whose declarations carry the affix — index.ts is out.
        assert_eq!(got[0].exemplars, vec!["app/configs/clients.ts", "app/configs/contracts.ts"]);
    }

    #[test]
    fn filename_signature_wins_over_declaration_fallback() {
        // When ≥2 filenames carry the affix, the classic signature is used and
        // the fallback never fires (a README-style neighbour with a matching
        // declaration cannot dilute the exemplar set).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [{"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services"}],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"},
                {"path":"apps/api/services/helpers.ts","declarations":[{"kind":"fn","name":"buildService"}]}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].exemplars,
            vec!["apps/api/services/OrderService.ts", "apps/api/services/UserService.ts"],
            "filename signature only — helpers.ts stays out"
        );
    }

    #[test]
    fn collect_resolves_exemplars_under_generalized_common_dir() {
        // The bug that hid the whole feature-folder backend: a role whose
        // common_dir carries the miner's `<Name>s` placeholder must still
        // resolve its real exemplars (which live in per-feature subfolders).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"app","dir":"app"}],
              "roles": [{"affix":"Service","kind":"suffix","count":9,"common_dir":"app/modules/<Name>s/services"}],
              "modules": [
                {"path":"app/modules/contracts/services/ContractService.cs"},
                {"path":"app/modules/partners/services/PartnerService.cs"}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "generalized cluster resolves and becomes a candidate");
        assert_eq!(got[0].slug, "app-service");
        assert_eq!(
            got[0].exemplars,
            vec!["app/modules/contracts/services/ContractService.cs", "app/modules/partners/services/PartnerService.cs"]
        );
    }

    #[test]
    fn collect_excludes_declined_slugs() {
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
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            root.join(".claude/scan-declined.json"),
            r#"{"api-service":"covered by another mold"}"#,
        )
        .unwrap();
        assert!(collect(root).is_empty(), "a recorded decline stops the re-proposal");
    }

    #[test]
    fn collect_fail_open_on_missing_model() {
        let dir = tempfile::tempdir().unwrap();
        assert!(collect(dir.path()).is_empty(), "missing model → empty worklist, never a panic");
    }

    #[test]
    fn collect_resolves_folder_role_exemplars_by_location() {
        // A FOLDER role's affix is the DIRECTORY name, so by construction no
        // filename and no declaration carries it. Both exemplar passes used to
        // run `contains(affix)` against the folder name, resolved 0, and the
        // cluster died mutely at MIN_EXEMPLARS — every folder-borne convention
        // was gated on the coincidence of a redundantly-named file.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [{"affix":"Inbound","kind":"folder","count":37,
                         "common_dir":"apps/api/modules/<Name>s/Inbound","decl_kind":"class"}],
              "modules": [
                {"path":"apps/api/modules/banks/Inbound/BankQueryHandler.cs",
                 "declarations":[{"kind":"class","name":"BankQueryHandler"}]},
                {"path":"apps/api/modules/clients/Inbound/ClientQueryHandler.cs",
                 "declarations":[{"kind":"class","name":"ClientQueryHandler"}]}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "a folder role earns a mold: location IS the convention");
        assert_eq!(got[0].slug, "api-inbound");
        assert_eq!(got[0].affix_kind, "folder");
        assert_eq!(
            got[0].exemplars,
            vec![
                "apps/api/modules/banks/Inbound/BankQueryHandler.cs",
                "apps/api/modules/clients/Inbound/ClientQueryHandler.cs"
            ],
            "neither filename nor declaration carries `Inbound` — residence resolved them"
        );
    }

    #[test]
    fn folder_role_exemplars_never_leave_the_role_home() {
        // `folder => true` waives the NAME test, so `homes`/`dir_matches` is the
        // only gate left. A sibling role folder and a deeper subfolder must both
        // stay out, even sitting under the same feature parent.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [{"affix":"Inbound","kind":"folder","count":9,
                         "common_dir":"apps/api/modules/<Name>s/Inbound"}],
              "modules": [
                {"path":"apps/api/modules/banks/Inbound/BankQueryHandler.cs"},
                {"path":"apps/api/modules/banks/Inbound/internal/Helper.cs"},
                {"path":"apps/api/modules/banks/Store/BankStore.cs"},
                {"path":"apps/api/modules/clients/Inbound/ClientQueryHandler.cs"}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].exemplars,
            vec![
                "apps/api/modules/banks/Inbound/BankQueryHandler.cs",
                "apps/api/modules/clients/Inbound/ClientQueryHandler.cs"
            ],
            "sibling folder and deeper subfolder are not the role's home"
        );
    }

    #[test]
    fn folder_role_never_teaches_from_generated_code_nor_a_test_home() {
        // A mold teaches PRODUCTION convention, and the affix test is waived for
        // a folder role, so the two live gates are: hand-written only, and the
        // role's home (a test home is dropped in `collect`, before this).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [
                {"affix":"Inbound","kind":"folder","count":9,
                 "common_dir":"apps/api/modules/<Name>s/Inbound"},
                {"affix":"Stub","kind":"folder","count":9,
                 "common_dir":"apps/api/tests/<Name>s/Stub"}
              ],
              "modules": [
                {"path":"apps/api/modules/banks/Inbound/BankQueryHandler.cs"},
                {"path":"apps/api/modules/clients/Inbound/ClientQueryHandler.cs"},
                {"path":"apps/api/modules/orders/Inbound/OrderHandler.cs","file_class":"generated"},
                {"path":"apps/api/tests/banks/Stub/BankStub.cs"},
                {"path":"apps/api/tests/clients/Stub/ClientStub.cs"}
              ]
            }"#,
        );
        let got = collect(root);
        let slugs: Vec<&str> = got.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, vec!["api-inbound"], "a role homed in test terrain never earns a mold");
        assert_eq!(
            got[0].exemplars,
            vec![
                "apps/api/modules/banks/Inbound/BankQueryHandler.cs",
                "apps/api/modules/clients/Inbound/ClientQueryHandler.cs"
            ],
            "generated code never teaches convention"
        );
    }

    #[test]
    fn an_entity_named_like_test_terrain_still_teaches() {
        // `abstract_entity` only wildcards the segment that IS the entity, so a
        // `<Name>s` standing over `specs`/`tests` names a DOMAIN entity, not a
        // fixture dir — genuinely test terrain is not the entity, stays literal
        // in the home, and is dropped there. Re-checking the module's own path
        // for test segments therefore punishes production files for the entity
        // they belong to: it silently killed the real `Tab` cluster, whose
        // exemplars live under the `specs` FEATURE.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"app","dir":"apps/app"}],
              "roles": [{"affix":"TabBar","kind":"suffix","count":3,
                         "common_dir":"apps/app/features/<name>s/<Name>TabBar"}],
              "modules": [
                {"path":"apps/app/features/specs/SpecTabBar/index.tsx",
                 "declarations":[{"kind":"function","name":"SpecTabBar"}]},
                {"path":"apps/app/features/runs/RunTabBar/index.tsx",
                 "declarations":[{"kind":"function","name":"RunTabBar"}]}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "the `specs` entity is domain, not test terrain");
        assert_eq!(
            got[0].exemplars,
            vec!["apps/app/features/runs/RunTabBar/index.tsx", "apps/app/features/specs/SpecTabBar/index.tsx"],
            "a file is not a fixture because its ENTITY is spelled like one"
        );
    }

    #[test]
    fn matches_affix_honours_every_mined_kind() {
        // suffix/prefix keep their POSITION semantics — the fix must not loosen them.
        assert!(matches_affix("userservice".into(), "service", "suffix"));
        assert!(!matches_affix("servicebroker".into(), "service", "suffix"), "not a suffix");
        assert!(matches_affix("usebank".into(), "use", "prefix"));
        assert!(!matches_affix("bankuse".into(), "use", "prefix"), "not a prefix");
        // A folder role's affix is the DIRECTORY name — never in the stem. There
        // is no name test to run; `exemplars_for` already proved residence.
        assert!(matches_affix("bankqueryhandler".into(), "inbound", "folder"));
        // A bare recurring declaration keeps the permissive contains test…
        assert!(matches_affix("put".into(), "put", "nested"));
        assert!(!matches_affix("bank".into(), "put", "nested"));
        // …and so does any kind the miner grows after this code was written.
        assert!(matches_affix("anything".into(), "any", "some-future-kind"));
    }

    #[test]
    fn rank_orders_the_worklist_but_never_filters_it() {
        // Slice recurrence steers READ ORDER only. An orphan role (in no
        // convention) still earns candidacy — it just reads last. Filtering on
        // this signal was measured and refuted: `conventions[]` mines PER-ENTITY
        // slices, so a cross-cutting role scores 0 while being a real
        // convention, and dropping it kills good molds.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "conventions": [
                {"roles":["Inbound"],"optional_roles":["Store"],"recurrence":33}
              ],
              "roles": [
                {"affix":"Widget","kind":"suffix","count":4,"common_dir":"apps/api/widgets"},
                {"affix":"Store","kind":"suffix","count":5,"common_dir":"apps/api/stores"},
                {"affix":"Inbound","kind":"folder","count":9,"common_dir":"apps/api/modules/<Name>s/Inbound"}
              ],
              "modules": [
                {"path":"apps/api/widgets/AlphaWidget.cs"},
                {"path":"apps/api/widgets/BetaWidget.cs"},
                {"path":"apps/api/stores/AlphaStore.cs"},
                {"path":"apps/api/stores/BetaStore.cs"},
                {"path":"apps/api/modules/banks/Inbound/BankQueryHandler.cs"},
                {"path":"apps/api/modules/clients/Inbound/ClientQueryHandler.cs"}
              ]
            }"#,
        );
        let got = collect(root);
        let slugs: Vec<&str> = got.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(
            slugs,
            vec!["api-inbound", "api-store", "api-widget"],
            "ranked by slice recurrence (33, 33, 0); the orphan `widget` SURVIVES, last"
        );
        assert_eq!(got[2].rank, 0, "orphan scores 0 and is still proposed");
        // Ties fall back to slug, so the order stays total and deterministic.
        let a = serde_json::to_string(&collect(root)).unwrap();
        let b = serde_json::to_string(&collect(root)).unwrap();
        assert_eq!(a, b, "two runs must produce identical bytes");
        assert!(!a.contains("\"rank\""), "rank steers order; it is not part of the contract");
    }

    #[test]
    fn rejected_reports_every_silent_drop_with_its_reason() {
        // The funnel's drop points are otherwise mute — that muteness is how a
        // whole family of conventions stayed dead unnoticed.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [
                {"affix":"Small","kind":"suffix","count":2,"common_dir":"apps/api/small"},
                {"affix":"Fixture","kind":"suffix","count":9,"common_dir":"apps/api/tests/fixtures"},
                {"affix":"Homeless","kind":"suffix","count":9,"common_dir":""},
                {"affix":"Stray","kind":"suffix","count":9,"common_dir":"elsewhere/stray"},
                {"affix":"Client","kind":"suffix","count":8,"common_dir":"apps/api/gen"}
              ],
              "modules": [
                {"path":"apps/api/gen/UserClient.ts","file_class":"generated"}
              ]
            }"#,
        );
        let got = collect_rejected(root);
        let pairs: Vec<(&str, &str)> = got.iter().map(|r| (r.affix.as_str(), r.reason)).collect();
        assert_eq!(
            pairs,
            vec![
                ("Small", "below_cluster_min"),
                ("Homeless", "no_common_dir"),
                ("Client", "no_exemplars"),
                ("Stray", "no_owner"),
                ("Fixture", "test_terrain"),
            ],
            "every drop names itself, sorted by (reason, affix)"
        );
        assert!(collect(root).is_empty(), "and none of them became a candidate");
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
