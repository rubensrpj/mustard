//! `mustard-rt run spec-lint` — a standalone, ADVISORY structural linter for a
//! capability or spec document.
//!
//! It catches exactly the brittle-markdown failures this project keeps hitting:
//! an Acceptance-Criterion the [`qa_run`](crate::commands::review::qa_run)
//! parser cannot read, a `command:` line that is blank, a `[[..]]` reference
//! that resolves to nothing (the same `⚠ unresolved` the wikilink-footer
//! observer surfaces). It never blocks anything — it prints a byte-stable JSON
//! report and **always exits 0**. The owner may later wire it into `doctor`.
//!
//! ## What it reuses (no parallel parsers / scanners)
//!
//! - **Capability docs** — parsed via [`capability::parse`], the single
//!   capability parser; requirements/scenarios/links come straight off the
//!   resulting [`Capability`].
//! - **Spec docs** — the `## Acceptance Criteria` section is located with the
//!   i18n-aware [`extract_ac_section`] and parsed with [`parse_ac_items`], the
//!   exact reader `qa-run` executes, so the lint cannot drift from what QA does.
//! - **Wikilinks** — extracted with the single [`scan_links`] byte-scanner and
//!   resolved with [`wikilink::resolve`] against the canonical search dirs
//!   (`memory/`, `knowledge/`, `spec/`, `capabilities/`, `graph/`), identical to
//!   the wikilink-footer observer's set.
//!
//! ## Shape
//!
//! ```json
//! { "ok": true, "doc": "<path>", "issues": [ {"level":"error","rule":"<id>","message":"<text>"} ] }
//! ```
//!
//! `ok` is `true` when there are no `error`-level issues; `issues` are sorted
//! deterministically (by rule, then message) so the output is byte-stable.
//!
//! ## Purity
//!
//! The decision logic is a pair of **fold-pure** functions
//! ([`lint_capability`] / [`lint_spec`]) whose only inputs are the parsed
//! document and a `resolve: &dyn Fn(&str) -> bool` closure — no filesystem, no
//! globals — so they are unit-testable without IO, mirroring the projection
//! pattern. The IO (read the doc, build the resolver, print) lives in the thin
//! dispatch wrappers. Nothing here uses `unwrap`/`expect` outside `#[cfg(test)]`.

use crate::commands::capability;
use crate::commands::review::qa_run::{extract_ac_section, parse_ac_items};
use crate::shared::context::project_dir;
use mustard_core::domain::capability::Capability;
use mustard_core::io::atomic_md::{scan_links, wikilink};
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs as mfs;
use serde_json::json;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Issue — one lint finding
// ---------------------------------------------------------------------------

/// Severity of a lint [`Issue`]. `Error` participates in the `ok` verdict.
/// Serialised as the lowercase string `"error"` for a stable JSON contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Error,
}

impl Level {
    /// The stable JSON token for this level.
    fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
        }
    }
}

/// One structural finding: a severity, the rule id that produced it, and a
/// human-readable message. Pure data — built by the fold, rendered by dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Issue {
    level: Level,
    rule: &'static str,
    message: String,
}

impl Issue {
    fn error(rule: &'static str, message: impl Into<String>) -> Self {
        Issue { level: Level::Error, rule, message: message.into() }
    }

    /// Render to the stable JSON object shape.
    fn to_json(&self) -> serde_json::Value {
        json!({ "level": self.level.as_str(), "rule": self.rule, "message": self.message })
    }
}

/// Sort issues deterministically (by rule, then message) so the report is
/// byte-stable regardless of the order the fold discovered them.
fn sort_issues(issues: &mut [Issue]) {
    issues.sort_by(|a, b| a.rule.cmp(b.rule).then_with(|| a.message.cmp(&b.message)));
}

// ---------------------------------------------------------------------------
// Fold-pure lint cores
// ---------------------------------------------------------------------------

/// Lint a parsed [`Capability`] against the structural rules, using `resolve`
/// to decide whether each `[[..]]` link dereferences. Pure: no IO, no globals —
/// `resolve` is the only window onto the filesystem and is injected by the
/// caller (real resolver in production, a fixed set in tests).
///
/// Rules:
/// - `cap.id-missing` — the doc has no non-empty `id`.
/// - `cap.no-requirements` — the doc declares no requirement.
/// - `cap.requirement-without-scenario` — a requirement has zero scenarios.
/// - `cap.scenario-blank-command` — a scenario declares a `command:` that is
///   blank (the [`Capability`] parser drops a blank command, so this fires when
///   the scenario carries a `command` field that trims to empty — see the note
///   in [`scenario_has_blank_command`]).
/// - `cap.unresolved-link` — a covers/specs/related `[[..]]` does not resolve.
///
/// Returned issues are sorted (by rule, then message).
fn lint_capability(cap: &Capability, resolve: &dyn Fn(&str) -> bool) -> Vec<Issue> {
    let mut issues = Vec::new();

    // (4) non-empty id + at least one requirement.
    if cap.id.trim().is_empty() {
        issues.push(Issue::error("cap.id-missing", "capability has no `id`"));
    }
    if cap.requirements.is_empty() {
        issues.push(Issue::error(
            "cap.no-requirements",
            "capability declares no requirement",
        ));
    }

    for req in &cap.requirements {
        // (1) every requirement has >= 1 scenario.
        if req.scenarios.is_empty() {
            issues.push(Issue::error(
                "cap.requirement-without-scenario",
                format!("requirement has no scenario: \"{}\"", req.statement.trim()),
            ));
        }
        // (2) a scenario that declares a `command:` must carry a non-blank one.
        for sc in &req.scenarios {
            if scenario_has_blank_command(sc) {
                issues.push(Issue::error(
                    "cap.scenario-blank-command",
                    format!("scenario \"{}\" declares a blank command", sc.name.trim()),
                ));
            }
        }
    }

    // (3) every covers/specs/related link resolves.
    for id in cap.covers.iter().chain(&cap.specs).chain(&cap.related) {
        if !resolve(id) {
            issues.push(Issue::error(
                "cap.unresolved-link",
                format!("`[[{id}]]` does not resolve"),
            ));
        }
    }

    sort_issues(&mut issues);
    issues
}

/// Whether a [`Scenario`] carries a `command` field that is present but trims to
/// blank. The capability parser ([`capability::parse`]) only stores a `command`
/// when it is non-empty after stripping the inline-code backticks, so on a
/// parsed-from-markdown capability this is effectively always `false`. It is
/// kept as the rule's decision point so the fold expresses the intent directly
/// and a future producer (e.g. a hand-built `Capability` or a relaxed parser)
/// that lets a blank `Some("")` through is still caught here.
fn scenario_has_blank_command(sc: &mustard_core::domain::capability::Scenario) -> bool {
    sc.command.as_deref().is_some_and(|c| c.trim().is_empty())
}

/// The parsed view of a spec's `## Acceptance Criteria` that the spec lint folds
/// over — built by [`extract_spec_view`] so the fold stays IO-free.
struct SpecView {
    /// `true` when the `## Acceptance Criteria` section was found.
    has_ac_section: bool,
    /// One `(id, command)` per parsed AC item, in source order. `command` is the
    /// raw extracted command (may be blank only if a producer allowed it — the
    /// `qa-run` parser drops command-less headers, so a parsed item normally
    /// has a non-blank command; the rule still guards it explicitly).
    ac_items: Vec<(String, String)>,
    /// Every `[[..]]` token in the spec body, in source order (duplicates kept).
    links: Vec<String>,
}

/// Lint a [`SpecView`] against the (lean) spec rules, using `resolve` for link
/// dereference. Pure — no IO.
///
/// Rules:
/// - `spec.no-ac-section` — no `## Acceptance Criteria` section, OR the section
///   exists but `parse_ac_items` extracts zero ACs (the `**AC-N**`-format
///   fragility this linter exists to catch — qa-run would degrade to
///   `overall: skip`).
/// - `spec.ac-blank-command` — a parsed AC has a blank runnable command.
/// - `spec.unresolved-link` — a body `[[..]]` does not resolve.
///
/// Returned issues are sorted (by rule, then message).
fn lint_spec(view: &SpecView, resolve: &dyn Fn(&str) -> bool) -> Vec<Issue> {
    let mut issues = Vec::new();

    // (1) AC section exists AND yields >= 1 parseable AC.
    if !view.has_ac_section {
        issues.push(Issue::error(
            "spec.no-ac-section",
            "no `## Acceptance Criteria` section",
        ));
    } else if view.ac_items.is_empty() {
        issues.push(Issue::error(
            "spec.no-ac-section",
            "`## Acceptance Criteria` section has no parseable AC item",
        ));
    }

    // (2) every parsed AC has a non-blank runnable command.
    for (id, command) in &view.ac_items {
        if command.trim().is_empty() {
            issues.push(Issue::error(
                "spec.ac-blank-command",
                format!("{id} has a blank command"),
            ));
        }
    }

    // (3) every body `[[..]]` resolves. Dedup so a link repeated in the prose
    // is reported once; the report stays byte-stable after the final sort.
    let mut seen = std::collections::BTreeSet::new();
    for token in &view.links {
        if seen.insert(token.as_str()) && !resolve(token) {
            issues.push(Issue::error(
                "spec.unresolved-link",
                format!("`[[{token}]]` does not resolve"),
            ));
        }
    }

    sort_issues(&mut issues);
    issues
}

/// Build the IO-free [`SpecView`] from raw spec markdown, reusing the qa-run
/// section extractor + AC parser and the single `[[ ]]` scanner. Keeping this
/// separate from [`lint_spec`] is what lets the fold be unit-tested with a
/// hand-built view and no markdown at all.
fn extract_spec_view(markdown: &str) -> SpecView {
    let section = extract_ac_section(markdown);
    let ac_items = section
        .as_deref()
        .map(|s| {
            parse_ac_items(s)
                .iter()
                .map(|it| (it.id().to_string(), it.command().to_string()))
                .collect()
        })
        .unwrap_or_default();
    SpecView {
        has_ac_section: section.is_some(),
        ac_items,
        links: scan_links(markdown),
    }
}

// ---------------------------------------------------------------------------
// Resolver — the canonical search-dir set
// ---------------------------------------------------------------------------

/// The canonical wikilink search dirs for `project`, in resolution order:
/// `memory/`, `knowledge/`, `spec/`, `capabilities/`, `graph/`. Identical to
/// the wikilink-footer observer's set, so the lint and the observer agree on
/// what "resolves". Missing directories are tolerated by [`wikilink::resolve`].
fn search_dirs(project: &Path) -> Vec<PathBuf> {
    let Ok(paths) = ClaudePaths::for_project(project) else {
        return Vec::new();
    };
    let claude = paths.claude_dir();
    vec![
        claude.join("memory"),
        claude.join("knowledge"),
        paths.spec_dir(),
        paths.capabilities_dir(),
        paths.graph_dir(),
    ]
}

/// Build the `resolve(token) -> bool` closure over `project`'s canonical search
/// dirs by reusing [`wikilink::resolve`] (the single resolver). Owns the
/// `PathBuf` set so the returned closure can borrow it for its whole lifetime.
fn make_resolver(project: &Path) -> impl Fn(&str) -> bool {
    let dirs = search_dirs(project);
    move |token: &str| {
        let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
        wikilink::resolve(token, &refs).is_some()
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch `mustard-rt run spec-lint`. Exactly one of `--capability` / `--spec`
/// must be given; neither (or both) prints a usage-error JSON. ALWAYS exits 0.
pub fn run(capability_slug: Option<&str>, spec_slug: Option<&str>) {
    run_with_root(capability_slug, spec_slug, &PathBuf::from(project_dir()));
}

/// Same as [`run`] with an explicit project root, so tests inject a temp dir
/// without mutating the process environment.
fn run_with_root(capability_slug: Option<&str>, spec_slug: Option<&str>, project: &Path) {
    let cap = capability_slug.map(str::trim).filter(|s| !s.is_empty());
    let spec = spec_slug.map(str::trim).filter(|s| !s.is_empty());
    match (cap, spec) {
        (Some(slug), None) => lint_capability_doc(slug, project),
        (None, Some(slug)) => lint_spec_doc(slug, project),
        (None, None) => emit_usage("missing --capability or --spec"),
        (Some(_), Some(_)) => emit_usage("pass exactly one of --capability or --spec"),
    }
}

/// Read `.claude/capabilities/{slug}.md`, parse it, fold the capability rules,
/// and print the report. A missing doc is itself an `error`-level issue (so
/// `ok` is `false`) — still exit 0.
fn lint_capability_doc(slug: &str, project: &Path) {
    let dir = match ClaudePaths::for_project(project) {
        Ok(p) => p.capabilities_dir(),
        Err(e) => {
            emit_usage(&format!("invalid project path: {e}"));
            return;
        }
    };
    let target = dir.join(format!("{slug}.md"));
    let display = target.display().to_string();
    let Ok(md) = mfs::read_to_string(&target) else {
        emit_report(&display, &[Issue::error("doc.not-found", "capability doc not found")]);
        return;
    };
    let cap = capability::parse(&md);
    let resolve = make_resolver(project);
    let issues = lint_capability(&cap, &resolve);
    emit_report(&display, &issues);
}

/// Read `.claude/spec/{slug}/spec.md`, build the [`SpecView`], fold the spec
/// rules, and print the report. A missing doc is an `error`-level issue. Exit 0.
fn lint_spec_doc(slug: &str, project: &Path) {
    let target = match ClaudePaths::for_project(project).and_then(|p| p.for_spec(slug)) {
        Ok(sp) => sp.spec_md_path(),
        Err(e) => {
            emit_usage(&format!("invalid spec path: {e}"));
            return;
        }
    };
    let display = target.display().to_string();
    let Ok(md) = mfs::read_to_string(&target) else {
        emit_report(&display, &[Issue::error("doc.not-found", "spec.md not found")]);
        return;
    };
    let view = extract_spec_view(&md);
    let resolve = make_resolver(project);
    let issues = lint_spec(&view, &resolve);
    emit_report(&display, &issues);
}

/// Print the byte-stable report. `ok` is `true` when no issue is `error`-level.
/// Compact single-line JSON (fixed key order, no timestamps/volatile paths
/// beyond `doc`) so snapshot/gate comparisons are stable. ALWAYS exits 0
/// (the caller returns normally; `main` exits 0).
fn emit_report(doc: &str, issues: &[Issue]) {
    let ok = !issues.iter().any(|i| i.level == Level::Error);
    let report = json!({
        "ok": ok,
        "doc": doc,
        "issues": issues.iter().map(Issue::to_json).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string(&report).unwrap_or_else(|_| "{}".into()));
}

/// Print a usage-error JSON (no `doc`/`issues` — it never got that far) and
/// return. ALWAYS exit 0, like every `run` emitter.
fn emit_usage(message: &str) {
    let report = json!({ "ok": false, "error": "usage", "message": message });
    println!("{}", serde_json::to_string(&report).unwrap_or_else(|_| "{}".into()));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::capability::{Requirement, Scenario};

    /// A resolver that accepts a fixed allow-set of ids and rejects everything
    /// else — the IO-free stand-in for the real filesystem resolver.
    fn allow(ids: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |token: &str| ids.contains(&token)
    }

    fn req(statement: &str, scenarios: Vec<Scenario>) -> Requirement {
        Requirement { statement: statement.into(), scenarios }
    }

    fn scenario(name: &str, command: Option<&str>) -> Scenario {
        Scenario {
            name: name.into(),
            when: "x".into(),
            then: "y".into(),
            command: command.map(str::to_string),
        }
    }

    // --- capability fold ---------------------------------------------------

    /// A well-formed capability — every requirement has a scenario, commands are
    /// non-blank, and every link resolves — yields no issues.
    #[test]
    fn clean_capability_has_no_issues() {
        let cap = Capability {
            id: "cap.invoicing".into(),
            requirements: vec![req(
                "The system SHALL invoice.",
                vec![scenario("happy", Some("cargo test"))],
            )],
            covers: vec!["entity.Invoice".into()],
            specs: vec!["spec.invoicing".into()],
            related: vec!["cap.billing".into()],
            ..Capability::default()
        };
        let issues = lint_capability(
            &cap,
            &allow(&["entity.Invoice", "spec.invoicing", "cap.billing"]),
        );
        assert!(issues.is_empty(), "expected no issues, got {issues:?}");
    }

    /// The two expected issues: a requirement with no scenario AND an unresolved
    /// `[[cap.ghost]]` cover. (The id + a resolved sibling keep the other rules
    /// quiet.) Issues come back sorted by rule.
    #[test]
    fn capability_flags_requirement_without_scenario_and_unresolved_link() {
        let cap = Capability {
            id: "cap.invoicing".into(),
            requirements: vec![
                req("The system SHALL invoice.", vec![scenario("ok", Some("cargo test"))]),
                req("The system SHALL link entities.", vec![]), // no scenario
            ],
            covers: vec!["cap.ghost".into()], // does not resolve
            ..Capability::default()
        };
        let issues = lint_capability(&cap, &allow(&[]));
        assert_eq!(issues.len(), 2, "exactly two issues: {issues:?}");
        // Sorted by rule: `cap.requirement-without-scenario` < `cap.unresolved-link`.
        assert_eq!(issues[0].rule, "cap.requirement-without-scenario");
        assert!(issues[0].message.contains("link entities"));
        assert_eq!(issues[1].rule, "cap.unresolved-link");
        assert!(issues[1].message.contains("cap.ghost"));
        // Both are errors → not ok.
        assert!(issues.iter().all(|i| i.level == Level::Error));
    }

    /// Missing id + zero requirements both fire (the doc-level rules).
    #[test]
    fn capability_flags_missing_id_and_no_requirements() {
        let cap = Capability::default();
        let issues = lint_capability(&cap, &allow(&[]));
        let rules: Vec<&str> = issues.iter().map(|i| i.rule).collect();
        assert!(rules.contains(&"cap.id-missing"));
        assert!(rules.contains(&"cap.no-requirements"));
    }

    /// A scenario with a present-but-blank command is flagged (the rule's
    /// explicit guard, independent of whether the parser would have dropped it).
    #[test]
    fn capability_flags_blank_command_scenario() {
        let cap = Capability {
            id: "cap.x".into(),
            requirements: vec![req("R", vec![scenario("blank", Some("   "))])],
            ..Capability::default()
        };
        let issues = lint_capability(&cap, &allow(&[]));
        assert!(issues.iter().any(|i| i.rule == "cap.scenario-blank-command"));
    }

    // --- spec fold ---------------------------------------------------------

    /// A spec with a parseable AC carrying a command and a resolved link is
    /// clean.
    #[test]
    fn clean_spec_has_no_issues() {
        let view = SpecView {
            has_ac_section: true,
            ac_items: vec![("AC-1".into(), "cargo test".into())],
            links: vec!["spec.sibling".into()],
        };
        let issues = lint_spec(&view, &allow(&["spec.sibling"]));
        assert!(issues.is_empty(), "expected clean, got {issues:?}");
    }

    /// A parsed AC whose command is blank is the expected issue.
    #[test]
    fn spec_flags_ac_blank_command() {
        let view = SpecView {
            has_ac_section: true,
            ac_items: vec![("AC-1".into(), "  ".into())],
            links: vec![],
        };
        let issues = lint_spec(&view, &allow(&[]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].rule, "spec.ac-blank-command");
        assert!(issues[0].message.contains("AC-1"));
    }

    /// No AC section, and an AC section with zero parseable items, both produce
    /// the `spec.no-ac-section` rule (different messages).
    #[test]
    fn spec_flags_missing_and_empty_ac_section() {
        let none = lint_spec(
            &SpecView { has_ac_section: false, ac_items: vec![], links: vec![] },
            &allow(&[]),
        );
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].rule, "spec.no-ac-section");

        let empty = lint_spec(
            &SpecView { has_ac_section: true, ac_items: vec![], links: vec![] },
            &allow(&[]),
        );
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].rule, "spec.no-ac-section");
    }

    /// `extract_spec_view` reuses the qa-run parser: the canonical drafter
    /// `**AC-N**` multi-line form is recognised, and a body `[[..]]` is scanned.
    #[test]
    fn extract_spec_view_uses_qa_parser_and_scanner() {
        let md = "\
# Spec

Body links [[cap.x]] here.

## Acceptance Criteria

- **AC-1** — Workspace builds.
  Command: `cargo build`
- **AC-2** — Dangling header with no command.

## Files
- a.rs
";
        let view = extract_spec_view(md);
        assert!(view.has_ac_section);
        // Only AC-1 has a command → AC-2 is dropped by the qa-run parser.
        assert_eq!(view.ac_items.len(), 1);
        assert_eq!(view.ac_items[0].0, "AC-1");
        assert_eq!(view.ac_items[0].1, "cargo build");
        assert!(view.links.contains(&"cap.x".to_string()));
    }

    /// A duplicated unresolved link is reported once (dedup), keeping the report
    /// byte-stable.
    #[test]
    fn spec_dedups_repeated_unresolved_link() {
        let view = SpecView {
            has_ac_section: true,
            ac_items: vec![("AC-1".into(), "true".into())],
            links: vec!["cap.ghost".into(), "cap.ghost".into()],
        };
        let issues = lint_spec(&view, &allow(&[]));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].rule, "spec.unresolved-link");
    }

    // --- dispatch / IO -----------------------------------------------------

    /// `run_with_root` on a clean capability prints `"ok":true` with no issues,
    /// resolving a cover against a real graph node (frontmatter `id:`).
    #[test]
    fn dispatch_clean_capability_doc() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let caps = project.join(".claude").join("capabilities");
        let graph = project.join(".claude").join("graph");
        std::fs::create_dir_all(&caps).unwrap();
        std::fs::create_dir_all(&graph).unwrap();
        // A graph node so `[[entity.Order]]` resolves by frontmatter id.
        std::fs::write(
            graph.join("entity.Order.md"),
            "---\nid: entity.Order\n---\n# Order\n",
        )
        .unwrap();
        let cap = Capability {
            id: "cap.orders".into(),
            requirements: vec![req("R", vec![scenario("s", Some("cargo test"))])],
            covers: vec!["entity.Order".into()],
            ..Capability::default()
        };
        std::fs::write(caps.join("orders.md"), capability::render(&cap)).unwrap();

        // Just assert it does not panic and the fold is clean (the IO path is
        // exercised; stdout shape is covered by the fold tests).
        let parsed = capability::parse(&std::fs::read_to_string(caps.join("orders.md")).unwrap());
        let resolve = make_resolver(project);
        assert!(lint_capability(&parsed, &resolve).is_empty());
        // Smoke: dispatch must not panic.
        run_with_root(Some("orders"), None, project);
    }

    /// Missing capability doc → a `doc.not-found` error issue (ok:false), no
    /// panic, and dispatch returns (exit 0 happens in `main`).
    #[test]
    fn dispatch_missing_capability_is_doc_not_found() {
        let dir = tempfile::tempdir().unwrap();
        run_with_root(Some("ghost"), None, dir.path());
        // No assertion on stdout (println); the point is no panic + reuse path.
    }

    /// Neither flag → usage error; both flags → usage error. No panic.
    #[test]
    fn dispatch_usage_errors() {
        let dir = tempfile::tempdir().unwrap();
        run_with_root(None, None, dir.path());
        run_with_root(Some("a"), Some("b"), dir.path());
    }

    /// A spec missing its AC commands surfaces the expected issue end-to-end
    /// through the IO path (write spec.md, fold, observe `spec.ac-blank-command`
    /// is absent because the qa-run parser drops command-less ACs → instead the
    /// section is empty → `spec.no-ac-section`).
    #[test]
    fn dispatch_spec_missing_ac_commands() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let spec_dir = ClaudePaths::for_project(project)
            .unwrap()
            .for_spec("demo")
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::create_dir_all(&spec_dir).unwrap();
        // AC headers with NO command line → qa-run parser yields zero items.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Demo\n\n## Acceptance Criteria\n\n- **AC-1** — does a thing.\n- **AC-2** — does another.\n",
        )
        .unwrap();
        let md = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        let view = extract_spec_view(&md);
        let issues = lint_spec(&view, &make_resolver(project));
        // The fragility this linter exists to catch: section present, zero ACs.
        assert!(issues.iter().any(|i| i.rule == "spec.no-ac-section"));
        run_with_root(None, Some("demo"), project);
    }
}
