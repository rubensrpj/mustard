//! The wave-scaffold renderer — the canonical SDD wave layout for a spec,
//! rendered from a declarative JSON plan.
//!
//! NOT a `run` subcommand: it was absorbed into
//! [`crate::commands::pipeline::plan_materialize`], which is the ONLY published
//! entry point (`mustard-rt run plan-materialize --spec-dir <dir> --plan
//! plan.json`) and calls [`scaffold`] in-process.
//!
//! Part of the wave-network spec (`2026-05-20-mustard-wave-network-standard`).
//! The SKILL `/feature` generates the plan JSON during PLAN; this renderer
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
//! file block (the empty-tasks case emits a visible stderr WARN — see
//! [`scaffold`]).
//!
//! `lang` accepts BCP-47 (`pt-BR` / `en-US`); the legacy short forms
//! (`pt` / `en`) are tolerated on read for back-compat with old plan JSON
//! and normalised to BCP-47 in the rendered headings. The *effective* heading
//! language follows the project's `mustard.json#specLang` (root wins) when the
//! scaffold runs inside a workspace; the plan's `lang` is the fallback for a
//! standalone scaffold. Every generated artefact (headings, placeholders) is
//! rendered in that effective language per the i18n rule.
//!
//! Idempotent, in both write modes (see [`WriteMode`]): re-running an UNCHANGED
//! plan creates, refreshes and removes nothing. Before the user approves the
//! spec the layout is reconciled onto the plan (a differing file is rewritten,
//! a wave the plan dropped is deleted); once `.approved-by-user` exists it is
//! frozen — skip-if-present, with ONE stderr WARN when a file would have
//! changed. `plan-materialize` reports `created_files` / `skipped` /
//! `refreshed` / `removed` on stdout.

use crate::shared::context::APPROVED_BY_USER_MARKER;
use mustard_core::domain::spec::contract::ChecklistItem;
use mustard_core::io::fs;
use mustard_core::{Meta, MetaFlags, read_meta, write_meta};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

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
    /// empty case is surfaced by a visible stderr WARN in [`scaffold`] rather
    /// than a silent empty heading.
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
/// [`scaffold`] via a stderr WARN, not here — an empty task block emits **no**
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
/// surface the uncovered-criterion gap (the coverage gate `plan-materialize`
/// enforces) while the untraced-wave signal stays a non-blocking WARN.
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

/// Write mode for one scaffold pass, decided by the canonical per-spec approval
/// marker (`.approved-by-user`) — never by a flag or an env knob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteMode {
    /// No approval marker yet: the layout is still the Plan agent's draft, so
    /// every artefact is a pure function of the plan. A rendered body that
    /// differs from disk is REWRITTEN (reported under `refreshed`) and a
    /// `wave-N-*` directory the plan no longer declares is DELETED (reported
    /// under `removed`) — so re-running `plan-materialize` after fixing
    /// `plan.json` repairs the scaffold instead of leaving stale files behind.
    Reconcile,
    /// The plan left the authoring window: the layout is FROZEN. Skip-if-present,
    /// byte-for-byte the historical behaviour, and nothing is ever deleted — a
    /// would-be change surfaces as ONE stderr WARN naming the change-request
    /// route.
    Frozen,
}

/// Decide the write mode. [`WriteMode::Reconcile`] requires the pass to be
/// inside the PLAN AUTHORING window — all three facts, each read from state the
/// orchestrator cannot assert by hand:
///
/// 1. No `.approved-by-user` ([`APPROVED_BY_USER_MARKER`]). The marker can only
///    be born from the user's real approval answer.
/// 2. The root `meta.json#stage` has not advanced past `Plan`. A spec already in
///    EXECUTE has agents editing against these wave dirs and `done` flags the
///    auto-mark hook flipped — rewriting them from a plan is never repair there.
///    An ABSENT sidecar is the fresh-scaffold case and stays reconcilable.
/// 3. No `scopeOverride: "user-rejected-waves"` — `wave-collapse` stamps that
///    when the user explicitly REJECTED the decomposition and merged the waves
///    back down by hand. Reconciling from the pre-collapse plan would delete
///    exactly the merge the user asked for.
///
/// Rewriting and pruning are destructive; anything short of all three facts
/// falls back to the historical skip-if-present behaviour.
///
/// This governs the CONTENT (bodies, sidecars, the pruner). The root sidecar's
/// structural wave count is a narrower question — see [`write_parent_meta`],
/// which freezes on fact 1 alone.
fn write_mode(spec_dir: &Path) -> WriteMode {
    if is_approved(spec_dir) {
        return WriteMode::Frozen;
    }
    let Some(meta) = read_meta(&spec_dir.join("meta.json")) else {
        // No sidecar yet — a fresh scaffold, nothing to protect.
        return WriteMode::Reconcile;
    };
    let past_plan = meta
        .stage
        .as_deref()
        .is_some_and(|s| !s.trim().eq_ignore_ascii_case("Plan"));
    let waves_rejected = meta
        .raw
        .get("scopeOverride")
        .and_then(Value::as_str)
        .is_some_and(|s| s == "user-rejected-waves");
    if past_plan || waves_rejected {
        WriteMode::Frozen
    } else {
        WriteMode::Reconcile
    }
}

/// `true` when the user really approved this spec ([`APPROVED_BY_USER_MARKER`]
/// beside it). The one fact the orchestrator cannot assert by hand.
fn is_approved(spec_dir: &Path) -> bool {
    fs::exists(spec_dir.join(APPROVED_BY_USER_MARKER))
}

/// The exact bytes [`write_meta`] would put on disk for `meta` — pretty JSON
/// plus the trailing newline.
///
/// Used ONLY to decide whether a sidecar already matches the plan;
/// [`write_meta`] stays the single writer. If the two shapes ever drift, the
/// idempotency test (`composite_plan_materialize_scaffolds_validates_and_emits`,
/// which asserts an unchanged plan refreshes nothing) is the alarm.
fn render_meta(meta: &Meta) -> Option<String> {
    serde_json::to_string_pretty(meta).ok().map(|mut s| {
        s.push('\n');
        s
    })
}

/// `true` when `name` is a scaffolded wave directory (`wave-<n>-<role>`) — the
/// `wave-` prefix followed by a digit, the same shape `wave-size-check` and the
/// review-role derivation enumerate. Everything else under the spec root (the
/// root `spec.md` / `meta.json` / `wave-plan.md`, plus the `.events/`, `qa/`
/// and `review/` phase folders) is therefore invisible to the pruner.
fn is_wave_dir(name: &str) -> bool {
    name.to_ascii_lowercase()
        .strip_prefix("wave-")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// Per-file bookkeeping for one scaffold pass — the lists [`ScaffoldOutcome`]
/// carries, plus the frozen-plan drift flag [`scaffold`] turns into its single
/// stderr WARN.
struct Ledger<'a> {
    /// Spec root every recorded path is made relative to.
    spec_dir: &'a Path,
    mode: WriteMode,
    created: Vec<String>,
    skipped: Vec<String>,
    refreshed: Vec<String>,
    removed: Vec<String>,
    /// [`WriteMode::Frozen`] only: at least one artefact WOULD have changed.
    drift: bool,
}

impl<'a> Ledger<'a> {
    fn new(spec_dir: &'a Path, mode: WriteMode) -> Self {
        Self {
            spec_dir,
            mode,
            created: Vec::new(),
            skipped: Vec::new(),
            refreshed: Vec::new(),
            removed: Vec::new(),
            drift: false,
        }
    }

    /// `path` relative to the spec root, forward-slashed so the JSON report is
    /// identical on every platform.
    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(self.spec_dir).map_or_else(
            |_| path.to_string_lossy().to_string(),
            |p| p.to_string_lossy().replace('\\', "/"),
        )
    }

    /// Materialise one rendered markdown artefact.
    ///
    /// Absent → written, recorded under `created` (both modes — restoring a
    /// missing artefact is what the dashboard's broken-wave-link hint asks for;
    /// under [`WriteMode::Frozen`] it also raises the drift flag, because a file
    /// appearing under an approved plan CHANGES that layout and the operator
    /// must hear about it). Present and byte-identical → `skipped`. Present and
    /// DIFFERENT → rewritten + `refreshed` under [`WriteMode::Reconcile`]; left
    /// untouched + `skipped` with drift raised under [`WriteMode::Frozen`]. A
    /// write failure degrades to `skipped` — the historical `write_if_absent`
    /// contract — but says so on stderr instead of leaving a silent gap.
    fn emit(&mut self, path: &Path, body: &str) {
        let rel = self.rel(path);
        if !fs::exists(path) {
            if fs::write_atomic(path, body.as_bytes()).is_ok() {
                if self.mode == WriteMode::Frozen {
                    self.drift = true;
                }
                self.created.push(rel);
            } else {
                eprintln!("[wave-scaffold] WARN: could not write {}", path.display());
                self.skipped.push(rel);
            }
            return;
        }
        // An existing file that cannot be read counts as "differs": Reconcile
        // rewrites it from the plan, Frozen still leaves it alone.
        if fs::read_to_string(path).ok().as_deref() == Some(body) {
            self.skipped.push(rel);
            return;
        }
        match self.mode {
            WriteMode::Reconcile if fs::write_atomic(path, body.as_bytes()).is_ok() => {
                self.refreshed.push(rel);
            }
            WriteMode::Reconcile => {
                eprintln!(
                    "[wave-scaffold] WARN: could not rewrite {} — it stays STALE relative to the plan",
                    path.display()
                );
                self.skipped.push(rel);
            }
            WriteMode::Frozen => {
                self.drift = true;
                self.skipped.push(rel);
            }
        }
    }

    /// Materialise one per-wave `meta.json` sidecar, mirroring [`Self::emit`]'s
    /// reconcile-vs-freeze decision.
    ///
    /// The sidecars stay OUT of `created`/`skipped` — those two lists have only
    /// ever carried the markdown artefacts and `plan-materialize` publishes them
    /// verbatim. A sidecar therefore only ever appears under `refreshed`, when
    /// an unapproved plan actually rewrote it (resetting a `done` flag the
    /// auto-mark hook flipped is the point: before approval the checklist is a
    /// function of the plan's file census, and EXECUTE cannot have started).
    fn emit_meta(&mut self, dir: &Path, meta: &Meta) {
        let path = dir.join("meta.json");
        let _ = fs::create_dir_all(dir);
        let write = |path: &Path| {
            if let Err(e) = write_meta(path, meta) {
                eprintln!(
                    "[wave-scaffold] WARN: could not write {} ({e})",
                    path.display()
                );
                return false;
            }
            true
        };
        if !fs::exists(&path) {
            write(&path);
            return;
        }
        // No renderable form → cannot prove a difference; leave the sidecar be.
        let Some(body) = render_meta(meta) else {
            return;
        };
        if fs::read_to_string(&path).ok().as_deref() == Some(body.as_str()) {
            return;
        }
        match self.mode {
            WriteMode::Reconcile => {
                if write(&path) {
                    let rel = self.rel(&path);
                    self.refreshed.push(rel);
                }
            }
            WriteMode::Frozen => self.drift = true,
        }
    }

    /// Delete `wave-N-*` directories present on disk but absent from `planned`,
    /// recording each under `removed`.
    ///
    /// [`WriteMode::Reconcile`] only — an approved layout is never pruned. Only
    /// wave DIRECTORIES are considered ([`is_wave_dir`]), so the root `spec.md`,
    /// `meta.json`, `wave-plan.md`, `.events/`, `qa/` and `review/` can never be
    /// touched. Fail-open: a directory that refuses to go warns and is not
    /// reported as removed.
    fn prune_stale_waves(&mut self, planned: &BTreeSet<String>) {
        if self.mode != WriteMode::Reconcile {
            return;
        }
        let Ok(entries) = fs::read_dir(self.spec_dir) else {
            return;
        };
        for entry in entries {
            if !entry.is_dir
                || !is_wave_dir(&entry.file_name)
                || planned.contains(&entry.file_name)
            {
                continue;
            }
            if fs::remove_dir_all(&entry.path).is_ok() {
                self.removed.push(entry.file_name.clone());
            } else {
                eprintln!(
                    "[wave-scaffold] WARN: could not remove stale wave dir {}",
                    entry.path.display()
                );
            }
        }
        self.removed.sort();
    }
}

/// The single stderr WARN a FROZEN pass emits when the plan renders something
/// the approved layout does not carry.
///
/// It names the route that DOES accept a change on an approved spec — stating
/// it in chat, which the change-request observer records in the spec's
/// `change-log.md` — because silently re-planning an approved spec is exactly
/// what the approval marker exists to prevent. Composed here (rather than
/// inlined at the `eprintln!`) so the wording is assertable.
fn frozen_plan_warn() -> String {
    format!(
        "[wave-scaffold] WARN: the plan does not match the layout on disk, which is FROZEN \
         ({APPROVED_BY_USER_MARKER} exists, or the spec left PLAN, or the waves were \
         user-rejected). No existing file was rewritten, no wave was pruned and the wave count \
         was left as approved; only artefacts missing from disk were restored. Route the change \
         through a change request (state it in chat; it is recorded in the spec's \
         change-log.md), never through a silent re-plan."
    )
}

/// The minimal valid plan appended to BOTH unreadable-plan messages, plus the
/// pointer to the authoritative schema. stderr only — the `plan-materialize`
/// stdout keeps its stable `error: "plan unreadable"` marker.
const PLAN_SCHEMA_HINT: &str = concat!(
    "[wave-scaffold] the minimal plan JSON this command accepts:\n",
    "{\n",
    "  \"waves\": [\n",
    "    { \"n\": 1, \"role\": \"general\", \"summary\": \"one line\",\n",
    "      \"depends_on\": [],\n",
    "      \"tasks\": [\"wire the contract\"], \"files\": [\"src/api/handler.rs\"],\n",
    "      \"acceptance\": [\"**AC-1** - handler returns 200. Command: `curl -sf ...`\"],\n",
    "      \"satisfies\": [\"AC-1\"] }\n",
    "  ],\n",
    "  \"total_waves\": 1, \"lang\": \"en-US\"\n",
    "}\n",
    "[wave-scaffold] full schema: the /feature reference full-plan.md, \
     section `Plan JSON schema`",
);

/// Outcome of one scaffold pass — the miolo result `plan-materialize` folds
/// into its composite report.
pub(crate) enum ScaffoldOutcome {
    /// The layout was materialised (idempotently). `uncovered_acs` lists the
    /// parent/plan acceptance criteria that no wave covers — the coverage gate
    /// `plan-materialize` enforces (a non-empty list blocks the PLAN transition
    /// so the orchestrator notices the untraced criterion). Always the real
    /// list; there is no mode knob.
    Created {
        created: Vec<String>,
        skipped: Vec<String>,
        /// Artefacts whose rendered body differed from disk and were REWRITTEN
        /// from the plan ([`WriteMode::Reconcile`] only). Sorted, and always
        /// present (empty when nothing changed) so stdout stays byte-stable.
        refreshed: Vec<String>,
        /// `wave-N-*` directories deleted because the plan no longer declares
        /// them ([`WriteMode::Reconcile`] only). Sorted, and always present.
        removed: Vec<String>,
        uncovered_acs: Vec<String>,
    },
    /// `plan.waves` was empty — operator error (W10.T10.3 hard gate).
    EmptyPlan,
    /// The plan file could not be read or parsed; carries the stderr message
    /// (which teaches [`PLAN_SCHEMA_HINT`]).
    Unreadable(String),
}


/// Materialise the wave layout for an already-resolved `spec_dir` + `plan_path`.
///
/// The non-printing renderer behind
/// [`crate::commands::pipeline::plan_materialize`], called in-process (no
/// subprocess). Warnings (declared-total mismatch, empty-tasks waves, a frozen
/// plan that would have changed) go to stderr; the result is returned typed
/// instead of printed.
///
/// Write mode follows the spec's approval marker — see [`WriteMode`]: an
/// UNAPPROVED layout is reconciled onto the plan (rewrite what differs, prune
/// waves the plan dropped), an APPROVED one is frozen.
pub(crate) fn scaffold(spec_dir: &Path, plan_path: &Path) -> ScaffoldOutcome {
    let raw = match fs::read_to_string(plan_path) {
        Ok(t) => t,
        Err(e) => {
            return ScaffoldOutcome::Unreadable(format!(
                "[wave-scaffold] cannot read plan {}: {e}\n{PLAN_SCHEMA_HINT}",
                plan_path.display()
            ));
        }
    };
    let plan: Plan = match serde_json::from_str::<Plan>(&raw) {
        Ok(p) => p,
        Err(e) => {
            return ScaffoldOutcome::Unreadable(format!(
                "[wave-scaffold] plan JSON parse error: {e}\n{PLAN_SCHEMA_HINT}"
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

    // Decided ONCE, before anything is written: the same mode governs the
    // markdown, the per-wave sidecars, the pruner and the root sidecar.
    let mode = write_mode(spec_dir);
    let mut ledger = Ledger::new(spec_dir, mode);

    // Before rendering anything, drop the wave directories this plan no longer
    // declares (Reconcile only) — the repair path the field report asked for:
    // fix `plan.json`, re-run `plan-materialize`, no hand deletion.
    let planned: BTreeSet<String> = plan.waves.iter().map(wave_name).collect();
    ledger.prune_stale_waves(&planned);

    // Synthesize the global Acceptance Criteria block from the per-wave
    // `acceptance` arrays. When any wave carries AC, their union is written into
    // `wave-plan.md` under the localised `## Acceptance Criteria` heading, so the
    // QA gate still reads them via `section_block(_, "acceptanceCriteria")`. When
    // NO wave carries AC, `None` is passed and the wave-plan output stays
    // byte-stable for summary-only (pre-body) plans.
    let ac_block = build_ac_block(&plan, &hd);

    // wave-plan.md.
    let wave_plan_md = render_wave_plan(&plan, &hd, ac_block.as_deref(), &parent_name);
    ledger.emit(&spec_dir.join("wave-plan.md"), &wave_plan_md);

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
        ledger.emit(
            &dir.join("spec.md"),
            &render_wave_spec(&parent_name, w, &hd),
        );
    }

    // AC↔wave traceability (F6): the untraced-wave signal (Gap 1) stays a
    // non-blocking WARN; the uncovered-criterion signal (Gap 2 — an AC of the
    // plan OR the parent spec.md that no wave claims) is the coverage gate,
    // ENFORCED by `plan-materialize` (the pipeline entry) — no env knob. The
    // parent monolithic spec is `spec.md` at PLAN time, or `spec.original.md`
    // once a rewave archived it; an absent parent contributes no ids.
    let parent_ac_md = fs::read_to_string(spec_dir.join("spec.md"))
        .or_else(|_| fs::read_to_string(spec_dir.join("spec.original.md")))
        .ok();
    let gaps = traceability_gaps(&plan, parent_ac_md.as_deref());
    for gap in &gaps.untraced_waves {
        eprintln!("[wave-scaffold] WARN: {gap}");
    }
    for gap in &gaps.uncovered_acs {
        eprintln!("[wave-scaffold] WARN: {gap}");
    }
    // `scaffold` never exits — it stays reusable in-process (plan-materialize),
    // which blocks the PLAN transition when this list is non-empty.
    let uncovered_acs = gaps.uncovered_acs;

    // Wave 3 of mustard-unification: emit `meta.json` alongside every spec.md
    // we just wrote so consumers can read lifecycle metadata as structured
    // JSON instead of regexing the markdown. Fail-open per file.
    // `total_waves` is the count we ACTUALLY scaffold — one wave dir + one
    // `wave-plan.md` row per `plan.waves` entry. Derive it from `plan.waves.len()`,
    // NOT the declared `plan.total_waves` (only cross-checked / WARNed above): a
    // plan that declares a stale total must not poison the sidecar the dashboard
    // and `status` render the wave count from.
    let total_waves = plan.waves.len() as u32;
    let parent_drift = write_parent_meta(
        spec_dir,
        is_approved(spec_dir),
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
        ledger.emit_meta(
            &wave_dir,
            &Meta {
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
                // target file. Once the plan is approved the sidecar is FROZEN,
                // so a re-scaffold never resets `done` flags already flipped by
                // the auto-mark hook / `mark-checklist-item`; before approval it
                // is reconciled back onto the plan's census (EXECUTE cannot have
                // started, so there is no progress to lose).
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

    let Ledger { created, skipped, mut refreshed, removed, drift, .. } = ledger;
    // A frozen plan says so ONCE — for any kind of divergence, including a wave
    // count the approved layout does not carry — and names the route that does
    // accept a change.
    if drift || parent_drift {
        eprintln!("{}", frozen_plan_warn());
    }
    // Sorted so stdout stays byte-stable regardless of directory-read order.
    refreshed.sort();
    ScaffoldOutcome::Created { created, skipped, refreshed, removed, uncovered_acs }
}

/// Write / reconcile the wave-plan PARENT `meta.json` (the wave-plan root).
///
/// Unlike the per-wave sidecars ([`Ledger::emit_meta`]), the parent typically
/// already exists: `spec-draft` creates it at PLAN time with an *estimated*
/// `total_waves` (the Full floor of ≥1, before the real plan is known). The
/// scaffold is the authoritative source of the real wave count, so it must
/// reconcile `total_waves` + `isWavePlan` — and UPGRADE a non-Full `scope` to
/// the wave-plan scope (a wave-plan parent is Full by construction) — onto
/// whatever the pipeline has advanced the file to, preserving every OTHER
/// lifecycle field (`stage` / `outcome` / `phase` / `lang` / `checkpoint` /
/// `flags` / `raw`). Skipping the count reconcile (the old behaviour) left a
/// stale `totalWaves: 1` on multi-wave epics; skipping the scope upgrade left a
/// `light` parent (drafted before `plan-prepare` bumped it Full) that
/// `scope_guard`'s Full gate never recognised.
///
/// Frozen by the APPROVAL MARKER ALONE (`approved`), not by the full
/// [`write_mode`] window: once the user approved, the stored count is the count
/// they approved and stays put (returning `true` — drift — instead of writing).
/// Bumping it there produced a spec whose `wave-plan.md` listed N waves while
/// the sidecar every consumer reads (`wave-advance`, `status`, the dashboard)
/// claimed N+1 — an approved spec silently growing a wave, the mirror of what
/// the marker exists to prevent.
///
/// The narrower gate is deliberate. This reconcile is STRUCTURAL and
/// non-destructive (two fields; no body, no deletion), and skipping it is itself
/// a known defect: a stale `totalWaves: 1` on a multi-wave epic mis-renders the
/// dashboard and `status`. So an unapproved spec that already left PLAN — frozen
/// for content, because agents are editing there — still gets its count
/// corrected.
///
/// Fail-open: a write failure warns on stderr and never panics.
fn write_parent_meta(dir: &Path, approved: bool, fresh: Meta) -> bool {
    let path = dir.join("meta.json");
    let meta = match read_meta(&path) {
        // Reconcile the structural wave-plan fields; the OTHER lifecycle fields
        // the pipeline owns (and may have advanced past Plan) are preserved.
        Some(mut existing) => {
            // A wave-plan parent is Full-scope BY CONSTRUCTION. When the pipeline
            // left a non-Full scope on it — e.g. `spec-draft` drafted `light`
            // before `plan-prepare` bumped the unit to Full — upgrade it, else
            // `scope_guard`'s Full gate (which matches the `full` /
            // `full (wave plan)` string) never engages on the parent and the
            // "no code before /approve" hard-gate silently opens. An already-Full
            // scope is left untouched — no churn, no false drift.
            let scope_upgrade = existing
                .scope
                .as_deref()
                .map_or(true, |s| !s.starts_with("full"));
            if approved
                && (existing.total_waves != fresh.total_waves
                    || existing.is_wave_plan != fresh.is_wave_plan
                    || scope_upgrade)
            {
                return true;
            }
            existing.is_wave_plan = fresh.is_wave_plan;
            existing.total_waves = fresh.total_waves;
            if scope_upgrade {
                existing.scope = fresh.scope;
            }
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
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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

        let _ = scaffold(&spec_dir, &plan_path);

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
        let _ = scaffold(&spec_dir, &plan_path);
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

        let _ = scaffold(&spec_dir, &plan_path);

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

        let _ = scaffold(&spec_dir, &plan_path);

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
        let _ = scaffold(&spec_dir, &plan_path);
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

        let _ = scaffold(&spec_dir, &plan_path);

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

    /// Regression (light→full recovery): when `spec-draft` drafted the parent at
    /// `light` and `plan-prepare` then classified the unit Full, `plan-materialize`
    /// builds a multi-wave plan onto a `light` parent. The scaffold MUST upgrade
    /// the parent `scope` to `full (wave plan)` — else `scope_guard`'s Full gate
    /// (which matches the `full` string) never engages and the "no code before
    /// /approve" hard-gate silently opens. Other lifecycle fields survive; an
    /// already-`full` scope is left untouched (covered by the stale-total test).
    #[test]
    fn upgrades_light_parent_scope_to_full_wave_plan() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-light-draft");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // A parent `spec-draft` wrote at `light` (unapproved), before
        // `plan-prepare` bumped the unit to Full and a 2-wave plan was authored.
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Plan","outcome":"Active","phase":"PLAN","scope":"light","lang":"en-US","isWavePlan":true,"totalWaves":2}"#,
        )
        .unwrap();
        let plan_path = dir.path().join("plan.json");
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": [
                    { "n": 1, "role": "backend", "summary": "a", "depends_on": [] },
                    { "n": 2, "role": "backend", "summary": "b", "depends_on": ["wave-1-backend"] }
                ],
                "total_waves": 2,
                "lang": "en-US"
            }))
            .unwrap(),
        )
        .unwrap();

        let _ = scaffold(&spec_dir, &plan_path);

        let root = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();
        // The `light` draft is upgraded so `scope_guard`'s Full gate recognises it.
        assert_eq!(
            root.scope.as_deref(),
            Some("full (wave plan)"),
            "a light-drafted wave-plan parent must be upgraded to Full"
        );
        // Other lifecycle fields survive the reconcile.
        assert_eq!(root.stage.as_deref(), Some("Plan"));
        assert_eq!(root.is_wave_plan, Some(true));
        assert_eq!(root.total_waves, Some(2));
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

        let _ = scaffold(&spec_dir, &plan_path);

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

        let _ = scaffold(&spec_dir, &plan_path);

        let s1 = std::fs::read_to_string(spec_dir.join("wave-1-rt").join("spec.md")).unwrap();
        assert!(!s1.contains("- [ ] - [ ]"), "doubled checkbox on disk: {s1}");
        assert!(s1.contains("- [ ] do the thing"), "{s1}");
        assert!(s1.contains("- [ ] clean label"), "{s1}");
    }

    /// Task 1 (checklist-progresso-por-onda W2): the scaffold seeds each
    /// wave's `meta.json#checklist` with one `{label, path, done:false}` item
    /// per target file; the PARENT root meta carries NO checklist (explicit
    /// OUT). The sidecar follows the write mode: reconciled back onto the plan
    /// while the spec is unapproved (EXECUTE cannot have started, so no
    /// progress is lost), FROZEN once `.approved-by-user` exists — which is
    /// what keeps a `done` flag flipped by the auto-mark hook intact.
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

        let _ = scaffold(&spec_dir, &plan_path);

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

        // BEFORE approval the sidecar is a pure function of the plan, so a
        // re-scaffold reconciles it back (and says so under `refreshed`).
        let wave_meta_path = spec_dir.join("wave-1-rt").join("meta.json");
        let mut marked = wave_meta.clone();
        marked.checklist[0].done = true;
        mustard_core::write_meta(&wave_meta_path, &marked).unwrap();
        let (.., refreshed, _) = lists(scaffold(&spec_dir, &plan_path));
        assert!(
            refreshed.contains(&"wave-1-rt/meta.json".to_string()),
            "an unapproved sidecar that drifted is refreshed: {refreshed:?}"
        );
        assert!(
            !mustard_core::read_meta(&wave_meta_path).unwrap().checklist[0].done,
            "before approval the checklist is re-derived from the plan"
        );

        // AFTER approval the sidecar is frozen — a `done` flipped by the
        // auto-mark hook during EXECUTE survives any re-scaffold.
        let mut marked = wave_meta.clone();
        marked.checklist[0].done = true;
        mustard_core::write_meta(&wave_meta_path, &marked).unwrap();
        std::fs::write(spec_dir.join(APPROVED_BY_USER_MARKER), "").unwrap();
        let _ = scaffold(&spec_dir, &plan_path);
        let again = mustard_core::read_meta(&wave_meta_path).unwrap();
        assert!(again.checklist[0].done, "an approved sidecar preserves done state");
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

    /// End-to-end through `scaffold`: a parent spec.md whose AC-2 no wave claims
    /// makes `scaffold` return a non-empty `uncovered_acs` — the list
    /// `plan-materialize` maps to a blocked PLAN + non-zero exit. The layout is
    /// still materialised (idempotent); the list is the data the caller actions.
    #[test]
    fn scaffold_flags_uncovered_parent_ac() {
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
            ScaffoldOutcome::Created { created, uncovered_acs, .. } => {
                // The layout was still materialised.
                assert!(created.iter().any(|f| f == "wave-plan.md"), "created: {created:?}");
                // The uncovered parent AC-2 is flagged (plan-materialize blocks on it).
                assert!(
                    uncovered_acs.iter().any(|g| g.contains("AC-2")),
                    "the uncovered AC-2 must be flagged: {uncovered_acs:?}"
                );
                assert!(
                    !uncovered_acs.iter().any(|g| g.contains("AC-1")),
                    "the covered AC-1 must not be flagged: {uncovered_acs:?}"
                );
            }
            _ => panic!("expected ScaffoldOutcome::Created"),
        }
    }


    // -----------------------------------------------------------------------
    // Write mode: reconcile before approval, freeze after
    // -----------------------------------------------------------------------

    /// Write a plan.json with `waves` verbatim; returns the path.
    fn write_plan(dir: &Path, waves: Value) -> std::path::PathBuf {
        let plan_path = dir.join("plan.json");
        let total = waves.as_array().map_or(0, Vec::len);
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": waves,
                "total_waves": total,
                "lang": "en-US",
            }))
            .unwrap(),
        )
        .unwrap();
        plan_path
    }

    /// Destructure a `Created` outcome into `(created, skipped, refreshed, removed)`.
    fn lists(outcome: ScaffoldOutcome) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
        match outcome {
            ScaffoldOutcome::Created { created, skipped, refreshed, removed, .. } => {
                (created, skipped, refreshed, removed)
            }
            _ => panic!("expected ScaffoldOutcome::Created"),
        }
    }

    /// AC-1 — a spec that carries NO `.approved-by-user` marker is still the
    /// Plan agent's draft, so re-running the scaffold after editing `plan.json`
    /// REWRITES what differs and reports it under `refreshed`. This is the
    /// repair path the field report asked for: fix the plan, re-run, done — no
    /// hand deletion, no guard workaround.
    #[test]
    fn reconciles_scaffold_before_approval() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-reconcile");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([{ "n": 1, "role": "rt", "summary": "first take",
                     "tasks": ["do it"], "files": ["src/a.rs"] }]),
        );
        let (created, ..) = lists(scaffold(&spec_dir, &plan_path));
        assert!(created.contains(&"wave-plan.md".to_string()), "{created:?}");

        // The Plan agent sharpens the wave: new summary, new task, new census.
        let plan_path = write_plan(
            dir.path(),
            json!([{ "n": 1, "role": "rt", "summary": "sharpened take",
                     "tasks": ["do it properly"], "files": ["src/b.rs"] }]),
        );
        let (created, _skipped, refreshed, removed) = lists(scaffold(&spec_dir, &plan_path));

        assert!(created.is_empty(), "nothing is new on a re-run: {created:?}");
        assert!(removed.is_empty(), "no wave was dropped: {removed:?}");
        for expected in ["wave-plan.md", "wave-1-rt/spec.md", "wave-1-rt/meta.json"] {
            assert!(
                refreshed.contains(&expected.to_string()),
                "{expected} must be reported refreshed: {refreshed:?}"
            );
        }
        assert_eq!(
            refreshed.clone().iter().collect::<BTreeSet<_>>().len(),
            refreshed.len(),
            "no duplicate entries: {refreshed:?}"
        );
        let mut sorted = refreshed.clone();
        sorted.sort();
        assert_eq!(refreshed, sorted, "refreshed must be sorted for byte-stable stdout");

        // Disk actually carries the new plan, not the stale first take.
        let wave_spec =
            std::fs::read_to_string(spec_dir.join("wave-1-rt").join("spec.md")).unwrap();
        assert!(wave_spec.contains("sharpened take"), "{wave_spec}");
        assert!(wave_spec.contains("- [ ] do it properly"), "{wave_spec}");
        assert!(!wave_spec.contains("first take"), "stale body survived: {wave_spec}");
        let wave_meta =
            mustard_core::read_meta(&spec_dir.join("wave-1-rt").join("meta.json")).unwrap();
        assert_eq!(wave_meta.checklist[0].path.as_deref(), Some("src/b.rs"));
    }

    /// AC-2 — once `.approved-by-user` exists the layout is FROZEN: a plan that
    /// renders something else leaves every file byte-identical, reports nothing
    /// refreshed or removed, and raises the single stderr WARN that names the
    /// change-request route.
    #[test]
    fn approved_plan_scaffold_is_frozen() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-frozen");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([{ "n": 1, "role": "rt", "summary": "approved take",
                     "tasks": ["do it"], "files": ["src/a.rs"] }]),
        );
        let _ = scaffold(&spec_dir, &plan_path);
        let before_plan = std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap();
        let before_spec =
            std::fs::read_to_string(spec_dir.join("wave-1-rt").join("spec.md")).unwrap();
        let before_meta =
            std::fs::read_to_string(spec_dir.join("wave-1-rt").join("meta.json")).unwrap();

        // The user approves — and only then does the plan change underneath.
        std::fs::write(spec_dir.join(APPROVED_BY_USER_MARKER), "").unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([{ "n": 1, "role": "rt", "summary": "a late rewrite",
                     "tasks": ["something else"], "files": ["src/z.rs"] }]),
        );
        let (created, skipped, refreshed, removed) = lists(scaffold(&spec_dir, &plan_path));

        assert!(created.is_empty(), "{created:?}");
        assert!(refreshed.is_empty(), "an approved layout is never rewritten: {refreshed:?}");
        assert!(removed.is_empty(), "an approved layout is never pruned: {removed:?}");
        assert!(skipped.contains(&"wave-1-rt/spec.md".to_string()), "{skipped:?}");
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap(),
            before_plan,
            "wave-plan.md must stay byte-identical"
        );
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("wave-1-rt").join("spec.md")).unwrap(),
            before_spec,
            "the wave spec must stay byte-identical"
        );
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("wave-1-rt").join("meta.json")).unwrap(),
            before_meta,
            "the wave sidecar must stay byte-identical"
        );

        // The drift signal that drives the single stderr WARN, and its wording.
        let mut ledger = Ledger::new(&spec_dir, WriteMode::Frozen);
        ledger.emit(&spec_dir.join("wave-plan.md"), "a different body\n");
        assert!(ledger.drift, "a would-be change must raise the frozen-plan drift flag");
        assert_eq!(
            std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap(),
            before_plan,
            "the frozen emit must not write"
        );
        let warn = frozen_plan_warn();
        assert!(warn.contains("change request"), "the WARN must name the route: {warn}");
        assert!(warn.contains("change-log.md"), "the WARN must name the record: {warn}");
        assert!(warn.contains(APPROVED_BY_USER_MARKER), "the WARN must name the marker: {warn}");
    }

    /// AC-2 (the half the freeze first missed) — an APPROVED spec must not grow
    /// a wave. A later plan that adds one still materialises the missing dir
    /// (the dashboard's broken-link repair depends on absent artefacts being
    /// restored), but the root sidecar every consumer reads — `wave-advance`,
    /// `status`, the dashboard — keeps the APPROVED `totalWaves`, and the
    /// divergence is announced instead of applied silently.
    ///
    /// Field-review finding: bumping it produced a spec whose `wave-plan.md`
    /// listed one wave while `meta.json` claimed two — an approved plan quietly
    /// acquiring work the user never saw, which is exactly what the marker
    /// exists to prevent.
    #[test]
    fn approved_plan_keeps_its_wave_count() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-grow");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([{ "n": 1, "role": "rt", "summary": "one", "tasks": ["t"] }]),
        );
        let _ = scaffold(&spec_dir, &plan_path);
        assert_eq!(read_meta(&spec_dir.join("meta.json")).unwrap().total_waves, Some(1));

        std::fs::write(spec_dir.join(APPROVED_BY_USER_MARKER), "").unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([
                { "n": 1, "role": "rt", "summary": "one", "tasks": ["t"] },
                { "n": 2, "role": "cli", "summary": "smuggled in", "tasks": ["t"] },
            ]),
        );
        let (created, _, refreshed, removed) = lists(scaffold(&spec_dir, &plan_path));

        assert_eq!(
            read_meta(&spec_dir.join("meta.json")).unwrap().total_waves,
            Some(1),
            "the approved wave count is authoritative — a later plan cannot move it",
        );
        assert!(refreshed.is_empty(), "{refreshed:?}");
        assert!(removed.is_empty(), "{removed:?}");
        // The absent artefact is still restored (the doctor-repair path)…
        assert!(created.contains(&"wave-2-cli/spec.md".to_string()), "{created:?}");
        // …and `wave-plan.md`, which the user approved, still lists only wave 1.
        let table = std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap();
        assert!(!table.contains("2-cli"), "the approved table must not gain a row: {table}");
    }

    /// The reconcile window is the PLAN AUTHORING window, not merely "no marker".
    /// Two states outside it must fall back to skip-if-present, because
    /// rewriting and pruning there destroy real work:
    ///
    /// - `stage` past `Plan`: EXECUTE agents are already editing these dirs.
    /// - `scopeOverride: "user-rejected-waves"`: `wave-collapse` merged the
    ///   waves down BECAUSE the user rejected the decomposition; reconciling
    ///   from the pre-collapse plan would delete exactly that merge.
    #[test]
    fn write_mode_freezes_outside_the_plan_authoring_window() {
        let dir = tempdir().unwrap();

        // No sidecar at all → a fresh scaffold, reconcilable.
        let fresh = dir.path().join("fresh");
        std::fs::create_dir_all(&fresh).unwrap();
        assert_eq!(write_mode(&fresh), WriteMode::Reconcile);

        let stage_meta = |dir: &Path, stage: &str, raw: Value| {
            std::fs::create_dir_all(dir).unwrap();
            let mut meta = Meta { stage: Some(stage.into()), ..Meta::default() };
            meta.raw = raw;
            write_meta(&dir.join("meta.json"), &meta).unwrap();
        };

        let planning = dir.path().join("planning");
        stage_meta(&planning, "Plan", Value::Null);
        assert_eq!(write_mode(&planning), WriteMode::Reconcile, "PLAN stays reconcilable");

        let executing = dir.path().join("executing");
        stage_meta(&executing, "Execute", Value::Null);
        assert_eq!(
            write_mode(&executing),
            WriteMode::Frozen,
            "a spec in EXECUTE has agents working against these dirs",
        );

        let rejected = dir.path().join("rejected");
        stage_meta(&rejected, "Plan", json!({ "scopeOverride": "user-rejected-waves" }));
        assert_eq!(
            write_mode(&rejected),
            WriteMode::Frozen,
            "a user-rejected decomposition is never re-exploded from the stale plan",
        );
    }

    /// AC-3 — before approval, a wave dropped from `plan.json` has its
    /// directory deleted and listed under `removed`. The root `spec.md` /
    /// `meta.json` and the `.events/`, `qa/` and `review/` phase folders are
    /// never touched — only `wave-N-*` directories are in scope.
    #[test]
    fn removes_wave_dropped_from_plan() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-prune");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Epic\n").unwrap();
        for phase in [".events", "qa", "review"] {
            std::fs::create_dir_all(spec_dir.join(phase)).unwrap();
            std::fs::write(spec_dir.join(phase).join("keep.md"), "keep me\n").unwrap();
        }
        let plan_path = write_plan(
            dir.path(),
            json!([
                { "n": 1, "role": "rt", "summary": "a", "tasks": ["t1"] },
                { "n": 2, "role": "cli", "summary": "b", "depends_on": ["wave-1-rt"],
                  "tasks": ["t2"] }
            ]),
        );
        let _ = scaffold(&spec_dir, &plan_path);
        assert!(spec_dir.join("wave-2-cli").join("spec.md").exists());

        // The plan drops wave 2.
        let plan_path = write_plan(
            dir.path(),
            json!([{ "n": 1, "role": "rt", "summary": "a", "tasks": ["t1"] }]),
        );
        let (_, _, _, removed) = lists(scaffold(&spec_dir, &plan_path));

        assert_eq!(removed, vec!["wave-2-cli".to_string()], "{removed:?}");
        assert!(!spec_dir.join("wave-2-cli").exists(), "the stale wave dir must be gone");
        assert!(spec_dir.join("wave-1-rt").join("spec.md").exists(), "the planned wave stays");
        // Untouchables.
        assert!(spec_dir.join("spec.md").exists());
        assert!(spec_dir.join("meta.json").exists());
        for phase in [".events", "qa", "review"] {
            assert!(
                spec_dir.join(phase).join("keep.md").exists(),
                "{phase}/ must never be pruned"
            );
        }
    }

    /// AC-8 — a plan that cannot be read OR cannot be parsed answers with a
    /// stderr message that carries a minimal valid plan and points at the
    /// authoritative schema, so the operator does not have to go find it after
    /// failing.
    #[test]
    fn unreadable_plan_message_teaches_schema() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-schema");
        std::fs::create_dir_all(&spec_dir).unwrap();

        let assert_teaches = |msg: &str| {
            for token in ["\"waves\"", "\"role\"", "\"total_waves\"", "full-plan.md", "Plan JSON schema"] {
                assert!(msg.contains(token), "message must carry {token}: {msg}");
            }
        };

        // 1. The file is not there at all.
        match scaffold(&spec_dir, &dir.path().join("nope.json")) {
            ScaffoldOutcome::Unreadable(msg) => {
                assert!(msg.contains("cannot read plan"), "{msg}");
                assert_teaches(&msg);
            }
            _ => panic!("a missing plan must be Unreadable"),
        }

        // 2. The file is there but is not valid plan JSON.
        let broken = dir.path().join("broken.json");
        std::fs::write(&broken, "{ this is not json }").unwrap();
        match scaffold(&spec_dir, &broken) {
            ScaffoldOutcome::Unreadable(msg) => {
                assert!(msg.contains("parse error"), "{msg}");
                assert_teaches(&msg);
            }
            _ => panic!("a malformed plan must be Unreadable"),
        }
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
