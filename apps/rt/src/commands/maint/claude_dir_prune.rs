//! `mustard-rt run claude-dir-prune` — audit (and optionally remove) drift in
//! a project's `.claude/` directory.
//!
//! ## Why
//!
//! The `.claude/` folder accumulates artefacts across many Mustard versions:
//! retired hook payloads, abandoned metric writers, scratch directories from
//! the JS era. Each one survives as a phantom consumer — nothing in the live
//! `apps/{rt,cli,dashboard}` source references it, but the file walkers (vault
//! indexer, doctor, security-scan) still traverse them.
//!
//! This subcommand performs the `.claude/` audit the deep refactor asked for: it
//! enumerates every direct child of `.claude/`, classifies it against a
//! declared consumer list, and either reports candidates (default `--dry-run`)
//! or removes the ORPHAN / LEGACY ones (`--apply`).
//!
//! ## Classification
//!
//! | Class    | Meaning                                                                  |
//! |----------|--------------------------------------------------------------------------|
//! | `KEEP`   | Known, actively consumed by the live tree (spec/, skills/, graph/, ...). |
//! | `STALE`  | KEEP-listed but the most recent mtime exceeds the staleness window.      |
//! | `ORPHAN` | Not in the KEEP set and no live source references the dirname.          |
//! | `LEGACY` | Explicit dead-on-arrival names from prior Mustard versions.              |
//! | `CACHE`  | Top-level `.X.json` cache files (always KEEP-equivalent, never pruned).  |
//!
//! ## Output
//!
//! Byte-stable pretty JSON:
//!
//! ```json
//! {
//!   "scanned": 14,
//!   "entries": [
//!     {
//!       "path": ".claude/scripts",
//!       "classification": "LEGACY",
//!       "evidence": ["pre-Rust-monorepo JS payload, retired 2026-05-19"],
//!       "recommendation": "remove"
//!     }
//!   ],
//!   "removed": [],
//!   "errors": []
//! }
//! ```
//!
//! Exit code 0 always (fail-open). The recommendation surfaces in `entries[]`
//! so the user — or the SessionStart probe — can act on it.

use crate::shared::context::project_dir;
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Static classification tables
// ---------------------------------------------------------------------------

/// Directory basenames that are KNOWN to be consumed by the live source tree.
/// Cross-referenced manually against `apps/{rt,cli,dashboard}/src/` — any new
/// addition here must come with at least one consumer in code or doc.
const KEEP_DIRS: &[&str] = &[
    // Core knowledge surfaces (consumed by mustard-rt + Obsidian vault).
    "skills",
    "spec",
    "graph",
    "refs",
    "commands",
    "context",
    // Runtime state directories (declared in pipeline-config + session_start).
    ".harness",
    ".session",
    ".obsidian",
];

/// Directory basenames that are **documented ephemeral** state.
///
/// ONE source: the canonical catalog [`ClaudePaths::documented_dirs`] — every
/// top-level directory under `<root>/.claude/` the path catalog documents.
///
/// It used to be two. A hold-over set carried `worktrees`, which the catalog
/// did not document; the catalog carries it now, so the second source is gone
/// and cannot drift from the first. Adding a new ephemeral directory means
/// editing `ClaudePaths::documented_dirs` and nothing else — this auditor and
/// the private-install exclude rules both derive from it.
///
/// The set is computed at the first call and cached for the rest of the
/// process via `OnceLock`.
fn documented_dirs() -> &'static BTreeSet<&'static str> {
    static SET: std::sync::OnceLock<BTreeSet<&'static str>> = std::sync::OnceLock::new();
    SET.get_or_init(|| ClaudePaths::documented_dirs().into_iter().collect())
}

/// Directory basenames that are explicit retired-payloads from prior Mustard
/// versions. Always classified `LEGACY` and recommended for removal — every
/// one of these had its consumer code deleted in earlier waves.
const LEGACY_DIRS: &[&str] = &[
    "scripts",          // pre-Rust-monorepo JS payload, retired 2026-05-19
    "adapters",         // pre-mustard-rt adapter shims
    "agent-memory",     // superseded by SQLite memory_decisions/lessons
    ".agent-memory",    // dotfile variant of above
    "memory",           // superseded by per-spec memory/ inside spec dirs
    "metrics",          // superseded by .metrics/ + SQLite projections
    ".tmp",             // scratch tmp dir from JS scripts
];

/// Staleness window applied to KEEP_DIRS — anything older than this with no
/// recent activity gets reclassified `STALE`. Generous because skills + specs
/// can legitimately sit untouched for months between feature waves.
const KEEP_STALE_DAYS: u64 = 90;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// CLI options for `mustard-rt run claude-dir-prune`.
pub struct ClaudeDirPruneOpts {
    /// Project root override. Defaults to the current working directory.
    pub repo: Option<PathBuf>,
    /// Preview only — emit the report, mutate nothing (the default).
    pub apply: bool,
    /// Reserved for parity with sibling subcommands — JSON is the only output
    /// format today, but the flag exists so callers can pass it explicitly.
    pub json: bool,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
enum Classification {
    Keep,
    Stale,
    Orphan,
    Legacy,
    Cache,
}

impl Classification {
    fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "KEEP",
            Self::Stale => "STALE",
            Self::Orphan => "ORPHAN",
            Self::Legacy => "LEGACY",
            Self::Cache => "CACHE",
        }
    }
}

#[derive(Serialize)]
struct Entry {
    path: String,
    classification: &'static str,
    evidence: Vec<String>,
    /// `"keep"` | `"investigate"` | `"remove"`.
    recommendation: &'static str,
}

#[derive(Serialize)]
struct ErrorEntry {
    path: String,
    error: String,
}

#[derive(Serialize)]
struct Report {
    scanned: usize,
    entries: Vec<Entry>,
    removed: Vec<String>,
    errors: Vec<ErrorEntry>,
    dry_run: bool,
}

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

/// Dispatch `mustard-rt run claude-dir-prune [--repo p] [--apply] [--json]`.
pub fn run(opts: ClaudeDirPruneOpts) {
    let repo = opts
        .repo
        .clone()
        .unwrap_or_else(|| PathBuf::from(project_dir()));

    let report = audit_and_act(&repo, opts.apply);
    let _ = opts.json; // JSON is the only format; flag kept for parity.

    let out = serde_json::to_string_pretty(&json!({
        "scanned": report.scanned,
        "entries": report.entries,
        "removed": report.removed,
        "errors": report.errors,
        "dry_run": report.dry_run,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    println!("{out}");
}

// ---------------------------------------------------------------------------
// SessionStart helper
// ---------------------------------------------------------------------------

/// `SessionStart` advisory probe. Read-only — never mutates the filesystem,
/// never blocks. Emits a single `eprintln!` warning when one or more entries
/// classify as `ORPHAN`. Fail-open at every step.
pub fn check_orphans(repo: &Path) {
    let report = audit(repo);
    let orphan_count = report
        .entries
        .iter()
        .filter(|e| e.classification == Classification::Orphan.as_str())
        .count();
    if orphan_count > 0 {
        eprintln!(
            "[claude-dir-prune] {orphan_count} orphan path(s) in {}/.claude — \
             run `mustard-rt run claude-dir-prune` to inspect.",
            repo.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Audit core (testable; takes a path, returns a report — no IO side effects
// beyond reading the filesystem).
// ---------------------------------------------------------------------------

/// Perform the audit, optionally removing `ORPHAN` / `LEGACY` entries.
fn audit_and_act(repo: &Path, apply: bool) -> Report {
    let mut report = audit(repo);
    report.dry_run = !apply;

    if apply {
        for entry in &report.entries {
            if entry.recommendation != "remove" {
                continue;
            }
            let abs = repo.join(&entry.path);
            if !abs.exists() {
                continue;
            }
            let outcome = if abs.is_dir() {
                std::fs::remove_dir_all(&abs)
            } else {
                std::fs::remove_file(&abs)
            };
            match outcome {
                Ok(()) => report.removed.push(entry.path.clone()),
                Err(e) => report.errors.push(ErrorEntry {
                    path: entry.path.clone(),
                    error: format!("remove failed: {e}"),
                }),
            }
        }
    }

    report
}

/// Build the audit report for `<repo>/.claude/` without mutating anything.
/// Fail-open: a missing `.claude/` yields an empty report.
fn audit(repo: &Path) -> Report {
    let mut entries: Vec<Entry> = Vec::new();
    let errors: Vec<ErrorEntry> = Vec::new();
    let Ok(paths) = ClaudePaths::for_project(repo) else {
        return Report {
            scanned: 0,
            entries,
            removed: Vec::new(),
            errors,
            dry_run: true,
        };
    };
    let claude_root = paths.claude_dir();

    let Ok(read) = std::fs::read_dir(&claude_root) else {
        return Report {
            scanned: 0,
            entries,
            removed: Vec::new(),
            errors,
            dry_run: true,
        };
    };

    // Collect basenames first so we can sort for byte-stable output.
    let mut children: Vec<(String, PathBuf, bool)> = Vec::new();
    for de in read.flatten() {
        let name = match de.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let is_dir = de.file_type().is_ok_and(|t| t.is_dir());
        children.push((name, de.path(), is_dir));
    }
    children.sort_by(|a, b| a.0.cmp(&b.0));

    let scanned = children.len();

    for (name, path, is_dir) in &children {
        let (class, evidence) = classify(name, path, *is_dir, repo);
        let rel = format!(".claude/{name}");
        let recommendation = match class {
            Classification::Keep | Classification::Cache => "keep",
            Classification::Stale => "investigate",
            Classification::Orphan | Classification::Legacy => "remove",
        };
        entries.push(Entry {
            path: rel,
            classification: class.as_str(),
            evidence,
            recommendation,
        });
    }

    Report {
        scanned,
        entries,
        removed: Vec::new(),
        errors,
        dry_run: true,
    }
}

/// Classify a single `.claude/` child. The `is_dir` flag short-circuits some
/// branches: file children that don't match the cache-pattern are always
/// `ORPHAN` because every documented file artefact under `.claude/` is either
/// declared (cache `.X.json`) or lives inside a known subdirectory.
fn classify(
    name: &str,
    path: &Path,
    is_dir: bool,
    _repo: &Path,
) -> (Classification, Vec<String>) {
    // 1. Top-level `.X.json` cache files are always exempt.
    if !is_dir && name.starts_with('.') && name.ends_with(".json") {
        return (
            Classification::Cache,
            vec!["top-level dot-cache file".to_string()],
        );
    }

    // 2. CLAUDE.md / pipeline-config.md / grain.model.json / settings.json
    //    sit at the root of every installed `.claude/`. Treat them as KEEP
    //    (their consumers are declared in templates/ and in mustard-rt itself).
    //    `pipeline-config.md` is no longer generated there by init/update (the
    //    live rules live in the plugin), but the keep-list entry is PROTECTIVE:
    //    it stops the pruner from deleting a legacy or hand-authored copy in a
    //    downstream `.claude/`. Left intentionally.
    //
    //    Os cinco últimos são as OUTRAS saídas do scan, ao lado do
    //    `grain.model.json` com que esta lista nasceu. Faltavam, e a falta era
    //    destrutiva: sem entrada aqui um arquivo de raiz cai em ORPHAN com
    //    `recommendation: remove`, e a face `--remove` apaga. O `scan-map.md` é
    //    importado pelo `CLAUDE.md` de TODO subprojeto (`@.claude/scan-map.md`)
    //    e lido em uma dúzia de módulos; os `grain.*` alimentam o vocabulário
    //    injetado nos prompts. Apagá-los não degrada em silêncio — quebra o
    //    censo que orienta cada sessão. Medido em 07/09/2026.
    //
    //    A regra é da CLASSE, não do scan: todo arquivo que o produto escreve
    //    ou lê na raiz do `.claude/` entra aqui no mesmo commit que o cria. O
    //    podador não tem como descobrir sozinho quem escreve o quê, e o defeito
    //    fica dormente até alguém rodar a remoção. A primeira rodada consertou
    //    só as saídas do scan e a revisão achou logo em seguida o
    //    `grammars-suggestions.json`, que é override do operador — prova de que
    //    listar instâncias não fecha o buraco, e de que a lista é a única
    //    fronteira que existe hoje. Unificá-la com o `CACHE_FILES` de
    //    `claude_paths` (que cobre só `.claude/.cache/`) é unidade própria.
    let well_known_files: BTreeSet<&'static str> = [
        "CLAUDE.md",
        "pipeline-config.md",
        "settings.json",
        "settings.local.json",
        "mustard.json",
        ".docs-audit.json",
        ".gitignore",
        ".gitkeep",
        // Saídas do `scan`, todas na raiz do `.claude/`.
        "grain.model.json",
        "grain.dictionary.json",
        "grain.equivalences.json",
        "grain.equivalences.learned.json",
        "scan-map.md",
        "scan-declined.json",
        "feature-digest.json",
        // Override por projeto que o OPERADOR escreve, mesclado por
        // `install_grammars`. Não é saída de máquina — é trabalho de autor, e
        // apagá-lo perde o que nenhum comando sabe reconstruir.
        "grammars-suggestions.json",
    ]
    .iter()
    .copied()
    .collect();
    if !is_dir && well_known_files.contains(name) {
        return (
            Classification::Keep,
            vec!["declared root config file".to_string()],
        );
    }

    // 3. Legacy hit-list — explicit retired payloads from prior versions.
    if LEGACY_DIRS.contains(&name) {
        return (
            Classification::Legacy,
            vec![format!(
                "matches LEGACY_DIRS entry '{name}' — retired prior to current refactor"
            )],
        );
    }

    // 4. Documented ephemerals — derived from the canonical path catalog,
    //    which is the only source.
    if documented_dirs().contains(&name) {
        return (
            Classification::Keep,
            vec![format!("documented ephemeral '{name}'")],
        );
    }

    // 5. Known KEEP directories (consumed by the live source tree).
    if KEEP_DIRS.contains(&name) {
        let evidence = vec![format!(
            "declared consumer in apps/{{rt,cli,dashboard}} for '{name}'"
        )];
        // Apply staleness check on KEEP dirs.
        if is_dir
            && let Some(days) = newest_mtime_days(path)
                && days > KEEP_STALE_DAYS {
                    return (
                        Classification::Stale,
                        vec![format!(
                            "KEEP but newest mtime is {days}d old (>{KEEP_STALE_DAYS}d window)"
                        )],
                    );
                }
        return (Classification::Keep, evidence);
    }

    // 6. Everything else: ORPHAN — no declared consumer.
    (
        Classification::Orphan,
        vec![format!(
            "no entry in KEEP_DIRS / DOCUMENTED_DIRS / LEGACY_DIRS for '{name}'"
        )],
    )
}

/// Walk `dir` (one level) and return the newest mtime expressed in whole days
/// since now. `None` when the walk fails or the directory is empty.
fn newest_mtime_days(dir: &Path) -> Option<u64> {
    let entries = std::fs::read_dir(dir).ok()?;
    let now = std::time::SystemTime::now();
    let mut newest: Option<std::time::SystemTime> = None;
    for de in entries.flatten() {
        let Ok(meta) = de.metadata() else { continue };
        let Ok(m) = meta.modified() else { continue };
        if newest.as_ref().is_none_or(|cur| m > *cur) {
            newest = Some(m);
        }
    }
    let mtime = newest?;
    let delta = now.duration_since(mtime).ok()?;
    Some(delta.as_secs() / 86_400)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a fake `.claude/` with the given child basenames (dirs).
    fn fake_dirs(repo: &Path, names: &[&str]) {
        for n in names {
            fs::create_dir_all(repo.join(".claude").join(n)).unwrap();
        }
    }

    #[test]
    fn missing_claude_yields_empty_report() {
        let dir = tempdir().unwrap();
        let report = audit(dir.path());
        assert_eq!(report.scanned, 0);
        assert!(report.entries.is_empty());
    }

    #[test]
    fn legacy_dirs_are_classified_legacy() {
        let dir = tempdir().unwrap();
        fake_dirs(dir.path(), &["scripts", "adapters", "memory", "metrics"]);
        let report = audit(dir.path());
        let legacy: Vec<&Entry> = report
            .entries
            .iter()
            .filter(|e| e.classification == "LEGACY")
            .collect();
        assert_eq!(legacy.len(), 4, "all 4 legacy names hit");
        for e in &legacy {
            assert_eq!(e.recommendation, "remove");
        }
    }

    #[test]
    fn known_keep_dirs_classified_keep() {
        let dir = tempdir().unwrap();
        fake_dirs(dir.path(), &["skills", "spec", "graph"]);
        let report = audit(dir.path());
        for e in &report.entries {
            assert_eq!(e.classification, "KEEP", "{} should be KEEP", e.path);
            assert_eq!(e.recommendation, "keep");
        }
    }

    #[test]
    fn documented_ephemerals_are_keep() {
        let dir = tempdir().unwrap();
        // Four names the canonical catalog documents — `worktrees` among
        // them since it joined the catalog and stopped being held over
        // here. `.pipeline-states` and `.qa-reports` were retired from that catalog and
        // are correctly flagged as orphans now, so they are not exercised
        // here.
        fake_dirs(dir.path(), &["worktrees", ".cache", ".metrics", ".agent-state"]);
        let report = audit(dir.path());
        for e in &report.entries {
            assert_eq!(e.classification, "KEEP");
            assert!(e.evidence[0].contains("documented ephemeral"));
        }
    }

    #[test]
    fn unknown_dir_classified_orphan() {
        let dir = tempdir().unwrap();
        fake_dirs(dir.path(), &["weird-leftover"]);
        let report = audit(dir.path());
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].classification, "ORPHAN");
        assert_eq!(report.entries[0].recommendation, "remove");
    }

    #[test]
    fn dot_cache_json_files_classified_cache() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join(".cluster-cache.json"), "{}").unwrap();
        fs::write(claude.join(".resolve-cache.json"), "{}").unwrap();
        let report = audit(dir.path());
        let caches: Vec<&Entry> = report
            .entries
            .iter()
            .filter(|e| e.classification == "CACHE")
            .collect();
        assert_eq!(caches.len(), 2);
        for c in &caches {
            assert_eq!(c.recommendation, "keep");
        }
    }

    #[test]
    fn well_known_root_files_classified_keep() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("CLAUDE.md"), "# x").unwrap();
        fs::write(claude.join("settings.json"), "{}").unwrap();
        let report = audit(dir.path());
        for e in &report.entries {
            assert_eq!(e.classification, "KEEP");
        }
    }

    #[test]
    fn dry_run_does_not_remove() {
        let dir = tempdir().unwrap();
        fake_dirs(dir.path(), &["scripts"]);
        let report = audit_and_act(dir.path(), /* apply = */ false);
        assert!(report.removed.is_empty());
        assert!(dir.path().join(".claude/scripts").exists());
        assert!(report.dry_run);
    }

    #[test]
    fn apply_removes_legacy_and_orphan() {
        let dir = tempdir().unwrap();
        fake_dirs(dir.path(), &["scripts", "weird-leftover", "skills"]);
        let report = audit_and_act(dir.path(), /* apply = */ true);
        // scripts (LEGACY) + weird-leftover (ORPHAN) must be removed.
        assert!(report.removed.iter().any(|p| p.ends_with("scripts")));
        assert!(report.removed.iter().any(|p| p.ends_with("weird-leftover")));
        // skills (KEEP) must survive.
        assert!(dir.path().join(".claude/skills").exists());
        assert!(!dir.path().join(".claude/scripts").exists());
        assert!(!dir.path().join(".claude/weird-leftover").exists());
    }

    #[test]
    fn check_orphans_does_not_mutate() {
        let dir = tempdir().unwrap();
        fake_dirs(dir.path(), &["weird-leftover"]);
        check_orphans(dir.path()); // must not panic / mutate
        assert!(dir.path().join(".claude/weird-leftover").exists());
    }

    #[test]
    fn check_orphans_is_noop_when_dir_missing() {
        let dir = tempdir().unwrap();
        // No .claude at all — must not panic.
        check_orphans(dir.path());
    }

    #[test]
    fn report_json_shape_is_stable() {
        let dir = tempdir().unwrap();
        fake_dirs(dir.path(), &["scripts"]);
        let report = audit(dir.path());
        let value = serde_json::to_value(&report).unwrap();
        // The four named arrays/scalars the AC checks for must be present.
        assert!(value.get("scanned").is_some_and(|v| v.is_number()));
        assert!(value.get("entries").is_some_and(|v| v.is_array()));
        assert!(value.get("removed").is_some_and(|v| v.is_array()));
        assert!(value.get("errors").is_some_and(|v| v.is_array()));
        // Per-entry contract.
        let entries = value.get("entries").unwrap().as_array().unwrap();
        for e in entries {
            assert!(e.get("path").is_some_and(|v| v.is_string()));
            assert!(e.get("classification").is_some_and(|v| v.is_string()));
            assert!(e.get("evidence").is_some_and(|v| v.is_array()));
            assert!(e.get("recommendation").is_some_and(|v| v.is_string()));
        }
    }

    /// O que o próprio produto escreve na raiz do `.claude/` nunca é órfão.
    ///
    /// A lista `well_known_files` nasceu com `grain.model.json` e não seguiu o
    /// scan quando ele passou a escrever mais quatro arquivos ao lado dele. O
    /// resultado é um podador que, com `--remove`, apaga o censo que o produto
    /// inteiro lê: o `scan-map.md` é importado pelo `CLAUDE.md` de TODO
    /// subprojeto (`@.claude/scan-map.md`) e lido em uma dúzia de módulos; os
    /// `grain.*` alimentam o vocabulário injetado. Medido em 07/09/2026: os
    /// cinco saíam `ORPHAN` com `recommendation: remove`.
    ///
    /// O defeito é dormente — só morde em `--remove` — e é destrutivo quando
    /// morde, que é a combinação que um teste tem de travar.
    #[test]
    fn the_scans_own_outputs_are_never_orphans() {
        let dir = tempdir().unwrap();
        let owned = [
            "scan-map.md",
            "grain.dictionary.json",
            "grain.equivalences.json",
            "grain.equivalences.learned.json",
            "feature-digest.json",
            "scan-declined.json",
            "grammars-suggestions.json",
        ];
        let claude = dir.path().join(".claude");
        fs::create_dir_all(&claude).unwrap();
        for name in owned {
            fs::write(claude.join(name), "{}").unwrap();
        }
        fs::write(claude.join("lixo-de-alguem.md"), "x").unwrap();

        // Pelo `audit`, não pelo `classify`: o que poupa um arquivo do
        // `audit_and_act` é a RECOMENDAÇÃO, e classificar certo sem recomendar
        // certo pouparia o arquivo em teste e o apagaria em produção.
        let report = audit(dir.path());
        for name in owned {
            let e = report
                .entries
                .iter()
                .find(|e| e.path.ends_with(name))
                .unwrap_or_else(|| panic!("{name} ausente do relatório"));
            assert_eq!(
                e.recommendation, "keep",
                "`{name}` é escrito ou lido pelo produto — apagá-lo quebra o censo. Evidência: {:?}",
                e.evidence
            );
        }

        // Dois lados: um arquivo que o produto realmente não conhece continua
        // órfão, senão a asserção acima passaria com um podador que nunca poda.
        let junk = report
            .entries
            .iter()
            .find(|e| e.path.ends_with("lixo-de-alguem.md"))
            .expect("o arquivo desconhecido tem de aparecer no relatório");
        assert_eq!(junk.recommendation, "remove", "um arquivo desconhecido segue órfão");
    }
}
