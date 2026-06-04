//! `mustard-rt run wave-scaffold` — render the canonical SDD wave layout for
//! a spec from a declarative JSON plan.
//!
//! Part of the wave-network spec (`2026-05-20-mustard-wave-network-standard`).
//! The SKILL `/feature` generates the plan JSON during PLAN; this subcommand
//! materialises every wave-N spec file and the top-level `wave-plan.md` index.
//! `qa/` and `review/` are NOT scaffolded — they are event-driven phases;
//! `qa-run` / `review-result` create `qa/report.md` / `review/verdict.md` on
//! demand (each `create_dir_all`s its own folder).
//!
//! Plan shape (lenient — extra fields ignored):
//!
//! ```json
//! {
//!   "waves": [
//!     { "n": 1, "role": "general", "summary": "…", "depends_on": [] },
//!     { "n": 2, "role": "general", "summary": "…", "depends_on": ["wave-1-general"] }
//!   ],
//!   "total_waves": 2,
//!   "lang": "pt-BR"
//! }
//! ```
//!
//! `lang` accepts BCP-47 (`pt-BR` / `en-US`); the legacy short forms
//! (`pt` / `en`) are tolerated on read for back-compat with old plan JSON
//! and normalised to BCP-47 in the rendered headings.
//!
//! Idempotent: each output file is only created when absent. The stdout JSON
//! reports which were created vs skipped.

use mustard_core::io::fs;
use mustard_core::{Meta, MetaFlags, read_meta, write_meta};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// One wave entry inside the plan JSON.
///
/// `pub(crate)` so the EXECUTE-entry re-wave path
/// ([`crate::commands::wave::exec_rewave_check`]) can build the *same* entry
/// shape from its DAG output and render through the canonical renderers here —
/// rather than maintaining a second, divergent freeform renderer (F4-d item 2).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct WavePlanEntry {
    /// Wave number (1-based).
    pub(crate) n: u32,
    /// Role label (`general`, `frontend`, `backend`, …) — drives the folder
    /// name `wave-{n}-{role}`.
    pub(crate) role: String,
    /// Short one-line summary surfaced in `wave-plan.md` and the wave's
    /// `## Summary` heading.
    #[serde(default)]
    pub(crate) summary: String,
    /// Other wave names this wave depends on (e.g. `["wave-1-general"]`).
    /// Rendered in the wave-plan table's `Depends on` column and the wave
    /// spec's `## Network` section. `alias` accepts a hand-authored camelCase
    /// `dependsOn` — the tool's own producer emits snake_case, but humans/LLMs
    /// writing a plan.json reach for camelCase, and a bare `default` would
    /// silently drop it to an empty list (→ a "—" deps column).
    #[serde(default, alias = "dependsOn")]
    pub(crate) depends_on: Vec<String>,
}

/// Top-level plan shape.
///
/// `pub(crate)` for the same reason as [`WavePlanEntry`] — the re-wave path
/// constructs one of these from the dependency DAG and feeds it to
/// [`render_wave_plan`] / [`render_wave_spec`].
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Plan {
    pub(crate) waves: Vec<WavePlanEntry>,
    #[serde(default, alias = "totalWaves")]
    pub(crate) total_waves: Option<u32>,
    #[serde(default)]
    pub(crate) lang: Option<String>,
}

/// Heading strings for the wave layout.
///
/// These render **internal artefacts** — the operational `wave-plan.md` index,
/// the per-wave `spec.md` skeletons, the review/qa scaffolds. Per the i18n
/// rule (`feedback-mustard-i18n-agnostic`) every internal artefact is **EN by
/// default**, independent of the user's natural `language`: the `language`
/// only colours the user-facing `spec.md` PRD. The struct is retained (rather
/// than inlining the literals) so the re-wave path renders through the same
/// canonical renderers (F4-d item 2).
///
/// `pub(crate)` so the re-wave path can render through the same canonical
/// renderers (F4-d item 2).
pub(crate) struct Headings<'a> {
    wave_plan_title: &'a str,
    table_header: &'a str,
    table_sep: &'a str,
    network: &'a str,
    parent: &'a str,
    wave_table_caption: &'a str,
    /// `## Summary` heading for the per-wave spec skeleton.
    summary: &'a str,
    /// Placeholder body when a wave has no summary yet.
    summary_placeholder: &'a str,
    /// `Depends on` label for the wave spec's Network section.
    depends_on: &'a str,
}

/// The canonical EN heading set for the (internal) wave layout. The `lang`
/// the plan JSON carries is recorded in `meta.json#lang` (it describes the
/// spec-facing locale) but never localises these internal artefacts.
pub(crate) fn headings() -> Headings<'static> {
    Headings {
        wave_plan_title: "# Wave Plan",
        table_header: "| Wave | Spec | Role | Depends on | Summary |",
        table_sep: "|------|------|------|------------|---------|",
        network: "## Network",
        parent: "Parent",
        wave_table_caption: "## Wave Table",
        summary: "## Summary",
        summary_placeholder: "_(fill in)_",
        depends_on: "Depends on",
    }
}

/// Render the wave-plan markdown index. Lifecycle metadata (stage / scope /
/// total waves) lives only in the `meta.json` sidecar — the markdown is pure
/// narrative + the wave table.
pub(crate) fn render_wave_plan(plan: &Plan, hd: &Headings<'_>, ac_block: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str(hd.wave_plan_title);
    out.push_str("\n\n");
    out.push_str(hd.wave_table_caption);
    out.push_str("\n\n");
    out.push_str(hd.table_header);
    out.push('\n');
    out.push_str(hd.table_sep);
    out.push('\n');
    for w in &plan.waves {
        let name = wave_name(w);
        let deps = if w.depends_on.is_empty() {
            "—".to_string()
        } else {
            w.depends_on
                .iter()
                .map(|d| format!("[[{d}]]"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let summary = w.summary.replace('|', "\\|");
        let _ = writeln!(
            out,
            "| {n} | [[{name}]] | {role} | {deps} | {summary} |",
            n = w.n,
            role = w.role,
        );
    }
    // Carry the parent spec's `## Acceptance Criteria` verbatim so the QA gate,
    // which reads global ACs from `wave-plan.md` once the monolithic `spec.md`
    // is renamed to `spec.original.md`, still finds them. `None` (the /feature
    // scaffold path, where `spec.md` survives) leaves the output byte-stable.
    if let Some(ac) = ac_block {
        let ac = ac.trim();
        if !ac.is_empty() {
            out.push('\n');
            out.push_str(ac);
            out.push('\n');
        }
    }
    out
}

/// `wave-{n}-{role}` folder/spec name.
pub(crate) fn wave_name(w: &WavePlanEntry) -> String {
    format!("wave-{n}-{role}", n = w.n, role = w.role)
}

/// Render an individual wave's `spec.md` skeleton.
pub(crate) fn render_wave_spec(parent: &str, w: &WavePlanEntry, hd: &Headings<'_>) -> String {
    let name = wave_name(w);
    let mut out = String::new();
    let _ = writeln!(out, "# {name}\n");
    // Lifecycle metadata (stage / parent) lives only in the `meta.json` sidecar;
    // the parent is still surfaced as a body link in the `## Network` section.
    let _ = writeln!(out, "{}\n", hd.summary);
    if w.summary.is_empty() {
        let _ = writeln!(out, "{}\n", hd.summary_placeholder);
    } else {
        let _ = writeln!(out, "{}\n", w.summary);
    }
    out.push_str(hd.network);
    out.push_str("\n\n");
    let _ = writeln!(out, "- {p}: [[{parent}]]", p = hd.parent);
    if !w.depends_on.is_empty() {
        let deps: Vec<String> = w
            .depends_on
            .iter()
            .map(|d| format!("[[{d}]]"))
            .collect();
        let _ = writeln!(out, "- {dep}: {}", deps.join(", "), dep = hd.depends_on);
    }
    out
}

/// Write `content` to `path` only when the file does not already exist.
/// Returns `true` when the file was created, `false` when it was skipped.
fn write_if_absent(path: &Path, content: &str) -> bool {
    if fs::exists(path) {
        return false;
    }
    fs::write_atomic(path, content.as_bytes()).is_ok()
}

/// Run `mustard-rt run wave-scaffold --spec-dir <dir> --plan <json-file>`.
///
/// Idempotent and fail-open. Stdout is `{"created_files":[...],"skipped":[...]}`.
pub fn run(spec_dir_arg: Option<&str>, plan_arg: Option<&str>) {
    let Some(spec_dir_arg) = spec_dir_arg else {
        eprintln!("Usage: wave-scaffold --spec-dir <dir> --plan <json-file>");
        return;
    };
    let Some(plan_arg) = plan_arg else {
        eprintln!("Usage: wave-scaffold --spec-dir <dir> --plan <json-file>");
        return;
    };
    let spec_dir = if Path::new(spec_dir_arg).is_absolute() {
        PathBuf::from(spec_dir_arg)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(spec_dir_arg)
    };
    let plan_path = if Path::new(plan_arg).is_absolute() {
        PathBuf::from(plan_arg)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(plan_arg)
    };

    let raw = match fs::read_to_string(&plan_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[wave-scaffold] cannot read plan {}: {e}", plan_path.display());
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "created_files": [], "skipped": [] }))
                    .unwrap_or_else(|_| "{}".to_string())
            );
            return;
        }
    };
    let plan: Plan = match serde_json::from_str::<Plan>(&raw) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[wave-scaffold] plan JSON parse error: {e}");
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "created_files": [], "skipped": [] }))
                    .unwrap_or_else(|_| "{}".to_string())
            );
            return;
        }
    };

    // W10.T10.3 — hard gate: an empty plan is operator error, not "scaffold
    // nothing". Print to stderr and exit non-zero so the orchestrator notices.
    if plan.waves.is_empty() {
        eprintln!(
            "[wave-scaffold] ERROR: plan.waves is empty — nothing to scaffold ({})",
            plan_path.display()
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "created_files": [],
                "skipped": [],
                "error": "plan.waves is empty",
            }))
            .unwrap_or_else(|_| "{}".to_string())
        );
        std::process::exit(2);
    }
    // W10.T10.3 — mismatch is operator typo, not fatal: warn and continue
    // using the actual length so the table matches the directories on disk.
    if let Some(declared) = plan.total_waves {
        let actual = plan.waves.len() as u32;
        if declared != actual {
            eprintln!(
                "[wave-scaffold] WARN: plan.total_waves={declared} but waves.length={actual}; \
                 using {actual}",
            );
        }
    }

    let parent_name = spec_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // `lang` is recorded in meta.json (it is the spec-facing locale) but the
    // wave layout itself is an internal artefact rendered in EN.
    let lang = plan.lang.as_deref().unwrap_or("pt-BR");
    let hd = headings();

    let _ = fs::create_dir_all(&spec_dir);

    let mut created: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut emit = |path: &Path, body: String| {
        let rel = path
            .strip_prefix(&spec_dir)
            .map_or_else(
                |_| path.to_string_lossy().to_string(),
                |p| p.to_string_lossy().replace('\\', "/"),
            );
        if write_if_absent(path, &body) {
            created.push(rel);
        } else {
            skipped.push(rel);
        }
    };

    // wave-plan.md.
    let wave_plan_md = render_wave_plan(&plan, &hd, None);
    emit(&spec_dir.join("wave-plan.md"), wave_plan_md);

    // Per-wave spec.
    for w in &plan.waves {
        let dir = spec_dir.join(wave_name(w));
        emit(&dir.join("spec.md"), render_wave_spec(&parent_name, w, &hd));
    }

    // Wave 3 of mustard-unification: emit `meta.json` alongside every spec.md
    // we just wrote so consumers can read lifecycle metadata as structured
    // JSON instead of regexing the markdown. Fail-open per file.
    // `total_waves` is the count we ACTUALLY scaffold — one wave dir + one
    // `wave-plan.md` row per `plan.waves` entry. Derive it from `plan.waves.len()`,
    // NOT the declared `plan.total_waves` (only cross-checked / WARNed above): a
    // plan that declares a stale total must not poison the sidecar the dashboard
    // and `status` render the wave count from.
    let total_waves = plan.waves.len() as u32;
    write_parent_meta(
        &spec_dir,
        Meta {
            stage: Some("Plan".into()),
            outcome: Some("Active".into()),
            phase: None,
            scope: Some("full (wave plan)".into()),
            lang: Some(mustard_core::normalise_lang(lang)),
            checkpoint: None,
            parent: None,
            is_wave_plan: Some(true),
            total_waves: Some(total_waves),
            flags: MetaFlags::default(),
            raw: Value::Null,
        },
    );
    for w in &plan.waves {
        let wave_dir = spec_dir.join(wave_name(w));
        write_scaffold_meta(
            &wave_dir,
            Meta {
                stage: Some("Plan".into()),
                outcome: Some("Active".into()),
                phase: None,
                scope: None,
                lang: Some(mustard_core::normalise_lang(lang)),
                checkpoint: None,
                parent: Some(parent_name.clone()),
                is_wave_plan: None,
                total_waves: None,
                flags: MetaFlags::default(),
                raw: Value::Null,
            },
        );
    }
    // D3: `qa/` and `review/` are pipeline *phases*, not specs — they carry no
    // lifecycle, so no `meta.json` sidecar is written for them. Only the root
    // and each `wave-N` directory get a sidecar (above). The result of each
    // phase is materialised by code into `qa/report.md` / `review/verdict.md`
    // (D4), not tracked through a dead sidecar.

    let out: Value = json!({
        "created_files": created,
        "skipped": skipped,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
    );
}

/// Write a per-wave `meta.json` beside a scaffolded wave `spec.md`, only when
/// one is absent. Skip-if-absent so a hand/agent edit to a wave's lifecycle
/// survives a re-scaffold. (The PARENT root is reconciled instead — see
/// [`write_parent_meta`].) Fail-open: a write failure warns on stderr and never
/// panics.
fn write_scaffold_meta(dir: &Path, meta: Meta) {
    let path = dir.join("meta.json");
    if fs::exists(&path) {
        return;
    }
    let _ = fs::create_dir_all(dir);
    if let Err(e) = write_meta(&path, &meta) {
        eprintln!(
            "[wave-scaffold] WARN: could not write {} ({e})",
            path.display()
        );
    }
}

/// Write / reconcile the wave-plan PARENT `meta.json` (the wave-plan root).
///
/// Unlike the per-wave sidecars ([`write_scaffold_meta`], skip-if-absent), the
/// parent typically already exists: `spec-draft` creates it at PLAN time with an
/// *estimated* `total_waves` (the Full floor of ≥1, before the real plan is
/// known). `wave-scaffold` is the authoritative source of the real wave count,
/// so it must reconcile `total_waves` + `isWavePlan` onto whatever the pipeline
/// has advanced the file to — preserving every lifecycle field
/// (`stage` / `outcome` / `phase` / `scope` / `lang` / `checkpoint` / `flags` /
/// `raw`). Skipping (the old behaviour, inherited from `write_scaffold_meta`)
/// left a stale `totalWaves: 1` on multi-wave epics, mis-rendering the
/// dashboard / `status` wave count.
///
/// Fail-open: a write failure warns on stderr and never panics.
fn write_parent_meta(dir: &Path, fresh: Meta) {
    let path = dir.join("meta.json");
    let meta = match read_meta(&path) {
        // Reconcile ONLY the structural wave-plan fields; the lifecycle the
        // pipeline owns (and may have advanced past Plan) is preserved as-is.
        Some(mut existing) => {
            existing.is_wave_plan = fresh.is_wave_plan;
            existing.total_waves = fresh.total_waves;
            existing
        }
        // No draft pre-created it (standalone scaffold / migration) → write the
        // fresh wave-plan root verbatim.
        None => fresh,
    };
    let _ = fs::create_dir_all(dir);
    if let Err(e) = write_meta(&path, &meta) {
        eprintln!(
            "[wave-scaffold] WARN: could not write {} ({e})",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_plan() -> Plan {
        Plan {
            waves: vec![
                WavePlanEntry {
                    n: 1,
                    role: "general".to_string(),
                    summary: "foundations".to_string(),
                    depends_on: vec![],
                },
                WavePlanEntry {
                    n: 2,
                    role: "frontend".to_string(),
                    summary: "ui pieces".to_string(),
                    depends_on: vec!["wave-1-general".to_string()],
                },
            ],
            total_waves: Some(2),
            lang: Some("pt".to_string()),
        }
    }

    #[test]
    fn wave_plan_carries_acceptance_criteria_for_qa() {
        use crate::commands::spec::spec_sections;
        let hd = headings();
        let ac = "## Acceptance Criteria\n- **AC-1** — works.\n  Command: `true`";
        let md = render_wave_plan(&sample_plan(), &hd, Some(ac));
        // The QA gate reads global ACs back from `wave-plan.md` via the shared
        // `section_block` extractor once `spec.md` is renamed away — it must find
        // the carried section.
        let block = spec_sections::section_block(&md, "acceptanceCriteria")
            .expect("wave-plan must carry the AC section for the QA gate");
        assert!(block.contains("AC-1"));
        assert!(block.contains("Command: `true`"));

        // `None` (the /feature scaffold path, where `spec.md` survives) appends
        // no AC section — the table stays byte-identical.
        let bare = render_wave_plan(&sample_plan(), &hd, None);
        assert!(spec_sections::section_block(&bare, "acceptanceCriteria").is_none());
    }

    #[test]
    fn renders_wave_plan_table_with_wikilinks() {
        let hd = headings();
        let md = render_wave_plan(&sample_plan(), &hd, None);
        assert!(md.contains("[[wave-1-general]]"));
        assert!(md.contains("[[wave-2-frontend]]"));
        assert!(md.contains("foundations"));
        assert!(md.contains("[[wave-1-general]]"));
        // Internal artefact → EN headings regardless of the plan's `lang`.
        assert!(md.contains("# Wave Plan"));
        assert!(md.contains("Depends on"));
        assert!(!md.contains("Plano de Waves"));
        assert!(!md.contains("Depende de"));
    }

    #[test]
    fn renders_wave_spec_with_parent_link_and_no_header() {
        let hd = headings();
        let plan = sample_plan();
        let s1 = render_wave_spec("epic-x", &plan.waves[0], &hd);
        // Lifecycle metadata is NOT in the markdown — no `### Stage:`/`### Parent:`
        // header lines. The parent is surfaced only as a body link in `## Network`.
        assert!(!s1.contains("### Stage:"));
        assert!(!s1.contains("### Outcome:"));
        assert!(!s1.contains("### Parent:"));
        assert!(s1.contains("## Network"));
        assert!(s1.contains("[[epic-x]]"));
        // Internal artefact → EN summary heading, never PT.
        assert!(s1.contains("## Summary"));
        assert!(!s1.contains("## Resumo"));
        let s2 = render_wave_spec("epic-x", &plan.waves[1], &hd);
        assert!(!s2.contains("### Stage:"));
        assert!(s2.contains("[[wave-1-general]]"));
        assert!(s2.contains("## Network"));
        assert!(s2.contains("Depends on"));
    }

    #[test]
    fn creates_full_layout() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-x");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // Write plan JSON to a tempfile.
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "general", "summary": "foundations", "depends_on": [] },
                    { "n": 2, "role": "frontend", "summary": "ui", "depends_on": ["wave-1-general"] }
                ],
                "total_waves": 2,
                "lang": "pt"
            }))
            .unwrap(),
        )
        .unwrap();

        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );

        // 3 files for a 2-wave plan: wave-plan + 2× wave-N/spec.md. qa/ and
        // review/ are event-driven phases — NOT scaffolded here.
        assert!(spec_dir.join("wave-plan.md").exists());
        assert!(spec_dir.join("wave-1-general").join("spec.md").exists());
        assert!(spec_dir.join("wave-2-frontend").join("spec.md").exists());
        assert!(!spec_dir.join("review").join("spec.md").exists(), "review scaffold removed");
        assert!(!spec_dir.join("qa").join("spec.md").exists(), "qa scaffold removed");

        // Validate wave-1 spec content has the expected headings & wikilinks,
        // and that no lifecycle header leaked into the markdown.
        let s1 =
            std::fs::read_to_string(spec_dir.join("wave-1-general").join("spec.md")).unwrap();
        assert!(!s1.contains("### Stage:"));
        assert!(!s1.contains("### Parent:"));
        assert!(s1.contains("[[epic-x]]"));
        assert!(s1.contains("## Network"));
        // meta.json carries the lifecycle metadata instead.
        assert!(spec_dir.join("wave-1-general").join("meta.json").exists());
        // Root + each wave carry a meta.json sidecar.
        assert!(spec_dir.join("meta.json").exists());

        // Second run is idempotent — no overwrites, no errors.
        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );
        // File still exists, still has draft content (not overwritten).
        let s1_again =
            std::fs::read_to_string(spec_dir.join("wave-1-general").join("spec.md")).unwrap();
        assert_eq!(s1, s1_again);
    }

    /// Regression: a hand-authored plan.json using camelCase `dependsOn` /
    /// `totalWaves` must NOT be silently dropped. The wave-plan "Depends on"
    /// column must render the dependency wikilink, not "—". Feeds camelCase
    /// through the REAL JSON deserializer (run → from_str), not the in-memory
    /// sample helper.
    #[test]
    fn camelcase_depends_on_alias_renders_dependency() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-camel");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "backend", "summary": "contract", "dependsOn": [] },
                    { "n": 2, "role": "frontend", "summary": "ui", "dependsOn": ["wave-1-backend"] }
                ],
                "totalWaves": 2
            }))
            .unwrap(),
        )
        .unwrap();

        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );

        let plan_md = std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap();
        // The deps column of wave 2 carries the wikilink (not "—") — proves the
        // camelCase `dependsOn` survived deserialization.
        assert!(
            plan_md.contains("| frontend | [[wave-1-backend]] |"),
            "camelCase dependsOn must render in the Depends-on column, got:\n{plan_md}"
        );
    }

    /// Invariant (2026-06-02-full-sempre-uma-wave): a **single-wave** Full plan
    /// scaffolds cleanly — parent orchestrator (`wave-plan.md` + root
    /// `meta.json` with `totalWaves: 1` / `isWavePlan: true`) plus exactly one
    /// `wave-1-{role}/`. No N≥2 assumption: a Full "reject decomposition"
    /// collapses to one wave, never to a wave-less parent.
    #[test]
    fn scaffolds_single_wave_plan() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("solo-epic");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "general", "summary": "the only wave", "depends_on": [] }
                ],
                "total_waves": 1,
                "lang": "pt-BR"
            }))
            .unwrap(),
        )
        .unwrap();

        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );

        // Parent orchestrator artefacts.
        assert!(spec_dir.join("wave-plan.md").exists());
        assert!(spec_dir.join("meta.json").exists());
        // Exactly one wave dir, with its own spec + meta.
        assert!(spec_dir.join("wave-1-general").join("spec.md").exists());
        assert!(spec_dir.join("wave-1-general").join("meta.json").exists());
        // No phantom second wave.
        assert!(!spec_dir.join("wave-2-general").exists());
        // qa/ and review/ are event-driven phases — NOT scaffolded.
        assert!(!spec_dir.join("review").join("spec.md").exists());
        assert!(!spec_dir.join("qa").join("spec.md").exists());

        // Root meta records the wave-plan parent invariant: 1 wave, isWavePlan.
        let root_meta = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();
        assert_eq!(root_meta.total_waves, Some(1));
        assert_eq!(root_meta.is_wave_plan, Some(true));

        // Idempotent.
        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );
        let again =
            std::fs::read_to_string(spec_dir.join("wave-1-general").join("spec.md")).unwrap();
        let first =
            std::fs::read_to_string(spec_dir.join("wave-1-general").join("spec.md")).unwrap();
        assert_eq!(again, first);
    }

    /// Regression (Cause 1 — stale draft estimate): `spec-draft` pre-creates the
    /// parent `meta.json` at PLAN time with an ESTIMATED `total_waves` (the Full
    /// floor of 1, before the real plan is known). `wave-scaffold` must overwrite
    /// that estimate with the REAL wave count — NOT skip the file (the old
    /// behaviour left `totalWaves: 1` on a 4-wave epic, mis-rendering the
    /// dashboard / `status`). Lifecycle fields the pipeline already advanced
    /// (stage / outcome / phase / checkpoint / flags) MUST survive the reconcile.
    #[test]
    fn reconciles_stale_parent_total_waves_preserving_lifecycle() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-stale");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // Simulate a draft-time parent meta whose lifecycle has since advanced to
        // Execute and picked up a `blocked` qualifier — with the stale estimate.
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","phase":"EXECUTE","scope":"full","lang":"pt-BR","checkpoint":"2026-06-03T00:00:00Z","isWavePlan":true,"totalWaves":1,"flags":["blocked"]}"#,
        )
        .unwrap();
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "backend", "summary": "a", "depends_on": [] },
                    { "n": 2, "role": "backend", "summary": "b", "depends_on": ["wave-1-backend"] },
                    { "n": 3, "role": "core", "summary": "c", "depends_on": ["wave-2-backend"] },
                    { "n": 4, "role": "client", "summary": "d", "depends_on": ["wave-3-core"] }
                ],
                "total_waves": 4,
                "lang": "pt-BR"
            }))
            .unwrap(),
        )
        .unwrap();

        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );

        let root = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();
        // The stale estimate is corrected to the real wave count.
        assert_eq!(root.total_waves, Some(4), "parent totalWaves reconciled to real count");
        assert_eq!(root.is_wave_plan, Some(true));
        // Advanced lifecycle + qualifier flag preserved (NOT reset to Plan/Active).
        assert_eq!(root.stage.as_deref(), Some("Execute"));
        assert_eq!(root.outcome.as_deref(), Some("Active"));
        assert_eq!(root.phase.as_deref(), Some("EXECUTE"));
        assert_eq!(root.checkpoint.as_deref(), Some("2026-06-03T00:00:00Z"));
        assert!(root.flags.0.blocked, "qualifier flag survives reconciliation");
    }

    /// Regression (Cause 2 — declared total ignored): a plan that DECLARES
    /// `total_waves: 1` but carries 4 entries scaffolds 4 table rows + 4 wave
    /// dirs, so the parent sidecar MUST record 4 (the actual count) — honouring
    /// the WARN's own stated "using {actual}" policy, never the contradictory
    /// declared value. Exercises the absent-parent (fresh-write) path.
    #[test]
    fn parent_total_waves_follows_actual_entries_not_declared() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-mismatch");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "a", "depends_on": [] },
                    { "n": 2, "role": "b", "depends_on": [] },
                    { "n": 3, "role": "c", "depends_on": [] },
                    { "n": 4, "role": "d", "depends_on": [] }
                ],
                "total_waves": 1
            }))
            .unwrap(),
        )
        .unwrap();

        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );

        let root = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();
        assert_eq!(root.total_waves, Some(4), "actual entry count wins over declared");
        assert_eq!(root.is_wave_plan, Some(true));
    }
}
