//! `mustard-rt run active-specs` — filesystem-canonical active-spec discovery.
//!
//! Replaces the LLM-side glob/grep loop that `/mustard:spec` used to run: globs
//! `.claude/spec/*/spec.md` + `.claude/spec/*/wave-plan.md`, parses the header
//! of each, filters to `Outcome=Active` + `Stage ∈ {Analyze, Plan, Execute}`, counts
//! wave progress for wave-plans, extracts a one-line resumo, resolves short
//! aliases for parent specs, and backfills SQLite events when absent (the
//! multi-dev gap: after a `git pull`, a teammate's spec is on disk but has no
//! local events).
//!
//! ## Three places a spec can live
//!
//! The working tree is not the whole project. A spec directory belongs with the
//! work it describes, so while that work is in flight the directory exists ONLY
//! on its work branch — and a listing that globs the checkout answers "nothing
//! in progress" when it means "I only looked at one branch". Discovery
//! therefore ALSO reads the spec area of every work unit, through
//! `git ls-tree` + `git show`, without checking anything out; each row then
//! carries where it lives ([`ActiveSpec::location`] / [`ActiveSpec::branch`]),
//! so an in-flight spec is never mistaken for one present in the checkout.
//!
//! The third place is the one this listing used to miss entirely: a unit a
//! colleague pushed, alive on a remote with no local head here. Discovery does
//! not enumerate refs itself any more — it asks
//! [`crate::shared::branch_state::BranchEnumerator`], which sweeps `refs/heads/`
//! AND `refs/remotes/`, so that unit arrives with the rest and is reported
//! `location:"remote"`.
//!
//! ## Fail-open contract
//!
//! Every error path prints a warning on stderr and continues. The process exits
//! `0` regardless. Missing directories, unparseable headers, and SQLite failures
//! all degrade to partial results, never to a panic or non-zero exit. The
//! cross-branch scan keeps the same contract with one addition: when git cannot
//! answer, the listing degrades to the working-tree answer AND says so
//! ([`BranchScan::reason`]) — never a silent empty list that reads like a
//! verified absence, which is the exact habit this module was fixed to stop.

use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use mustard_core::domain::meta;
use mustard_core::domain::model::event::HarnessEvent;
use mustard_core::view::projection::read_workspace_events;
use mustard_core::{translate, SupportedLocale};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::commands::git_settle::git_out;
use crate::commands::pipeline::resume_bootstrap::wave_dispatch_recorded;
use crate::shared::branch_state::{BranchEnumerator, BranchRefs};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Parsed header fields from a spec.md.
#[derive(Debug, Clone, Default)]
struct SpecHeader {
    stage: Option<String>,
    outcome: Option<String>,
    scope: Option<String>,
    parent: Option<String>,
    checkpoint: Option<String>,
}

/// Where a discovered spec's directory physically lives.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpecLocation {
    /// Present in the CURRENT working tree, under `.claude/spec/`.
    Tree,
    /// Absent from the working tree; carried only by this LOCAL work branch.
    Branch(String),
    /// No local ref carries it at all — only the remote-tracking ref named here
    /// (`<remote>/<branch>`) does. A colleague pushed the unit and this
    /// checkout never fetched a local head for it, so acting on the spec means
    /// getting the branch first. Reported apart from [`Self::Branch`] because
    /// "switch to it" and "fetch it" are different next steps.
    Remote(String),
}

/// The facts a branch-carried spec cannot get from the filesystem, because its
/// directory is not there. Read from the branch's own tree instead.
#[derive(Debug, Clone, Default)]
struct BranchFacts {
    /// One-line summary, extracted from the branch's `spec.md` blob.
    resumo: String,
    /// Wave progress, counted over the branch's `wave-N-*` subtrees.
    progress: Option<WaveProgress>,
    /// Number of the first wave the branch still has `Outcome=Active`.
    first_active_wave: Option<String>,
}

/// A discovered spec candidate before filtering.
#[derive(Debug, Clone)]
struct SpecCandidate {
    name: String,
    spec_dir: PathBuf,
    spec_md: PathBuf,
    is_wave_plan: bool,
    header: SpecHeader,
    /// Working tree, or the work branch that carries the directory.
    location: SpecLocation,
    /// `Some` exactly when `location` is [`SpecLocation::Branch`] — the paths
    /// above do NOT exist on disk for such a candidate, so everything the
    /// filesystem would have answered is precomputed here instead.
    branch_facts: Option<BranchFacts>,
}

/// Progress for a wave-plan spec.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct WaveProgress {
    pub done: usize,
    pub total: usize,
}

/// One active spec entry in the final output.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ActiveSpec {
    pub name: String,
    pub stage: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(rename = "parentAlias", skip_serializing_if = "Option::is_none")]
    pub parent_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<WaveProgress>,
    pub resumo: String,
    pub letter: String,
    pub status: String,
    /// **Advisory, never blocking.** `true` when this spec's `<spec>/.clarified`
    /// marker EXISTS but records nothing — no captured term, no stated reason.
    ///
    /// `approve-spec` REFUSES on exactly this state, and that refusal lands at
    /// the worst possible moment: the operator has just asked for the
    /// implementation. Surfacing the same fact in the listing moves the
    /// discovery earlier; the refusal itself is untouched. Classified by the
    /// single definition of "hollow" —
    /// [`crate::commands::spec::approve_spec::clarify_state`] — never a second
    /// one.
    #[serde(rename = "clarifyRecordsNothing")]
    pub clarify_records_nothing: bool,
    /// `"tree"` when the spec directory exists in the current working tree;
    /// `"branch"` when it exists ONLY on the local work branch named by
    /// [`Self::branch`] — an **in-flight** spec, whose work has not merged yet;
    /// `"remote"` when NO local ref carries it and only the remote-tracking ref
    /// in [`Self::branch`] does.
    ///
    /// The distinction is the point: a `tree` row is here, a `branch` row means
    /// switching first, a `remote` row means fetching first — and a caller that
    /// cannot tell them apart looks for a directory that is not there.
    pub location: String,
    /// The ref carrying this spec — the branch name when `location` is
    /// `"branch"`, the `<remote>/<branch>` ref when it is `"remote"`, absent
    /// when it is `"tree"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// What the cross-branch scan managed to do, reported alongside the specs.
///
/// This is the honest half of the fail-open contract: `ok:false` plus a
/// `reason` says the listing covers the working tree ONLY, which is a different
/// claim from "these are all the active specs".
#[derive(Debug, Clone, Serialize)]
pub(crate) struct BranchScan {
    /// `true` when the work branches were actually enumerated and read.
    pub ok: bool,
    /// Why the scan could not run — present ONLY when `ok` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The work-unit refs inspected, in the enumerator's name order — a branch
    /// name when a local head carries the unit, `<remote>/<branch>` when only a
    /// remote does. Empty with `ok:true` simply means the project has no work
    /// unit other than the one checked out.
    pub branches: Vec<String>,
}

/// Full JSON output schema.
#[derive(Debug, Serialize)]
pub(crate) struct ActiveSpecsOutput {
    pub specs: Vec<ActiveSpec>,
    #[serde(rename = "parentMap")]
    pub parent_map: HashMap<String, String>,
    #[serde(rename = "branchScan")]
    pub branch_scan: BranchScan,
}

// ---------------------------------------------------------------------------
// Header cap: only read the first 2 KiB of each spec.md to parse the header.
// ---------------------------------------------------------------------------
const HEADER_CAP: usize = 2048;

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

/// Strip `[[wikilink]]` brackets from a value, returning the inner text.
fn strip_wikilink(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")) {
        inner.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Read at most `HEADER_CAP` bytes from a file as lossy UTF-8.
fn read_header_bytes(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEADER_CAP];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Try to read the spec header from a `meta.json` sidecar next to `spec_file`.
///
/// Returns `None` when the sidecar is absent or unreadable — the caller
/// falls through to the legacy `.md` header parser.  A sidecar that *is*
/// readable but lacks both `stage` and `outcome` is still considered
/// authoritative (the sidecar won over `.md`); the caller will detect the
/// missing fields and mark the spec as malformed (`??`).
///
/// Delegates the IO + lenient parse to [`mustard_core::domain::meta`] — this is
/// just the type-shape conversion from [`meta::Meta`] to the local
/// [`SpecHeader`] (picker uses a narrower projection — `phase` /
/// `isWavePlan` / `totalWaves` are not needed here).
fn parse_header_from_meta(spec_file: &Path) -> Option<SpecHeader> {
    // NOTE: we no longer drop incomplete sidecars here.  A sidecar with
    // missing stage/outcome is returned as-is; classify_spec() will flag it
    // as malformed so it shows up in the picker with "??" instead of being
    // silently hidden.
    Some(header_from_meta(meta::read_meta_beside(spec_file)?))
}

/// Narrow a parsed [`meta::Meta`] to the picker's [`SpecHeader`] projection
/// (`phase` / `isWavePlan` / `totalWaves` are not needed here).
fn header_from_meta(m: meta::Meta) -> SpecHeader {
    let nonempty = |opt: Option<String>| opt.filter(|s| !s.is_empty());
    SpecHeader {
        stage: nonempty(m.stage),
        outcome: nonempty(m.outcome),
        scope: nonempty(m.scope),
        parent: nonempty(m.parent).map(|s| strip_wikilink(&s)),
        checkpoint: nonempty(m.checkpoint),
    }
}

/// [`parse_header_from_meta`] over a sidecar's BYTES rather than its path — the
/// door the cross-branch scan comes through, since a branch-carried `meta.json`
/// has no path in this checkout. Same lenient parse, same projection.
fn header_from_meta_text(text: &str) -> Option<SpecHeader> {
    serde_json::from_str::<meta::Meta>(text).ok().map(header_from_meta)
}

/// Parse the header fields, preferring `meta.json` sidecar (W3 onward),
/// with fall-back to the legacy `### Key:` markdown header.
///
/// Resolution order:
/// 1. `meta.json` next to `spec_file` — authoritative after the W3
///    `meta-sidecar` migration removed header lines from the `.md`.
/// 2. Legacy header lines in the first 2 KiB of the `.md` — kept so a
///    teammate's un-migrated spec (e.g. pulled from a feature branch)
///    still shows up in the picker.
///
/// Uses the **first occurrence** of each `### Key:` line in the `.md`
/// fallback — any duplicate further down in the body is intentionally
/// ignored (header-drift handling).
fn parse_header(path: &Path) -> SpecHeader {
    if let Some(header) = parse_header_from_meta(path) {
        return header;
    }
    let Some(text) = read_header_bytes(path) else {
        return SpecHeader::default();
    };
    parse_header_md(&text)
}

/// The legacy `### Key:` markdown header parser, over TEXT — shared by the
/// path-based [`parse_header`] and by the cross-branch scan, which holds the
/// `spec.md` as a blob rather than a file.
fn parse_header_md(text: &str) -> SpecHeader {
    let mut header = SpecHeader::default();
    let mut last_header_line = false;
    let mut past_header = false;

    for (i, line) in text.lines().enumerate() {
        if i > 35 {
            break;
        }
        let trimmed = line.trim();

        if trimmed.starts_with("### ") {
            last_header_line = true;
            past_header = false;
            // Parse the key/value
            if let Some(rest) = trimmed.strip_prefix("### ") {
                if let Some(colon_pos) = rest.find(':') {
                    let key = rest[..colon_pos].trim();
                    let val = rest[colon_pos + 1..].trim().to_string();
                    match key.to_ascii_lowercase().as_str() {
                        "stage"
                            if header.stage.is_none() && !val.is_empty() => {
                                header.stage = Some(val);
                            }
                        "outcome"
                            if header.outcome.is_none() && !val.is_empty() => {
                                header.outcome = Some(val);
                            }
                        "scope"
                            if header.scope.is_none() && !val.is_empty() => {
                                header.scope = Some(val);
                            }
                        "parent"
                            if header.parent.is_none() && !val.is_empty() => {
                                header.parent = Some(strip_wikilink(&val));
                            }
                        "checkpoint"
                            if header.checkpoint.is_none() && !val.is_empty() => {
                                header.checkpoint = Some(val);
                            }
                        _ => {}
                    }
                }
            }
        } else if last_header_line && !trimmed.is_empty() && !trimmed.starts_with('#') {
            // Non-header content after seeing header lines: header block has ended.
            past_header = true;
        }

        if past_header {
            break;
        }
    }

    header
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Discover root-level spec candidates.
///
/// Globs `.claude/spec/*/spec.md` and `.claude/spec/*/wave-plan.md`. Excludes
/// paths that are inside wave subdirectories (contain `/wave-N-*/`) or inside
/// `review/` or `qa/` subdirs. Only spec.md or wave-plan.md at the root of a
/// named spec directory are included.
fn discover_root_specs(root: &Path) -> Vec<SpecCandidate> {
    let Ok(paths) = ClaudePaths::for_project(root) else {
        return Vec::new();
    };
    let spec_root = paths.spec_dir();
    let Ok(entries) = fs::read_dir(&spec_root) else {
        return Vec::new();
    };

    let mut candidates: Vec<SpecCandidate> = Vec::new();

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let name = entry.file_name.clone();
        // Skip wave-N-*, review/, qa/ directories at the top level
        // (these are subdirs of spec parents, not top-level spec names).
        // At the top spec level, all entries are valid spec names.
        let dir_path = &entry.path;

        // Check for spec.md first
        let spec_md = dir_path.join("spec.md");
        let wave_plan_md = dir_path.join("wave-plan.md");

        let (use_path, is_wave_plan) = if wave_plan_md.is_file() {
            (wave_plan_md.clone(), true)
        } else if spec_md.is_file() {
            (spec_md.clone(), false)
        } else {
            continue;
        };

        // Parse header from spec.md always (the canonical state header lives there)
        let header_path = if spec_md.is_file() { &spec_md } else { &use_path };
        let header = parse_header(header_path);

        candidates.push(SpecCandidate {
            name,
            spec_dir: dir_path.clone(),
            spec_md: spec_md.clone(),
            is_wave_plan,
            header,
            location: SpecLocation::Tree,
            branch_facts: None,
        });
    }

    candidates
}

// ---------------------------------------------------------------------------
// Cross-branch discovery
// ---------------------------------------------------------------------------

/// Read one blob out of `branch` without checking anything out. `None` when the
/// path is absent on that branch (or git fails) — indistinguishable on purpose:
/// both mean "this listing has nothing to say about that file".
fn show_blob(root: &Path, branch: &str, path: &str) -> Option<String> {
    git_out(root, &["show", &format!("{branch}:{path}")])
}

/// Enumerate the project's WORK branches and the active specs they carry but
/// the current working tree does not.
///
/// The enumeration is NOT done here: it is [`BranchEnumerator`], the crate's
/// single answer to "which work-unit refs exist". This listing used to sweep
/// `refs/heads/` on its own, and that private half-sweep is precisely why a
/// spec pushed by a colleague — alive on a remote, with no local head in this
/// checkout — was invisible to it. Two sweeps answering one question drift; one
/// sweep with two consumers cannot.
///
/// A work branch is one whose name starts with `{base}_` for a base the project
/// itself declares (`mustard.json#git.flow` via
/// [`mustard_core::domain::config::GitConfig::integration_bases`]) — the same
/// question `work-unit-open` and the work-branch gate ask, so no branch name is
/// hardcoded here either. The branch that is checked out is skipped: its specs
/// ARE the working tree.
///
/// `seen` carries every spec name already accounted for and is extended as
/// branches are read, so the checkout always wins over a branch and the first
/// ref (in the enumerator's name order) wins over the rest — one row per spec,
/// never a duplicate wearing two locations.
///
/// **Fail-open.** Any git step that cannot answer returns the working-tree
/// answer with `ok:false` and a stated reason. It never panics and never
/// reports an empty branch set as a verified "nothing else in flight" — which
/// is why the enumerator is asked through its fallible face.
fn scan_work_branches(
    root: &Path,
    seen: &mut BTreeSet<String>,
) -> (Vec<SpecCandidate>, BranchScan) {
    let mut scan = BranchScan { ok: false, reason: None, branches: Vec::new() };

    if git_out(root, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        scan.reason = Some("git indisponível ou diretório fora de um repositório".to_string());
        return (Vec::new(), scan);
    }

    // `.claude/spec` as GIT names it: relative to the repository root, which is
    // not necessarily `root` (a subproject checkout carries a prefix).
    let prefix = git_out(root, &["rev-parse", "--show-prefix"]).unwrap_or_default();
    let spec_root = format!("{prefix}.claude/spec");

    let current = git_out(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();

    let config = mustard_core::ProjectConfig::load(root);
    let flow = crate::shared::work_kind::BaseFlow::of(&config.git);

    let git_read = |args: &[&str]| git_out(root, args);
    let Some(enumerated) = BranchEnumerator::try_sweep(&git_read, &flow) else {
        scan.reason =
            Some("git não enumerou as refs — branches de trabalho não inspecionados".to_string());
        return (Vec::new(), scan);
    };

    // The checked-out branch is skipped: its specs are the working tree, and
    // the tree already won every slug it carries.
    let units: Vec<&BranchRefs> =
        enumerated.units().iter().filter(|u| u.branch != current).collect();

    scan.ok = true;
    scan.branches = units.iter().map(|u| u.read_ref()).collect();

    let mut candidates: Vec<SpecCandidate> = Vec::new();
    for unit in units {
        candidates.extend(specs_on_branch(root, unit, &spec_root, seen));
    }
    (candidates, scan)
}

/// The active-spec candidates `unit` carries whose name is not already in
/// `seen`; kept names are added to `seen`.
///
/// The ref actually read is [`BranchRefs::read_ref`] — the local head when
/// there is one, otherwise the remote-tracking ref, which is what makes a
/// remote-only unit readable at all. ONE `git ls-tree -r` describes its whole
/// spec area; every further read is a `git show` of a single blob, so the cost
/// tracks what is actually in flight rather than the size of the repository.
fn specs_on_branch(
    root: &Path,
    unit: &BranchRefs,
    spec_root: &str,
    seen: &mut BTreeSet<String>,
) -> Vec<SpecCandidate> {
    let branch = unit.read_ref();
    let branch = branch.as_str();
    let dir = format!("{spec_root}/");
    let Some(listing) = git_out(root, &["ls-tree", "-r", "--name-only", branch, "--", &dir]) else {
        return Vec::new();
    };

    // spec name → its paths, relative to `<spec_root>/<name>/`.
    let mut by_spec: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in listing.lines() {
        let Some(rest) = line.trim().strip_prefix(dir.as_str()) else { continue };
        let Some((name, inner)) = rest.split_once('/') else { continue };
        if name.is_empty() || inner.is_empty() {
            continue;
        }
        by_spec.entry(name.to_string()).or_default().insert(inner.to_string());
    }

    let mut out: Vec<SpecCandidate> = Vec::new();
    for (name, files) in by_spec {
        if seen.contains(&name) {
            continue;
        }
        let has_spec_md = files.contains("spec.md");
        let is_wave_plan = files.contains("wave-plan.md");
        if !has_spec_md && !is_wave_plan {
            continue;
        }

        let spec_md_text =
            has_spec_md.then(|| show_blob(root, branch, &format!("{spec_root}/{name}/spec.md"))).flatten();
        // Same resolution order as on disk: `meta.json` is authoritative, the
        // legacy `.md` header is the fallback.
        let header = show_blob(root, branch, &format!("{spec_root}/{name}/meta.json"))
            .as_deref()
            .and_then(header_from_meta_text)
            .or_else(|| spec_md_text.as_deref().map(parse_header_md))
            .unwrap_or_default();

        // A terminal spec on an old branch is not in flight — leave it out AND
        // leave `seen` alone, so a later branch carrying an active copy of the
        // same slug still gets its turn.
        if classify_spec(&header).is_none() {
            continue;
        }
        seen.insert(name.clone());

        let (progress, first_active_wave) =
            branch_wave_facts(root, branch, spec_root, &name, &files);
        let facts = BranchFacts {
            resumo: spec_md_text.as_deref().map(extract_resumo_text).unwrap_or_default(),
            progress,
            first_active_wave,
        };

        let spec_dir = ClaudePaths::spec_dir_or_unchecked(root, &name);
        let location = if unit.is_remote_only() {
            SpecLocation::Remote(branch.to_string())
        } else {
            SpecLocation::Branch(branch.to_string())
        };
        out.push(SpecCandidate {
            name,
            spec_md: spec_dir.join("spec.md"),
            spec_dir,
            is_wave_plan,
            header,
            location,
            branch_facts: Some(facts),
        });
    }
    out
}

/// Wave progress + the first still-active wave, read from `branch`'s tree.
///
/// Mirrors [`count_wave_progress`] and [`find_first_active_wave`] over blobs
/// instead of directory entries — one pass, so "done" and "first active" keep
/// the one definition they already had (`Close`+`Completed`, `Active`).
fn branch_wave_facts(
    root: &Path,
    branch: &str,
    spec_root: &str,
    name: &str,
    files: &BTreeSet<String>,
) -> (Option<WaveProgress>, Option<String>) {
    // wave dir → its files.
    let mut waves: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for rel in files {
        let Some((wave_dir, inner)) = rel.split_once('/') else { continue };
        let Some(after) = wave_dir.strip_prefix("wave-") else { continue };
        if !after.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        waves.entry(wave_dir.to_string()).or_default().insert(inner.to_string());
    }

    let mut total = 0usize;
    let mut done = 0usize;
    let mut active: Vec<u32> = Vec::new();

    for (wave_dir, inner) in &waves {
        if !inner.contains("spec.md") {
            continue;
        }
        total += 1;
        let base = format!("{spec_root}/{name}/{wave_dir}");
        let hdr = show_blob(root, branch, &format!("{base}/meta.json"))
            .as_deref()
            .and_then(header_from_meta_text)
            .or_else(|| {
                show_blob(root, branch, &format!("{base}/spec.md"))
                    .as_deref()
                    .map(parse_header_md)
            })
            .unwrap_or_default();

        let stage_close =
            hdr.stage.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("close"));
        let outcome_completed =
            hdr.outcome.as_deref().is_some_and(|o| o.eq_ignore_ascii_case("completed"));
        if stage_close && outcome_completed {
            done += 1;
        }
        if hdr.outcome.as_deref().is_some_and(|o| o.eq_ignore_ascii_case("active")) {
            let num: String =
                wave_dir["wave-".len()..].chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = num.parse::<u32>() {
                active.push(n);
            }
        }
    }

    let progress = (total > 0).then_some(WaveProgress { done, total });
    active.sort_unstable();
    (progress, active.first().map(u32::to_string))
}

// ---------------------------------------------------------------------------
// Spec classification
// ---------------------------------------------------------------------------

/// Classification of a discovered spec candidate for picker inclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpecKind {
    /// Normal active spec: Stage ∈ {Analyze, Plan, Execute} + Outcome=Active.
    Active,
    /// Stage=Close + Outcome=Active: the spec closed but generated a follow-up.
    ClosedFollowup,
    /// Stage or Outcome is missing/empty: header could not be parsed.
    Malformed,
}

/// Classify a candidate into a [`SpecKind`].
///
/// Precedence:
/// 1. Both fields absent / empty → `Malformed`
/// 2. Stage=Close + Outcome=Active → `ClosedFollowup`
/// 3. Outcome=Active + Stage ∈ {Analyze, Plan, Execute} → `Active`
/// 4. Anything else → `None` (excluded from the picker)
fn classify_spec(header: &SpecHeader) -> Option<SpecKind> {
    let stage = header.stage.as_deref().unwrap_or("").trim();
    let outcome = header.outcome.as_deref().unwrap_or("").trim();

    if stage.is_empty() && outcome.is_empty() {
        return Some(SpecKind::Malformed);
    }

    let stage_lower = stage.to_ascii_lowercase();
    let outcome_lower = outcome.to_ascii_lowercase();

    if stage_lower == "close" && outcome_lower == "active" {
        return Some(SpecKind::ClosedFollowup);
    }

    if outcome_lower == "active"
        && (stage_lower == "analyze" || stage_lower == "plan" || stage_lower == "execute")
    {
        return Some(SpecKind::Active);
    }

    None
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Keep specs that belong to the picker:
///
/// - `Outcome=Active` + `Stage ∈ {Analyze, Plan, Execute}` (normal active)
/// - `Stage=Close` + `Outcome=Active` (closed-followup — spec closed but
///   generated a follow-up task; must remain visible so the user can act)
/// - Missing or empty `stage` AND `outcome` (malformed — shown with `??` so
///   the user can decide what to do, instead of the spec silently vanishing)
///
/// All other combinations (e.g. `Completed`, `Stage=Close+Outcome=Completed`)
/// are excluded.
fn filter_active(candidates: Vec<SpecCandidate>) -> Vec<SpecCandidate> {
    candidates
        .into_iter()
        .filter(|c| classify_spec(&c.header).is_some())
        .collect()
}

// ---------------------------------------------------------------------------
// Wave progress
// ---------------------------------------------------------------------------

/// Count completed waves for a wave-plan spec.
///
/// Globs `<spec_dir>/wave-N-*/spec.md`, reads each header, counts waves with
/// `Stage=Close` AND `Outcome=Completed`. Returns `Some((done, total))` when
/// there is at least one wave subdir, `None` otherwise.
fn count_wave_progress(spec_dir: &Path) -> Option<WaveProgress> {
    let Ok(entries) = fs::read_dir(spec_dir) else {
        return None;
    };

    let mut total = 0usize;
    let mut done = 0usize;

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        // Must match wave-N-* pattern
        if !entry.file_name.starts_with("wave-") {
            continue;
        }
        // Skip review/ qa/ (they don't start with wave-)
        let wave_spec = entry.path.join("spec.md");
        if !wave_spec.is_file() {
            continue;
        }
        // Verify it's a "wave-N-" directory (has digits after "wave-")
        let after_wave = &entry.file_name["wave-".len()..];
        if !after_wave.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        total += 1;
        let hdr = parse_header(&wave_spec);
        let stage_close = hdr
            .stage
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("close"));
        let outcome_completed = hdr
            .outcome
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case("completed"));
        if stage_close && outcome_completed {
            done += 1;
        }
    }

    if total == 0 {
        None
    } else {
        Some(WaveProgress { done, total })
    }
}

// ---------------------------------------------------------------------------
// Resumo extraction
// ---------------------------------------------------------------------------

/// Strip markdown bold/italic markers (`**`, `*`, `__`, `_`) and wikilinks
/// (`[[X]]` → `X`) from a string.
fn strip_markdown(s: &str) -> String {
    // Walks CHARACTERS, not bytes. Pushing a byte as a `char` re-encodes every
    // non-ASCII scalar as the two Latin-1 characters its UTF-8 bytes look like,
    // which turned every accented resumo in the listing into mojibake — the
    // column is pt-BR prose, so that was most of them.
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(ch) = rest.chars().next() {
        // Strip [[wikilink]]
        if let Some(after) = rest.strip_prefix("[[") {
            if let Some(end) = after.find("]]") {
                out.push_str(after[..end].trim());
                rest = &after[end + "]]".len()..];
                continue;
            }
        }
        // Strip ** or __
        if let Some(after) = rest.strip_prefix("**").or_else(|| rest.strip_prefix("__")) {
            rest = after;
            continue;
        }
        // Strip * or _
        rest = &rest[ch.len_utf8()..];
        if ch != '*' && ch != '_' {
            out.push(ch);
        }
    }
    out
}

/// Truncate a string to at most `max_chars` Unicode characters, appending `…`
/// when truncated. (Characters, not bytes, to avoid splitting multibyte chars.)
fn truncate_str(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = chars[..max_chars].iter().collect();
        format!("{truncated}…")
    }
}

/// Extract a one-line summary (≤70 chars) from a spec.md.
///
/// Priority: `## Resumo` > `## Contexto` > `## Summary` > `## Context`.
/// Takes the first non-empty line after the heading, then truncates at the
/// first sentence break (`.`, `\n\n`, or `:`). Strips wikilinks and markdown
/// bold/italic. Truncates to 70 chars with `…`.
fn extract_resumo(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else { return String::new() };
    extract_resumo_text(&text)
}

/// [`extract_resumo`] over TEXT — the door the cross-branch scan comes through,
/// holding a `spec.md` blob instead of a file.
fn extract_resumo_text(text: &str) -> String {
    // Try headings in priority order
    let headings = ["## Resumo", "## Contexto", "## Summary", "## Context"];
    for heading in headings {
        if let Some(first) = find_section_first_line(text, heading) {
            if first.is_empty() {
                continue;
            }
            // Truncate at first sentence break
            let sentence = first_sentence(&first);
            let cleaned = strip_markdown(&sentence);
            let cleaned = cleaned.trim().to_string();
            if !cleaned.is_empty() {
                return truncate_str(&cleaned, 70);
            }
        }
    }
    String::new()
}

/// Find the first non-empty line after a markdown section heading.
/// Scans the document for a line that starts with `heading` (case-insensitive),
/// then returns the first non-empty line that follows it.
fn find_section_first_line(text: &str, heading: &str) -> Option<String> {
    let heading_lower = heading.to_ascii_lowercase();
    let mut found_heading = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if !found_heading {
            if trimmed.to_ascii_lowercase().starts_with(&heading_lower) {
                found_heading = true;
            }
            continue;
        }
        // We are past the heading: return the first non-empty, non-heading line
        if trimmed.is_empty() {
            continue;
        }
        // Stop at the next heading
        if trimmed.starts_with('#') {
            return None;
        }
        return Some(trimmed.to_string());
    }
    None
}

/// Extract the first sentence from a string (until `.`, `:`, or a blank line).
fn first_sentence(s: &str) -> String {
    // Stop at double newline
    let s = if let Some(idx) = s.find("\n\n") {
        &s[..idx]
    } else {
        s
    };
    // Stop at first period or colon (keeping the character for period)
    let mut result = String::new();
    for ch in s.chars() {
        if ch == '.' {
            result.push(ch);
            break;
        }
        if ch == ':' {
            break;
        }
        result.push(ch);
    }
    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Parent alias resolution
// ---------------------------------------------------------------------------

/// Generate a short alias for each parent spec slug.
///
/// Algorithm: take the last hyphen-separated "word" of the slug, then the
/// first 2 chars. In case of collision, try the last 2 words, then add chars.
/// Guarantees uniqueness.
fn resolve_parent_aliases(parents: &[String]) -> HashMap<String, String> {
    let mut alias_map: HashMap<String, String> = HashMap::new();
    let mut used: HashMap<String, String> = HashMap::new();

    for parent in parents {
        let alias = make_unique_alias(parent, &used);
        used.insert(alias.clone(), parent.clone());
        alias_map.insert(parent.clone(), alias);
    }
    alias_map
}

/// Make a unique alias for `slug` that doesn't collide with anything already
/// in `used` (a map from alias → slug).
fn make_unique_alias(slug: &str, used: &HashMap<String, String>) -> String {
    let words: Vec<&str> = slug.split('-').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return slug.chars().take(2).collect();
    }

    // Skip common date-like prefixes (YYYY)
    let significant: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
        .collect();

    if significant.is_empty() {
        return make_unique_from_chars(slug, used);
    }

    // Strategy 1: first 2 chars of the last significant word
    let last = *significant.last().unwrap_or(&words[words.len() - 1]);
    let candidate = last.chars().take(2).collect::<String>();
    if !used.contains_key(&candidate) {
        return candidate;
    }

    // Strategy 2: first char of second-to-last + first char of last
    if significant.len() >= 2 {
        let second_last = significant[significant.len() - 2];
        let candidate2 = format!(
            "{}{}",
            second_last.chars().next().unwrap_or('_'),
            last.chars().next().unwrap_or('_')
        );
        if !used.contains_key(&candidate2) {
            return candidate2;
        }
    }

    // Strategy 3: initials of last N significant words
    for n in 2..=significant.len() {
        let initials: String = significant[significant.len() - n..]
            .iter()
            .filter_map(|w| w.chars().next())
            .collect();
        if !used.contains_key(&initials) {
            return initials;
        }
    }

    // Strategy 4: keep extending the last word
    make_unique_from_chars(last, used)
}

fn make_unique_from_chars(s: &str, used: &HashMap<String, String>) -> String {
    for n in 2..=s.len().max(2) {
        let candidate: String = s.chars().take(n).collect();
        if !used.contains_key(&candidate) {
            return candidate;
        }
    }
    // Absolute fallback: hash-like suffix
    format!("{}{}", &s.chars().take(2).collect::<String>(), used.len())
}

// ---------------------------------------------------------------------------
// Status derivation
// ---------------------------------------------------------------------------

/// Derive the display status for a spec row in the table.
///
/// `first_active_wave` is supplied by the caller because the answer has two
/// sources — the filesystem for a checked-out spec, the branch's tree for an
/// in-flight one — and this function must not care which.
///
/// `dispatched` says whether anything was ever handed out for this spec
/// ([`plan_was_dispatched`]). It is REQUIRED to word the wave column, because
/// `first_active_wave` alone cannot: `wave-scaffold` writes every wave
/// directory `Outcome=Active` before an agent runs, so the first active wave of
/// a plan nobody started is wave 1 — the same answer a plan whose wave 1 is in
/// flight gives. The two words ask for opposite actions (start it vs. resume
/// it), and this table is the surface where the operator CHOOSES.
fn derive_status(
    spec: &SpecCandidate,
    kind: &SpecKind,
    parent_aliases: &HashMap<String, String>,
    first_active_wave: Option<&str>,
    dispatched: bool,
) -> String {
    // Special kinds override the normal status derivation.
    match kind {
        SpecKind::Malformed => return "⚠ malformed".to_string(),
        SpecKind::ClosedFollowup => return "closed-followup".to_string(),
        SpecKind::Active => {}
    }
    // Tactical fix: has a parent → TF→{alias}
    if let Some(parent) = &spec.header.parent {
        if !parent.is_empty() {
            let alias = parent_aliases
                .get(parent)
                .cloned()
                .unwrap_or_else(|| parent.chars().take(2).collect());
            return format!("TF→{alias}");
        }
    }
    // Wave plan with active waves: "W{N} em exec" once something was actually
    // dispatched, "W{N} a iniciar" while the plan is only scaffolded.
    if spec.is_wave_plan {
        if let Some(wave) = first_active_wave {
            return if dispatched {
                format!("W{wave} em exec")
            } else {
                format!("W{wave} a iniciar")
            };
        }
    }
    "-".to_string()
}

/// Whether this spec's plan was ever actually HANDED OUT, as opposed to merely
/// scaffolded.
///
/// The witness is the dispatch record — the very one `resume-bootstrap` folds
/// into `neverDispatched` ([`wave_dispatch_recorded`]), asked here rather than
/// re-derived, so the picker and the resume cannot answer "has this started?"
/// differently.
///
/// The second clause covers the rows the checkout does not carry. The event log
/// is local: it has nothing to say about a unit whose waves ran in another
/// clone, and a listing that read its silence as "never started" would invent
/// the mirror image of the defect being fixed. A wave already CLOSED on that
/// branch is retroactive proof of a dispatch — the same category
/// `pipeline.wave.complete` occupies in the witness list above.
///
/// Fail-open in the direction of the old reading: with no record and no closed
/// wave, the row says "start it", which is what an empty log means.
fn plan_was_dispatched(
    events: &[HarnessEvent],
    name: &str,
    progress: Option<&WaveProgress>,
) -> bool {
    wave_dispatch_recorded(events, name) || progress.is_some_and(|p| p.done > 0)
}

/// Find the number of the first wave-N subdir that has `Outcome=Active`.
fn find_first_active_wave(spec_dir: &Path) -> Option<String> {
    let Ok(entries) = fs::read_dir(spec_dir) else {
        return None;
    };

    let mut active_waves: Vec<(u32, String)> = Vec::new();

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        if !entry.file_name.starts_with("wave-") {
            continue;
        }
        let after_wave = &entry.file_name["wave-".len()..];
        if !after_wave.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        // Extract wave number
        let num_str: String = after_wave.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(num) = num_str.parse::<u32>() else {
            continue;
        };
        let wave_spec = entry.path.join("spec.md");
        if !wave_spec.is_file() {
            continue;
        }
        let hdr = parse_header(&wave_spec);
        let outcome_active = hdr
            .outcome
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case("active"));
        if outcome_active {
            active_waves.push((num, entry.file_name.clone()));
        }
    }

    active_waves.sort_by_key(|(n, _)| *n);
    active_waves.first().map(|(n, _)| n.to_string())
}

// ---------------------------------------------------------------------------
// Hollow-clarification advisory
// ---------------------------------------------------------------------------

/// `true` when `spec`'s clarification marker EXISTS but records nothing.
///
/// Delegates to [`crate::commands::spec::approve_spec::clarify_state`] — the
/// ONE definition of "hollow" in the crate (it in turn reads
/// `MarkerProvenance::records_substance`). This wrapper only narrows the
/// four-state answer to the single state the listing warns about: `Missing` is
/// not this warning's business (the clarification simply has not run yet) and
/// `NotGated` / `Recorded` are fine.
///
/// Advisory only — the listing never refuses; `approve-spec` still owns the
/// refusal.
fn clarify_records_nothing(root: &Path, spec: &str) -> bool {
    use crate::commands::spec::approve_spec::{clarify_state, ClarifyState};
    clarify_state(&root.to_string_lossy(), spec) == ClarifyState::Hollow
}

// ---------------------------------------------------------------------------
// Scope abbreviation
// ---------------------------------------------------------------------------

fn scope_abbrev(scope: &Option<String>) -> String {
    match scope.as_deref() {
        Some(s) => {
            let lower = s.to_ascii_lowercase();
            if lower.starts_with("light") {
                "lt".to_string()
            } else if lower.starts_with("full") {
                "fl".to_string()
            } else {
                "-".to_string()
            }
        }
        None => "-".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Stage abbreviation
// ---------------------------------------------------------------------------

fn stage_abbrev(stage: &str) -> String {
    match stage.to_ascii_lowercase().as_str() {
        "analyze" => "ANLZ".to_string(),
        "plan" => "PLAN".to_string(),
        "execute" => "EXEC".to_string(),
        // Sentinel values produced by classify_spec for special cases
        "??" => "??".to_string(),
        "clr→fu" => "CLR→fu".to_string(),
        other => other.to_ascii_uppercase().chars().take(4).collect(),
    }
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

/// Width of the `Onde` column — long enough for a `{base}_{slug}` branch to
/// stay recognisable, short enough not to push `Resumo` off the screen.
const ONDE_WIDTH: usize = 20;

/// Width of the `Status` column — wide enough for the LONGEST value
/// [`derive_status`] can return, so no row pushes `Onde`/`Resumo` out of
/// alignment.
///
/// The census: `closed-followup` (15) is the widest, then `W10 a iniciar` (13),
/// `W1 a iniciar` (12), `⚠ malformed` (11), `W1 em exec` (10), `TF→xx` (5),
/// `-` (1). The column was 10 while the widest was 15, so every long row shifted
/// the two columns to its right — a table that mis-renders the very status this
/// unit added is the same class of defect it exists to remove.
///
/// Header, separator and row all derive from this one number; they cannot drift.
const STATUS_WIDTH: usize = 15;

/// Generate a markdown table from the list of active specs.
///
/// Columns: `#`, `Spec`, `Esc`, `Estágio`, `Prog`, `Status`, `Onde`, `Resumo`
///
/// `lang` drives the legend line for the third `Onde` value — new user-facing
/// copy comes from the catalogue, never from a literal in this file.
fn render_table(specs: &[ActiveSpec], scan: &BranchScan, lang: SupportedLocale) -> String {
    // Column headers — the `Status` cell is built from STATUS_WIDTH rather than
    // typed out, so widening the column can never leave the header behind.
    let header = format!(
        "| #  | Spec                                          | Esc | Estágio | Prog | {label:<width$} | Onde                 | Resumo",
        label = "Status",
        width = STATUS_WIDTH,
    );
    let separator = format!(
        "|----|-----------------------------------------------|-----|---------|------|{dashes}|----------------------|-----------------------------------------------------|",
        dashes = "-".repeat(STATUS_WIDTH + 2),
    );

    let mut lines: Vec<String> = Vec::new();
    lines.push(header);
    lines.push(separator);

    for spec in specs {
        let prog = spec
            .progress
            .as_ref()
            .map_or_else(|| " - ".to_string(), |p| format!("{}/{}", p.done, p.total));

        let scope_str = spec
            .scope
            .as_ref()
            .map_or_else(|| "-".to_string(), |s| scope_abbrev(&Some(s.clone())));

        let stage_str = stage_abbrev(&spec.stage);

        // Pad/truncate columns for alignment
        let letter = format!("{:<2}", spec.letter);
        let name = format!("{:<45}", &spec.name);
        let esc = format!("{scope_str:<3}");
        let stage_col = format!("{stage_str:<7}");
        let prog_col = format!("{prog:>4}");
        let status_col = format!("{:<width$}", spec.status, width = STATUS_WIDTH);
        // `-` for the checkout, the branch name for an in-flight spec: the eye
        // only catches the rows that are NOT where the operator is standing.
        let onde = spec
            .branch
            .as_deref()
            .map_or_else(|| "-".to_string(), |b| truncate_str(b, ONDE_WIDTH - 1));
        let onde_col = format!("{onde:<width$}", width = ONDE_WIDTH);
        let resumo_col = &spec.resumo;

        lines.push(format!(
            "| {letter} | {name} | {esc} | {stage_col} | {prog_col} | {status_col} | {onde_col} | {resumo_col}"
        ));
    }

    // Legend — always rendered so the user does not need to memorise siglas.
    // Keep in sync with stage_abbrev() and derive_status().
    lines.push(String::new());
    lines.push(
        "Estágio: ANLZ=Analyze  PLAN=Plan  EXEC=Execute  ??=malformed (meta ausente)  CLR→fu=closed-followup (Close+Active)".to_string(),
    );
    lines.push(
        "Esc: lt=light  fl=full  Status: TF→xx=tactical-fix  Wn em exec=wave em execução  Wn a iniciar=plano montado, nada despachado ainda (comece por ela)  ⚠ malformed=meta incompleta  closed-followup=spec fechada com follow-up pendente".to_string(),
    );
    lines.push(
        "Onde: -=na árvore atual  {branch}=spec em voo, o diretório só existe nesse branch (troque de branch antes de agir)".to_string(),
    );
    // The third `Onde` value: a unit alive only on a remote. Catalogue copy —
    // the row exists because the enumerator sweeps `refs/remotes/` too.
    lines.push(translate("specs.location.remote_only", lang).to_string());

    // Fail-open, said out loud: without the branch scan this listing covers the
    // checkout ONLY, which is a different claim from "estas são todas".
    if !scan.ok {
        lines.push(format!(
            "⚠ Varredura de branches indisponível ({reason}) — a listagem cobre APENAS a árvore atual; \
             specs em voo em outros branches podem não aparecer.",
            reason = scan.reason.as_deref().unwrap_or("motivo não informado")
        ));
    }

    // Advisory block (never blocking): a `.clarified` that records NOTHING is
    // refused by `approve-spec`. Saying it here costs the operator nothing; not
    // saying it costs a refusal at the moment the implementation was requested.
    for spec in specs.iter().filter(|s| s.clarify_records_nothing) {
        lines.push(format!(
            "⚠ {name}: `.clarified` existe mas não registra termo nem motivo — approve-spec vai recusar. \
             Rode `mustard-rt run grill-capture --finalize --spec {name} --term <termo>` (um por termo) \
             ou `--reason \"<frase>\"`.",
            name = spec.name
        ));
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// JSON rendering
// ---------------------------------------------------------------------------

/// Serialize the output to pretty JSON.
fn render_json(output: &ActiveSpecsOutput) -> String {
    serde_json::to_string_pretty(output).unwrap_or_else(|_| r#"{"specs":[],"parentMap":{},"backfilledCount":0}"#.to_string())
}

// ---------------------------------------------------------------------------
// Date parsing for sort
// ---------------------------------------------------------------------------

/// Extract the `YYYY-MM-DD` prefix from a spec name for date-descending sort.
/// Returns `"0000-00-00"` for names that don't start with a date.
fn spec_date_prefix(name: &str) -> &str {
    if name.len() >= 10 && name.chars().nth(4) == Some('-') && name.chars().nth(7) == Some('-') {
        &name[..10]
    } else {
        "0000-00-00"
    }
}

// ---------------------------------------------------------------------------
// Active-pipeline count (consumed by `active_spec_limit_gate`)
// ---------------------------------------------------------------------------

/// Count the **running pipelines** rooted under `<root>/.claude/spec/`.
///
/// A running pipeline is a top-level spec classified [`SpecKind::Active`]
/// (`Outcome=Active` + `Stage ∈ {Analyze, Plan, Execute}`) — the exact set the
/// picker shows as in-flight. `ClosedFollowup` and `Malformed` specs are
/// **not** counted: a closed-followup is a finished pipeline awaiting a
/// follow-up action, and a malformed spec has no resolvable lifecycle, so
/// neither represents a concurrent pipeline competing for attention.
///
/// Uses the same deterministic projection as [`run`] —
/// [`discover_root_specs`] + [`classify_spec`] — so the gate's count and the
/// `active-specs` picker can never disagree about the working tree.
///
/// **The working tree ONLY** — deliberately, unlike [`run`], which also lists
/// the specs other work branches carry. This number gates an EDIT, and an edit
/// happens in one checkout: a pipeline parked on a branch nobody has out is not
/// competing for attention here, and counting it would refuse writes over work
/// that is not in the room.
///
/// **Fail-open by construction.** Every discovery step degrades to "fewer
/// specs" on an IO error (a missing / unreadable `.claude/spec` reads as an
/// empty directory). A counting error can therefore only *under*-count, never
/// over-count, so it can never spuriously trip the limit. The gate treats this
/// value as authoritative for an *upper* bound check.
#[must_use]
pub fn count_active(root: &Path) -> usize {
    discover_root_specs(root)
        .iter()
        .filter(|c| classify_spec(&c.header) == Some(SpecKind::Active))
        .count()
}

/// The spec a picker ROW LETTER names, resolved through the SAME enumeration
/// the table the user read was rendered from ([`build_output`], whose
/// `'a'..='z'` is assigned by row index). `None` when no row carries that
/// letter — the caller treats that as "decide nothing".
///
/// **Why this exists rather than "the active spec".** A consumer that reads the
/// letter cannot fall back to the session's current spec: the picker's whole
/// purpose is choosing a row that is NOT the current one, so a fallback
/// attributes the user's choice to whatever the session happened to be bound
/// to. The letter IS the user's naming of the row, and it is only meaningful
/// against the ordering the table showed — which is why this resolves through
/// the enumerator itself instead of a second copy of the sort.
///
/// Case-folded: the letters are rendered lowercase, and a user who types `AR`
/// named the same row.
#[must_use]
pub(crate) fn spec_for_letter(root: &Path, letter: char) -> Option<String> {
    let wanted = letter.to_ascii_lowercase().to_string();
    let (output, _) = build_output(root);
    output.specs.into_iter().find(|s| s.letter == wanted).map(|s| s.name)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run active-specs`.
pub struct ActiveSpecsOpts {
    /// Output format: `table` (default) or `json`.
    pub format: String,
    /// Project root directory (default: cwd).
    pub root: PathBuf,
}

/// Main entry point for `mustard-rt run active-specs`.
pub fn run(opts: ActiveSpecsOpts) {
    let (output, extra) = build_output(&opts.root);

    match opts.format.as_str() {
        "json" => {
            println!("{}", render_json(&output));
        }
        _ => {
            // table (default)
            let lang = mustard_core::ProjectConfig::load(&opts.root).i18n().lang;
            println!("{}", render_table(&output.specs, &output.branch_scan, lang));
            if extra > 0 {
                println!("\n({extra} specs adicionais)");
            }
        }
    }
}

/// Build the whole listing for `root` — the projection with no stdout in it, so
/// the rendering above and the tests below read the SAME answer.
///
/// Returns the output plus how many active specs the 26-letter cap left out.
fn build_output(root: &Path) -> (ActiveSpecsOutput, usize) {
    // 1. Discover all root-level spec.md / wave-plan.md in the WORKING TREE
    let mut candidates = discover_root_specs(root);

    // 2. Extend beyond the checkout: the specs the project's work branches
    //    carry and this tree does not. Every tree name — active or not — is
    //    already `seen`, so the checkout always wins the slug.
    let mut seen: BTreeSet<String> = candidates.iter().map(|c| c.name.clone()).collect();
    let (branch_candidates, branch_scan) = scan_work_branches(root, &mut seen);
    candidates.extend(branch_candidates);

    // 3. Parse headers and filter to active specs
    candidates = filter_active(candidates);

    // 3b. The dispatch record, read ONCE for the whole listing — and only when
    //     a wave plan is actually being rendered, so a listing with none never
    //     pays for the walk. Fail-open: an unreadable log is an empty slice.
    let events: Vec<HarnessEvent> = if candidates.iter().any(|c| c.is_wave_plan) {
        read_workspace_events(root)
    } else {
        Vec::new()
    };

    // 4. Sort by date descending (newest first), name ascending to break ties —
    //    two sources merge here, so the order must not depend on read_dir.
    candidates.sort_by(|a, b| {
        spec_date_prefix(&b.name)
            .cmp(spec_date_prefix(&a.name))
            .then_with(|| a.name.cmp(&b.name))
    });

    // 4. Collect all unique parents for alias resolution
    let parents: Vec<String> = candidates
        .iter()
        .filter_map(|c| c.header.parent.clone())
        .filter(|p| !p.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    // Sort for deterministic alias assignment
    let mut parents_sorted = parents.clone();
    parents_sorted.sort();
    let parent_aliases = resolve_parent_aliases(&parents_sorted);

    // 6. Build ActiveSpec entries (capped at 26)
    let letters: Vec<char> = ('a'..='z').collect();
    let mut specs: Vec<ActiveSpec> = Vec::new();
    let cap = candidates.len().min(26);

    for (i, candidate) in candidates.iter().enumerate().take(cap) {
        let letter = letters[i].to_string();

        // Classify the spec to drive stage_code and status display.
        // classify_spec is guaranteed Some here because filter_active already
        // removed all None-classified candidates; unwrap_or is just a safe fallback.
        let kind = classify_spec(&candidate.header).unwrap_or(SpecKind::Active);

        // Stage and outcome: use sentinel values for special kinds.
        let (stage, outcome) = match kind {
            SpecKind::Malformed => (
                "??".to_string(),
                candidate.header.outcome.clone().unwrap_or_default(),
            ),
            SpecKind::ClosedFollowup => (
                "CLR→fu".to_string(),
                candidate.header.outcome.clone().unwrap_or_else(|| "Active".to_string()),
            ),
            SpecKind::Active => (
                candidate.header.stage.clone().unwrap_or_else(|| "Plan".to_string()),
                candidate.header.outcome.clone().unwrap_or_else(|| "Active".to_string()),
            ),
        };

        // Scope — wave plans are always "fl"
        let scope = if candidate.is_wave_plan {
            Some("full".to_string())
        } else {
            candidate.header.scope.clone().map(|s| {
                // Normalize: strip extra annotations like "full (wave N of M)"
                let lower = s.to_ascii_lowercase();
                if lower.starts_with("full") {
                    "full".to_string()
                } else if lower.starts_with("light") {
                    "light".to_string()
                } else {
                    s
                }
            })
        };

        let parent = candidate.header.parent.clone().filter(|p| !p.is_empty());
        let parent_alias = parent
            .as_ref()
            .and_then(|p| parent_aliases.get(p))
            .cloned();

        // An in-flight spec has no directory here: everything the filesystem
        // would have answered was read from its branch's tree instead.
        let facts = candidate.branch_facts.as_ref();

        let progress = match (candidate.is_wave_plan, facts) {
            (false, _) => None,
            (true, Some(f)) => f.progress.clone(),
            (true, None) => count_wave_progress(&candidate.spec_dir),
        };

        let resumo = match facts {
            Some(f) => f.resumo.clone(),
            None if candidate.spec_md.is_file() => extract_resumo(&candidate.spec_md),
            None => String::new(),
        };

        let first_active_wave = match (candidate.is_wave_plan, facts) {
            (false, _) => None,
            (true, Some(f)) => f.first_active_wave.clone(),
            (true, None) => find_first_active_wave(&candidate.spec_dir),
        };

        let dispatched = plan_was_dispatched(&events, &candidate.name, progress.as_ref());
        let status = derive_status(
            candidate,
            &kind,
            &parent_aliases,
            first_active_wave.as_deref(),
            dispatched,
        );

        let (location, branch) = match &candidate.location {
            SpecLocation::Tree => ("tree".to_string(), None),
            SpecLocation::Branch(b) => ("branch".to_string(), Some(b.clone())),
            SpecLocation::Remote(r) => ("remote".to_string(), Some(r.clone())),
        };

        // Advisory: reuse the approval gate's own classifier so the listing and
        // the refusal can never disagree about what "hollow" means.
        let clarify_records_nothing = clarify_records_nothing(root, &candidate.name);

        specs.push(ActiveSpec {
            name: candidate.name.clone(),
            stage,
            outcome,
            scope,
            parent,
            parent_alias,
            progress,
            resumo,
            letter,
            status,
            clarify_records_nothing,
            location,
            branch,
        });
    }

    // 7. Note if there were more than 26 specs
    let extra = candidates.len().saturating_sub(26);

    // 8. Assemble
    let output = ActiveSpecsOutput {
        specs,
        parent_map: parent_aliases.into_iter().map(|(k, v)| (v, k)).collect(),
        branch_scan,
    };
    (output, extra)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// A branch scan that succeeded and found nothing — the neutral value for
    /// tests that are not about the cross-branch advisory.
    fn ok_scan() -> BranchScan {
        BranchScan { ok: true, reason: None, branches: Vec::new() }
    }

    fn make_wave_spec(root: &Path, parent: &str, wave: &str, stage: &str, outcome: &str) {
        let dir = root
            .join(".claude")
            .join("spec")
            .join(parent)
            .join(wave);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("spec.md"),
            format!("# Wave {wave}\n\n### Stage: {stage}\n### Outcome: {outcome}\n"),
        )
        .unwrap();
    }

    // -----------------------------------------------------------------------
    // parse_header tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_header_all_four_fields() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n### Stage: Execute\n### Outcome: Active\n### Scope: light\n### Parent: my-parent\n\n## Body\n",
        )
        .unwrap();
        let h = parse_header(&path);
        assert_eq!(h.stage.as_deref(), Some("Execute"));
        assert_eq!(h.outcome.as_deref(), Some("Active"));
        assert_eq!(h.scope.as_deref(), Some("light"));
        assert_eq!(h.parent.as_deref(), Some("my-parent"));
    }

    #[test]
    fn parse_header_strips_wikilink_from_parent() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n### Stage: Plan\n### Parent: [[my-parent-slug]]\n",
        )
        .unwrap();
        let h = parse_header(&path);
        assert_eq!(h.parent.as_deref(), Some("my-parent-slug"));
    }

    #[test]
    fn parse_header_uses_first_occurrence_on_drift() {
        // spec that has duplicate Stage header (body drift)
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n### Stage: Close\n### Outcome: Completed\n\n## Body\n\n### Stage: Execute\n### Outcome: Active\n",
        )
        .unwrap();
        let h = parse_header(&path);
        // Must use Close (first occurrence), NOT Execute
        assert_eq!(h.stage.as_deref(), Some("Close"));
        assert_eq!(h.outcome.as_deref(), Some("Completed"));
    }

    #[test]
    fn parse_header_missing_file_returns_default() {
        let h = parse_header(Path::new("/nonexistent/path/spec.md"));
        assert!(h.stage.is_none());
        assert!(h.outcome.is_none());
    }

    // -----------------------------------------------------------------------
    // filter_active tests
    // -----------------------------------------------------------------------

    fn make_candidate(name: &str, stage: &str, outcome: &str) -> SpecCandidate {
        SpecCandidate {
            name: name.to_string(),
            spec_dir: PathBuf::from(format!(".claude/spec/{name}")),
            spec_md: PathBuf::from(format!(".claude/spec/{name}/spec.md")),
            is_wave_plan: false,
            header: SpecHeader {
                stage: Some(stage.to_string()),
                outcome: Some(outcome.to_string()),
                scope: None,
                parent: None,
                checkpoint: None,
            },
            location: SpecLocation::Tree,
            branch_facts: None,
        }
    }

    #[test]
    fn filter_active_keeps_analyze_plan_and_execute_with_active_outcome() {
        let candidates = vec![
            make_candidate("a", "Plan", "Active"),
            make_candidate("b", "Execute", "Active"),
            make_candidate("c", "Close", "Completed"),
            make_candidate("d", "Plan", "Completed"),
            make_candidate("e", "Close", "Active"),
            make_candidate("f", "Analyze", "Active"),
            make_candidate("g", "Analyze", "Completed"),
        ];
        let active = filter_active(candidates);
        let names: Vec<&str> = active.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"a"), "Plan+Active should pass");
        assert!(names.contains(&"b"), "Execute+Active should pass");
        assert!(!names.contains(&"c"), "Close+Completed should be filtered");
        assert!(!names.contains(&"d"), "Plan+Completed should be filtered");
        assert!(names.contains(&"e"), "Close+Active should pass as closed-followup");
        assert!(names.contains(&"f"), "Analyze+Active should pass");
        assert!(!names.contains(&"g"), "Analyze+Completed should be filtered");
    }

    // -----------------------------------------------------------------------
    // count_wave_progress tests
    // -----------------------------------------------------------------------

    #[test]
    fn count_wave_progress_four_close_two_plan() {
        let td = tempdir().unwrap();
        let parent = "my-wave-spec";
        // 4 closed waves
        for i in 1..=4 {
            make_wave_spec(td.path(), parent, &format!("wave-{i}-impl"), "Close", "Completed");
        }
        // 2 active waves
        make_wave_spec(td.path(), parent, "wave-5-impl", "Plan", "Active");
        make_wave_spec(td.path(), parent, "wave-6-impl", "Plan", "Active");

        let spec_dir = td.path().join(".claude").join("spec").join(parent);
        let progress = count_wave_progress(&spec_dir).expect("should have progress");
        assert_eq!(progress.done, 4);
        assert_eq!(progress.total, 6);
    }

    #[test]
    fn count_wave_progress_none_when_no_waves() {
        let td = tempdir().unwrap();
        let spec_dir = td.path().join(".claude").join("spec").join("no-waves");
        std::fs::create_dir_all(&spec_dir).unwrap();
        assert!(count_wave_progress(&spec_dir).is_none());
    }

    // -----------------------------------------------------------------------
    // extract_resumo tests
    // -----------------------------------------------------------------------

    #[test]
    fn extract_resumo_prefers_resumo_over_contexto() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n## Resumo\n\nPrimeira frase do resumo.\n\n## Contexto\n\nDeve ser ignorado.\n",
        )
        .unwrap();
        let r = extract_resumo(&path);
        assert!(r.contains("Primeira frase"), "got: {r:?}");
        assert!(!r.contains("Deve ser ignorado"), "got: {r:?}");
    }

    #[test]
    fn extract_resumo_falls_back_to_contexto() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n## Contexto\n\nContexto do spec aqui.\n",
        )
        .unwrap();
        let r = extract_resumo(&path);
        assert!(r.contains("Contexto"), "got: {r:?}");
    }

    /// The resumo column is prose in the project's language: an extractor that
    /// walked bytes turned every accent into mojibake on the way out, while
    /// still stripping the markers around it.
    #[test]
    fn extract_resumo_keeps_accents_and_still_strips_markers() {
        assert_eq!(strip_markdown("**Só** no _remoto_"), "Só no remoto");
        assert_eq!(strip_markdown("ação e [[link-ção]] após"), "ação e link-ção após");
    }

    #[test]
    fn extract_resumo_truncates_at_70_chars() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        let long_text = "A".repeat(100);
        std::fs::write(&path, format!("# Title\n\n## Resumo\n\n{long_text}\n")).unwrap();
        let r = extract_resumo(&path);
        // Should be truncated to 70 chars + ellipsis
        let char_count = r.chars().count();
        // The last char should be the ellipsis "…" (1 char) after 70 chars
        assert!(char_count <= 72, "expected ≤72 chars (70 + ellipsis), got {char_count}: {r:?}");
        assert!(r.ends_with('…'), "expected ellipsis suffix, got: {r:?}");
    }

    // -----------------------------------------------------------------------
    // resolve_parent_aliases tests
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_parent_aliases_unique_for_different_last_tokens() {
        let parents = vec![
            "2026-05-23-dashboard-design-system".to_string(),
            "2026-05-23-flatten-spec-layout-and-multi-collab".to_string(),
        ];
        let map = resolve_parent_aliases(&parents);
        let aliases: Vec<&String> = map.values().collect();
        // All aliases must be unique
        let unique: std::collections::HashSet<&&String> = aliases.iter().collect();
        assert_eq!(unique.len(), aliases.len(), "aliases must be unique: {map:?}");
    }

    #[test]
    fn resolve_parent_aliases_no_collisions_with_many_parents() {
        let parents: Vec<String> = (0..10)
            .map(|i| format!("2026-05-23-spec-number-{i:02}"))
            .collect();
        let map = resolve_parent_aliases(&parents);
        let aliases: Vec<&String> = map.values().collect();
        let unique: std::collections::HashSet<&&String> = aliases.iter().collect();
        assert_eq!(unique.len(), aliases.len(), "all aliases unique: {map:?}");
    }

    // -----------------------------------------------------------------------
    // T3.4 — malformed + closed-followup inclusion tests
    // -----------------------------------------------------------------------

    /// Helper: create a spec dir with an explicit meta.json sidecar.
    fn make_spec_with_meta(root: &Path, name: &str, stage: &str, outcome: &str) {
        let dir = root.join(".claude").join("spec").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        // Write a minimal spec.md (no header lines — meta.json is authoritative)
        std::fs::write(dir.join("spec.md"), format!("# {name}\n\n## Resumo\n\nTest spec.\n")).unwrap();
        // Write meta.json with the requested fields
        let meta_content = if stage.is_empty() && outcome.is_empty() {
            // Intentionally omit both fields to trigger malformed path
            r#"{"scope":null,"parent":null,"checkpoint":null}"#.to_string()
        } else {
            format!(r#"{{"stage":"{stage}","outcome":"{outcome}","scope":null,"parent":null,"checkpoint":null}}"#)
        };
        std::fs::write(dir.join("meta.json"), meta_content).unwrap();
    }

    /// Helper: create a spec dir with NO meta.json and NO header lines in spec.md.
    fn make_spec_no_meta(root: &Path, name: &str) {
        let dir = root.join(".claude").join("spec").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), format!("# {name}\n\n## Resumo\n\nOrphaned spec.\n")).unwrap();
        // Deliberately no meta.json
    }

    /// Helper: create a spec dir with NO meta.json but WITH a canonical
    /// `### Stage:` / `### Outcome:` header in spec.md — i.e. a spec written
    /// before the meta.json sidecar existed.
    fn make_spec_header_only(root: &Path, name: &str, stage: &str, outcome: &str) {
        let dir = root.join(".claude").join("spec").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("spec.md"),
            format!("# {name}\n\n### Stage: {stage}\n### Outcome: {outcome}\n### Flags: \n\n## Resumo\n\nLegacy spec.\n"),
        )
        .unwrap();
        // Deliberately no meta.json — the .md header is the only source.
    }

    /// Legacy-read contract (F4-f item 1): a spec ALREADY ON DISK that has its
    /// lifecycle header in `spec.md` and **no `meta.json`** must still be read
    /// correctly via the markdown fallback — classified Active (not Malformed)
    /// and surfacing its real stage. This protects specs written before
    /// meta.json became the single source of lifecycle state.
    #[test]
    fn active_specs_reads_header_only_legacy_spec_via_fallback() {
        let td = tempdir().unwrap();
        make_spec_header_only(td.path(), "2026-01-01-legacy-header", "Execute", "Active");
        // No meta.json sidecar exists for this spec.
        assert!(!td
            .path()
            .join(".claude")
            .join("spec")
            .join("2026-01-01-legacy-header")
            .join("meta.json")
            .exists());

        let candidates = discover_root_specs(td.path());
        let filtered = filter_active(candidates);

        let legacy = filtered
            .iter()
            .find(|c| c.name == "2026-01-01-legacy-header")
            .expect("legacy header-only spec must be discovered via .md fallback");
        // Read straight from the .md header — Execute/Active.
        assert_eq!(legacy.header.stage.as_deref(), Some("Execute"));
        assert_eq!(legacy.header.outcome.as_deref(), Some("Active"));
        // And it classifies as a normal Active spec, NOT malformed.
        assert_eq!(classify_spec(&legacy.header), Some(SpecKind::Active));
    }

    /// A header-only spec whose terminal state is Close/Completed must be
    /// filtered OUT of the active set (proving the fallback parse drives the
    /// same filtering meta.json would).
    #[test]
    fn active_specs_filters_terminal_header_only_legacy_spec() {
        let td = tempdir().unwrap();
        make_spec_header_only(td.path(), "2026-01-01-done-legacy", "Close", "Completed");
        let candidates = discover_root_specs(td.path());
        let filtered = filter_active(candidates);
        assert!(
            !filtered.iter().any(|c| c.name == "2026-01-01-done-legacy"),
            "terminal header-only spec must be filtered out via .md fallback"
        );
    }

    #[test]
    fn active_specs_includes_malformed_with_question_marks() {
        let td = tempdir().unwrap();
        // Valid spec
        make_spec_with_meta(td.path(), "2026-01-01-valid-spec", "Plan", "Active");
        // Malformed: no meta.json at all (falls through to .md parser which finds no headers)
        make_spec_no_meta(td.path(), "2026-01-02-no-meta-spec");

        let candidates = discover_root_specs(td.path());
        let filtered = filter_active(candidates);

        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"2026-01-01-valid-spec"), "valid spec must be present");
        assert!(names.contains(&"2026-01-02-no-meta-spec"), "malformed spec must be present");

        // Verify classification
        let malformed = filtered.iter().find(|c| c.name == "2026-01-02-no-meta-spec").unwrap();
        let kind = classify_spec(&malformed.header);
        assert_eq!(kind, Some(SpecKind::Malformed), "must be classified as Malformed");
    }

    #[test]
    fn active_specs_includes_closed_followup() {
        let td = tempdir().unwrap();
        // Normal active spec
        make_spec_with_meta(td.path(), "2026-01-01-active", "Plan", "Active");
        // Closed-followup: Stage=Close + Outcome=Active
        make_spec_with_meta(td.path(), "2026-01-02-closed-fu", "Close", "Active");

        let candidates = discover_root_specs(td.path());
        let filtered = filter_active(candidates);

        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"2026-01-02-closed-fu"), "closed-followup must be present");

        let fu = filtered.iter().find(|c| c.name == "2026-01-02-closed-fu").unwrap();
        let kind = classify_spec(&fu.header);
        assert_eq!(kind, Some(SpecKind::ClosedFollowup), "must be ClosedFollowup");
    }

    #[test]
    fn active_specs_all_three_kinds_present_in_output() {
        let td = tempdir().unwrap();
        make_spec_with_meta(td.path(), "2026-01-01-normal", "Plan", "Active");
        make_spec_with_meta(td.path(), "2026-01-02-fu", "Close", "Active");
        make_spec_no_meta(td.path(), "2026-01-03-broken");

        let opts = ActiveSpecsOpts {
            format: "json".to_string(),
            root: td.path().to_path_buf(),
        };

        // Capture stdout via manual pipeline: call run() and check the candidates
        // directly (run() prints to stdout; we test the data layer instead).
        let candidates = discover_root_specs(td.path());
        let filtered = filter_active(candidates);
        assert_eq!(filtered.len(), 3, "all three specs must appear; got: {:?}",
            filtered.iter().map(|c| &c.name).collect::<Vec<_>>());

        let kinds: Vec<Option<SpecKind>> = filtered.iter().map(|c| classify_spec(&c.header)).collect();
        assert!(kinds.contains(&Some(SpecKind::Active)), "Active missing");
        assert!(kinds.contains(&Some(SpecKind::ClosedFollowup)), "ClosedFollowup missing");
        assert!(kinds.contains(&Some(SpecKind::Malformed)), "Malformed missing");

        // stage_code checks (what the JSON field will carry)
        for c in &filtered {
            let kind = classify_spec(&c.header).unwrap();
            let stage_code = match kind {
                SpecKind::Malformed => "??".to_string(),
                SpecKind::ClosedFollowup => "CLR→fu".to_string(),
                SpecKind::Active => c.header.stage.clone().unwrap_or_default(),
            };
            match kind {
                SpecKind::Malformed => assert_eq!(stage_code, "??"),
                SpecKind::ClosedFollowup => assert_eq!(stage_code, "CLR→fu"),
                SpecKind::Active => assert_eq!(stage_code, "Plan"),
            }
        }

        // Verify opts compiles (satisfies AC-W3.4 that the struct is usable)
        drop(opts);
    }

    // -----------------------------------------------------------------------
    // strip_markdown tests
    // -----------------------------------------------------------------------

    #[test]
    fn strip_markdown_removes_bold_and_wikilinks() {
        let input = "This is **bold** and [[a-wikilink]] text";
        let result = strip_markdown(input);
        assert_eq!(result, "This is bold and a-wikilink text");
    }

    // -----------------------------------------------------------------------
    // spec_date_prefix tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Hollow-clarification advisory
    // -----------------------------------------------------------------------

    /// Seed a Full spec whose `.clarified` marker carries the given body.
    fn make_full_spec_with_clarify(root: &Path, name: &str, marker_body: &str) {
        let dir = root.join(".claude").join("spec").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), format!("# {name}\n\n## Resumo\n\nFull spec.\n"))
            .unwrap();
        std::fs::write(
            dir.join("meta.json"),
            r#"{"stage":"Plan","outcome":"Active","scope":"full (wave plan)"}"#,
        )
        .unwrap();
        std::fs::write(dir.join(".clarified"), marker_body).unwrap();
    }

    /// The listing now SAYS that a `.clarified` marker records nothing, instead
    /// of staying silent until `approve-spec` refuses.
    ///
    /// Asserts both halves: the new signal (flag + the advisory line naming the
    /// minting command) and the disappearance of the old silence — the rendered
    /// table used to carry no trace of a hollow marker at all. A marker that
    /// DOES record something is the control: it must stay silent, so the warning
    /// is the hollow state and not merely "a marker exists".
    #[test]
    fn a_hollow_clarify_marker_is_visible_before_the_approval_gesture() {
        let td = tempdir().unwrap();
        // Hollow: `via=` present (so the body parses) but no terms, no reason.
        make_full_spec_with_clarify(
            td.path(),
            "2026-01-01-hollow",
            "spec=2026-01-01-hollow\nvia=grill-finalize\nsession=s\nat=t\n",
        );
        // Control: the same marker, but recording what was settled.
        make_full_spec_with_clarify(
            td.path(),
            "2026-01-02-recorded",
            "spec=2026-01-02-recorded\nvia=grill-finalize\nsession=s\nat=t\nterms=payable\n",
        );

        assert!(
            clarify_records_nothing(td.path(), "2026-01-01-hollow"),
            "a `.clarified` naming no term and no reason must be flagged"
        );
        assert!(
            !clarify_records_nothing(td.path(), "2026-01-02-recorded"),
            "a marker that records a term must NOT be flagged"
        );

        // The rendered listing carries the advisory — the old behaviour (total
        // silence in the picker) is gone.
        let hollow = ActiveSpec {
            name: "2026-01-01-hollow".to_string(),
            stage: "Plan".to_string(),
            outcome: "Active".to_string(),
            scope: Some("full".to_string()),
            parent: None,
            parent_alias: None,
            progress: None,
            resumo: "Full spec.".to_string(),
            letter: "a".to_string(),
            status: "-".to_string(),
            clarify_records_nothing: true,
            location: "tree".to_string(),
            branch: None,
        };
        let recorded = ActiveSpec {
            name: "2026-01-02-recorded".to_string(),
            letter: "b".to_string(),
            clarify_records_nothing: false,
            ..hollow.clone()
        };
        let table = render_table(&[hollow, recorded], &ok_scan(), SupportedLocale::default());
        assert!(
            table.contains("2026-01-01-hollow: `.clarified` existe mas não registra"),
            "the advisory must name the hollow spec: {table}"
        );
        assert!(
            table.contains("grill-capture --finalize --spec 2026-01-01-hollow"),
            "the advisory must name the minting command: {table}"
        );
        assert!(
            !table.contains("2026-01-02-recorded: `.clarified`"),
            "a spec whose marker records substance must not be warned about: {table}"
        );
        // Advisory only — nothing about the row itself changed.
        assert!(table.contains("| a  | 2026-01-01-hollow"), "row still listed: {table}");
    }

    // -----------------------------------------------------------------------
    // Scaffolded vs. running
    // -----------------------------------------------------------------------

    /// Seed a wave-plan spec exactly as `wave-scaffold` leaves it: every wave
    /// directory materialised and `Outcome=Active`, none of them closed.
    fn make_scaffolded_plan(root: &Path, name: &str) {
        let dir = root.join(".claude").join("spec").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), format!("# {name}\n\n## Resumo\n\nPlano.\n")).unwrap();
        std::fs::write(dir.join("wave-plan.md"), "# wave plan\n").unwrap();
        std::fs::write(dir.join("meta.json"), r#"{"stage":"Execute","outcome":"Active"}"#).unwrap();
        for wave in ["wave-1-a", "wave-2-b"] {
            let wave_dir = dir.join(wave);
            std::fs::create_dir_all(&wave_dir).unwrap();
            std::fs::write(wave_dir.join("spec.md"), format!("# {wave}\n")).unwrap();
            std::fs::write(
                wave_dir.join("meta.json"),
                r#"{"stage":"Execute","outcome":"Active"}"#,
            )
            .unwrap();
        }
    }

    /// Put ONE `pipeline.wave.start` for `name` on the record — the witness
    /// `resume-bootstrap` reads, in the shard layout it reads it from.
    fn record_a_dispatch(root: &Path, name: &str) {
        let events_dir = root.join(".claude").join("spec").join(name).join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();
        let line = serde_json::json!({
            "event": mustard_core::domain::model::event::EVENT_PIPELINE_WAVE_START,
            "kind": "pipeline",
            "ts": "2026-02-01T00:00:00.000Z",
            "v": 1,
            "spec": name,
            "session_id": "seed",
            "wave": 1,
            "actor": "test",
            "payload": { "wave": 1 },
        });
        std::fs::write(events_dir.join("seed.ndjson"), format!("{line}\n")).unwrap();
    }

    /// **AC-4.** A wave directory is born `Outcome=Active` at scaffold time, so
    /// the first active wave of a plan NOBODY dispatched is wave 1 — the same
    /// answer a plan whose wave 1 is in flight gives. The table used to print
    /// `W1 em exec` for both, which asks the operator to RESUME work that never
    /// started; measured live as `0/2` + `W1 em exec` against a spec
    /// `resume-bootstrap` reported as `neverDispatched: true`.
    ///
    /// Both halves are asserted, on two trees that differ ONLY by the dispatch
    /// record: the scaffolded plan stops claiming execution (and the legend
    /// explains the word it prints instead), while the plan with one
    /// `pipeline.wave.start` on record still reads `em exec` — otherwise the fix
    /// would just have moved the wrong word onto the other state.
    #[test]
    fn a_scaffolded_plan_is_not_reported_as_running() {
        let name = "2026-02-01-scaffolded-plan";

        let idle = tempdir().unwrap();
        make_scaffolded_plan(idle.path(), name);
        let (out, _) = build_output(idle.path());
        let row = out
            .specs
            .iter()
            .find(|s| s.name == name)
            .expect("the scaffolded plan must still be listed");
        let progress = row.progress.as_ref().expect("a wave plan reports progress");
        assert_eq!(
            (progress.done, progress.total),
            (0, 2),
            "fixture: nothing is closed yet"
        );
        assert!(
            !row.status.contains("em exec"),
            "a plan nobody dispatched must not read as running: {}",
            row.status
        );
        assert_eq!(
            row.status, "W1 a iniciar",
            "the column must ask the operator to START it"
        );

        // The legend is what the reader trusts to decode the column.
        let table = render_table(&out.specs, &out.branch_scan, SupportedLocale::default());
        assert!(
            table.contains("Wn a iniciar="),
            "the legend must explain the status the table now prints: {table}"
        );

        // Control: the same tree, plus one dispatch on record.
        let running = tempdir().unwrap();
        make_scaffolded_plan(running.path(), name);
        record_a_dispatch(running.path(), name);
        let (out, _) = build_output(running.path());
        let row = out
            .specs
            .iter()
            .find(|s| s.name == name)
            .expect("the dispatched plan must be listed");
        assert_eq!(
            row.status, "W1 em exec",
            "a plan with a dispatch on record still reads as running"
        );
    }

    /// **AC-11.** Every value [`derive_status`] can return has to FIT the column
    /// it is printed in, or that row pushes `Onde` and `Resumo` to the right and
    /// the table stops lining up on exactly the rows the operator most needs to
    /// read.
    ///
    /// The column was 10 wide while `closed-followup` (15) was already
    /// reachable, and wave 2 of this unit added `W{N} a iniciar` (12-13) — a
    /// status that mis-renders the table is the same class of defect this unit
    /// exists to remove, in miniature.
    ///
    /// Asserted structurally, against the pipe that OPENS `Onde`: it must sit at
    /// the same offset on the header, on the separator and on every row. So
    /// widening one of the three and forgetting the other two fails here rather
    /// than in the operator's terminal.
    #[test]
    fn the_status_column_never_shifts_the_columns_to_its_right() {
        // Every shape `derive_status` can return, widest included.
        let statuses = [
            "-",
            "TF→ab",
            "W1 em exec",
            "W1 a iniciar",
            "W10 a iniciar",
            "⚠ malformed",
            "closed-followup",
        ];
        let rows: Vec<ActiveSpec> = statuses
            .iter()
            .zip('a'..='z')
            .map(|(status, letter)| ActiveSpec {
                name: format!("2026-03-0{letter}-row"),
                stage: "Plan".to_string(),
                outcome: "Active".to_string(),
                scope: Some("full".to_string()),
                parent: None,
                parent_alias: None,
                progress: None,
                resumo: "Uma linha.".to_string(),
                letter: letter.to_string(),
                status: (*status).to_string(),
                clarify_records_nothing: false,
                location: "tree".to_string(),
                branch: None,
            })
            .collect();

        let table = render_table(&rows, &ok_scan(), SupportedLocale::default());
        let mut lines = table.lines();
        let header = lines.next().expect("the table opens with its header");
        // Char offset, not byte: `Estágio` and `⚠ malformed` are multi-byte, and
        // the padding that has to line up counts characters.
        let anchor_byte = header
            .find("| Onde")
            .expect("the header no longer carries the `Onde` column");
        let anchor = header[..anchor_byte].chars().count();

        for line in std::iter::once(lines.next().expect("separator")).chain(
            // The data rows, and nothing after them (legend lines follow).
            lines.take_while(|l| l.starts_with("| ")),
        ) {
            assert_eq!(
                line.chars().nth(anchor),
                Some('|'),
                "the `Onde` column does not start where the header puts it \
                 (offset {anchor}) — the Status column is too narrow for one of \
                 its own values:\n{table}"
            );
        }

        // And the widest status is printed WHOLE, not silently truncated.
        assert!(
            table.contains("closed-followup"),
            "the widest status must survive the column it is padded into: {table}"
        );
    }

    // -----------------------------------------------------------------------
    // Cross-branch discovery
    // -----------------------------------------------------------------------

    /// Run git in `dir`, failing the test loudly — a fixture that half-built
    /// would make the assertions below prove nothing.
    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must be on PATH for this test");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Write `body` at `<root>/<rel>`, creating parents.
    fn seed(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, body).expect("write");
    }

    /// A repo on `dev` (flow `{*: dev, dev: main}`) carrying one spec in the
    /// working tree, plus a wave-plan spec that exists ONLY on the work branch
    /// `dev_in-flight` — the shape of a spec whose work has not merged yet.
    fn branch_fixture() -> tempfile::TempDir {
        let td = tempdir().expect("tempdir");
        let root = td.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["checkout", "-b", "dev"]);
        seed(root, "mustard.json", r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#);
        // `-f`: a user-level excludes file that ignores `.claude/` must not be
        // able to hollow out this fixture.
        git(root, &["add", "-A", "-f", "."]);
        git(root, &["commit", "-m", "seed"]);

        // The in-flight spec is authored on its own work branch and stays there.
        git(root, &["checkout", "-b", "dev_in-flight"]);
        let spec = ".claude/spec/2026-01-09-in-flight";
        seed(root, &format!("{spec}/spec.md"), "# in-flight\n\n## Resumo\n\nEm voo.\n");
        seed(root, &format!("{spec}/wave-plan.md"), "# wave plan\n");
        seed(root, &format!("{spec}/meta.json"), r#"{"stage":"Execute","outcome":"Active"}"#);
        seed(root, &format!("{spec}/wave-1-a/spec.md"), "# w1\n");
        seed(root, &format!("{spec}/wave-1-a/meta.json"), r#"{"stage":"Close","outcome":"Completed"}"#);
        seed(root, &format!("{spec}/wave-2-b/spec.md"), "# w2\n");
        seed(root, &format!("{spec}/wave-2-b/meta.json"), r#"{"stage":"Execute","outcome":"Active"}"#);
        // `-f`: a user-level excludes file that ignores `.claude/` must not be
        // able to hollow out this fixture.
        git(root, &["add", "-A", "-f", "."]);
        git(root, &["commit", "-m", "spec on work branch"]);

        // Back on dev the directory is gone from the tree — the exact state in
        // which discovery used to answer "nothing in progress".
        git(root, &["checkout", "dev"]);
        assert!(!root.join(spec).exists(), "fixture: the spec must NOT be in the dev tree");

        // One ordinary spec that IS in the checkout, as the control.
        seed(
            root,
            ".claude/spec/2026-01-08-on-dev/spec.md",
            "# on dev\n\n## Resumo\n\nNa árvore.\n",
        );
        seed(
            root,
            ".claude/spec/2026-01-08-on-dev/meta.json",
            r#"{"stage":"Plan","outcome":"Active"}"#,
        );
        td
    }

    /// **AC-3.** A spec living on an unmerged work branch is listed as
    /// in-flight, naming the branch that holds it — instead of being reported
    /// as absent because the checkout does not carry its directory.
    ///
    /// The control in the same table is the working-tree spec: it must stay
    /// `location:"tree"` with no branch, so the two are never confused.
    #[test]
    fn active_specs_lists_in_flight_from_other_branches() {
        let td = branch_fixture();
        let (out, _) = build_output(td.path());

        // The scan actually ran and looked at the work branch.
        assert!(out.branch_scan.ok, "branch scan must run: {:?}", out.branch_scan);
        assert!(
            out.branch_scan.branches.iter().any(|b| b == "dev_in-flight"),
            "the work branch must be inspected: {:?}",
            out.branch_scan.branches
        );

        let in_flight = out
            .specs
            .iter()
            .find(|s| s.name == "2026-01-09-in-flight")
            .expect("a spec on an unmerged work branch must be listed, not reported absent");
        assert_eq!(in_flight.location, "branch");
        assert_eq!(
            in_flight.branch.as_deref(),
            Some("dev_in-flight"),
            "the row must name the branch that holds it"
        );
        assert_eq!(in_flight.stage, "Execute", "state read from the branch's meta.json");
        assert_eq!(in_flight.resumo, "Em voo.", "resumo read from the branch's spec.md");
        // Wave facts come from the branch's tree too — 1 of 2 done, wave 2 live.
        let progress = in_flight.progress.as_ref().expect("wave progress from the branch");
        assert_eq!((progress.done, progress.total), (1, 2));
        assert_eq!(in_flight.status, "W2 em exec");

        // The control: a spec in the checkout is marked as such.
        let on_dev = out
            .specs
            .iter()
            .find(|s| s.name == "2026-01-08-on-dev")
            .expect("the working-tree spec must still be listed");
        assert_eq!(on_dev.location, "tree");
        assert_eq!(on_dev.branch, None);

        // And the operator SEES the difference in the rendered table.
        let table = render_table(&out.specs, &out.branch_scan, SupportedLocale::default());
        assert!(
            table.contains("dev_in-flight"),
            "the table must show where the in-flight spec lives: {table}"
        );
    }

    /// The fail-open half: with no git to ask, the listing degrades to the
    /// working-tree answer AND says why — never a silent empty list that reads
    /// like a verified absence, and never a panic.
    #[test]
    fn active_specs_states_the_reason_when_the_branch_scan_cannot_run() {
        let td = tempdir().expect("tempdir");
        // A plain directory: no repository, so no branch can be enumerated.
        make_spec_with_meta(td.path(), "2026-01-07-local-only", "Plan", "Active");

        let (out, _) = build_output(td.path());
        assert!(!out.branch_scan.ok, "no repository → the scan did not run");
        assert!(
            out.branch_scan.reason.is_some(),
            "a scan that did not run must say why: {:?}",
            out.branch_scan
        );
        // The working-tree answer survives intact.
        assert!(
            out.specs.iter().any(|s| s.name == "2026-01-07-local-only" && s.location == "tree"),
            "the current-tree answer must survive: {:?}",
            out.specs.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
        // And the table admits the gap instead of implying completeness.
        let table = render_table(&out.specs, &out.branch_scan, SupportedLocale::default());
        assert!(
            table.contains("Varredura de branches indisponível"),
            "the table must state the gap: {table}"
        );
    }

    /// A spec whose unit lives ONLY on a remote — a colleague pushed it and
    /// this checkout has no local head for it. The old private sweep of
    /// `refs/heads/` could not see it at all; through the enumerator it arrives
    /// with the rest, wearing the third location value.
    #[test]
    fn active_specs_sees_a_spec_on_a_remote_only_branch() {
        let td = tempdir().expect("tempdir");
        let root = td.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["checkout", "-b", "dev"]);
        seed(root, "mustard.json", r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#);
        git(root, &["add", "-A", "-f", "."]);
        git(root, &["commit", "-m", "seed"]);

        git(root, &["checkout", "-b", "dev_pushed"]);
        let spec = ".claude/spec/2026-01-10-pushed-elsewhere";
        seed(root, &format!("{spec}/spec.md"), "# pushed\n\n## Resumo\n\nSó no remoto.\n");
        seed(root, &format!("{spec}/meta.json"), r#"{"stage":"Execute","outcome":"Active"}"#);
        git(root, &["add", "-A", "-f", "."]);
        git(root, &["commit", "-m", "spec pushed by a colleague"]);

        // Turn the unit into the shape this checkout would really be in: a
        // remote-tracking ref and NO local head.
        git(root, &["checkout", "dev"]);
        git(root, &["update-ref", "refs/remotes/origin/dev_pushed", "refs/heads/dev_pushed"]);
        git(root, &["branch", "-D", "dev_pushed"]);

        let (out, _) = build_output(root);
        assert!(out.branch_scan.ok, "the sweep ran: {:?}", out.branch_scan);
        assert!(
            out.branch_scan.branches.iter().any(|b| b == "origin/dev_pushed"),
            "the remote-only unit must be inspected: {:?}",
            out.branch_scan.branches
        );

        let row = out
            .specs
            .iter()
            .find(|s| s.name == "2026-01-10-pushed-elsewhere")
            .expect("a spec alive only on a remote must be listed, not reported absent");
        assert_eq!(row.location, "remote", "the third location value");
        assert_eq!(
            row.branch.as_deref(),
            Some("origin/dev_pushed"),
            "the row names the ref to fetch, remote included"
        );
        assert_eq!(row.resumo, "Só no remoto.", "read from the remote ref's blob");

        // And the legend explains the value the operator is now seeing.
        let table = render_table(&out.specs, &out.branch_scan, SupportedLocale::default());
        assert!(table.contains("origin/dev_pushed"), "the table shows where it lives: {table}");
        assert!(
            table.contains(translate("specs.location.remote_only", SupportedLocale::default())),
            "the legend must explain the remote-only value: {table}"
        );
    }

    /// **AC-2.** BOTH consumers ask the same enumerator, and neither of the two
    /// earlier sweeps survives: the sweep MOVED, it did not get copied.
    ///
    /// Every half is asserted, like the deletion-reach test in `branch_state`:
    /// the argv is absent in each consumer and present in the enumerator, and
    /// each consumer is seen calling it. Without the presence halves the
    /// absences could pass against a needle that never existed anywhere.
    /// The needles are assembled at runtime so writing the test does not put
    /// the spelling into a file under assertion.
    #[test]
    fn settle_and_active_specs_share_one_enumerator() {
        let listing = include_str!("active_specs.rs");
        let ritual = include_str!("../git_settle.rs");
        let enumerator = include_str!("../../shared/branch_state.rs");

        let sweep = ["for-each", "-ref"].concat();
        assert!(
            enumerator.contains(sweep.as_str()),
            "the enumerator must own the sweep, otherwise this test asserts nothing",
        );

        // The spec listing enumerated `refs/heads/` itself and was blind to a
        // unit that only a remote carries.
        assert!(
            !listing.contains(sweep.as_str()),
            "the spec listing must not enumerate refs itself — it asks the enumerator",
        );
        // The exit ritual derived the pending units from the WORKTREE table,
        // which an in-place cut never appears in — the omission the field
        // report measured as six invisible units.
        let worktree_scan = ["\"worktree\", \"", "list\""].concat();
        let pending = ritual
            .split_once("let also_mergeable")
            .and_then(|(_, rest)| rest.split_once(';'))
            .map(|(expr, _)| expr)
            .unwrap_or_default();
        assert!(!pending.is_empty(), "the pending-units field must still be readable here");
        assert!(
            !pending.contains(&worktree_scan),
            "the pending-units field must not be derived from the worktree table: {pending}",
        );

        // And both are seen asking the ONE enumerator.
        for (name, consumer) in [("the spec listing", listing), ("the exit ritual", ritual)] {
            assert!(
                consumer.contains("BranchEnumerator"),
                "{name} must consult the enumerator",
            );
        }
    }

    #[test]
    fn spec_date_prefix_extracts_date() {
        assert_eq!(spec_date_prefix("2026-05-23-my-spec"), "2026-05-23");
        assert_eq!(spec_date_prefix("no-date-here"), "0000-00-00");
    }
}
