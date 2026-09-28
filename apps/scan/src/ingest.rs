//! Layer 0 — Ingestion.
//!
//! Walk the tree (respecting .gitignore via the `ignore` crate), detect file
//! languages, count LOC, parse build manifests, and infer frameworks from
//! dependencies. Manifests are the cheapest, highest-signal fingerprint there
//! is: they reveal language + framework + deps without parsing a line of code.

use crate::model::{Coverage, DirCoverage, ExtCount, LanguageStat, Manifest, Module, ProjectModel};
use anyhow::Result;
use ignore::WalkBuilder;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mustard_core::io::project_map::Listing;

pub(crate) struct Ingested {
    pub root: PathBuf,
    /// Every source file the walk visited, in walk order: read now, or taken
    /// from the previous map as it was.
    pub files: Vec<Walked>,
    pub manifests: Vec<Manifest>,
    pub languages: Vec<LanguageStat>,
    pub frameworks: Vec<String>,
    /// Every file path the walk visited (relative, /-normalized, sorted) — the
    /// path evidence class, kept so later stages can slice it per unit and run
    /// the same inference on a unit's own evidence.
    pub walk_paths: Vec<String>,
    pub coverage: Coverage,
    /// The files whose content this pass read (sources and manifests), sorted.
    pub read: Vec<String>,
    /// Source files that could not be decoded, sorted.
    pub non_utf8: Vec<String>,
}

pub struct SourceFile {
    pub rel_path: String,
    pub language: String,
    pub loc: usize,
    pub content: String,
}

/// A source file as the walk left it: read now, or kept from the previous map.
pub(crate) enum Walked {
    Fresh(SourceFile),
    Kept(Box<Module>),
    /// A ler ainda: só dentro da caminhada, que lê todos no fim dela.
    Pending(Pending),
}

/// Um arquivo de código que a caminhada vai ler.
pub(crate) struct Pending {
    rel: String,
    language: String,
    path: PathBuf,
    topdir: String,
}

/// `work` sobre cada item de `items`, dividido entre os núcleos da máquina,
/// com as respostas na ordem dos itens.
pub(crate) fn in_parallel<T: Send, R: Send>(items: Vec<T>, work: impl Fn(T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    if threads < 2 || items.len() < 2 {
        return items.into_iter().map(work).collect();
    }
    let per_thread = items.len().div_ceil(threads);
    let mut chunks: Vec<Vec<T>> = Vec::new();
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        chunks.push(items.by_ref().take(per_thread).collect());
    }
    let work = &work;
    std::thread::scope(|scope| {
        let handles: Vec<_> =
            chunks.into_iter().map(|chunk| scope.spawn(move || chunk.into_iter().map(work).collect::<Vec<R>>())).collect();
        handles.into_iter().flat_map(|handle| handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))).collect()
    })
}

/// What an incremental pass may take from the previous map instead of reading
/// the file again: everything outside `changed`.
pub(crate) struct Reuse<'a> {
    changed: &'a BTreeSet<String>,
    modules: HashMap<&'a str, &'a Module>,
    manifests: HashMap<&'a str, &'a Manifest>,
    non_utf8: HashSet<&'a str>,
}

impl<'a> Reuse<'a> {
    pub(crate) fn new(changed: &'a BTreeSet<String>, prev: &'a ProjectModel) -> Self {
        Self {
            changed,
            modules: prev.modules.iter().map(|m| (m.path.as_str(), m)).collect(),
            manifests: prev.manifests.iter().map(|m| (m.path.as_str(), m)).collect(),
            non_utf8: prev.state.non_utf8.iter().map(String::as_str).collect(),
        }
    }

    fn keeps(&self, rel: &str) -> bool {
        !self.changed.contains(rel)
    }

    fn kept_module(&self, rel: &str) -> Option<&'a Module> {
        if self.keeps(rel) { self.modules.get(rel).copied() } else { None }
    }

    fn kept_manifest(&self, rel: &str) -> Option<&'a Manifest> {
        if self.keeps(rel) { self.manifests.get(rel).copied() } else { None }
    }

    fn kept_undecodable(&self, rel: &str) -> bool {
        self.keeps(rel) && self.non_utf8.contains(rel)
    }
}

/// As pastas, relativas à raiz, que guardam algum arquivo de código que o
/// índice do git tem, em qualquer profundidade.
fn dirs_with_indexed_code(listing: &Listing) -> HashSet<String> {
    let mut dirs = HashSet::new();
    for path in listing.indexed.iter().filter(|path| crate::extract::detect_language(Path::new(path)).is_some()) {
        for (slash, _) in path.match_indices('/') {
            dirs.insert(path[..slash].to_string());
        }
    }
    dirs
}

/// A caminhada pela pasta `root`: respeita o `.gitignore`, entra nas pastas
/// escondidas e pula as que o registro dos manifestos declara, nunca uma
/// lista escrita aqui. A pasta que nunca guarda código do projeto fica
/// sempre fora. A de saída ou de dependências fica fora quando o índice do
/// git, pela listagem `listing`, não guarda nela nenhum arquivo de código, e
/// sempre fora do git, sem listagem. Cada pasta pulada entra em `skipped`,
/// pelo caminho relativo; a que o `.gitignore` já pula não é da lista.
fn walker(root: &Path, listing: Option<&Listing>, skipped: Arc<Mutex<Vec<String>>>) -> ignore::Walk {
    let never = crate::manifests::skip_dirs();
    let output = crate::manifests::output_dirs();
    let holding_code = listing.map(dirs_with_indexed_code).unwrap_or_default();
    let base = root.to_path_buf();
    WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .filter_entry(move |e| {
            if !e.file_type().is_some_and(|kind| kind.is_dir()) {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            let named = |list: &[String]| list.iter().any(|s| s.as_str() == name.as_ref());
            if !named(never) && !named(output) {
                return true;
            }
            let rel = relative(&base, e.path());
            if !named(never) && holding_code.contains(&rel) {
                return true;
            }
            skipped.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(rel);
            false
        })
        .build()
}

/// As pastas que a caminhada pulou, em ordem.
fn taken(skipped: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    let mut dirs = std::mem::take(&mut *skipped.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    dirs.sort();
    dirs
}

/// O caminho de `path` relativo a `root`, com `/` entre as partes.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// O que a caminhada vê sem abrir arquivo nenhum.
pub(crate) struct Walk {
    /// Cada arquivo que a caminhada visita, relativo à raiz e ordenado: os
    /// mesmos caminhos que a leitura inteira guarda para as pilhas.
    pub paths: Vec<String>,
    /// As pastas que a caminhada pulou pela lista do registro, pelo caminho
    /// relativo, em qualquer profundidade, ordenadas.
    pub skipped_build_dirs: Vec<String>,
}

/// A caminhada pela pasta `root` que a leitura faz, sem abrir arquivo nenhum,
/// com a regra das pastas puladas da leitura inteira.
pub(crate) fn walk(root: &Path, listing: Option<&Listing>) -> Walk {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let skipped = Arc::new(Mutex::new(Vec::new()));
    let mut paths: Vec<String> = walker(&root, listing, Arc::clone(&skipped))
        .flatten()
        .filter(|dent| dent.path().is_file())
        .map(|dent| relative(&root, dent.path()))
        .collect();
    paths.sort();
    Walk { paths, skipped_build_dirs: taken(&skipped) }
}

/// A caminhada `paths` não traz arquivo que a leitura abriria sem que o mapa
/// anterior `prev` o guarde, e o mapa não guarda arquivo que ela não traga:
/// todo arquivo de código ou manifesto que ela visita é um módulo, um
/// manifesto ou um arquivo que não se decodificou no mapa, e todo arquivo
/// dele continua nela. Com o conteúdo de cada um igual ao que ele leu, a
/// leitura inteira daria os mesmos módulos e manifestos; só o que depende
/// dos outros caminhos muda.
pub(crate) fn same_sources(paths: &[String], prev: &ProjectModel) -> bool {
    let stored: HashSet<&str> = prev
        .modules
        .iter()
        .map(|m| m.path.as_str())
        .chain(prev.manifests.iter().map(|m| m.path.as_str()))
        .chain(prev.state.non_utf8.iter().map(String::as_str))
        .collect();
    let mut seen = 0usize;
    for rel in paths {
        if stored.contains(rel.as_str()) {
            seen += 1;
            continue;
        }
        let name = rel.rsplit('/').next().unwrap_or(rel);
        if crate::manifests::is_manifest(name) || crate::extract::detect_language(Path::new(rel)).is_some() {
            return false;
        }
    }
    seen == stored.len()
}

pub(crate) fn ingest(root: &Path, reuse: Option<&Reuse>, listing: Option<&Listing>) -> Result<Ingested> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut files: Vec<Walked> = Vec::new();
    let mut read: Vec<String> = Vec::new();
    let mut non_utf8_paths: Vec<String> = Vec::new();
    let mut manifests = Vec::new();
    let mut lang_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    // Every file path the walk visits (relative, /-normalized) — the path
    // evidence class for stack inference. Includes non-source files, since
    // layout markers are often not source code.
    let mut walk_paths: Vec<String> = Vec::new();

    // Coverage accounting.
    let mut top_code: BTreeMap<String, usize> = BTreeMap::new();
    let mut top_other: BTreeMap<String, usize> = BTreeMap::new();
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    let mut non_utf8 = 0usize;
    let skipped = Arc::new(Mutex::new(Vec::new()));

    for dent in walker(&root, listing, Arc::clone(&skipped)).flatten() {
        let path = dent.path();
        if !path.is_file() {
            continue;
        }
        let rel = relative(&root, path);
        let fname = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let topdir = match rel.split_once('/') {
            Some((d, _)) => d.to_string(),
            None => "(root)".to_string(),
        };
        walk_paths.push(rel.clone());

        // Manifest? Detection + dep/script parsing is data-driven (manifests.toml).
        // An unchanged manifest the previous map parsed is taken as it was.
        // O manifesto escrito na própria língua do projeto (um roteiro de
        // instalação) marca o projeto e segue lido como código.
        let language = crate::extract::detect_language(path);
        let source = language.is_some();
        if crate::manifests::is_manifest(&fname) {
            if let Some(kept) = reuse.and_then(|r| r.kept_manifest(&rel)) {
                manifests.push(kept.clone());
                if !source {
                    *top_other.entry(topdir).or_default() += 1;
                    continue;
                }
            } else if let Ok(content) = fs::read_to_string(path)
                && let Some(p) = crate::manifests::parse(&rel, &fname, &content)
            {
                if !source {
                    read.push(rel.clone());
                }
                manifests.push(Manifest {
                    path: rel.clone(),
                    kind: p.kind,
                    dependencies: p.deps,
                    scripts: p.scripts,
                    name: p.name,
                    module: p.module,
                    package: p.package,
                });
                if !source {
                    *top_other.entry(topdir).or_default() += 1;
                    continue;
                }
            }
        }

        // Source file? Language is detected from data (the tree-sitter language
        // registry), never a hardcoded extension map — see extract::detect_language.
        if let Some(lang) = language {
            // An unchanged file the previous map knows is taken as it was,
            // without opening it.
            if let Some(kept) = reuse.and_then(|r| r.kept_module(&rel)) {
                let entry = lang_counts.entry(kept.language.clone()).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += kept.loc;
                *top_code.entry(topdir).or_default() += 1;
                files.push(Walked::Kept(Box::new(kept.clone())));
                continue;
            }
            if reuse.is_some_and(|r| r.kept_undecodable(&rel)) {
                non_utf8 += 1;
                non_utf8_paths.push(rel);
                continue;
            }
            // Lido depois da caminhada, junto com os outros, em paralelo.
            files.push(Walked::Pending(Pending { rel, language: lang, path: path.to_path_buf(), topdir }));
        } else {
            // Seen but not mined: record its extension so the user can verify
            // nothing relevant was silently dropped.
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_lowercase()))
                .unwrap_or_else(|| "(no-ext)".to_string());
            *unsupported.entry(ext).or_default() += 1;
            *top_other.entry(topdir).or_default() += 1;
        }
    }

    // Os arquivos a ler, lidos todos de uma vez, e contados na ordem da
    // caminhada, como se tivessem sido lidos nela.
    let pending: Vec<Walked> = std::mem::take(&mut files);
    let contents = in_parallel(pending, |walked| match walked {
        Walked::Pending(file) => {
            let content = fs::read_to_string(&file.path).ok();
            (Walked::Pending(file), content)
        }
        other => (other, None),
    });
    for (walked, content) in contents {
        let Walked::Pending(file) = walked else {
            files.push(walked);
            continue;
        };
        let Some(content) = content else {
            non_utf8 += 1; // a code-extension file we couldn't decode
            non_utf8_paths.push(file.rel);
            continue;
        };
        read.push(file.rel.clone());
        let loc = content.lines().filter(|l| !l.trim().is_empty()).count();
        let entry = lang_counts.entry(file.language.clone()).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += loc;
        *top_code.entry(file.topdir).or_default() += 1;
        files.push(Walked::Fresh(SourceFile { rel_path: file.rel, language: file.language, loc, content }));
    }

    let mut languages: Vec<LanguageStat> = lang_counts
        .into_iter()
        .map(|(language, (files, loc))| LanguageStat { language, files, loc })
        .collect();
    languages.sort_by_key(|a| std::cmp::Reverse(a.loc));

    let frameworks = infer_frameworks(&manifests);
    // Sorted for stable input to the stack inference, whatever the
    // filesystem walk order.
    walk_paths.sort();

    let mut dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    dirs.extend(top_code.keys().cloned());
    dirs.extend(top_other.keys().cloned());
    let mut top_dirs: Vec<DirCoverage> = dirs
        .into_iter()
        .map(|d| DirCoverage {
            code_files: *top_code.get(&d).unwrap_or(&0),
            other_files: *top_other.get(&d).unwrap_or(&0),
            dir: d,
        })
        .collect();
    top_dirs.sort_by(|a, b| b.code_files.cmp(&a.code_files).then(a.dir.cmp(&b.dir)));

    let mut unsupported_exts: Vec<ExtCount> =
        unsupported.into_iter().map(|(ext, count)| ExtCount { ext, count }).collect();
    unsupported_exts.sort_by(|a, b| b.count.cmp(&a.count).then(a.ext.cmp(&b.ext)));

    let code_files_read = files.len();
    let coverage = Coverage {
        top_dirs,
        skipped_build_dirs: taken(&skipped),
        unsupported_exts,
        code_files_read,
        non_utf8_skipped: non_utf8,
    };
    read.sort();
    non_utf8_paths.sort();

    Ok(Ingested {
        root,
        files,
        manifests,
        languages,
        frameworks,
        walk_paths,
        coverage,
        read,
        non_utf8: non_utf8_paths,
    })
}

/// Map dependency names to framework labels. A framework strongly implies the
/// architecture the project is *expected* to follow.
///
/// Surface the dependencies the project declares — verbatim from its manifests,
/// most-common first, ties broken by first-appearance order. No curated catalog:
/// whatever the repo lists is what we report, so this stays agnostic to language
/// and framework. The ranking itself is the shared projection owned by
/// `crate::facts::rank_by_frequency`; this just feeds it the repo-wide deps.
fn infer_frameworks(manifests: &[Manifest]) -> Vec<String> {
    crate::facts::rank_by_frequency(manifests.iter().flat_map(|m| m.dependencies.iter()))
}
