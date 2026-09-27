//! grain — map a codebase (files, declarations, links, history and stacks)
//! into a rich, language-agnostic model. Framework- and language-agnostic.
//!
//! Pipeline: ingest -> extract -> graph -> condense. Fully deterministic
//! and blind to any framework/language. `scan` writes the model into the
//! project map, the SQLite file the core port declares
//! (`mustard_core::io::project_map`).

mod classify;
mod condense;
mod facts;
mod extract;
mod graph;
mod ingest;
mod manifests;
mod model;
mod path_aliases;
mod refresh;
mod testmap;

use anyhow::Result;
use clap::{Parser, Subcommand};
use model::{Module, ProjectModel};
use mustard_core::domain::ast::is_test_path;
use mustard_core::domain::normalize::Languages;
use mustard_core::io::project_map::{self as store, Listing};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "grain",
    version,
    about = "Map a codebase (files, declarations, links, history and stacks) into a language-agnostic model."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze a project and write the model into the map database (the
    /// product).
    ///
    /// When `--out` already holds a model of this project, only the files that
    /// changed since that pass are read again (see `refresh`), and only the
    /// blocks that changed are written again; with nothing changed, the file is
    /// left alone. The map of before the database, in the same folder, is
    /// deleted once the database is written.
    Scan {
        path: PathBuf,
        #[arg(long, default_value = store::MAP_FILE_NAME)]
        out: PathBuf,
        /// Read every file, ignoring the previous model.
        #[arg(long)]
        all: bool,
        /// Print one JSON line (what was read) instead of the text summary.
        #[arg(long)]
        json: bool,
    },
}

/// Apaga o mapa de antes do banco, na pasta do banco em `out`, quando ele
/// existe: o banco gravado o substitui.
fn drop_legacy_map(out: &Path) -> Result<()> {
    let legacy = out.with_file_name(store::LEGACY_MAP_FILE_NAME);
    if legacy == out {
        return Ok(());
    }
    match std::fs::remove_file(&legacy) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan { path, out, all, json } => {
            let analysis = match census_pass(&path, &out, all) {
                Some(analysis) => analysis,
                None => {
                    let previous: Option<ProjectModel> = if all { None } else { ProjectModel::load(&out) };
                    analyze(&path, previous.as_ref())?
                }
            };
            // Nothing changed → the file is left alone (same bytes, same date).
            let written = if analysis.census_only {
                analysis.model.save_census(&out, refresh::FORMAT)?
            } else {
                analysis.model.save(&out, refresh::FORMAT, &Languages::of_project(&path))?
            };
            drop_legacy_map(&out)?;
            if json {
                let report = serde_json::json!({
                    "ok": true,
                    "full": analysis.full,
                    "read": analysis.read,
                    "files": analysis.model.modules.len(),
                    "head": analysis.model.state.head,
                });
                println!("{report}");
            } else {
                // O modelo da passada que só refez o censo não traz os
                // arquivos inteiros: o resumo lê o mapa que ela gravou.
                if analysis.census_only {
                    print_summary(&ProjectModel::read(&out)?);
                } else {
                    print_summary(&analysis.model);
                }
                if written {
                    println!("\nMap written to {}", out.display());
                } else {
                    println!("\nMap unchanged at {}", out.display());
                }
                println!(
                    "Read {} file(s){}",
                    analysis.read.len(),
                    if analysis.full { " (every file)" } else { " (only what changed)" }
                );
            }
        }
    }
    Ok(())
}

/// What one pass produced.
struct Analysis {
    model: ProjectModel,
    /// The files whose content this pass read, sorted.
    read: Vec<String>,
    /// Every file was read (no usable previous model).
    full: bool,
    /// A passada só refez o censo: o modelo traz o censo e, de cada arquivo,
    /// o caminho, o blob e os sinais de código, e só o censo se grava.
    census_only: bool,
}

/// A passada sem arquivo a reler, quando é o caso: o mapa em `out` é desta
/// versão do scan e do mesmo commit, nenhum arquivo que ele guarda mudou ou
/// saiu, e não entrou arquivo de código nem manifesto. Ela lê do mapa só o
/// estado, caminha pela pasta sem abrir arquivo e refaz o que depende dos
/// caminhos: a marca da listagem, as pastas de compilação e as pilhas do
/// projeto e de cada subprojeto, pela mesma conta da leitura inteira. As
/// declarações, o grafo e a história ficam como estão. `None` com `all`, ou
/// quando há o que reler.
fn census_pass(root: &Path, out: &Path, all: bool) -> Option<Analysis> {
    if all {
        return None;
    }
    let mut model = ProjectModel::load_state(out)?;
    let listing = store::listing(root)?;
    if !refresh::nothing_to_read(root, &model, &listing) {
        return None;
    }
    let walk = ingest::walk(root);
    if !ingest::same_sources(&walk.paths, &model) {
        return None;
    }
    let (detected_stacks, projects) = stacks(&model.manifests, &walk.paths, &model.modules);
    let manifest_paths: Vec<&str> = model.manifests.iter().map(|m| m.path.as_str()).collect();
    let inputs = refresh::inputs(&listing, &manifest_paths, &model.state.non_utf8);
    model.state.inputs = inputs;
    model.state.listing = listing.digest();
    model.detected_stacks = detected_stacks;
    model.projects = projects;
    model.coverage.skipped_build_dirs = walk.skipped_build_dirs;
    Some(Analysis { model, read: Vec::new(), full: false, census_only: true })
}

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

/// What one read of the project gives before the declarations are linked:
/// the walk, the modules with their imports resolved, the packages the
/// manifests name and the file graph.
struct Read {
    ing: ingest::Ingested,
    modules: Vec<Module>,
    packages: Vec<(String, String)>,
    /// Os apelidos de pasta das configurações do projeto, lidos uma vez e
    /// usados tanto nas importações quanto nos vínculos entre declarações.
    aliases: path_aliases::PathAliases,
    graph: graph::GraphBuild,
}

/// Walk the project and read every file `reuse` does not keep from the
/// previous map (all of them without one), then resolve what each module
/// imports. Each module carries the blob `listing` gives its path.
fn read_modules(root: &Path, reuse: Option<&ingest::Reuse>, listing: Option<&Listing>) -> Result<Read> {
    use mustard_core::domain::vocabulary::stacks::code_signals;

    let mut ing = ingest::ingest(root, reuse)?;
    let analyzers = extract::registry();
    // Repo classification overrides (.gitattributes / .editorconfig) — loaded
    // once; they beat the marker catalog in both directions.
    let overrides = classify::Overrides::load(&ing.root);

    // Cada arquivo lido agora é extraído em paralelo com os outros; o que a
    // passada toma do mapa anterior só perde o que se recalcula abaixo.
    let extracted = ingest::in_parallel(std::mem::take(&mut ing.files), |walked| match walked {
        ingest::Walked::Kept(mut kept) => {
            // Recomputed below from the whole set of modules. The call
            // sites, the citations and the fixed texts are NOT cleared: they
            // are what the file itself says, and a pass that did not read it
            // again resolves the same declaration links from them and keeps
            // the same texts.
            kept.deps.clear();
            kept.test_deps.clear();
            kept.tests.clear();
            Some(*kept)
        }
        ingest::Walked::Fresh(sf) => {
            let extracted = analyzers.get(sf.language.as_str()).map(|a| a.extract(&sf.content)).unwrap_or_default();
            // Machine-written class (generated/vendored/lockfile/minified) —
            // additive provenance on the module. The map keeps the module —
            // its file, its place in the graph and its declarations — and
            // leaves it out of its search and of its examples.
            let (file_class, marker) = classify::classify(&sf.rel_path, &sf.content, &overrides)
                .map(|c| (c.class, c.marker))
                .unwrap_or_default();
            // O texto fixo do arquivo de teste descreve o teste, e o do
            // arquivo escrito por máquina fica fora da busca: nenhum dos dois
            // se guarda.
            let texts =
                if file_class.is_empty() && !is_test_path(&sf.rel_path) { extracted.texts } else { Vec::new() };
            Some(Module {
                path: sf.rel_path.clone(),
                blob: String::new(),
                language: sf.language,
                loc: sf.loc,
                imports: extracted.imports,
                global_imports: extracted.global_imports,
                test_imports: extracted.test_imports,
                test_deps: Vec::new(),
                test_lines: extracted.test_lines,
                module_lines: extracted.module_lines,
                import_lines: extracted.import_lines,
                call_paths: extracted.call_paths,
                brought: extracted.brought,
                namespaces: extracted.namespaces,
                declarations: extracted.declarations,
                file_class,
                marker,
                deps: Vec::new(),
                tests: Vec::new(),
                has_tests: testmap::has_inline_tests(&sf.content),
                signals: code_signals(&sf.content),
                calls: extracted.calls,
                cites: extracted.cites,
                texts,
            })
        }
        // A caminhada lê todo arquivo que deixou para depois.
        ingest::Walked::Pending(_) => None,
    });
    let mut modules: Vec<Module> = extracted.into_iter().flatten().collect();
    // O blob do conteúdo lido, o de agora: o do arquivo tomado do mapa
    // anterior é o mesmo que ele guardava.
    if let Some(listing) = listing {
        for module in &mut modules {
            module.blob = listing.blobs.get(&module.path).cloned().unwrap_or_default();
        }
    }

    let packages = graph::packages(&ing.manifests);
    let aliases = path_aliases::PathAliases::load(&ing.root, &ing.walk_paths);
    let graph = graph::build(&modules, &ing.go_module, &packages, &aliases);
    // The project files each module imports, from the same resolved edges the
    // graph counts — the answer to "who imports this file", read backwards.
    // A importação de namespace chega aqui já estreitada aos arquivos que
    // declaram um nome que o módulo usa.
    let mut deps: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); modules.len()];
    for (from, to, _) in graph::resolve_edges(&modules, &ing.go_module, &packages, &aliases) {
        deps[from].insert(to);
    }
    let paths: Vec<String> = modules.iter().map(|m| m.path.clone()).collect();
    for (m, targets) in modules.iter_mut().zip(deps) {
        let mut named: Vec<String> = targets.into_iter().map(|i| paths[i].clone()).collect();
        named.sort();
        m.deps = named;
    }
    // O que o trecho de teste de cada módulo importa, resolvido no mesmo passo
    // e guardado à parte: não é dependência do arquivo, é o que o teste cobre.
    let test_deps = graph::resolve_test_deps(&modules, &ing.go_module, &packages, &aliases);
    for (m, found) in modules.iter_mut().zip(test_deps) {
        m.test_deps = found;
    }
    Ok(Read { ing, modules, packages, aliases, graph })
}

/// Deterministic stages (no synthesis, no AI): produce the project model, and
/// the dictionary sidecar when every file was read. With a `previous` model of
/// the same project, only the files that changed since are read (see
/// [`refresh`]); everything else is taken from it, and the result is the same
/// model a pass reading every file would give.
fn analyze(root: &Path, previous: Option<&ProjectModel>) -> Result<Analysis> {
    use mustard_core::domain::project_map::History;

    let listing = store::listing(root);
    let plan = refresh::plan(root, previous, listing.as_ref());
    let (full, read) = match (&plan, previous) {
        (refresh::Plan::Only(changed), Some(prev)) => {
            let first = read_modules(root, Some(&ingest::Reuse::new(changed, prev)), listing.as_ref())?;
            // A file that did not change is read again when what changed may
            // give its citations a link the previous map had no room for.
            let fresh: BTreeSet<String> = first.ing.read.iter().cloned().collect();
            let stale = refresh::stale_citers(&first.ing.root, prev, &first.modules, &fresh);
            if stale.is_empty() {
                (false, first)
            } else {
                let wider: BTreeSet<String> = changed.iter().cloned().chain(stale).collect();
                (false, read_modules(root, Some(&ingest::Reuse::new(&wider, prev)), listing.as_ref())?)
            }
        }
        _ => (true, read_modules(root, None, listing.as_ref())?),
    };
    let Read { ing, mut modules, packages, aliases, graph: (graph_stats, depth_by_path) } = read;

    // The named edges between declarations: who calls or cites whom, in which
    // file and on which line. Read from the call sites and the citations every
    // module carries, so a pass that read only what changed links the same
    // declarations a full pass does.
    graph::link_declarations(&mut modules, &ing.go_module, &packages, &ing.manifests, &aliases);
    // Cada tipo com os membros dele e cada método com o do contrato que ele
    // cumpre, refeitos do projeto inteiro como as ligações acima.
    graph::link_members(&mut modules);
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

    let (detected_stacks, projects) = stacks(&ing.manifests, &ing.walk_paths, &modules);

    // What the next pass needs to read only what changed: this commit, the
    // mark of what git lists now and the blob of each file that decides a
    // reading without being code.
    let manifest_paths: Vec<&str> = ing.manifests.iter().map(|m| m.path.as_str()).collect();
    let state = model::ScanState {
        head: head.unwrap_or_default(),
        listing: listing.as_ref().map(Listing::digest).unwrap_or_default(),
        inputs: listing.as_ref().map(|l| refresh::inputs(l, &manifest_paths, &ing.non_utf8)).unwrap_or_default(),
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
            coverage: ing.coverage,
            projects,
            state,
            history,
            marks: Default::default(),
        },
        read: ing.read,
        full,
        census_only: false,
    })
}

/// As pilhas do projeto e os subprojetos, cada um com as pilhas dele: a
/// mesma conta na passada que lê os arquivos e na que só refaz o censo.
///
/// Stack inference: the three evidence classes — parsed dependency names of
/// `manifests`, the file paths `walk_paths` and the code signatures the
/// `modules` carry. Which stacks exist and what identifies them is DATA in
/// mustard-core's registry. Evidence from a test file — under a conventional
/// test/fixture tree or named as a test — is discounted from all three: a
/// committed fixture of another stack describes what the project tests, not
/// what it is. The rule is the core's `is_test_path`, the one the whole scan
/// reads. Paths are relative to the SCANNED ROOT, so a fixture scanned
/// directly as the root carries no test segment and is not discounted.
fn stacks(
    manifests: &[model::Manifest],
    walk_paths: &[String],
    modules: &[Module],
) -> (Vec<mustard_core::domain::vocabulary::stacks::StackDetection>, Vec<model::ProjectUnit>) {
    use mustard_core::domain::vocabulary::stacks::infer_stacks;

    let evidence_deps: Vec<String> = manifests
        .iter()
        .filter(|m| !is_test_path(&m.path))
        .flat_map(|m| m.dependencies.iter().cloned())
        .collect();
    let evidence_paths: Vec<String> = walk_paths.iter().filter(|p| !is_test_path(p)).cloned().collect();
    let evidence_code = code_evidence(modules.iter().filter(|m| !is_test_path(&m.path)));
    let detected_stacks = infer_stacks(&evidence_deps, &evidence_paths, &evidence_code);

    let mut projects = build_projects(manifests, modules);
    infer_unit_stacks(&mut projects, manifests, walk_paths, modules);
    (detected_stacks, projects)
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
    // manifests it owns — the projection `facts::enrich_projects` owns, so the
    // grain `projects[]` carry the data. `scan_claude` reads `scripts` (for `## Commands`)
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
        // Same test-file discount as the repo-wide inference above: evidence
        // whose path (relative to the SCANNED ROOT, not the unit dir) is a test
        // file by the core's rule is excluded from all three classes — a unit
        // that ships fixtures of another stack must not report that stack as
        // its own.
        let owned = facts::owned_manifests(project, &snapshot, manifests);
        let deps: Vec<String> = owned
            .iter()
            .filter(|m| !is_test_path(&m.path))
            .flat_map(|m| m.dependencies.iter().cloned())
            .collect();
        let paths: Vec<String> = walk_paths
            .iter()
            .filter(|p| facts::dir_contains(&project.dir, p) && !is_test_path(p))
            .cloned()
            .collect();
        let contents = code_evidence(
            modules.iter().filter(|m| facts::dir_contains(&project.dir, &m.path) && !is_test_path(&m.path)),
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

    /// As palavras em maiúsculas fora de crase em `texts`, cada uma uma vez,
    /// no defeito que diz onde ela está.
    fn push_defects(place: &str, texts: &[String], out: &mut Vec<String>) {
        let mut seen: Vec<&str> = Vec::new();
        for text in texts {
            for word in mustard_core::platform::i18n::uppercase_words(text) {
                if !seen.contains(&word) {
                    seen.push(word);
                    out.push(format!("{place}: uppercase word {word} outside backticks"));
                }
            }
        }
    }

    /// Cada palavra em maiúsculas fora de crase na ajuda de `cmd` e dos
    /// comandos abaixo dele: o texto do comando, o de cada argumento e o de
    /// cada valor que o argumento aceita. O defeito diz o comando, o
    /// argumento, quando há, e a palavra.
    fn help_uppercase_defects(path: &str, cmd: &clap::Command, out: &mut Vec<String>) {
        let own: Vec<String> = [
            cmd.get_about(),
            cmd.get_long_about(),
            cmd.get_before_help(),
            cmd.get_before_long_help(),
            cmd.get_after_help(),
            cmd.get_after_long_help(),
        ]
        .into_iter()
        .flatten()
        .map(ToString::to_string)
        .collect();
        push_defects(path, &own, out);
        for arg in cmd.get_arguments() {
            let name = arg.get_long().map_or_else(|| arg.get_id().to_string(), |long| format!("--{long}"));
            let mut texts: Vec<String> =
                [arg.get_help(), arg.get_long_help()].into_iter().flatten().map(ToString::to_string).collect();
            texts.extend(arg.get_possible_values().iter().filter_map(|value| value.get_help().map(ToString::to_string)));
            push_defects(&format!("{path} {name}"), &texts, out);
        }
        for sub in cmd.get_subcommands() {
            help_uppercase_defects(&format!("{path} {}", sub.get_name()), sub, out);
        }
    }

    /// Os defeitos da árvore inteira, a partir do nome do programa.
    fn tree_defects(tree: &clap::Command) -> Vec<String> {
        let mut out = Vec::new();
        help_uppercase_defects(tree.get_name(), tree, &mut out);
        out
    }

    /// A ajuda de todo comando do scan segue a regra das frases do programa:
    /// nenhuma palavra toda em maiúsculas fora de crase, salvo a lista curta
    /// de siglas e unidades. A falha lista o comando, o argumento e a palavra.
    #[test]
    fn every_command_help_keeps_uppercase_inside_backticks() {
        use clap::CommandFactory;
        let tree = Cli::command();
        let names: Vec<&str> = tree.get_subcommands().map(clap::Command::get_name).collect();
        assert_eq!(names, ["scan"], "the check reached every command");
        let defects = tree_defects(&tree);
        assert!(defects.is_empty(), "{} help texts break the uppercase rule:\n{}", defects.len(), defects.join("\n"));
    }

    /// Um "THE" solto na ajuda de um comando, ou na de um argumento dele, cai
    /// com o comando, o argumento e a palavra; entre crases ele passa.
    #[test]
    fn a_loose_uppercase_word_in_a_help_fails_naming_the_command() {
        use clap::CommandFactory;
        let with = |about: &str, help: &str| {
            let (about, help) = (about.to_string(), help.to_string());
            Cli::command().mut_subcommand("scan", move |scan| scan.about(about).mut_arg("all", move |all| all.help(help)))
        };
        assert_eq!(
            tree_defects(&with("Writes THE model.", "Reads every file.")),
            vec!["grain scan: uppercase word THE outside backticks"]
        );
        assert_eq!(
            tree_defects(&with("Writes the model.", "Reads THE files.")),
            vec!["grain scan --all: uppercase word THE outside backticks"]
        );
        assert_eq!(tree_defects(&with("Writes `THE` model.", "Reads `THE` files.")), Vec::<String>::new());
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
