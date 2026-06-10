//! Source-vs-support classification for tool-target paths.
//!
//! `digest-adherence-finalize` measures how much an ANALYZE session read
//! project SOURCE directly instead of going through the scan digest. Reading
//! config, docs or lockfiles directly is legitimate — only code files count
//! against adherence. The decision is purely lexical (extension allowlist +
//! anchor file names): no IO, deterministic, language-data kept in two flat
//! tables.
//!
//! Unknown extensions classify as NON-source on purpose — the metric must
//! under-count rather than flag a `.csv` or `.snap` read as a source read.

/// Extensions that mark a path as project source code. Lowercase, no dot.
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "py", "go", "rb", "php", "java",
    "kt", "kts", "cs", "c", "h", "cpp", "cc", "hh", "hpp", "swift", "scala", "svelte", "vue",
    "ex", "exs", "erl", "hrl", "hs", "lua", "dart", "zig", "clj", "cljs", "fs", "fsx", "sh",
];

/// Extensions that are always support files (config / docs / locks) — listed
/// explicitly for intent even though the allowlist default already rejects
/// them, so a future addition to [`SOURCE_EXTENSIONS`] cannot silently absorb
/// one of these.
const SUPPORT_EXTENSIONS: &[&str] =
    &["md", "markdown", "json", "jsonc", "lock", "toml", "yaml", "yml", "txt"];

/// Well-known anchor file names that are never source, independent of any
/// extension rule. Lowercase.
const SUPPORT_FILE_NAMES: &[&str] = &[
    "cargo.toml",
    "cargo.lock",
    "mustard.json",
    "package.json",
    "pnpm-lock.yaml",
    "package-lock.json",
    "yarn.lock",
    "claude.md",
];

/// Classify `path` as source code (`true`) or support file (`false`).
///
/// Decision order: anchor file name → support extension → source-extension
/// allowlist. Anything else — no extension, dotfiles, unknown extensions,
/// directories — is non-source. Mixed `/` and `\` separators are tolerated
/// (tool targets arrive in both shapes on Windows).
#[must_use]
pub fn is_source_file(path: &str) -> bool {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    if SUPPORT_FILE_NAMES.contains(&name.as_str()) {
        return false;
    }
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    // A dotfile like `.gitignore` splits into an empty stem — never source.
    if stem.is_empty() || SUPPORT_EXTENSIONS.contains(&ext) {
        return false;
    }
    SOURCE_EXTENSIONS.contains(&ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_extensions_classify_as_source() {
        for p in [
            "apps/rt/src/main.rs",
            "src/components/App.tsx",
            "C:\\repo\\lib\\util.ts",
            "scripts/build.py",
            "cmd/server/main.go",
            "web/Component.svelte",
            "app/Models/User.php",
        ] {
            assert!(is_source_file(p), "{p} must classify as source");
        }
    }

    #[test]
    fn config_doc_and_lock_files_classify_as_support() {
        for p in [
            "README.md",
            "docs/guide.markdown",
            ".claude/grain.model.json",
            "Cargo.toml",
            "Cargo.lock",
            "mustard.json",
            "pnpm-lock.yaml",
            "ci/pipeline.yml",
            "notes.txt",
        ] {
            assert!(!is_source_file(p), "{p} must classify as support");
        }
    }

    #[test]
    fn unknown_extension_no_extension_and_dotfiles_are_not_source() {
        assert!(!is_source_file("LICENSE"));
        assert!(!is_source_file(".gitignore"));
        assert!(!is_source_file("apps/rt/src")); // directory-looking target
        assert!(!is_source_file("data/output.snap"));
        assert!(!is_source_file(""));
    }

    #[test]
    fn anchor_names_win_regardless_of_directory_and_case() {
        assert!(!is_source_file("apps/rt/Cargo.toml"));
        assert!(!is_source_file("C:\\Atiz\\mustard\\MUSTARD.JSON"));
        assert!(!is_source_file("apps/dashboard/package.json"));
    }
}
