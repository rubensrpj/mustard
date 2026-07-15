//! `grain` — typed client for the external grain tool.
//!
//! grain is the deterministic codebase miner (it replaces Mustard's old scan
//! engine entirely). Mustard never reads project source to understand a repo;
//! it shells out to the grain binary and consumes its JSON/Markdown:
//!
//! - `grain scan <root> --out <model.json>` — the durable model (run once/repo).
//! - `grain digest <model> --query "<terms>"` — the cheap per-interaction lookup
//!   a `feature` does to research the repo without reading files.
//! - `grain spec <model> --entity … [--like …] [--ops …] [--invariant …]` — the
//!   deterministic implementation-spec DRAFT (English; localized to the
//!   project's `mustard.json` language/tone only at the lapidation step).
//!
//! The boundary is a TOOL (process + JSON/MD), not a library link: no shared
//! build, no tree-sitter version coupling, grain stays standalone. This module
//! is the single owner of that boundary. Nothing here is language- or
//! framework-specific — grain is itself fully data-driven.
//!
//! Fail-open: spawning or parsing failures return [`Error`]; callers degrade
//! (e.g. treat a digest miss as "no precedent found, confirm by reading").

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::domain::vocabulary::stacks::StackDetection;
use crate::platform::error::{Error, Result};

/// Default tool name — resolved on `PATH`. A project can point at a pinned
/// binary later (e.g. via `mustard.json`); the locator is injected, never
/// hardcoded at a call site.
pub(crate) const DEFAULT_BINARY: &str = "scan";

/// A handle to the grain tool at a known location.
#[derive(Debug, Clone)]
pub struct Scan {
    binary: String,
}

impl Default for Scan {
    fn default() -> Self {
        Self { binary: DEFAULT_BINARY.to_string() }
    }
}

/// What to compile a spec for — the deterministic inputs grain pins. The AI
/// (decomposition/feature) chooses these; persisting them makes the spec
/// reproducible (same request → byte-identical draft).
#[derive(Debug, Clone, Default)]
pub struct SpecRequest {
    /// Entity/unit to create (substitutes `<Name>` in the recipe).
    pub entity: String,
    /// Existing sibling to mirror; empty = none (grain auto-picks the pattern).
    pub like: String,
    /// Operations beyond the base vertical (e.g. `["approve"]`).
    pub ops: Vec<String>,
    /// Cross-cutting invariants the unit must obey (e.g. an injected contract).
    pub invariants: Vec<String>,
}

/// The FULL capability digest — grain's `digest <model>` output with NO
/// `--query` (the searchable catalog, not a per-query slice). Mustard owns its
/// own view and only deserializes the fields it consumes: today the domain-term
/// index ([`Self::terms`]), the proactive-lexicon `enrich` input. The published
/// term list is already discriminative-rank ordered + capped by the scan tool.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Digest {
    /// Domain-term index (token + occurrence count + sample files), ordered by
    /// the scan tool's discriminative rank. Defaulted so an older scan binary
    /// (or a model that mined no vocabulary) degrades to an empty list.
    #[serde(default)]
    pub terms: Vec<DigestTerm>,
    /// Recurring structural role affixes the scan tool mined (suffix/prefix/
    /// folder/nested + the count of distinct entities each pairs with). Consumed
    /// by the proactive lexicon `enrich` to DEMOTE structural type-glue affixes
    /// so domain vocabulary survives the candidate cap. Defaulted so a model from
    /// an older scan binary (no `roles` field) degrades to an empty list.
    #[serde(default)]
    pub roles: Vec<DigestRole>,
}

/// One row of the digest's role index ([`Digest::roles`]): a recurring affix,
/// the convention it forms (`suffix` | `prefix` | `folder` | `nested`), the
/// number of distinct entities it pairs with, and the directory it concentrates
/// in. Same shape grain's `RoleD` serializes; Mustard owns its own (read-only)
/// view and only deserializes the fields it consumes.
#[derive(Debug, Clone, Deserialize)]
pub struct DigestRole {
    pub affix: String,
    /// The convention the affix forms: `suffix` | `prefix` | `folder` | `nested`.
    /// Defaulted so a partial / older payload still deserialises.
    #[serde(default)]
    pub kind: String,
    /// Distinct entities the affix pairs with — its structural recurrence.
    #[serde(default)]
    pub count: usize,
    /// The directory the affix concentrates in (module organisation hint).
    #[serde(default)]
    pub common_dir: String,
}

/// One row of the digest's domain-term index ([`Digest::terms`]): the mined
/// code token, its (machine-class-demoted) occurrence count, and a few sample
/// files where the vocabulary lives. Same shape grain's `TermD` serializes.
#[derive(Debug, Clone, Deserialize)]
pub struct DigestTerm {
    pub term: String,
    #[serde(default)]
    pub count: usize,
    /// Domain specificity ×1024 (TF·IDF, `ranking::domain_specificity_x1024`):
    /// the discriminative-power signal that peaks at mid frequency. Defaulted to
    /// 0 so a model from an older scan binary (no field) still deserialises — a
    /// consumer sorting by it then sees a flat 0 and falls back to scan's order.
    #[serde(default)]
    pub specificity_x1024: u64,
    #[serde(default)]
    pub samples: Vec<String>,
    /// One-sentence business-action summary for the declaration that anchors
    /// this term. Written by `enrich-purpose --apply`; absent on older models
    /// (serde default = None). Additive: consumers that do not use it are
    /// unaffected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

/// The focused slice of the digest matching some domain terms — grain's
/// `digest --query` output. Mirrors grain's schema; Mustard owns its own view.
#[derive(Debug, Clone, Deserialize)]
pub struct DigestQuery {
    #[serde(default)]
    pub query: Vec<String>,
    /// Stacks the scanned model carries (inferred at scan time, copied verbatim
    /// into every `digest --query` payload — hit or miss). Same contract type
    /// as [`Project::detected_stacks`]; defaulted so payloads from an older
    /// scan binary (without the field) keep deserialising.
    #[serde(default)]
    pub detected_stacks: Vec<StackDetection>,
    #[serde(default)]
    pub matched_terms: Vec<TermHit>,
    #[serde(default)]
    pub terms_omitted: usize,
    #[serde(default)]
    pub slices: Vec<SliceHit>,
    /// Slices that matched but were trimmed by the per-query cap — scan's
    /// additive mirror of `terms_omitted` (no silent loss). `0` from an older
    /// scan binary without the field.
    #[serde(default)]
    pub slices_omitted: usize,
    #[serde(default)]
    pub contracts: Vec<ContractHit>,
    #[serde(default)]
    pub hubs: Vec<Hub>,
    #[serde(default)]
    pub touchpoints: Vec<Touchpoint>,
    /// Real files to read next (anchor candidates), hubs first.
    #[serde(default)]
    pub files: Vec<String>,
    /// Audit trail for [`Self::files`], additive and same order: per anchor,
    /// the fixed-point selection score and the matched terms that carried it.
    /// Defaulted so payloads from an older scan binary (without the field)
    /// keep deserialising.
    #[serde(default)]
    pub files_detail: Vec<FileDetail>,
    /// Legacy flag: `true` when every view came back empty. Kept for payloads
    /// from older scan binaries; [`Self::report`] is the truth — a non-miss
    /// answer can still be `weak`.
    #[serde(default)]
    pub miss: bool,
    /// Honest per-term match report (scan's tier ladder): what each request
    /// term matched, at which tier, in which language, and where — plus the
    /// aggregate `matched k/n` and a reason. Defaulted so payloads from an
    /// older scan binary (without the field) keep deserialising; an empty
    /// `reason` means "old binary, fall back to `miss`".
    #[serde(default)]
    pub report: DigestReport,
    /// Concern split: when the query's concepts form ≥2 disconnected groups
    /// (no shared module, no import bridge), scan returns one [`ConcernHit`]
    /// per group, each with its OWN ranked `files`/`files_detail` restricted to
    /// that concern. Empty for a single-concern query (the flat [`Self::files`]
    /// already IS that one concern). Defaulted so payloads from an older scan
    /// binary (without the field) keep deserialising.
    #[serde(default)]
    pub concerns: Vec<ConcernHit>,
}

/// One concern of a multi-concern `digest --query` answer — a connected group
/// of the query's concepts with its own ranked anchors. Mirrors scan's
/// `ConcernD`; Mustard owns its own view. A consumer reads `files` per concern
/// instead of the blended [`DigestQuery::files`] when a request mixes concerns.
#[derive(Debug, Clone, Deserialize)]
pub struct ConcernHit {
    /// The concern's concept tokens joined with '+' (sorted asc).
    pub label: String,
    /// The query concepts in this concern (sorted asc).
    #[serde(default)]
    pub concepts: Vec<String>,
    /// Files to read for THIS concern, ranked over its concepts only.
    #[serde(default)]
    pub files: Vec<String>,
    /// Audit trail for [`Self::files`], same order (parallel to
    /// [`DigestQuery::files_detail`]).
    #[serde(default)]
    pub files_detail: Vec<FileDetail>,
    /// This concern's strength on its own evidence: `strong` (a concept hit
    /// exact/fold), `weak` (derived tiers only), `none` (no anchor surfaced).
    #[serde(default)]
    pub reason: String,
}

/// The aggregate match report of a `digest --query` answer. Reasons:
/// `none` (nothing matched — treat as net-new, confirm by reading),
/// `generated_only` (matches live only in machine-written modules —
/// regenerate, never edit them), `weak` (under half the terms matched, or
/// only stem/lexicon-derived matches — re-query in the code's vocabulary or
/// explore), `strong` (solid precedent). Empty = payload predates the report.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DigestReport {
    #[serde(default)]
    pub matched: usize,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub reason: String,
    /// `true` when a `weak` answer is weak ONLY because no term hit exact/fold,
    /// yet a CURATED lexicon bridge (seed or the project's own overlay) carried
    /// a non-thin query (`matched*2 >= total`) — the request vocabulary
    /// translated onto the code's. The consumer keeps the planning fields (with
    /// a caveat) instead of forcing a re-query; speculative `stem`-only weakness
    /// stays `false`. Defaulted `false` for payloads that predate the marker.
    #[serde(default)]
    pub bridged: bool,
    #[serde(default)]
    pub terms: Vec<TermReport>,
}

/// One request term's outcome on scan's match ladder: the tier that carried
/// it (`exact` | `fold` | `stem` | `lexicon` | `none`), the natural-language
/// evidence (stemmer language / lexicon pair label; empty for exact/fold)
/// and the top sample files where the matched vocabulary lives.
#[derive(Debug, Clone, Deserialize)]
pub struct TermReport {
    pub term: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub files: Vec<String>,
}

/// One anchor's audit row (parallel to [`DigestQuery::files`]): the fixed-point
/// BM25F relevance score (`score_x1024`, scan's integer scale — never a float,
/// so the value is byte-stable) and the matched index terms that carried the
/// file (by declaration or path/filename field).
#[derive(Debug, Clone, Deserialize)]
pub struct FileDetail {
    pub file: String,
    #[serde(default)]
    pub score_x1024: u64,
    #[serde(default)]
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TermHit {
    pub term: String,
    pub count: usize,
    #[serde(default)]
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SliceHit {
    pub label: String,
    pub recurrence: usize,
    #[serde(default)]
    pub entities: Vec<String>,
    /// Real file paths that exemplify this slice (the reference-implementation
    /// files to mirror), passed through verbatim from the scan digest's
    /// per-slice `exemplar_files`. `default` so an older scan payload without
    /// the field still deserializes (empty).
    #[serde(default)]
    pub exemplar_files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractHit {
    pub name: String,
    pub implementors: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hub {
    pub module: String,
    pub degree: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Touchpoint {
    pub module: String,
    pub fan_out: usize,
    pub breadth: usize,
}

/// One compilation unit from grain's model (`grain.model.json` `projects[]`) —
/// the subproject list. Replaces the deleted sync-detect discovery: grain mines
/// the same build-manifest set deterministically.
#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub code_files: usize,
    /// Frameworks/deps recurring across this unit's manifests (mined by `scan`,
    /// frequency-ranked, top-12). Empty when none mined / older model.
    #[serde(default)]
    pub frameworks: Vec<String>,
    /// Distinct dependencies declared by this unit's manifests (sorted, deduped).
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Build/codegen scripts declared by this unit's manifests (sorted, deduped).
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Stacks inferred for this unit (registry-driven, see
    /// `domain::vocabulary::stacks`). Additive next to [`Self::frameworks`]
    /// (which stays the raw frequency-ranked dep list); empty when the model
    /// predates the field or nothing was inferred.
    #[serde(default)]
    pub detected_stacks: Vec<StackDetection>,
}

/// The small, stable FACTS the orchestrator consumes from a grain model — the
/// subproject list and the known declaration names. Produced by `scan facts`;
/// Mustard deserializes this tiny shape but never the model's own (large)
/// schema, so the scan tool stays the single owner of the model format.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelFacts {
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub entities: Vec<String>,
}

/// Read the `projects[]` (subproject list) from a grain model — via the scan
/// tool's `facts` command ([`Scan::facts`]), so this crate never parses the
/// model's own schema. Fail-open: a missing model (no scan yet) or any
/// spawn/parse error yields an empty list.
#[must_use]
pub fn read_projects(model_path: &std::path::Path) -> Vec<Project> {
    if !model_path.is_file() {
        return Vec::new();
    }
    Scan::locate().facts(model_path).map(|f| f.projects).unwrap_or_default()
}

/// Read the distinct declaration names (entities / types / functions) from a
/// grain model — the "known entities" set — via the scan tool's `facts` command.
/// Sorted + deduped by the tool. Fail-open: empty on a missing model or any
/// spawn/parse error.
#[must_use]
pub fn read_entity_names(model_path: &std::path::Path) -> Vec<String> {
    if !model_path.is_file() {
        return Vec::new();
    }
    Scan::locate().facts(model_path).map(|f| f.entities).unwrap_or_default()
}

/// One ranked row of `scan rank` (`files[]`): the path plus the ADDITIVE
/// per-file `terms` evidence (the matched dictionary terms / direct query
/// tokens that seeded it — empty on an older scan binary or a purely
/// propagation-carried file). The fixed-point score stays in the tool's own
/// output (the fusion downstream is rank-based, never score-based).
#[derive(Debug, Clone, Deserialize)]
pub struct RankFile {
    pub file: String,
    #[serde(default)]
    pub terms: Vec<String>,
}

/// The `scan rank` output envelope (`{query, matched_terms, files}`) — only
/// `files` is projected; the rest stays with the tool.
#[derive(Debug, Clone, Default, Deserialize)]
struct RankOut {
    #[serde(default)]
    files: Vec<RankFile>,
}

/// The one-shot `feature-bundle` payload — the digest, the full domain-term
/// index (the non-strong vocabulary menu) and the personalized-PageRank pool,
/// all from ONE `scan` spawn / ONE model parse. Replaces the digest_query +
/// digest + rank + rank_detail spawn fan-out `feature` used to do (each of which
/// re-parsed the whole model).
#[derive(Debug, Clone)]
pub struct FeatureBundle {
    /// The per-query digest (the shape [`Scan::digest_query`] returns).
    pub digest: DigestQuery,
    /// The FULL domain-term index (same as [`Scan::digest`]'s `terms`) — the
    /// non-strong `vocabulary` menu.
    pub terms: Vec<DigestTerm>,
    /// The personalized-PageRank pool at the requested depth WITH per-file term
    /// evidence (the rows [`Scan::rank_detail`] returns); its top-10 file prefix
    /// is the insumos list [`Scan::rank`] returned. Empty when the dictionary is
    /// absent (the fail-open gate).
    pub rank: Vec<RankFile>,
}

/// Wire shape of the `feature-bundle` stdout (`{digest, terms, rank}`). Mustard
/// owns its own view and normalises the rank file separators on the way in — the
/// same fold [`parse_rank_detail`] applies.
#[derive(Deserialize)]
struct FeatureBundleWire {
    digest: DigestQuery,
    #[serde(default)]
    terms: Vec<DigestTerm>,
    #[serde(default)]
    rank: Vec<RankFile>,
}

/// Project a `scan rank` stdout into the ordered file list, tolerating any
/// non-JSON preamble (parse from the first `{`, the same rule the benchmark
/// harness used) and normalising separators to `/` for stable fusion keys.
///
/// # Errors
/// [`Error::Parse`] when no JSON object is present or it is not the rank shape.
fn parse_rank_files(out: &str) -> Result<Vec<String>> {
    Ok(parse_rank_detail(out)?.into_iter().map(|f| f.file).collect())
}

/// Project a `scan rank` stdout into the ordered rows WITH the per-file
/// `terms` evidence (additive in the tool output; missing → empty, so an
/// older scan binary degrades to term-less rows). Same preamble tolerance and
/// separator normalisation as [`parse_rank_files`].
///
/// # Errors
/// [`Error::Parse`] when no JSON object is present or it is not the rank shape.
fn parse_rank_detail(out: &str) -> Result<Vec<RankFile>> {
    let json = out.find('{').map_or(out, |i| &out[i..]);
    let parsed: RankOut = serde_json::from_str(json)?;
    Ok(parsed
        .files
        .into_iter()
        .map(|mut f| {
            f.file = f.file.replace('\\', "/");
            f
        })
        .collect())
}

impl Scan {
    /// A client for the grain binary at `binary` (a name on `PATH` or a path).
    #[must_use]
    pub fn new(binary: impl Into<String>) -> Self {
        Self { binary: binary.into() }
    }

    /// Locate the bundled grain binary — built as a sibling of the running
    /// executable in the same workspace `target/` dir — falling back to
    /// [`DEFAULT_BINARY`] on `PATH`. Fail-open: any probe error → the fallback.
    #[must_use]
    pub fn locate() -> Self {
        let sibling = std::env::current_exe().ok().and_then(|exe| {
            let dir = exe.parent()?;
            let cand = dir.join(if cfg!(windows) { "scan.exe" } else { "scan" });
            cand.is_file().then(|| cand.to_string_lossy().into_owned())
        });
        Self { binary: sibling.unwrap_or_else(|| DEFAULT_BINARY.to_string()) }
    }

    /// Mine `root` into the model file at `out` (`grain scan`).
    ///
    /// # Errors
    /// [`Error::Io`] if the tool cannot be spawned, [`Error::CheckFailed`] on a
    /// non-zero exit.
    pub fn scan(&self, root: &Path, out: &Path) -> Result<()> {
        self.run(&scan_args(root, out)).map(|_| ())
    }

    /// Read the model's FULL capability digest (`grain digest <model>`, no
    /// `--query`) — the whole catalog, including the discriminative-ranked
    /// domain-term index. Used by the proactive `enrich` flow to learn the
    /// code's vocabulary; the per-query [`Self::digest_query`] is the cheap
    /// research lookup instead.
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure,
    /// [`Error::Parse`] if the output is not the expected JSON.
    pub fn digest(&self, model: &Path) -> Result<Digest> {
        let out = self.run(&digest_args(model))?;
        Ok(serde_json::from_str(&out)?)
    }

    /// Look up the model's digest by domain term(s) (`grain digest --query`).
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure,
    /// [`Error::Parse`] if the output is not the expected JSON.
    pub fn digest_query(&self, model: &Path, terms: &[String]) -> Result<DigestQuery> {
        let out = self.run(&digest_query_args(model, terms))?;
        Ok(serde_json::from_str(&out)?)
    }

    /// Read the model's FACTS (subproject list + known declaration names) via
    /// `scan facts <model>` — so Mustard never parses the model's own schema.
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure,
    /// [`Error::Parse`] if the output is not the expected JSON.
    pub fn facts(&self, model: &Path) -> Result<ModelFacts> {
        let out = self.run(&facts_args(model))?;
        Ok(serde_json::from_str(&out)?)
    }

    /// Rank the model's files for a raw (e.g. PT) request via the tool's
    /// personalized PageRank (`scan rank`), seeded by the
    /// `grain.dictionary.json` sidecar. Returns the ordered file list
    /// (separators normalised to `/`); every tuning knob stays on the tool's
    /// defaults except the explicit `top` / `direct_base` contract.
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure,
    /// [`Error::Parse`] if the output is not the expected JSON.
    pub fn rank(&self, model: &Path, dict: &Path, query: &str, top: usize, direct_base: u64) -> Result<Vec<String>> {
        let out = self.run(&rank_args(model, dict, query, top, direct_base))?;
        parse_rank_files(&out)
    }

    /// Like [`Self::rank`], but returning each ranked row WITH its per-file
    /// matched-term evidence (`files[].terms`, additive in the tool output) —
    /// the audit line the retrieval hop shows per candidate. An older scan
    /// binary (no `terms` field) yields empty term lists (fail-open).
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure,
    /// [`Error::Parse`] if the output is not the expected JSON.
    pub fn rank_detail(&self, model: &Path, dict: &Path, query: &str, top: usize, direct_base: u64) -> Result<Vec<RankFile>> {
        let out = self.run(&rank_args(model, dict, query, top, direct_base))?;
        parse_rank_detail(&out)
    }

    /// Fetch the one-shot [`FeatureBundle`] (`scan feature-bundle`): the digest,
    /// the full term index and the rank pool from ONE spawn / ONE model parse —
    /// the collapse of the digest_query + digest + rank + rank_detail fan-out.
    /// `query_terms` are the digest terms, `rank_query` the expanded rank query,
    /// `dict` the dictionary sidecar (the rank pool is empty when it is absent).
    /// `top` is the pool depth (the insumos list is its top-10 prefix). Rank file
    /// separators are normalised to `/` on the way in.
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure,
    /// [`Error::Parse`] if the output is not the expected JSON.
    pub fn feature_bundle(&self, model: &Path, dict: &Path, query_terms: &[String], rank_query: &str, top: usize, direct_base: u64) -> Result<FeatureBundle> {
        let out = self.run(&feature_bundle_args(model, dict, query_terms, rank_query, top, direct_base))?;
        let json = out.find('{').map_or(out.as_str(), |i| &out[i..]);
        let mut wire: FeatureBundleWire = serde_json::from_str(json)?;
        for r in &mut wire.rank {
            r.file = r.file.replace('\\', "/");
        }
        Ok(FeatureBundle { digest: wire.digest, terms: wire.terms, rank: wire.rank })
    }

    /// Compile the deterministic spec draft for `req` (`grain spec`). Returns the
    /// Markdown verbatim (English — the lapidation step localizes per mustard.json).
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure.
    pub fn spec(&self, model: &Path, req: &SpecRequest) -> Result<String> {
        self.run(&spec_args(model, req))
    }

    /// Run grain with `args`, returning stdout. Maps a non-zero exit (with
    /// stderr) to [`Error::CheckFailed`].
    fn run(&self, args: &[String]) -> Result<String> {
        let output = Command::new(&self.binary).args(args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::check_failed(format!("scan {}: {}", args.join(" "), stderr.trim())));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

// --- pure arg builders (unit-testable without the binary present) -----------

fn scan_args(root: &Path, out: &Path) -> Vec<String> {
    vec![
        "scan".to_string(),
        root.to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
    ]
}

fn digest_args(model: &Path) -> Vec<String> {
    vec!["digest".to_string(), model.to_string_lossy().into_owned()]
}

fn digest_query_args(model: &Path, terms: &[String]) -> Vec<String> {
    vec![
        "digest".to_string(),
        model.to_string_lossy().into_owned(),
        "--query".to_string(),
        terms.join(","),
    ]
}

fn facts_args(model: &Path) -> Vec<String> {
    vec!["facts".to_string(), model.to_string_lossy().into_owned()]
}

fn rank_args(model: &Path, dict: &Path, query: &str, top: usize, direct_base: u64) -> Vec<String> {
    vec![
        "rank".to_string(),
        model.to_string_lossy().into_owned(),
        "--dict".to_string(),
        dict.to_string_lossy().into_owned(),
        "--query".to_string(),
        query.to_string(),
        "--top".to_string(),
        top.to_string(),
        "--direct-base".to_string(),
        direct_base.to_string(),
    ]
}

fn feature_bundle_args(model: &Path, dict: &Path, query_terms: &[String], rank_query: &str, top: usize, direct_base: u64) -> Vec<String> {
    vec![
        "feature-bundle".to_string(),
        model.to_string_lossy().into_owned(),
        "--dict".to_string(),
        dict.to_string_lossy().into_owned(),
        "--query".to_string(),
        query_terms.join(","),
        "--rank-query".to_string(),
        rank_query.to_string(),
        "--top".to_string(),
        top.to_string(),
        "--direct-base".to_string(),
        direct_base.to_string(),
    ]
}

fn spec_args(model: &Path, req: &SpecRequest) -> Vec<String> {
    let ops = if req.ops.is_empty() { "create".to_string() } else { req.ops.join(",") };
    let mut args = vec![
        "spec".to_string(),
        model.to_string_lossy().into_owned(),
        "--entity".to_string(),
        req.entity.clone(),
        "--ops".to_string(),
        ops,
    ];
    if !req.like.is_empty() {
        args.push("--like".to_string());
        args.push(req.like.clone());
    }
    if !req.invariants.is_empty() {
        args.push("--invariant".to_string());
        args.push(req.invariants.join(","));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn scan_args_shape() {
        let a = scan_args(&PathBuf::from("repo"), &PathBuf::from("m.json"));
        assert_eq!(a, vec!["scan", "repo", "--out", "m.json"]);
    }

    #[test]
    fn digest_query_joins_terms() {
        let a = digest_query_args(&PathBuf::from("m.json"), &["tenant".into(), "charge".into()]);
        assert_eq!(a, vec!["digest", "m.json", "--query", "tenant,charge"]);
    }

    #[test]
    fn facts_args_shape() {
        let a = facts_args(&PathBuf::from("m.json"));
        assert_eq!(a, vec!["facts", "m.json"]);
    }

    #[test]
    fn rank_args_shape() {
        let a = rank_args(
            &PathBuf::from("m.json"),
            &PathBuf::from("d.json"),
            "onde é feita a conciliação reconciliation",
            10,
            100_000,
        );
        assert_eq!(
            a,
            vec![
                "rank", "m.json", "--dict", "d.json", "--query",
                "onde é feita a conciliação reconciliation", "--top", "10",
                "--direct-base", "100000",
            ]
        );
    }

    #[test]
    fn feature_bundle_args_shape() {
        let a = feature_bundle_args(
            &PathBuf::from("m.json"),
            &PathBuf::from("d.json"),
            &["spec".into(), "pipeline".into()],
            "spec pipeline reconciliation",
            25,
            100_000,
        );
        assert_eq!(
            a,
            vec![
                "feature-bundle", "m.json", "--dict", "d.json", "--query", "spec,pipeline",
                "--rank-query", "spec pipeline reconciliation", "--top", "25",
                "--direct-base", "100000",
            ]
        );
    }

    #[test]
    fn feature_bundle_wire_deserializes_all_three_parts() {
        // The bundle collapses digest_query + digest + rank_detail into one
        // `{digest, terms, rank}` payload. The wire view deserializes each part
        // into Mustard's own mirror; the rank rows keep {file, terms} and ignore
        // the tool score field. Separator normalisation happens in the method.
        let json = r#"{
            "digest":{"query":["spec"],"files":["src/a.rs"],"miss":false,
                "report":{"matched":1,"total":1,"reason":"strong","terms":[]}},
            "terms":[{"term":"spec","count":9,"specificity_x1024":100,"samples":["src/a.rs"]}],
            "rank":[{"file":"src/a.rs","score_x1024":42,"terms":["spec"]},
                    {"file":"src/b.rs","score_x1024":7}]
        }"#;
        let wire: FeatureBundleWire = serde_json::from_str(json).expect("bundle json");
        assert_eq!(wire.digest.report.reason, "strong");
        assert_eq!(wire.digest.files, vec!["src/a.rs"]);
        assert_eq!(wire.terms.len(), 1);
        assert_eq!(wire.terms[0].term, "spec");
        assert_eq!(wire.terms[0].count, 9);
        assert_eq!(wire.rank.len(), 2);
        assert_eq!(wire.rank[0].file, "src/a.rs");
        assert_eq!(wire.rank[0].terms, vec!["spec"]);
        assert!(wire.rank[1].terms.is_empty(), "older/absent terms default empty");
    }
    #[test]
    fn parse_rank_files_tolerates_preamble_and_normalises_separators() {
        // The benchmark-harness rule: parse from the first `{` (a tool warning
        // line before the JSON must not break the client) and fold `\` → `/`.
        let out = "note: something\n{\"query\":[\"x\"],\"matched_terms\":[],\"files\":[{\"file\":\"src\\\\a.cs\",\"score_x1024\":42},{\"file\":\"src/b.cs\",\"score_x1024\":7}]}";
        let files = parse_rank_files(out).expect("rank output parses");
        assert_eq!(files, vec!["src/a.cs", "src/b.cs"], "order preserved, separators normalised");

        // The detailed projection reads the ADDITIVE per-file `terms` when the
        // tool emits them, and degrades to empty lists when it does not (an
        // older scan binary) — same rows, same order, same normalisation.
        let detail = parse_rank_detail(out).expect("term-less rank output parses");
        assert_eq!(detail.len(), 2);
        assert_eq!(detail[0].file, "src/a.cs");
        assert!(detail[0].terms.is_empty(), "older binary → empty terms");
        let with_terms = "{\"files\":[{\"file\":\"src\\\\a.cs\",\"score_x1024\":42,\"terms\":[\"aging\",\"payable\"]}]}";
        let detail = parse_rank_detail(with_terms).expect("terms parse");
        assert_eq!(detail[0].terms, vec!["aging", "payable"]);

        // An empty ranked list is a valid answer (nothing bridged), not an error.
        let none = parse_rank_files(r#"{"query":[],"files":[]}"#).expect("empty rank parses");
        assert!(none.is_empty());

        // No JSON at all → a parse error the caller degrades on (fail-open there).
        assert!(parse_rank_files("garbage with no json").is_err());
    }

    #[test]
    fn model_facts_deserializes_scan_output() {
        let json = r#"{"projects":[{"name":"api","dir":"apps/api","kind":"node","code_files":3}],"entities":["Invoice","User"]}"#;
        let f: ModelFacts = serde_json::from_str(json).expect("valid scan facts json");
        assert_eq!(f.projects.len(), 1);
        assert_eq!(f.projects[0].name, "api");
        assert_eq!(f.entities, vec!["Invoice", "User"]);
    }

    #[test]
    fn detected_stacks_serde_compat() {
        // An old payload without `detected_stacks` still deserialises, and
        // `frameworks` is untouched by the new field.
        let old = r#"{"name":"api","dir":"apps/api","kind":"node","code_files":3,"frameworks":["express"]}"#;
        let p: Project = serde_json::from_str(old).expect("old payload without detected_stacks");
        assert!(p.detected_stacks.is_empty());
        assert_eq!(p.frameworks, vec!["express"]);

        // A new payload carrying the field round-trips into the contract type.
        let new = r#"{"name":"web","frameworks":["laravel/framework"],"detected_stacks":[{"name":"laravel","confidence":0.9,"signals":["dep:laravel/framework"]}]}"#;
        let p: Project = serde_json::from_str(new).expect("payload with detected_stacks");
        assert_eq!(p.detected_stacks.len(), 1);
        assert_eq!(p.detected_stacks[0].name, "laravel");
        assert_eq!(p.detected_stacks[0].signals, vec!["dep:laravel/framework"]);
        assert_eq!(p.frameworks, vec!["laravel/framework"]);
    }

    #[test]
    fn model_facts_defaults_missing_fields() {
        let f: ModelFacts = serde_json::from_str("{}").expect("empty object ok");
        assert!(f.projects.is_empty());
        assert!(f.entities.is_empty());
    }

    #[test]
    fn spec_args_omit_empty_like_and_invariants() {
        let req = SpecRequest { entity: "Order".into(), ..Default::default() };
        let a = spec_args(&PathBuf::from("m.json"), &req);
        assert_eq!(a, vec!["spec", "m.json", "--entity", "Order", "--ops", "create"]);
    }

    #[test]
    fn spec_args_include_like_invariant_and_ops() {
        let req = SpecRequest {
            entity: "RefundCharge".into(),
            like: "CancelCharge".into(),
            ops: vec!["create".into(), "approve".into()],
            invariants: vec!["ICurrentTenant".into()],
        };
        let a = spec_args(&PathBuf::from("m.json"), &req);
        assert_eq!(
            a,
            vec![
                "spec", "m.json", "--entity", "RefundCharge", "--ops", "create,approve",
                "--like", "CancelCharge", "--invariant", "ICurrentTenant",
            ]
        );
    }

    #[test]
    fn digest_query_detected_stacks_serde_compat() {
        // An old payload without `detected_stacks` still deserialises (default).
        let old = r#"{"query":["tenant"],"matched_terms":[],"terms_omitted":0,"miss":true}"#;
        let q: DigestQuery = serde_json::from_str(old).expect("old payload without detected_stacks");
        assert!(q.detected_stacks.is_empty());
        assert!(q.miss);

        // A new payload carrying the field round-trips into the contract type.
        let new = r#"{"query":["page"],"detected_stacks":[{"name":"nextjs","confidence":0.65,"signals":["dep:next","path:next.config.js"]}],"files":["pages/index.tsx"],"miss":false}"#;
        let q: DigestQuery = serde_json::from_str(new).expect("payload with detected_stacks");
        assert_eq!(q.detected_stacks.len(), 1);
        assert_eq!(q.detected_stacks[0].name, "nextjs");
        assert_eq!(q.detected_stacks[0].signals, vec!["dep:next", "path:next.config.js"]);
        assert_eq!(q.files, vec!["pages/index.tsx"]);
    }

    #[test]
    fn digest_query_deserializes_grain_output() {
        // The REAL shape the scan binary emits since the tier-ladder redesign:
        // `report` with per-term {term, tier, lang, files} + matched k/n +
        // reason, alongside the legacy `miss` flag.
        let json = r#"{"query":["tenant","cancelado"],"matched_terms":[{"term":"tenant","count":242,"samples":["a.cs"]}],"terms_omitted":0,"slices":[],"contracts":[],"hubs":[{"module":"ICurrentTenant.cs","degree":738}],"touchpoints":[],"files":["ICurrentTenant.cs"],"miss":false,"report":{"matched":2,"total":2,"reason":"strong","terms":[{"term":"tenant","tier":"exact","lang":"","files":["a.cs"]},{"term":"cancelado","tier":"lexicon","lang":"pt-en","files":["b.cs"]}]}}"#;
        let q: DigestQuery = serde_json::from_str(json).expect("valid grain digest json");
        assert_eq!(q.matched_terms.len(), 1);
        assert_eq!(q.matched_terms[0].count, 242);
        assert_eq!(q.hubs[0].module, "ICurrentTenant.cs");
        assert!(!q.miss);
        assert_eq!(q.report.matched, 2);
        assert_eq!(q.report.total, 2);
        assert_eq!(q.report.reason, "strong");
        assert_eq!(q.report.terms.len(), 2);
        assert_eq!(q.report.terms[0].tier, "exact");
        assert_eq!(q.report.terms[1].tier, "lexicon");
        assert_eq!(q.report.terms[1].lang, "pt-en");
        assert_eq!(q.report.terms[1].files, vec!["b.cs"]);
    }

    #[test]
    fn digest_query_report_serde_compat_with_old_payloads() {
        // A payload from an OLDER scan binary (no `report`) keeps
        // deserialising; the defaulted report's empty reason is the caller's
        // "fall back to `miss`" signal.
        let old = r#"{"query":["tenant"],"matched_terms":[],"terms_omitted":0,"miss":true}"#;
        let q: DigestQuery = serde_json::from_str(old).expect("old payload without report");
        assert!(q.miss);
        assert_eq!(q.report.reason, "");
        assert_eq!(q.report.total, 0);
        assert!(q.report.terms.is_empty());
        assert!(!q.report.bridged, "the bridged marker defaults false for payloads that predate it");
    }

    #[test]
    fn digest_query_deserializes_bridged_marker() {
        // The scan binary flags a `weak` answer a CURATED lexicon bridge carried
        // (no exact/fold hit, non-thin) with `report.bridged: true`. The consumer
        // (feature) reads it to keep the planning fields instead of withholding.
        let json = r#"{"query":["cancelado"],"matched_terms":[{"term":"cancel","count":3,"samples":["b.cs"]}],"files":["b.cs"],"miss":false,"report":{"matched":1,"total":1,"reason":"weak","bridged":true,"terms":[{"term":"cancelado","tier":"lexicon","lang":"pt-en","files":["b.cs"]}]}}"#;
        let q: DigestQuery = serde_json::from_str(json).expect("valid bridged digest json");
        assert_eq!(q.report.reason, "weak");
        assert!(q.report.bridged, "the curated-bridge marker round-trips from the scan binary's JSON");
    }

    #[test]
    fn digest_query_concerns_serde_compat() {
        // An OLD payload without `concerns` keeps deserialising — empty.
        let old = r#"{"query":["tenant"],"files":["a.cs"],"miss":false}"#;
        let q: DigestQuery = serde_json::from_str(old).expect("old payload without concerns");
        assert!(q.concerns.is_empty(), "single-concern / old binary → no split");

        // A multi-concern payload round-trips: each concern carries its own
        // label, concepts and ranked files restricted to that concern.
        let new = r#"{"query":["tenant","export"],"files":["t.cs","e.cs"],"miss":false,"concerns":[{"label":"tenant","concepts":["tenant"],"files":["t.cs"],"files_detail":[{"file":"t.cs","score_x1024":2048,"terms":["tenant"]}],"reason":"strong"},{"label":"export","concepts":["export"],"files":["e.cs"],"files_detail":[{"file":"e.cs","score_x1024":1024,"terms":["export"]}],"reason":"weak"}]}"#;
        let q: DigestQuery = serde_json::from_str(new).expect("payload with concerns");
        assert_eq!(q.concerns.len(), 2);
        assert_eq!(q.concerns[0].label, "tenant");
        assert_eq!(q.concerns[0].concepts, vec!["tenant"]);
        assert_eq!(q.concerns[0].files, vec!["t.cs"]);
        assert_eq!(q.concerns[0].files_detail[0].score_x1024, 2048);
        assert_eq!(q.concerns[0].reason, "strong");
        assert_eq!(q.concerns[1].label, "export");
        assert_eq!(q.concerns[1].reason, "weak");
    }

    #[test]
    fn digest_roles_serde_compat() {
        // An OLD payload (scan binary predating the roles index in the FULL
        // digest) without `roles` keeps deserialising — empty, never an error.
        let old = r#"{"terms":[{"term":"payable","count":12}]}"#;
        let d: Digest = serde_json::from_str(old).expect("old payload without roles");
        assert!(d.roles.is_empty(), "old binary / no roles → empty list");
        assert_eq!(d.terms.len(), 1);

        // A NEW payload carrying the roles index round-trips: each role keeps its
        // affix, kind and structural-recurrence count.
        let new = r#"{"terms":[{"term":"payable","count":12}],"roles":[
            {"affix":"Handler","kind":"suffix","count":24,"common_dir":"src/handlers"},
            {"affix":"Repository","kind":"suffix","count":9,"common_dir":""}]}"#;
        let d: Digest = serde_json::from_str(new).expect("payload with roles");
        assert_eq!(d.roles.len(), 2);
        assert_eq!(d.roles[0].affix, "Handler");
        assert_eq!(d.roles[0].kind, "suffix");
        assert_eq!(d.roles[0].count, 24);
        assert_eq!(d.roles[0].common_dir, "src/handlers");
        assert_eq!(d.roles[1].affix, "Repository");
        assert_eq!(d.roles[1].count, 9);
    }

    #[test]
    fn digest_query_files_detail_and_slices_omitted_serde_compat() {
        // An OLD payload (scan binary predating lote 1) without
        // `files_detail`/`slices_omitted` keeps deserialising — both default.
        let old = r#"{"query":["payable"],"files":["src/a.rs"],"miss":false}"#;
        let q: DigestQuery = serde_json::from_str(old).expect("old payload");
        assert!(q.files_detail.is_empty());
        assert_eq!(q.slices_omitted, 0);

        // The NEW payload shape (per-anchor audit + capped-slices count)
        // round-trips into the contract type, parallel to `files`.
        let new = r#"{"query":["payable"],"slices":[{"label":"List","recurrence":3}],"slices_omitted":2,"files":["src/a.rs","src/b.rs"],"files_detail":[{"file":"src/a.rs","score_x1024":2048,"terms":["payable","nature"]},{"file":"src/b.rs","score_x1024":0,"terms":[]}],"miss":false,"report":{"matched":2,"total":2,"reason":"strong","terms":[]}}"#;
        let q: DigestQuery = serde_json::from_str(new).expect("payload with files_detail");
        assert_eq!(q.slices_omitted, 2);
        assert_eq!(q.files_detail.len(), 2);
        assert_eq!(q.files_detail[0].file, "src/a.rs");
        assert_eq!(q.files_detail[0].score_x1024, 2048);
        assert_eq!(q.files_detail[0].terms, vec!["payable", "nature"]);
        // Touchpoint-tail anchor: honest score 0, no terms.
        assert_eq!(q.files_detail[1].score_x1024, 0);
        assert!(q.files_detail[1].terms.is_empty());
    }
}
