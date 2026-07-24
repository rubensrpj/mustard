// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! The drafted `## Context` is prose, and the validator enforces it.
//!
//! The shipped spec law (`plugin/refs/feature/spec-language.md`, "Contexto
//! rules") makes the PRD layer prose-only: Context briefs a human rediscovering
//! the work, so file paths, line numbers and bullet lists belong to Root cause /
//! Files / Tasks. The law shipped but nothing enforced it — and the drafter
//! itself broke it, splicing the scan digest's anchors into Context as a bullet
//! list of paths.
//!
//! Two halves, so the check cannot pass vacuously:
//!
//! 1. a freshly drafted skeleton validates with no `context-not-prose` issue;
//! 2. a Context carrying the forbidden shapes IS flagged, and the message names
//!    what it found.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt drafted_context_is_prose_only -- --exact`, and
//! libtest matches `--exact` against the FULL test path — which equals the bare
//! function name only at the root of an integration-test binary.

use mustard_rt::commands::review::analyze_validation::validate;
use mustard_rt::commands::spec::spec_draft::{run, SpecDraftOpts};
use serde_json::{json, Value};

/// The `context-not-prose` issue in a validation result, if any.
fn context_issue(issues: &[Value]) -> Option<&Value> {
    issues.iter().find(|i| i["type"] == json!("context-not-prose"))
}

#[test]
fn drafted_context_is_prose_only() {
    let tmp = tempfile::tempdir().unwrap();

    // --- 1. A freshly drafted skeleton is clean ---------------------------
    for (scope, lang, waves) in [("light", "en-US", 0), ("full", "pt-BR", 2)] {
        let out = tmp.path().join(format!("draft-{scope}-{lang}"));
        run(SpecDraftOpts {
            intent: "Keep the harness honest about what it measured".into(),
            scope: scope.into(),
            lang: lang.into(),
            signals: None,
            output: Some(out.clone()),
            waves,
            force: false,
            query_terms: None,
            force_scope: false,
        });

        let spec_md = out.join("spec.md");
        let body = std::fs::read_to_string(&spec_md)
            .unwrap_or_else(|e| panic!("{scope}/{lang}: draft not written: {e}"));
        let issues = validate(&spec_md, &body);
        assert!(
            context_issue(&issues).is_none(),
            "{scope}/{lang}: a virgin draft must carry a prose-only Context: {issues:?}\n{body}",
        );
    }

    // --- 2. The forbidden shapes ARE flagged ------------------------------
    // Without this half the assertion above would pass on a validator that
    // never looks — the exact defect this spec is about.
    let planted = tmp.path().join("planted.md");
    let body = "# Spec\n\n## Context\n\
                The reporting command answers from a directory nothing writes.\n\n\
                Anchors (from scan):\n\
                - apps/rt/src/commands/economy/metrics.rs (metrics, collect)\n\
                - apps/rt/src/shared/context.rs (spec)\n";
    std::fs::write(&planted, body).unwrap();
    let issues = validate(&planted, body);
    let issue = context_issue(&issues)
        .unwrap_or_else(|| panic!("a bullet list of paths in Context must be flagged: {issues:?}"));
    assert_eq!(issue["severity"], json!("WARN"), "advisory, never blocking");
    let message = issue["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("bullet list"),
        "the message names the bullet list it found: {message}",
    );
    assert!(
        message.contains("file path"),
        "the message names the file path it found: {message}",
    );

    // A Context that is genuinely prose — including prose that happens to carry
    // a version number and a URL — stays clean, so the check cannot be a
    // blanket "no dots in Context" rule.
    let prose = "# Spec\n\n## Context\n\
                 The switch silenced the safety net instead of the harness, and version 2.1 \
                 of the platform documents why at https://code.claude.com/docs.\n";
    std::fs::write(&planted, prose).unwrap();
    let issues = validate(&planted, prose);
    assert!(
        context_issue(&issues).is_none(),
        "plain prose with a version and a URL is not a path list: {issues:?}",
    );
}
