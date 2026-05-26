//! `mustard-rt run wave-scaffold` — render the canonical SDD wave layout for
//! a spec from a declarative JSON plan.
//!
//! Part of the wave-network spec (`2026-05-20-mustard-wave-network-standard`).
//! The SKILL `/feature` generates the plan JSON during PLAN; this subcommand
//! materialises every wave-N spec file, the `review/spec.md` scaffold, the
//! `qa/spec.md` scaffold, and the top-level `wave-plan.md` index.
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

use mustard_core::fs;
use mustard_core::spec;
use mustard_core::{Flags, Meta, Outcome, SpecState, Stage, write_meta};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The canonical three-line lifecycle header for a freshly scaffolded spec at
/// `stage` (always `Outcome::Active`, no flags), terminated by `\n` per line.
/// Wave plans / sub-plan / review / qa scaffolds all start active — a `queued`
/// or `draft` sub-plan is a not-yet-started Plan item, so its canonical stage
/// stays `Plan`. Delegates the spelling to [`mustard_core::spec`].
fn header_block(stage: Stage) -> String {
    let state = SpecState::new(stage, Outcome::Active, Flags::default()).unwrap_or(SpecState {
        stage: Stage::Plan,
        outcome: Outcome::Active,
        flags: Flags::default(),
    });
    let [s, o, f] = spec::serialize_header(&state);
    format!("{s}\n{o}\n{f}\n")
}

/// One wave entry inside the plan JSON.
#[derive(Debug, Clone, Deserialize)]
struct WavePlanEntry {
    /// Wave number (1-based).
    n: u32,
    /// Role label (`general`, `frontend`, `backend`, …) — drives the folder
    /// name `wave-{n}-{role}`.
    role: String,
    /// Short one-line summary surfaced in `wave-plan.md` and the wave's
    /// `Resumo` heading.
    #[serde(default)]
    summary: String,
    /// Other wave names this wave depends on (e.g. `["wave-1-general"]`).
    /// Rendered in the wave-plan table's `Depende de` column and the wave
    /// spec's `## Network` section.
    #[serde(default)]
    depends_on: Vec<String>,
}

/// Top-level plan shape.
#[derive(Debug, Clone, Deserialize)]
struct Plan {
    waves: Vec<WavePlanEntry>,
    #[serde(default)]
    total_waves: Option<u32>,
    #[serde(default)]
    lang: Option<String>,
}

/// Localised heading strings.
struct Headings<'a> {
    wave_plan_title: &'a str,
    table_header: &'a str,
    table_sep: &'a str,
    network: &'a str,
    parent: &'a str,
    review_title: &'a str,
    qa_title: &'a str,
    review_intro: &'a str,
    qa_intro: &'a str,
    wave_table_caption: &'a str,
}

fn headings_for(lang: &str) -> Headings<'static> {
    // Tolerant read: accept BCP-47 + legacy short codes.
    let lc = lang.trim().to_ascii_lowercase();
    if lc == "en" || lc == "en-us" {
        Headings {
            wave_plan_title: "# Wave Plan",
            table_header:
                "| Wave | Spec | Role | Depends on | Summary |",
            table_sep: "|------|------|------|------------|---------|",
            network: "## Network",
            parent: "Parent",
            review_title: "# Review Plan",
            qa_title: "# QA Plan",
            review_intro: "Checklist for the review agent.",
            qa_intro:
                "Acceptance Criteria consolidated from every wave.",
            wave_table_caption: "## Wave Table",
        }
    } else {
        Headings {
            wave_plan_title: "# Plano de Waves",
            table_header:
                "| Wave | Spec | Role | Depende de | Resumo |",
            table_sep: "|------|------|------|------------|--------|",
            network: "## Network",
            parent: "Parent",
            review_title: "# Plano de Review",
            qa_title: "# Plano de QA",
            review_intro: "Checklist para o agente de review.",
            qa_intro:
                "Critérios de Aceitação consolidados de todas as waves.",
            wave_table_caption: "## Tabela de Waves",
        }
    }
}

/// Render the wave-plan markdown index.
fn render_wave_plan(plan: &Plan, hd: &Headings<'_>) -> String {
    let total = plan.total_waves.unwrap_or(plan.waves.len() as u32);
    let mut out = String::new();
    out.push_str(hd.wave_plan_title);
    out.push_str("\n\n");
    out.push_str(&header_block(Stage::Plan));
    out.push_str("### Scope: full (wave plan)\n");
    let _ = writeln!(out, "### Total waves: {total}\n");
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
    out
}

/// `wave-{n}-{role}` folder/spec name.
fn wave_name(w: &WavePlanEntry) -> String {
    format!("wave-{n}-{role}", n = w.n, role = w.role)
}

/// Render an individual wave's `spec.md` skeleton.
fn render_wave_spec(parent: &str, w: &WavePlanEntry, hd: &Headings<'_>) -> String {
    let name = wave_name(w);
    let mut out = String::new();
    let _ = writeln!(out, "# {name}\n");
    let _ = writeln!(out, "### {p}: [[{parent}]]", p = hd.parent);
    // Both `draft` (wave 1) and `queued` (later waves) are not-yet-started Plan
    // items in the canonical model — the wave-plan tracks progression via
    // events, not a per-wave header status word.
    out.push_str(&header_block(Stage::Plan));
    out.push('\n');
    out.push_str("## Resumo\n\n");
    if w.summary.is_empty() {
        out.push_str("_(preencher)_\n\n");
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
        let _ = writeln!(out, "- Depende de: {}", deps.join(", "));
    }
    out
}

/// Render `review/spec.md`.
fn render_review(parent: &str, hd: &Headings<'_>) -> String {
    let mut out = String::new();
    out.push_str(hd.review_title);
    out.push_str("\n\n");
    let _ = writeln!(out, "### {p}: [[{parent}]]", p = hd.parent);
    out.push_str(&header_block(Stage::Plan));
    out.push('\n');
    out.push_str(hd.review_intro);
    out.push_str("\n\n");
    out.push_str("## Checklist\n\n");
    out.push_str("- [ ] SOLID\n");
    out.push_str("- [ ] Design System\n");
    out.push_str("- [ ] Patterns\n");
    out.push_str("- [ ] i18n\n");
    out.push_str("- [ ] Integration\n");
    out.push_str("- [ ] Build\n");
    out.push_str("- [ ] Elegance\n\n");
    out.push_str("<!-- verdict → review/verdict.md -->\n");
    out
}

/// Render `qa/spec.md`.
fn render_qa(parent: &str, hd: &Headings<'_>) -> String {
    let mut out = String::new();
    out.push_str(hd.qa_title);
    out.push_str("\n\n");
    let _ = writeln!(out, "### {p}: [[{parent}]]", p = hd.parent);
    out.push_str(&header_block(Stage::Plan));
    out.push('\n');
    out.push_str(hd.qa_intro);
    out.push_str("\n\n");
    out.push_str("## Acceptance Criteria (consolidated)\n\n");
    out.push_str("_(populated from each wave's AC at QA time)_\n\n");
    out.push_str("<!-- report → qa/report.md -->\n");
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
    let lang = plan.lang.as_deref().unwrap_or("pt-BR");
    let hd = headings_for(lang);

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
    let wave_plan_md = render_wave_plan(&plan, &hd);
    emit(&spec_dir.join("wave-plan.md"), wave_plan_md);

    // Per-wave spec.
    for w in &plan.waves {
        let dir = spec_dir.join(wave_name(w));
        emit(&dir.join("spec.md"), render_wave_spec(&parent_name, w, &hd));
    }

    // review/spec.md + qa/spec.md.
    emit(
        &spec_dir.join("review").join("spec.md"),
        render_review(&parent_name, &hd),
    );
    emit(
        &spec_dir.join("qa").join("spec.md"),
        render_qa(&parent_name, &hd),
    );

    // Wave 3 of mustard-unification: emit `meta.json` alongside every spec.md
    // we just wrote so consumers can read lifecycle metadata as structured
    // JSON instead of regexing the markdown. Fail-open per file.
    let total_waves = plan.total_waves.unwrap_or(plan.waves.len() as u32);
    write_scaffold_meta(
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
                raw: Value::Null,
            },
        );
    }
    write_scaffold_meta(
        &spec_dir.join("review"),
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
            raw: Value::Null,
        },
    );
    write_scaffold_meta(
        &spec_dir.join("qa"),
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
            raw: Value::Null,
        },
    );

    let out: Value = json!({
        "created_files": created,
        "skipped": skipped,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
    );
}

/// Write `meta.json` beside a scaffolded spec.md, only when one is absent.
/// Fail-open: a write failure warns on stderr and never panics.
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
    fn renders_wave_plan_table_with_wikilinks() {
        let hd = headings_for("pt");
        let md = render_wave_plan(&sample_plan(), &hd);
        assert!(md.contains("[[wave-1-general]]"));
        assert!(md.contains("[[wave-2-frontend]]"));
        assert!(md.contains("foundations"));
        assert!(md.contains("[[wave-1-general]]"));
    }

    #[test]
    fn renders_wave_spec_with_parent_and_status() {
        let hd = headings_for("pt");
        let plan = sample_plan();
        let s1 = render_wave_spec("epic-x", &plan.waves[0], &hd);
        assert!(s1.contains("### Parent: [[epic-x]]"));
        // New canonical header: every freshly scaffolded wave is Plan + Active.
        assert!(s1.contains("### Stage: Plan"));
        assert!(s1.contains("### Outcome: Active"));
        let s2 = render_wave_spec("epic-x", &plan.waves[1], &hd);
        assert!(s2.contains("### Stage: Plan"));
        assert!(s2.contains("[[wave-1-general]]"));
        assert!(s2.contains("## Network"));
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

        // 4 files for a 2-wave plan: wave-plan + 2× wave-N/spec.md + review + qa = 5
        assert!(spec_dir.join("wave-plan.md").exists());
        assert!(spec_dir.join("wave-1-general").join("spec.md").exists());
        assert!(spec_dir.join("wave-2-frontend").join("spec.md").exists());
        assert!(spec_dir.join("review").join("spec.md").exists());
        assert!(spec_dir.join("qa").join("spec.md").exists());

        // Validate wave-1 spec content has the expected headings & wikilinks.
        let s1 =
            std::fs::read_to_string(spec_dir.join("wave-1-general").join("spec.md")).unwrap();
        assert!(s1.contains("### Parent: [[epic-x]]"));
        assert!(s1.contains("### Stage: Plan"));
        assert!(s1.contains("## Network"));

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
}
