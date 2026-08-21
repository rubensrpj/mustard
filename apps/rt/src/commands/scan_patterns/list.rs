//! `scan-patterns-list` — derive the missing pattern-skill *mold* worklist from
//! `grain.model.json` and emit it as a JSON array for the enrich agent.
//!
//! This is the pattern-mold twin of `scan-guards-list`. Where Guards walks the
//! `CLAUDE.md` tree, patterns projects FROM the deterministic model: for each
//! mined role cluster (`roles[]`) with at least [`MIN_CLUSTER`] members, it
//! resolves the cluster's real hand-written exemplars (generated/vendored code
//! never teaches convention), attributes them to the subprojects they actually
//! live in, and proposes one `{subproject-basename}-{role}-pattern` mold per
//! house that holds enough of them. The houses come from `projects[]` (the
//! build manifests) and, ONLY when that list names none, from `skeleton[]` (the
//! mined architectural units) — a repository whose single manifest sits at the
//! root has no house of the first kind and would otherwise teach nothing at
//! all. Machine-authored molds stay FRESH by
//! DELETION, not by refresh: [`super::sweep`] removes every `source: scan` mold
//! before any authoring runs, so by the time this worklist is built every
//! proposal is a create. A hand-edited or `source: manual` mold survives the
//! sweep and is never re-proposed ([`super::origin`]), and a slug recorded in
//! `.claude/scan-declined.json` ([`super::decline`]) is skipped entirely.
//! Uncapped — every cluster clearing the quality bars is proposed.
//!
//! Output: a JSON array `[{subproject, label, slug, moldPath, affix, affixKind,
//! declKind, implements, count, exemplars}]` to stdout, ordered by
//! `(subproject, rank desc, slug)` — byte-stable, and within a house the
//! strongest convention reads first. Fail-open: a missing or unparseable model
//! degrades to `[]` and exit 0 — the enrich step then skips silently, exactly
//! like Guards on an empty worklist.

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
///
/// The same floor re-earns a role's `implements` hint per house
/// ([`confirm_implements`]): fewer than two evidencing files cannot show a
/// shared contract either.
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
    /// The miner's ARCHITECTURAL units — one row per `{first}/{second}` path
    /// segment pair, biggest first, truncated at 25 by `build_skeleton`. Read
    /// only as the FALLBACK owner list ([`collect_inner`]): `projects[]` comes
    /// from build manifests, so a repository with a single root manifest offers
    /// no house at all and every cluster dies ownerless, while this list still
    /// names `src/sira`, `src/mlplan`, `src/core`. Additive — an older model
    /// without the key degrades to an empty vec, which is the same empty owner
    /// list as before.
    skeleton: Vec<Skel>,
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
    /// The FAMILY's contract — the supertype a repo-wide majority of the role
    /// shares (the miner's majority gate). Re-earned against each house's own
    /// files before it reaches a candidate ([`confirm_implements`]).
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

/// One mined declaration — the name (the declaration-affix exemplar signal) and
/// the supertypes it builds on (the house-local evidence a role's `implements`
/// hint is validated against — [`confirm_implements`]).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Decl {
    name: String,
    supertypes: Vec<String>,
}

/// A workspace subproject (one per build manifest).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Proj {
    name: String,
    dir: String,
}

/// One `skeleton[]` row — a mined architectural unit, keyed by its directory.
/// Only `dir` is consumed here; the row's `role`/`files` are census signals the
/// ownership question has no use for.
#[derive(Deserialize)]
struct Skel {
    #[serde(default)]
    dir: String,
}

/// The `skeleton[]` bucket `build_skeleton` gives modules that live at the
/// repository ROOT (a path with no directory segment). It is a label, not a
/// directory, so it can never own a mold — `root.join("(root)")` is a path that
/// does not exist, and the root is the one house molds are never authored for.
const SKELETON_ROOT: &str = "(root)";

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
    /// The family's `implements` hint validated against THIS house
    /// ([`confirm_implements`]): `None` when fewer than [`MIN_EXEMPLARS`] of the
    /// house's own files build on it; kept untouched when the model carries no
    /// inheritance data to judge with.
    pub(crate) implements: Option<String>,
    /// How many hand-written files of this role live in THIS subproject — not
    /// the role's repo-wide tally. A role spread over several subprojects would
    /// otherwise tell each house it holds all the files, and the agent would
    /// author a mold claiming a recurrence this house does not have.
    pub(crate) count: usize,
    /// 1-3 real hand-written files of the cluster the agent must read.
    pub(crate) exemplars: Vec<String>,
    /// Globs for the mold's `paths:` frontmatter — the ONE key the platform
    /// honours to scope when a skill is loaded. Derived by [`globs_for`] from
    /// the cluster's real files, never from the role's `common_dir` (the miner
    /// abstracts that one, so it can carry a `<name>` placeholder no glob
    /// engine understands) and never from a folder name this code recognises.
    pub(crate) paths: Vec<String>,
    /// Every hand-written file of the cluster in this house, in resolution
    /// order — the set `count`/`exemplars` are derived from. Not serialised:
    /// it exists so [`fold_collisions`] can UNION two case-variants' files and
    /// count the union, since their sets overlap (one file can declare both
    /// `struct FooReport` and `fn foo_report`).
    #[serde(skip)]
    pub(crate) files: Vec<String>,
    /// Slice recurrence ([`recurrence_by_role`]) — READ ORDER only, never a
    /// gate. Not serialised: it steers which candidate the agent reads first
    /// and is not part of the worklist contract.
    #[serde(skip)]
    pub(crate) rank: usize,
}

/// One role the worklist did NOT propose, with the reason it was dropped —
/// the `--rejected` diagnostic ([`collect_rejected`]).
#[derive(Serialize, Debug)]
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

/// Collapse a cluster's real files into the smallest set of directory globs
/// that covers them — the value the mold carries as `paths:`.
///
/// Agnostic by construction: the only input is the list of files the census
/// already resolved for the cluster, so no folder name is recognised, inferred
/// or hardcoded here. A file at the repository root contributes nothing (a
/// root-wide glob would scope the mold to everything, which is the same as not
/// scoping it).
///
/// Deterministic: deduplicated, ancestor-collapsed (a parent directory absorbs
/// every descendant it already matches, so `a/**` and `a/b/**` never both ship)
/// and sorted — the worklist stays byte-stable, as the `run` output contract
/// requires.
pub(crate) fn globs_for(files: &[String], code_dirs: &std::collections::BTreeSet<String>) -> Vec<String> {
    let mut dirs: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let norm = f.replace('\\', "/");
            let cut = norm.rfind('/')?;
            let dir = &norm[..cut];
            (!dir.is_empty()).then(|| dir.to_string())
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs = promote_dominated_parents(dirs, code_dirs);
    // Ancestor-collapse: keep a dir only when no ALREADY-KEPT dir is a parent
    // of it. Sorted order guarantees a parent is visited before its children.
    let mut kept: Vec<String> = Vec::new();
    for d in dirs {
        if kept.iter().any(|k| d.starts_with(&format!("{k}/"))) {
            continue;
        }
        kept.push(d);
    }
    kept.into_iter().map(|d| format!("{d}/**")).collect()
}

/// The FACTUAL lead of a mold's `## Convention` section — where the cluster
/// lives, what its members are named, and how many of them this house holds.
///
/// This exists because the authoring agent is measurably bad at exactly these
/// three values and measurably good at everything else in a mold. Audited over
/// one real enrich: every wrong claim a mold made was a TALLY, an ORDER or a
/// PATH ("9 files" where there were 10; "all of them derive X" where two did
/// not), while every claim that only reading could produce was right. Counting
/// is not a judgement call — the census already holds the answer exactly, so
/// asking a model to estimate it is spending inference to get a worse number.
///
/// So the line is rendered here, handed to the agent to copy verbatim (the same
/// contract `paths:` already uses), and verified on the way in by
/// `scan-patterns-apply`. The agent keeps the rest of `## Convention`: the
/// visibility habits, the test placement, the derive sets — the things a
/// program cannot see.
///
/// Deterministic: extensions are deduplicated and sorted, the globs come from
/// [`globs_for`], and nothing here reads a clock or an absolute path.
pub(crate) fn convention_line(c: &Candidate) -> String {
    let mut exts: Vec<String> = c
        .files
        .iter()
        .filter_map(|f| {
            let name = f.rsplit(['/', '\\']).next()?;
            let (_, ext) = name.rsplit_once('.')?;
            (!ext.is_empty()).then(|| format!(".{}", ext.to_lowercase()))
        })
        .collect();
    exts.sort();
    exts.dedup();
    let where_ = if c.paths.is_empty() { "(no recurring folder)".to_string() } else { c.paths.join(", ") };
    let what = if exts.is_empty() { "(mixed)".to_string() } else { exts.join(", ") };
    format!("Folder: {where_} · Extension: {what} · Files of this role in this subproject: {}", c.count)
}

/// Share of a subproject's own modules a cluster may cover before it stops
/// being a ROLE and becomes the subproject itself.
///
/// A mold auto-loads whenever an edit matches its `paths:`. One that matches
/// four edits in five is not teaching "how we write an X here" — it is teaching
/// "how we write here", which is what the subproject's `CLAUDE.md` Guards
/// already do, and it stacks on top of every genuine mold in the same house.
/// The enrich agent kept reaching this conclusion by hand and stating it in its
/// own words ("a folder-name cluster whose paths would load on top of every
/// other mold"), inconsistently — refusing it in one subproject and keeping it
/// in four others. Whether a glob covers the house is arithmetic over the
/// model, so it stops being the agent's call.
const SUBPROJECT_SATURATION: f32 = 0.8;

/// …and the ratio above is only asked of a subproject with at least this many
/// modules. Below it the measure carries no information: a house of three files
/// has no corner for a role to occupy, so EVERY cluster in it covers most of
/// the house and the test would drop them all — including the small, genuinely
/// useful mold a two-file crate deserves. The floor is a property of the
/// measure, not an exemption granted to any particular subproject.
const MIN_MODULES_FOR_SATURATION: usize = 10;

/// A cluster must claim at least this many sibling folders before their shared
/// parent is even considered — two siblings are a coincidence, not a layout.
const MIN_DOMINATING_SIBLINGS: usize = 3;

/// …and must account for at least this share of the parent's code-bearing
/// children. Below it the parent holds work the cluster has nothing to say
/// about, and scoping the mold there would load it over unrelated edits.
const DOMINANCE: f32 = 0.6;

/// Replace a run of sibling directories with their shared parent when the
/// cluster DOMINATES that parent.
///
/// Why this exists: [`globs_for`] derives one glob per directory that holds a
/// member, and ancestor-collapse only removes a directory when another KEPT
/// directory is its parent. That works for a flat layout — twenty modules
/// directly inside one folder yield one glob — and fails completely for the
/// layout where every member gets its own folder, because the shared parent
/// holds no member FILE and so never enters the list to collapse against. The
/// siblings are then all kept: one cluster in this workspace produced 37 globs
/// while two neighbours in the SAME subproject and the SAME language produced
/// one each. The difference was never the language; it was whether members sit
/// in a folder together or in a folder each — and folder-per-member is the
/// ordinary shape of component trees, feature modules, command directories and
/// package-per-module layouts alike.
///
/// Dominance, not mere sharing, is the bar: the parent's OTHER code-bearing
/// children are counted from the model's own module paths, so a cluster that
/// occupies three folders out of thirty stays at three globs and never widens
/// itself over twenty-seven folders of unrelated work. `code_dirs` is that
/// evidence — every directory the model saw a module in.
///
/// Runs to a fixpoint (bounded), so a promoted parent can itself join its own
/// siblings and promote again; deterministic, since every step sorts and
/// deduplicates and the inputs are byte-stable.
fn promote_dominated_parents(
    dirs: Vec<String>,
    code_dirs: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut current = dirs;
    // A path has finitely many ancestors; the bound only guards against a
    // future edit turning this into a cycle.
    for _ in 0..16 {
        let mut by_parent: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for d in &current {
            if let Some(cut) = d.rfind('/') {
                by_parent.entry(d[..cut].to_string()).or_default().push(d.clone());
            }
        }
        let mut promoted: Vec<String> = Vec::new();
        let mut consumed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (parent, siblings) in &by_parent {
            if siblings.len() < MIN_DOMINATING_SIBLINGS {
                continue;
            }
            let total = code_bearing_children(parent, code_dirs);
            if total == 0 {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let share = siblings.len() as f32 / total as f32;
            if share >= DOMINANCE {
                promoted.push(parent.clone());
                consumed.extend(siblings.iter().cloned());
            }
        }
        if promoted.is_empty() {
            return current;
        }
        let mut next: Vec<String> =
            current.into_iter().filter(|d| !consumed.contains(d)).chain(promoted).collect();
        next.sort();
        next.dedup();
        current = next;
    }
    current
}

/// Whether `paths` covers so much of `subproject` that the mold would load on
/// nearly every edit in it — see [`SUBPROJECT_SATURATION`] for why that stops
/// being a role.
///
/// Both sides are counted from the model's modules, never the filesystem: the
/// denominator is the modules the subproject owns (a nested subproject's files
/// belong to the nested one, so a parent house is not inflated by its child),
/// and the numerator is those the globs match. A subproject the model has no
/// modules for is never saturated — an empty denominator is missing evidence,
/// not proof of coverage.
///
/// Both sides also exclude test terrain, and they must: a cluster is dropped
/// outright when its home is under a test segment, so a glob can only ever
/// cover production modules. Counting tests in the denominator alone measured
/// the numerator against a universe the numerator could not reach, and the
/// ratio fell by however many tests the subproject happened to carry. Measured
/// here: a crate with 17 production modules and 57 test/fixture modules scored
/// 23% for a glob covering every one of its production files, and shipped the
/// whole-house mold this check exists to refuse. The bias is proportional to
/// test volume, so the better-tested a subproject is the more reliably the
/// check fails — which is exactly backwards.
fn saturates_subproject(
    paths: &[String],
    subproject: &str,
    modules: &[&Mod],
    projects: &[&Proj],
) -> bool {
    let dirs: Vec<&str> = paths.iter().map(|p| p.trim_end_matches("/**")).collect();
    let mut owned = 0usize;
    let mut covered = 0usize;
    for m in modules {
        let path = m.path.replace('\\', "/");
        // Ownership goes to the LONGEST matching project dir, so a nested
        // subproject's files never inflate the house that contains it.
        if owner_of(&path, projects).map(|p| p.dir.as_str()) != Some(subproject) {
            continue;
        }
        if under_test(&path) {
            continue; // same universe on both sides — see the doc comment.
        }
        owned += 1;
        if dirs.iter().any(|d| path.starts_with(&format!("{d}/"))) {
            covered += 1;
        }
    }
    if owned < MIN_MODULES_FOR_SATURATION {
        return false;
    }
    #[allow(clippy::cast_precision_loss)]
    let share = covered as f32 / owned as f32;
    share >= SUBPROJECT_SATURATION
}

/// How many DIRECT children of `parent` hold code anywhere beneath them,
/// counted from the model's module directories. This is the denominator of the
/// dominance test: it answers "how much of this folder would the mold be
/// claiming?" without touching the filesystem.
fn code_bearing_children(parent: &str, code_dirs: &std::collections::BTreeSet<String>) -> usize {
    let prefix = format!("{parent}/");
    let mut children: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for d in code_dirs {
        if let Some(rest) = d.strip_prefix(&prefix) {
            let head = rest.split('/').next().unwrap_or(rest);
            if !head.is_empty() {
                children.insert(head);
            }
        }
    }
    children.len()
}

/// Run `scan-patterns-list`. Prints a JSON array to stdout; exit 0 always.
/// `rejected` swaps the payload for the drop diagnostic; `subproject` narrows
/// EITHER payload to one house. With neither, the output is byte-identical to
/// before the flags existed (the `/scan` flow consumes it).
pub fn run(root: &Path, rejected: bool, subproject: Option<&str>) {
    // `to_string` cannot fail for these shapes; fall back to `[]` defensively.
    let json = if rejected {
        serde_json::to_string(&collect_rejected_in(root, subproject))
    } else {
        serde_json::to_string(&collect_in(root, subproject))
    };
    println!("{}", json.unwrap_or_else(|_| "[]".to_string()));
}

/// Normalise a subproject path to the forward-slashed root-relative form
/// [`Candidate::subproject`] carries: backslashes folded, a trailing `/` and a
/// leading `./` stripped; the root (`.`) maps to `""`, which matches no
/// candidate — molds are never authored for the workspace root.
///
/// This lives with the worklist rather than with any consumer of it. Normalising
/// a subproject path is the worklist's own concern; the prompt renderer that
/// filters by it (`agent-prompt-render --role patterns`) is a consumer of the
/// filter, not its owner — and while it owned this function the CLI could not
/// offer the same filter without copying it.
pub(crate) fn normalize_subproject(subproject: &str) -> String {
    let s = subproject.replace('\\', "/");
    let s = s.trim_end_matches('/');
    let s = s.strip_prefix("./").unwrap_or(s);
    if s == "." {
        String::new()
    } else {
        s.to_string()
    }
}

/// [`collect`] narrowed to ONE house when the caller names one — the worklist
/// question the `/scan` convergence check asks ("how many clusters are left in
/// THIS subproject?"), which otherwise needed a grouping script over the whole
/// list.
///
/// An unknown subproject yields an EMPTY list, never everything: the filter is
/// an equality test on the normalised dir, so a typo answers "nothing here"
/// rather than silently widening to the workspace. Crate-visible because the
/// patterns prompt renderer asks the very same question in-process.
pub(crate) fn collect_in(root: &Path, subproject: Option<&str>) -> Vec<Candidate> {
    let mut out = collect(root);
    if let Some(want) = subproject.map(normalize_subproject) {
        out.retain(|c| c.subproject == want);
    }
    out
}

/// The `--rejected` twin of [`collect_in`]: the drop diagnostic narrowed to one
/// house. Only drops the funnel could ATTRIBUTE to a subproject survive the
/// filter — a role rejected before ownership was settled carries no house
/// (`subproject: ""`), so it belongs to no per-house view by construction.
fn collect_rejected_in(root: &Path, subproject: Option<&str>) -> Vec<Rejection> {
    let mut out = collect_rejected(root);
    if let Some(want) = subproject.map(normalize_subproject) {
        out.retain(|r| r.subproject == want);
    }
    out
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

    // Subprojects with a non-empty dir (the root unit, dir "", is excluded —
    // molds are never authored for the workspace root).
    let manifest_units: Vec<&Proj> =
        model.projects.iter().filter(|p| !p.dir.is_empty()).collect();

    // …and when the manifests named NO house at all, the skeleton's houses
    // stand in. A repository with a single root `package.json` has exactly one
    // manifest unit — the root — which the filter above removes, leaving every
    // exemplar ownerless and the worklist empty: measured in the field, 34 of
    // 47 mined clusters dropped as `no_owner` while the same model carried 25
    // skeleton units. The branch is taken ONLY on that empty list, so a
    // workspace whose manifests answer keeps today's output byte for byte and
    // there is no prior behaviour to regress. Owned here because these rows are
    // synthesised, not borrowed from the model; declared before `projects` so
    // it outlives the references into it.
    let skeleton_units: Vec<Proj> = if manifest_units.is_empty() {
        model
            .skeleton
            .iter()
            .filter(|s| !s.dir.is_empty() && s.dir != SKELETON_ROOT)
            .map(|s| Proj { name: basename(&s.dir).to_string(), dir: s.dir.clone() })
            .collect()
    } else {
        Vec::new()
    };

    // Longest dir first so `common_dir` is attributed to its most-specific
    // owner — which is also what keeps a skeleton's aggregate entry (`src`)
    // from stealing the files of the unit below it (`src/sira`).
    let mut projects: Vec<&Proj> = if manifest_units.is_empty() {
        skeleton_units.iter().collect()
    } else {
        manifest_units
    };
    projects.sort_by(|a, b| b.dir.len().cmp(&a.dir.len()).then(a.dir.cmp(&b.dir)));

    // Module paths sorted once — every exemplar scan reads this in a stable order.
    let mut modules: Vec<&Mod> = model.modules.iter().collect();
    modules.sort_by(|a, b| a.path.cmp(&b.path));

    // Every directory the model saw a module in — the denominator of the
    // dominance test in [`promote_dominated_parents`]. Built once here, from
    // the model and never the filesystem, so the worklist stays a projection.
    let code_dirs: std::collections::BTreeSet<String> = model
        .modules
        .iter()
        .filter_map(|m| {
            let norm = m.path.replace('\\', "/");
            let cut = norm.rfind('/')?;
            (cut > 0).then(|| norm[..cut].to_string())
        })
        .collect();

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
        let label = slugify(&role.affix);
        if label.is_empty() {
            drop(role, "", "empty_label");
            continue;
        }
        // EVERY non-test home of the role — not just the owning subproject's.
        // Ownership is settled below, from where the exemplars actually live.
        let homes: Vec<&str> = std::iter::once(role.common_dir.as_str())
            .chain(role.dirs.iter().map(String::as_str))
            .filter(|d| !under_test(d))
            .collect();
        let found = exemplars_for(role, &modules, &homes);
        if found.is_empty() {
            drop(role, "", "no_exemplars"); // nothing hand-written left to read.
            continue;
        }
        let by_project = group_by_project(found, &projects);
        if by_project.is_empty() {
            // Exemplars exist, but none sits in a named subproject (root-level).
            drop(role, "", "no_owner");
            continue;
        }
        let mut earned = false;
        for (project_dir, house) in &by_project {
            if house.count() < MIN_EXEMPLARS {
                // This house is too thin; another may still teach — so the role
                // is not dropped, only THIS house is. Recorded per house all the
                // same: the global fallback below fires only when no house
                // qualified, so a role that earned a mold somewhere left every
                // OTHER house silent, and the per-house rule that most often
                // explains "why is there no mold for X here?" was the one rule
                // the diagnostic could not answer. Measured on a real
                // workspace: 8 of 34 vanished molds landed exactly here.
                drop(role, project_dir, "house_below_exemplars");
                continue;
            }
            // Lower-kebab the subproject short name too, so a PascalCase C# unit
            // (`DataAccess`) yields a consistent `dataaccess-<role>-pattern` folder.
            let subproj = slugify(basename(project_dir));
            if subproj.is_empty() {
                continue;
            }
            let slug = format!("{subproj}-{label}");
            if declined.contains_key(&slug) {
                drop(role, project_dir, "declined"); // agent refused, with a recorded reason.
                continue;
            }
            // Any surviving mold (this slug, or another `*-{label}-pattern`
            // claiming the role) is preserved — the sweep already deleted the
            // mustard-generated ones, so whatever remains is hand-authored.
            if mold_present(root, project_dir, &slug, &label) {
                drop(role, project_dir, "mold_exists");
                continue;
            }
            let paths = globs_for(&house.files, &code_dirs);
            // A glob set that covers the house is not a role's scope — see
            // [`SUBPROJECT_SATURATION`]. Judged AFTER the globs are final, so
            // the promotion above cannot smuggle a cluster past it.
            if saturates_subproject(&paths, project_dir, &modules, &projects) {
                drop(role, project_dir, "covers_whole_subproject");
                continue;
            }
            earned = true;
            candidates.push(Candidate {
                subproject: project_dir.clone(),
                label: label.clone(),
                slug,
                mold_path: format!("{project_dir}/.claude/skills/{subproj}-{label}-pattern/SKILL.md"),
                affix: role.affix.clone(),
                affix_kind: role.kind.clone(),
                decl_kind: role.decl_kind.clone(),
                implements: role.implements.clone(),
                count: house.count(),
                exemplars: house.exemplars(),
                paths,
                files: house.files.clone(),
                rank: rank_of.get(&role.affix.to_lowercase()).copied().unwrap_or(0),
            });
        }
        // Exemplars exist, but no single house holds enough to show a recurrence.
        if !earned && !by_project.iter().any(|(_, h)| h.count() >= MIN_EXEMPLARS) {
            drop(role, "", "no_exemplars");
        }
    }

    // One mold path, one candidate — see [`fold_collisions`].
    let mut candidates = fold_collisions(candidates);

    // The family hint is re-earned against each house's own files — after the
    // fold, so a folded candidate is judged on its unified file set.
    confirm_implements(&mut candidates, &model.modules);

    // Byte-stable order, now rank-first: within a subproject the agent reads the
    // strongest convention first (highest slice recurrence), and an unranked role
    // reads last WITHOUT being excluded. Slug breaks ties, so the order stays
    // total and deterministic.
    candidates.sort_by(|a, b| {
        a.subproject.cmp(&b.subproject).then(b.rank.cmp(&a.rank)).then(a.slug.cmp(&b.slug))
    });
    (candidates, rejected)
}

/// Fold candidates that resolve to the SAME mold path into one.
///
/// Roles are mined case-sensitively, but [`slugify`] lower-kebabs the affix —
/// so two spellings of one role (a naming convention that cases the affix
/// differently per declaration kind is ordinary, in any language that has one)
/// land on the same `label`, the same `slug` and, in the same house, the very
/// same `moldPath`. They are not two roles competing for one file: they are ONE
/// role spelled twice, which is exactly what the shared label already says. The
/// key is the mold path itself, so this stays blind to WHY they collided — any
/// future signal that folds two affixes onto one path folds here too.
///
/// Emitting both was silent data loss. The apply is create-only: the first
/// block wins and the second is dropped with `exit 0` and a message blaming a
/// hand-authored mold — so an agent burned a read + an authoring pass for
/// nothing, and no report could tell that from a legitimate preserve. This
/// repo's own model carries six such collisions, and it is not about its
/// language: any codebase whose convention varies the affix case has them.
///
/// The fold KEEPS every signal rather than picking a winner: the file sets
/// UNION (never add — one file can carry both spellings, so the sets overlap,
/// and inflating the local tally would undo the very honesty it was given),
/// exemplars come off that union, and `affix`/`declKind` carry each spelling
/// verbatim from the miner — so the agent is told both, and can teach the whole
/// convention instead of half of it. Deterministic: variants fold
/// strongest-first, affix breaking ties, so the output stays byte-stable.
///
/// This is a MERGE, never a cap: no cluster is dropped, and the uncapped
/// contract of the worklist is untouched.
fn fold_collisions(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut by_path: std::collections::BTreeMap<String, Vec<Candidate>> =
        std::collections::BTreeMap::new();
    for c in candidates {
        by_path.entry(c.mold_path.clone()).or_default().push(c);
    }
    by_path.into_values().filter_map(fold_variants).collect()
}

/// Fold one mold path's variants into a single candidate (see
/// [`fold_collisions`]). `None` only for an empty group, which
/// [`fold_collisions`] never builds.
fn fold_variants(mut group: Vec<Candidate>) -> Option<Candidate> {
    group.sort_by(|a, b| b.count.cmp(&a.count).then(a.affix.cmp(&b.affix)));
    let mut variants = group.into_iter();
    let mut folded = variants.next()?;
    for v in variants {
        folded.rank = folded.rank.max(v.rank);
        folded.affix = join_distinct(&folded.affix, &v.affix, "/");
        folded.affix_kind = join_distinct(&folded.affix_kind, &v.affix_kind, ", ");
        folded.decl_kind = join_distinct(&folded.decl_kind, &v.decl_kind, ", ");
        // First non-empty hint wins here; [`confirm_implements`] then judges it
        // against the folded file union, so a hint no variant's house earns
        // does not survive on collision order.
        folded.implements = folded.implements.or(v.implements);
        for f in v.files {
            if !folded.files.contains(&f) {
                folded.files.push(f);
            }
        }
    }
    // count/exemplars are re-derived from the UNION, never carried over.
    folded.count = folded.files.len();
    folded.exemplars = folded.files.iter().take(MAX_EXEMPLARS).cloned().collect();
    Some(folded)
}

/// Append `add` to the `sep`-joined `acc` unless it is already a member — the
/// two variants of a folded role usually share `affixKind` (`suffix`/`suffix`)
/// while differing in `affix` and `declKind`.
fn join_distinct(acc: &str, add: &str, sep: &str) -> String {
    if add.is_empty() || acc.split(sep).any(|part| part == add) {
        return acc.to_string();
    }
    if acc.is_empty() {
        return add.to_string();
    }
    format!("{acc}{sep}{add}")
}

/// Validate each candidate's repo-global `implements` hint against ITS house.
///
/// `roles[].implements` is the FAMILY's contract — the miner keeps it only when
/// a majority of the repo-wide family shares it. But a house is not the family:
/// the majority can live entirely in another subproject, and handing the global
/// hint to every house has the agent author a mold teaching a contract this
/// house never wrote (a unit whose types build on nothing, told to implement
/// the base contract a sibling unit's majority follows). So the hint is re-earned
/// locally: it survives only when at least [`MIN_EXEMPLARS`] of the candidate's
/// own files declare something building on it (union of each file's
/// declarations' supertypes). A demotion never drops the candidate — the house
/// keeps its mold; the agent just isn't handed a contract its files refute.
///
/// Demotion requires EVIDENCE, twice over:
/// - a model with no declarations anywhere (an older grain) skips the pass —
///   the global hint is all the data there is;
/// - a candidate none of whose files carries a known declaration (a language
///   the miner only walked textually) keeps its hint. A file WITH declarations
///   and an empty supertype union is the opposite case — a parsed type that
///   builds on nothing — and that absence IS evidence against.
///
/// Runs AFTER [`fold_collisions`]: the folded candidate's `files` union is
/// final there, so the hint is judged against every file the agent will be told
/// about, not one variant's half.
fn confirm_implements(candidates: &mut [Candidate], modules: &[Mod]) {
    if modules.iter().all(|m| m.declarations.is_empty()) {
        return; // older grain: no inheritance data exists to judge against.
    }
    // path -> that file's declared supertypes. Keyed only for files with at
    // least one declaration: key presence means "the miner parsed this file",
    // and an empty value means "parsed, builds on nothing".
    let mut supers: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in modules.iter().filter(|m| !m.declarations.is_empty()) {
        supers
            .entry(m.path.as_str())
            .or_default()
            .extend(m.declarations.iter().flat_map(|d| d.supertypes.iter().map(String::as_str)));
    }
    for c in candidates.iter_mut() {
        let Some(hint) = c.implements.as_deref() else { continue };
        let known: Vec<&Vec<&str>> =
            c.files.iter().filter_map(|f| supers.get(f.as_str())).collect();
        if known.is_empty() {
            continue; // no parsed file in this house — nothing to judge with.
        }
        let evidencing = known.iter().filter(|ss| ss.iter().any(|s| *s == hint)).count();
        if evidencing < MIN_EXEMPLARS {
            c.implements = None;
        }
    }
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

/// The subproject that owns `path`: the one whose `dir` is its longest prefix.
/// `projects` is pre-sorted longest-first, so the first match wins.
///
/// Feed this REAL paths (an exemplar's), never a role's `common_dir`: the miner
/// abstracts that one, and a wildcard sitting on the owning segment
/// (`backend/App/<Name>/Extensions`) matches no project literally, which read as
/// "ownerless" and killed the role.
fn owner_of<'a>(path: &str, projects: &[&'a Proj]) -> Option<&'a Proj> {
    projects
        .iter()
        .copied()
        .find(|p| path == p.dir || path.starts_with(&format!("{}/", p.dir)))
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

/// Group resolved exemplars by the subproject that really contains them, keyed
/// by project dir and sorted by dir for byte-stable output, capped at
/// [`MAX_EXEMPLARS`] PER subproject.
///
/// Ownership is settled from the exemplars' REAL paths, never from the role's
/// `common_dir`. The miner abstracts that dir (`abstract_entity`), and when the
/// entity happens to be named after the subproject the wildcard lands ON the
/// owning segment — `backend/App/<Name>/Extensions` — so a literal prefix test
/// finds no owner and the whole role dies "ownerless" though its files sit in a
/// perfectly ordinary subproject. Exemplar paths carry no wildcard, so they
/// answer the question the abstracted dir cannot.
///
/// One role yields one candidate PER subproject it lives in: the mold slug is
/// `{subproject}-{label}`, and each house has its own local convention (an
/// `Extension` in an infrastructure unit looks nothing like one in an
/// application unit). Picking a single owner also silently dropped every other
/// house — a role spread over four subprojects taught only one of them.
fn group_by_project(found: Vec<String>, projects: &[&Proj]) -> Vec<(String, House)> {
    let mut by_project: HashMap<&str, House> = HashMap::new();
    for path in found {
        let Some(project) = owner_of(&path, projects) else {
            continue; // root-level file: molds are never authored for the root.
        };
        by_project.entry(project.dir.as_str()).or_default().files.push(path);
    }
    let mut out: Vec<(String, House)> =
        by_project.into_iter().map(|(d, h)| (d.to_string(), h)).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// One subproject's share of a role: the role's hand-written files that live
/// there, in resolution order. The tally is LOCAL, not the role's repo-wide one
/// — a role spread over four subprojects would otherwise tell each of them it
/// has all the files, and the agent would author a mold claiming a recurrence
/// this house does not have.
///
/// The full path list is kept (not just the [`MAX_EXEMPLARS`] the agent reads)
/// because two case-variants of one role can resolve OVERLAPPING file sets — a
/// file declaring both `struct FooReport` and `fn foo_report`. Folding them
/// ([`fold_collisions`]) therefore has to union the paths and count the union;
/// adding the two tallies would inflate the very recurrence figure the local
/// count exists to keep honest.
#[derive(Default)]
struct House {
    files: Vec<String>,
}

impl House {
    /// How many hand-written files of the role live in this house.
    fn count(&self) -> usize {
        self.files.len()
    }

    /// The 1-3 files the agent is asked to read.
    fn exemplars(&self) -> Vec<String> {
        self.files.iter().take(MAX_EXEMPLARS).cloned().collect()
    }
}

/// Resolve the hand-written exemplar files of `role`: the files *directly* in
/// one of its homes that carry the affix. Two accepted signatures, tried in
/// order:
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
///
/// UNCAPPED: [`group_by_project`] caps per subproject. Capping here would let
/// path order decide ownership and erase every house but the first.
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

    // Uncapped on purpose: [`exemplars_by_project`] caps per SUBPROJECT. Taking
    // the first [`MAX_EXEMPLARS`] here would silently decide ownership by path
    // order — the three alphabetically-first files — and erase every other house
    // the role lives in.
    let by_filename: Vec<String> = located
        .iter()
        .filter(|m| matches_affix(stem(&m.path), &affix, &role.kind))
        .map(|m| m.path.clone())
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

    /// A repository with ONE build manifest, at the root. `projects[]` carries
    /// only the root unit, which the owner list drops — so before the skeleton
    /// fallback every cluster died `no_owner` and the whole worklist came back
    /// empty (measured in the field: 34 of 47 clusters, on a 1085-file backend
    /// whose model listed 25 skeleton units). The skeleton names the real
    /// houses, and `basename` already gives the slug its prefix — no new naming
    /// rule is involved.
    #[test]
    fn skeleton_houses_own_clusters_when_no_manifest_unit_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"backend","dir":""}],
              "skeleton": [
                {"dir":"src/sira","role":"L1","files":180},
                {"dir":"src","role":"L2","files":12},
                {"dir":"(root)","role":"L0","files":3}
              ],
              "roles": [{"affix":"Service","kind":"suffix","count":111,
                         "common_dir":"src/sira/service","decl_kind":"class"}],
              "modules": [
                {"path":"src/sira/service/UserService.ts"},
                {"path":"src/sira/service/OrderService.ts"},
                {"path":"src/sira/service/PlanService.ts"}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "the skeleton's house teaches exactly one cluster");
        let c = &got[0];
        assert_eq!(c.subproject, "src/sira", "the mined unit owns it, not the aggregate `src`");
        assert_eq!(c.slug, "sira-service");
        assert_eq!(c.mold_path, "src/sira/.claude/skills/sira-service-pattern/SKILL.md");
    }

    /// The fallback is not a second opinion: one manifest unit with a non-empty
    /// dir and the skeleton is never read. The `Handler` cluster below lives in
    /// a skeleton house and in no manifest house, so it must still drop
    /// `no_owner` — which is exactly the output a monorepo has today.
    #[test]
    fn skeleton_fallback_stays_out_when_manifest_units_exist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "skeleton": [{"dir":"src/sira","role":"L1","files":180}],
              "roles": [
                {"affix":"Service","kind":"suffix","count":5,
                 "common_dir":"apps/api/services","decl_kind":"class"},
                {"affix":"Handler","kind":"suffix","count":4,
                 "common_dir":"src/sira/handlers","decl_kind":"class"}
              ],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"},
                {"path":"src/sira/handlers/AuthHandler.ts"},
                {"path":"src/sira/handlers/CartHandler.ts"}
              ]
            }"#,
        );
        let slugs: Vec<String> = collect(root).into_iter().map(|c| c.slug).collect();
        assert_eq!(slugs, vec!["api-service"], "only the manifest house teaches");
        let ownerless: Vec<String> = collect_rejected(root)
            .into_iter()
            .filter(|r| r.reason == "no_owner")
            .map(|r| r.affix)
            .collect();
        assert_eq!(ownerless, vec!["Handler"], "the skeleton house was never consulted");
    }

    /// Fail-open all the way down: a model written before `skeleton[]` existed
    /// and with no manifest house either yields the empty worklist and prints
    /// `[]` — the same silent skip Guards performs, never an error.
    #[test]
    fn no_skeleton_degrades_to_empty_worklist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"backend","dir":""}],
              "roles": [{"affix":"Service","kind":"suffix","count":9,
                         "common_dir":"src/sira/service","decl_kind":"class"}],
              "modules": [
                {"path":"src/sira/service/UserService.ts"},
                {"path":"src/sira/service/OrderService.ts"}
              ]
            }"#,
        );
        let got = collect(root);
        assert!(got.is_empty(), "no house to own it, yet {} candidate(s) appeared", got.len());
        assert_eq!(serde_json::to_string(&got).unwrap(), "[]");
        // `run` prints and returns — the command's exit is 0 whatever happened.
        run(root, false, None);
    }

    /// AC-5 — the same normalised filter the prompt renderer already used,
    /// exposed on the CLI: it narrows BOTH faces (the worklist and the
    /// `--rejected` diagnostic), and an unknown house yields an EMPTY list
    /// rather than everything.
    #[test]
    fn list_filters_the_worklist_by_subproject() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Two houses share one `Service` role (both earn a mold), and a second
        // `Repo` role is thick enough in `apps/api` but too thin in `apps/web`
        // — so the fixture yields candidates in both houses AND a rejection
        // attributed to one of them.
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"},{"name":"web","dir":"apps/web"}],
              "roles": [
                {"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services","dirs":["apps/web/services"],"decl_kind":"class"},
                {"affix":"Repo","kind":"suffix","count":3,"common_dir":"apps/api/repos","dirs":["apps/web/repos"],"decl_kind":"class"}
              ],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"},
                {"path":"apps/web/services/CartService.ts"},
                {"path":"apps/web/services/AuthService.ts"},
                {"path":"apps/api/repos/UserRepo.ts"},
                {"path":"apps/api/repos/OrderRepo.ts"},
                {"path":"apps/web/repos/CartRepo.ts"}
              ]
            }"#,
        );

        let all = collect_in(root, None);
        let houses: std::collections::BTreeSet<&str> =
            all.iter().map(|c| c.subproject.as_str()).collect();
        assert!(houses.len() >= 2, "the fixture must span two houses: {houses:?}");

        // One house only — and every spelling of the same path resolves to it,
        // because the CLI value goes through the SAME normalisation the render
        // uses (backslashes folded, trailing `/` and leading `./` stripped).
        for spelling in ["apps/api", "apps/api/", "./apps/api", r"apps\api"] {
            let mine = collect_in(root, Some(spelling));
            assert!(!mine.is_empty(), "`{spelling}` must resolve to the house");
            assert!(
                mine.iter().all(|c| c.subproject == "apps/api"),
                "`{spelling}` leaked another house: {:?}",
                mine.iter().map(|c| c.subproject.as_str()).collect::<Vec<_>>()
            );
            assert!(mine.len() < all.len(), "`{spelling}` must narrow the worklist");
        }

        // The `--rejected` diagnostic takes the SAME filter — that face is what
        // the convergence check reads, so a filter on only one of them would
        // still need the grouping script this replaces.
        let dropped = collect_rejected_in(root, None);
        assert!(
            dropped.iter().any(|r| r.subproject == "apps/web"),
            "the fixture must drop a web house: {dropped:?}"
        );
        let web = collect_rejected_in(root, Some("apps/web"));
        assert!(!web.is_empty(), "the diagnostic must answer for one house");
        assert!(
            web.iter().all(|r| r.subproject == "apps/web"),
            "the diagnostic leaked another house: {web:?}"
        );

        // An unknown subproject yields NOTHING — never everything. A typo that
        // silently widened back to the workspace would make the convergence
        // count read as if no cluster had been closed.
        assert!(collect_in(root, Some("apps/nope")).is_empty(), "unknown house → empty worklist");
        assert!(
            collect_rejected_in(root, Some("apps/nope")).is_empty(),
            "unknown house → empty diagnostic"
        );
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
    fn case_variants_of_one_role_fold_into_a_single_mold() {
        // The miner is case-sensitive, so a convention that cases the affix per
        // declaration kind yields TWO roles — but slugify lower-kebabs both, so
        // both resolve to ONE moldPath. Emitting two meant the create-only apply
        // silently dropped the second and blamed a hand-authored mold for it.
        // `declKind` values are the miner's, verbatim: this test asserts they
        // survive the fold, never what they are.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = r#"{"projects":[{"name":"api","dir":"apps/api"}],
          "roles":[
            {"affix":"Report","kind":"suffix","decl_kind":"typedecl","count":16,"common_dir":"apps/api/reports"},
            {"affix":"report","kind":"suffix","decl_kind":"function","count":10,"common_dir":"apps/api/reports"}
          ],
          "modules":[
            {"path":"apps/api/reports/DriftReport.x"},{"path":"apps/api/reports/PathReport.x"},
            {"path":"apps/api/reports/drift_report.x"},{"path":"apps/api/reports/path_report.x"}
          ]}"#;
        write_model(root, model);
        let got = collect(root);
        let reports: Vec<&Candidate> = got.iter().filter(|c| c.label == "report").collect();
        assert_eq!(reports.len(), 1, "one mold path, one candidate: {:?}", got.iter().map(|c| &c.slug).collect::<Vec<_>>());
        let folded = reports[0];
        // Nothing is picked over the other — every signal survives the fold.
        assert_eq!(folded.affix, "Report/report", "both spellings reach the agent");
        assert_eq!(folded.decl_kind, "typedecl, function", "both declaration kinds do too");
        assert_eq!(folded.affix_kind, "suffix", "a shared kind is not repeated");
        // The tally is the UNION of the two file sets, never their sum: the
        // variants resolve overlapping files, so adding would claim a
        // recurrence this house does not have.
        assert_eq!(folded.count, 4, "four distinct files carry the role");
        assert_eq!(folded.exemplars.len(), MAX_EXEMPLARS, "read-list still capped");
        let mut uniq = folded.exemplars.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), folded.exemplars.len(), "no file is offered twice");
    }

    #[test]
    fn implements_hint_must_be_re_earned_by_each_house() {
        // One family, one repo-wide hint. The first house really writes the
        // contract (2 evidencing files); the second holds plain classes (one
        // building on nothing, one on an unrelated contract). The hint must
        // reach only the house that earns it — and the demoted house keeps its
        // candidate, just without the line.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [
                {"name":"api","dir":"apps/api"},
                {"name":"gateway","dir":"apps/gateway"}
              ],
              "roles": [{"affix":"Service","kind":"suffix","count":6,"common_dir":"apps/api/services","dirs":["apps/gateway/services"],"decl_kind":"class","implements":"BaseService"}],
              "modules": [
                {"path":"apps/api/services/UserService.x","declarations":[{"name":"UserService","supertypes":["BaseService"]}]},
                {"path":"apps/api/services/OrderService.x","declarations":[{"name":"OrderService","supertypes":["BaseService"]}]},
                {"path":"apps/gateway/services/EmailService.x","declarations":[{"name":"EmailService","supertypes":[]}]},
                {"path":"apps/gateway/services/QueueService.x","declarations":[{"name":"QueueService","supertypes":["IAlpha"]}]}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 2, "both houses keep their candidate: {:?}", got.iter().map(|c| &c.slug).collect::<Vec<_>>());
        let api = got.iter().find(|c| c.subproject == "apps/api").unwrap();
        let gateway = got.iter().find(|c| c.subproject == "apps/gateway").unwrap();
        assert_eq!(api.implements.as_deref(), Some("BaseService"), "the house that writes the contract keeps the hint");
        assert_eq!(gateway.implements, None, "the house that never writes it is not told to");
        assert!(collect_rejected(root).is_empty(), "a demotion is not a drop");
    }

    #[test]
    fn implements_hint_survives_a_model_without_declarations() {
        // An older grain carries no declarations at all — there is no
        // inheritance data to judge with, so the global hint stays untouched.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [{"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services","decl_kind":"class","implements":"BaseService"}],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].implements.as_deref(), Some("BaseService"));
    }

    #[test]
    fn implements_hint_survives_a_house_without_structural_evidence() {
        // The model DOES carry declarations (so the pass runs), but none of the
        // cluster's own files has any — a language the miner only walked
        // textually. Demoting needs evidence; the hint stays.
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
                {"path":"apps/api/other/Widget.ts","declarations":[{"name":"Widget","supertypes":["Base"]}]}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].implements.as_deref(), Some("BaseService"), "no local evidence — the hint is kept, not demoted");
    }

    #[test]
    fn a_folded_hint_is_validated_against_the_union_of_files() {
        // Two variants of one role fold into one mold path. EACH variant alone
        // holds ONE evidencing file (< MIN_EXEMPLARS — a pre-fold pass would
        // demote both); their union holds two. The hint must survive, proving
        // the validation runs on the folded file set.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [
                {"affix":"Report","kind":"suffix","decl_kind":"typedecl","count":8,"common_dir":"apps/api/reports","implements":"IReport"},
                {"affix":"Report","kind":"prefix","decl_kind":"class","count":8,"common_dir":"apps/api/reporting"}
              ],
              "modules": [
                {"path":"apps/api/reports/DriftReport.x","declarations":[{"name":"DriftReport","supertypes":["IReport"]}]},
                {"path":"apps/api/reports/PathReport.x","declarations":[{"name":"PathReport","supertypes":[]}]},
                {"path":"apps/api/reporting/ReportDrift.x","declarations":[{"name":"ReportDrift","supertypes":["IReport"]}]},
                {"path":"apps/api/reporting/ReportPath.x","declarations":[{"name":"ReportPath","supertypes":[]}]}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "one mold path, one folded candidate: {:?}", got.iter().map(|c| &c.slug).collect::<Vec<_>>());
        assert_eq!(got[0].count, 4, "the union carries all four files");
        assert_eq!(got[0].implements.as_deref(), Some("IReport"), "two evidencing files across the union re-earn the hint");
    }

    #[test]
    fn folding_never_drops_a_distinct_cluster() {
        // The fold is a merge keyed by mold path, NOT a cap: two roles that do
        // not collide stay two candidates.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = r#"{"projects":[{"name":"api","dir":"apps/api"}],
          "roles":[
            {"affix":"Service","kind":"suffix","decl_kind":"class","count":5,"common_dir":"apps/api/svc"},
            {"affix":"Handler","kind":"suffix","decl_kind":"class","count":4,"common_dir":"apps/api/hdl"}
          ],
          "modules":[
            {"path":"apps/api/svc/UserService.ts"},{"path":"apps/api/svc/BankService.ts"},
            {"path":"apps/api/hdl/UserHandler.ts"},{"path":"apps/api/hdl/BankHandler.ts"}
          ]}"#;
        write_model(root, model);
        let slugs: Vec<String> = collect(root).into_iter().map(|c| c.slug).collect();
        assert_eq!(slugs, vec!["api-handler", "api-service"], "distinct roles keep distinct molds");
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
    fn a_role_spread_across_subprojects_teaches_each_of_them() {
        // One role, several houses. Each subproject gets its OWN mold: the slug
        // is `{subproject}-{label}`, and the local convention differs (an
        // `Extension` in an infra unit looks nothing like one in an app unit).
        // Picking a single owner silently dropped every other house.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [
                {"name":"infra","dir":"backend/Infra"},
                {"name":"application","dir":"backend/Application"}
              ],
              "roles": [{"affix":"Extension","kind":"suffix","count":9,
                         "common_dir":"backend/<Name>/Extensions"}],
              "modules": [
                {"path":"backend/Infra/Extensions/ServiceExtension.cs"},
                {"path":"backend/Infra/Extensions/ModuleExtension.cs"},
                {"path":"backend/Application/Extensions/ContractExtension.cs"},
                {"path":"backend/Application/Extensions/PlanExtension.cs"}
              ]
            }"#,
        );
        let got = collect(root);
        let slugs: Vec<&str> = got.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(
            slugs,
            vec!["application-extension", "infra-extension"],
            "every house that can teach earns its own mold"
        );
        assert_eq!(
            got[0].exemplars,
            vec![
                "backend/Application/Extensions/ContractExtension.cs",
                "backend/Application/Extensions/PlanExtension.cs"
            ],
            "each mold reads only ITS subproject's files"
        );
        assert_eq!(got[0].subproject, "backend/Application");
        assert_eq!(got[1].subproject, "backend/Infra");
    }

    #[test]
    fn each_house_reports_its_own_count_not_the_repo_wide_one() {
        // The role has 9 members repo-wide, but this house holds 2. Handing it
        // the global tally would have the agent author a mold claiming a
        // recurrence the subproject does not have.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [
                {"name":"big","dir":"apps/big"},
                {"name":"small","dir":"apps/small"}
              ],
              "roles": [{"affix":"Card","kind":"suffix","count":9,
                         "common_dir":"apps/<Name>/cards"}],
              "modules": [
                {"path":"apps/big/cards/AlphaCard.tsx"},
                {"path":"apps/big/cards/BetaCard.tsx"},
                {"path":"apps/big/cards/GammaCard.tsx"},
                {"path":"apps/big/cards/DeltaCard.tsx"},
                {"path":"apps/small/cards/SoloCard.tsx"},
                {"path":"apps/small/cards/DuoCard.tsx"}
              ]
            }"#,
        );
        let got = collect(root);
        let counts: Vec<(&str, usize)> =
            got.iter().map(|c| (c.slug.as_str(), c.count)).collect();
        assert_eq!(
            counts,
            vec![("big-card", 4), ("small-card", 2)],
            "local tally per house, never the role's repo-wide 9"
        );
        assert_eq!(got[0].exemplars.len(), MAX_EXEMPLARS, "reading is still capped at 3");
    }

    #[test]
    fn ownership_survives_the_wildcard_over_the_subproject_segment() {
        // `abstract_entity` replaces the segment that IS the entity — and when
        // the entity is named after the subproject, the wildcard lands ON the
        // owning segment. A literal prefix test then finds no owner and the role
        // dies "ownerless" though its files sit in an ordinary subproject.
        // Ownership must come from the exemplars' REAL paths.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"infra","dir":"backend/Infra"}],
              "roles": [{"affix":"Extension","kind":"suffix","count":5,
                         "common_dir":"backend/<Name>/Extensions"}],
              "modules": [
                {"path":"backend/Infra/Extensions/ServiceExtension.cs"},
                {"path":"backend/Infra/Extensions/ModuleExtension.cs"}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "the wildcard hid the owner; the files did not");
        assert_eq!(got[0].slug, "infra-extension");
        let rejected = collect_rejected(root);
        assert!(rejected.is_empty(), "and nothing was dropped: {rejected:?}");
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
                {"path":"apps/api/gen/UserClient.ts","file_class":"generated"},
                {"path":"elsewhere/stray/AlphaStray.ts"},
                {"path":"elsewhere/stray/BetaStray.ts"}
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
    fn a_house_too_thin_to_teach_says_so_instead_of_going_quiet() {
        // A role can earn a mold in one house and miss in another. The global
        // fallback only fires when NO house qualified, so the miss used to leave
        // no trace at all — and "why does THIS subproject have no mold for X?"
        // was the one question the diagnostic could not answer.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"},{"name":"web","dir":"apps/web"}],
              "roles": [
                {"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/<Name>/services",
                 "dirs":["apps/api/services","apps/web/services"]}
              ],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"},
                {"path":"apps/web/services/CartService.ts"}
              ]
            }"#,
        );
        let got = collect(root);
        assert_eq!(got.len(), 1, "the house with two exemplars still teaches");
        assert_eq!(got[0].subproject, "apps/api");

        let rejected = collect_rejected(root);
        let thin: Vec<&Rejection> =
            rejected.iter().filter(|r| r.reason == "house_below_exemplars").collect();
        assert_eq!(thin.len(), 1, "the thin house is recorded, not skipped: {rejected:?}");
        assert_eq!(thin[0].subproject, "apps/web", "and it names WHICH house went thin");
        assert_eq!(thin[0].affix, "Service");
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

    /// The mold's `paths:` glob is derived from the cluster's REAL files — the
    /// only derivation that stays agnostic (a role's `common_dir` is abstracted
    /// by the miner and can carry a `<name>` placeholder; a folder name matched
    /// in code would be detecting role by name, which apps/scan forbids).
    ///
    /// Four ways this could silently go wrong, asserted together: a repeated
    /// directory must collapse to one glob; a descendant must be absorbed by an
    /// ancestor already kept (else `a/**` and `a/b/**` both ship and the second
    /// is dead weight); a Windows separator must normalise (a `\` in a
    /// versioned glob is a platform leak); and a root-level file must
    /// contribute nothing, because a root-wide glob scopes the mold to
    /// everything, which is the same as not scoping it at all.
    #[test]
    fn globs_for_derives_from_real_files_and_collapses() {
        let files = vec![
            "apps/api/services/user.rs".to_string(),
            "apps/api/services/order.rs".to_string(),
            "apps/api/services/deep/nested.rs".to_string(),
            "apps/gateway/services/edge.rs".to_string(),
            "README.md".to_string(),
        ];
        let none = std::collections::BTreeSet::new();
        assert_eq!(
            globs_for(&files, &none),
            vec![
                "apps/api/services/**".to_string(),
                "apps/gateway/services/**".to_string()
            ]
        );
        assert_eq!(
            globs_for(&[r"apps\api\services\user.rs".to_string()], &none),
            vec!["apps/api/services/**".to_string()]
        );
        assert!(globs_for(&[], &none).is_empty(), "no files must mean no glob, never a root-wide one");
    }

    /// Folder-per-member: each member owns a directory, so the shared parent
    /// holds no member FILE and ancestor-collapse — which only fires when a KEPT
    /// directory is another's parent — has nothing to work with. Without
    /// promotion every sibling ships its own glob.
    #[test]
    fn a_cluster_that_dominates_its_parent_collapses_to_it() {
        let files: Vec<String> = ["Alpha", "Beta", "Gamma", "Delta"]
            .iter()
            .map(|n| format!("src/components/{n}/index.x"))
            .collect();
        // The parent holds one more code-bearing child the cluster does not
        // claim: 4 of 5 is 80%, over the bar.
        let code_dirs: std::collections::BTreeSet<String> =
            ["Alpha", "Beta", "Gamma", "Delta", "Unrelated"]
                .iter()
                .map(|n| format!("src/components/{n}"))
                .collect();
        assert_eq!(
            globs_for(&files, &code_dirs),
            vec!["src/components/**".to_string()],
            "four of five children is dominance — one glob, not four"
        );
    }

    /// The other direction, and the reason dominance is the bar rather than
    /// mere sharing: a cluster occupying a corner of a large folder must stay in
    /// its corner. Widening here would load the mold over every unrelated edit
    /// in the parent — the failure the promotion exists to avoid, inverted.
    #[test]
    fn a_cluster_that_does_not_dominate_keeps_its_own_globs() {
        let files: Vec<String> =
            ["Alpha", "Beta", "Gamma"].iter().map(|n| format!("src/components/{n}/index.x")).collect();
        let mut code_dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for i in 0..20 {
            code_dirs.insert(format!("src/components/Other{i}"));
        }
        for n in ["Alpha", "Beta", "Gamma"] {
            code_dirs.insert(format!("src/components/{n}"));
        }
        let got = globs_for(&files, &code_dirs);
        assert_eq!(got.len(), 3, "3 of 23 children is not dominance: {got:?}");
        assert!(got.iter().all(|g| g.starts_with("src/components/") && g != "src/components/**"));
    }

    /// A cluster whose glob covers the whole house is the house, not a role.
    /// Such a mold auto-loads on nearly every edit and stacks on top of every
    /// genuine mold beside it — which is what the subproject's own Guards are
    /// for. The enrich agent kept reaching this verdict by hand and applying it
    /// inconsistently; the arithmetic settles it.
    #[test]
    fn a_cluster_that_covers_the_whole_subproject_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Every module of `apps/api` sits under `src/`, so a role mined on the
        // `src` folder covers 100% of the house. The house is deliberately over
        // MIN_MODULES_FOR_SATURATION — below that floor the ratio says nothing
        // and the check correctly abstains.
        // Directly in `src/`, so the folder role resolves its exemplars there.
        let modules: String =
            (0..10).map(|i| format!(r#"{{"path":"apps/api/src/thing{i}.x"}},"#)).collect();
        write_model(
            root,
            &format!(
                r#"{{
              "projects": [{{"name":"api","dir":"apps/api"}}],
              "roles": [
                {{"affix":"src","kind":"folder","count":12,"common_dir":"apps/api/src"}},
                {{"affix":"Service","kind":"suffix","count":4,"common_dir":"apps/api/src/services"}}
              ],
              "modules": [
                {modules}
                {{"path":"apps/api/src/services/UserService.x"}},
                {{"path":"apps/api/src/services/OrderService.x"}}
              ]
            }}"#
            ),
        );
        let slugs: Vec<String> = collect(root).into_iter().map(|c| c.slug).collect();
        assert!(!slugs.contains(&"api-src".to_string()), "the house-wide cluster is dropped: {slugs:?}");
        assert!(slugs.contains(&"api-service".to_string()), "the real role survives: {slugs:?}");
        // The drop is REPORTED, never silent — that diagnostic is why the
        // `--rejected` face exists.
        let reasons: Vec<&str> = collect_rejected(root).iter().map(|r| r.reason).collect();
        assert!(reasons.contains(&"covers_whole_subproject"), "reason recorded: {reasons:?}");
    }

    /// …and a well-tested subproject must reach the same verdict. The check
    /// counts modules, and a test module can never be covered by a mold's glob
    /// (a cluster under a test segment is dropped before it gets one), so
    /// counting tests in the denominator alone measures the numerator against a
    /// universe it cannot reach. The bias grows with test volume: the better
    /// tested a subproject is, the more reliably the check would fail. Here the
    /// house is the SAME 10 production modules as the test above, plus 30
    /// tests — a 100% glob scoring 25% on the old arithmetic.
    #[test]
    fn test_volume_never_buys_a_cluster_past_the_house_check() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let prod: String =
            (0..10).map(|i| format!(r#"{{"path":"apps/api/src/thing{i}.x"}},"#)).collect();
        let tests: String =
            (0..30).map(|i| format!(r#"{{"path":"apps/api/tests/fixtures/case{i}.x"}},"#)).collect();
        write_model(
            root,
            &format!(
                r#"{{
              "projects": [{{"name":"api","dir":"apps/api"}}],
              "roles": [
                {{"affix":"src","kind":"folder","count":12,"common_dir":"apps/api/src"}},
                {{"affix":"Service","kind":"suffix","count":4,"common_dir":"apps/api/src/services"}}
              ],
              "modules": [
                {prod}
                {tests}
                {{"path":"apps/api/src/services/UserService.x"}},
                {{"path":"apps/api/src/services/OrderService.x"}}
              ]
            }}"#
            ),
        );
        let slugs: Vec<String> = collect(root).into_iter().map(|c| c.slug).collect();
        assert!(
            !slugs.contains(&"api-src".to_string()),
            "tests must not dilute the denominator into letting the house through: {slugs:?}"
        );
        assert!(slugs.contains(&"api-service".to_string()), "the real role still survives: {slugs:?}");
    }

    /// Two siblings are a coincidence, not a layout — the floor keeps a pair
    /// from claiming a parent even when the parent holds nothing else.
    #[test]
    fn two_siblings_alone_never_claim_their_parent() {
        let files = vec!["src/x/A/index.x".to_string(), "src/x/B/index.x".to_string()];
        let code_dirs: std::collections::BTreeSet<String> =
            ["src/x/A", "src/x/B"].iter().map(ToString::to_string).collect();
        assert_eq!(globs_for(&files, &code_dirs).len(), 2, "a pair stays a pair");
    }

    /// The worklist entry carries the glob, so the dispatched agent copies a
    /// computed value instead of authoring one. Without this the `paths:` key
    /// would be the agent's invention — exactly what the census exists to
    /// prevent.
    #[test]
    fn globs_for_reaches_the_candidate_worklist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "roles": [{"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services","decl_kind":"class"}],
              "modules": [
                {"path":"apps/api/services/user_service.rs"},
                {"path":"apps/api/services/order_service.rs"},
                {"path":"apps/api/services/mail_service.rs"}
              ],
              "projects": [{"dir":"apps/api","kind":"cargo"}]
            }"#,
        );
        let out = collect(root);
        let c = out.first().expect("one candidate");
        assert_eq!(c.paths, vec!["apps/api/services/**".to_string()]);
        let json = serde_json::to_string(&out).unwrap();
        assert!(json.contains("\"paths\""), "the glob must reach the worklist JSON: {json}");
    }
}
