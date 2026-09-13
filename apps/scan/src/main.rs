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
mod rank;
mod refresh;
mod spec;
mod stemmers;
mod testmap;

use anyhow::Result;
use clap::{Parser, Subcommand};
use model::{Module, ProjectModel};
use std::collections::{BTreeSet, HashMap, HashSet};
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
    ///
    /// When `--out` already holds a model of this project, only the files that
    /// changed since that pass are read again (see `refresh`).
    Scan {
        path: PathBuf,
        #[arg(long, default_value = "grain.model.json")]
        out: PathBuf,
        /// Read every file, ignoring the previous model.
        #[arg(long)]
        all: bool,
        /// Print one JSON line (what was read) instead of the text summary.
        #[arg(long)]
        json: bool,
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
        /// Disable the ungated direct-identifier seeding: when set,
        /// only dictionary-matched terms seed (the pre-fix, dict-gated behavior).
        #[arg(long)]
        no_direct_seed: bool,
        /// Multiplier ×1024 on the absolute direct identifier-match score (the
        /// fan-in-exempt floor); calibrated so a strong match competes with the
        /// top propagated mass. `0` = no floor (walk only).
        #[arg(long, default_value_t = 100_000)]
        direct_base: u64,
        /// How many ranked files to emit.
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// One-shot research bundle for the `feature` flow: parse the model ONCE and
    /// return the per-query digest, the full domain-term index (the non-strong
    /// vocabulary menu) and the personalized-PageRank pool — the three
    /// projections `feature` used to fetch with three separate spawns, each
    /// re-parsing the model. `--query` carries the digest terms, `--rank-query`
    /// the expanded rank query, `--dict` the dictionary sidecar (rank is SKIPPED
    /// when the dict is absent, matching the fail-open gate the caller applies —
    /// an absent dict must yield an empty rank, never a direct-seeded one).
    /// Byte-stable JSON `{digest, terms, rank}`; `rank` is the pool at `--top`.
    FeatureBundle {
        path: PathBuf,
        /// Comma/space-separated digest query terms (the `digest --query` input).
        #[arg(long, default_value = "")]
        query: String,
        /// The `grain.dictionary.json` sidecar; rank is skipped when it is absent.
        #[arg(long)]
        dict: PathBuf,
        /// The expanded rank query (raw intent + equivalence tokens) for PageRank.
        #[arg(long, default_value = "")]
        rank_query: String,
        /// Rank pool depth; the caller derives the top-10 insumos list from this.
        #[arg(long, default_value_t = 25)]
        top: usize,
        /// Direct identifier-match floor multiplier (the `rank` --direct-base).
        #[arg(long, default_value_t = 100_000)]
        direct_base: u64,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

/// The `feature-bundle` output — the three projections `feature` consumes,
/// serialized together from ONE model parse (borrowed, so nothing is cloned).
#[derive(serde::Serialize)]
struct FeatureBundleOut<'a> {
    digest: &'a digest::QueryResult,
    terms: &'a [digest::TermD],
    rank: &'a [pagerank::ScoredFile],
}

/// Load a model: scan a project directory, or read a prebuilt grain.model.json.
fn load_model(path: &Path) -> Result<ProjectModel> {
    if path.extension().and_then(|e| e.to_str()) == Some("json") {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    } else {
        // Projections (digest/facts/spec) want only the model; the
        // dictionary sidecar is a scan-write concern, discarded here.
        Ok(analyze(path, None)?.model)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan { path, out, all, json } => {
            let previous: Option<ProjectModel> = if all {
                None
            } else {
                std::fs::read_to_string(&out).ok().and_then(|text| serde_json::from_str(&text).ok())
            };
            let analysis = analyze(&path, previous.as_ref())?;
            let model_json = serde_json::to_string_pretty(&analysis.model)?;
            // Nothing changed → the file is left alone (same bytes, same date).
            if std::fs::read_to_string(&out).ok().as_deref() != Some(model_json.as_str()) {
                if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&out, &model_json)?;
            }
            // The distinctive-vocabulary sidecar lands NEXT TO the model
            // (`grain.dictionary.json` beside `grain.model.json`). It is built
            // from every file's comments, so only a pass that read every file
            // rewrites it; an incremental pass leaves it as it was.
            let dict_out = out.with_file_name("grain.dictionary.json");
            if let Some(dictionary) = &analysis.dictionary {
                std::fs::write(&dict_out, serde_json::to_string_pretty(dictionary)?)?;
            }
            if json {
                let report = serde_json::json!({
                    "ok": true,
                    "full": analysis.full,
                    "read": analysis.read,
                    "files": analysis.model.modules.len(),
                    "head": analysis.model.state.head,
                    "dictionary": analysis.dictionary.is_some(),
                });
                println!("{report}");
            } else {
                print_summary(&analysis.model);
                println!("\nModel written to {}", out.display());
                println!(
                    "Read {} file(s){}",
                    analysis.read.len(),
                    if analysis.full { " (every file)" } else { " (only what changed)" }
                );
                if let Some(dictionary) = &analysis.dictionary {
                    println!("Dictionary written to {} ({} terms)", dict_out.display(), dictionary.terms.len());
                    if dictionary.non_english_comments > 0 {
                        println!(
                            "  {} non-English comment(s) detected — code smell to fix; raw tokens kept (they are the query-bridge keys)",
                            dictionary.non_english_comments
                        );
                    }
                }
            }
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
        Command::Rank { path, dict, query, direction, damping, iters, seed_weight, no_propagate, hub_penalty, fanin_penalty, no_direct_seed, direct_base, top, out } => {
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
                direct_seed: !no_direct_seed,
                direct_base_x1024: direct_base,
            };
            // Fail-open: a degraded/unreadable model or
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
        Command::FeatureBundle { path, query, dict, rank_query, top, direct_base, out } => {
            let model = load_model(&path)?;
            let terms: Vec<String> = query.split([',', ' ']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let digest = digest::query(&model, &terms);
            // The full domain-term index (the non-strong vocabulary menu) from the
            // SAME parsed model — so `feature` never spawns a second `digest`.
            let full = digest::build(&model);
            // Rank pool: SKIPPED when the dictionary is absent (the fail-open gate
            // the caller applies — an absent dict must yield an empty rank, never a
            // direct-seeded one). Present -> personalized PageRank at `top` depth
            // with the SAME config `rank` uses (only top + direct_base overridden),
            // so the pool, and its top-10 prefix (the insumos list), is byte-
            // identical to the two `rank` spawns it replaces.
            let rank: Vec<pagerank::ScoredFile> = if dict.is_file() {
                let dictionary: dictionary::Dictionary =
                    std::fs::read_to_string(&dict).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                let rank_terms: Vec<String> =
                    rank_query.split([',', ' ']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                let cfg = pagerank::RankConfig { top, direct_base_x1024: direct_base, ..Default::default() };
                pagerank::rank(&model, &dictionary, &rank_terms, &cfg).files
            } else {
                Vec::new()
            };
            let bundle = FeatureBundleOut { digest: &digest, terms: &full.terms, rank: &rank };
            let json = serde_json::to_string_pretty(&bundle)?;
            match out {
                Some(p) => {
                    std::fs::write(&p, &json)?;
                    println!("bundle written to {} ({} bytes)", p.display(), json.len());
                }
                None => println!("{json}"),
            }
        }
    }
    Ok(())
}

/// What one pass produced.
struct Analysis {
    model: ProjectModel,
    /// The distinctive-vocabulary dictionary, built only when every file was
    /// read: it comes from every file's comments.
    dictionary: Option<dictionary::Dictionary>,
    /// The files whose content this pass read, sorted.
    read: Vec<String>,
    /// Every file was read (no usable previous model).
    full: bool,
}

/// Only a specific import counts as a dependency in the map: one spread over a
/// bucket of more than eight files (weight below 1024 / 8) is left out.
const DEP_MIN_WEIGHT: u64 = 1024 / 8;

/// The code-signature evidence of some modules, as the stack inference takes
/// it: one text with every signature they carry, one per line. The inference
/// only asks which signatures fired, so this gives the same stacks the file
/// contents gave, without opening the files again.
fn code_evidence<'a>(modules: impl Iterator<Item = &'a Module>) -> Vec<String> {
    let signals: BTreeSet<&str> = modules.flat_map(|m| m.signals.iter().map(String::as_str)).collect();
    if signals.is_empty() {
        Vec::new()
    } else {
        vec![signals.into_iter().collect::<Vec<_>>().join("\n")]
    }
}

/// Deterministic stages (no synthesis, no AI): produce the project model, and
/// the dictionary sidecar when every file was read. With a `previous` model of
/// the same project, only the files that changed since are read (see
/// [`refresh`]); everything else is taken from it, and the result is the same
/// model a pass reading every file would give.
fn analyze(root: &Path, previous: Option<&ProjectModel>) -> Result<Analysis> {
    use mustard_core::domain::project_map::History;
    use mustard_core::domain::vocabulary::stacks::{code_signals, infer_stacks};

    let plan = refresh::plan(root, previous);
    let reuse = match (&plan, previous) {
        (refresh::Plan::Only(changed), Some(prev)) => Some(ingest::Reuse::new(changed, prev)),
        _ => None,
    };
    let full = reuse.is_none();
    let ing = ingest::ingest(root, reuse.as_ref())?;
    let analyzers = extract::registry();
    // Repo classification overrides (.gitattributes / .editorconfig) — loaded
    // once; they beat the marker catalog in both directions.
    let overrides = classify::Overrides::load(&ing.root);

    let mut modules: Vec<Module> = Vec::with_capacity(ing.files.len());
    let mut content: HashMap<String, String> = HashMap::new();
    for walked in ing.files {
        match walked {
            ingest::Walked::Kept(mut kept) => {
                // Recomputed below from the whole set of modules.
                kept.fan_in = 0;
                kept.deps.clear();
                kept.tests.clear();
                modules.push(kept);
            }
            ingest::Walked::Fresh(sf) => {
                let extracted =
                    analyzers.get(sf.language.as_str()).map(|a| a.extract(&sf.content)).unwrap_or_default();
                // Machine-written class (generated/vendored/lockfile/minified) —
                // additive provenance on the module. The model keeps the module
                // fully visible to the miner; only the digest projection demotes
                // by class.
                let (file_class, marker) = classify::classify(&sf.rel_path, &sf.content, &overrides)
                    .map(|c| (c.class, c.marker))
                    .unwrap_or_default();
                let module = Module {
                    path: sf.rel_path.clone(),
                    language: sf.language,
                    loc: sf.loc,
                    imports: extracted.imports,
                    namespaces: extracted.namespaces,
                    declarations: extracted.declarations,
                    file_class,
                    marker,
                    fan_in: 0, // filled below, once the import graph is resolved
                    deps: Vec::new(),
                    tests: Vec::new(),
                    has_tests: testmap::has_inline_tests(&sf.content),
                    signals: code_signals(&sf.content),
                };
                if full {
                    content.insert(sf.rel_path, sf.content);
                }
                modules.push(module);
            }
        }
    }

    let (graph_stats, degrees, depth_by_path) = graph::build(&modules, &ing.go_module);
    // Persist each module's fan-in (graph::build already computed the full
    // degree map) — additive on the model, so digest projections rank anchors
    // without re-deriving the graph.
    for m in &mut modules {
        m.fan_in = degrees.get(&m.path).map_or(0, |d| d.0);
    }
    // The project files each module imports, from the same resolved edges the
    // graph counts — the answer to "who imports this file", read backwards.
    let mut deps: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); modules.len()];
    for (from, to, weight) in graph::resolve_edges(&modules, &ing.go_module) {
        if weight >= DEP_MIN_WEIGHT {
            deps[from].insert(to);
        }
    }
    let paths: Vec<String> = modules.iter().map(|m| m.path.clone()).collect();
    for (m, targets) in modules.iter_mut().zip(deps) {
        let mut named: Vec<String> = targets.into_iter().map(|i| paths[i].clone()).collect();
        named.sort();
        m.deps = named;
    }
    let mined = mine::mine(&modules, &degrees);
    // Distinctive-vocabulary dictionary: a stage right after mining, over the
    // same `modules` + in-memory `content` (the only place comments survive),
    // reusing the mined role affixes to demote structural glue.
    let dictionary = full.then(|| dictionary::build(&modules, &content, &mined.roles));
    let skeleton = condense::build_skeleton(&modules, &depth_by_path);

    // The git history: only the commits since the previous pass, when that
    // pass read this same project.
    let root_text = ing.root.to_string_lossy().to_string();
    let same = previous.filter(|p| p.root == root_text);
    let head = refresh::head(&ing.root);
    let history = match &head {
        Some(now) => refresh::history(
            &ing.root,
            same.map(|p| &p.history),
            same.map_or("", |p| p.state.head.as_str()),
            now,
        ),
        None => History::default(),
    };
    testmap::assign(&mut modules, &history);

    // Stack inference: the three evidence classes — parsed dependency names,
    // file paths and the code signatures found in the sources. Which stacks
    // exist and what identifies them is DATA in mustard-core's registry.
    // Evidence under a conventional test/fixture tree is discounted from all
    // three: a committed fixture of another stack describes what the project
    // tests, not what it is.
    let evidence_deps: Vec<String> = ing
        .manifests
        .iter()
        .filter(|m| !ingest::under_test_dir(&m.path))
        .flat_map(|m| m.dependencies.iter().cloned())
        .collect();
    let evidence_paths: Vec<String> =
        ing.walk_paths.iter().filter(|p| !ingest::under_test_dir(p)).cloned().collect();
    let evidence_code = code_evidence(modules.iter().filter(|m| !ingest::under_test_dir(&m.path)));
    let detected_stacks = infer_stacks(&evidence_deps, &evidence_paths, &evidence_code);

    let mut projects = build_projects(&ing.manifests, &modules);
    infer_unit_stacks(&mut projects, &ing.manifests, &ing.walk_paths, &modules);

    // What the next pass needs to read only what changed: this commit and the
    // files not committed now (the ones the walk visits).
    let walked: HashSet<&str> = ing.walk_paths.iter().map(String::as_str).collect();
    let dirty: Vec<String> =
        refresh::dirty(&ing.root).unwrap_or_default().into_iter().filter(|p| walked.contains(p.as_str())).collect();
    let state = model::ScanState {
        format: refresh::FORMAT.to_string(),
        head: head.unwrap_or_default(),
        dirty,
        non_utf8: ing.non_utf8,
    };

    Ok(Analysis {
        model: ProjectModel {
            root: root_text,
            languages: ing.languages,
            manifests: ing.manifests,
            frameworks: ing.frameworks,
            detected_stacks,
            skeleton,
            modules,
            graph: graph_stats,
            roles: mined.roles,
            conventions: mined.conventions,
            coverage: ing.coverage,
            projects,
            shared_contracts: mined.shared_contracts,
            state,
            history,
        },
        dictionary,
        read: ing.read,
        full,
    })
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
    modules: &[Module],
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
        let contents = code_evidence(
            modules.iter().filter(|m| facts::dir_contains(&project.dir, &m.path) && !ingest::under_test_dir(&m.path)),
        );
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
