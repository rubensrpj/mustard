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
//!     {
//!       "n": 1,
//!       "role": "general",
//!       "summary": "…",
//!       "depends_on": [],
//!       "tasks": ["wire the contract", "add the handler"],
//!       "files": ["src/api/handler.rs", "src/api/mod.rs"],
//!       "acceptance": ["**AC-1** — handler returns 200. Command: `curl -sf …`"]
//!     },
//!     { "n": 2, "role": "general", "summary": "…", "depends_on": ["wave-1-general"] }
//!   ],
//!   "total_waves": 2,
//!   "lang": "pt-BR"
//! }
//! ```
//!
//! ### Per-wave body fields (the materialised work, authored by the Plan agent)
//!
//! - `tasks` — checklist lines for this wave. Materialised as
//!   `## Tasks`/`## Tarefas` with `- [ ] {task}` items in the wave's `spec.md`.
//!   `agent-prompt-render --spec <wave-dir>` reads this section back as the
//!   dispatched agent's `## TASK` block — so the body is no longer hand-authored
//!   after the scaffold.
//! - `files` — the file census for this wave. Materialised as
//!   `## Files`/`## Arquivos` with `` - `{path}` `` items; `agent-prompt-render`
//!   reads it back into `{reference_files}`.
//! - `acceptance` — Acceptance Criteria lines. NOT written into the per-wave
//!   `spec.md` (the renderer does not read AC from a wave spec); instead the
//!   union across waves is carried into `wave-plan.md` under
//!   `## Acceptance Criteria`/`## Critérios de Aceitação`, where the QA gate
//!   reads it via `spec_sections::section_block(_, "acceptanceCriteria")`.
//!
//! Each is `#[serde(default)]`: a plan that predates these fields (summary-only)
//! still deserialises, and a wave that omits them materialises with no task /
//! file block (the empty-tasks case emits a visible stderr WARN — see [`run`]).
//!
//! `lang` accepts BCP-47 (`pt-BR` / `en-US`); the legacy short forms
//! (`pt` / `en`) are tolerated on read for back-compat with old plan JSON
//! and normalised to BCP-47 in the rendered headings. The *effective* heading
//! language follows the project's `mustard.json#specLang` (root wins) when the
//! scaffold runs inside a workspace; the plan's `lang` is the fallback for a
//! standalone scaffold. Every generated artefact (headings, placeholders) is
//! rendered in that effective language per the i18n rule.
//!
//! Idempotent: each output file is only created when absent. The stdout JSON
//! reports which were created vs skipped.

use crate::shared::gate_mode::{resolve_mode, GateMode};
use mustard_core::domain::spec::contract::ChecklistItem;
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
    /// Checklist of work items for this wave, authored by the Plan agent.
    /// Materialised as a `## Tasks`/`## Tarefas` section of `- [ ] {task}`
    /// lines in the wave's `spec.md` (read back by `agent-prompt-render`).
    /// `#[serde(default)]` is an explicit retrocompat affordance: a
    /// summary-only plan (pre-dating this field) still deserialises, and the
    /// empty case is surfaced by a visible stderr WARN in [`run`] rather than a
    /// silent empty heading.
    #[serde(default)]
    pub(crate) tasks: Vec<String>,
    /// File census for this wave. Materialised as a `## Files`/`## Arquivos`
    /// section of `` - `{path}` `` lines (read back into `{reference_files}`).
    /// `#[serde(default)]` for the same retrocompat reason as `tasks`.
    #[serde(default)]
    pub(crate) files: Vec<String>,
    /// Acceptance Criteria lines for this wave. NOT written into the per-wave
    /// `spec.md` (the renderer never reads AC from a wave spec); the union of
    /// every wave's `acceptance` is carried into `wave-plan.md` so the QA gate
    /// finds it. `#[serde(default)]` for the same retrocompat reason as `tasks`.
    #[serde(default)]
    pub(crate) acceptance: Vec<String>,
    /// AC ids this wave is responsible for satisfying (e.g. `["AC-1", "AC-3"]`),
    /// tracing the parent spec's criteria onto the wave that implements them.
    /// `#[serde(default)]` retrocompat: a plan predating the field (or one that
    /// carries only `acceptance` lines) still deserialises — the traceability
    /// check then derives the wave's satisfied ids from its `acceptance` lines.
    #[serde(default)]
    pub(crate) satisfies: Vec<String>,
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
/// These render MACHINE artefacts — the operational `wave-plan.md` index and the
/// per-wave `spec.md` skeletons (with their materialised `## Tasks` / `## Files`
/// / `## Acceptance Criteria` bodies). They are ENGLISH-FIXED regardless of the
/// project's configured language (only the user-facing spec narrative follows
/// config-lang). The struct is retained (rather than inlining the literals) so
/// the re-wave path renders through the same canonical renderers (F4-d item 2).
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
    /// `## Summary`/`## Resumo` heading for the per-wave spec skeleton.
    summary: &'a str,
    /// Placeholder body when a wave has no summary yet, in the effective locale.
    summary_placeholder: &'a str,
    /// `Depends on`/`Depende de` label for the wave spec's Network section.
    depends_on: &'a str,
    /// `## Tasks`/`## Tarefas` heading for the per-wave materialised checklist.
    tasks: &'a str,
    /// `## Files`/`## Arquivos` heading for the per-wave file census.
    files: &'a str,
    /// `## Acceptance Criteria`/`## Critérios de Aceitação` heading for the
    /// AC union carried into `wave-plan.md`.
    acceptance: &'a str,
}

/// Build the heading set. These render MACHINE artefacts — the operational
/// `wave-plan.md` index and the per-wave `spec.md` skeletons (with their
/// materialised `## Tasks` / `## Files` / `## Acceptance Criteria` bodies) — so
/// the headings are ENGLISH-FIXED regardless of the project's configured
/// language (the reverted "generated artefacts follow config-lang" rule for
/// machine artefacts; only the user-facing spec narrative still follows
/// config-lang). The display names are the EN spellings
/// `spec_sections::is_heading` recognises, so `agent-prompt-render` and the QA
/// gate keep consuming the materialised body.
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
        tasks: "## Tasks",
        files: "## Files",
        acceptance: "## Acceptance Criteria",
    }
}

/// Render the wave-plan markdown index. Lifecycle metadata (stage / scope /
/// total waves) lives only in the `meta.json` sidecar — the markdown is pure
/// narrative + the wave table.
///
/// `parent_slug` is the parent spec's directory name; it seeds the leading
/// `id: wave.{slug}.plan` frontmatter — the rename-proof identity handle that
/// makes `[[wave.{slug}.plan]]` a mustard-resolvable wikilink
/// (`atomic_md::wikilink::resolve` prefers a frontmatter `id:` over the
/// filename). Identity is NOT lifecycle metadata, so it does not violate the
/// "pure narrative" rule. A blank `parent_slug` (defensive) omits the block so
/// the document still parses.
pub(crate) fn render_wave_plan(
    plan: &Plan,
    hd: &Headings<'_>,
    ac_block: Option<&str>,
    parent_slug: &str,
) -> String {
    let mut out = String::new();
    if !parent_slug.is_empty() {
        let _ = write!(out, "---\nid: wave.{parent_slug}.plan\n---\n\n");
    }
    out.push_str(hd.wave_plan_title);
    out.push_str("\n\n");
    out.push_str(hd.wave_table_caption);
    out.push_str("\n\n");
    out.push_str(hd.table_header);
    out.push('\n');
    out.push_str(hd.table_sep);
    out.push('\n');
    for w in &plan.waves {
        let link = wave_self_link(parent_slug, w);
        let deps = if w.depends_on.is_empty() {
            "—".to_string()
        } else {
            w.depends_on
                .iter()
                .map(|d| wave_dep_link(parent_slug, d))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let summary = w.summary.replace('|', "\\|");
        let _ = writeln!(
            out,
            "| {n} | {link} | {role} | {deps} | {summary} |",
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

/// Convert a `wave-{n}-{role}` dependency string into its resolvable
/// `[[wave.{parent}.{n}-{role}]]` wikilink — the same `id:` shape
/// [`render_wave_spec`] stamps on each wave's own frontmatter. A bare
/// `[[wave-1-backend]]` link can never resolve: the wave lives at
/// `wave-1-backend/spec.md` (a directory, not a flat `wave-1-backend.md`),
/// and its stamped `id:` carries the `wave.{parent}.` prefix — the
/// resolver's `id:`-match path is the only one that can ever succeed, and
/// only when the token is prefixed to match. Falls back to the bare
/// bracketed form when `dep` does not start with `wave-` (a malformed/
/// legacy dependency string) or `parent` is empty (defensive) — the
/// resolver then honestly flags it `⚠ unresolved` rather than silently
/// mis-linking.
fn wave_dep_link(parent: &str, dep: &str) -> String {
    match dep.strip_prefix("wave-") {
        Some(suffix) if !parent.is_empty() => format!("[[wave.{parent}.{suffix}]]"),
        _ => format!("[[{dep}]]"),
    }
}

/// The resolvable `[[wave.{parent}.{n}-{role}]]` self-identity wikilink for
/// wave `w` — the exact `id:` [`render_wave_spec`] stamps on its own
/// frontmatter. Used for the wave-plan table's own-wave column, so the row
/// actually links to the wave instead of rendering `⚠ unresolved` for every
/// row. [`wave_dep_link`] is the dependency-column sibling (same target
/// shape, built from a plan-supplied string instead of a [`WavePlanEntry`]).
/// Falls back to the bare `[[wave-{n}-{role}]]` form when `parent` is empty
/// (defensive — the resolver then honestly flags it unresolved).
fn wave_self_link(parent: &str, w: &WavePlanEntry) -> String {
    if parent.is_empty() {
        format!("[[{}]]", wave_name(w))
    } else {
        format!("[[wave.{parent}.{n}-{role}]]", n = w.n, role = w.role)
    }
}

/// Render an individual wave's `spec.md` — `## Summary` + `## Network`, then
/// the materialised `## Tasks` / `## Files` work body from the plan entry.
///
/// Pure: returns the rendered String, no IO. The empty-`tasks` signal (a wave
/// the Plan agent left without a checklist) is surfaced by the caller in
/// [`run`] via a stderr WARN, not here — an empty task block emits **no**
/// `## Tasks` heading (a bare heading is noise; `agent-prompt-render` falls
/// back to an empty TASK block, which the WARN makes visible).
pub(crate) fn render_wave_spec(parent: &str, w: &WavePlanEntry, hd: &Headings<'_>) -> String {
    let name = wave_name(w);
    let mut out = String::new();
    // Leading `id:` frontmatter — the rename-proof identity handle, derived from
    // the parent spec slug plus this wave's `{n}-{role}` (the same tokens
    // `wave_name` builds the folder from). `[[wave.{slug}.{n}-{role}]]` resolves
    // to this file via `atomic_md::wikilink::resolve`'s frontmatter-id
    // precedence. Identity is not lifecycle metadata (which stays in
    // `meta.json`). A blank `parent` (defensive) omits the block.
    if !parent.is_empty() {
        let _ = write!(out, "---\nid: wave.{parent}.{n}-{role}\n---\n\n", n = w.n, role = w.role);
    }
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
    // `spec.{parent}` — not the bare slug — because `spec-draft` stamps the
    // spec root's own identity as `id: spec.{slug}` inside `{slug}/spec.md`
    // (a directory, not a flat `{slug}.md`). The wikilink resolver's filename
    // fallback (`{token}.md`) can never match a directory-nested `spec.md`,
    // so the `id:` match is the only path — and it only succeeds prefixed.
    // A bare `[[{parent}]]` here always rendered `⚠ unresolved` in the footer.
    let _ = writeln!(out, "- {p}: [[spec.{parent}]]", p = hd.parent);
    if !w.depends_on.is_empty() {
        let deps: Vec<String> = w
            .depends_on
            .iter()
            .map(|d| wave_dep_link(parent, d))
            .collect();
        let _ = writeln!(out, "- {dep}: {}", deps.join(", "), dep = hd.depends_on);
    }
    // Materialise the work body the Plan agent authored, so it no longer has to
    // be hand-written after the scaffold. `agent-prompt-render --spec <wave-dir>`
    // reads these sections back (`## Tasks`/`## Tarefas` → `{task_steps}`,
    // `## Files`/`## Arquivos` → `{reference_files}`). Emit a heading only when
    // there is content under it — a bare heading is noise.
    if !w.tasks.is_empty() {
        let _ = write!(out, "\n{}\n\n", hd.tasks);
        for task in &w.tasks {
            // Strip any checkbox/bullet prefix the Plan agent already authored
            // (`- [ ] foo` → `foo`) via the canonical normaliser, so a
            // pre-prefixed plan never renders the doubled `- [ ] - [ ]` form
            // (measured in 3 real specs).
            let _ = writeln!(
                out,
                "- [ ] {task}",
                task = mustard_core::domain::spec::contract::normalize_task_label(task)
            );
        }
    }
    if !w.files.is_empty() {
        let _ = write!(out, "\n{}\n\n", hd.files);
        for file in &w.files {
            let _ = writeln!(out, "- `{file}`", file = file.trim());
        }
    }
    out
}

/// Synthesize the global `## Acceptance Criteria` block carried into
/// `wave-plan.md` from the per-wave `acceptance` arrays.
///
/// Returns `Some(block)` when at least one wave carries an AC line — the block
/// is the localised heading followed by the union of every wave's AC lines, in
/// wave order, de-duplicated. Returns `None` when no wave carries AC, so a
/// summary-only (pre-body) plan renders a byte-stable `wave-plan.md` (no AC
/// section appended). The QA gate reads the block back via
/// `spec_sections::section_block(md, "acceptanceCriteria")`, which the
/// localised heading resolves against.
fn build_ac_block(plan: &Plan, hd: &Headings<'_>) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    for w in &plan.waves {
        for ac in &w.acceptance {
            let trimmed = ac.trim();
            if trimmed.is_empty() {
                continue;
            }
            let bullet = if trimmed.starts_with('-') {
                trimmed.to_string()
            } else {
                format!("- {trimmed}")
            };
            if !lines.contains(&bullet) {
                lines.push(bullet);
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!("{}\n{}", hd.acceptance, lines.join("\n")))
}

/// The two AC↔wave traceability gap kinds, kept apart so [`scaffold`] can
/// escalate ONLY the uncovered-criterion gap under `MUSTARD_TRACE_GATE_MODE`
/// while the untraced-wave signal stays a non-blocking WARN in every mode.
struct TraceGaps {
    /// Gap 1 — a wave that does work (`tasks` non-empty) but satisfies NO
    /// criterion. Always WARN-level: a wave can legitimately be plumbing /
    /// scaffolding no single AC pins down, so this never blocks.
    untraced_waves: Vec<String>,
    /// Gap 2 — an acceptance criterion NO wave satisfies. `defined` is the
    /// union of every wave's `acceptance` ids AND the parent spec.md
    /// `## Acceptance Criteria` ids, so a criterion the plan forgot to route
    /// onto a wave is caught. This is the escalatable gap.
    uncovered_acs: Vec<String>,
}

/// Compute AC↔wave traceability gaps, splitting the untraced-wave signal
/// (Gap 1, always WARN) from the uncovered-criterion signal (Gap 2,
/// escalatable — see [`scaffold`]):
///
/// 1. A wave that does work (`tasks` non-empty) but satisfies NO acceptance
///    criterion — its work traces to no criterion.
/// 2. An AC in the `defined` set that NO wave claims to satisfy — an orphan
///    criterion.
///
/// A wave's satisfied set is its explicit `satisfies` ids, or — when that is
/// empty (back-compat with pre-`satisfies` plans) — the ids parsed from its
/// `acceptance` lines through the SAME `qa-run` parser QA executes ([`parse_ac_items`]),
/// so the two can never drift.
///
/// The `defined` set (every id a wave must cover) is the union of every wave's
/// `acceptance` ids AND the parent spec.md `## Acceptance Criteria` ids — the
/// latter read through the SAME shared qa-run extractor + parser
/// (`extract_ac_section` + `parse_ac_items`), never a forked reader.
/// `parent_ac_md` is the monolithic parent spec markdown (`None` for a
/// standalone scaffold with no parent, in which case only the plan's own
/// `acceptance` ids define the set — the historical behaviour).
fn traceability_gaps(plan: &Plan, parent_ac_md: Option<&str>) -> TraceGaps {
    use crate::commands::review::qa_run::{extract_ac_section, parse_ac_items};
    use std::collections::BTreeSet;
    let norm = |s: &str| s.trim().to_uppercase();
    let mut untraced_waves: Vec<String> = Vec::new();
    let mut defined: BTreeSet<String> = BTreeSet::new();
    let mut covered: BTreeSet<String> = BTreeSet::new();

    // The parent spec's own criteria are authoritative — every one must be
    // claimed by some wave. Read via the shared qa-run extractor + parser so
    // this reader can never drift from the section QA actually executes. An
    // absent parent / no AC section simply contributes nothing.
    if let Some(section) = parent_ac_md.and_then(extract_ac_section) {
        for it in parse_ac_items(&section) {
            defined.insert(norm(&it.id));
        }
    }

    for w in &plan.waves {
        // The ACs this wave DEFINES, via the shared qa-run parser. Acceptance
        // lines may arrive without a leading bullet (the plan schema example
        // does); normalise to `- <line>` — exactly as `build_ac_block` does —
        // so the parser (which requires a bullet) finds them.
        let ac_text = w
            .acceptance
            .iter()
            .map(|line| {
                let t = line.trim();
                if t.starts_with('-') { t.to_string() } else { format!("- {t}") }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let ac_ids: Vec<String> = parse_ac_items(&ac_text)
            .into_iter()
            .map(|it| norm(&it.id))
            .collect();
        for id in &ac_ids {
            defined.insert(id.clone());
        }
        // Satisfied set: explicit `satisfies` wins; else the acceptance ids.
        let satisfied: Vec<String> = if w.satisfies.is_empty() {
            ac_ids
        } else {
            w.satisfies.iter().map(|s| norm(s)).filter(|s| !s.is_empty()).collect()
        };
        for id in &satisfied {
            covered.insert(id.clone());
        }
        if !w.tasks.is_empty() && satisfied.is_empty() {
            untraced_waves.push(format!(
                "wave-{n}-{role} has tasks but satisfies no AC — add `satisfies` ids or an `acceptance` line so its work traces to a criterion",
                n = w.n,
                role = w.role,
            ));
        }
    }
    let uncovered_acs: Vec<String> = defined
        .difference(&covered)
        .map(|id| format!("{id} — no wave satisfies it (add it to a wave's `satisfies` or `acceptance`)"))
        .collect();
    TraceGaps { untraced_waves, uncovered_acs }
}

/// The blocking gap list for a resolved `MUSTARD_TRACE_GATE_MODE`: `strict`
/// carries every uncovered criterion through (so [`run`] exits non-zero),
/// `warn`/`off` carry none — the WARN [`scaffold`] already printed is the whole
/// signal in those modes. Pure: the env read stays in the caller, so the
/// escalation is unit-testable without mutating the process environment.
fn trace_block_for(mode: GateMode, uncovered_acs: Vec<String>) -> Vec<String> {
    if mode == GateMode::Strict {
        uncovered_acs
    } else {
        Vec::new()
    }
}

/// Seed the per-wave trackable checklist from the wave's file census — one
/// item per target file (`{label, path, done: false}`), reusing the core
/// [`ChecklistItem`]. The path doubles as the label (deterministic, no
/// narrative to localise) and as the auto-mark anchor the
/// `checklist-auto-mark` hook / `mark-checklist-item` key off. Blank entries
/// are dropped; order follows the plan (byte-stable output).
fn checklist_from_files(files: &[String]) -> Vec<ChecklistItem> {
    files
        .iter()
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .map(|f| ChecklistItem {
            label: f.to_string(),
            path: Some(f.to_string()),
            done: false,
        })
        .collect()
}

/// Write `content` to `path` only when the file does not already exist.
/// Returns `true` when the file was created, `false` when it was skipped.
fn write_if_absent(path: &Path, content: &str) -> bool {
    if fs::exists(path) {
        return false;
    }
    fs::write_atomic(path, content.as_bytes()).is_ok()
}

/// Outcome of one scaffold pass — the miolo result [`run`] prints and
/// `plan-materialize` folds into its composite report.
pub(crate) enum ScaffoldOutcome {
    /// The layout was materialised (idempotently). `trace_block` lists the
    /// uncovered parent/plan acceptance criteria ONLY when `MUSTARD_TRACE_GATE_MODE`
    /// is `strict` (empty in `warn`/`off`); a non-empty list makes the standalone
    /// [`run`] exit non-zero so the orchestrator notices the untraced criterion.
    /// The in-process caller (`plan-materialize`) ignores it — the trace gate is
    /// on the `wave-scaffold` command, never the pipeline composite.
    Created {
        created: Vec<String>,
        skipped: Vec<String>,
        trace_block: Vec<String>,
    },
    /// `plan.waves` was empty — operator error (W10.T10.3 hard gate).
    EmptyPlan,
    /// The plan file could not be read or parsed; carries the stderr message.
    Unreadable(String),
}

/// Run `mustard-rt run wave-scaffold --spec-dir <dir> --plan <json-file>`.
///
/// Idempotent. Stdout is `{"created_files":[...],"skipped":[...]}`; operator
/// errors (empty plan, unreadable/unparseable plan) add an `error` field —
/// plus an actionable `hint` for the missing-`n`/`role` case — and exit 2 so
/// the orchestrator notices.
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

    match scaffold(&spec_dir, &plan_path) {
        ScaffoldOutcome::Unreadable(msg) => {
            eprintln!("{msg}");
            // Same rationale as the EmptyPlan arm below: an unreadable /
            // unparseable plan is operator error — express it on stdout too
            // (the orchestrator parses the JSON, not stderr) and exit non-zero
            // so it notices, instead of the old `created_files: []` + exit 0
            // that looked like a clean no-op.
            let summary = msg.strip_prefix("[wave-scaffold] ").unwrap_or(msg.as_str());
            // A read failure embeds the absolutized (cwd-dependent) plan path
            // plus the OS-specific io message — both volatile, and `run`
            // stdout must stay byte-stable (crate guard). The full message
            // already went to stderr above; stdout keeps the deterministic
            // prefix only, mirroring plan-materialize's scrubbed
            // `plan unreadable` constant (the EmptyPlan arm below likewise
            // keeps the path on stderr).
            let summary = if summary.starts_with("cannot read plan") {
                "cannot read plan"
            } else {
                summary
            };
            // The failure measured in production (≥6× in 6 days on the sialia
            // telemetry) is a hand-authored plan omitting the required
            // `n`/`role` — serde reports it as `missing field`. Attach the
            // actionable fix, not just the symptom.
            let out = if msg.contains("missing field") {
                json!({
                    "created_files": [],
                    "skipped": [],
                    "error": summary,
                    "hint": "every waves[] entry requires \"n\" (1-based wave number) and \
                             \"role\"; generate the plan with `mustard-rt run \
                             plan-materialize` (the pipeline entry)",
                })
            } else {
                json!({
                    "created_files": [],
                    "skipped": [],
                    "error": summary,
                })
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
            );
            std::process::exit(2);
        }
        // W10.T10.3 — hard gate: an empty plan is operator error, not "scaffold
        // nothing". Print to stderr and exit non-zero so the orchestrator notices.
        ScaffoldOutcome::EmptyPlan => {
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
        ScaffoldOutcome::Created { created, skipped, trace_block } => {
            // strict `MUSTARD_TRACE_GATE_MODE` + an uncovered parent/plan AC:
            // the layout WAS materialised (idempotent), but the command fails so
            // the orchestrator notices the untraced criterion. The gap ids are
            // deterministic (no paths/timestamps), so listing them on stdout
            // keeps `run` byte-stable; warn/off leave `trace_block` empty and
            // fall through to the clean success output.
            if !trace_block.is_empty() {
                let out: Value = json!({
                    "created_files": created,
                    "skipped": skipped,
                    "error": "uncovered acceptance criteria",
                    "trace_gaps": trace_block,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
                );
                std::process::exit(2);
            }
            let out: Value = json!({
                "created_files": created,
                "skipped": skipped,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
            );
        }
    }
}

/// Materialise the wave layout for an already-resolved `spec_dir` + `plan_path`.
///
/// The non-printing miolo of [`run`], reused in-process by
/// [`crate::commands::pipeline::plan_materialize`] (no subprocess). Warnings
/// (declared-total mismatch, empty-tasks waves) still go to stderr; the result
/// is returned typed instead of printed.
pub(crate) fn scaffold(spec_dir: &Path, plan_path: &Path) -> ScaffoldOutcome {
    let raw = match fs::read_to_string(plan_path) {
        Ok(t) => t,
        Err(e) => {
            return ScaffoldOutcome::Unreadable(format!(
                "[wave-scaffold] cannot read plan {}: {e}",
                plan_path.display()
            ));
        }
    };
    let plan: Plan = match serde_json::from_str::<Plan>(&raw) {
        Ok(p) => p,
        Err(e) => {
            return ScaffoldOutcome::Unreadable(format!(
                "[wave-scaffold] plan JSON parse error: {e}"
            ));
        }
    };

    if plan.waves.is_empty() {
        return ScaffoldOutcome::EmptyPlan;
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
    // `lang` is recorded verbatim in meta.json (the spec-facing locale the plan
    // declared — the user-facing narrative still follows config-lang). The
    // headings, by contrast, render MACHINE artefacts, so they are ENGLISH-FIXED
    // regardless of the configured language.
    let lang = plan.lang.as_deref().unwrap_or("pt-BR");
    let hd = headings();

    let _ = fs::create_dir_all(spec_dir);

    let mut created: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut emit = |path: &Path, body: String| {
        let rel = path
            .strip_prefix(spec_dir)
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

    // Synthesize the global Acceptance Criteria block from the per-wave
    // `acceptance` arrays. When any wave carries AC, their union is written into
    // `wave-plan.md` under the localised `## Acceptance Criteria` heading, so the
    // QA gate still reads them via `section_block(_, "acceptanceCriteria")`. When
    // NO wave carries AC, `None` is passed and the wave-plan output stays
    // byte-stable for summary-only (pre-body) plans.
    let ac_block = build_ac_block(&plan, &hd);

    // wave-plan.md.
    let wave_plan_md = render_wave_plan(&plan, &hd, ac_block.as_deref(), &parent_name);
    emit(&spec_dir.join("wave-plan.md"), wave_plan_md);

    // Per-wave spec. A wave the Plan agent left with no `tasks` is a visible
    // signal — emit a stderr WARN so the operator notices the gap instead of it
    // silently materialising an empty TASK block downstream.
    for w in &plan.waves {
        if w.tasks.is_empty() {
            eprintln!(
                "[wave-scaffold] WARN: wave-{n}-{role} materialised with no tasks — \
                 agent-prompt-render will fall back to an empty task block",
                n = w.n,
                role = w.role,
            );
        }
        let dir = spec_dir.join(wave_name(w));
        emit(&dir.join("spec.md"), render_wave_spec(&parent_name, w, &hd));
    }

    // AC↔wave traceability (F6): the untraced-wave signal (Gap 1) stays a
    // non-blocking WARN in every mode; the uncovered-criterion signal (Gap 2 —
    // an AC of the plan OR the parent spec.md that no wave claims) escalates
    // under `MUSTARD_TRACE_GATE_MODE` (default strict). The parent monolithic
    // spec is `spec.md` at PLAN time, or `spec.original.md` once a rewave
    // archived it; an absent parent contributes no ids (standalone scaffold).
    let parent_ac_md = fs::read_to_string(spec_dir.join("spec.md"))
        .or_else(|_| fs::read_to_string(spec_dir.join("spec.original.md")))
        .ok();
    let gaps = traceability_gaps(&plan, parent_ac_md.as_deref());
    for gap in &gaps.untraced_waves {
        eprintln!("[wave-scaffold] WARN: {gap}");
    }
    // off silences Gap 2 entirely; warn/strict print each as a WARN; only
    // strict carries them into `trace_block` so the standalone `run` exits
    // non-zero. `scaffold` itself never exits — it stays reusable in-process
    // (plan-materialize), which is why the block travels back typed.
    let trace_mode = resolve_mode("MUSTARD_TRACE_GATE_MODE", None, GateMode::Strict);
    if trace_mode != GateMode::Off {
        for gap in &gaps.uncovered_acs {
            eprintln!("[wave-scaffold] WARN: {gap}");
        }
    }
    let trace_block = trace_block_for(trace_mode, gaps.uncovered_acs);

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
        spec_dir,
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
            // The PARENT is a coordination doc — its actionable checklist
            // lives in each wave's sidecar (seeded below), never in the root
            // meta (explicit OUT of the checklist-progresso spec).
            checklist: Vec::new(),
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
                // Events-first per-wave progress: one trackable item per
                // target file. `write_scaffold_meta` is skip-if-absent, so a
                // re-scaffold never resets `done` flags already flipped by
                // the auto-mark hook / `mark-checklist-item`.
                checklist: checklist_from_files(&w.files),
                raw: Value::Null,
            },
        );
    }
    // D3: `qa/` and `review/` are pipeline *phases*, not specs — they carry no
    // lifecycle, so no `meta.json` sidecar is written for them. Only the root
    // and each `wave-N` directory get a sidecar (above). The result of each
    // phase is materialised by code into `qa/report.md` / `review/verdict.md`
    // (D4), not tracked through a dead sidecar.

    ScaffoldOutcome::Created { created, skipped, trace_block }
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
                    tasks: vec![],
                    files: vec![],
                    acceptance: vec![],
                    satisfies: Vec::new(),
                },
                WavePlanEntry {
                    n: 2,
                    role: "frontend".to_string(),
                    summary: "ui pieces".to_string(),
                    depends_on: vec!["wave-1-general".to_string()],
                    tasks: vec![],
                    files: vec![],
                    acceptance: vec![],
                    satisfies: Vec::new(),
                },
            ],
            total_waves: Some(2),
            lang: Some("pt".to_string()),
        }
    }

    #[test]
    fn wave_plan_carries_acceptance_criteria_for_qa() {
        use crate::commands::spec::spec_sections;
        // EN locale for this AC-passthrough test — the AC heading is matched by
        // the i18n-aware `section_block`, so the carried section is found in
        // either language; EN keeps the literal block here readable.
        let hd = headings();
        let ac = "## Acceptance Criteria\n- **AC-1** — works.\n  Command: `true`";
        let md = render_wave_plan(&sample_plan(), &hd, Some(ac), "epic-x");
        // The QA gate reads global ACs back from `wave-plan.md` via the shared
        // `section_block` extractor once `spec.md` is renamed away — it must find
        // the carried section.
        let block = spec_sections::section_block(&md, "acceptanceCriteria")
            .expect("wave-plan must carry the AC section for the QA gate");
        assert!(block.contains("AC-1"));
        assert!(block.contains("Command: `true`"));

        // `None` (the /feature scaffold path, where `spec.md` survives) appends
        // no AC section — the table stays byte-identical.
        let bare = render_wave_plan(&sample_plan(), &hd, None, "epic-x");
        assert!(spec_sections::section_block(&bare, "acceptanceCriteria").is_none());
    }

    #[test]
    fn renders_wave_plan_table_with_wikilinks() {
        // The wave-plan is a MACHINE artefact, so its headings are ENGLISH-FIXED
        // regardless of the plan's declared `lang` (`sample_plan` declares
        // `lang: "pt"`).
        let hd = headings();
        let md = render_wave_plan(&sample_plan(), &hd, None, "epic-x");
        // `spec.`/`wave.`-prefixed — matches the `id:` each target actually
        // stamps (a bare `[[wave-1-general]]` never resolves).
        assert!(md.contains("[[wave.epic-x.1-general]]"));
        assert!(md.contains("[[wave.epic-x.2-frontend]]"));
        assert!(md.contains("foundations"));
        // The wave-plan carries its rename-proof identity handle as leading
        // `id:` frontmatter (parent slug + `.plan`).
        assert!(md.starts_with("---\nid: wave.epic-x.plan\n---\n\n"), "{md}");
        // English-fixed headings even for a pt-declared plan.
        assert!(md.contains("# Wave Plan"));
        assert!(md.contains("Depends on"));
        assert!(!md.contains("# Plano de Waves"));
    }

    #[test]
    fn renders_wave_spec_with_parent_link_and_no_header() {
        // Machine artefact → ENGLISH-FIXED headings (sample_plan declares `lang: "pt"`).
        let hd = headings();
        let plan = sample_plan();
        let s1 = render_wave_spec("epic-x", &plan.waves[0], &hd);
        // Identity (allowed) IS present as leading `id:` frontmatter, while
        // lifecycle metadata is NOT — no `### Stage:`/`### Parent:` header lines.
        // The two are distinct: `id:` is a rename-proof handle, lifecycle lives
        // in `meta.json`. The parent is surfaced only as a body link in `## Network`.
        assert!(s1.starts_with("---\nid: wave.epic-x.1-general\n---\n\n"), "{s1}");
        assert!(!s1.contains("### Stage:"));
        assert!(!s1.contains("### Outcome:"));
        assert!(!s1.contains("### Parent:"));
        assert!(s1.contains("## Network"));
        // `spec.`-prefixed — matches the `id: spec.{slug}` spec-draft actually
        // stamps on the root spec.md (a bare `[[epic-x]]` never resolves).
        assert!(s1.contains("[[spec.epic-x]]"));
        // English-fixed summary heading, never the PT form.
        assert!(s1.contains("## Summary"));
        assert!(!s1.contains("## Resumo"));
        let s2 = render_wave_spec("epic-x", &plan.waves[1], &hd);
        assert!(s2.starts_with("---\nid: wave.epic-x.2-frontend\n---\n\n"), "{s2}");
        assert!(!s2.contains("### Stage:"));
        assert!(s2.contains("[[wave.epic-x.1-general]]"));
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
        // and that no lifecycle header leaked into the markdown. The wave spec is
        // a MACHINE artefact, so its headings are ENGLISH-FIXED even though the
        // plan declares `lang: "pt"`.
        let s1 =
            std::fs::read_to_string(spec_dir.join("wave-1-general").join("spec.md")).unwrap();
        assert!(!s1.contains("### Stage:"));
        assert!(!s1.contains("### Parent:"));
        assert!(s1.contains("[[spec.epic-x]]"));
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
        // camelCase `dependsOn` survived deserialization. `wave.epic-camel.`-
        // prefixed to match the `id:` the target wave actually stamps.
        assert!(
            plan_md.contains("| frontend | [[wave.epic-camel.1-backend]] |"),
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

    /// A summary-only plan (predating the per-wave body fields) still
    /// deserialises — the explicit retrocompat affordance (`#[serde(default)]`).
    /// The 3 body fields default to empty and the rendered spec carries no Tasks
    /// / Files block (only `## Summary` + `## Network`, the historical output).
    #[test]
    fn summary_only_plan_still_deserialises_and_renders() {
        let raw = serde_json::to_string(&json!({
            "waves": [
                { "n": 1, "role": "general", "summary": "foundations", "depends_on": [] }
            ],
            "total_waves": 1,
            "lang": "en-US"
        }))
        .unwrap();
        let plan: Plan = serde_json::from_str(&raw).expect("summary-only plan deserialises");
        assert!(plan.waves[0].tasks.is_empty());
        assert!(plan.waves[0].files.is_empty());
        assert!(plan.waves[0].acceptance.is_empty());
        let hd = headings();
        let spec = render_wave_spec("epic", &plan.waves[0], &hd);
        assert!(spec.contains("## Summary"));
        assert!(spec.contains("## Network"));
        // No materialised body → no Tasks / Files heading.
        assert!(!spec.contains("## Tasks"), "no bare Tasks heading: {spec}");
        assert!(!spec.contains("## Files"), "no bare Files heading: {spec}");
    }

    /// Validation 3: `tasks` / `files` materialise into the wave spec as the
    /// localised `## Tasks` / `## Files` sections, and the body is consumable by
    /// `agent_prompt_render` — its `read_task_steps` / `files_section_paths`
    /// read the sections back as non-empty.
    #[test]
    fn render_wave_spec_materialises_tasks_and_files_consumable_by_agent_render() {
        use crate::commands::agent::agent_prompt_render as apr;
        let w = WavePlanEntry {
            n: 1,
            role: "backend".to_string(),
            summary: "the contract".to_string(),
            depends_on: vec![],
            tasks: vec!["wire the handler".to_string(), "add the route".to_string()],
            files: vec!["src/api/handler.rs".to_string(), "src/api/mod.rs".to_string()],
            acceptance: vec![],
            satisfies: Vec::new(),
        };
        let hd = headings();
        let spec = render_wave_spec("epic", &w, &hd);
        assert!(spec.contains("## Tasks"), "{spec}");
        assert!(spec.contains("- [ ] wire the handler"), "{spec}");
        assert!(spec.contains("- [ ] add the route"), "{spec}");
        assert!(spec.contains("## Files"), "{spec}");
        assert!(spec.contains("- `src/api/handler.rs`"), "{spec}");

        // Write the spec to disk and read it back through the agent-prompt-render
        // consumers to prove the materialised body is what the dispatch reads.
        let dir = tempdir().unwrap();
        let spec_path = dir.path().join("spec.md");
        std::fs::write(&spec_path, &spec).unwrap();
        let steps = apr::read_task_steps(&spec_path);
        assert!(!steps.trim().is_empty(), "task steps must be non-empty: {steps}");
        assert!(steps.contains("wire the handler"), "task body missing: {steps}");
        let files = apr::files_section_paths(&spec);
        assert!(
            files.contains(&"src/api/handler.rs".to_string()),
            "files section must be parsed back: {files:?}"
        );
    }

    /// Validation 4: per-wave `acceptance` reaches `wave-plan.md` and is found by
    /// `section_block(_, "acceptanceCriteria")`; a plan with no AC → no section.
    #[test]
    fn per_wave_acceptance_reaches_wave_plan_and_is_findable() {
        use crate::commands::spec::spec_sections;
        let plan = Plan {
            waves: vec![
                WavePlanEntry {
                    n: 1,
                    role: "backend".to_string(),
                    summary: "a".to_string(),
                    depends_on: vec![],
                    tasks: vec!["t1".to_string()],
                    files: vec![],
                    acceptance: vec!["**AC-1** — builds. Command: `true`".to_string()],
                    satisfies: Vec::new(),
                },
                WavePlanEntry {
                    n: 2,
                    role: "frontend".to_string(),
                    summary: "b".to_string(),
                    depends_on: vec!["wave-1-backend".to_string()],
                    tasks: vec!["t2".to_string()],
                    files: vec![],
                    acceptance: vec!["**AC-2** — renders. Command: `true`".to_string()],
                    satisfies: Vec::new(),
                },
            ],
            total_waves: Some(2),
            lang: Some("en-US".to_string()),
        };
        let hd = headings();
        let ac_block = build_ac_block(&plan, &hd);
        let md = render_wave_plan(&plan, &hd, ac_block.as_deref(), "epic-x");
        let block = spec_sections::section_block(&md, "acceptanceCriteria")
            .expect("AC union must be carried into wave-plan.md");
        assert!(block.contains("AC-1"), "{block}");
        assert!(block.contains("AC-2"), "{block}");

        // No-AC plan → no section (byte-stable output for summary-only plans).
        let mut no_ac = plan.clone();
        for w in &mut no_ac.waves {
            w.acceptance.clear();
        }
        let no_ac_block = build_ac_block(&no_ac, &hd);
        assert!(no_ac_block.is_none(), "no AC → no block synthesized");
        let bare = render_wave_plan(&no_ac, &hd, no_ac_block.as_deref(), "epic-x");
        assert!(spec_sections::section_block(&bare, "acceptanceCriteria").is_none());
    }

    /// Validation 5: the wave headings are ENGLISH-FIXED machine artefacts — a
    /// plan's declared `lang` no longer changes them. A pt-declared wave still
    /// renders `## Tasks`, never `## Tarefas`.
    #[test]
    fn wave_headings_are_english_fixed_regardless_of_lang() {
        let entry = |tasks: Vec<String>| WavePlanEntry {
            n: 1,
            role: "general".to_string(),
            summary: "s".to_string(),
            depends_on: vec![],
            tasks,
            files: vec![],
            acceptance: vec![],
            satisfies: Vec::new(),
        };
        let spec = render_wave_spec(
            "epic",
            &entry(vec!["fazer X".to_string()]),
            &headings(),
        );
        assert!(spec.contains("## Tasks"), "machine artefact → ## Tasks: {spec}");
        assert!(!spec.contains("## Tarefas"), "no PT heading even for a pt plan: {spec}");
    }

    /// Onda-1 fio pendente: a plan whose `tasks` already carry the checkbox
    /// prefix (`- [ ] foo` / `- [x] bar` / `- baz`) must render a SINGLE
    /// `- [ ]` per line in the wave spec — never the doubled `- [ ] - [ ]`
    /// form (measured in 3 real specs). The label is routed through the
    /// canonical `normalize_task_label` strip.
    #[test]
    fn checkbox_normalize_scaffold_prefixed_tasks_render_single_checkbox() {
        let w = WavePlanEntry {
            n: 1,
            role: "rt".to_string(),
            summary: "s".to_string(),
            depends_on: vec![],
            tasks: vec![
                "- [ ] wire the handler".to_string(),
                "- [x] already done item".to_string(),
                "- plain bullet item".to_string(),
                "bare label".to_string(),
            ],
            files: vec![],
            acceptance: vec![],
            satisfies: Vec::new(),
        };
        let spec = render_wave_spec("epic", &w, &headings());
        assert!(!spec.contains("- [ ] - [ ]"), "doubled checkbox: {spec}");
        assert!(!spec.contains("- [ ] - [x]"), "doubled checkbox: {spec}");
        assert!(!spec.contains("- [ ] - plain"), "doubled bullet: {spec}");
        assert!(spec.contains("- [ ] wire the handler"), "{spec}");
        assert!(spec.contains("- [ ] already done item"), "{spec}");
        assert!(spec.contains("- [ ] plain bullet item"), "{spec}");
        assert!(spec.contains("- [ ] bare label"), "{spec}");
    }

    /// Same invariant through the REAL plan-JSON path (`run` → deserialize →
    /// scaffold to disk): pre-prefixed tasks in plan.json never materialise the
    /// doubled `- [ ] - [ ]` form in the wave spec on disk.
    #[test]
    fn checkbox_normalize_scaffold_end_to_end_plan_json() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-prefixed");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "rt", "summary": "s", "depends_on": [],
                      "tasks": ["- [ ] do the thing", "clean label"] }
                ],
                "total_waves": 1,
                "lang": "en-US"
            }))
            .unwrap(),
        )
        .unwrap();

        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );

        let s1 = std::fs::read_to_string(spec_dir.join("wave-1-rt").join("spec.md")).unwrap();
        assert!(!s1.contains("- [ ] - [ ]"), "doubled checkbox on disk: {s1}");
        assert!(s1.contains("- [ ] do the thing"), "{s1}");
        assert!(s1.contains("- [ ] clean label"), "{s1}");
    }

    /// Task 1 (checklist-progresso-por-onda W2): the scaffold seeds each
    /// wave's `meta.json#checklist` with one `{label, path, done:false}` item
    /// per target file; the PARENT root meta carries NO checklist (explicit
    /// OUT). Skip-if-absent keeps an already-marked wave sidecar intact on
    /// re-scaffold.
    #[test]
    fn scaffold_seeds_wave_meta_checklist_from_files() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-checklist");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "rt", "summary": "s", "depends_on": [],
                      "tasks": ["wire it"],
                      "files": ["src/api/handler.rs", "  ", "src/api/mod.rs"] }
                ],
                "total_waves": 1,
                "lang": "en-US"
            }))
            .unwrap(),
        )
        .unwrap();

        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );

        let wave_meta =
            mustard_core::read_meta(&spec_dir.join("wave-1-rt").join("meta.json")).unwrap();
        assert_eq!(wave_meta.checklist.len(), 2, "one item per non-blank file");
        assert_eq!(wave_meta.checklist[0].path.as_deref(), Some("src/api/handler.rs"));
        assert_eq!(wave_meta.checklist[0].label, "src/api/handler.rs");
        assert!(!wave_meta.checklist[0].done, "seeded unchecked");
        assert_eq!(wave_meta.checklist[1].path.as_deref(), Some("src/api/mod.rs"));

        // Parent root meta carries no checklist key (OUT of scope).
        let root_text = std::fs::read_to_string(spec_dir.join("meta.json")).unwrap();
        assert!(!root_text.contains("\"checklist\""), "{root_text}");

        // Re-scaffold must not reset a flipped `done` (skip-if-absent).
        let wave_meta_path = spec_dir.join("wave-1-rt").join("meta.json");
        let mut marked = wave_meta.clone();
        marked.checklist[0].done = true;
        mustard_core::write_meta(&wave_meta_path, &marked).unwrap();
        run(
            Some(spec_dir.to_str().unwrap()),
            Some(plan_path.to_str().unwrap()),
        );
        let again = mustard_core::read_meta(&wave_meta_path).unwrap();
        assert!(again.checklist[0].done, "re-scaffold must preserve done state");
    }

    /// The empty-`tasks` retrocompat path: a wave with no checklist materialises
    /// no `## Tasks` heading (the WARN is the visible signal, emitted in `run`).
    #[test]
    fn empty_tasks_emits_no_bare_heading() {
        let w = WavePlanEntry {
            n: 1,
            role: "general".to_string(),
            summary: "s".to_string(),
            depends_on: vec![],
            tasks: vec![],
            files: vec![],
            acceptance: vec![],
            satisfies: Vec::new(),
        };
        let spec = render_wave_spec("epic", &w, &headings());
        assert!(!spec.contains("## Tasks"), "bare empty Tasks heading is noise: {spec}");
    }

    /// F6 traceability: a wave that does work (`tasks`) but satisfies no AC is a
    /// gap; a well-traced wave (satisfies its own acceptance ids) is clean; and
    /// an AC the plan defines that no wave's `satisfies` claims is an orphan gap.
    #[test]
    fn traceability_gaps_flags_untraced_work_and_orphan_acs() {
        let wave = |tasks: Vec<&str>, acceptance: Vec<&str>, satisfies: Vec<&str>| WavePlanEntry {
            n: 1,
            role: "backend".to_string(),
            summary: "s".to_string(),
            depends_on: vec![],
            tasks: tasks.into_iter().map(String::from).collect(),
            files: vec![],
            acceptance: acceptance.into_iter().map(String::from).collect(),
            satisfies: satisfies.into_iter().map(String::from).collect(),
        };
        let plan = |w: WavePlanEntry| Plan { waves: vec![w], total_waves: Some(1), lang: None };

        // (a) tasks but no AC → untraced-wave gap naming the wave (Gap 1).
        let gaps = traceability_gaps(&plan(wave(vec!["do the thing"], vec![], vec![])), None);
        assert!(
            gaps.untraced_waves.iter().any(|g| g.contains("wave-1-backend") && g.contains("satisfies no AC")),
            "wave with tasks but no AC must be a gap: {:?}",
            gaps.untraced_waves
        );
        assert!(gaps.uncovered_acs.is_empty(), "no defined ACs → no uncovered gap");
        // (b) declares AND satisfies its own AC → clean on both axes.
        let clean = traceability_gaps(
            &plan(wave(
                vec!["do it"],
                vec!["**AC-1** — works. Command: `true`"],
                vec!["AC-1"],
            )),
            None,
        );
        assert!(clean.untraced_waves.is_empty() && clean.uncovered_acs.is_empty(), "well-traced wave is clean");
        // (c) defines AC-1 but satisfies only AC-2 → AC-1 is an uncovered gap (Gap 2).
        let orphan = traceability_gaps(
            &plan(wave(
                vec!["do it"],
                vec!["**AC-1** — works. Command: `true`"],
                vec!["AC-2"],
            )),
            None,
        );
        assert!(
            orphan.uncovered_acs.iter().any(|g| g.contains("AC-1") && g.contains("no wave satisfies it")),
            "an AC defined but unsatisfied is an orphan gap: {:?}",
            orphan.uncovered_acs
        );
    }

    /// A parent spec.md `## Acceptance Criteria` id that NO wave claims (neither
    /// `satisfies` nor an `acceptance` line) fires the uncovered-criterion gap —
    /// even for a plan that carries no per-wave `acceptance` of its own. The
    /// parent ACs are read through the shared qa-run `extract_ac_section` +
    /// `parse_ac_items`, never a forked reader.
    #[test]
    fn traceability_gaps_parent_spec_ac_uncovered_fires_gap() {
        let parent = "# Epic\n\n## Acceptance Criteria\n\
- **AC-1** — first. Command: `true`\n\
- **AC-2** — second. Command: `true`\n";
        // One wave that claims only AC-1 (via an explicit satisfies).
        let plan = Plan {
            waves: vec![WavePlanEntry {
                n: 1,
                role: "backend".to_string(),
                summary: "s".to_string(),
                depends_on: vec![],
                tasks: vec!["do it".to_string()],
                files: vec![],
                acceptance: vec![],
                satisfies: vec!["AC-1".to_string()],
            }],
            total_waves: Some(1),
            lang: None,
        };
        let gaps = traceability_gaps(&plan, Some(parent));
        // AC-2 is defined by the parent but claimed by no wave → uncovered.
        assert!(
            gaps.uncovered_acs.iter().any(|g| g.contains("AC-2")),
            "parent AC-2 absent from every wave must fire the gap: {:?}",
            gaps.uncovered_acs
        );
        // AC-1 is covered → never flagged.
        assert!(
            !gaps.uncovered_acs.iter().any(|g| g.contains("AC-1")),
            "the satisfied AC-1 must not be a gap: {:?}",
            gaps.uncovered_acs
        );
    }

    /// A parent spec.md AC id that a wave carries in its own `acceptance` line
    /// (no explicit `satisfies`) counts as covered — the back-compat path where
    /// a wave's satisfied set is derived from its acceptance ids. So a covered
    /// criterion never escalates.
    #[test]
    fn traceability_gaps_parent_spec_ac_covered_via_acceptance_counts() {
        let parent = "# Epic\n\n## Acceptance Criteria\n- **AC-1** — first. Command: `true`\n";
        let plan = Plan {
            waves: vec![WavePlanEntry {
                n: 1,
                role: "backend".to_string(),
                summary: "s".to_string(),
                depends_on: vec![],
                tasks: vec!["do it".to_string()],
                files: vec![],
                // Same id via an acceptance line, NOT an explicit satisfies.
                acceptance: vec!["**AC-1** — first. Command: `true`".to_string()],
                satisfies: vec![],
            }],
            total_waves: Some(1),
            lang: None,
        };
        let gaps = traceability_gaps(&plan, Some(parent));
        assert!(
            gaps.uncovered_acs.is_empty(),
            "AC-1 covered via the wave's acceptance line must not fire the gap: {:?}",
            gaps.uncovered_acs
        );
        // The wave does work AND traces a criterion → no untraced-wave gap.
        assert!(gaps.untraced_waves.is_empty(), "wave traces AC-1 → not untraced");
    }

    /// End-to-end through `scaffold` under the DEFAULT mode (strict, env unset):
    /// a parent spec.md whose AC-2 no wave claims makes `scaffold` return a
    /// non-empty `trace_block` — the list [`run`] maps to a non-zero exit. The
    /// layout is still materialised (idempotent); the block is the data the
    /// caller actions.
    #[test]
    fn scaffold_parent_spec_ac_strict_blocks() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-trace");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // The parent monolithic spec defines two criteria.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Epic\n\n## Acceptance Criteria\n- **AC-1** — a. Command: `true`\n- **AC-2** — b. Command: `true`\n",
        )
        .unwrap();
        // The plan routes only AC-1 onto a wave.
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "backend", "summary": "s", "tasks": ["do it"], "satisfies": ["AC-1"] }
                ],
                "total_waves": 1,
                "lang": "en-US"
            }))
            .unwrap(),
        )
        .unwrap();

        match scaffold(&spec_dir, &plan_path) {
            ScaffoldOutcome::Created { created, trace_block, .. } => {
                // The layout was still materialised.
                assert!(created.iter().any(|f| f == "wave-plan.md"), "created: {created:?}");
                // strict (default) → the uncovered parent AC-2 blocks.
                assert!(
                    trace_block.iter().any(|g| g.contains("AC-2")),
                    "strict mode must carry the uncovered AC-2 into trace_block: {trace_block:?}"
                );
                assert!(
                    !trace_block.iter().any(|g| g.contains("AC-1")),
                    "the covered AC-1 must not block: {trace_block:?}"
                );
            }
            _ => panic!("expected ScaffoldOutcome::Created"),
        }
    }

    /// warn / off modes do NOT block: `trace_block_for` returns an empty list
    /// even when a parent AC is uncovered, so [`run`] exits 0. The gap is still
    /// detected (`traceability_gaps` surfaces it — the stderr WARN `scaffold`
    /// prints), it just no longer fails the command. strict is the only
    /// escalating mode.
    #[test]
    fn trace_block_for_parent_spec_ac_warn_passes() {
        let uncovered = vec!["AC-2 — no wave satisfies it".to_string()];
        // strict escalates (blocks) — carries the uncovered AC through.
        assert_eq!(
            trace_block_for(GateMode::Strict, uncovered.clone()).len(),
            1,
            "strict carries the uncovered AC into the block"
        );
        // warn and off pass (no block), leaving the WARN as the only signal.
        assert!(trace_block_for(GateMode::Warn, uncovered.clone()).is_empty(), "warn does not block");
        assert!(trace_block_for(GateMode::Off, uncovered).is_empty(), "off does not block");
    }

    /// The `satisfies` field deserialises from a hand-authored plan.json and
    /// defaults to empty for a plan that predates it (retrocompat).
    #[test]
    fn satisfies_field_deserialises_and_defaults_empty() {
        let raw = serde_json::to_string(&json!({
            "waves": [
                { "n": 1, "role": "backend", "summary": "s", "satisfies": ["AC-1", "AC-3"] },
                { "n": 2, "role": "frontend", "summary": "s" }
            ],
            "total_waves": 2
        }))
        .unwrap();
        let plan: Plan = serde_json::from_str(&raw).expect("plan with satisfies deserialises");
        assert_eq!(plan.waves[0].satisfies, vec!["AC-1".to_string(), "AC-3".to_string()]);
        assert!(plan.waves[1].satisfies.is_empty(), "absent satisfies defaults to empty");
    }
}
