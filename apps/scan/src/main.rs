//! grain — learn the grain of a codebase from what recurs, and expose it as a
//! rich, language-agnostic model. Framework- and language-agnostic.
//!
//! Pipeline: ingest -> extract -> graph -> mine -> condense. Fully deterministic
//! and blind to any framework/language. `scan` writes the model; `spec` compiles
//! a per-task implementation draft from it.

mod classify;
mod condense;
mod dictionary;
mod digest;
mod facts;
mod extract;
mod graph;
mod ingest;
mod manifests;
mod matching;
mod mine;
mod model;
mod pagerank;
mod purpose;
mod rank;
mod spec;
mod stemmers;

use anyhow::Result;
use clap::{Parser, Subcommand};
use model::{Module, ProjectModel};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "grain", version, about = "Mine a codebase's recurring conventions into a language-agnostic model.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze a project and write the intermediate model as JSON (the product).
    Scan {
        path: PathBuf,
        #[arg(long, default_value = "grain.model.json")]
        out: PathBuf,
    },
    /// Emit a small, AI-sized capability DIGEST of the model (slices, roles,
    /// contracts, hubs, projects + a domain-term index) — the searchable surface
    /// a decomposition/feature step queries instead of reading source.
    ///
    /// With `--query`, returns only the slice of the digest matching the terms
    /// (a few KB instead of the whole catalog) — the cheap per-interaction lookup
    /// a `feature` does to research the repo without reading source files.
    Digest {
        path: PathBuf,
        /// Comma/space-separated domain terms to look up (OR across terms; terms
        /// <3 chars ignored), e.g. "tenant,receivable". Empty = full digest.
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Emit the small, stable FACTS the orchestrator consumes — the subproject
    /// list and the known declaration names — as JSON, so a consumer never has
    /// to parse the (large) model itself. `path` is a project dir to scan, or a
    /// model.json.
    Facts {
        path: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Compile a self-contained, deterministic implementation SPEC (draft) for an
    /// entity from the model. `path` is a project dir to scan, or a model.json.
    Spec {
        path: PathBuf,
        /// Entity to create (substitutes <Name> in the recipe).
        #[arg(long)]
        entity: String,
        /// Existing entity to mirror — its slice and its real files (e.g. a new
        /// entity modeled on an existing one of the same shape).
        #[arg(long, default_value = "")]
        like: String,
        /// Comma-separated operations beyond the base CRUD (e.g. "approve").
        #[arg(long, default_value = "create")]
        ops: String,
        /// Comma-separated cross-cutting invariants the unit must obey (e.g. an
        /// injected contract like "ICurrentTenant"). Surfaced as a must-obey
        /// section anchored on the real defining + consumer files (by graph
        /// fan-in + name), so the AI mirrors the mechanism instead of inventing it.
        #[arg(long, default_value = "")]
        invariant: String,
        /// Write the spec to a file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Find the files whose declarations' `purpose` summaries answer a free-text
    /// intent — the recall path for a method whose NAME diverges from the request
    /// vocabulary (e.g. PT "efetivar" vs `EffectivateAsync`). UNCAPPED over the
    /// model's purposed declarations and matched through the SAME ladder
    /// `digest --query` uses (with the trigram rescue rung ON). Deterministic, no
    /// LLM (the purposes are already in the model). `path` is a project dir to
    /// scan, or a model.json. Emits byte-stable JSON `{intent, files:[{file,
    /// matchedTerms}]}`; an empty `files` means nothing bridged.
    PurposeSearch {
        path: PathBuf,
        /// Comma/space-separated intent terms to search the purpose index for
        /// (OR across terms; terms <3 chars ignored), e.g. "approve,settle".
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Rank the model's files for a raw (e.g. Portuguese) request via personalized
    /// PageRank over the dependency graph, SEEDED by the distinctive-vocabulary
    /// dictionary — the localization layer over the dictionary's PT→term bridge.
    /// `path` is a project dir to scan, or a grain.model.json; `--dict` is the
    /// `grain.dictionary.json` sidecar. Emits byte-stable JSON `{query,
    /// matched_terms, files:[{file, score_x1024}]}`; an empty `files` means
    /// nothing bridged. Deterministic, no LLM.
    Rank {
        path: PathBuf,
        /// The `grain.dictionary.json` sidecar (the seed vocabulary).
        #[arg(long)]
        dict: PathBuf,
        /// Comma/space-separated request terms (the raw intent), e.g. a PT prompt.
        #[arg(long, default_value = "")]
        query: String,
        /// Edge orientation: `forward` | `reverse` | `undirected` (default —
        /// the graph splits by language, so domain-locality is undirected).
        #[arg(long, default_value = "undirected")]
        direction: String,
        /// Damping ×1024 (default ≈ 0.60 → 614: a strong topic bias keeps mass
        /// near the seeds; classic PageRank ≈ 0.85 → 870).
        #[arg(long, default_value_t = 614)]
        damping: u64,
        /// Fixed power-iteration count (byte-stable — never a float convergence test).
        #[arg(long, default_value_t = 50)]
        iters: usize,
        /// Seed weighting: `specificity` (default) | `idf` | `balanced` | `uniform`.
        #[arg(long, default_value = "specificity")]
        seed_weight: String,
        /// Rank the personalization vector alone (ablation: no graph walk).
        #[arg(long)]
        no_propagate: bool,
        /// Hub penalty ×1024 against a file's dictionary-anchor promiscuity
        /// (cross-cutting comment-dense files); 0 = off (default).
        #[arg(long, default_value_t = 0)]
        hub_penalty: u64,
        /// Fan-in penalty ×1024 against a file's global import fan-in (deep
        /// shared sinks a walk piles onto); default 1.0 → 1024, `0` = off.
        #[arg(long, default_value_t = 1024)]
        fanin_penalty: u64,
        /// How many ranked files to emit.
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Verify an implementation against the spec's acceptance criteria: for each
    /// required location, check the project has a file for the entity. `path` is
    /// the (implemented) project dir, or a grain.model.json (+ its root).
    Verify {
        path: PathBuf,
        #[arg(long)]
        entity: String,
        #[arg(long, default_value = "")]
        like: String,
        #[arg(long, default_value = "create")]
        ops: String,
    },
}

/// Count files under `dir` that belong to the entity. If the folder itself is
/// the entity's (its path contains the entity), any file counts; in a SHARED
/// folder, only files whose name contains the entity token count.
fn count_entity_files(dir: &Path, entity_lower: &str, owned: bool) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| owned || e.file_name().to_string_lossy().to_lowercase().contains(entity_lower))
        .count()
}

/// Load a model: scan a project directory, or read a prebuilt grain.model.json.
fn load_model(path: &Path) -> Result<ProjectModel> {
    if path.extension().and_then(|e| e.to_str()) == Some("json") {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    } else {
        // Projections (digest/facts/spec/purpose) want only the model; the
        // dictionary sidecar is a scan-write concern, discarded here.
        Ok(analyze(path)?.0)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan { path, out } => {
            let (model, dictionary) = analyze(&path)?;
            print_summary(&model);
            std::fs::write(&out, serde_json::to_string_pretty(&model)?)?;
            println!("\nModel written to {}", out.display());
            // The distinctive-vocabulary sidecar lands NEXT TO the model
            // (`grain.dictionary.json` beside `grain.model.json`), so `/scan`
            // (rt → grain --out .claude/grain.model.json) produces both.
            let dict_out = out.with_file_name("grain.dictionary.json");
            std::fs::write(&dict_out, serde_json::to_string_pretty(&dictionary)?)?;
            println!("Dictionary written to {} ({} terms)", dict_out.display(), dictionary.terms.len());
        }
        Command::Digest { path, query, out } => {
            let model = load_model(&path)?;
            let terms: Vec<String> = query.split([',', ' ']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let json = if terms.is_empty() {
                serde_json::to_string_pretty(&digest::build(&model))?
            } else {
                serde_json::to_string_pretty(&digest::query(&model, &terms))?
            };
            match out {
                Some(p) => {
                    std::fs::write(&p, &json)?;
                    println!("digest written to {} ({} bytes)", p.display(), json.len());
                }
                None => println!("{json}"),
            }
        }
        Command::Facts { path, out } => {
            let model = load_model(&path)?;
            let json = serde_json::to_string_pretty(&facts::build(&model))?;
            match out {
                Some(p) => {
                    std::fs::write(&p, &json)?;
                    println!("facts written to {} ({} bytes)", p.display(), json.len());
                }
                None => println!("{json}"),
            }
        }
        Command::Spec { path, entity, like, ops, invariant, out } => {
            let model = load_model(&path)?;
            let ops_vec: Vec<String> = ops.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let inv_vec: Vec<String> = invariant.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let spec_md = spec::compile(&model, &entity, &like, &ops_vec, &inv_vec);
            match out {
                Some(p) => {
                    std::fs::write(&p, &spec_md)?;
                    println!("spec written to {}", p.display());
                }
                None => println!("{spec_md}"),
            }
        }
        Command::PurposeSearch { path, query, out } => {
            let terms: Vec<String> = query.split([',', ' ']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            // Fail-open: an unreadable / unparseable model yields an empty result
            // (the orchestrator calls this on a miss; a degraded model must never
            // turn the recall attempt into a hard error). On a load failure the
            // empty intent stands.
            let result = match load_model(&path) {
                Ok(model) => purpose::search(&model, &terms),
                Err(_) => purpose::PurposeResult { intent: terms.join(" "), files: Vec::new() },
            };
            let json = serde_json::to_string_pretty(&result)?;
            match out {
                Some(p) => {
                    std::fs::write(&p, &json)?;
                    println!("purpose-search written to {} ({} bytes)", p.display(), json.len());
                }
                None => println!("{json}"),
            }
        }
        Command::Rank { path, dict, query, direction, damping, iters, seed_weight, no_propagate, hub_penalty, fanin_penalty, top, out } => {
            let terms: Vec<String> = query.split([',', ' ']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let cfg = pagerank::RankConfig {
                direction: pagerank::Direction::parse(&direction),
                damping_x1024: damping,
                iterations: iters,
                top,
                seed_weight: pagerank::SeedWeight::parse(&seed_weight),
                propagate: !no_propagate,
                hub_penalty_x1024: hub_penalty,
                fanin_penalty_x1024: fanin_penalty,
            };
            // Fail-open like purpose-search: a degraded/unreadable model or
            // dictionary yields an empty ranked list, never a hard error.
            let dictionary: dictionary::Dictionary =
                std::fs::read_to_string(&dict).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            let result = match load_model(&path) {
                Ok(model) => pagerank::rank(&model, &dictionary, &terms, &cfg),
                Err(_) => pagerank::rank(&ProjectModel::default(), &dictionary, &terms, &cfg),
            };
            let json = serde_json::to_string_pretty(&result)?;
            match out {
                Some(p) => {
                    std::fs::write(&p, &json)?;
                    println!("rank written to {} ({} bytes)", p.display(), json.len());
                }
                None => println!("{json}"),
            }
        }
        Command::Verify { path, entity, like, ops } => {
            let model = load_model(&path)?;
            // Where to check files: the project dir given, or the model's own root.
            let root = if path.extension().and_then(|e| e.to_str()) == Some("json") {
                PathBuf::from(&model.root)
            } else {
                path.clone()
            };
            let ops_vec: Vec<String> = ops.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let items = spec::acceptance(&model, &entity, &like, &ops_vec);
            let elc = entity.to_lowercase();

            println!("== verify: {entity} — {} criterion(s) (is there a file for the entity in the folder?) ==", items.len());
            let (mut done, mut req_total, mut req_done) = (0usize, 0usize, 0usize);
            for it in &items {
                // A folder whose path already names the entity is "owned" by it
                // (e.g. `.../<Entity>s/Services/`) — any file there counts.
                let owned = it.folder.to_lowercase().contains(&elc);
                let count = count_entity_files(&root.join(&it.folder), &elc, owned);
                let ok = count > 0;
                if ok {
                    done += 1;
                }
                if !it.optional {
                    req_total += 1;
                    if ok {
                        req_done += 1;
                    }
                }
                let mark = if ok { "[x]" } else { "[ ]" };
                let tag = if it.optional { " (optional)" } else { "" };
                println!("  {mark} {}{tag} — {} ({count} file(s) for {entity})", it.roles, it.folder);
            }
            let pct = if req_total > 0 { req_done * 100 / req_total } else { 100 };
            println!("\nrequired: {req_done}/{req_total} ({pct}%) · total (with optionals): {done}/{}", items.len());
            if req_done < req_total {
                println!("INCOMPLETE — required locations above are missing.");
            } else {
                println!("All required locations have a file for the entity.");
            }
        }
    }
    Ok(())
}

/// Deterministic stages: produce the project model AND the distinctive-
/// vocabulary dictionary sidecar (no synthesis, no LLM). The dictionary is
/// returned alongside the model because it is mined from the in-memory `content`
/// (comments), which only exists during the scan — see [`dictionary`].
fn analyze(root: &Path) -> Result<(ProjectModel, dictionary::Dictionary)> {
    let ing = ingest::ingest(root)?;
    let analyzers = extract::registry();
    // Repo classification overrides (.gitattributes / .editorconfig) — loaded
    // once; they beat the marker catalog in both directions.
    let overrides = classify::Overrides::load(&ing.root);

    let mut modules: Vec<Module> = Vec::new();
    let mut content: HashMap<String, String> = HashMap::new();

    for sf in &ing.source_files {
        let extracted = analyzers.get(sf.language.as_str()).map(|a| a.extract(&sf.content)).unwrap_or_default();
        // Machine-written class (generated/vendored/lockfile/minified) —
        // additive provenance on the module. The model keeps the module fully
        // visible to the miner; only the digest projection demotes by class.
        let (file_class, marker) =
            classify::classify(&sf.rel_path, &sf.content, &overrides).map(|c| (c.class, c.marker)).unwrap_or_default();
        content.insert(sf.rel_path.clone(), sf.content.clone());
        modules.push(Module {
            path: sf.rel_path.clone(),
            language: sf.language.clone(),
            loc: sf.loc,
            imports: extracted.imports,
            namespaces: extracted.namespaces,
            declarations: extracted.declarations,
            file_class,
            marker,
            fan_in: 0, // filled below, once the import graph is resolved
        });
    }

    let (graph_stats, degrees, depth_by_path) = graph::build(&modules, &ing.go_module);
    // Persist each module's fan-in (graph::build already computed the full
    // degree map) — additive on the model, so digest projections rank anchors
    // without re-deriving the graph.
    for m in &mut modules {
        m.fan_in = degrees.get(&m.path).map_or(0, |d| d.0);
    }
    let mined = mine::mine(&modules, &degrees, &content);
    // Distinctive-vocabulary dictionary: a stage right after mining, over the
    // same `modules` + in-memory `content` (the only place comments survive),
    // reusing the mined role affixes to demote structural glue.
    let dictionary = dictionary::build(&modules, &content, &mined.roles);
    let skeleton = condense::build_skeleton(&modules, &depth_by_path);

    let mut projects = build_projects(&ing.manifests, &modules);
    infer_unit_stacks(&mut projects, &ing.manifests, &ing.walk_paths, &ing.source_files);

    Ok((
        ProjectModel {
            root: ing.root.to_string_lossy().to_string(),
            languages: ing.languages,
            manifests: ing.manifests,
            frameworks: ing.frameworks,
            detected_stacks: ing.detected_stacks,
            skeleton,
            modules,
            graph: graph_stats,
            roles: mined.roles,
            conventions: mined.conventions,
            coverage: ing.coverage,
            projects,
            shared_contracts: mined.shared_contracts,
        },
        dictionary,
    ))
}

/// Map each project (one per manifest) to its directory and count the source
/// files that live under it, attributing each file to the *longest* matching
/// project dir so nested projects are not double-counted.
fn build_projects(manifests: &[model::Manifest], modules: &[Module]) -> Vec<model::ProjectUnit> {
    use model::ProjectUnit;
    let dir_of = |p: &str| -> String {
        match p.rfind('/') {
            Some(i) => p[..i].to_string(),
            None => String::new(),
        }
    };
    // Project name was derived at ingest time per the manifest's own rule
    // (manifests.toml) — no build-system literal here. Fall back to the dir.
    let name_of = |m: &model::Manifest| -> String {
        if !m.name.is_empty() {
            m.name.clone()
        } else {
            dir_of(&m.path).rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("(root)").to_string()
        }
    };
    let mut projects: Vec<ProjectUnit> = manifests
        .iter()
        .map(|m| ProjectUnit { name: name_of(m), dir: dir_of(&m.path), kind: m.kind.clone(), code_files: 0, ..Default::default() })
        .collect();
    // longest-prefix attribution
    for md in modules {
        let mut best: Option<usize> = None;
        let mut best_len = 0usize;
        for (i, p) in projects.iter().enumerate() {
            let under = if p.dir.is_empty() { true } else { md.path == p.dir || md.path.starts_with(&format!("{}/", p.dir)) };
            if under && (best.is_none() || p.dir.len() >= best_len) {
                best = Some(i);
                best_len = p.dir.len();
            }
        }
        if let Some(i) = best {
            projects[i].code_files += 1;
        }
    }
    let mut projects = dedup_by_dir(projects);
    projects.sort_by(|a, b| b.code_files.cmp(&a.code_files).then(a.name.cmp(&b.name)));
    // Enrich each unit with the frameworks/dependencies/scripts mined from the
    // manifests it owns — the SAME projection the facts view uses, so the grain
    // `projects[]` carry the data. `scan_claude` reads `scripts` (for `## Commands`)
    // and `frameworks` (for the Guards facts) straight off `projects[]`; without
    // this they were left at `..Default` (empty), so `## Commands` stayed dormant.
    let snapshot = projects.clone();
    facts::enrich_projects(&mut projects, &snapshot, manifests);
    projects
}

/// Populate each unit's `detected_stacks` from the unit's OWN evidence slice:
/// the dependencies of the manifests it owns (the same longest-prefix crossing
/// `facts::enrich_projects` applies to frameworks/deps/scripts), the walk paths
/// under its dir, and the source contents under its dir. Same engine, same
/// generic call as the repo-wide inference in `ingest` — which stacks exist is
/// DATA in mustard-core's registry, never logic here. Deterministic: the walk
/// paths arrive sorted from `ingest` and prefix-filtering preserves that order;
/// for a single-unit repo the result coincides with the model-level field by
/// construction.
fn infer_unit_stacks(
    projects: &mut [model::ProjectUnit],
    manifests: &[model::Manifest],
    walk_paths: &[String],
    source_files: &[ingest::SourceFile],
) {
    use mustard_core::domain::vocabulary::stacks::infer_stacks;
    // Immutable snapshot for the longest-prefix ownership test while mutating.
    let snapshot: Vec<model::ProjectUnit> = projects.to_vec();
    for project in projects.iter_mut() {
        // Same test-tree discount as the repo-wide inference in `ingest`:
        // evidence whose path (relative to the SCANNED ROOT, not the unit dir)
        // sits under a conventional test/fixture segment is excluded from all
        // three classes — a unit that ships fixtures of another stack must not
        // report that stack as its own.
        let owned = facts::owned_manifests(project, &snapshot, manifests);
        let deps: Vec<String> = owned
            .iter()
            .filter(|m| !ingest::under_test_dir(&m.path))
            .flat_map(|m| m.dependencies.iter().cloned())
            .collect();
        let paths: Vec<String> = walk_paths
            .iter()
            .filter(|p| facts::dir_contains(&project.dir, p) && !ingest::under_test_dir(p))
            .cloned()
            .collect();
        let contents: Vec<String> = source_files
            .iter()
            .filter(|s| facts::dir_contains(&project.dir, &s.rel_path) && !ingest::under_test_dir(&s.rel_path))
            .map(|s| s.content.clone())
            .collect();
        project.detected_stacks = infer_stacks(&deps, &paths, &contents);
    }
}

/// Collapse units that resolve to the same directory into one, keeping the entry
/// with the most `code_files` (ties: first occurrence, which is the model's
/// manifest order). Several manifests can map to one dir — most visibly a Cargo
/// workspace whose root `Cargo.toml` yields an empty-dir root unit alongside
/// another root manifest — and the duplicate steals part of the file attribution,
/// surfacing as a "0 arquivos" root. Merging by dir gives one honest count.
fn dedup_by_dir(projects: Vec<model::ProjectUnit>) -> Vec<model::ProjectUnit> {
    use std::collections::HashMap;
    // dir -> index into `out` of the winning unit so far.
    let mut winner: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<model::ProjectUnit> = Vec::with_capacity(projects.len());
    for p in projects {
        match winner.get(&p.dir).copied() {
            Some(idx) if p.code_files <= out[idx].code_files => {
                // An earlier unit on this dir already counts at least as many
                // files — keep it (stable: first occurrence wins ties).
            }
            Some(idx) => {
                // This unit attributed more files; promote it as the survivor.
                out[idx] = p;
            }
            None => {
                winner.insert(p.dir.clone(), out.len());
                out.push(p);
            }
        }
    }
    out
}

fn print_summary(model: &ProjectModel) {
    println!("== scan ==");
    println!("root: {}", model.root);
    let langs: Vec<String> = model.languages.iter().take(4).map(|l| format!("{} ({})", l.language, l.files)).collect();
    println!("languages: {}", langs.join(", "));
    if !model.frameworks.is_empty() {
        println!("dependencies: {}", model.frameworks.join(", "));
    }
    if model.projects.len() > 1 {
        let ps: Vec<String> = model.projects.iter().map(|p| format!("{} ({}, {} files)", p.name, if p.dir.is_empty() { "." } else { &p.dir }, p.code_files)).collect();
        println!("projects: {}", ps.join("; "));
    }
    println!("graph: {} modules, {} edges, cyclic={}", model.graph.nodes, model.graph.edges, model.graph.cyclic);
    println!("roles discovered: {}", model.roles.iter().map(|r| format!("{}({})", r.affix, r.count)).collect::<Vec<_>>().join(", "));
    println!("mined conventions:");
    for c in &model.conventions {
        let tag = if c.is_slice { "slice " } else { "single" };
        println!("  - [{tag}] {} (recurs {}x, conf {:.2})", c.name, c.recurrence, c.confidence);
    }

    let cov = &model.coverage;
    println!("\n== coverage (what was read) ==");
    println!("code files read: {} ({} non-utf8 skipped)", cov.code_files_read, cov.non_utf8_skipped);
    println!("by top dir:");
    for d in &cov.top_dirs {
        let other = if d.other_files > 0 { format!(", {} other", d.other_files) } else { String::new() };
        println!("  {:<22} {} code{}", d.dir, d.code_files, other);
    }
    if !cov.skipped_build_dirs.is_empty() {
        println!("build/dep dirs skipped: {}", cov.skipped_build_dirs.join(", "));
    }
    if !cov.unsupported_exts.is_empty() {
        let top: Vec<String> = cov.unsupported_exts.iter().take(10).map(|e| format!("{} {}", e.ext, e.count)).collect();
        println!("seen but not mined (non-code): {}", top.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(path: &str, kind: &str, name: &str) -> model::Manifest {
        model::Manifest { path: path.into(), kind: kind.into(), name: name.into(), ..Default::default() }
    }

    fn module(path: &str) -> Module {
        Module { path: path.into(), ..Default::default() }
    }

    #[test]
    fn root_dedup() {
        // Two manifests resolve to the SAME (root) dir — the classic Cargo
        // workspace shape where a virtual root `Cargo.toml` produces a root unit
        // and a second root-level manifest produces another. Without dedup the
        // file attribution is split across the twins, surfacing a "0 arquivos"
        // root. After dedup there is exactly one root unit carrying the count.
        let manifests = vec![
            manifest("Cargo.toml", "cargo", "workspace"),
            manifest("rust-toolchain.toml", "cargo", "(root)"),
            manifest("apps/rt/Cargo.toml", "cargo", "rt"),
        ];
        let modules = vec![
            module("src/main.rs"),
            module("build.rs"),
            module("apps/rt/src/lib.rs"),
        ];
        let projects = build_projects(&manifests, &modules);

        // Exactly one unit per distinct dir — the two root manifests collapse.
        let root_units: Vec<&model::ProjectUnit> =
            projects.iter().filter(|p| p.dir.is_empty()).collect();
        assert_eq!(root_units.len(), 1, "root must be deduped to one unit: {projects:?}");
        // The surviving root keeps its real file count, never 0.
        assert_eq!(root_units[0].code_files, 2, "root file count merged, not split");
        // The nested subproject is untouched.
        let rt = projects.iter().find(|p| p.dir == "apps/rt").unwrap();
        assert_eq!(rt.code_files, 1, "nested unit keeps its own files");
    }

    #[test]
    fn dedup_keeps_unit_with_most_files() {
        // When two units share a dir, the survivor is the one that attributed the
        // most files (ties → first occurrence).
        let mut a = model::ProjectUnit { name: "a".into(), dir: "pkg".into(), code_files: 1, ..Default::default() };
        let b = model::ProjectUnit { name: "b".into(), dir: "pkg".into(), code_files: 5, ..Default::default() };
        let c = model::ProjectUnit { name: "c".into(), dir: "other".into(), code_files: 2, ..Default::default() };
        a.kind = "x".into();
        let out = dedup_by_dir(vec![a, b, c]);
        assert_eq!(out.len(), 2, "one per dir: {out:?}");
        let pkg = out.iter().find(|p| p.dir == "pkg").unwrap();
        assert_eq!(pkg.name, "b", "the higher-count unit wins");
        assert_eq!(pkg.code_files, 5);
    }
}
