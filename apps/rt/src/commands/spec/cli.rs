//! The `run` subcommands for the spec lifecycle (`spec/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`SpecCmd`] AND its arm in [`dispatch`] below; forgetting the arm still
//! compiles, but the command vanishes from the CLI. The other two live in
//! the tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run spec <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{spec};

/// The `run` subcommands owned by the spec lifecycle (`spec/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum SpecCmd {
    /// Finalize a pipeline spec — single-stage close straight to `completed`.
    #[command(display_order = 9)]
    CompleteSpec {
        /// Spec name (required unless `--archive-stale`/`--archive-followups`).
        spec: Option<String>,
        /// Idempotent alias of the single complete: re-emit `completed` + meta
        /// sync and drop any legacy state file. No filesystem move.
        #[arg(long)]
        archive: bool,
        /// No-op (retained for compatibility): the single-stage close no longer
        /// produces `closed-followup` specs, so there is nothing to sweep.
        #[arg(long = "archive-stale")]
        archive_stale: bool,
        /// No-op (retained for compatibility): see `--archive-stale`.
        #[arg(long = "archive-followups")]
        archive_followups: bool,
    },
    /// Suggest wave decomposition by file/entity count.
    ///
    /// With `--from-spec <path>`, computes `fileCount` / `layerCount` /
    /// `newEntityCount` deterministically in Rust from the spec's `## Files`
    /// section + a diff against the repo model's entity names (no LLM). Without
    /// it, reads a pre-computed signals JSON from stdin (legacy / override).
    #[command(display_order = 16)]
    ScopeDecompose {
        /// Compute the signals deterministically from this spec file instead of
        /// reading them from stdin.
        #[arg(long = "from-spec", alias = "spec")]
        from_spec: Option<String>,
    },
    /// Classify a spec's scope (light / extended-light / full) deterministically.
    ///
    /// Reuses the same structural signals as `scope-decompose --from-spec`
    /// (fileCount / layerCount / newEntityCount), plus `--slice-match-count`
    /// from the `feature` digest's `sliceMatchCount`, and encodes the `/feature`
    /// SKILL's prose thresholds in code. Fail-open: an unreadable spec yields
    /// `{"scope":"full",...}` (the conservative default).
    #[command(display_order = 17)]
    ScopeClassify {
        /// Compute the signals deterministically from this spec file.
        #[arg(long = "from-spec", alias = "spec")]
        from_spec: String,
        /// Count of matched recurring slices from the `feature` digest's
        /// `sliceMatchCount` — vocabulary-overlap precedent: >=2 counts toward
        /// full only alongside layer spread (layerCount >= 2); alone it is
        /// precedent evidence for the extended-light band. Defaults to 0.
        #[arg(long = "slice-match-count", default_value_t = 0)]
        slice_match_count: i64,
    },
    /// Fused pre-PLAN decision: `scope-classify` + `scope-decompose` from ONE
    /// signal computation (one spec read, one `scan facts` spawn, one turn).
    /// Returns `{scope, decompose, reason, waves, signals, filesSectionEmpty?}`
    /// — the union the `/feature` PLAN step needs to route, pick 1-vs-N, and
    /// seed `spec-draft --waves`. Replaces calling the two commands in sequence.
    #[command(display_order = 18)]
    PlanPrepare {
        /// Compute the signals deterministically from this spec file.
        #[arg(long = "from-spec", alias = "spec")]
        from_spec: String,
        /// `sliceMatchCount` from the `feature` digest (same meaning as
        /// `scope-classify`). Defaults to 0.
        #[arg(long = "slice-match-count", default_value_t = 0)]
        slice_match_count: i64,
    },
    /// Rematerialise the denormalised `specs` + `metrics_projection` tables
    /// from the event stream. Closes the gap the move off Bun
    /// opened: pre-2026-05-20 nothing populated those tables since the JS
    /// harness writer was removed, which is why every dashboard spec card
    /// fell back to `"unknown"`.
    #[command(display_order = 24)]
    RebuildSpecs,
    /// Discover active specs from the filesystem (Outcome=Active, Stage=Plan|Execute).
    ///
    /// Replaces the LLM-side glob/grep loop in `/mustard:spec`: reads
    /// `.claude/spec/*/spec.md` directly, filters headers, counts wave
    /// progress, extracts a one-line resumo.
    /// Output is either a markdown table (default) or a JSON document.
    #[command(display_order = 40)]
    ActiveSpecs {
        /// Output format: `table` (default) or `json`.
        #[arg(long, default_value = "table")]
        format: String,
        /// Project root directory (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Draft a new spec layout (`spec.md` + `meta.json`) conforming to
    /// `mustard_core::domain::spec::contract`. Replaces the literal ~80-line
    /// template block inside the `/mustard:feature` SKILL.md.
    ///
    /// `spec-draft` materialises ONLY the top-level `spec.md` + `meta.json`
    /// (recording `scope`/`totalWaves`/`isWavePlan`); full-scope wave dirs are
    /// materialised by `wave-scaffold`. The narrative is written in the
    /// project's text language (`mustard.json` `language.text`). `--signals` is
    /// a free-form comma-separated list embedded in `spec.md` as a comment.
    #[command(display_order = 47)]
    SpecDraft {
        /// Free-text intent — the spec TITLE, and the last-resort slug seed.
        #[arg(long)]
        intent: String,
        /// The unit's canonical name, as minted by the base gate
        /// (`emit-pipeline --kind pipeline.kind` reports it as `spec`). Used
        /// VERBATIM — the draft consumes the name the unit already carries
        /// instead of deriving a second one from `--intent`. Omitted: the slug
        /// half of the unit's work branch — the one this call cuts, else the
        /// one already checked out — and only then the intent.
        #[arg(long)]
        slug: Option<String>,
        /// `light` (single-shot) or `full` (wave plan).
        #[arg(long, default_value = "full")]
        scope: String,
        /// Optional comma-separated signal list (`layers,files,registry`).
        #[arg(long)]
        signals: Option<String>,
        /// Output directory (default `.claude/spec/{slug}/`).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Path to the conversation-material JSON: `{ "definitions": [{term,
        /// meaning}], "decisions": [{decision, reason}], "findings":
        /// [{statement, file, line?}] }`. Each kind lands in a section of its
        /// own. A FILE, not a flag value — the payload carries newlines,
        /// quotes and non-ASCII that a shell argument would mangle. Omitted
        /// (or carrying nothing): the draft is byte-identical to today's.
        #[arg(long)]
        material: Option<PathBuf>,
        /// Refresh ONLY the `## Definitions` / `## Decisions` / `## Evidence`
        /// sections of a spec that already exists; every other byte of
        /// `spec.md` is left alone. Needs `--slug` and `--material`, and never
        /// creates a spec.
        ///
        /// This is the frequent move: one decision settled, one
        /// `material-add`, and the spec has to carry it. The alternative was a
        /// full `--force` re-draft of the whole body for each one.
        #[arg(long = "material-only")]
        material_only: bool,
        /// Why this draft carries no conversation material. REQUIRED when
        /// `--material` is absent or carries nothing: an empty channel has to
        /// be a stated choice, not an omission that looks like success. One
        /// sentence; it is recorded in the report as `noMaterialReason`.
        #[arg(long)]
        no_material_reason: Option<String>,
        /// Waves recorded in `meta.json#totalWaves` under Full scope (default 1).
        /// Without `--plan` the wave dirs themselves are materialised later, by
        /// `plan-materialize`.
        #[arg(long, default_value_t = 1)]
        waves: u32,
        /// Path to the plan JSON. FUSES the draft with the PLAN-phase
        /// materialisation: this one call also runs the wave-scaffold renderer,
        /// analyze-validation, the dependency DAG and the NEGATIVE PROOF, so
        /// `wave-plan.md` and every wave dir land in the same pass. The plan's
        /// `acceptance` lines become the spec's acceptance criteria. A refusal
        /// exits 2 and leaves no layout behind. Omitted: unchanged behaviour —
        /// `plan-materialize` remains the re-materialisation door.
        #[arg(long)]
        plan: Option<PathBuf>,
        /// Overwrite an existing output directory.
        #[arg(long)]
        force: bool,
        /// Comma-separated repo-vocabulary terms for the internal Context
        /// enrichment query — pass the terms that produced a strong digest
        /// report during ANALYZE. Omitted: the raw intent is tokenised (a
        /// translated intent then repeats the weak query and the enrichment
        /// withholds itself).
        #[arg(long = "query-terms")]
        query_terms: Option<String>,
        /// Honour the requested `--scope full` even when the deterministic
        /// routing gate would auto-rebaixar it to light/extended-light. The
        /// override is recorded (a `pipeline.scope.override` event) so it is
        /// auditable, never silent.
        #[arg(long = "force-scope")]
        force_scope: bool,
    },
    /// Compile the deterministic spec draft for one entity via `grain spec` and
    /// print the resulting Markdown verbatim to stdout. Thin passthrough to
    /// `mustard_core::domain::scan::Scan::spec`. Invoke as
    /// `mustard-rt run scan spec --entity <Name>`.
    #[command(name = "scan-spec")]
    #[command(display_order = 48)]
    ScanSpec {
        /// Entity/unit to create (substitutes `<Name>` in the grain recipe).
        #[arg(long)]
        entity: String,
        /// Existing sibling to mirror; omit for auto-pick.
        #[arg(long)]
        like: Option<String>,
        /// Extra operations beyond the base vertical (comma-separated, e.g. `approve,cancel`).
        #[arg(long, value_delimiter = ',')]
        ops: Vec<String>,
        /// Cross-cutting invariants the unit must obey (repeatable).
        #[arg(long)]
        invariant: Vec<String>,
        /// Workspace root (must contain `.claude/grain.model.json`).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Refuses at the door with exit 1 and writes nothing: a spec is approved
    /// by the user's answer to the approval question, which the witness
    /// records.
    #[command(name = "approve-spec")]
    #[command(display_order = 55)]
    ApproveSpec {
        /// Spec slug under `.claude/spec/` to approve.
        #[arg(long)]
        spec: String,
        /// The spec is a wave plan — patch the wave-1 `meta.json` for dispatch.
        #[arg(long = "wave-plan")]
        wave_plan: bool,
        /// Inline-resume: also emit `pipeline.stage Execute` (the `r`-suffix
        /// branch). Without it, the flow stops at `approved` for a fresh session.
        #[arg(long)]
        resume: bool,
    },
    /// Declare the DESTINATION of one collected finding, and why it went there.
    ///
    /// The seeding half (`finding-collect`) decides nothing: it reads the
    /// reviewer's `review/findings*.md` and records what was found. This is the
    /// other half — the
    /// only writer of a finding's destination, mirroring `mark-checklist-item
    /// --drop --reason`. A destination with no stated reason is REFUSED (exit
    /// 2): it would leave the finding in exactly the silence it started in, and
    /// the close gate would keep reading it as open.
    ///
    /// Terminal: a finding already carrying a destination answers
    /// `already-routed` when the same decision is restated, and refuses a
    /// different one rather than overwriting a decision in silence.
    #[command(name = "mark-finding")]
    #[command(display_order = 69)]
    MarkFinding {
        /// Spec slug under `.claude/spec/`, or a path to the spec markdown or
        /// its directory.
        #[arg(long, alias = "from-spec")]
        spec: Option<String>,
        /// The finding's id, as `finding-collect` reported it (`F-findings…`
        /// for a reviewer file, the criterion id for a ledger column).
        #[arg(long)]
        id: Option<String>,
        /// Where the finding went: `criterion` | `change-request` | `queued` |
        /// `dropped`.
        #[arg(long)]
        to: Option<String>,
        /// Why it went there. Mandatory — a blank reason is refused.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Monta o resumo legível da spec em `.claude/spec/<slug>/resumo.html`, no
    /// layout padrão do Mustard: resumo da conversa, onde estamos, o que foi
    /// esclarecido, decisões, riscos, a spec, critérios com o estado da prova,
    /// ondas com as skills prescritas, evidências, pendências abertas e o
    /// próximo passo.
    ///
    /// Vem ANTES de qualquer pergunta de aprovação: o usuário recusou aprovar
    /// uma spec que só conseguia ler no terminal. Devolve `{ok, path, url,
    /// hash, changed, publishedUrl}` — `url` é o `file://` que o usuário clica,
    /// `changed` diz se a página mudou desde a última geração (só então ela é
    /// regravada) e `publishedUrl` é o endereço publicado gravado, ou `null`.
    #[command(name = "spec-doc")]
    #[command(display_order = 75)]
    SpecDoc {
        /// Slug da spec em `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// O endereço em que a página foi publicada no claude.ai. Fica gravado
        /// em `.claude/spec/<slug>/published-url` antes de a página ser
        /// montada; a retomada e o gancho de fim de resposta o leem de lá.
        /// Recusado quando não é um link `http(s)://`.
        #[arg(long = "published-url")]
        published_url: Option<String>,
    },
    /// Gera uma página no layout do Mustard, pelo motor de página, com as
    /// fontes do Google Fonts.
    ///
    /// Com `--body` e `--out`, gera uma página avulsa (análise, relatório,
    /// plano) a partir de um arquivo markdown: escreve-se markdown, nunca
    /// HTML. Sem `--title`, o título é a primeira linha `# Título`. Com
    /// `--spec`, refaz o `spec.md` e o `spec.html` da spec a partir do
    /// `spec.ndjson`. Devolve `{ok, path}` ou `{ok, spec, md, html}`.
    #[command(name = "page")]
    #[command(display_order = 77)]
    Page {
        /// A spec cuja página e cujo `.md` são refeitos.
        #[arg(long, conflicts_with_all = ["body", "out", "title", "subtitle", "kind"])]
        spec: Option<String>,
        /// O arquivo markdown da página avulsa.
        #[arg(long)]
        body: Option<PathBuf>,
        /// Onde gravar a página avulsa; pastas ausentes são criadas.
        #[arg(long)]
        out: Option<PathBuf>,
        /// O título, no `<title>` e no `<h1>`; sem ele, a primeira linha
        /// `# Título` do markdown.
        #[arg(long)]
        title: Option<String>,
        /// Uma linha solta sob o título, na faixa do cabeçalho.
        #[arg(long)]
        subtitle: Option<String>,
        /// O que vem depois de `Mustard · ` na faixa do cabeçalho.
        #[arg(long)]
        kind: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `spec`-family `run` subcommand.
pub fn dispatch(cmd: SpecCmd) {
    match cmd {
        SpecCmd::CompleteSpec {
            spec,
            archive,
            archive_stale,
            archive_followups,
        } => spec::complete_spec::run(spec.as_deref(), archive, archive_stale, archive_followups),
        SpecCmd::ScopeDecompose { from_spec } => spec::scope_decompose::run(from_spec.as_deref()),
        SpecCmd::ScopeClassify {
            from_spec,
            slice_match_count,
        } => spec::scope_decompose::run_classify(&from_spec, slice_match_count),
        SpecCmd::PlanPrepare {
            from_spec,
            slice_match_count,
        } => spec::scope_decompose::run_prepare(&from_spec, slice_match_count),
        SpecCmd::RebuildSpecs => spec::rebuild_specs::run(),
        SpecCmd::ActiveSpecs { format, root } => {
            spec::active_specs::run(spec::active_specs::ActiveSpecsOpts { format, root });
        }
        SpecCmd::SpecDraft {
            intent,
            slug,
            scope,
            signals,
            output,
            material,
            material_only,
            no_material_reason,
            waves,
            plan,
            force,
            query_terms,
            force_scope,
        } => {
            spec::spec_draft::run(spec::spec_draft::SpecDraftOpts {
                intent,
                slug,
                scope,
                signals,
                output,
                material,
                material_only,
                no_material_reason,
                waves,
                plan,
                force,
                query_terms,
                force_scope,
            });
        }
        SpecCmd::ScanSpec { entity, like, ops, invariant, root } => {
            spec::scan_spec::run(spec::scan_spec::ScanSpecOpts {
                entity,
                like,
                ops,
                invariants: invariant,
                root,
            });
        }
        SpecCmd::ApproveSpec { spec, wave_plan, resume } => {
            spec::approve_spec::run(spec::approve_spec::ApproveSpecOpts {
                spec,
                wave_plan,
                resume,
            });
        }
        SpecCmd::MarkFinding { spec: slug, id, to, reason } => {
            spec::mark_finding::run(
                slug.as_deref(),
                id.as_deref(),
                to.as_deref(),
                reason.as_deref(),
            );
        }
        SpecCmd::SpecDoc { spec: slug, published_url } => {
            spec::spec_doc::run(&spec::spec_doc::SpecDocOpts { spec: slug, published_url });
        }
        SpecCmd::Page { spec: slug, body, out, title, subtitle, kind, root } => {
            spec::page::run(&spec::page::PageOpts {
                root,
                spec: slug,
                body,
                out,
                title,
                subtitle,
                kind,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SpecCmd;
    use clap::Parser;

    /// A wrapper so the family enum can be parsed on its own — the binary's own
    /// `Cli` lives in `main.rs` and is out of reach from the lib.
    #[derive(Parser)]
    struct Probe {
        #[command(subcommand)]
        cmd: SpecCmd,
    }

}
