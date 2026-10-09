//! `source_lang` — map source file paths to programming languages.
//!
//! This module is the SINGLE owner of the "what language is this path, and
//! what languages does a set of paths involve?" decision, so every caller asks
//! the same question the same way instead of each re-deriving it.
//!
//! ## Signals
//!
//! The primary signal is the file EXTENSION — always present and unambiguous.
//! The repo model's detected stacks (framework → registry
//! [`StackDef::language`]) corroborate it so an extension-less path set still
//! resolves under a scanned project. Both are fail-open: an unknown extension,
//! a missing model, or a parse error contributes nothing rather than a wrong
//! language.

use std::collections::BTreeSet;
use std::path::Path;

use crate::domain::scan::Project;
use crate::domain::vocabulary::stacks::{StackRegistry, DEFAULT_STACKS_NAME};

/// Canonical `(extension, language)` table — DATA, not logic. Lowercase, no
/// dot. Unknown extensions resolve to `None` (agnostic floor): callers must
/// under-claim rather than invent a language. Extended freely without touching
/// the decision logic below.
const EXT_LANG: &[(&str, &str)] = &[
    // JS / TS family.
    ("ts", "typescript"),
    ("tsx", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("js", "javascript"),
    ("jsx", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("vue", "vue"),
    ("svelte", "svelte"),
    ("rs", "rust"),
    // Backends and other languages.
    ("cs", "csharp"),
    ("py", "python"),
    ("go", "go"),
    ("java", "java"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("rb", "ruby"),
    ("php", "php"),
    ("swift", "swift"),
    ("scala", "scala"),
    ("dart", "dart"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("erl", "erlang"),
    ("hrl", "erlang"),
    ("hs", "haskell"),
    ("lua", "lua"),
    ("zig", "zig"),
    ("clj", "clojure"),
    ("cljs", "clojure"),
    ("fs", "fsharp"),
    ("fsx", "fsharp"),
    ("c", "c"),
    ("h", "c"),
    ("cpp", "cpp"),
    ("cc", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("hh", "cpp"),
];

/// The lowercase language for `path` by its extension, or `None` when the path
/// has no extension or an extension outside [`EXT_LANG`]. Tolerates both `/` and
/// `\` separators (tool targets arrive in both shapes on Windows) and any
/// trailing backtick left by a markdown bullet.
#[must_use]
pub(crate) fn language_of_path(path: &str) -> Option<&'static str> {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .trim_end_matches('`');
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() {
        // A dotfile like `.gitignore` has an empty stem — not a source language.
        return None;
    }
    let ext = ext.to_ascii_lowercase();
    EXT_LANG
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, lang)| *lang)
}

/// LSP document IDs for extension variants, separate from server selection.
pub(crate) fn lsp_language_of_path(path: &str) -> Option<&'static str> {
    const VARIANTS: &[(&str, &str)] = &[("tsx", "typescriptreact"), ("jsx", "javascriptreact")];
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    VARIANTS
        .iter()
        .find(|(name, _)| *name == ext)
        .map(|(_, id)| *id)
        .or_else(|| language_of_path(path))
}

/// The languages the repo model DETECTED for the projects enclosing `paths` —
/// each path attributed to the project whose `dir` is a path-prefix of it, that
/// project's `detected_stacks` mapped to a language via the stack registry
/// (override-aware). `projects` is the model already read, so this mapping never
/// runs the scan tool itself. Fail-open: no projects, no detection, or a
/// registry error yields an empty set (the extension signal then stands alone).
#[must_use]
pub(crate) fn detected_languages(paths: &[String], projects: &[Project], project_root: &Path) -> BTreeSet<String> {
    if paths.is_empty() || projects.is_empty() {
        return BTreeSet::new();
    }
    let Ok(registry) = StackRegistry::load(DEFAULT_STACKS_NAME, project_root) else {
        return BTreeSet::new();
    };

    let mut langs = BTreeSet::new();
    for path in paths {
        // The project whose dir is the longest path-prefix (most specific
        // enclosing unit) owns this path's stacks.
        let Some(project) = projects
            .iter()
            .filter(|p| !p.dir.is_empty() && path_has_prefix(path, &p.dir))
            .max_by_key(|p| p.dir.len())
        else {
            continue;
        };
        for stack in &project.detected_stacks {
            if let Some(lang) = registry.language_of(&stack.name) {
                langs.insert(lang.to_ascii_lowercase());
            }
        }
    }
    langs
}

/// `true` when `dir` is a path-prefix of `file` on SEGMENT boundaries:
/// `apps/api` is a prefix of `apps/api/x.cs` but not of `apps/apiv2/x.cs`.
/// Tolerant of `\` separators. An empty `dir` never matches (the repo root is
/// not a project attribution).
fn path_has_prefix(file: &str, dir: &str) -> bool {
    let file = file.replace('\\', "/");
    let dir = dir.replace('\\', "/");
    let dir = dir.trim_end_matches('/');
    if dir.is_empty() {
        return false;
    }
    file == dir || file.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scan::read_projects;

    #[test]
    fn language_of_path_maps_known_extensions() {
        assert_eq!(language_of_path("apps/dashboard/src/App.tsx"), Some("typescript"));
        assert_eq!(language_of_path("src/util.ts"), Some("typescript"));
        assert_eq!(language_of_path("backend/App/DTOs/Payable.cs"), Some("csharp"));
        assert_eq!(language_of_path("api/handler.go"), Some("go"));
        assert_eq!(language_of_path("apps/rt/src/main.rs"), Some("rust"));
        // Windows separators + a trailing backtick from a markdown bullet.
        assert_eq!(language_of_path("C:\\repo\\App\\Payable.cs`"), Some("csharp"));
    }

    #[test]
    fn language_of_path_unknown_and_dotfiles_are_none() {
        assert_eq!(language_of_path("README.md"), None);
        assert_eq!(language_of_path("Cargo.toml"), None);
        assert_eq!(language_of_path(".gitignore"), None);
        assert_eq!(language_of_path("LICENSE"), None);
        assert_eq!(language_of_path("data/output.snap"), None);
    }

    #[test]
    fn path_has_prefix_respects_segment_boundaries() {
        assert!(path_has_prefix("apps/api/x.cs", "apps/api"));
        assert!(path_has_prefix("apps/api/x.cs", "apps/api/"));
        assert!(!path_has_prefix("apps/apiv2/x.cs", "apps/api"));
        assert!(!path_has_prefix("apps/api/x.cs", ""));
        assert!(path_has_prefix("apps\\api\\x.cs", "apps/api"));
    }

    #[test]
    fn detected_languages_maps_stacks_to_language_via_registry() {
        let tmp = tempfile::tempdir().unwrap();
        // The model as the scan tool hands it over, already read: one project
        // under `backend/` detected as aspnet (→ csharp in the built-in
        // registry). Reading the model is the tool's own concern, tested there;
        // this proves only the path → project → stack → language mapping.
        let projects: Vec<Project> = serde_json::from_str(
            r#"[{"name":"api","dir":"backend","kind":"dotnet","detected_stacks":[{"name":"aspnet","confidence":0.65,"signals":["dep:Swashbuckle.AspNetCore"]}]}]"#,
        )
        .unwrap();
        let files = vec!["backend/App/Controllers/PayableController.cs".to_string()];
        let langs = detected_languages(&files, &projects, tmp.path());
        assert!(langs.contains("csharp"), "aspnet stack resolves to csharp: {langs:?}");
    }

    #[test]
    fn detected_languages_fail_open_without_model() {
        let tmp = tempfile::tempdir().unwrap();
        let files = vec!["backend/App/Payable.cs".to_string()];
        // No model on disk reads as no projects, without running the scan tool
        // → empty (extension signal carries the decision).
        let projects = read_projects(&tmp.path().join("absent.json"));
        assert!(detected_languages(&files, &projects, tmp.path()).is_empty());
    }
}
