//! `source_lang` — map source file paths to programming languages, and decide
//! whether a target (a set of paths) is one the JS/TS-family gates can reason
//! about.
//!
//! ## Why this module exists
//!
//! Two Mustard gates were built around JS/TS conventions and emit GUARANTEED
//! noise when pointed at a non-JS/TS subproject:
//!
//! - **`dependency-precheck`** extracts symbols with a JSX/`import {}` scanner
//!   and greps for `export …` / `pub …` in `.ts/.tsx/.js/.jsx/.rs/.vue/.svelte`
//!   files ONLY. A C# spec never matches (there is no `.cs` in the walk and no
//!   C# `public class` in the needles), so every C# symbol is reported
//!   "missing" — and `List<Payable>` lexes as a `<Payable>` JSX tag, tagged
//!   `jsx`. The verdict is a false positive by construction.
//! - **`wave-size-check`** derives a per-wave `layerCount` from folder roles and
//!   flags `multi-layer` on any cross-layer wave. That signal is only meaningful
//!   where the role vocabulary was tuned (JS/TS); elsewhere it fires on every
//!   intrinsically cross-layer backend feature.
//!
//! This module is the SINGLE owner of the "what language is this target, and
//! can those gates reason about it?" decision, so both gates loosen
//! consistently instead of each re-deriving it (SRP + DRY).
//!
//! ## Signals
//!
//! The primary signal is the file EXTENSION — always present, unambiguous, and
//! exactly the unit the precheck's grep keys on. The repo model's detected
//! stacks (framework → registry [`StackDef::language`]) corroborate it so an
//! extension-less path set still resolves under a scanned project. Both are
//! fail-open: an unknown extension, a missing model, or a parse error
//! contributes nothing rather than a wrong language.

use std::collections::BTreeSet;
use std::path::Path;

use crate::domain::scan::read_projects;
use crate::domain::vocabulary::stacks::{StackRegistry, DEFAULT_STACKS_NAME};

/// Canonical `(extension, language)` table — DATA, not logic. Lowercase, no
/// dot. Unknown extensions resolve to `None` (agnostic floor): the gates must
/// under-claim rather than invent a language. Extended freely without touching
/// the decision logic below.
const EXT_LANG: &[(&str, &str)] = &[
    // JS / TS family — what the precheck extractor + grep understand.
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
    // Rust — greppable by the precheck needles, but the extractor never emits
    // Rust symbols, so a Rust-only spec has nothing to check (classed foreign).
    ("rs", "rust"),
    // Backends the gate cannot reason about — presence of any of these (with no
    // JS/TS alongside) is what makes a target "not understood".
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

/// Languages whose symbol/role conventions the JS/TS-family gates were built
/// for — the precheck's JSX/import extractor and export grep, and the role
/// vocabulary the wave-size audit leans on. A target with at least one of these
/// is "understood"; a target with only foreign languages is not.
pub(crate) const JS_TS_FAMILY: &[&str] = &["typescript", "javascript", "vue", "svelte"];

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

/// The distinct source languages a path set involves, by extension. Non-source
/// / unknown extensions contribute nothing.
#[must_use]
pub(crate) fn languages_of_paths(paths: &[String]) -> BTreeSet<String> {
    paths
        .iter()
        .filter_map(|p| language_of_path(p))
        .map(str::to_string)
        .collect()
}

/// The languages the repo model DETECTED for the projects enclosing `paths` —
/// each path attributed to the project whose `dir` is a path-prefix of it, that
/// project's `detected_stacks` mapped to a language via the stack registry
/// (override-aware). Fail-open: a missing model, no detection, or a registry
/// error yields an empty set (the extension signal then stands alone).
#[must_use]
pub(crate) fn detected_languages(paths: &[String], model_path: &Path, project_root: &Path) -> BTreeSet<String> {
    if paths.is_empty() {
        return BTreeSet::new();
    }
    let projects = read_projects(model_path);
    if projects.is_empty() {
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

/// The distinct languages a target involves — the union of the extension signal
/// ([`languages_of_paths`]) and the model's detected stacks
/// ([`detected_languages`]). The one entry point both gates call so their notion
/// of "the target language" is identical.
#[must_use]
pub fn resolve_target_languages(paths: &[String], model_path: &Path, project_root: &Path) -> BTreeSet<String> {
    let mut langs = languages_of_paths(paths);
    langs.extend(detected_languages(paths, model_path, project_root));
    langs
}

/// Whether the JS/TS-family gates can reason about this target: `true` when it
/// shows NO foreign-language evidence — either no recognised source language at
/// all (nothing to misread) or at least one [`JS_TS_FAMILY`] language present.
/// `false` only when the target is affirmatively foreign (one or more source
/// languages, none of them JS/TS-family) — the case where the gates must loosen.
#[must_use]
pub fn target_understood(langs: &BTreeSet<String>) -> bool {
    langs.is_empty() || langs.iter().any(|l| JS_TS_FAMILY.contains(&l.as_str()))
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
    fn languages_of_paths_collects_distinct() {
        let files = vec![
            "backend/App/DTOs/Payable.cs".to_string(),
            "backend/App/Services/Recur.cs".to_string(),
            "docs/notes.md".to_string(),
        ];
        let langs = languages_of_paths(&files);
        assert_eq!(langs, BTreeSet::from(["csharp".to_string()]));
    }

    #[test]
    fn csharp_only_target_is_not_understood() {
        let langs = languages_of_paths(&["backend/App/Payable.cs".to_string()]);
        assert!(!target_understood(&langs), "a C#-only target must not be understood");
    }

    #[test]
    fn js_ts_target_is_understood() {
        let langs = languages_of_paths(&["apps/web/src/Page.tsx".to_string()]);
        assert!(target_understood(&langs));
    }

    #[test]
    fn mixed_target_with_any_js_ts_is_understood() {
        // A spec touching both a C# file and a TSX file still has real imports
        // to check — run the gate.
        let langs = languages_of_paths(&[
            "backend/App/Payable.cs".to_string(),
            "apps/web/src/Page.tsx".to_string(),
        ]);
        assert!(target_understood(&langs));
    }

    #[test]
    fn no_recognised_source_is_understood_no_regression() {
        // Config/doc-only or extension-less paths carry no foreign evidence — the
        // gate keeps its historical behaviour rather than suppressing itself.
        let langs = languages_of_paths(&["docs/plan.md".to_string(), "config.json".to_string()]);
        assert!(langs.is_empty());
        assert!(target_understood(&langs));
    }

    #[test]
    fn rust_only_target_is_not_understood() {
        // The precheck extractor never emits Rust symbols, so a Rust-only spec
        // has nothing to check — classed foreign, the gate declines.
        let langs = languages_of_paths(&["apps/rt/src/commands/mod.rs".to_string()]);
        assert!(!target_understood(&langs));
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
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("grain.model.json");
        // A minimal grain model: one project under `backend/` detected as aspnet
        // (→ csharp in the built-in registry).
        fs::write(
            &model,
            r#"{"projects":[{"name":"api","dir":"backend","kind":"dotnet","detected_stacks":[{"name":"aspnet","confidence":0.65,"signals":["dep:Swashbuckle.AspNetCore"]}]}]}"#,
        )
        .unwrap();
        let files = vec!["backend/App/Controllers/PayableController.cs".to_string()];
        let langs = detected_languages(&files, &model, tmp.path());
        assert!(langs.contains("csharp"), "aspnet stack resolves to csharp: {langs:?}");
    }

    #[test]
    fn detected_languages_fail_open_without_model() {
        let tmp = tempfile::tempdir().unwrap();
        let files = vec!["backend/App/Payable.cs".to_string()];
        // No model on disk → empty (extension signal carries the decision).
        assert!(detected_languages(&files, &tmp.path().join("absent.json"), tmp.path()).is_empty());
    }
}
