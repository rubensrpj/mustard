//! The intermediate "project model".
//!
//! Produced by the deterministic analysis stages and consumed by synthesis +
//! generation. Nothing here encodes any framework: conventions are whatever
//! *recurs* in the repo, named by the repo's own vocabulary offline and
//! (optionally) given a semantic name by the LLM stage.

use mustard_core::domain::vocabulary::stacks::StackDetection;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ProjectModel {
    pub root: String,
    pub languages: Vec<LanguageStat>,
    pub manifests: Vec<Manifest>,
    pub frameworks: Vec<String>,
    pub skeleton: Vec<SkeletonEntry>,
    pub modules: Vec<Module>,
    pub graph: GraphStats,
    /// Role affixes that recur across many names (e.g. "Repository", "use").
    pub roles: Vec<RoleStat>,
    /// Recurring vertical slices + single-role conventions, ranked.
    pub conventions: Vec<Convention>,
    /// What the scan visited vs skipped — verifiable answer to "did you read it all?".
    #[serde(default)]
    pub coverage: Coverage,
    /// Projects/compilation units in the workspace (a slice usually spans several).
    #[serde(default)]
    pub projects: Vec<ProjectUnit>,
    /// Base types/interfaces many entities build on — the shared foundation.
    #[serde(default)]
    pub shared_contracts: Vec<SharedContract>,
    /// Stacks inferred by evidence convergence (manifest deps + path markers +
    /// code signatures). The engine and the registry live in `mustard-core` —
    /// stacks are DATA there, never names in this crate. Additive: older
    /// models without the field keep deserialising.
    #[serde(default)]
    pub detected_stacks: Vec<StackDetection>,
}

/// What the scan actually visited — so "did you read everything?" is verifiable.
/// One compilation unit / project in the workspace (one per build manifest)
/// and how many source files live under it. A single entity slice
/// typically spans several of these.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ProjectUnit {
    pub name: String,
    pub dir: String,
    pub kind: String,
    pub code_files: usize,
    /// Frameworks/deps that recur across this unit's own manifests — the same
    /// frequency-ranked projection [`crate::ingest`] applies repo-wide, restricted
    /// to the manifests under `dir`. No catalog; agnostic to language/framework.
    #[serde(default)]
    pub frameworks: Vec<String>,
    /// Distinct dependencies declared by this unit's manifests — aggregated,
    /// deduped, sorted (deterministic output).
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Build/codegen scripts declared by this unit's manifests, verbatim —
    /// aggregated, deduped, sorted.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Stacks inferred for this unit (same engine/contract as
    /// [`ProjectModel::detected_stacks`]). Additive — defaults to empty so
    /// older payloads keep deserialising; population is per-unit evidence,
    /// owned by the consumer-side projection.
    #[serde(default)]
    pub detected_stacks: Vec<StackDetection>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Coverage {
    pub top_dirs: Vec<DirCoverage>,
    /// Build/dependency dirs skipped on purpose (from manifests.toml skip_dirs).
    pub skipped_build_dirs: Vec<String>,
    /// Extensions seen but not mined (not a supported source language).
    pub unsupported_exts: Vec<ExtCount>,
    pub code_files_read: usize,
    pub non_utf8_skipped: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DirCoverage {
    pub dir: String,
    pub code_files: usize,
    pub other_files: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ExtCount {
    pub ext: String,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LanguageStat {
    pub language: String,
    pub files: usize,
    pub loc: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Manifest {
    pub path: String,
    pub kind: String,
    pub dependencies: Vec<String>,
    /// Build/codegen scripts declared by the manifest, verbatim ("name: cmd").
    /// Surfaced as-is (no catalog) so a `generate`/codegen step is identified
    /// from the repo's own scripts, not from a hardcoded list.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Project name derived per the manifest's rule (stem or parent dir).
    #[serde(default)]
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SkeletonEntry {
    pub dir: String,
    pub role: String,
    pub files: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Module {
    pub path: String,
    pub language: String,
    pub loc: usize,
    pub imports: Vec<String>,
    pub namespaces: Vec<String>,
    pub declarations: Vec<Decl>,
    /// Machine-written class, when one applies: "generated" | "vendored" |
    /// "lockfile" | "minified" (empty = hand-written). Decided by the generic
    /// engine in `classify` from catalog DATA (generated-markers.toml) plus
    /// the repo's own overrides (.gitattributes / .editorconfig). Additive:
    /// older models keep deserialising; hand-written modules don't serialise it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_class: String,
    /// Which marker decided `file_class` (catalog literal/regex/glob or the
    /// override attribute) — provenance, so a classification is explainable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub marker: String,
    /// Incoming dependency edges (fan-in) from the resolved import graph —
    /// persisted on the module so projections (digest anchor ranking) read it
    /// without recomputing the graph. Additive: older models default to 0;
    /// leaf modules don't serialise it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fan_in: usize,
}

/// serde helper for additive numeric fields (mirrors `String::is_empty` above).
fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Decl {
    pub kind: String,
    pub name: String,
    pub line: usize,
    /// Names this declaration builds on — base classes, implemented interfaces,
    /// embedded structs, implemented traits. Language-specific to capture,
    /// generic to mine: a base name shared by many entities is a shared contract.
    #[serde(default)]
    pub supertypes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GraphStats {
    pub nodes: usize,
    pub edges: usize,
    pub cyclic: bool,
    pub top_fan_in: Vec<NodeDegree>,
    pub top_fan_out: Vec<NodeDegree>,
    pub layers: Vec<LayerInfo>,
    /// High fan-out hubs that import across many directories — the registration
    /// points (DI container, menu, barrels) you EDIT when adding an entity, not
    /// the per-entity files you create. Frequency-derived; tests excluded.
    #[serde(default)]
    pub touchpoints: Vec<Touchpoint>,
}

/// A registration hub: a file that wires many modules together, so adding a new
/// entity usually means editing it (register a service, add a menu route, …).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Touchpoint {
    pub module: String,
    /// How many internal modules it imports.
    pub fan_out: usize,
    /// How many distinct directories those imports span (breadth = "central").
    pub breadth: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NodeDegree {
    pub module: String,
    pub degree: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LayerInfo {
    pub name: String,
    pub modules: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CodeExample {
    pub path: String,
    pub start_line: usize,
    pub snippet: String,
    /// The role this exemplar plays in its slice. Empty for bare entities.
    #[serde(default)]
    pub role: String,
}

/// A role affix discovered by frequency (no hardcoded list).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RoleStat {
    pub affix: String,
    /// "suffix" or "prefix"
    pub kind: String,
    pub count: usize,
    /// Most common folder these live in (relative), for the role->folder map.
    pub common_dir: String,
    /// EVERY recurring folder of the role (abstracted, ≥2 members each, count
    /// desc then name) — a convention spread across several parents
    /// (`configs/` AND `(dashboard)/<name>s`) keeps all its homes; `common_dir`
    /// alone loses everything outside the single most frequent one. Additive:
    /// absent in older models.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirs: Vec<String>,
    /// A representative declaration kind (class/function/const/...).
    pub decl_kind: String,
    /// The base type these files most often extend/implement, if any (from
    /// supertypes; populated when AST parsing is available). The role's contract.
    #[serde(default)]
    pub implements: Option<String>,
    /// Namespaces/modules files of this role commonly pull in — its collaborators.
    #[serde(default)]
    pub collaborators: Vec<String>,
}

/// A base type / interface that many distinct entities build on — the shared
/// foundation a slice plugs into (e.g. EntityBase, RepositoryBase). Mined by
/// frequency over supertypes; never from a catalog.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SharedContract {
    pub name: String,
    pub implementors: usize,
}

/// A concrete reference implementation at a complexity tier.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Exemplar {
    /// "simples" | "média" | "complexa"
    pub level: String,
    pub entity: String,
    pub roles_present: Vec<String>,
    pub files: Vec<String>,
}

/// A recurring convention mined from the repo. Either a multi-role *slice*
/// (a vertical recipe) or a single-role convention.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Convention {
    /// Repo-vocabulary name offline; may be replaced by a semantic name in synthesis.
    pub name: String,
    /// The core role affixes that define this convention (always present).
    pub roles: Vec<String>,
    /// Roles that recur but are not universal — added "when needed".
    #[serde(default)]
    pub optional_roles: Vec<String>,
    /// How many distinct entities exhibit this shape (the recurrence count).
    pub recurrence: usize,
    /// Example entities that share the shape (e.g. ["Order", "Product"]).
    pub entities: Vec<String>,
    pub confidence: f32,
    /// True when this is a multi-role vertical slice (renders as a recipe).
    pub is_slice: bool,
    /// Ordered build steps for the recipe (abstracted with <Name>).
    pub steps: Vec<String>,
    /// Simple/medium/complex reference implementations.
    #[serde(default)]
    pub exemplars: Vec<Exemplar>,
    /// One snippet per role from the complex exemplar, abstracted with <Name>.
    pub examples: Vec<CodeExample>,
    /// The concrete exemplar entity the snippets were taken from (the complex one).
    pub exemplar: String,
    pub summary: String,
}
