//! `mustard install-grammars` — UX helper that suggests tree-sitter grammar
//! repos for the languages detected in a project.
//!
//! ## What this does
//!
//! Walks the target project, sniffs manifest files declared in the grammar
//! catalogue, derives a set of language ids, then prints a shell-ready
//! markdown block per language with the canonical tree-sitter repo and the
//! install command.
//!
//! ## What this does NOT do
//!
//! - **Never downloads, clones, or compiles** any grammar. The user copy-pastes
//!   the suggested commands. This is a non-goal of the parent spec
//!   (`2026-05-27-mustard-v4-foundation` § Não-Objetivos — "Linkar grammars
//!   individuais no binário Mustard — proibido sempre").
//! - **Never feeds the suggestion catalogue back into the regression gate.** The
//!   catalogue here is a *UX bookmark*; `mustard_core::domain::ast::GrammarLoader`
//!   discovers grammars from `~/.config/tree-sitter/config.json` at runtime,
//!   never from anything declared here.
//!
//! ## Catalogue lives in JSON, not Rust
//!
//! The grammar catalogue (lang_id → repo + install command + manifest signals)
//! ships as `apps/cli/templates/grammars-suggestions.json` — the file is the
//! single source of truth. The binary embeds it via [`EMBEDDED_CATALOG`] so
//! the helper works standalone, and looks for a per-project override at
//! `<project>/.claude/grammars-suggestions.json` first (so a teammate can add
//! a language entry without rebuilding Mustard).
//!
//! This keeps the binary agnostic in spirit: no language identifier appears
//! inside any `.rs` file under `apps/cli/src/`. Adding a new language is a
//! one-line edit to the JSON.

use anyhow::Result;
use mustard_core::domain::ast::GrammarLoader;
use mustard_core::platform::i18n::{self, Locale};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Flags accepted by `mustard install-grammars`.
#[derive(Debug, Default, Clone)]
pub struct InstallGrammarsArgs {
    /// Project root to scan. Defaults to the current working directory.
    pub project_root: Option<PathBuf>,
}

/// Embedded copy of `apps/cli/templates/grammars-suggestions.json`. The file
/// is the source of truth; the binary embeds it so the helper works
/// standalone. A per-project override at `<project>/.claude/grammars-suggestions.json`
/// supersedes the embedded copy when present.
const EMBEDDED_CATALOG: &str = include_str!("../../templates/grammars-suggestions.json");

/// Filename used for per-project overrides under `.claude/`.
const OVERRIDE_FILENAME: &str = "grammars-suggestions.json";

/// One row in the grammar catalogue. Mirrors a JSON entry verbatim — see
/// `apps/cli/templates/grammars-suggestions.json` for the canonical schema.
#[derive(Debug, Clone, Deserialize)]
struct GrammarEntry {
    /// Canonical tree-sitter language id (matches `tree_sitter_loader`'s
    /// `language_name`). Also the lookup key.
    lang_id: String,
    /// Human-facing heading (e.g. `"C#"` for `c_sharp`). Falls back to
    /// `lang_id` when missing.
    #[serde(default)]
    label: String,
    /// Canonical upstream repository (HTTPS).
    repo_url: String,
    /// Shell-ready install snippet (single line, paste-friendly).
    install_cmd: String,
    /// Manifest file patterns whose presence on disk identifies this
    /// language. `*` prefix means "any file ending with this suffix"
    /// (lightweight, no glob crate dependency).
    #[serde(default)]
    manifest_signals: Vec<String>,
    /// Sibling lang_ids to surface alongside this one. Used e.g. so
    /// `typescript` also lights up `javascript`.
    #[serde(default)]
    implies: Vec<String>,
}

/// Top-level catalogue document. The `_doc` field is documentation-only —
/// we ignore unknown fields to keep the JSON forward-compatible.
#[derive(Debug, Clone, Deserialize)]
struct GrammarsCatalog {
    #[serde(default)]
    grammars: Vec<GrammarEntry>,
}

impl GrammarsCatalog {
    /// Parse the embedded catalogue. Panics on malformed JSON — that is a
    /// compile-time problem (the JSON ships with the binary), not a runtime
    /// one. Caller is `load`.
    fn from_embedded() -> Self {
        serde_json::from_str(EMBEDDED_CATALOG)
            .expect("apps/cli/templates/grammars-suggestions.json is malformed at build time")
    }

    /// Load the catalogue. Always starts from the embedded source of truth and
    /// **merges** entries from `<project>/.claude/grammars-suggestions.json`
    /// on top when present (W8.5#1): an override entry whose `lang_id` matches
    /// an embedded one replaces only that row; new `lang_id`s are appended;
    /// embedded entries the override doesn't mention are kept. This lets users
    /// add ONE language or tweak ONE repo without re-stating the other nine.
    ///
    /// A malformed override file degrades silently to the embedded catalogue
    /// — never panic, never empty (per `feedback_no_stub_fail_open`).
    fn load(project_root: &Path) -> Self {
        let mut merged = Self::from_embedded();
        let override_path = project_root.join(".claude").join(OVERRIDE_FILENAME);
        if let Ok(text) = std::fs::read_to_string(&override_path) {
            if let Ok(over) = serde_json::from_str::<Self>(&text) {
                for entry in over.grammars {
                    if let Some(slot) = merged
                        .grammars
                        .iter_mut()
                        .find(|g| g.lang_id == entry.lang_id)
                    {
                        *slot = entry;
                    } else {
                        merged.grammars.push(entry);
                    }
                }
            }
        }
        merged
    }

    fn lookup(&self, lang_id: &str) -> Option<&GrammarEntry> {
        self.grammars.iter().find(|g| g.lang_id == lang_id)
    }
}

/// Detect every language id whose manifest signals are present under
/// `project_root` (depth ≤ 3). Walks subprojects the same way `sync-detect`
/// does so a monorepo with Rust + TypeScript + Python returns all three.
fn detect_languages(project_root: &Path, catalog: &GrammarsCatalog) -> Vec<String> {
    let mut detected: BTreeSet<String> = BTreeSet::new();
    walk_for_signals(project_root, 0, catalog, &mut detected);
    // Expand `implies` once we have the base set so the user sees every
    // grammar they likely need (typescript → javascript, etc.).
    let mut expanded = detected.clone();
    for lang in &detected {
        if let Some(entry) = catalog.lookup(lang) {
            for implied in &entry.implies {
                expanded.insert(implied.clone());
            }
        }
    }
    expanded.into_iter().collect()
}

/// Recursive walker bounded to depth 3 — mirrors `scan_for_subprojects` in
/// `sync_detect.rs` so monorepo detection stays consistent without pulling
/// in that module.
fn walk_for_signals(
    dir: &Path,
    depth: usize,
    catalog: &GrammarsCatalog,
    out: &mut BTreeSet<String>,
) {
    if depth > 3 {
        return;
    }
    for entry in &catalog.grammars {
        if entry
            .manifest_signals
            .iter()
            .any(|p| signal_present(dir, p))
        {
            out.insert(entry.lang_id.clone());
        }
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for child in entries.flatten() {
        let Ok(ft) = child.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = child.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || matches!(
                name.as_str(),
                "node_modules"
                    | "bin"
                    | "obj"
                    | "dist"
                    | "target"
                    | "_backup"
                    | "migrations"
                    | ".git"
            )
        {
            continue;
        }
        walk_for_signals(&child.path(), depth + 1, catalog, out);
    }
}

/// `true` if `pattern` matches an entry directly inside `dir` (supports a
/// leading `*` for suffix matching).
fn signal_present(dir: &Path, pattern: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix('*') {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        return entries.flatten().any(|e| {
            e.file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(&suffix.to_ascii_lowercase())
        });
    }
    dir.join(pattern).exists()
}

/// Probe the user's installed grammars via [`GrammarLoader`]. Fail-open: any
/// IO error returns an empty set so we degrade to "potentially not installed".
fn installed_languages(project_root: &Path) -> BTreeSet<String> {
    match GrammarLoader::from_project(project_root) {
        Ok(loader) => loader.available_languages().into_iter().collect(),
        Err(_) => BTreeSet::new(),
    }
}

/// Resolve the human label, falling back to the lang_id when none is set.
fn label_for(entry: &GrammarEntry) -> &str {
    if entry.label.is_empty() {
        entry.lang_id.as_str()
    } else {
        entry.label.as_str()
    }
}

/// Render a single suggestion block as shell-ready markdown.
fn render_suggestion_block(entry: &GrammarEntry, locale: Locale, is_installed: bool) -> String {
    let mut block = String::new();
    let marker = if is_installed {
        format!(
            " — ✓ {}",
            i18n::translate("cli.install_grammars.already_installed", locale)
        )
    } else {
        String::new()
    };
    block.push_str(&format!("### {}{marker}\n", label_for(entry)));
    block.push_str(&format!(
        "- {}: <{}>\n",
        i18n::translate("cli.install_grammars.repo_label", locale),
        entry.repo_url
    ));
    block.push_str(&format!(
        "- {}:\n",
        i18n::translate("cli.install_grammars.install_cmd_label", locale)
    ));
    block.push_str("```sh\n");
    block.push_str(&entry.install_cmd);
    block.push('\n');
    block.push_str("```\n");
    block
}

/// Render an unknown-language fallback block — explicit fail-open per
/// `feedback_no_stub_fail_open`.
fn render_unknown_block(lang_id: &str, locale: Locale) -> String {
    let template = i18n::translate("cli.install_grammars.unknown_lang_fallback", locale);
    let line = template.replace("{lang}", lang_id);
    format!("### {lang_id}\n- {line}\n")
}

/// Compose the full text output. Split out so the test suite can exercise
/// rendering without spawning the real loader or touching the user's
/// `~/.config/tree-sitter`.
fn render_output(
    langs: &[String],
    installed: &BTreeSet<String>,
    catalog: &GrammarsCatalog,
    locale: Locale,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {}\n\n",
        i18n::translate("cli.install_grammars.title", locale)
    ));
    out.push_str(i18n::translate("cli.install_grammars.lead", locale));
    out.push_str("\n\n");

    if langs.is_empty() {
        out.push_str(i18n::translate("cli.install_grammars.no_stack", locale));
        out.push('\n');
        return out;
    }

    for lang_id in langs {
        let is_installed = installed.contains(lang_id);
        match catalog.lookup(lang_id) {
            Some(entry) => {
                out.push_str(&render_suggestion_block(entry, locale, is_installed));
            }
            None => {
                out.push_str(&render_unknown_block(lang_id, locale));
            }
        }
        out.push('\n');
    }
    out.push_str(i18n::translate("cli.install_grammars.footer", locale));
    out.push('\n');
    out
}

/// Entry point — wired by `apps/cli/src/cli.rs`.
///
/// Resolves the project root, loads the catalogue (override or embedded),
/// detects languages, queries the loader for installed grammars, and prints
/// the rendered markdown to stdout. Returns `Ok(())` on every reachable path
/// — failures degrade to printed fallbacks (the helper is advisory; nothing
/// it does is load-bearing).
pub fn run(args: InstallGrammarsArgs) -> Result<()> {
    let project_root = match args.project_root {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    let catalog = GrammarsCatalog::load(&project_root);
    let locale = i18n::project_locale(&project_root);
    let langs = detect_languages(&project_root, &catalog);
    let installed = installed_languages(&project_root);
    let rendered = render_output(&langs, &installed, &catalog, locale);
    print!("{rendered}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn embedded_catalog() -> GrammarsCatalog {
        GrammarsCatalog::from_embedded()
    }

    /// AC-A-18 — catalogued language renders repo + install cmd; unknown
    /// renders fallback; installed marker appears when the loader reports
    /// the lang; pt-BR locale honoured; empty stack produces `no_stack`.
    #[test]
    fn test_known_languages_table_and_fallback() {
        let catalog = embedded_catalog();
        let empty: BTreeSet<String> = BTreeSet::new();

        // 1. Catalogued language (`rust`) — output carries repo_url + install_cmd.
        let rendered = render_output(&["rust".to_string()], &empty, &catalog, Locale::EnUs);
        assert!(
            rendered.contains("https://github.com/tree-sitter/tree-sitter-rust"),
            "expected rust repo URL in output, got:\n{rendered}"
        );
        assert!(
            rendered.contains("tree-sitter generate"),
            "expected install command in output, got:\n{rendered}"
        );
        assert!(
            rendered.contains("### Rust"),
            "expected human label heading in output, got:\n{rendered}"
        );
        assert!(
            !rendered.contains("already installed"),
            "should NOT show installed marker when loader is empty"
        );

        // 2. Unknown language (`brainfuck`) — fallback message, no panic.
        let rendered_unknown =
            render_output(&["brainfuck".to_string()], &empty, &catalog, Locale::EnUs);
        assert!(
            rendered_unknown.contains("not catalogued"),
            "expected unknown-language fallback, got:\n{rendered_unknown}"
        );
        assert!(
            rendered_unknown.contains("brainfuck"),
            "expected the unknown lang id verbatim, got:\n{rendered_unknown}"
        );

        // 3. Catalogued + installed — `✓ already installed` marker appears.
        let mut installed = BTreeSet::new();
        installed.insert("rust".to_string());
        let rendered_installed =
            render_output(&["rust".to_string()], &installed, &catalog, Locale::EnUs);
        assert!(
            rendered_installed.contains("already installed"),
            "expected installed marker, got:\n{rendered_installed}"
        );
        assert!(
            rendered_installed.contains("✓"),
            "expected ✓ marker, got:\n{rendered_installed}"
        );

        // 4. pt-BR locale renders the localised marker.
        let rendered_pt =
            render_output(&["rust".to_string()], &installed, &catalog, Locale::PtBr);
        assert!(
            rendered_pt.contains("já instalada"),
            "expected pt-BR installed marker, got:\n{rendered_pt}"
        );

        // 5. Empty stack — explicit `no_stack` message, no panic.
        let rendered_empty = render_output(&[], &empty, &catalog, Locale::EnUs);
        assert!(
            rendered_empty.contains("No language detected"),
            "expected no-stack fallback, got:\n{rendered_empty}"
        );
    }

    /// Sanity check: every entry in the embedded JSON catalogue is
    /// internally consistent.
    #[test]
    fn embedded_catalog_is_well_formed() {
        let catalog = embedded_catalog();
        assert!(
            !catalog.grammars.is_empty(),
            "embedded catalogue must not be empty"
        );
        for entry in &catalog.grammars {
            assert!(!entry.lang_id.is_empty(), "lang_id must not be empty");
            assert!(
                entry.repo_url.starts_with("https://"),
                "repo_url must be HTTPS for {}",
                entry.lang_id
            );
            assert!(
                entry.install_cmd.contains("tree-sitter generate"),
                "install_cmd must invoke tree-sitter generate for {}",
                entry.lang_id
            );
        }
    }

    /// `detect_languages` against a synthetic Rust project yields `rust`.
    #[test]
    fn detect_languages_finds_rust_via_cargo_toml() {
        let catalog = embedded_catalog();
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        let langs = detect_languages(tmp.path(), &catalog);
        assert!(
            langs.contains(&"rust".to_string()),
            "expected `rust` in detected langs, got {langs:?}"
        );
    }

    /// `detect_languages` against a mixed monorepo (Rust + Python) yields both.
    #[test]
    fn detect_languages_walks_monorepo() {
        let catalog = embedded_catalog();
        let tmp = tempdir().unwrap();
        let rust_dir = tmp.path().join("apps").join("rust-app");
        let py_dir = tmp.path().join("apps").join("py-app");
        fs::create_dir_all(&rust_dir).unwrap();
        fs::create_dir_all(&py_dir).unwrap();
        fs::write(rust_dir.join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        fs::write(py_dir.join("pyproject.toml"), "[project]\nname=\"y\"").unwrap();
        let langs = detect_languages(tmp.path(), &catalog);
        assert!(
            langs.contains(&"rust".to_string()),
            "missing rust in {langs:?}"
        );
        assert!(
            langs.contains(&"python".to_string()),
            "missing python in {langs:?}"
        );
    }

    /// TypeScript implies JavaScript via the `implies` field in the JSON
    /// catalogue — no implicit language knowledge in the Rust source.
    #[test]
    fn typescript_implies_javascript_via_catalog() {
        let catalog = embedded_catalog();
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("tsconfig.json"), "{}").unwrap();
        let langs = detect_languages(tmp.path(), &catalog);
        assert!(
            langs.contains(&"typescript".to_string()),
            "expected typescript in {langs:?}"
        );
        assert!(
            langs.contains(&"javascript".to_string()),
            "expected javascript via implies in {langs:?}"
        );
    }

    /// A per-project override at `.claude/grammars-suggestions.json` merges
    /// on top of the embedded catalogue: matching `lang_id`s are replaced,
    /// new `lang_id`s are appended, and embedded entries the override doesn't
    /// mention are kept (W8.5#1 — merge, not wholesale replace).
    #[test]
    fn per_project_override_merges_with_embedded() {
        let tmp = tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        // Override the rust entry with a fake repo + add a brand-new lang
        // (`brainfuck`) to prove both branches of the merge logic.
        let override_json = r#"{
            "version": 1,
            "grammars": [
                {
                    "lang_id": "rust",
                    "label": "Rust (override)",
                    "repo_url": "https://example.test/fake-rust",
                    "install_cmd": "echo override && tree-sitter generate",
                    "manifest_signals": ["Cargo.toml"],
                    "implies": []
                },
                {
                    "lang_id": "brainfuck",
                    "label": "Brainfuck",
                    "repo_url": "https://example.test/tree-sitter-brainfuck",
                    "install_cmd": "git clone https://example.test/tree-sitter-brainfuck && tree-sitter generate",
                    "manifest_signals": ["*.bf"],
                    "implies": []
                }
            ]
        }"#;
        fs::write(claude_dir.join("grammars-suggestions.json"), override_json).unwrap();
        let catalog = GrammarsCatalog::load(tmp.path());
        // Replaced entry — fields come from the override.
        let rust = catalog.lookup("rust").expect("rust entry from override");
        assert_eq!(rust.label, "Rust (override)");
        assert_eq!(rust.repo_url, "https://example.test/fake-rust");
        // Appended entry — brand-new lang_id surfaces.
        let bf = catalog.lookup("brainfuck").expect("brainfuck appended");
        assert_eq!(bf.label, "Brainfuck");
        // Untouched embedded entry — Python is kept verbatim from the
        // embedded catalogue (this is the regression W8.5#1 guards).
        assert!(
            catalog.lookup("python").is_some(),
            "embedded entries the override doesn't mention must be preserved"
        );
    }

    /// A malformed override falls back to the embedded catalogue rather
    /// than panicking — the helper is advisory, never load-bearing.
    #[test]
    fn malformed_override_falls_back_to_embedded() {
        let tmp = tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("grammars-suggestions.json"),
            "{ not valid json",
        )
        .unwrap();
        let catalog = GrammarsCatalog::load(tmp.path());
        assert!(
            catalog.lookup("rust").is_some(),
            "embedded catalogue should be the fallback when override is malformed"
        );
    }
}
