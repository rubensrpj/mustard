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
//!       "acceptance": ["**AC-1** — handler returns 200. Command: `curl -sf …`"],
//!       "reality_obligations": ["read the provider's official webhook doc"]
//!     },
//!     { "n": 2, "role": "general", "summary": "…", "depends_on": ["wave-1-general"] }
//!   ],
//!   "total_waves": 2
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
//! - `acceptance` — Acceptance Criteria lines. The union across waves is carried
//!   into `wave-plan.md` under `## Acceptance Criteria`/`## Critérios de
//!   Aceitação`, where the QA gate reads it via
//!   `spec_sections::section_block(_, "acceptanceCriteria")` — that union is the
//!   judge and stays intact. The wave's `spec.md` carries NO copy of the text:
//!   only WHICH ids it satisfies, as `satisfies:` frontmatter (see
//!   [`satisfied_ids`]), and the dispatch prompt cuts the criteria themselves
//!   out of that same union at render time (falling back to the parent only for
//!   a spec whose plan declares no `acceptance` line at all) — one file for the
//!   reader and the judge. A copy would be a snapshot, and the layout is frozen
//!   after approval — an `ac-amend` or `ac-add` would never reach it.
//! - `reality_obligations` — duties to check the world OUTSIDE the repository
//!   before writing the code they govern. Materialised as
//!   `## Reality Obligations` with `- **RO-{n}.{i}** — {duty}` items; the
//!   dispatch prompt renders them as their own section and `wave-done` reads
//!   them back to name the duties the returning wave left unaccounted for.
//!
//! Each is `#[serde(default)]`: a plan that predates these fields (summary-only)
//! still deserialises, and a wave that omits them materialises with no task /
//! file block (the empty-tasks case emits a visible stderr WARN — see
//! [`scaffold`]).
//!
//! The plan carries no language. The language every `meta.json` records is the
//! project's text language, read through `ProjectConfig::language()` at the
//! project the spec lives in (`pt-BR` when the project declares none); a `lang`
//! key left in an old plan is ignored. The headings are English whatever the
//! language: the layout is read back by machines, not by people.
//!
//! Idempotent, in both write modes (see [`WriteMode`]): re-running an UNCHANGED
//! plan creates, refreshes and removes nothing. Before the user approves the
//! spec the layout is reconciled onto the plan (a differing file is rewritten,
//! a wave the plan dropped is deleted); once the user approved the spec (its
//! state in `spec.ndjson`) it is frozen — skip-if-present, with ONE stderr
//! WARN when a file would have changed. `plan-materialize` reports
//! `created_files` / `skipped` / `refreshed` / `removed` on stdout.

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
/// rather than maintaining a second, divergent freeform renderer.
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
    /// `spec.md` (the wave carries only the ids it satisfies, as frontmatter —
    /// the prompt reads the text from the parent); the union of every wave's
    /// `acceptance` is carried into `wave-plan.md` so the QA gate finds it.
    /// `#[serde(default)]` for the same retrocompat reason as `tasks`.
    #[serde(default)]
    pub(crate) acceptance: Vec<String>,
    /// Reality obligations for this wave — duties to verify something OUTSIDE
    /// the repository (read an official document, call a live endpoint, read a
    /// stored row) before writing the code they govern. Materialised as a
    /// `## Reality Obligations` section of `- **RO-{n}.{i}** — {duty}` lines in
    /// the wave's `spec.md`, which the dispatch prompt renders as its own
    /// section and `wave-done` reads back to report the duties the returning
    /// wave left unaccounted for.
    ///
    /// `#[serde(default)]` for the same retrocompat reason as `tasks`: every
    /// plan written before this field still deserialises, and a wave that
    /// declares no duty materialises exactly as it did before (no heading).
    #[serde(default, alias = "realityObligations")]
    pub(crate) reality_obligations: Vec<String>,
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
}

/// Heading strings for the wave layout.
///
/// These render MACHINE artefacts — the operational `wave-plan.md` index (with
/// the `## Acceptance Criteria` union) and the per-wave `spec.md` skeletons
/// (with their materialised `## Tasks` / `## Files` bodies and a `satisfies:`
/// frontmatter line). They are ENGLISH-FIXED regardless of the
/// project's text language (only the spec narrative people read follows
/// it). The struct is retained (rather than inlining the literals) so
/// the re-wave path renders through the same canonical renderers.
///
/// `pub(crate)` so the re-wave path can render through the same canonical
/// renderers.
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
    /// `## Reality Obligations` heading for the per-wave duties to check the
    /// world outside the repository.
    reality_obligations: &'a str,
    /// `## Acceptance Criteria`/`## Critérios de Aceitação` heading for the
    /// AC union carried into `wave-plan.md`.
    acceptance: &'a str,
    /// `## Material` heading for the parent spec's decisions and traps, cut to
    /// this wave. See [`render_wave_spec`].
    material: &'a str,
    /// `true` quando o `satisfies` da onda NÃO é um recorte por onda, e sim a
    /// régua do pai inteira — o frontmatter da onda então carrega
    /// `satisfies-scope: unit` ([`SATISFIES_SCOPE_KEY`]).
    ///
    /// O prompt renderiza a seção sob "estes critérios são o JUIZ desta onda"
    /// ([`crate::commands::agent::render::sections::read_wave_acceptance`]), e
    /// essa frase é falsa quando a régua foi carregada inteira por não haver
    /// como atribuí-la — ver [`headings_for_rewave`]. `false` na porta do PLAN,
    /// onde o autor declara `satisfies` e o recorte é real: o arquivo sai
    /// byte-idêntico ao de antes deste campo existir.
    carried_whole: bool,
}

/// Build the heading set. These render MACHINE artefacts — the operational
/// `wave-plan.md` index (with the `## Acceptance Criteria` union) and the
/// per-wave `spec.md` skeletons (with their `## Tasks` / `## Files` bodies) — so
/// the headings are ENGLISH-FIXED regardless of the project's text language
/// (only the spec narrative people read follows it). The display names are
/// the EN spellings
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
        reality_obligations: "## Reality Obligations",
        acceptance: "## Acceptance Criteria",
        material: "## Material",
        carried_whole: false,
    }
}

/// A linha que o prompt de uma onda nascida de um REWAVE carrega no topo do
/// `## ACCEPTANCE` dela.
///
/// O rewave não tem autor: as ondas nascem de um DAG de ARQUIVOS, que não tem
/// como dizer qual critério pertence a qual onda. A régua é então carregada
/// INTEIRA para cada onda ([`super::exec_rewave_check`]), senão toda onda
/// rewaveada seria despachada com `## ACCEPTANCE` vazio — o estado que a outra
/// porta AVISA (a onda com tarefas e sem critério é `untraced_waves`, nunca
/// recusa).
///
/// Carregar a união e deixar o prompt afirmar "estes critérios são o JUIZ desta
/// onda" seria dizer ao executor uma coisa que o plano não sabe. Então a união
/// continua, e o prompt DIZ que é união: é a diferença entre uma resposta
/// honesta e uma atribuição inventada. O arquivo da onda carrega só o marcador
/// (`satisfies-scope: unit`); o texto vive aqui, e o renderizador o lê.
pub(crate) const REWAVE_ACCEPTANCE_NOTE: &str =
    "_Carried WHOLE from the parent, not cut per wave: this layout was derived from the file \
     dependency graph, which cannot say which criterion belongs to which wave. The set below is \
     the UNIT's ruler — some of these lines only pass once a sibling wave lands, and that is \
     expected._";

/// O mesmo conjunto de títulos da porta do PLAN, mais a nota que declara a
/// régua como união do pai — ver [`REWAVE_ACCEPTANCE_NOTE`].
///
/// Duas portas, dois construtores nomeados, um renderizador: o campo fica
/// privado e nenhuma delas pode esquecer qual é a sua.
pub(crate) fn headings_for_rewave() -> Headings<'static> {
    Headings { carried_whole: true, ..headings() }
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
///
/// ## Why the material is COPIED here
///
/// `parent_material_text` is the parent `spec.md` body (empty when there is
/// none). The decisions and traps the conversation settled live there, and until
/// now they reached only the rendered dispatch prompt — never the wave file.
/// Measured in the field: the human reading `wave-1-backend/spec.md` saw 37
/// lines of tasks with no reason behind any of them, and concluded the spec was
/// shallow. They were right about what they were looking at.
///
/// So the cut runs here too, through the SAME rule the prompt uses
/// ([`cut_material_for_files`]) — definitions and decisions bind every wave, a
/// finding rides to the wave that declares its file. It is a COPY, and the
/// parent stays the source: `plan-materialize` re-renders these files, so a
/// decision settled later lands here on the next materialisation.
///
/// ## Why the wave carries WHICH criteria, and never their text
///
/// The wave used to materialise none of its criteria: the union lived in
/// `wave-plan.md`, which is where QA reads from, and the wave file said what to
/// do and where — never by which ruler it would be measured. The first remedy
/// COPIED the subset into the wave file, and the copy was the mistake: the
/// layout is frozen once approved ([`WriteMode::Frozen`]), so a criterion
/// rewritten by `ac-amend` or added by `ac-add` landed in the parent and never
/// reached any wave's prompt — the agent re-dispatched for a review finding was
/// exactly the one that could not see the criterion written for it.
///
/// So the wave persists only the ids it satisfies ([`satisfied_ids`]), as one
/// `satisfies:` frontmatter line ([`SATISFIES_KEY`]), written once and stable
/// across the unit's life. The prompt is rendered at DISPATCH time from the
/// CURRENT `## Acceptance Criteria` of the file the JUDGE reads — the union in
/// `wave-plan.md`, and the parent only for a spec that materialised no plan —
/// filtered by that line
/// ([`crate::commands::agent::render::sections::read_wave_acceptance`]). A
/// wave that satisfies nothing gets no line, and renders no `## ACCEPTANCE`.
pub(crate) fn render_wave_spec(
    parent: &str,
    w: &WavePlanEntry,
    hd: &Headings<'_>,
    parent_material_text: &str,
) -> String {
    let name = wave_name(w);
    let mut out = String::new();
    // Leading `id:` frontmatter — the rename-proof identity handle, derived from
    // the parent spec slug plus this wave's `{n}-{role}` (the same tokens
    // `wave_name` builds the folder from). `[[wave.{slug}.{n}-{role}]]` resolves
    // to this file via `atomic_md::wikilink::resolve`'s frontmatter-id
    // precedence. Identity is not lifecycle metadata (which stays in
    // `meta.json`). A blank `parent` (defensive) omits the block.
    if !parent.is_empty() {
        let _ = write!(out, "---\nid: wave.{parent}.{n}-{role}\n", n = w.n, role = w.role);
        // A régua desta onda: QUAIS ids ela satisfaz, uma linha, e nunca o
        // texto deles — ver o doc da função. Um id repetido não entra duas vezes.
        let mut ids: Vec<String> = Vec::new();
        for id in satisfied_ids(w) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if !ids.is_empty() {
            let _ = writeln!(out, "{SATISFIES_KEY}: [{}]", ids.join(", "));
            if hd.carried_whole {
                let _ = writeln!(out, "{SATISFIES_SCOPE_KEY}: {SATISFIES_SCOPE_UNIT}");
            }
        }
        out.push_str("---\n\n");
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
    // The duties this wave owes the world outside the repository. Each carries
    // an id so the dispatched agent can account for it BY NAME and `wave-done`
    // can say which duty has no account — a bare prose line would leave both
    // sides guessing which sentence answered which duty. Emitted only when the
    // plan declares one, so a plan without them renders byte-identically to a
    // plan written before the field existed.
    if !w.reality_obligations.is_empty() {
        let _ = write!(out, "\n{}\n\n", hd.reality_obligations);
        for (i, duty) in w.reality_obligations.iter().enumerate() {
            let duty = duty.trim();
            if duty.is_empty() {
                continue;
            }
            let _ = writeln!(
                out,
                "- **{id}** — {duty}",
                id = reality_obligation_id(w.n, i)
            );
        }
    }
    // What the conversation settled, cut to THIS wave — see the doc comment.
    // Last, so the operational body (tasks, files, duties) keeps its position
    // and a plan carrying no material renders byte-identically to before.
    let (material, _) = crate::commands::agent::render::sections::cut_material_for_files(
        parent_material_text,
        &w.files,
    );
    if !material.is_empty() {
        let _ = write!(out, "\n{}\n\n", hd.material);
        let _ = writeln!(
            out,
            "_Copied from the parent spec at materialisation. The parent is the source — \
             re-run `plan-materialize` after adding material._\n"
        );
        out.push_str(&material);
        out.push('\n');
    }
    out
}

/// The frontmatter key that names WHICH criteria a wave satisfies —
/// `satisfies: [AC-1, AC-3]`. Written once by [`render_wave_spec`], from
/// `plan.json` and nowhere else; read by the prompt renderer
/// ([`parse_wave_ruler`]) and named in the `ac-add` WARN as the line only a
/// re-materialisation may rewrite.
pub(crate) const SATISFIES_KEY: &str = "satisfies";

/// The frontmatter key a REWAVE wave carries beside `satisfies:` to say the set
/// is the unit's whole ruler, not a per-wave cut — see [`REWAVE_ACCEPTANCE_NOTE`].
pub(crate) const SATISFIES_SCOPE_KEY: &str = "satisfies-scope";

/// The one value [`SATISFIES_SCOPE_KEY`] takes.
const SATISFIES_SCOPE_UNIT: &str = "unit";

/// What a wave's frontmatter says about its ruler — the reader twin of the
/// `satisfies:` line [`render_wave_spec`] writes.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WaveRuler {
    /// The ids the wave satisfies, normalised (trim + uppercase) exactly as
    /// [`satisfied_ids`] normalises them, in file order, de-duplicated.
    pub(crate) satisfies: Vec<String>,
    /// `true` when the set is the unit's whole ruler carried by a rewave.
    pub(crate) carried_whole: bool,
}

/// The `---` frontmatter block of a markdown document, as its inner lines.
/// `None` when the document does not open with one.
fn frontmatter_lines(md: &str) -> Option<Vec<&str>> {
    let mut lines = md.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    Some(lines.take_while(|l| l.trim_end() != "---").collect())
}

/// Parse a wave `spec.md`'s frontmatter into its [`WaveRuler`]. Pure, total: no
/// frontmatter, or one without a `satisfies:` line, yields the empty ruler —
/// which the renderer turns into no `## ACCEPTANCE` at all.
///
/// `[AC-1, AC-2]` and a bare `AC-1, AC-2` both parse: the brackets are the
/// written form, and a hand edit that drops them must not silently drop the
/// wave's ruler with them.
pub(crate) fn parse_wave_ruler(md: &str) -> WaveRuler {
    let mut ruler = WaveRuler::default();
    for line in frontmatter_lines(md).unwrap_or_default() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key == SATISFIES_KEY {
            for id in value.trim().trim_matches(['[', ']']).split(',') {
                let id = id.trim().to_uppercase();
                if !id.is_empty() && !ruler.satisfies.contains(&id) {
                    ruler.satisfies.push(id);
                }
            }
        } else if key == SATISFIES_SCOPE_KEY {
            ruler.carried_whole = value.trim() == SATISFIES_SCOPE_UNIT;
        }
    }
    ruler
}

/// The id of the `i`-th (0-based) reality obligation of wave `n` — `RO-{n}.{i+1}`.
///
/// The wave number is IN the id on purpose: `wave-done` looks for an account of
/// a duty in what the wave recorded on the spec's own event log, which is
/// per-spec and not per-wave. A bare `RO-1` would let one wave's account clear
/// another wave's duty; `RO-3.1` cannot be confused with `RO-4.1`.
fn reality_obligation_id(n: u32, i: usize) -> String {
    format!("RO-{n}.{}", i + 1)
}

/// Parse a rendered `## Reality Obligations` section back into `(id, duty)`
/// pairs — the reader twin of [`render_wave_spec`]'s writer, kept in the same
/// file so the two cannot drift into different notions of what a duty line is.
///
/// `md` is a whole wave `spec.md`; the section is located through the canonical
/// [`is_heading`](crate::commands::spec::spec_sections::is_heading) resolver, so
/// a localised heading resolves here exactly as it does everywhere else. Lines
/// that do not carry the `- **{id}** — {duty}` shape are skipped rather than
/// guessed at. Pure and total: no section, or a section of prose, yields an
/// empty list.
pub(crate) fn parse_reality_obligations(md: &str) -> Vec<(String, String)> {
    use crate::commands::spec::spec_sections::{is_heading, section_end};
    let lines: Vec<&str> = md.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|l| is_heading(l, "reality-obligations"))
    else {
        return Vec::new();
    };
    let end = section_end(&lines, start);
    let mut out = Vec::new();
    for line in &lines[start + 1..end] {
        let Some(rest) = line.trim_start().strip_prefix("- **") else {
            continue;
        };
        let Some((id, tail)) = rest.split_once("**") else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        // The separator is an em dash in the rendered form; tolerate a plain
        // hyphen so a hand-edited wave spec still parses.
        let duty = tail
            .trim_start()
            .trim_start_matches(['—', '-'])
            .trim()
            .to_string();
        out.push((id.to_string(), duty));
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

/// As linhas de `acceptance` de UMA onda, com o bullet que o parser exige.
///
/// Mesma normalização que [`build_ac_block`] faz para a união: o schema do plano
/// aceita a linha sem bullet, e o parser do `qa-run` não. Um único jeito de
/// preparar o texto, senão o portão enxerga critérios que o recorte da onda não
/// enxerga.
fn wave_ac_text(w: &WavePlanEntry) -> String {
    w.acceptance
        .iter()
        .map(|line| {
            let t = line.trim();
            if t.starts_with('-') { t.to_string() } else { format!("- {t}") }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Os ids de critério que UMA onda satisfaz: o `satisfies` explícito quando
/// declarado, senão os ids parseados das suas próprias linhas de `acceptance`.
///
/// Uma regra só, de propósito. O portão de rastreabilidade
/// ([`traceability_gaps`]) e a linha `satisfies:` que [`render_wave_spec`]
/// escreve no frontmatter da onda — pela qual o prompt recorta o `## ACCEPTANCE`
/// — precisam responder EXATAMENTE o mesmo conjunto: se divergirem, ou a onda é
/// avisada por um critério que o prompt nunca carrega, ou é medida por um que
/// ninguém contou como coberto.
///
/// Os ids saem normalizados (trim + maiúsculas) pelo MESMO parser que o `qa-run`
/// executa, então o pareamento com o texto do critério não depende de como o
/// autor escreveu o id.
///
/// O que esta função NÃO responde é se cada id nomeia um critério que EXISTE:
/// ela devolve os ids COMO ESCRITOS. Quem julga cruza este conjunto com o
/// `defined` de [`traceability_gaps`], e quem renderiza cruza com o
/// `## Acceptance Criteria` atual do pai — os dois cruzamentos dão o mesmo
/// resultado, que é o ponto: um id sem texto não renderiza critério nenhum e por
/// isso não conta como régua.
pub(crate) fn satisfied_ids(w: &WavePlanEntry) -> Vec<String> {
    use crate::commands::review::qa_run::parse_ac_items;
    let norm = |s: &str| s.trim().to_uppercase();
    if !w.satisfies.is_empty() {
        return w
            .satisfies
            .iter()
            .map(|s| norm(s))
            .filter(|s| !s.is_empty())
            .collect();
    }
    parse_ac_items(&wave_ac_text(w))
        .into_iter()
        .map(|it| norm(&it.id))
        .collect()
}

/// The two AC↔wave traceability gap kinds, kept apart so [`scaffold`] can
/// surface the uncovered-criterion gap (the coverage gate `plan-materialize`
/// enforces) while the untraced-wave signal stays a non-blocking WARN.
struct TraceGaps {
    /// Gap 1 — a wave that does work (`tasks` non-empty) but traces to NO
    /// criterion that exists. ADVISORY: a WARN on stderr and an `untraced_waves`
    /// list in the report, never a refusal — a wave can legitimately be plumbing
    /// no single criterion pins down, and whoever approves the plan decides.
    /// What the WARN says is real, though: the wave's `satisfies:` line — and
    /// therefore the `## ACCEPTANCE` block of its dispatched prompt — is CUT by
    /// exactly this set ([`satisfied_ids`]), so such a wave is dispatched with
    /// no ruler and still judged by one at QA.
    ///
    /// The PHANTOM `satisfies` id (`AC-01` where the plan defines `AC-1`) lands
    /// here too, as its own sentence naming the id and the ids that do exist:
    /// nothing renders for it. Both sentences derive from ONE per-wave
    /// set — the ids that actually judge the wave — so neither can assert
    /// something false about what materialised. When NOTHING defines a
    /// criterion, the list says that once, as the reason both checks are
    /// silent.
    ///
    /// Refusing on this was tried and reverted: it refused legitimate plans and
    /// flipped the archive; "optional channels stop being optional" is its own
    /// unit, with the migration modelled first.
    untraced_waves: Vec<String>,
    /// Gap 2 — an acceptance criterion NO wave satisfies. `defined` is the
    /// union of every wave's `acceptance` ids AND the parent spec.md
    /// `## Acceptance Criteria` ids, so a criterion the plan forgot to route
    /// onto a wave is caught. This is the escalatable gap.
    uncovered_acs: Vec<String>,
    /// Gap 3 — a claim the plan's own contents REFUTE: a wave that satisfies a
    /// criterion while declaring no files. It says it will do the work and, in
    /// the same document, that it has nowhere to do it. Escalatable, alongside
    /// Gap 2, because no reading of the plan makes it hold — this is a
    /// contradiction, not a judgement.
    unsupportable_claims: Vec<String>,
    /// Gap 4 — a criterion whose command names a repository path that none of
    /// its claimants declares. ESCALATABLE, alongside Gaps 2 and 3, and the one
    /// of the four that answers SUFFICIENCY rather than coverage: Gap 2 asks
    /// whether SOME wave claimed the id, this asks whether the claiming wave can
    /// actually satisfy it. A wave that claims a criterion must contain every
    /// path that criterion's command inspects — including one it only reads,
    /// because a criterion reading a file nobody in the group will touch is a
    /// criterion the group cannot move.
    ///
    /// It was a stderr WARN dropped before the outcome was built, so no machine
    /// consumer of `plan-materialize` could see it at all.
    criteria_outside_claimants: Vec<String>,
}

/// Compute AC↔wave traceability gaps, splitting the untraced-wave signal
/// (Gap 1, always WARN) from the uncovered-criterion signal (Gap 2,
/// escalatable — see [`scaffold`]):
///
/// 1. A wave that does work (`tasks` non-empty) but satisfies no acceptance
///    criterion THAT EXISTS — its work traces to no criterion. Counted against
///    `defined`, never against the length of the wave's own list: an id that
///    names nothing cuts the same empty section a missing id does.
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
/// Whether a DECLARED file and a path named by a criterion's command refer to
/// the same file — one being the other's tail, matched on whole path COMPONENTS.
///
/// The boundary is the whole point. A bare suffix compare answers `true` for
/// `apps/backend/src/data.rs` against a declared `a.rs`, which silences exactly
/// the mismatch this check exists to surface: a criterion needing a backend no
/// claiming wave has in scope (found by review, reproduced end-to-end). Both
/// sides are normalised to `/` first, because a plan may spell a path with
/// either separator.
///
/// Errs toward MATCHING (staying quiet) rather than flagging: the two spellings
/// this accepts — repo-relative and subproject-relative — are both legitimate in
/// a `## Files` list, and a false flag on a correct plan costs more trust than a
/// missed advisory. That direction matters more now that the gap this feeds
/// BLOCKS the PLAN transition.
///
/// A DECLARED entry may carry a WILDCARD: `apps/rt/src/commands/**/*.rs` is a
/// legitimate way for a wave to say what it will touch, and comparing it BYTE
/// FOR BYTE against the path a criterion names is a guaranteed refusal of a
/// correct plan. Such an entry is matched as the glob it is, through the
/// crate's existing path-glob matcher (`*` does not cross `/`, `**` does) — a
/// pattern match rather than a directory walk, so the verdict is a function of
/// the plan alone and the report stays byte-stable. The subproject-relative
/// spelling gets the same tolerance the literal case gets, via a `**/` prefix.
///
/// Pure, total.
fn same_or_contains_path(declared: &str, named: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/");
    let (d, n) = (norm(declared), norm(named));
    if d == n || d.ends_with(&format!("/{n}")) || n.ends_with(&format!("/{d}")) {
        return true;
    }
    if !d.contains('*') {
        return false;
    }
    use crate::util::glob::glob_match;
    glob_match(&n, &d) || glob_match(&n, &format!("**/{d}"))
}

fn traceability_gaps(plan: &Plan, parent_ac_md: Option<&str>) -> TraceGaps {
    use crate::commands::review::qa_run::{extract_ac_section, parse_ac_items};
    let norm = |s: &str| s.trim().to_uppercase();
    let mut untraced_waves: Vec<String> = Vec::new();
    let mut defined: BTreeSet<String> = BTreeSet::new();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut unsupportable_claims: Vec<String> = Vec::new();
    // Per criterion, the union of the files declared by every wave claiming it —
    // the other half of the question the claim resolution above already answers.
    let mut claimant_files: std::collections::BTreeMap<String, BTreeSet<String>> =
        std::collections::BTreeMap::new();

    // The parent spec's own criteria are authoritative — every one must be
    // claimed by some wave. Read via the shared qa-run extractor + parser so
    // this reader can never drift from the section QA actually executes. An
    // absent parent / no AC section simply contributes nothing.
    if let Some(section) = parent_ac_md.and_then(extract_ac_section) {
        for it in parse_ac_items(&section) {
            defined.insert(norm(&it.id));
        }
    }

    // O conjunto DEFINIDO fecha ANTES de qualquer onda ser julgada. Fechá-lo
    // dentro do laço faria a onda 1 ser medida contra um conjunto que ainda não
    // conhece os critérios que a onda 2 declara — e a pergunta "este id existe?"
    // só tem uma resposta honesta depois de o plano inteiro ter sido lido.
    for w in &plan.waves {
        // The ACs this wave DEFINES, via the shared qa-run parser. Acceptance
        // lines may arrive without a leading bullet (the plan schema example
        // does); normalise to `- <line>` — exactly as `build_ac_block` does —
        // so the parser (which requires a bullet) finds them.
        for it in parse_ac_items(&wave_ac_text(w)) {
            defined.insert(norm(&it.id));
        }
    }

    // O PLANO SEM CRITÉRIO NENHUM — nem no pai, nem em onda alguma — é dito UMA
    // vez, como o motivo de as duas checagens por onda ficarem caladas: "esta
    // onda traça a um critério que existe?" e "este id nomeia um critério que
    // existe?" não têm contra o que ser respondidas com o conjunto vazio. Não é
    // isenção: é a mesma frase, dita sobre o plano em vez de repetida em cada
    // onda. Aviso, não recusa — o plano materializa, cada onda despacha com o
    // `## ACCEPTANCE` colapsado, e quem aprova o plano lê isto.
    let no_criterion_anywhere = defined.is_empty();
    if no_criterion_anywhere {
        untraced_waves.push(
            "no acceptance criterion is defined anywhere — neither the parent spec's \
             `## Acceptance Criteria` nor any wave's `acceptance` lines declare one — so no wave \
             can trace to a criterion and no `satisfies` id can be checked against one. Every \
             wave is dispatched with its `## ACCEPTANCE` block collapsed until the plan declares \
             criteria."
                .to_string(),
        );
    }

    for w in &plan.waves {
        // Conjunto satisfeito: a MESMA regra que escreve o `satisfies:` da onda,
        // pelo qual o prompt recorta o `## ACCEPTANCE` — ver [`satisfied_ids`].
        let satisfied: Vec<String> = satisfied_ids(w);
        for id in &satisfied {
            covered.insert(id.clone());
        }
        // Os ids que REALMENTE julgam esta onda, computados UMA vez: o
        // `satisfies` cruzado com o que existe, mais os ids das próprias linhas
        // de `acceptance` (que definem critério por construção). As duas
        // mensagens abaixo derivam deste conjunto e do seu complemento, então
        // nenhuma afirma algo falso sobre o que materializou.
        let own: BTreeSet<String> =
            parse_ac_items(&wave_ac_text(w)).into_iter().map(|it| norm(&it.id)).collect();
        let judging: BTreeSet<String> = satisfied
            .iter()
            .filter(|id| defined.contains(*id))
            .cloned()
            .chain(own.iter().cloned())
            .collect();
        let phantom: Vec<String> =
            satisfied.iter().filter(|id| !defined.contains(*id)).cloned().collect();
        let known = || defined.iter().cloned().collect::<Vec<_>>().join(", ");
        if !no_criterion_anywhere {
            // Um id que não nomeia critério NENHUM é um `satisfies` fantasma —
            // `AC-01` onde o pai define `AC-1`. O pai não tem esse id, então o
            // prompt não renderiza nada por ele. Nomeado com o id e os que
            // existem, com ou sem tarefas: um erro de digitação é um erro de
            // digitação, e um aviso sobre ele não recusa nada.
            for id in &phantom {
                untraced_waves.push(format!(
                    "wave-{n}-{role} names {id}, which no criterion defines — the wave's prompt \
                     renders nothing for it. The ones that do exist: {known}",
                    n = w.n,
                    role = w.role,
                    known = known(),
                ));
            }
            // Uma onda que declara tarefas e não traça a critério que EXISTE é
            // despachada sem régua e julgada por uma no QA assim mesmo. Aviso,
            // não recusa: uma onda de encanamento que nenhum critério isolado
            // mede é legítima, e quem aprova o plano decide. A pergunta é sobre
            // um critério que existe, não sobre uma lista não vazia — uma lista
            // só de ids fantasmas recorta a mesma seção vazia que lista nenhuma.
            if !w.tasks.is_empty() && judging.is_empty() {
                untraced_waves.push(format!(
                    "wave-{n}-{role} has tasks but traces to no criterion that exists — add \
                     `satisfies` ids naming criteria this plan declares, or an `acceptance` line \
                     spelling one out. The dispatched prompt's `## ACCEPTANCE` block is cut by \
                     that set, so as written the wave is dispatched with no ruler. The criteria \
                     this plan declares: {known}",
                    n = w.n,
                    role = w.role,
                    known = known(),
                ));
            }
        }
        // Gap 3 — a claim the plan's own contents refute. The wave says it
        // covers these criteria and, in the same document, declares nowhere to
        // do the work. No reading makes that hold, which is why it joins the
        // escalatable gap rather than the advisory one.
        // `!tasks.is_empty()` is the SAME "does work" predicate Gap 1 uses: a
        // summary-only stub that carries a `satisfies` but no tasks is a
        // placeholder mid-authoring, not a commitment, and refusing it would
        // block a plan being written rather than a plan that contradicts itself.
        if !w.tasks.is_empty() && !satisfied.is_empty() && w.files.is_empty() {
            for id in &satisfied {
                unsupportable_claims.push(format!(
                    "{id} — claimed by wave-{n}-{role}, which declares NO files: the plan says \
                     this wave covers the criterion and, in the same breath, that it has nowhere \
                     to do the work. Give the wave the files it will touch, or move the claim to \
                     the wave that has them",
                    n = w.n,
                    role = w.role,
                ));
            }
        }
        for id in &satisfied {
            claimant_files
                .entry(id.clone())
                .or_default()
                .extend(w.files.iter().map(|f| f.trim().to_string()));
        }
    }
    let uncovered_acs: Vec<String> = defined
        .difference(&covered)
        .map(|id| format!("{id} — no wave satisfies it (add it to a wave's `satisfies` or `acceptance`)"))
        .collect();

    // Gap 4 — a criterion whose OWN command names a repository path that none of
    // its claimants declares: it points at something nobody in that group will
    // touch. This is SUFFICIENCY, and it refuses. Coverage (Gap 2) only asks
    // whether some wave claimed the id; a wave that claims a criterion must
    // CONTAIN every path that criterion's command inspects, including one it
    // only reads — a criterion reading a file nobody in the group will touch is
    // a criterion the group cannot move, and the remedy is one line either way
    // (give the claimant the file, or move the claim).
    //
    // A criterion whose command names no path at all is not judged — most
    // criteria here run a NAMED TEST, so the path lives inside the test rather
    // than in the command, and guessing there would be the
    // heuristic-dressed-as-a-gate this check exists to avoid.
    let mut criteria_outside_claimants: Vec<String> = Vec::new();
    if let Some(section) = parent_ac_md.and_then(extract_ac_section) {
        for it in parse_ac_items(&section) {
            let id = norm(&it.id);
            let Some(declared) = claimant_files.get(&id) else {
                continue; // unclaimed — that is Gap 2's business, not this one
            };
            // An EMPTY claimant set means every wave claiming this criterion is
            // already a Gap 3 refusal. Reporting the same fact a second time,
            // once per path its command names, buries the refusal that matters
            // under advisory noise.
            if declared.is_empty() {
                continue;
            }
            for token in it.command.split_whitespace() {
                let path = token.trim_matches(['`', '"', '\'', '(', ')', ',']);
                if !crate::commands::review::analyze_validation::looks_like_file_path(path) {
                    continue;
                }
                if declared.iter().any(|d| same_or_contains_path(d, path)) {
                    continue;
                }
                criteria_outside_claimants.push(format!(
                    "{id} — its command inspects `{path}`, which no wave claiming it declares, so \
                     the claiming wave cannot satisfy the criterion it claims. Add `{path}` to \
                     that wave's files (a path it only READS still has to be in scope), or move \
                     the claim to the wave that has it"
                ));
            }
        }
    }

    TraceGaps { untraced_waves, uncovered_acs, unsupportable_claims, criteria_outside_claimants }
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
            dropped: None,
        })
        .collect()
}

/// Write mode for one scaffold pass, decided by the spec's approved state —
/// never by a flag or an env knob.
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
/// 1. The spec is not approved ([`is_approved`]). The approved state is born
///    only from the user's real answer to the approval question.
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

/// `true` when the user really approved this spec — the one fact the
/// orchestrator cannot assert by hand. Read through the same reader every
/// other door uses: the project root and the spec name come off
/// `<root>/.claude/spec/<name>`, so a spec folder inside a linked worktree is
/// answered from the main checkout's state. A folder outside that layout is
/// no project's spec, and is not approved.
pub(crate) fn is_approved(spec_dir: &Path) -> bool {
    fn named<'a>(dir: Option<&'a Path>, name: &str) -> Option<&'a Path> {
        dir.filter(|d| d.file_name().is_some_and(|n| n == name))
    }
    let Some(spec) = spec_dir.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let claude = named(named(spec_dir.parent(), "spec").and_then(Path::parent), ".claude");
    let Some(root) = claude.and_then(Path::parent) else {
        return false;
    };
    crate::shared::spec_state::approved(root, spec)
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
/// what the approval exists to prevent. Composed here (rather than inlined at
/// the emission) so the wording is assertable; a test that wants to pin the
/// EMISSION drives [`scaffold_warning_to`] with its own sink.
fn frozen_plan_warn() -> String {
    "[wave-scaffold] WARN: the plan does not match the layout on disk, which is FROZEN (the \
     spec is approved, or the spec left PLAN, or the waves were user-rejected). No existing \
     file was rewritten, no wave was pruned and the wave count was left as approved; only \
     artefacts missing from disk were restored. Route the change through a change request \
     (state it in chat; it is recorded in the spec's change-log.md), never through a silent \
     re-plan."
        .to_string()
}

/// Os números que FALTAM na numeração das ondas de um plano, em ordem.
///
/// A contagem que vai para o sidecar é `plan.waves.len()`, mas cada diretório é
/// nomeado pelo `n` que a onda declara — então um plano com as ondas 1, 2 e 4
/// materializa três diretórios e uma tabela que pula o 3, e nada em lugar nenhum
/// nota o buraco. Aqui ele vira um WARN nominal.
///
/// Deliberadamente NÃO renumera: `wave-advance` ordena por nível e é indiferente
/// ao número, e reescrever o `n` do autor mudaria os nomes de diretório debaixo
/// de um plano que talvez já esteja citado em prosa. O buraco quase sempre é uma
/// onda removida à mão — o aviso é para o autor decidir.
///
/// A faixa examinada é `1..=maior n declarado`, então uma numeração que começa
/// em 2 acusa o 1 que falta. Um `n` repetido não é buraco e não aparece aqui.
fn numbering_gaps(plan: &Plan) -> Vec<u32> {
    let declared: BTreeSet<u32> = plan.waves.iter().map(|w| w.n).collect();
    let Some(&highest) = declared.iter().next_back() else {
        return Vec::new();
    };
    (1..=highest).filter(|n| !declared.contains(n)).collect()
}

/// A prosa do WARN de numeração — composta aqui (e não inline na emissão) pelo
/// mesmo motivo de [`frozen_plan_warn`]: para o texto ser assertável. Quem
/// decide EMITI-LO é [`scaffold_warning_to`], e é lá que o teste do critério
/// entra — a prosa certa num aviso que ninguém emite não vale nada.
fn numbering_gap_warn(gaps: &[u32]) -> String {
    let missing: Vec<String> = gaps.iter().map(u32::to_string).collect();
    format!(
        "[wave-scaffold] WARN: plan wave numbering has a hole — no wave {}. Each directory is \
         named by the `n` its wave declares, so the layout on disk skips the same number; \
         re-number the waves in plan.json, or leave the hole deliberately (wave-advance orders \
         by dependency level and never reads the number).",
        missing.join(", "),
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
    "  \"total_waves\": 1\n",
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
        /// Claims the plan's own contents refute — a wave that does work and
        /// satisfies a criterion while declaring no files. Escalatable like
        /// `uncovered_acs`, and kept SEPARATE from it: one list answering two
        /// questions would tell a consumer a claimed criterion is uncovered.
        unsupportable_claims: Vec<String>,
        /// Criteria whose command inspects a path no claiming wave declares —
        /// the SUFFICIENCY gap. Escalatable, and a THIRD separate list for the
        /// reason the first two are separate: a consumer asking "is AC-1
        /// uncovered" must not get `true` for a criterion that IS claimed by a
        /// wave that simply cannot reach one of its paths.
        ///
        /// It reached no consumer at all before: the list was computed, printed
        /// to stderr, and then dropped when this outcome was built.
        criteria_outside_claimants: Vec<String>,
        /// Ondas que fazem trabalho (`tasks`) e não traçam a critério que
        /// existe, e ids de `satisfies` que não nomeiam critério nenhum — a
        /// QUARTA lista, e a única ADVISORY: `plan-materialize` a publica como
        /// `untraced_waves` e NÃO retém o plano por ela. Separada das outras
        /// três pelo mesmo motivo de sempre: quem pergunta "que critério ficou
        /// sem onda" (cobertura) não pode receber como resposta uma ONDA. Aqui o
        /// sujeito é a onda, e a linha a editar é a dela.
        untraced_waves: Vec<String>,
    },
    /// `plan.waves` was empty — operator error (a hard gate).
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
///
/// Uma linha só: todo o miolo é [`scaffold_warning_to`], com o stderr do
/// processo como destino dos avisos. Nada além da escolha do destino mora aqui,
/// para que um teste que dirige o materializador com outro destino exercite o
/// materializador INTEIRO — inclusive a fiação que decide se cada aviso sai.
pub(crate) fn scaffold(spec_dir: &Path, plan_path: &Path) -> ScaffoldOutcome {
    scaffold_warning_to(spec_dir, plan_path, &mut std::io::stderr())
}

/// [`scaffold`] com o destino dos avisos INJETADO.
///
/// Os WARNs deste materializador (total declarado divergente, buraco na
/// numeração, onda sem tarefa, os quatro grupos de rastreabilidade, o plano
/// congelado) saíam por `eprintln!` direto, e o que um teste consegue observar
/// de um `eprintln!` é nada — então cada aviso só tinha teste da função que
/// COMPÕE o texto, nunca da fiação que decide EMITI-LO. Apagar um `if` inteiro
/// aqui deixava a suíte verde.
///
/// O destino é um `&mut dyn Write` porque é a menor mudança que fecha isso: em
/// produção ele é `std::io::stderr()` (ver [`scaffold`]) e num teste é um
/// `Vec<u8>`, com o mesmo código correndo dos dois lados. Falha de escrita é
/// ignorada de propósito — um aviso que não sai nunca pode derrubar a
/// materialização.
pub(crate) fn scaffold_warning_to(
    spec_dir: &Path,
    plan_path: &Path,
    warn: &mut dyn std::io::Write,
) -> ScaffoldOutcome {
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
    // A mismatch is an operator typo, not fatal: warn and continue
    // using the actual length so the table matches the directories on disk.
    if let Some(declared) = plan.total_waves {
        let actual = plan.waves.len() as u32;
        if declared != actual {
            let _ = writeln!(
                warn,
                "[wave-scaffold] WARN: plan.total_waves={declared} but waves.length={actual}; \
                 using {actual}",
            );
        }
    }
    // Sibling signal, same severity and same reason to exist: the count and the
    // per-wave `n` are two different numbers, and only the count was ever
    // cross-checked. A hole in the numbering is an operator edit nobody notices.
    let gaps = numbering_gaps(&plan);
    if !gaps.is_empty() {
        let _ = writeln!(warn, "{}", numbering_gap_warn(&gaps));
    }

    let parent_name = spec_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // Every `meta.json` records the project's text language, the one the spec
    // narrative is written in; the plan has no say. The headings, by contrast,
    // render MACHINE artefacts, so they are ENGLISH-FIXED whatever it is.
    let lang = mustard_core::ProjectConfig::load(&mustard_core::io::spec_events::spec_root(spec_dir))
        .language()
        .text_or_default()
        .as_str();
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

    // O spec monolítico do pai, lido UMA vez — e uma vez de verdade: é `spec.md`
    // no tempo do PLAN, ou `spec.original.md` depois que um rewave arquivou o
    // original, e o fallback só é tentado quando a primeira leitura falhou.
    //
    // DOIS consumidores saem daqui: o recorte de `## Material` por onda e o
    // portão de rastreabilidade. Os critérios NÃO: a onda leva só os ids que
    // satisfaz, e o prompt lê o texto do pai na hora do despacho. Pai ausente
    // (um re-wave antes de o arquivo aterrissar) devolve `None`: o material não
    // renderiza e o portão não recebe id nenhum do pai.
    let parent_ac_md = fs::read_to_string(spec_dir.join("spec.md"))
        .or_else(|_| fs::read_to_string(spec_dir.join("spec.original.md")))
        .ok();
    let parent_material_text: &str = parent_ac_md.as_deref().unwrap_or_default();

    // Per-wave spec. A wave the Plan agent left with no `tasks` is a visible
    // signal — emit a stderr WARN so the operator notices the gap instead of it
    // silently materialising an empty TASK block downstream.
    for w in &plan.waves {
        if w.tasks.is_empty() {
            let _ = writeln!(
                warn,
                "[wave-scaffold] WARN: wave-{n}-{role} materialised with no tasks — \
                 agent-prompt-render will fall back to an empty task block",
                n = w.n,
                role = w.role,
            );
        }
        let dir = spec_dir.join(wave_name(w));
        ledger.emit(
            &dir.join("spec.md"),
            &render_wave_spec(&parent_name, w, &hd, parent_material_text),
        );
    }

    // AC↔wave traceability: TRÊS sinais são escaláveis, ENFORCED pelo
    // `plan-materialize` (a entrada do pipeline) — sem knob de ambiente — e o
    // quarto (onda sem régua) é aviso: sai aqui na stderr e viaja no relatório.
    // O spec monolítico do pai já foi lido acima (`parent_ac_md`); um pai
    // ausente não contribui id nenhum.
    let gaps = traceability_gaps(&plan, parent_ac_md.as_deref());
    for gap in &gaps.untraced_waves {
        let _ = writeln!(warn, "[wave-scaffold] WARN: {gap}");
    }
    for gap in &gaps.uncovered_acs {
        let _ = writeln!(warn, "[wave-scaffold] WARN: {gap}");
    }
    for gap in &gaps.criteria_outside_claimants {
        let _ = writeln!(warn, "[wave-scaffold] WARN: {gap}");
    }
    for gap in &gaps.unsupportable_claims {
        let _ = writeln!(warn, "[wave-scaffold] WARN: {gap}");
    }
    // `scaffold` never exits — it stays reusable in-process (plan-materialize),
    // which blocks the PLAN transition when either list is non-empty.
    //
    // The two travel SEPARATELY even though they share a severity. Folding the
    // unsupportable claims into the uncovered list was tried and reverted: a
    // consumer asking "is AC-1 uncovered" would then get `true` for a criterion
    // that IS claimed, just not supportably — one list answering two questions,
    // which is the blunt merge this codebase keeps paying for.
    let uncovered_acs = gaps.uncovered_acs;
    let unsupportable_claims = gaps.unsupportable_claims;
    // A quarta lista, e a única ADVISORY: uma onda com tarefas e sem critério é
    // despachada sem régua (o prompt recorta o `## ACCEPTANCE` pelo `satisfies:`
    // que este mesmo conjunto escreveu), e o relatório diz isso sem reter o
    // plano — como `validation.issues`.
    let untraced_waves = gaps.untraced_waves;
    // The THIRD escalatable list, and the one that used to stop at the stderr
    // loop above: it was computed and then dropped, so no machine consumer of
    // `plan-materialize` could see it. It travels separately for the same
    // reason the other two do — see `ScaffoldOutcome::Created`.
    let criteria_outside_claimants = gaps.criteria_outside_claimants;

    // Emit `meta.json` alongside every spec.md
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
            lang: Some(lang.to_string()),
            checkpoint: None,
            parent: None,
            // Only the CUT knows which base the unit came from; a scaffold
            // never invents it (and `write_parent_meta` preserves it).
            base: None,
            is_wave_plan: Some(true),
            total_waves: Some(total_waves),
            flags: MetaFlags::default(),
            // The PARENT is a coordination doc — its actionable checklist
            // lives in each wave's sidecar (seeded below), never in the root
            // meta (explicit OUT of the checklist-progresso spec).
            checklist: Vec::new(),
            // Findings are seeded by the collector from what the review and the
            // proof ledger actually recorded — never invented at scaffold time.
            findings: Vec::new(),
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
                lang: Some(lang.to_string()),
                checkpoint: None,
                parent: Some(parent_name.clone()),
                // A wave is not a unit — its base is the parent unit's.
                base: None,
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
                findings: Vec::new(),
                raw: Value::Null,
            },
        );
    }
    // `qa/` and `review/` are pipeline *phases*, not specs — they carry no
    // lifecycle, so no `meta.json` sidecar is written for them. Only the root
    // and each `wave-N` directory get a sidecar (above). The result of each
    // phase is materialised by code into `qa/report.md` / `review/verdict.md`,
    // not tracked through a dead sidecar.

    let Ledger { created, skipped, mut refreshed, removed, drift, .. } = ledger;
    // A frozen plan says so ONCE — for any kind of divergence, including a wave
    // count the approved layout does not carry — and names the route that does
    // accept a change.
    if drift || parent_drift {
        let _ = writeln!(warn, "{}", frozen_plan_warn());
    }
    // Sorted so stdout stays byte-stable regardless of directory-read order.
    refreshed.sort();
    ScaffoldOutcome::Created {
        created,
        skipped,
        refreshed,
        removed,
        uncovered_acs,
        unsupportable_claims,
        criteria_outside_claimants,
        untraced_waves,
    }
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
/// `light` parent (drafted before `plan-prepare` bumped it Full) that the gates
/// keyed on a Full scope never recognised.
///
/// Frozen by the APPROVAL ALONE (`approved`), not by the full [`write_mode`]
/// window: once the user approved, the stored count is the count they approved
/// and stays put (returning `true` — drift — instead of writing). Bumping it
/// there produced a spec whose `wave-plan.md` listed N waves while the sidecar
/// every consumer reads (`wave-advance`, `status`, the dashboard) claimed N+1 —
/// an approved spec silently growing a wave, the mirror of what the approval
/// exists to prevent.
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
            // the gates keyed on a Full scope (which match the `full` /
            // `full (wave plan)` string: the clarify gate of `approve-spec` and
            // the resume engine's Execute gates) never engage on the parent. An
            // already-Full scope is left untouched — no churn, no false drift.
            let scope_upgrade = existing
                .scope
                .as_deref()
                .is_none_or(|s| !s.starts_with("full"));
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
                    reality_obligations: Vec::new(),
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
                    reality_obligations: Vec::new(),
                },
            ],
            total_waves: Some(2),
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
        // whatever the project's language.
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
        // English-fixed headings, never the Portuguese ones.
        assert!(md.contains("# Wave Plan"));
        assert!(md.contains("Depends on"));
        assert!(!md.contains("# Plano de Waves"));
    }

    #[test]
    fn renders_wave_spec_with_parent_link_and_no_header() {
        // Machine artefact → ENGLISH-FIXED headings.
        let hd = headings();
        let plan = sample_plan();
        let s1 = render_wave_spec("epic-x", &plan.waves[0], &hd, "");
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
        let s2 = render_wave_spec("epic-x", &plan.waves[1], &hd, "");
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
        // a MACHINE artefact, so its headings are ENGLISH-FIXED.
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
    /// the parent `scope` to `full (wave plan)` — else the gates keyed on a Full
    /// scope (which match the `full` string) never engage. Other lifecycle
    /// fields survive; an already-`full` scope is left untouched (covered by the
    /// stale-total test).
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
            }))
            .unwrap(),
        )
        .unwrap();

        let _ = scaffold(&spec_dir, &plan_path);

        let root = mustard_core::read_meta(&spec_dir.join("meta.json")).unwrap();
        // The `light` draft is upgraded so the gates keyed on a Full scope recognise it.
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
        }))
        .unwrap();
        let plan: Plan = serde_json::from_str(&raw).expect("summary-only plan deserialises");
        assert!(plan.waves[0].tasks.is_empty());
        assert!(plan.waves[0].files.is_empty());
        assert!(plan.waves[0].acceptance.is_empty());
        let hd = headings();
        let spec = render_wave_spec("epic", &plan.waves[0], &hd, "");
        assert!(spec.contains("## Summary"));
        assert!(spec.contains("## Network"));
        // No materialised body → no Tasks / Files heading.
        assert!(!spec.contains("## Tasks"), "no bare Tasks heading: {spec}");
        assert!(!spec.contains("## Files"), "no bare Files heading: {spec}");
    }

    /// The wave's OWN `spec.md` carries the decisions and traps that govern it.
    ///
    /// The human who opens a wave file is the operator checking the plan, the
    /// reviewer reading the change, or someone picking the work up months later.
    /// Until now that file held tasks, files and criteria and NOT one reason for
    /// any of them — the material reached only the rendered dispatch prompt. The
    /// operator read a wave file, saw work with no `why`, and concluded the spec
    /// was shallow. They were describing exactly what was there.
    ///
    /// The cut is the same one the prompt uses: definitions and decisions bind
    /// every wave, a finding rides to the wave that declares its file.
    #[test]
    fn render_wave_spec_carries_the_parents_material_for_this_wave() {
        let w = WavePlanEntry {
            n: 1,
            role: "backend".to_string(),
            summary: "the contract".to_string(),
            depends_on: vec![],
            tasks: vec!["wire the handler".to_string()],
            files: vec!["src/api/handler.rs".to_string()],
            acceptance: vec![],
            satisfies: Vec::new(),
            reality_obligations: Vec::new(),
        };
        let parent = "# Epic\n\n## Definitions\n\n- [D-1] **wave** — one agent, one pass\n\n\
                      ## Decisions\n\n- [K-1] everything branches off dev\n  Reason: the train\n\n\
                      ## Evidence\n\n- [E-1] the handler parses twice\n  \
                      Evidence: `src/api/handler.rs:12`\n\
                      - [E-2] the widget leaks\n  Evidence: `src/ui/widget.tsx:3`\n";
        let spec = render_wave_spec("epic", &w, &headings(), parent);

        assert!(spec.contains("## Material"), "material heading missing: {spec}");
        assert!(spec.contains("**wave** — one agent, one pass"), "definition: {spec}");
        assert!(spec.contains("everything branches off dev"), "decision: {spec}");
        assert!(spec.contains("the handler parses twice"), "own finding: {spec}");
        assert!(!spec.contains("the widget leaks"), "another wave's finding leaked: {spec}");
        // The parent stays the source, and the file says so.
        assert!(spec.contains("Copied from the parent spec"), "provenance line: {spec}");

        // A parent with no material renders byte-identically to before: no
        // heading, no empty section.
        let bare = render_wave_spec("epic", &w, &headings(), "# Epic\n\n## Tasks\n\n- [ ] x\n");
        assert!(!bare.contains("## Material"), "empty channel emits no heading: {bare}");
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
            reality_obligations: Vec::new(),
        };
        let hd = headings();
        let spec = render_wave_spec("epic", &w, &hd, "");
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
        // Os dois blocos vão vazios de propósito: esta onda DECLARA `## Tasks`,
        // então o caminho estruturado vence e o ponteiro do tier 2 (o único
        // leitor deles) nem chega a ser montado.
        let steps = apr::read_task_steps(&spec_path, "", "");
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
                    reality_obligations: Vec::new(),
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
                    reality_obligations: Vec::new(),
                },
            ],
            total_waves: Some(2),
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

    /// The wave headings are ENGLISH-FIXED machine artefacts: a wave whose
    /// tasks are written in Portuguese still renders `## Tasks`, never
    /// `## Tarefas`.
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
            reality_obligations: Vec::new(),
        };
        let spec = render_wave_spec(
            "epic",
            &entry(vec!["fazer X".to_string()]),
            &headings(),
            "",
        );
        assert!(spec.contains("## Tasks"), "machine artefact → ## Tasks: {spec}");
        assert!(!spec.contains("## Tarefas"), "no PT heading even for Portuguese tasks: {spec}");
    }

    /// Todo `meta.json` que o materializador grava leva o idioma do texto do
    /// projeto, e o plano não tem voz nisso: um `lang` esquecido num plano
    /// antigo é ignorado. Sem idioma declarado, fica o português do Brasil.
    #[test]
    fn scaffold_records_the_project_text_language_in_every_meta() {
        let old_plan = |dir: &Path| {
            let plan_path = dir.join("plan.json");
            std::fs::write(
                &plan_path,
                serde_json::to_string(&json!({
                    "waves": [
                        { "n": 1, "role": "general", "summary": "a", "depends_on": [] },
                        { "n": 2, "role": "general", "summary": "b", "depends_on": ["wave-1-general"] }
                    ],
                    "total_waves": 2,
                    "lang": "pt"
                }))
                .unwrap(),
            )
            .unwrap();
            plan_path
        };
        let langs = |spec_dir: &Path| -> Vec<Option<String>> {
            ["meta.json", "wave-1-general/meta.json", "wave-2-general/meta.json"]
                .iter()
                .map(|file| mustard_core::read_meta(&spec_dir.join(file)).unwrap().lang)
                .collect()
        };

        let english = tempdir().unwrap();
        std::fs::write(english.path().join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        let spec_dir = english.path().join(".claude/spec/epic-lang");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let _ = scaffold(&spec_dir, &old_plan(english.path()));
        assert_eq!(langs(&spec_dir), vec![Some("en-US".to_string()); 3], "the project's language, not the plan's");

        let bare = tempdir().unwrap();
        let spec_dir = bare.path().join(".claude/spec/epic-lang");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let _ = scaffold(&spec_dir, &old_plan(bare.path()));
        assert_eq!(langs(&spec_dir), vec![Some("pt-BR".to_string()); 3], "no declared language gives pt-BR");
    }

    /// A plan whose `tasks` already carry the checkbox
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
            reality_obligations: Vec::new(),
        };
        let spec = render_wave_spec("epic", &w, &headings(), "");
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

    /// The scaffold seeds each
    /// wave's `meta.json#checklist` with one `{label, path, done:false}` item
    /// per target file; the PARENT root meta carries NO checklist (explicit
    /// OUT). The sidecar follows the write mode: reconciled back onto the plan
    /// while the spec is unapproved (EXECUTE cannot have started, so no
    /// progress is lost), FROZEN once the user approved the spec — which is
    /// what keeps a `done` flag flipped by the auto-mark hook intact.
    #[test]
    fn scaffold_seeds_wave_meta_checklist_from_files() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("epic-checklist");
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
        crate::shared::spec_state::approve_in(&spec_dir);
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
            reality_obligations: Vec::new(),
        };
        let spec = render_wave_spec("epic", &w, &headings(), "");
        assert!(!spec.contains("## Tasks"), "bare empty Tasks heading is noise: {spec}");
        // Nem uma seção de critérios: a onda não declara nenhum.
        assert!(!spec.contains("## Acceptance Criteria"), "bare AC heading is noise too: {spec}");
    }

    /// Cada onda persiste no seu PRÓPRIO `spec.md` só QUAIS critérios declara
    /// satisfazer — uma linha de frontmatter, nunca o texto — e o prompt dela,
    /// recortado por essa linha, carrega os critérios literais, com
    /// `Command:` / `Expect:` / `Control:`.
    ///
    /// Dois lados de propósito: o critério da onda vizinha vazando para esta
    /// devolveria exatamente o ruído que o recorte existe para tirar.
    ///
    /// De ONDE sai o texto é a outra metade, e ela tem duas formas de plano:
    /// quando as ondas declaram linhas de `acceptance`, a união em
    /// `wave-plan.md` é a régua (e é o arquivo que o `ac-amend` reescreve e que
    /// o QA executa depois de um rewave arquivar o pai); quando o plano só
    /// NOMEIA ids do pai, `wave-plan.md` não tem seção nenhuma e o pai continua
    /// sendo a fonte. As duas formas são materializadas aqui, porque a que
    /// ninguém mede é a que quebra.
    #[test]
    fn a_wave_materialises_only_the_criteria_it_satisfies() {
        use crate::commands::agent::render::sections::read_wave_acceptance;
        const AC1: &str = "**AC-1** — o handler responde 200.\n  Command: `cargo test alpha`\n  \
                           Expect: `1 passed`\n  Control: `cargo test --list`";
        const AC2: &str = "**AC-2** — o cli imprime a tabela.\n  Command: `cargo test beta`";
        let parent_md = format!("# Epic\n\n## Acceptance Criteria\n\n- {AC1}\n- {AC2}\n");

        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-ruler");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), &parent_md).unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([
                // Onda 1 nomeia o id explicitamente; onda 2 deixa o
                // `satisfies` sair da própria linha — os dois caminhos que
                // [`satisfied_ids`] cobre, num plano só.
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do alpha"],
                  "files": ["src/alpha.rs"], "satisfies": ["AC-1"], "acceptance": [AC1] },
                { "n": 2, "role": "cli", "summary": "s", "tasks": ["do beta"],
                  "files": ["src/beta.rs"], "acceptance": [AC2] }
            ]),
        );

        let _ = scaffold(&spec_dir, &plan_path);

        let parent = spec_dir.join("spec.md");
        let w1_path = spec_dir.join("wave-1-rt").join("spec.md");
        let w1 = std::fs::read_to_string(&w1_path).unwrap();
        // O arquivo da onda diz QUAL critério a julga, e nunca copia o texto: a
        // cópia seria um retrato, e o layout congela na aprovação.
        assert!(w1.starts_with("---\nid: wave.epic-ruler.1-rt\nsatisfies: [AC-1]\n---\n"), "{w1}");
        assert!(!w1.contains("## Acceptance Criteria"), "a cópia voltou: {w1}");
        assert!(!w1.contains("cargo test alpha"), "o texto do critério não mora na onda: {w1}");
        assert_eq!(parse_wave_ruler(&w1).satisfies, ["AC-1"], "{w1}");

        // O prompt, recortado por aquela linha: os três marcadores chegam
        // LITERAIS — é o comando que julga, não uma paráfrase dele.
        let ruler = read_wave_acceptance(&parent, Some(&w1_path));
        assert!(ruler.contains("**AC-1**"), "a onda não recebeu régua: {ruler}");
        assert!(ruler.contains("Command: `cargo test alpha`"), "comando perdido: {ruler}");
        assert!(ruler.contains("Expect: `1 passed`"), "Expect perdido: {ruler}");
        assert!(ruler.contains("Control: `cargo test --list`"), "Control perdido: {ruler}");
        assert!(!ruler.contains("**AC-2**"), "critério da onda vizinha vazou: {ruler}");

        // A onda 2 nomeou o id pela linha `acceptance` inteira, sem
        // `satisfies` — o mesmo [`satisfied_ids`] a resolve.
        let w2_path = spec_dir.join("wave-2-cli").join("spec.md");
        assert_eq!(parse_wave_ruler(&std::fs::read_to_string(&w2_path).unwrap()).satisfies, ["AC-2"]);
        let ruler2 = read_wave_acceptance(&parent, Some(&w2_path));
        assert!(ruler2.contains("**AC-2**") && !ruler2.contains("**AC-1**"), "espelho: {ruler2}");

        // A união está no wave-plan.md — é dela que a régua acima foi cortada, e
        // é o arquivo que o QA executa quando o pai já foi arquivado. O teste
        // prova que é ela mesmo: com o pai APAGADO, a régua não muda.
        let plan_md = std::fs::read_to_string(spec_dir.join("wave-plan.md")).unwrap();
        assert!(plan_md.contains("## Acceptance Criteria"), "{plan_md}");
        assert!(
            plan_md.contains("**AC-2**") && plan_md.contains("Command: `cargo test beta`"),
            "a união do QA não pode encolher: {plan_md}"
        );
        std::fs::remove_file(&parent).unwrap();
        assert_eq!(read_wave_acceptance(&parent, Some(&w1_path)), ruler, "a fonte era a união");

        // A OUTRA forma de plano: só ids, nenhuma linha de `acceptance`. Aí
        // `wave-plan.md` não declara seção nenhuma e o pai volta a ser a fonte —
        // que é também o arquivo que o QA lê enquanto ele existe.
        let ids_dir = dir.path().join("epic-ids");
        std::fs::create_dir_all(&ids_dir).unwrap();
        std::fs::write(ids_dir.join("spec.md"), &parent_md).unwrap();
        let ids_plan = write_plan(
            &ids_dir,
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do alpha"],
                  "files": ["src/alpha.rs"], "satisfies": ["AC-1"] }
            ]),
        );
        let _ = scaffold(&ids_dir, &ids_plan);
        let ids_plan_md = std::fs::read_to_string(ids_dir.join("wave-plan.md")).unwrap();
        assert!(!ids_plan_md.contains("## Acceptance Criteria"), "{ids_plan_md}");
        let ids_ruler = read_wave_acceptance(
            &ids_dir.join("spec.md"),
            Some(&ids_dir.join("wave-1-rt").join("spec.md")),
        );
        assert!(ids_ruler.contains("Command: `cargo test alpha`"), "o pai é a fonte: {ids_ruler}");
        assert!(!ids_ruler.contains("**AC-2**"), "e o recorte por onda vale igual: {ids_ruler}");
    }

    /// Uma onda que declara tarefas e não traça a critério nenhum é um AVISO —
    /// na stderr e no relatório — e o plano materializa.
    ///
    /// Substitui `wave_with_tasks_and_no_criterion_is_refused`, que trancava a
    /// tese "opcional deixa de ser opcional" com três asserções que agora falham
    /// por desenho: `report["scaffold"]["error"] == json!(ERR_UNTRACED_WAVES)`
    /// (o marcador não existe mais), `refused(&report)` (a leitura única diz
    /// "não recusado") e `report["events"] == json!([])` (a transição PLAN
    /// sai). O que a mensagem diz continua real: a onda é despachada com o
    /// `## ACCEPTANCE` colapsado, e quem aprova o plano lê isso.
    #[test]
    fn wave_with_tasks_and_no_criterion_warns_and_the_plan_materialises() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-untraced");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Epic\n\n## Acceptance Criteria\n\n- **AC-1** — a. Command: `true`\n",
        )
        .unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-1"] },
                { "n": 2, "role": "cli", "summary": "s", "tasks": ["do more"],
                  "files": ["src/b.rs"] }
            ]),
        );

        // O aviso sai na STDERR — o destino injetado mede a fiação que decide
        // emiti-lo, não só o texto.
        let mut warn: Vec<u8> = Vec::new();
        let ScaffoldOutcome::Created { untraced_waves, uncovered_acs, .. } =
            scaffold_warning_to(&spec_dir, &plan_path, &mut warn)
        else {
            panic!("expected ScaffoldOutcome::Created");
        };
        let stderr = String::from_utf8(warn).unwrap();
        assert!(
            stderr.contains(
                "[wave-scaffold] WARN: wave-2-cli has tasks but traces to no criterion that exists"
            ),
            "o aviso sai na stderr, como WARN: {stderr}"
        );
        let named = untraced_waves
            .iter()
            .find(|g| g.contains("wave-2-cli"))
            .unwrap_or_else(|| panic!("a onda sem critério chega ao consumidor: {untraced_waves:?}"));
        assert!(
            named.contains("traces to no criterion that exists") && named.contains("AC-1"),
            "a frase é honesta e nomeia o que existe: {named}"
        );
        assert!(
            !untraced_waves.iter().any(|g| g.contains("wave-1-rt")),
            "a onda bem rastreada não entra na lista: {untraced_waves:?}"
        );
        // A lista viaja SEPARADA da cobertura: aqui o sujeito é a onda, e AC-1
        // está coberto.
        assert!(uncovered_acs.is_empty(), "{uncovered_acs:?}");

        // …e o COMPOSTO: sem marcador de erro, NÃO recusado, transição PLAN
        // emitida, e a lista viaja como advisory — como `validation.issues`.
        //
        // Fixture própria porque só ela ISOLA a pergunta: o critério do pai
        // precisa vir vermelho para a prova negativa aprovar, senão o relatório
        // recusaria por outro motivo e a asserção não mediria este.
        use crate::commands::pipeline::plan_materialize::{materialize, refused};
        let comp = tempdir().unwrap();
        let project = comp.path();
        let comp_spec_dir = project.join(".claude").join("spec").join("epic-untraced");
        std::fs::create_dir_all(&comp_spec_dir).unwrap();
        std::fs::write(
            comp_spec_dir.join("spec.md"),
            "# Epic\n\n## Files\n- `src/a.rs` (create)\n- `src/b.rs` (create)\n\n\
             ## Acceptance Criteria\n\
             - **AC-1** — o comportamento novo vale. Command: `cd no-such-directory-abc`\n\
             - **AC-2** — build green. Command: `cd .`\n",
        )
        .unwrap();
        let comp_plan = write_plan(
            project,
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-1", "AC-2"] },
                { "n": 2, "role": "cli", "summary": "s", "tasks": ["do more"],
                  "files": ["src/b.rs"] }
            ]),
        );

        let report = materialize(project, &comp_spec_dir, &comp_plan);
        assert!(
            report["scaffold"]["error"].is_null(),
            "um aviso não é marcador de erro: {report}"
        );
        assert!(!refused(&report), "a leitura única do relatório diz NÃO recusado: {report}");
        assert!(
            report["scaffold"]["untraced_waves"]
                .as_array()
                .is_some_and(|l| l.iter().any(|g| g.as_str().unwrap_or_default().contains("wave-2-cli"))),
            "e a lista viaja no relatório, advisory: {report}"
        );
        assert_ne!(
            report["events"],
            json!([]),
            "a transição PLAN sai de um plano avisado: {report}",
        );
        assert_eq!(report["proof"]["ok"], json!(true), "{report}");
        assert_eq!(report["sharedFiles"]["ok"], json!(true), "{report}");
    }

    /// A metade que o aviso por onda sem régua NÃO pode apertar junto: um plano
    /// cujas ondas TRAÇAM a critérios materializa com a lista vazia e sem reter
    /// a transição PLAN.
    #[test]
    fn a_plan_whose_waves_trace_to_their_criteria_is_not_refused() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-rastreado");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Epic\n\n## Contexto\n\na história\n\n## Files\n- `src/a.rs` (create)\n\n\
             ## Acceptance Criteria\n\n\
             - **AC-1** — a. Command: `true`\n\
             - **AC-2** — b. Command: `true`\n",
        )
        .unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-1"] },
                { "n": 2, "role": "cli", "summary": "s", "tasks": ["do more"],
                  "files": ["src/b.rs"], "satisfies": ["AC-2"] }
            ]),
        );

        let ScaffoldOutcome::Created { untraced_waves, uncovered_acs, .. } =
            scaffold(&spec_dir, &plan_path)
        else {
            panic!("expected ScaffoldOutcome::Created");
        };
        assert!(
            untraced_waves.is_empty(),
            "toda onda traça a um critério que existe: {untraced_waves:?}",
        );
        assert!(uncovered_acs.is_empty(), "e todo critério é reivindicado: {uncovered_acs:?}");

        // …e a mesma resposta no COMPOSTO: lista vazia, sem marcador, não
        // recusado.
        use crate::commands::pipeline::plan_materialize::{materialize, refused};
        let comp = tempdir().unwrap();
        let project = comp.path();
        let comp_spec_dir = project.join(".claude").join("spec").join("epic-rastreado");
        std::fs::create_dir_all(&comp_spec_dir).unwrap();
        std::fs::write(
            comp_spec_dir.join("spec.md"),
            "# Epic\n\n## Contexto\n\na história\n\n## Files\n\
             - `src/a.rs` (create)\n- `src/b.rs` (create)\n\n\
             ## Acceptance Criteria\n\
             - **AC-1** — o comportamento novo vale. Command: `cd no-such-directory-abc`\n\
             - **AC-2** — build green. Command: `cd .`\n",
        )
        .unwrap();
        let comp_plan = write_plan(
            project,
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-1"] },
                { "n": 2, "role": "cli", "summary": "s", "tasks": ["do more"],
                  "files": ["src/b.rs"], "satisfies": ["AC-2"] }
            ]),
        );
        let report = materialize(project, &comp_spec_dir, &comp_plan);
        assert_eq!(report["scaffold"]["untraced_waves"], json!([]), "{report}");
        assert!(report["scaffold"]["error"].is_null(), "{report}");
        assert!(!refused(&report), "{report}");
    }

    /// Um plano com ZERO critérios declarados — nem no pai, nem em onda alguma
    /// — diz isso UMA vez, como o motivo de as duas checagens por onda ficarem
    /// caladas. Não é isenção nem recusa: é a mesma frase, dita sobre o plano.
    ///
    /// Substitui `a_plan_with_no_criterion_at_all_is_refused`, que exigia uma
    /// frase "satisfies no criterion" POR ONDA (`for wave in [...] assert!(…any(|g|
    /// g.contains(wave)…))`) — asserção que agora falha por desenho: nenhuma
    /// onda é nomeada, porque a causa não é de nenhuma delas.
    #[test]
    fn a_plan_with_no_criterion_at_all_says_so_once() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-sem-regua");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // O pai tem prosa e arquivos, e critério NENHUM.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Epic\n\n## Contexto\n\na história\n\n## Files\n- `src/a.rs` (create)\n",
        )
        .unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"] },
                { "n": 2, "role": "cli", "summary": "s", "tasks": ["do more"],
                  "files": ["src/b.rs"] }
            ]),
        );

        let ScaffoldOutcome::Created { untraced_waves, .. } = scaffold(&spec_dir, &plan_path) else {
            panic!("expected ScaffoldOutcome::Created");
        };
        assert_eq!(untraced_waves.len(), 1, "dito UMA vez, não por onda: {untraced_waves:?}");
        let only = &untraced_waves[0];
        assert!(
            only.contains("no acceptance criterion is defined anywhere"),
            "a frase nomeia a causa: {only}"
        );
        assert!(
            !only.contains("wave-1-rt") && !only.contains("wave-2-cli"),
            "e não culpa onda nenhuma — a causa é do plano: {only}"
        );

        // E o id FANTASMA num plano assim não ganha frase própria: sem conjunto
        // definido não há contra o que compará-lo, e a única frase é a de cima.
        let phantom_plan = write_plan(
            dir.path(),
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-99"] }
            ]),
        );
        let ScaffoldOutcome::Created { untraced_waves: phantom, .. } =
            scaffold(&spec_dir, &phantom_plan)
        else {
            panic!("expected ScaffoldOutcome::Created");
        };
        assert_eq!(phantom.len(), 1, "{phantom:?}");
        assert!(
            !phantom[0].contains("AC-99"),
            "sem conjunto definido não há id fantasma a nomear: {phantom:?}",
        );
    }

    /// Um `satisfies` inteiramente FANTASMA num plano onde nada está definido
    /// não passa em silêncio: o relatório diz, uma vez, que nada define
    /// critério — e o plano materializa, sem recusa, com o `## ACCEPTANCE` de
    /// cada onda colapsado, que é exatamente o que a frase avisa.
    ///
    /// A asserção de RECUSA que este teste carregava
    /// (`report["scaffold"]["error"] == json!(ERR_UNTRACED_WAVES)` e
    /// `refused(&report)`) trancava a tese que esta rodada remove, e agora
    /// falha por desenho. O par: uma onda de um plano com critérios continua
    /// carregando a régua dela.
    #[test]
    fn a_wholly_phantom_satisfies_does_not_buy_a_ruler_less_dispatch() {
        use crate::commands::agent::render::sections::read_wave_acceptance;
        use crate::commands::pipeline::plan_materialize::{materialize, refused};

        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec_dir = project.join(".claude").join("spec").join("epic-fantasma-total");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // Prosa e arquivos, e NENHUM `## Acceptance Criteria`.
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Epic\n\n## Contexto\n\na história\n\n## Files\n\
             - `src/a.rs` (create)\n- `src/b.rs` (create)\n",
        )
        .unwrap();
        let plan_path = write_plan(
            project,
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-1"] },
                { "n": 2, "role": "cli", "summary": "s", "tasks": ["do more"],
                  "files": ["src/b.rs"], "satisfies": ["AC-1"] }
            ]),
        );

        let report = materialize(project, &spec_dir, &plan_path);
        assert!(report["scaffold"]["error"].is_null(), "um aviso não recusa: {report}");
        assert!(!refused(&report), "{report}");
        let advisory = report["scaffold"]["untraced_waves"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert_eq!(advisory.len(), 1, "dito uma vez: {report}");
        assert!(
            advisory[0].as_str().unwrap_or_default().contains("no acceptance criterion is defined anywhere"),
            "e nomeia a causa: {report}"
        );
        // O que a frase avisa é verdade: a onda é despachada SEM régua — o pai
        // não define o id, então o prompt não renderiza nada por ele.
        let w1_path = spec_dir.join("wave-1-rt").join("spec.md");
        let ruler = read_wave_acceptance(&spec_dir.join("spec.md"), Some(&w1_path));
        assert!(ruler.is_empty(), "um id que nada define não rende régua: {ruler}");

        // A metade que NÃO pode ser apertada junto: um plano cujas ondas traçam a
        // critérios que EXISTEM materializa com a lista vazia, e a régua chega ao
        // prompt da onda — lida do pai pelo `satisfies:` que a onda carrega.
        let ok_dir = tempdir().unwrap();
        let ok_spec = ok_dir.path().join("epic-com-regua");
        std::fs::create_dir_all(&ok_spec).unwrap();
        std::fs::write(
            ok_spec.join("spec.md"),
            "# Epic\n\n## Acceptance Criteria\n\n\
             - **AC-1** — alpha vale.\n  Command: `cargo test alpha`\n  Expect: `1 passed`\n",
        )
        .unwrap();
        let ok_plan = write_plan(
            ok_dir.path(),
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-1"] }
            ]),
        );
        let ScaffoldOutcome::Created { untraced_waves, .. } = scaffold(&ok_spec, &ok_plan) else {
            panic!("expected ScaffoldOutcome::Created");
        };
        assert!(untraced_waves.is_empty(), "a onda traça a um critério real: {untraced_waves:?}");
        let ruler =
            read_wave_acceptance(&ok_spec.join("spec.md"), Some(&ok_spec.join("wave-1-rt").join("spec.md")));
        assert!(ruler.contains("**AC-1**"), "a régua não chegou à onda: {ruler}");
        assert!(
            ruler.contains("Command: `cargo test alpha`"),
            "e o comando que a julga tem de vir literal: {ruler}",
        );
    }

    /// Um `satisfies` que nomeia critério NENHUM é NOMEADO pelo aviso, com o id
    /// e os ids que existem — e o plano materializa.
    ///
    /// Substitui `a_satisfies_id_naming_no_criterion_is_refused`. O buraco que
    /// fechava continua fechado: `AC-01` onde o pai define `AC-1` devolve um
    /// conjunto satisfeito NÃO-vazio, o pai não tem esse id, e o
    /// `## ACCEPTANCE` do prompt colapsa. O que muda é a forma do sinal: uma
    /// frase honesta, derivada do mesmo conjunto que o renderizador recorta, em
    /// vez de uma recusa.
    #[test]
    fn a_satisfies_id_naming_no_criterion_is_named_by_the_warn() {
        use crate::commands::agent::render::sections::read_wave_acceptance;
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("epic-fantasma");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Epic\n\n## Acceptance Criteria\n\n\
             - **AC-1** — a. Command: `true`\n\
             - **AC-2** — b. Command: `true`\n",
        )
        .unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-01"] },
                { "n": 2, "role": "cli", "summary": "s", "tasks": ["do more"],
                  "files": ["src/b.rs"], "satisfies": ["AC-1", "AC-2"] }
            ]),
        );

        let ScaffoldOutcome::Created { untraced_waves, .. } = scaffold(&spec_dir, &plan_path) else {
            panic!("expected ScaffoldOutcome::Created");
        };
        let named = untraced_waves
            .iter()
            .find(|g| g.contains("wave-1-rt names AC-01, which no criterion defines"))
            .unwrap_or_else(|| panic!("o id fantasma tem de ser nomeado: {untraced_waves:?}"));
        assert!(named.contains("AC-1") && named.contains("AC-2"), "e os que existem: {named}");
        // O mesmo conjunto responde a outra pergunta: a onda 1 não traça a
        // critério NENHUM que exista, e a frase é a mesma da onda sem
        // `satisfies` — derivada do conjunto, nunca do tamanho da lista.
        assert!(
            untraced_waves
                .iter()
                .any(|g| g.contains("wave-1-rt has tasks but traces to no criterion that exists")),
            "uma lista só de fantasmas é o mesmo despacho sem régua: {untraced_waves:?}"
        );
        assert!(
            !untraced_waves.iter().any(|g| g.contains("wave-2-cli")),
            "a onda que declara ids reais não entra na lista: {untraced_waves:?}"
        );

        // E o outro lado: a onda 1 realmente despacha SEM régua — que é o que
        // as duas frases dizem. A linha está lá (o plano a escreveu), e o pai
        // não tem o id.
        let w1_path = spec_dir.join("wave-1-rt").join("spec.md");
        let w1 = std::fs::read_to_string(&w1_path).unwrap();
        assert_eq!(parse_wave_ruler(&w1).satisfies, ["AC-01"], "{w1}");
        assert!(
            read_wave_acceptance(&spec_dir.join("spec.md"), Some(&w1_path)).is_empty(),
            "um id que não existe não renderiza critério nenhum"
        );
    }

    /// O frontmatter da onda é escrito e lido pelo MESMO par — a linha
    /// `satisfies:` que o renderizador escreve do `plan.json` é exatamente a
    /// que o prompt lê de volta, inclusive na marca de união do rewave.
    #[test]
    fn satisfies_frontmatter_round_trips() {
        let w = WavePlanEntry {
            n: 2,
            role: "cli".to_string(),
            summary: "s".to_string(),
            depends_on: vec![],
            tasks: vec!["do it".to_string()],
            files: vec!["src/b.rs".to_string()],
            acceptance: vec![],
            satisfies: vec!["ac-2".to_string(), "AC-2".to_string(), " AC-5 ".to_string()],
            reality_obligations: Vec::new(),
        };
        let spec = render_wave_spec("epic", &w, &headings(), "");
        assert!(spec.starts_with("---\nid: wave.epic.2-cli\nsatisfies: [AC-2, AC-5]\n---\n\n"), "{spec}");
        let ruler = parse_wave_ruler(&spec);
        assert_eq!(ruler.satisfies, ["AC-2", "AC-5"]);
        assert!(!ruler.carried_whole);

        // O rewave marca a união como união, e o leitor a reconhece.
        let whole = render_wave_spec("epic", &w, &headings_for_rewave(), "");
        assert!(whole.contains("\nsatisfies-scope: unit\n"), "{whole}");
        assert!(parse_wave_ruler(&whole).carried_whole);

        // Uma onda sem a linha não tem régua nenhuma, e um documento sem
        // frontmatter também não — as duas silêncios que o prompt lê como
        // "nenhum critério", nunca como "todos".
        let bare = "---\nid: wave.epic.3-x\n---\n\n# W\n";
        assert_eq!(parse_wave_ruler(bare), WaveRuler::default());
        assert_eq!(parse_wave_ruler("# W\n\n## Tasks\n"), WaveRuler::default());
    }

    /// A onda que NÃO declara trabalho nenhum ainda tem o id fantasma NOMEADO
    /// — um erro de digitação é um erro de digitação — mas nunca é dita "sem
    /// régua" (ela não trabalha), e o plano não é recusado.
    ///
    /// Este teste media, sob a tese removida, que o aviso do fantasma não
    /// recusava o PLANO INTEIRO por uma onda de verificação. Agora nada recusa,
    /// então o que ele mede é o TEXTO: qual frase sai para uma onda sem tarefa,
    /// e qual não sai. A asserção antiga `untraced_waves.is_empty()` falha por
    /// desenho — o fantasma é nomeado com ou sem tarefas.
    #[test]
    fn a_task_less_wave_with_a_phantom_id_does_not_refuse_the_plan() {
        use crate::commands::pipeline::plan_materialize::{materialize, refused};
        let dir = tempdir().unwrap();
        let project = dir.path();
        let spec_dir = project.join(".claude").join("spec").join("epic-sem-tarefa");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# Epic\n\n## Files\n- `src/a.rs` (create)\n\n\
             ## Acceptance Criteria\n\
             - **AC-1** — o comportamento novo vale. Command: `cd no-such-directory-abc`\n\
             - **AC-2** — build green. Command: `cd .`\n",
        )
        .unwrap();
        let plan_path = write_plan(
            project,
            json!([
                { "n": 1, "role": "rt", "summary": "s", "tasks": ["do it"],
                  "files": ["src/a.rs"], "satisfies": ["AC-1", "AC-2"] },
                // A onda de verificação: nada a fazer, e um id com erro de
                // digitação.
                { "n": 2, "role": "qa", "summary": "s", "tasks": [],
                  "files": [], "satisfies": ["AC-01"] }
            ]),
        );

        let report = materialize(project, &spec_dir, &plan_path);
        assert!(!refused(&report), "nada recusa: {report}");
        assert!(report["scaffold"]["error"].is_null(), "{report}");
        let advisory: Vec<String> = report["scaffold"]["untraced_waves"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|g| g.as_str().unwrap_or_default().to_string())
            .collect();
        assert!(
            advisory.iter().any(|g| g.contains("wave-2-qa names AC-01, which no criterion defines")),
            "o erro de digitação é nomeado mesmo sem tarefas: {advisory:?}"
        );
        assert!(
            !advisory.iter().any(|g| g.contains("wave-2-qa has tasks")),
            "uma onda sem tarefa nenhuma não trabalha, então não é dita sem régua: {advisory:?}"
        );
        assert!(
            !advisory.iter().any(|g| g.contains("wave-1-rt")),
            "e a onda que declara ids reais não entra na lista: {advisory:?}"
        );
    }

    /// One wave, fully specified — the fixture the claim-support gaps need,
    /// since they turn on `files` as much as on `satisfies`.
    fn claim_wave(
        n: u32,
        role: &str,
        tasks: Vec<&str>,
        files: Vec<&str>,
        satisfies: Vec<&str>,
    ) -> WavePlanEntry {
        WavePlanEntry {
            n,
            role: role.to_string(),
            summary: "s".to_string(),
            depends_on: vec![],
            tasks: tasks.into_iter().map(String::from).collect(),
            files: files.into_iter().map(String::from).collect(),
            acceptance: vec![],
            satisfies: satisfies.into_iter().map(String::from).collect(),
            reality_obligations: Vec::new(),
        }
    }

    fn claim_plan(waves: Vec<WavePlanEntry>) -> Plan {
        let total = waves.len() as u32;
        Plan { waves, total_waves: Some(total) }
    }

    /// Materializa um plano com as ondas numeradas `ns` e devolve o que o
    /// materializador ESCREVEU no destino de avisos.
    ///
    /// Dirige [`scaffold_warning_to`] — o miolo inteiro de [`scaffold`], com um
    /// `Vec<u8>` no lugar do stderr do processo. É o que separa este teste do que
    /// ele era: chamar `numbering_gaps` + `numbering_gap_warn` direto media a
    /// prosa e deixava a FIAÇÃO sem rede, então apagar o `if` que emite o aviso
    /// mantinha a suíte verde.
    fn scaffold_warnings(project: &Path, slug: &str, ns: &[u32]) -> String {
        let spec_dir = project.join(slug);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let waves: Vec<Value> = ns
            .iter()
            .map(|n| {
                json!({
                    "n": n, "role": "rt", "summary": "s", "depends_on": [],
                    "tasks": ["do it"], "files": [format!("src/w{n}.rs")]
                })
            })
            .collect();
        let plan_path = project.join(format!("{slug}.json"));
        std::fs::write(
            &plan_path,
            serde_json::to_string(&json!({
                "waves": waves,
                // Igual ao número de ondas: o WARN de total divergente é outro
                // sinal e não pode entrar no lugar do que se mede aqui.
                "total_waves": ns.len(),
            }))
            .unwrap(),
        )
        .unwrap();
        let mut sink: Vec<u8> = Vec::new();
        let _ = scaffold_warning_to(&spec_dir, &plan_path, &mut sink);
        String::from_utf8_lossy(&sink).into_owned()
    }

    /// Um buraco na numeração das ondas vira aviso NOMINAL: o
    /// MATERIALIZADOR escreve o WARN, e o WARN diz quais números faltam.
    ///
    /// A contagem gravada no sidecar é `plan.waves.len()` e cada diretório é
    /// nomeado pelo `n` declarado; as duas coisas nunca foram confrontadas, então
    /// um plano de ondas 1, 2 e 4 materializava três diretórios com um 3 ausente
    /// que ninguém via.
    ///
    /// Dirige o materializador de ponta a ponta e observa o que ele EMITIU —
    /// nunca as duas funções auxiliares direto. Bilateral: uma numeração contígua
    /// não escreve nada, logo a asserção não pode passar por o aviso disparar
    /// sempre.
    #[test]
    fn scaffold_avisa_numeracao_com_buraco() {
        let dir = tempdir().unwrap();
        let project = dir.path();

        let holed = scaffold_warnings(project, "epic-holed", &[1, 2, 4]);
        assert!(
            holed.contains("[wave-scaffold] WARN: plan wave numbering has a hole"),
            "o materializador tem de emitir o aviso: {holed}"
        );
        assert!(holed.contains("no wave 3"), "e o aviso tem de nomear o número: {holed}");
        // O diretório da onda 4 existe com um wave-3 ausente ao lado — o buraco
        // que o aviso descreve é real, não uma leitura só do JSON.
        assert!(project.join("epic-holed").join("wave-4-rt").is_dir(), "{holed}");
        assert!(!project.join("epic-holed").join("wave-3-rt").exists(), "{holed}");

        // Uma numeração que começa em 2 também tem buraco — o 1 que falta.
        let offset = scaffold_warnings(project, "epic-offset", &[2, 3]);
        assert!(offset.contains("no wave 1"), "o 1 ausente também é buraco: {offset}");

        // O outro lado: contígua é silêncio.
        let contiguous = scaffold_warnings(project, "epic-contiguous", &[1, 2]);
        assert!(
            !contiguous.contains("numbering has a hole"),
            "uma numeração contígua não pode acusar nada: {contiguous}"
        );
    }

    /// A claim the plan's own contents refute: the wave says it covers
    /// the criterion and, in the same document, declares nowhere to do the work.
    ///
    /// It joins the ESCALATABLE list (the one `plan-materialize` refuses on)
    /// rather than the advisory one, because no reading of the plan makes it
    /// hold — this is a contradiction, not a judgement about sufficiency.
    #[test]
    fn a_wave_claiming_a_criterion_with_nowhere_to_work_is_refused() {
        let parent = "# S\n\n## Acceptance Criteria\n\n- **AC-1** — it holds. Command: `true`\n";
        let gaps = traceability_gaps(
            &claim_plan(vec![claim_wave(1, "rt", vec!["do it"], vec![], vec!["AC-1"])]),
            Some(parent),
        );
        assert!(
            gaps.unsupportable_claims
                .iter()
                .any(|g| g.contains("AC-1") && g.contains("wave-1-rt") && g.contains("NO files")),
            "the refusal must name the criterion AND the wave that cannot support it: {:?}",
            gaps.unsupportable_claims
        );

        // A summary-only STUB is not a commitment: no tasks means the plan is
        // still being written, and refusing it would block authoring rather than
        // a contradiction.
        let stub = traceability_gaps(
            &claim_plan(vec![claim_wave(1, "rt", vec![], vec![], vec!["AC-1"])]),
            Some(parent),
        );
        assert!(
            stub.unsupportable_claims.is_empty(),
            "a stub with no tasks must not be refused: {:?}",
            stub.unsupportable_claims
        );
    }

    /// A criterion whose command inspects a path none of its claimants
    /// declares is reported on its OWN channel, apart from the coverage and
    /// contradiction lists.
    ///
    /// The separation is the point: coverage asks whether SOME wave claimed the
    /// id, this asks whether the claiming wave can actually satisfy it, and a
    /// consumer reading one list for the other fixes the wrong plan.
    #[test]
    fn a_criterion_pointing_outside_its_claimants_is_flagged_not_refused() {
        let parent = "# S\n\n## Acceptance Criteria\n\n\
                      - **AC-1** — the doc says it. Command: `rg -q Modelo plugin/commands/close.md`\n";
        // Wave 1 claims AC-1 but declares only its own source file.
        let gaps = traceability_gaps(
            &claim_plan(vec![claim_wave(
                1,
                "rt",
                vec!["do it"],
                vec!["apps/rt/src/lib.rs"],
                vec!["AC-1"],
            )]),
            Some(parent),
        );
        assert!(
            gaps.criteria_outside_claimants
                .iter()
                .any(|g| g.contains("AC-1") && g.contains("plugin/commands/close.md")),
            "the flag must name the criterion and the path: {:?}",
            gaps.criteria_outside_claimants
        );
        assert!(
            gaps.unsupportable_claims.is_empty() && gaps.uncovered_acs.is_empty(),
            "a path mismatch must NOT reach the refusal channel: {:?} / {:?}",
            gaps.unsupportable_claims,
            gaps.uncovered_acs
        );

        // The claimant declaring that very path is clean — so the check is not
        // simply flagging every criterion that names anything.
        let ok = traceability_gaps(
            &claim_plan(vec![claim_wave(
                1,
                "rt",
                vec!["do it"],
                vec!["plugin/commands/close.md"],
                vec!["AC-1"],
            )]),
            Some(parent),
        );
        assert!(
            ok.criteria_outside_claimants.is_empty(),
            "a declared path must not be flagged: {:?}",
            ok.criteria_outside_claimants
        );

        // The match is on whole path COMPONENTS, not raw suffixes. A bare
        // suffix compare answers `true` for `apps/backend/src/data.rs` against a
        // declared `a.rs` and silences the mismatch — which is the field
        // scenario this check exists for (a criterion needing a backend no
        // claiming wave had in scope), found by review and reproduced live.
        let backend = "# S\n\n## Acceptance Criteria\n\n\
                       - **AC-1** — a. Command: `rg -q x apps/backend/src/data.rs`\n";
        let boundary = traceability_gaps(
            &claim_plan(vec![claim_wave(1, "rt", vec!["a"], vec!["a.rs"], vec!["AC-1"])]),
            Some(backend),
        );
        assert!(
            boundary
                .criteria_outside_claimants
                .iter()
                .any(|g| g.contains("apps/backend/src/data.rs")),
            "a declared `a.rs` must NOT silence `apps/backend/src/data.rs`: {:?}",
            boundary.criteria_outside_claimants
        );
        // And the legitimate subproject-relative spelling still matches, so the
        // boundary did not turn into a false flag on a correct plan.
        let relative = traceability_gaps(
            &claim_plan(vec![claim_wave(1, "rt", vec!["a"], vec!["src/data.rs"], vec!["AC-1"])]),
            Some(backend),
        );
        assert!(
            relative.criteria_outside_claimants.is_empty(),
            "a subproject-relative declaration still matches: {:?}",
            relative.criteria_outside_claimants
        );

        // A DECLARED entry may be a GLOB. Compared byte for byte it matches
        // nothing, so a correct plan that declares its files as a pattern was
        // guaranteed a flag — and now that this list REFUSES, guaranteed a
        // refusal. It is matched as the glob it is.
        let globbed = traceability_gaps(
            &claim_plan(vec![claim_wave(
                1,
                "rt",
                vec!["a"],
                vec!["apps/backend/src/*.rs"],
                vec!["AC-1"],
            )]),
            Some(backend),
        );
        assert!(
            globbed.criteria_outside_claimants.is_empty(),
            "a declared glob covers the path it expands to: {:?}",
            globbed.criteria_outside_claimants
        );
        // Two-sided: a glob that does NOT cover the path still flags, so the
        // wildcard arm is not a blanket amnesty.
        let elsewhere = traceability_gaps(
            &claim_plan(vec![claim_wave(
                1,
                "rt",
                vec!["a"],
                vec!["apps/frontend/src/*.rs"],
                vec!["AC-1"],
            )]),
            Some(backend),
        );
        assert!(
            elsewhere
                .criteria_outside_claimants
                .iter()
                .any(|g| g.contains("apps/backend/src/data.rs")),
            "a glob over another tree covers nothing here: {:?}",
            elsewhere.criteria_outside_claimants
        );
    }

    /// A wave already refused by Gap 3 must not ALSO be flagged by Gap 4, once
    /// per path its criteria name: one fact, one message. Reported by review —
    /// the advisory noise would bury the refusal that actually matters.
    #[test]
    fn a_refused_claim_is_not_also_flagged_path_by_path() {
        let parent = "# S\n\n## Acceptance Criteria\n\n\
                      - **AC-1** — a. Command: `rg -q x apps/rt/src/a.rs apps/rt/src/b.rs`\n";
        let gaps = traceability_gaps(
            &claim_plan(vec![claim_wave(1, "rt", vec!["do it"], vec![], vec!["AC-1"])]),
            Some(parent),
        );
        assert_eq!(
            gaps.unsupportable_claims.len(),
            1,
            "the refusal is stated once: {:?}",
            gaps.unsupportable_claims
        );
        assert!(
            gaps.criteria_outside_claimants.is_empty(),
            "an already-refused claim must not be flagged path by path: {:?}",
            gaps.criteria_outside_claimants
        );
    }

    /// Silence where the plan is consistent, AND silence where the check
    /// cannot judge.
    ///
    /// The second half is the load-bearing one: most criteria here run a NAMED
    /// TEST, so the path lives inside the test rather than in the command. Those
    /// must pass untouched — a check that guessed there would be the heuristic
    /// dressed as a gate this whole line of work exists to remove.
    #[test]
    fn a_consistent_plan_and_an_unjudgeable_one_both_stay_silent() {
        // (a) consistent: two waves, each claiming what it declares.
        let parent = "# S\n\n## Acceptance Criteria\n\n\
                      - **AC-1** — a. Command: `rg -q x apps/rt/src/a.rs`\n\
                      - **AC-2** — b. Command: `rg -q y apps/rt/src/b.rs`\n";
        let gaps = traceability_gaps(
            &claim_plan(vec![
                claim_wave(1, "rt", vec!["a"], vec!["apps/rt/src/a.rs"], vec!["AC-1"]),
                claim_wave(2, "rt", vec!["b"], vec!["apps/rt/src/b.rs"], vec!["AC-2"]),
            ]),
            Some(parent),
        );
        assert!(
            gaps.unsupportable_claims.is_empty() && gaps.criteria_outside_claimants.is_empty(),
            "a consistent plan fires neither signal: {:?} / {:?}",
            gaps.unsupportable_claims,
            gaps.criteria_outside_claimants
        );

        // (b) unjudgeable: the command names a TEST, not a path. The claimant
        // declares a file that has nothing to do with the command's text, and
        // that must still be silent — the check sees no path, so it says
        // nothing rather than guessing.
        let named_test = "# S\n\n## Acceptance Criteria\n\n\
                          - **AC-1** — the case passes. Command: `cargo test -p mustard-rt --lib m::tests::c`\n";
        let quiet = traceability_gaps(
            &claim_plan(vec![claim_wave(
                1,
                "rt",
                vec!["a"],
                vec!["apps/rt/src/unrelated.rs"],
                vec!["AC-1"],
            )]),
            Some(named_test),
        );
        assert!(
            quiet.criteria_outside_claimants.is_empty(),
            "a command naming no path must not be judged: {:?}",
            quiet.criteria_outside_claimants
        );
        assert!(quiet.unsupportable_claims.is_empty(), "and it supports its claim");
    }

    /// Traceability: a wave that does work (`tasks`) but satisfies no AC is a
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
            reality_obligations: Vec::new(),
        };
        let plan = |w: WavePlanEntry| Plan { waves: vec![w], total_waves: Some(1) };

        // (a) tasks but no AC, num plano que TEM critérios → untraced-wave gap
        // naming the wave (Gap 1).
        let parent_ac = "# S\n\n## Acceptance Criteria\n\n- **AC-1** — works. Command: `true`\n";
        let gaps = traceability_gaps(
            &plan(wave(vec!["do the thing"], vec![], vec![])),
            Some(parent_ac),
        );
        assert!(
            gaps.untraced_waves
                .iter()
                .any(|g| g.contains("wave-1-backend") && g.contains("traces to no criterion")),
            "wave with tasks but no AC must be a gap: {:?}",
            gaps.untraced_waves
        );
        assert!(
            gaps.uncovered_acs.iter().any(|g| g.contains("AC-1")),
            "e o critério que nenhuma onda reivindica é órfão: {:?}",
            gaps.uncovered_acs,
        );
        // (a') …e a MESMA onda com um `satisfies`, que é o que um plano real
        // carrega: ela sai da lista.
        let mut traced = wave(vec!["do the thing"], vec![], vec!["AC-1"]);
        traced.files = vec!["src/lib.rs".to_string()];
        let drafting = traceability_gaps(&plan(traced), Some(parent_ac));
        assert!(
            drafting.untraced_waves.is_empty(),
            "uma onda que traça ao critério do pai não é gap: {:?}",
            drafting.untraced_waves,
        );
        assert!(drafting.uncovered_acs.is_empty(), "e AC-1 está reivindicado");
        // (b) declares AND satisfies its own AC → clean on both axes. It also
        // declares a file: a wave that does work and claims a criterion while
        // declaring nowhere to do it is Gap 3, so the fixture has to be a
        // genuinely consistent plan to prove the clean case.
        let mut supported = wave(
            vec!["do it"],
            vec!["**AC-1** — works. Command: `true`"],
            vec!["AC-1"],
        );
        supported.files = vec!["src/lib.rs".to_string()];
        let clean = traceability_gaps(&plan(supported), None);
        assert!(clean.untraced_waves.is_empty() && clean.uncovered_acs.is_empty(), "well-traced wave is clean");
        // (c) defines AC-1 but satisfies only AC-2 → AC-1 is an uncovered gap.
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
                reality_obligations: Vec::new(),
            }],
            total_waves: Some(1),
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
                reality_obligations: Vec::new(),
            }],
            total_waves: Some(1),
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

    /// End-to-end through `scaffold`: the SUFFICIENCY gap reaches the outcome.
    ///
    /// It used to be computed, printed to stderr, and then dropped when the
    /// outcome was built — so `plan-materialize`, the only consumer, could not
    /// see it at all and the PLAN transition went through on a plan whose
    /// claiming wave could not reach a path its criterion inspects.
    ///
    /// Two-sided: the same plan with the path declared returns the list empty,
    /// so this cannot pass by the gap firing on everything.
    #[test]
    fn wave_claiming_a_criterion_must_contain_its_paths() {
        let build = |declared: Value| {
            let dir = tempdir().unwrap();
            let spec_dir = dir.path().join("epic-sufficiency");
            std::fs::create_dir_all(&spec_dir).unwrap();
            std::fs::write(
                spec_dir.join("spec.md"),
                "# Epic\n\n## Acceptance Criteria\n\
                 - **AC-1** — a. Command: `rg -q x apps/backend/src/data.rs`\n\
                 - **AC-2** — build green. Command: `true`\n",
            )
            .unwrap();
            let plan_path = dir.path().join("plan.json");
            std::fs::write(
                &plan_path,
                serde_json::to_string(&json!({
                    "waves": [{
                        "n": 1, "role": "backend", "summary": "s", "tasks": ["do it"],
                        "files": declared, "satisfies": ["AC-1", "AC-2"],
                    }],
                    "total_waves": 1,
                }))
                .unwrap(),
            )
            .unwrap();
            match scaffold(&spec_dir, &plan_path) {
                ScaffoldOutcome::Created { criteria_outside_claimants, .. } => {
                    // `dir` must outlive the scaffold call, hence the clone-out.
                    criteria_outside_claimants
                }
                _ => panic!("expected ScaffoldOutcome::Created"),
            }
        };

        let flagged = build(json!(["apps/backend/src/other.rs"]));
        assert!(
            flagged.iter().any(|g| g.contains("AC-1") && g.contains("apps/backend/src/data.rs")),
            "the gap must reach the outcome, naming the criterion and the path: {flagged:?}",
        );

        let clean = build(json!(["apps/backend/src/data.rs"]));
        assert!(
            clean.is_empty(),
            "a claimant declaring the path leaves nothing to report: {clean:?}",
        );
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

    /// A spec the user has NOT approved is still the
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

    /// Once the user approved the spec the layout is FROZEN: a plan that
    /// renders something else leaves every file byte-identical, reports nothing
    /// refreshed or removed, and raises the single stderr WARN that names the
    /// change-request route.
    #[test]
    fn approved_plan_scaffold_is_frozen() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("epic-frozen");
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
        crate::shared::spec_state::approve_in(&spec_dir);
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
        assert!(warn.contains("the spec is approved"), "the WARN must name the approval: {warn}");
    }

    /// The half the freeze first missed: an APPROVED spec must not grow
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
        let spec_dir = dir.path().join(".claude").join("spec").join("epic-grow");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let plan_path = write_plan(
            dir.path(),
            json!([{ "n": 1, "role": "rt", "summary": "one", "tasks": ["t"] }]),
        );
        let _ = scaffold(&spec_dir, &plan_path);
        assert_eq!(read_meta(&spec_dir.join("meta.json")).unwrap().total_waves, Some(1));

        crate::shared::spec_state::approve_in(&spec_dir);
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

    /// Before approval, a wave dropped from `plan.json` has its
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

    /// A plan that cannot be read OR cannot be parsed answers with a
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
