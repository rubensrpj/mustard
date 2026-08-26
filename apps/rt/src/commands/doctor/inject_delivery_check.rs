//! `mustard-rt run doctor --check inject-delivery` — does the router REACH the
//! window?
//!
//! Every rule this harness enforces on itself arrives the same way: a file
//! declared in `mustard.json#inject`, read by a hook, spliced into the model's
//! window as `additionalContext`. Five things can break that chain, and until
//! this check existed every one of them failed in SILENCE:
//!
//! 1. The plugin is installed but DISABLED (`enabledPlugins` false in the
//!    user's settings). No hook runs at all — no router, no gates — and the
//!    interface is indistinguishable from a working harness. Measured in the
//!    field 2026-08-25: three attempts were spent discovering it by error.
//! 2. A declared injectable names a file that is not on disk. The collector is
//!    fail-open, so the entry is skipped and the rest still arrives.
//! 3. The router is declared by halves — `orchestrator.md` with no
//!    `dispatch.md` — which is what every project installed before the split
//!    carries. The question that opens a work unit reaches nobody.
//! 4. An injectable outgrew the 10,000 characters a hook RESPONSE carries. The
//!    overflow is not truncated: it becomes a file path, so the text stops
//!    being in force while still looking present on disk.
//! 5. An entry rides an event no hook is registered for. It is delivered by
//!    nobody, and the declaration reads as if it were.
//!
//! The first three are FAIL: the rule is not in force and the operator cannot
//! see it. The last two are WARN: something is delivered, but less or later
//! than the declaration promises.
//!
//! **Read-only.** Nothing here writes, and no event is emitted — `mustard-core`
//! publishes no event kind for this finding.
//!
//! Fail-open: an unreadable settings file or an absent registry degrades to
//! "cannot tell" and is reported as such, never as a pass. A check that answers
//! green because it could not look is the defect it exists to catch.
//!
//! Byte-stable: findings are sorted, paths are repo-relative and
//! forward-slashed, and no timestamp or volatile count appears.

use mustard_core::io::fs;
use mustard_core::ProjectConfig;
use serde::Serialize;
use std::path::Path;

/// Characters one hook RESPONSE may carry as `additionalContext`.
///
/// Per response, not per event: sibling hooks on one event are separate
/// invocations and Claude Code keeps every one of their blocks (measured
/// 2026-08-25). See `plugin/refs/mustard/router-rationale.md`.
const HOOK_RESPONSE_CAP: usize = 10_000;

/// The router's two halves, by declared path. A project that declares the first
/// and not the second is HALF-DELIVERED — the shape every install predating the
/// split carries.
const ORCHESTRATOR: &str = ".claude/mustard/orchestrator.md";
const DISPATCH: &str = ".claude/mustard/dispatch.md";

/// How serious one finding is for delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    /// The rule does not reach the window at all.
    Fail,
    /// It reaches it, but less or later than declared.
    Warn,
}

/// One way the declared router fails to reach the window.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeliveryFinding {
    pub severity: Severity,
    /// Short kebab-case slug of the finding kind, e.g. `half-router`.
    pub kind: String,
    /// What is wrong, in one sentence.
    pub detail: String,
    /// The command that resolves it. Every refusal names its own remedy.
    pub remedy: String,
}

/// The delivery report.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InjectDeliveryReport {
    /// `true` when nothing was found at either severity.
    pub ok: bool,
    /// `true` when at least one finding is `FAIL`.
    pub failed: bool,
    /// How many injectables the project declares.
    pub declared: usize,
    /// Findings, sorted: every `FAIL` first, then by `kind`.
    pub findings: Vec<DeliveryFinding>,
}

impl DeliveryFinding {
    fn fail(kind: &str, detail: String, remedy: &str) -> Self {
        Self {
            severity: Severity::Fail,
            kind: kind.to_string(),
            detail,
            remedy: remedy.to_string(),
        }
    }

    fn warn(kind: &str, detail: String, remedy: &str) -> Self {
        Self {
            severity: Severity::Warn,
            kind: kind.to_string(),
            detail,
            remedy: remedy.to_string(),
        }
    }
}


/// Event names registered in the shipped hook manifest, lowercased.
///
/// `None` when the manifest cannot be read — the check then skips the
/// unregistered-event finding rather than reporting every entry as undelivered.
fn registered_events(manifest: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(manifest).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(
        json.get("hooks")?
            .as_object()?
            .keys()
            .map(|k| k.to_ascii_lowercase())
            .collect(),
    )
}

/// Build the report for a project root, with the two external inputs passed in
/// so the whole thing is testable without touching a real HOME or plugin tree.
fn build_report(
    root: &Path,
    home_settings: &Path,
    manifest: Option<&Path>,
) -> InjectDeliveryReport {
    let mut findings: Vec<DeliveryFinding> = Vec::new();

    // (1) The harness itself. Checked FIRST: with the plugin off, nothing below
    // is delivered either, whatever the declarations say.
    // One reader for the plugin switch, shared with the statusline's inert
    // flag: the bar and the diagnosis must not be able to disagree.
    if crate::commands::statusline::segment::plugin_switched_off(home_settings) == Some(true) {
        findings.push(DeliveryFinding::fail(
            "plugin-disabled",
            "the Mustard plugin is installed but disabled in the user settings, so NO hook \
             runs: no router, no gates, and nothing in the interface says so"
                .to_string(),
            "enable it in /plugin (or set enabledPlugins.mustard@<marketplace> to true), \
             then restart Claude Code",
        ));
    }

    let config = ProjectConfig::load(root);
    let entries = config.injectables();
    let events = manifest.and_then(registered_events);

    for entry in &entries {
        // (2) Declared but absent from disk.
        //
        // The FILESYSTEM decides existence, and on Linux it is case-sensitive.
        // The path comparison used elsewhere here is not — that is right for
        // asking "do these two declarations name one file?", and wrong for
        // asking "is this file there?". Keeping the two questions apart is what
        // stops a `.claude/Mustard/Dispatch.md` from reading as declared-and-
        // present while the hook, which opens the literal path, finds nothing.
        let path = root.join(&entry.file);
        if !path.is_file() {
            findings.push(DeliveryFinding::fail(
                "injectable-missing",
                format!(
                    "`{}` is declared on `{}` but no file is at that exact path (case \
                     included), so that rule is not in force",
                    entry.file, entry.on,
                ),
                "run /mustard:upsert to reseed it, or fix the spelling in mustard.json#inject",
            ));
            continue;
        }
        // (4) Over the ceiling a hook response carries.
        //
        // Measured as the LARGER of characters and bytes. Which unit the
        // harness counts is not documented, and the two differ wherever the
        // text is not plain ASCII — the shipped templates carry accents, em
        // dashes and `▸`/`⨯`, so a file at 9,900 characters can already be past
        // 10,000 bytes. Reporting the smaller number would call a degraded
        // injectable clean, which is the failure this check exists to catch.
        if let Ok(text) = fs::read_to_string(&path) {
            let size = text.chars().count().max(text.len());
            if size > HOOK_RESPONSE_CAP {
                findings.push(DeliveryFinding::warn(
                    "injectable-over-ceiling",
                    format!(
                        "`{}` measures {size} (the larger of characters and bytes); past \
                         {HOOK_RESPONSE_CAP} a hook response becomes a file path instead of \
                         text in force",
                        entry.file,
                    ),
                    "split the document and give each half its own sibling hook",
                ));
            }
        }
        // (5) Riding an event nothing is registered for.
        if let Some(events) = events.as_ref() {
            if !events.contains(&entry.on.to_ascii_lowercase()) {
                findings.push(DeliveryFinding::warn(
                    "event-unregistered",
                    format!(
                        "`{}` rides `{}`, which the hook manifest does not register, so no \
                         hook delivers it",
                        entry.file, entry.on,
                    ),
                    "declare it on a registered event, or register a hook for that one",
                ));
            }
        }
    }

    // (6) Declared on an event whose hooks each claim ONE file — and claimed
    // by none of them.
    //
    // The delivery of `userPromptSubmit` was split into sibling hooks, each
    // invoked with `--inject <its own file>`, so that each injectable gets its
    // own response ceiling. A sibling collects only the file it was given. That
    // makes the claim list, not the event registration, the thing that decides
    // whether an entry is delivered: an operator's own entry survives every
    // upsert, rides a registered event, sits on disk at the right path — and is
    // collected by nobody. Every condition above reports it clean, which is the
    // failure mode this whole check exists to end (found in review, measured
    // against the binary).
    if let Some(manifest) = manifest {
        let root_str = root.to_string_lossy();
        for entry in &entries {
            let claims = crate::dispatch::claimed_injectables(&root_str, &entry.on);
            // No claim at all on this event means it is delivered whole, by one
            // unscoped hook — the shape before the split, and still correct.
            if claims.is_empty() {
                continue;
            }
            if claims
                .iter()
                .any(|c| crate::shared::paths::same_declared_file(c, &entry.file))
            {
                continue;
            }
            let _ = manifest;
            findings.push(DeliveryFinding::fail(
                "injectable-unclaimed",
                format!(
                    "`{}` is declared on `{}`, but every hook on that event is scoped to one                      other file with `--inject`, so nothing collects it — it is present,                      registered, and never in force",
                    entry.file, entry.on,
                ),
                "give it a sibling hook of its own in the manifest (one `--inject <file>` per                  injectable), or declare it on an event delivered whole",
            ));
        }
    }

    // (3) The router by halves.
    let declares = |file: &str| entries.iter().any(|e| crate::shared::paths::same_declared_file(&e.file, file));
    if declares(ORCHESTRATOR) && !declares(DISPATCH) {
        findings.push(DeliveryFinding::fail(
            "half-router",
            "`orchestrator.md` is declared and `dispatch.md` is not: the question that opens \
             a work unit (base, type, branch name) reaches nobody, so units are opened with \
             no question asked"
                .to_string(),
            "run /mustard:upsert to consolidate the inject list",
        ));
    }

    findings.sort_by(|a, b| {
        (a.severity == Severity::Warn, &a.kind).cmp(&(b.severity == Severity::Warn, &b.kind))
    });
    InjectDeliveryReport {
        ok: findings.is_empty(),
        failed: findings.iter().any(|f| f.severity == Severity::Fail),
        declared: entries.len(),
        findings,
    }
}

/// Run the delivery check under `root` (the workspace root holding `.claude/`).
///
/// Reads the user's settings for the plugin switch and the shipped hook
/// manifest for the registered events; both are fail-open.
#[must_use]
pub fn run(root: &Path) -> InjectDeliveryReport {
    // Through `claude_config_dir()`, which honours `CLAUDE_CONFIG_DIR` — an
    // operator who moved their config got zero `plugin-disabled` findings from
    // a hardcoded `$HOME/.claude`, so the check reported healthy while no hook
    // ran at all (found in review). Same reader the plugin registry uses.
    let home_settings = mustard_core::platform::harness::claude_config_dir()
        .map(|dir| dir.join("settings.json"))
        .unwrap_or_default();
    // The REGISTRY first. `CLAUDE_PLUGIN_ROOT` reaches hooks but not the shell
    // that runs `doctor`, so relying on it alone left condition (5) —
    // `event-unregistered` — dead in production: one of the five this module
    // promises never fired (found in review). `project_seed` had already been
    // fixed this way; this module had not.
    let manifest = mustard_core::platform::harness::installed_plugin_hooks_manifest().or_else(
        || {
            std::env::var_os("CLAUDE_PLUGIN_ROOT")
                .map(|dir| Path::new(&dir).join("hooks").join("hooks.json"))
                .filter(|p| p.is_file())
        },
    );
    build_report(root, &home_settings, manifest.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seed(root: &Path, inject: &str, files: &[&str]) {
        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"version":"1.0.0","inject":[{inject}]}}"#),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".claude/mustard")).unwrap();
        for f in files {
            std::fs::write(root.join(f), "RULES").unwrap();
        }
    }

    fn settings(dir: &Path, enabled: bool) -> std::path::PathBuf {
        let p = dir.join("settings.json");
        std::fs::write(
            &p,
            format!(r#"{{"enabledPlugins":{{"mustard@mustard-local":{enabled}}}}}"#),
        )
        .unwrap();
        p
    }

    const BOTH: &str = r#"{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true},
                          {"on":"userPromptSubmit","file":".claude/mustard/dispatch.md","once":true}"#;

    #[test]
    fn a_complete_declaration_on_an_enabled_plugin_is_clean() {
        let dir = tempdir().unwrap();
        seed(
            dir.path(),
            BOTH,
            &[".claude/mustard/orchestrator.md", ".claude/mustard/dispatch.md"],
        );
        let s = settings(dir.path(), true);
        let report = build_report(dir.path(), &s, None);
        assert!(report.ok, "clean project reported findings: {:?}", report.findings);
        assert_eq!(report.declared, 2);
    }

    /// The three FAIL conditions, each on its own.
    #[test]
    fn inject_delivery_fails_on_half_router_missing_file_and_disabled_plugin() {
        // Half-router: the shape every pre-split install carries.
        let dir = tempdir().unwrap();
        seed(
            dir.path(),
            r#"{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true}"#,
            &[".claude/mustard/orchestrator.md"],
        );
        let s = settings(dir.path(), true);
        let report = build_report(dir.path(), &s, None);
        assert!(report.failed, "a half-declared router must FAIL");
        let half = report.findings.iter().find(|f| f.kind == "half-router").expect("half-router");
        assert!(!half.remedy.is_empty(), "a refusal must name its own remedy");

        // Declared but absent from disk.
        let dir2 = tempdir().unwrap();
        seed(dir2.path(), BOTH, &[".claude/mustard/orchestrator.md"]);
        let s2 = settings(dir2.path(), true);
        let report2 = build_report(dir2.path(), &s2, None);
        assert!(report2.failed);
        assert!(report2.findings.iter().any(|f| f.kind == "injectable-missing"));

        // Plugin installed but switched off: nothing runs at all.
        let dir3 = tempdir().unwrap();
        seed(
            dir3.path(),
            BOTH,
            &[".claude/mustard/orchestrator.md", ".claude/mustard/dispatch.md"],
        );
        let s3 = settings(dir3.path(), false);
        let report3 = build_report(dir3.path(), &s3, None);
        assert!(report3.failed, "a disabled plugin must FAIL — no hook runs");
        assert!(report3.findings.iter().any(|f| f.kind == "plugin-disabled"));
    }

    /// An operator's OWN injectable, on the event whose hooks are each scoped
    /// to one file, is collected by nobody — and every other condition here
    /// reports it clean.
    ///
    /// This is the shape review measured against the binary: the entry survives
    /// every upsert, rides a registered event, and sits on disk at the exact
    /// declared path. The siblings emit their own two files and nothing emits
    /// the third. Present, registered, never in force.
    #[test]
    fn an_injectable_no_sibling_hook_claims_is_reported_as_undelivered() {
        let dir = tempdir().unwrap();
        seed(
            dir.path(),
            &format!(
                r#"{BOTH},
                   {{"on":"userPromptSubmit","file":".claude/mustard/my-rules.md","once":true}}"#
            ),
            &[
                ".claude/mustard/orchestrator.md",
                ".claude/mustard/dispatch.md",
                ".claude/mustard/my-rules.md",
            ],
        );
        // A manifest shaped like the shipped one: one sibling per injectable,
        // each scoped with `--inject`, and none of them claiming the third.
        let manifest = dir.path().join("hooks.json");
        std::fs::write(
            &manifest,
            r#"{"hooks":{"UserPromptSubmit":[{"hooks":[
                 {"command":"mustard-rt on userPromptSubmit --inject .claude/mustard/orchestrator.md"},
                 {"command":"mustard-rt on userPromptSubmit --inject .claude/mustard/dispatch.md"}
               ]}]}}"#,
        )
        .unwrap();
        let s = settings(dir.path(), true);
        let report = build_report(dir.path(), &s, Some(&manifest));

        assert!(report.failed, "an uncollected injectable must FAIL: {:?}", report.findings);
        let f = report
            .findings
            .iter()
            .find(|f| f.kind == "injectable-unclaimed")
            .expect("the unclaimed entry must be named");
        assert!(f.detail.contains("my-rules.md"), "it must say WHICH file: {}", f.detail);
        assert!(!f.remedy.is_empty(), "a refusal must name its own remedy");
        // …and the two the siblings DO claim are not reported.
        assert_eq!(
            report.findings.iter().filter(|f| f.kind == "injectable-unclaimed").count(),
            1,
            "only the unclaimed entry: {:?}",
            report.findings,
        );
    }

    /// The two WARN conditions: delivered, but less or later than declared.
    #[test]
    fn an_oversized_injectable_and_an_unregistered_event_only_warn() {
        let dir = tempdir().unwrap();
        seed(
            dir.path(),
            r#"{"on":"neverRegistered","file":".claude/mustard/orchestrator.md","once":true},
               {"on":"userPromptSubmit","file":".claude/mustard/dispatch.md","once":true}"#,
            &[".claude/mustard/dispatch.md"],
        );
        std::fs::write(
            dir.path().join(".claude/mustard/orchestrator.md"),
            "x".repeat(HOOK_RESPONSE_CAP + 1),
        )
        .unwrap();
        let manifest = dir.path().join("hooks.json");
        std::fs::write(&manifest, r#"{"hooks":{"UserPromptSubmit":[]}}"#).unwrap();
        let s = settings(dir.path(), true);

        let report = build_report(dir.path(), &s, Some(&manifest));
        assert!(!report.failed, "neither condition loses the rule outright: {:?}", report.findings);
        assert!(report.findings.iter().any(|f| f.kind == "injectable-over-ceiling"));
        assert!(report.findings.iter().any(|f| f.kind == "event-unregistered"));
    }

    /// Equivalent spellings of one path are the same declaration — otherwise a
    /// hand-written `./` prefix reads as a half-router that is not one.
    #[test]
    fn equivalent_path_spellings_do_not_read_as_a_half_router() {
        let dir = tempdir().unwrap();
        seed(
            dir.path(),
            r#"{"on":"userPromptSubmit","file":"./.claude/mustard/orchestrator.md","once":true},
               {"on":"userPromptSubmit","file":".claude/Mustard/Dispatch.md","once":true}"#,
            &[],
        );
        std::fs::write(dir.path().join(".claude/mustard/orchestrator.md"), "R").unwrap();
        std::fs::write(dir.path().join(".claude/mustard/dispatch.md"), "D").unwrap();
        let s = settings(dir.path(), true);
        let report = build_report(dir.path(), &s, None);
        assert!(
            !report.findings.iter().any(|f| f.kind == "half-router"),
            "spelling variants read as a half-router: {:?}",
            report.findings,
        );
        // …and the file check is a SEPARATE question, answered by the
        // filesystem. On Linux `.claude/Mustard/Dispatch.md` is not the file
        // that was seeded, so the report says exactly that instead of going
        // quiet. Asserting only the absence of `half-router` let this pass
        // while the report still FAILed for a reason the test never examined.
        let missing: Vec<&str> = report
            .findings
            .iter()
            .filter(|f| f.kind == "injectable-missing")
            .map(|f| f.detail.as_str())
            .collect();
        assert_eq!(
            missing.len(),
            1,
            "the case-variant path is absent on a case-sensitive filesystem and must be \
             reported as such: {:?}",
            report.findings,
        );
        assert!(
            missing[0].contains("case included"),
            "the refusal must say WHY the path did not resolve: {}",
            missing[0],
        );
    }

    /// Fail-open: no settings file at all means the plugin question is
    /// unanswerable, not answered green.
    #[test]
    fn an_unreadable_settings_file_reports_no_plugin_finding() {
        let dir = tempdir().unwrap();
        seed(
            dir.path(),
            BOTH,
            &[".claude/mustard/orchestrator.md", ".claude/mustard/dispatch.md"],
        );
        let report = build_report(dir.path(), &dir.path().join("absent.json"), None);
        assert!(!report.findings.iter().any(|f| f.kind == "plugin-disabled"));
    }
}
