//! Layer 0 — Ingestion.
//!
//! Walk the tree (respecting .gitignore via the `ignore` crate), detect file
//! languages, count LOC, parse build manifests, and infer frameworks from
//! dependencies. Manifests are the cheapest, highest-signal fingerprint there
//! is: they reveal language + framework + deps without parsing a line of code.

use crate::model::{Coverage, DirCoverage, ExtCount, LanguageStat, Manifest};
use anyhow::Result;
use ignore::WalkBuilder;
use mustard_core::domain::vocabulary::stacks::{infer_stacks, StackDetection};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub struct Ingested {
    pub root: PathBuf,
    pub source_files: Vec<SourceFile>,
    pub manifests: Vec<Manifest>,
    pub languages: Vec<LanguageStat>,
    pub frameworks: Vec<String>,
    /// Stacks inferred from the walk's evidence (manifest deps + file paths +
    /// source contents) by the registry-driven engine in `mustard-core`.
    pub detected_stacks: Vec<StackDetection>,
    /// Every file path the walk visited (relative, /-normalized, sorted) — the
    /// path evidence class, kept so later stages can slice it per unit and run
    /// the same inference on a unit's own evidence.
    pub walk_paths: Vec<String>,
    /// A unit's own module path, if a manifest declares one (import resolution).
    pub go_module: Option<String>,
    pub coverage: Coverage,
}

pub struct SourceFile {
    pub rel_path: String,
    pub language: String,
    pub loc: usize,
    pub content: String,
}

pub fn ingest(root: &Path) -> Result<Ingested> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut source_files = Vec::new();
    let mut manifests = Vec::new();
    let mut lang_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut go_module = None;
    // Every file path the walk visits (relative, /-normalized) — the path
    // evidence class for stack inference. Includes non-source files, since
    // layout markers are often not source code.
    let mut walk_paths: Vec<String> = Vec::new();

    // Coverage accounting.
    let mut top_code: BTreeMap<String, usize> = BTreeMap::new();
    let mut top_other: BTreeMap<String, usize> = BTreeMap::new();
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    let mut non_utf8 = 0usize;
    // Build/dependency directories to skip come from data (manifests.toml), not
    // a hardcoded list — see crate::manifests.
    let skip: Vec<String> = crate::manifests::skip_dirs().to_vec();
    // Which skip-dirs actually exist at the root (so we report only real skips).
    let mut skipped_build_dirs: Vec<String> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| skip.iter().any(|s| s == n))
        .collect();
    skipped_build_dirs.sort();

    let walk_skip = skip.clone();
    let walker = WalkBuilder::new(&root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .filter_entry(move |e| {
            let name = e.file_name().to_string_lossy();
            !walk_skip.iter().any(|s| s.as_str() == name.as_ref())
        })
        .build();

    for dent in walker.flatten() {
        let path = dent.path();
        if !path.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let fname = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let topdir = match rel.split_once('/') {
            Some((d, _)) => d.to_string(),
            None => "(root)".to_string(),
        };
        walk_paths.push(rel.clone());

        // Manifest? Detection + dep/script parsing is data-driven (manifests.toml).
        if crate::manifests::is_manifest(&fname) {
            if let Ok(content) = fs::read_to_string(path) {
                if let Some(p) = crate::manifests::parse(&rel, &fname, &content) {
                    if p.module.is_some() {
                        go_module = p.module;
                    }
                    manifests.push(Manifest {
                        path: rel,
                        kind: p.kind,
                        dependencies: p.deps,
                        scripts: p.scripts,
                        name: p.name,
                    });
                    *top_other.entry(topdir).or_default() += 1;
                    continue;
                }
            }
        }

        // Source file? Language is detected from data (the tree-sitter language
        // registry), never a hardcoded extension map — see extract::detect_language.
        if let Some(lang) = crate::extract::detect_language(path) {
            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => {
                    non_utf8 += 1; // a code-extension file we couldn't decode
                    continue;
                }
            };
            let loc = content.lines().filter(|l| !l.trim().is_empty()).count();
            let entry = lang_counts.entry(lang.clone()).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += loc;
            *top_code.entry(topdir).or_default() += 1;
            source_files.push(SourceFile {
                rel_path: rel,
                language: lang,
                loc,
                content,
            });
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

    let mut languages: Vec<LanguageStat> = lang_counts
        .into_iter()
        .map(|(language, (files, loc))| LanguageStat { language, files, loc })
        .collect();
    languages.sort_by(|a, b| b.loc.cmp(&a.loc));

    let frameworks = infer_frameworks(&manifests);

    // Stack inference: hand the engine the three evidence classes the walk
    // already produced — parsed dependency names, file paths, and the source
    // contents read above. The call is generic: which stacks exist and what
    // signals identify them is DATA in mustard-core's registry, never logic
    // here. The engine's output is deterministically ordered (confidence
    // desc, registry order); paths are sorted for stable input regardless of
    // filesystem walk order.
    //
    // Evidence under a conventional test/fixture tree is discounted from ALL
    // THREE classes — a committed fixture of another stack (its manifest's
    // deps included) describes what the project tests, not what it is. The
    // filter applies only to this inference's inputs: `manifests`,
    // `source_files` and `walk_paths` themselves stay complete for the miner.
    walk_paths.sort();
    let deps: Vec<String> = manifests
        .iter()
        .filter(|m| !under_test_dir(&m.path))
        .flat_map(|m| m.dependencies.iter().cloned())
        .collect();
    let evidence_paths: Vec<String> = walk_paths.iter().filter(|p| !under_test_dir(p)).cloned().collect();
    let contents: Vec<String> = source_files
        .iter()
        .filter(|s| !under_test_dir(&s.rel_path))
        .map(|s| s.content.clone())
        .collect();
    let detected_stacks = infer_stacks(&deps, &evidence_paths, &contents);

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

    let code_files_read = source_files.len();
    let coverage = Coverage {
        top_dirs,
        skipped_build_dirs,
        unsupported_exts,
        code_files_read,
        non_utf8_skipped: non_utf8,
    };

    Ok(Ingested {
        root,
        source_files,
        manifests,
        languages,
        frameworks,
        detected_stacks,
        walk_paths,
        go_module,
        coverage,
    })
}

/// Conventional test/fixture directory segments. DATA, not logic: the list
/// lives in `test-dirs.toml` next to `stopwords.toml` (embedded at compile
/// time, justified in its header) — tuning which trees count as test trees is
/// a data change, never a code change. Parsed once per process; a malformed
/// embedded file is a programmer error caught by any test run, same contract
/// as `digest::stopwords`.
fn test_dir_segments() -> &'static BTreeSet<String> {
    static SET: OnceLock<BTreeSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        let raw: toml::Value = include_str!("../test-dirs.toml").parse().expect("test-dirs.toml is not valid TOML");
        raw.get("segments")
            .and_then(|v| v.as_array())
            .expect("test-dirs.toml must contain a `segments` array")
            .iter()
            .map(|w| w.as_str().expect("each segment must be a string").to_lowercase())
            .collect()
    })
}

/// True when `rel` — a `/`-normalized path RELATIVE TO THE SCANNED ROOT — has
/// a directory component equal (ASCII case-insensitively) to a conventional
/// test/fixture segment. Component-boundary match only: `src/contest/x` never
/// matches `test`, and the trailing filename is not a directory segment.
/// Because the path is relative to the scanned root, scanning a fixture
/// directly as the root yields paths with no test segment — a root inside a
/// test tree is never self-suppressed.
pub(crate) fn under_test_dir(rel: &str) -> bool {
    let segments = test_dir_segments();
    let mut components = rel.split('/');
    components.next_back(); // drop the filename — only directories qualify
    components.any(|c| segments.contains(&c.to_ascii_lowercase()))
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
