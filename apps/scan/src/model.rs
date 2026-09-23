//! The intermediate "project model".
//!
//! Produzido pelas etapas determinísticas da análise e consumido pelas
//! projeções. Nada aqui codifica framework nenhum. Os grupos por sufixo do nome
//! (`roles` e `conventions`) saíram do modelo: nenhum leitor sobrou, e era por
//! eles que as skills fracas nasciam.

use mustard_core::domain::project_map::History;
use mustard_core::domain::vocabulary::stacks::StackDetection;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
    /// Where this pass read from — the commit and the files not committed.
    #[serde(default)]
    pub state: ScanState,
    /// The git history, one entry per commit (created and changed files).
    #[serde(default, skip_serializing_if = "History::is_empty")]
    pub history: History,
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
    /// The module path the manifest declares for import resolution, when it
    /// declares one — kept so a pass that does not re-read the manifest still
    /// resolves imports the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// The package's own name, as the manifest declares it — kept so imports
    /// that name another package of the same project resolve inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
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
    /// The project files this one imports, resolved through the graph — the
    /// reverse of "who imports this file". Only specific imports count: an
    /// import spread over a bucket of more than eight files is left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
    /// The test files that cover this one: a test that imports it, or a test
    /// that keeps changing together with it in git.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
    /// The file carries its own tests (an inline test marker).
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_tests: bool,
    /// The stack code signatures found in this file's content, kept so a pass
    /// that does not read the file again still infers the same stacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<String>,
    /// Every call site read out of this file: the name called and the line it
    /// is called on, in document order. Raw on purpose — a name is not
    /// resolved to a declaration here, so a file that did not change still
    /// feeds the declaration links of a pass that only read what changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<CallSite>,
}

/// One call read out of a file: the name called and the line of the call. The
/// caller is the file it was read from, and the declaration that encloses the
/// line — resolved by [`crate::graph::link_declarations`], not stored twice.
///
/// Written as one string, `name:line`: there are tens of thousands of these,
/// and the model is written indented, so an object of two fields would cost
/// five lines each. The map is read by machine, and `name:line` is the form
/// every reader already knows.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallSite {
    pub name: String,
    pub line: usize,
}

impl Serialize for CallSite {
    fn serialize<S: Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        out.collect_str(&format_args!("{}:{}", self.name, self.line))
    }
}

impl<'de> Deserialize<'de> for CallSite {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let text = String::deserialize(input)?;
        let (name, line) = text
            .rsplit_once(':')
            .ok_or_else(|| D::Error::custom(format!("a call site reads `name:line`, not `{text}`")))?;
        let line = line.parse().map_err(D::Error::custom)?;
        Ok(Self { name: name.to_string(), line })
    }
}

/// serde helper for additive numeric fields (mirrors `String::is_empty` above).
fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Where the last pass read from, so the next one reads only what changed.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct ScanState {
    /// The scanner build that wrote the model; another build reads everything.
    pub format: String,
    /// The commit checked out at the last pass (empty outside git).
    pub head: String,
    /// The files that were not committed at the last pass: they are read
    /// again even when git says nothing changed since, because they may have
    /// been put back.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dirty: Vec<String>,
    /// Source files that could not be decoded, so an unchanged one is counted
    /// without being opened again.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub non_utf8: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Decl {
    pub kind: String,
    pub name: String,
    pub line: usize,
    /// The last line of the declaration's node — so a caller can point to the
    /// current start..end of the whole function/struct/etc without
    /// recomputing it. `0` when the extractor could not resolve it (older
    /// models default here too; additive field).
    #[serde(default)]
    pub end_line: usize,
    /// Names this declaration builds on — base classes, implemented interfaces,
    /// embedded structs, implemented traits. Language-specific to capture,
    /// generic to mine: a base name shared by many entities is a shared contract.
    #[serde(default)]
    pub supertypes: Vec<String>,
    /// The documentation comment written right above the declaration, cleaned
    /// of its comment markers and joined into one line. Empty when there is
    /// none there — and in this project it is the only part written in the
    /// developer's own language, so it is what makes a question in words meet
    /// the code. Additive: older models default to empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    /// The declaration's own signature: what comes before its body, whitespace
    /// collapsed (name, parameters, return/base types). Never the body — the
    /// whole body was measured as the worst thing to keep. Empty when the
    /// declaration has no header to speak of.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
    /// The declarations of this project that this one calls, by name, sorted
    /// and deduped. Filled by [`crate::graph::link_declarations`] from the
    /// call sites of the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<String>,
    /// Every use of this declaration: which file, which line, and which
    /// declaration the call starts from. Filled by
    /// [`crate::graph::link_declarations`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_by: Vec<UseSite>,
}

/// One use of a declaration, `file:line:from`. The type lives in the core,
/// next to the map questions that read it, so the side that writes the map and
/// the side that answers from it understand the same text.
pub use mustard_core::domain::project_map::UseSite;

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

/// A base type / interface that many distinct entities build on — the shared
/// foundation a slice plugs into (e.g. EntityBase, RepositoryBase). Mined by
/// frequency over supertypes; never from a catalog.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SharedContract {
    pub name: String,
    pub implementors: usize,
}
