// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! The `## Files` check answers about the project it was handed — and only
//! about the paths a plan really names.
//!
//! Three defects lived in one check: it re-derived the project root from the
//! process working directory (wrong tree in a worktree), its token scanner
//! dropped any path carrying routing punctuation (no verdict at all — silence
//! that reads like approval), and it warned about documentation prose that
//! merely looks like a path (a warning an operator learns to dismiss, and with
//! it the true one).
//!
//! Every test here is TWO-SIDED: something that exists must resolve AND
//! something genuinely absent must still warn. A one-sided assertion would
//! pass on a check that went silent, which is the failure this spec is about.
//!
//! Top-level `#[test]` fns on purpose: the acceptance criteria run
//! `cargo test -p mustard-rt <name> -- --exact`, and libtest matches `--exact`
//! against the FULL test path — a test nested in a module would report
//! `0 passed` and clear the gate without ever running.

use mustard_rt::commands::review::analyze_validation::validate;
use serde_json::json;
use std::path::Path;

/// The paths a validation run reports as `missing-file`.
fn missing_files(root: &Path, spec_md: &Path, body: &str) -> Vec<String> {
    validate(root, spec_md, body)
        .iter()
        .filter(|issue| issue["type"] == json!("missing-file"))
        .filter_map(|issue| issue["file"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn validation_resolves_from_any_working_directory() {
    // Cargo runs an integration test with the package root as the working
    // directory, so `Cargo.toml` exists THERE and nowhere under the temporary
    // project built below. That gap is what makes the last assertion a proof.
    assert!(
        Path::new("Cargo.toml").exists(),
        "precondition: tests run from a cargo package root",
    );

    let tmp = tempfile::tempdir().unwrap();
    // The project the validator is asked about...
    let root = tmp.path().join("project");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src").join("present.rs"), "// existing").unwrap();
    // ...and a spec that lives OUTSIDE it, so nothing resolves via the spec
    // directory and only the root the caller passed can answer.
    let spec_dir = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&spec_dir).unwrap();
    let spec_md = spec_dir.join("spec.md");

    let body = "# Spec\n\n## Files\n\
                - `src/present.rs`\n\
                - `src/absent.rs`\n\
                - `Cargo.toml`\n";
    let reported = missing_files(&root, &spec_md, body);

    assert!(
        !reported.contains(&"src/present.rs".to_string()),
        "a file under the passed root resolves, whatever the working directory is: {reported:?}",
    );
    assert!(
        reported.contains(&"src/absent.rs".to_string()),
        "a genuinely absent file still warns — the check must not go silent: {reported:?}",
    );
    assert!(
        reported.contains(&"Cargo.toml".to_string()),
        "a file that exists only in the WORKING DIRECTORY is still missing from the \
         project under validation — resolution must not fall back to `current_dir()`: {reported:?}",
    );
}

#[test]
fn validation_sees_paths_with_punctuated_segments() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // A route group and a dynamic segment: the parentheses and brackets are
    // part of the directory names a routing convention puts on disk.
    let group = root.join("app").join("(marketing)");
    let dynamic = root.join("app").join("[slug]");
    std::fs::create_dir_all(&group).unwrap();
    std::fs::create_dir_all(&dynamic).unwrap();
    std::fs::write(group.join("page.tsx"), "export default null;\n").unwrap();
    std::fs::write(dynamic.join("route.ts"), "export const GET = null;\n").unwrap();

    let spec_md = root.join("spec.md");
    let body = "# Spec\n\n## Files\n\
                - `app/(marketing)/page.tsx`\n\
                - `app/[slug]/route.ts`\n\
                - `app/(marketing)/absent.tsx`\n";
    let reported = missing_files(root, &spec_md, body);

    for existing in ["app/(marketing)/page.tsx", "app/[slug]/route.ts"] {
        assert!(
            !reported.contains(&existing.to_string()),
            "an existing punctuated path resolves: {existing} — {reported:?}",
        );
    }
    // The other side, and the one that catches the real defect: before the
    // token scanner learned this punctuation it dropped the whole token, so an
    // absent route file earned no verdict at all.
    assert!(
        reported.contains(&"app/(marketing)/absent.tsx".to_string()),
        "an absent punctuated path is validated, not skipped: {reported:?}",
    );
}

#[test]
fn validation_does_not_flag_prose_as_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let spec_md = root.join("spec.md");
    // Four prose shapes that read as paths to a character class: a template
    // placeholder, a documentation elision, a glob and a bare extension.
    let body = "# Spec\n\n## Files\n\
                - each wave writes `wave-N-{role}/spec.md`, so a nested `.../spec.md` is a shape\n\
                - the docs sweep covers `plugin/**/*.md`\n\
                - every `.tsx` under the app\n\
                - `src/ghost.rs`\n";
    let reported = missing_files(root, &spec_md, body);

    for prose in [
        "wave-N-{role}/spec.md",
        ".../spec.md",
        "plugin/**/*.md",
        ".tsx",
    ] {
        assert!(
            !reported.contains(&prose.to_string()),
            "prose is not a missing file: {prose} — {reported:?}",
        );
    }
    // Two-sided: a real path that is genuinely absent still warns, so the test
    // cannot pass by the check having gone quiet altogether.
    assert!(
        reported.contains(&"src/ghost.rs".to_string()),
        "a real absent path still warns: {reported:?}",
    );

    // ...and once that same path exists, the section validates clean — the
    // rule narrows what counts as a REFERENCE, not what counts as present.
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src").join("ghost.rs"), "// now it exists").unwrap();
    let after = missing_files(root, &spec_md, body);
    assert!(
        after.is_empty(),
        "with the one real path on disk nothing is missing: {after:?}",
    );
}
