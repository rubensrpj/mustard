//! `i18n` — central language module for Mustard banners.
//!
//! ## Why
//!
//! Before this module, hardcoded pt-BR strings lived in
//! `apps/rt/src/hooks/*.rs` (e.g. `amend_capture.rs`) and in
//! `apps/cli/src/commands/*.rs`. Bilingual lookup tables were copy-pasted
//! across three or more files. There was no single place to:
//!
//! - declare the canonical locale codes (BCP-47, never short forms);
//! - translate a banner key into the user's language;
//! - slugify free-form text in a way that respects PT-vs-EN accent rules.
//!
//! This module is now that single place, a boundary-typed module exported
//! from `mustard_core`.
//!
//! ## Locale vocabulary
//!
//! - [`Locale`] — BCP-47 typed locale. Only `pt-BR` and `en-US` are accepted;
//!   the legacy short forms `pt` / `en` are rejected with
//!   [`LocaleError::ShortForm`] (see memory `project_locale_codes`).
//! - [`I18n`] — the locale callers thread through banner rendering.
//!
//! The catalogue is written in one voice only, plain and didactic: there is
//! no tone to choose, so nothing here rewrites a translation after lookup.
//!
//! ## Canonical banner keys
//!
//! Banners are keyed by dotted-namespace identifiers. Every key is documented
//! in [`translate`] and surfaced verbatim when the key is unknown (fail-open).
//! The first keys:
//!
//! - `banner.close.success` — "Pipeline closed successfully." (CLOSE phase)
//! - `banner.amend.drift` — drift-warning message body.
//! - `wave.label` — short label for a wave index (`W{n}` / `Onda {n}`).
//! - `ac.label` — short label for an AC index (`AC-{id}`).
//! - `prompt.continue` — "Continue?" / "Continuar?" confirmation prompt.
//!
//! ## Forward compatibility
//!
//! New keys land here, not in consumer crates. A missing key returns the key
//! string itself so the caller still emits *something*; this is the fail-open
//! contract that keeps a typo in a hook from blocking user work.

use std::fmt;
use std::str::FromStr;

/// BCP-47 locale code used by the spec/header cascade.
///
/// Only `pt-BR` and `en-US` are valid Mustard locales. Short forms (`pt`,
/// `en`) are rejected by [`Locale::from_str`] with [`LocaleError::ShortForm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    /// Brazilian Portuguese, BCP-47 `pt-BR`.
    PtBr,
    /// United States English, BCP-47 `en-US`.
    EnUs,
}

impl Locale {
    /// Canonical BCP-47 code for this locale.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PtBr => "pt-BR",
            Self::EnUs => "en-US",
        }
    }
}

impl Default for Locale {
    /// pt-BR is the default for Mustard banners (the project's primary user
    /// locale per `project_locale_codes`).
    fn default() -> Self {
        Self::PtBr
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Locale parse errors.
///
/// `ShortForm` is intentionally distinct from `Unknown` so callers can warn the
/// user that their config still uses the legacy `pt`/`en` short codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocaleError {
    /// The input is the legacy short form (`pt` / `en`). Reject and ask the
    /// caller to upgrade to BCP-47.
    ShortForm(String),
    /// The input is not a recognised Mustard locale.
    Unknown(String),
}

impl fmt::Display for LocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShortForm(s) => write!(
                f,
                "locale {s:?} is a legacy short form; use a BCP-47 code (pt-BR / en-US)"
            ),
            Self::Unknown(s) => write!(f, "unknown locale {s:?}; expected pt-BR or en-US"),
        }
    }
}

impl std::error::Error for LocaleError {}

impl FromStr for Locale {
    type Err = LocaleError;

    /// Parse a BCP-47 code. Trimming + case-insensitive on the region part.
    /// Short forms (`pt` / `en`) are explicitly rejected — callers should
    /// surface the error and ask the user to update their config to the
    /// canonical BCP-47 spelling.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        // Reject short forms up-front with a typed error.
        let lc = trimmed.to_ascii_lowercase();
        if lc == "pt" || lc == "en" {
            return Err(LocaleError::ShortForm(trimmed.to_string()));
        }
        // BCP-47: `xx-YY` — language lowercase, region uppercase. Accept
        // mixed-case input by normalising.
        match lc.as_str() {
            "pt-br" => Ok(Self::PtBr),
            "en-us" => Ok(Self::EnUs),
            _ => Err(LocaleError::Unknown(trimmed.to_string())),
        }
    }
}

/// Banner-rendering context: the locale.
///
/// Threaded through hook / CLI banner code so a single struct call replaces
/// the bilingual lookup tables that used to sit in each module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct I18n {
    /// User locale (drives [`translate`]).
    pub lang: Locale,
}

impl I18n {
    /// Build an `I18n` for `lang`.
    #[must_use]
    pub fn new(lang: Locale) -> Self {
        Self { lang }
    }

    /// Translate `key` into this locale.
    #[must_use]
    pub fn render(&self, key: &str) -> String {
        translate(key, self.lang).to_string()
    }
}

/// Translate `key` into a literal banner string for `lang`.
///
/// A missing key returns the key itself (fail-open: the caller still emits
/// *something*). Known keys are documented at the module header. Adding a new
/// banner = adding one arm to the `match` and one arm per locale.
///
/// Lifetime: returns `&'static str` because every entry is a string literal —
/// no allocation in the hot banner path.
#[must_use]
pub fn translate(key: &str, lang: Locale) -> &'static str {
    match (key, lang) {
        // CLOSE-phase success banner.
        ("banner.close.success", Locale::PtBr) => "Pipeline fechado com sucesso.",
        ("banner.close.success", Locale::EnUs) => "Pipeline closed successfully.",

        // Drift warning emitted by `apps/rt/src/hooks/amend_capture.rs`.
        ("banner.amend.drift", Locale::PtBr) => {
            "Você está editando um arquivo fora do escopo da spec ativa (pós-CLOSE). \
             Considere abrir `/mustard:feature` ou `/mustard:task` separado — a sessão \
             continua, mas o drift não é absorvido pela spec original."
        }
        ("banner.amend.drift", Locale::EnUs) => {
            "You're editing a file outside the active spec scope (post-CLOSE). \
             Consider opening a separate `/mustard:feature` or `/mustard:task` — the \
             session continues, but drift is not absorbed by the original spec."
        }

        // Short wave label (e.g. "W3" vs "Onda 3"). The numeric suffix is
        // interpolated by the caller via `format!("{} {n}", translate(...))`.
        ("wave.label", Locale::PtBr) => "Onda",
        ("wave.label", Locale::EnUs) => "W",

        // Acceptance-criterion label (used as a prefix before the AC id).
        ("ac.label", Locale::PtBr) => "CA",
        ("ac.label", Locale::EnUs) => "AC",

        // Generic continue / confirm prompt.
        ("prompt.continue", Locale::PtBr) => "Continuar?",
        ("prompt.continue", Locale::EnUs) => "Continue?",

        // Spec narrative headings — canonical translation table mirrors
        // `refs/feature/spec-language.md § Header Translation Table` and the
        // section-key map in `apps/rt/src/run/spec_sections.rs::variants`.
        ("heading.spec.context", Locale::PtBr) => "Contexto",
        ("heading.spec.context", Locale::EnUs) => "Context",
        ("heading.spec.users", Locale::PtBr) => "Usuários/Stakeholders",
        ("heading.spec.users", Locale::EnUs) => "Users/Stakeholders",
        ("heading.spec.metric", Locale::PtBr) => "Métrica de sucesso",
        ("heading.spec.metric", Locale::EnUs) => "Success Metric",
        ("heading.spec.non_goals", Locale::PtBr) => "Não-Objetivos",
        ("heading.spec.non_goals", Locale::EnUs) => "Non-Goals",
        // Single AC heading key. The legacy `heading.spec.ac_list` twin (byte-
        // identical strings) was collapsed into this one (TF 2026-06-10-ac-
        // heading-unico): two keys for the same heading let the scaffold emit
        // the AC section twice, shadowing the real list for every
        // `section_block` reader.
        ("heading.spec.ac", Locale::PtBr) => "Critérios de Aceitação",
        ("heading.spec.ac", Locale::EnUs) => "Acceptance Criteria",
        ("heading.spec.tasks", Locale::PtBr) => "Tarefas",
        ("heading.spec.tasks", Locale::EnUs) => "Tasks",
        ("heading.spec.files", Locale::PtBr) => "Arquivos",
        ("heading.spec.files", Locale::EnUs) => "Files",
        ("heading.spec.limits", Locale::PtBr) => "Limites",
        ("heading.spec.limits", Locale::EnUs) => "Boundaries",
        ("heading.spec.summary", Locale::PtBr) => "Resumo",
        ("heading.spec.summary", Locale::EnUs) => "Summary",

        // Memory-note section headings (rendered by `spec_memory::render_template`).
        ("heading.memory.origin", Locale::PtBr) => "Origem",
        ("heading.memory.origin", Locale::EnUs) => "Origin",
        ("heading.memory.applies_to", Locale::PtBr) => "Aplica-se a",
        ("heading.memory.applies_to", Locale::EnUs) => "Applies to",
        ("heading.memory.status", Locale::PtBr) => "Status",
        ("heading.memory.status", Locale::EnUs) => "Status",
        ("heading.memory.related", Locale::PtBr) => "Relacionado",
        ("heading.memory.related", Locale::EnUs) => "Related",
        ("heading.memory.principles", Locale::PtBr) => "Princípios",
        ("heading.memory.principles", Locale::EnUs) => "Principles",

        // Memory-note intro lines + index columns. The `{spec}` slot is
        // already wikilink-wrapped here because the rendered note lives
        // next to its parent spec; the `{wave}` slot is wrapped by the
        // caller (it has to resolve "unknown wave" first).
        ("memory.intro.born_during", Locale::PtBr) => "Nasceu durante [[{spec}]] na onda {wave}.",
        ("memory.intro.born_during", Locale::EnUs) => "Born during [[{spec}]] in wave {wave}.",
        ("memory.origin.wave_unknown", Locale::PtBr) => "wave desconhecida",
        ("memory.origin.wave_unknown", Locale::EnUs) => "wave unknown",
        ("memory.status.active", Locale::PtBr) => "Ativa.",
        ("memory.status.active", Locale::EnUs) => "Active.",
        ("memory.index.title", Locale::PtBr) => "Memória da spec {title}",
        ("memory.index.title", Locale::EnUs) => "Spec memory — {title}",
        ("memory.index.intro", Locale::PtBr) => "Conhecimento capturado durante esta spec.",
        ("memory.index.intro", Locale::EnUs) => "Knowledge captured during this spec.",
        ("memory.index.empty", Locale::PtBr) => "Nenhum conhecimento capturado ainda.",
        ("memory.index.empty", Locale::EnUs) => "No knowledge captured yet.",
        ("memory.index.column.file", Locale::PtBr) => "Arquivo",
        ("memory.index.column.file", Locale::EnUs) => "File",
        ("memory.index.column.wave", Locale::PtBr) => "Onda",
        ("memory.index.column.wave", Locale::EnUs) => "Wave",

        // Orientation artifacts — the once-per-session terrain banner
        // (`commands/orient.rs`) and the machine-owned `.claude/scan-map.md`
        // (`commands/scan_claude.rs::render_map`). Both are DISPLAYED to the
        // developer and injected into the session, so they follow the
        // project's text language (`language.text` in `mustard.json`) — unlike
        // the internal census/index/search, which stays English by policy. The
        // `{kind}` / `{count}` slots are interpolated by the caller.
        ("orient.terrain.header", Locale::PtBr) => {
            "[Terreno] subprojetos mapeados pelo /scan — leia daqui, não grepe para se orientar:"
        }
        ("orient.terrain.header", Locale::EnUs) => {
            "[Terrain] subprojects mapped by /scan — read from here, don't grep to orient yourself:"
        }
        ("orient.census.files_suffix", Locale::PtBr) => " · {count} arquivos",
        ("orient.census.files_suffix", Locale::EnUs) => " · {count} files",
        ("orient.census.truncated", Locale::PtBr) => {
            "\n- (+{count} subprojetos não listados — o censo completo está em `.claude/grain.model.json`)"
        }
        ("orient.census.truncated", Locale::EnUs) => {
            "\n- (+{count} subprojects not listed — the full census is in `.claude/grain.model.json`)"
        }
        ("scan.map.type_line", Locale::PtBr) => "Tipo: {kind} · {count} arquivos",
        ("scan.map.type_line", Locale::EnUs) => "Type: {kind} · {count} files",
        ("scan.map.pointer", Locale::PtBr) => {
            "O terreno já está na sua janela (o census de orientação injetado no início da sessão). Para localizar: `grep` para termo exato conhecido; `mustard-rt run feature` (digest) para conceito; depois leia os arquivos apontados — o digest acha onde olhar, não substitui ler."
        }
        ("scan.map.pointer", Locale::EnUs) => {
            "The terrain is already in your window (the orientation census injected at session start). To locate: `grep` for a known exact term; `mustard-rt run feature` (digest) for a concept; then read the files it points to — the digest finds where to look, it does not replace reading."
        }

        // Spec-draft + section-body placeholders. EN strings use the
        // canonical "fill in <X>." shape so a single `body.contains("fill
        // in")` assertion can distinguish EN bodies from the PT catalogue
        // ("Preencher …"). PT mirrors the imperative form.
        ("placeholder.fill", Locale::PtBr) => "Preencher.",
        ("placeholder.fill", Locale::EnUs) => "fill in.",
        ("placeholder.fill_first_line", Locale::PtBr) => "Resuma o princípio em uma linha.",
        ("placeholder.fill_first_line", Locale::EnUs) => "fill in the principle in one line.",
        ("placeholder.fill_who_files", Locale::PtBr) => "Quem / quais arquivos.",
        ("placeholder.fill_who_files", Locale::EnUs) => "fill in who / which files.",
        ("placeholder.fill_wirelinks", Locale::PtBr) => "Wikilinks relacionados.",
        ("placeholder.fill_wirelinks", Locale::EnUs) => "fill in related wikilinks.",
        ("placeholder.fill_why_now", Locale::PtBr) => "Por que agora.",
        ("placeholder.fill_why_now", Locale::EnUs) => "fill in why now.",
        ("placeholder.fill_beneficiary", Locale::PtBr) => "Quem se beneficia.",
        ("placeholder.fill_beneficiary", Locale::EnUs) => "fill in who benefits.",
        ("placeholder.fill_metric", Locale::PtBr) => "Métrica de sucesso.",
        ("placeholder.fill_metric", Locale::EnUs) => "fill in the success metric.",
        ("placeholder.fill_excluded", Locale::PtBr) => "O que fica de fora.",
        ("placeholder.fill_excluded", Locale::EnUs) => "fill in what stays out.",
        // `placeholder.see_below` was retired with the single-AC-heading fix:
        // the AC PRD entry is no longer
        // rendered (the list block is the only emitter), so its body needs no
        // user-facing copy.
        ("placeholder.fill_files", Locale::PtBr) => "Listar arquivos afetados.",
        ("placeholder.fill_files", Locale::EnUs) => "fill in affected files.",

        // Trackable `## Checklist` item label (`spec_draft::build_checklist`).
        // `first_task` is the single hand-trackable task the draft seeds; the
        // draft no longer materialises per-anchor `touch_file` items (a digest
        // anchor is a READ candidate, never an implementation target — seeding
        // write-tracking from it baked lexical noise into the artifact).
        ("checklist.first_task", Locale::PtBr) => "T1 — primeira tarefa rastreável.",
        ("checklist.first_task", Locale::EnUs) => "T1 — first trackable task.",

        // EARS acceptance-criteria SKELETONS seeded by `spec_draft::build_input`.
        // The `<…>` angle-bracket markers are deliberate placeholders the
        // orchestrator MUST replace with the concrete behaviour — a draft is born
        // demanding specificity, never a lone `cargo build` rubber stamp. The
        // `when`/`then` glue is added by `capability::scenario_statement`.
        ("ac.skeleton.when_primary", Locale::PtBr) => "<o novo comportamento é acionado>",
        ("ac.skeleton.when_primary", Locale::EnUs) => "<the new behaviour is invoked>",
        ("ac.skeleton.then_primary", Locale::PtBr) => "<o resultado observável esperado se mantém>",
        ("ac.skeleton.then_primary", Locale::EnUs) => "<the expected observable outcome holds>",
        ("ac.skeleton.when_secondary", Locale::PtBr) => "<um caminho de erro ou de borda ocorre>",
        ("ac.skeleton.when_secondary", Locale::EnUs) => "<an error or edge path occurs>",
        ("ac.skeleton.then_secondary", Locale::PtBr) => "<o sistema responde conforme especificado>",
        ("ac.skeleton.then_secondary", Locale::EnUs) => "<the system responds as specified>",
        ("ac.skeleton.command", Locale::PtBr) => "<comando executável que verifica este critério>",
        ("ac.skeleton.command", Locale::EnUs) => "<runnable command that verifies this criterion>",
        // Trailing build-green SAFETY criterion — the ONE tautology the linter
        // tolerates (last AC), the compile-floor beneath the behaviour ACs above.
        //
        // It says BUILD and nothing more, because the command it is minted with is
        // the project's build command (`ProjectConfig::build_command_or_fallback`)
        // and nothing more. The older wording promised "and the tests" over a
        // command that never compiled a test — a safety net reporting on a pass it
        // never took, which is the exact defect an acceptance criterion exists to
        // catch. A spec that wants the suite says so in a criterion of its own.
        ("ac.safety.build_green", Locale::PtBr) => "o build do projeto passa verde",
        ("ac.safety.build_green", Locale::EnUs) => "the project build passes green",

        // Scan-digest enrichment block injected into the Context section by
        // `spec_draft::context_enrichment` — the anchors/precedent the digest
        // already found, so the drafted Context is not an empty placeholder.
        // The `_weak` variant labels the anchor list when the digest's honest
        // match report came back `weak`/`none`: the anchors are shown for
        // transparency but flagged so nobody plans on top of noise.
        ("context.scan_anchors", Locale::PtBr) => "Âncoras (do scan)",
        ("context.scan_anchors", Locale::EnUs) => "Anchors (from scan)",
        ("context.scan_anchors_weak", Locale::PtBr) => {
            "Âncoras (do scan — BAIXA CONFIANÇA: casamento fraco, confirme lendo antes de usar)"
        }
        ("context.scan_anchors_weak", Locale::EnUs) => {
            "Anchors (from scan — LOW CONFIDENCE: weak match, confirm by reading before relying)"
        }
        ("context.scan_slices", Locale::PtBr) => "Fatias recorrentes (precedente a espelhar)",
        ("context.scan_slices", Locale::EnUs) => "Recurring slices (precedent to mirror)",

        // File-operation markers accepted in a spec's `## Files` bullet lines
        // (e.g. "- `src/Payable.cs` (create)"). Synonyms for one locale are
        // `|`-separated DATA, merged across locales by
        // [`file_marker_synonyms`] — the single origin both the emitting
        // drafter prose and every validator share, so a pt-BR draft saying
        // `(novo)` is recognised exactly like the EN canonical `(create)`.
        ("marker.create", Locale::PtBr) => "(novo)|(criar)",
        ("marker.create", Locale::EnUs) => "(create)|(new)",
        ("marker.edit", Locale::PtBr) => "(editar)",
        ("marker.edit", Locale::EnUs) => "(edit)",

        // Wave `_summary.md` section headings.
        ("heading.summary.objective", Locale::PtBr) => "Objetivo",
        ("heading.summary.objective", Locale::EnUs) => "Objective",
        ("heading.summary.inheritance", Locale::PtBr) => "Herança",
        ("heading.summary.inheritance", Locale::EnUs) => "Inheritance",
        ("heading.summary.decisions", Locale::PtBr) => "Decisões",
        ("heading.summary.decisions", Locale::EnUs) => "Decisions",
        ("heading.summary.code", Locale::PtBr) => "Código",
        ("heading.summary.code", Locale::EnUs) => "Code",
        ("heading.summary.ac", Locale::PtBr) => "Critérios de Aceitação",
        ("heading.summary.ac", Locale::EnUs) => "Acceptance Criteria",
        ("heading.summary.verdict", Locale::PtBr) => "Verdict",
        ("heading.summary.verdict", Locale::EnUs) => "Verdict",
        ("heading.summary.next_steps", Locale::PtBr) => "Próximos passos",
        ("heading.summary.next_steps", Locale::EnUs) => "Next steps",

        // Wave `_context.md` section headings.
        ("heading.context.objective", Locale::PtBr) => "Objetivo",
        ("heading.context.objective", Locale::EnUs) => "Objective",
        ("heading.context.inheritance", Locale::PtBr) => "Herança",
        ("heading.context.inheritance", Locale::EnUs) => "Inheritance",
        ("heading.context.memory", Locale::PtBr) => "Memória",
        ("heading.context.memory", Locale::EnUs) => "Memory",
        ("heading.context.position", Locale::PtBr) => "Posição no mapa",
        ("heading.context.position", Locale::EnUs) => "Position in map",
        ("heading.context.next_steps_suggestion", Locale::PtBr) => "Sugestão de próximos passos",
        ("heading.context.next_steps_suggestion", Locale::EnUs) => "Next-steps suggestion",

        // Regression gate verdict labels + messages. These are
        // MACHINE / log strings (gate verdicts consumed by the orchestrator and
        // written to telemetry), so they are ENGLISH regardless of the user's
        // configured locale — only `gate.askuser.*` below stays config-lang.
        ("gate.verdict.green.label", _) => "Green",
        ("gate.verdict.amber.label", _) => "Amber",
        ("gate.verdict.red.label", _) => "Red",
        ("gate.verdict.green.message", _) => "No regression signals.",
        ("gate.verdict.amber.message", _) => "Ambiguous signals detected. Confirmation required.",
        ("gate.verdict.red.message", _) => "Regression detected. Consolidation blocked.",

        // Gate signal layer labels (MACHINE / log, English
        // regardless of locale). Use the `{slot}` placeholders to let callers
        // interpolate the matched term, function name, etc.
        ("gate.signal.vocabulary", _) => "Vocabulary matched: {term} (layer {layer})",
        ("gate.signal.stub", _) => "Stub pattern: {pattern} in {function}",
        ("gate.signal.snapshot", _) => "Function {function} emptied ({before_lines} → {after_lines} lines)",

        // Amber AskUserQuestion (printed as JSON, consumed by orchestrator).
        ("gate.askuser.amber.question", Locale::PtBr) => "O gate detectou sinais ambíguos. Autorizar a consolidação?",
        ("gate.askuser.amber.question", Locale::EnUs) => "The gate detected ambiguous signals. Authorize consolidation?",
        ("gate.askuser.amber.option_authorize", Locale::PtBr) => "Autorizar",
        ("gate.askuser.amber.option_authorize", Locale::EnUs) => "Authorize",
        ("gate.askuser.amber.option_block", Locale::PtBr) => "Bloquear",
        ("gate.askuser.amber.option_block", Locale::EnUs) => "Block",
        ("gate.askuser.amber.option_block_desc", Locale::PtBr) => "Bloqueia a consolidação até resolução.",
        ("gate.askuser.amber.option_block_desc", Locale::EnUs) => "Block consolidation until resolved.",

        // Span-level review (subagent_inject + agent_prompt_render).
        // Vocabulary inject block surfaced in the child agent's prompt so the
        // child knows which terms the gate's Moment 1 scan flags.
        ("gate.vocabulary.inject.heading", Locale::PtBr) => "Vocabulário de regressão",
        ("gate.vocabulary.inject.heading", Locale::EnUs) => "Regression vocabulary",
        ("gate.vocabulary.inject.lead", Locale::PtBr) => {
            "Termos que o gate vai checar no seu plano e diff. Evite usar como justificativa."
        }
        ("gate.vocabulary.inject.lead", Locale::EnUs) => {
            "Terms the gate checks in your plan and diff. Avoid using them as justification."
        }
        ("gate.vocabulary.inject.semantic", Locale::PtBr) => "Semântico (alto)",
        ("gate.vocabulary.inject.semantic", Locale::EnUs) => "Semantic (high)",
        ("gate.vocabulary.inject.pattern", Locale::PtBr) => "Padrão (médio)",
        ("gate.vocabulary.inject.pattern", Locale::EnUs) => "Pattern (medium)",
        // Consolidation block message surfaced when a red verdict closes the wave.
        ("gate.consolidation.blocked", Locale::PtBr) => {
            "Consolidação bloqueada: filho {child} retornou verdict vermelho — {message}"
        }
        ("gate.consolidation.blocked", Locale::EnUs) => {
            "Consolidation blocked: child {child} returned a red verdict — {message}"
        }

        // The install-grammars CLI helper.
        // User-facing strings for `mustard install-grammars`. The helper suggests
        // tree-sitter grammar repos for detected languages — Mustard never
        // downloads or compiles. Format is shell-ready markdown so the user can
        // copy + paste straight into a terminal.
        ("cli.install_grammars.title", Locale::PtBr) => {
            "Mustard — sugestões de grammars tree-sitter"
        }
        ("cli.install_grammars.title", Locale::EnUs) => {
            "Mustard — tree-sitter grammar suggestions"
        }
        ("cli.install_grammars.lead", Locale::PtBr) => {
            "Linguagens detectadas neste projeto. Mustard não baixa nem compila — \
             apenas sugere o repositório canônico e o comando shell."
        }
        ("cli.install_grammars.lead", Locale::EnUs) => {
            "Languages detected in this project. Mustard does not download or build — \
             it only suggests the canonical repo and the shell command."
        }
        ("cli.install_grammars.no_stack", Locale::PtBr) => {
            "Nenhuma linguagem detectada via sinais de manifesto. Nada a sugerir."
        }
        ("cli.install_grammars.no_stack", Locale::EnUs) => {
            "No language detected via manifest signals. Nothing to suggest."
        }
        ("cli.install_grammars.repo_label", Locale::PtBr) => "repositório",
        ("cli.install_grammars.repo_label", Locale::EnUs) => "repo",
        ("cli.install_grammars.install_cmd_label", Locale::PtBr) => "instalar",
        ("cli.install_grammars.install_cmd_label", Locale::EnUs) => "install",
        ("cli.install_grammars.already_installed", Locale::PtBr) => "já instalada",
        ("cli.install_grammars.already_installed", Locale::EnUs) => "already installed",
        ("cli.install_grammars.unknown_lang_fallback", Locale::PtBr) => {
            "{lang}: grammar não catalogado — buscar em https://tree-sitter.github.io/tree-sitter/#parsers"
        }
        ("cli.install_grammars.unknown_lang_fallback", Locale::EnUs) => {
            "{lang}: grammar not catalogued — search https://tree-sitter.github.io/tree-sitter/#parsers"
        }
        ("cli.install_grammars.footer", Locale::PtBr) => {
            "Copie o bloco `instalar` no seu shell. Mustard volta a usar a grammar \
             automaticamente assim que `tree-sitter generate` finalizar."
        }
        ("cli.install_grammars.footer", Locale::EnUs) => {
            "Copy the `install` block into your shell. Mustard will pick up the grammar \
             automatically once `tree-sitter generate` finishes."
        }

        // Work-branch gate — the dirty-tree note appended to a checkout-failure
        // verdict, and the reconciliation warning when the run continues on the
        // branch actually active. Config-language: both are user-facing hook
        // feedback (found in review, 2026-07-30: they shipped hardcoded in one
        // locale). `{paths}`/`{more}`, `{target}`/`{error}`/`{actual}`/`{note}`
        // are interpolated by the gate.
        ("workbranch.dirty.note", Locale::PtBr) => " Árvore suja: {paths}{more}.",
        ("workbranch.dirty.note", Locale::EnUs) => " Dirty tree: {paths}{more}.",
        ("workbranch.reconcile.warn", Locale::PtBr) => {
            "não consegui criar a branch '{target}': {error} — seguindo na branch atual \
             '{actual}'; registro do work branch reconciliado de '{target}' para '{actual}'.{note}"
        }
        ("workbranch.reconcile.warn", Locale::EnUs) => {
            "could not create branch '{target}': {error} — continuing on the current branch \
             '{actual}'; the work branch record was reconciled from '{target}' to '{actual}'.{note}"
        }

        // Work-branch REFUSAL — the checkout holds another unit's branch with
        // uncommitted files, so cutting the second unit here would carry them
        // off. Said by BOTH doors (the write gate and the `spec-draft`
        // cut), so it lives in the catalogue rather than at either surface.
        // `{current}`/`{target}`/`{paths}`/`{more}` are interpolated by
        // `work_branch::BusyCheckout::reason`.
        //
        // `{paths}` names the operator's own work — what WOULD ride along. The
        // census the tool itself wrote has sentences of its own below
        // (`workbranch.busy.census_*`): the remedy differs, so the sentence
        // does.
        ("workbranch.busy.refusal", Locale::PtBr) => {
            "O checkout está na branch '{current}', de OUTRA unidade de trabalho, com trabalho \
             NÃO commitado em: {paths}{more}. Criar '{target}' aqui levaria essas edições junto, \
             para dentro de outra unidade. Commite ou guarde (`git stash`) esse trabalho antes de \
             abrir a segunda unidade."
        }
        ("workbranch.busy.refusal", Locale::EnUs) => {
            "The checkout is on branch '{current}', which belongs to ANOTHER work unit, with \
             UNCOMMITTED work in: {paths}{more}. Cutting '{target}' here would carry those edits \
             along into a different unit. Commit or stash (`git stash`) that work before opening \
             the second unit."
        }

        // The SAME refusal when the probe could not answer at all (`git status`
        // failed, or answered in a shape the parser does not understand). It is
        // a distinct sentence because there are no paths to name, and rendering
        // the one above with an empty list would print "work in: ." — which
        // teaches the operator that the refusal is noise. `{current}`/`{target}`
        // are interpolated by `work_branch::BusyCheckout::reason`.
        ("workbranch.busy.unmeasured", Locale::PtBr) => {
            "O checkout está na branch '{current}', de OUTRA unidade de trabalho, e NÃO consegui \
             medir o que há de não commitado ali (o `git status` não respondeu). Criar '{target}' \
             aqui levaria junto qualquer trabalho pendente, para dentro de outra unidade. Commite \
             ou guarde (`git stash`) o que houver — ou conserte o estado do git — antes de abrir a \
             segunda unidade."
        }
        ("workbranch.busy.unmeasured", Locale::EnUs) => {
            "The checkout is on branch '{current}', which belongs to ANOTHER work unit, and the \
             uncommitted work there could NOT be measured (`git status` did not answer). Cutting \
             '{target}' here would carry whatever is pending along into a different unit. Commit \
             or stash (`git stash`) whatever is there — or repair the git state — before opening \
             the second unit."
        }

        // The SAME refusal when the only dirty thing is the CENSUS — the tool's
        // own output — and the checkout is not the base it is recorded on
        // (another unit's branch, a protected branch that is not the base, a
        // detached HEAD). The census must never travel into a unit's branch,
        // and it has nowhere to land here, so the sentence names the base it
        // belongs to instead of blaming the operator for a write that is the
        // tool's. `{current}`/`{target}`/`{base}`/`{paths}`/`{more}` are
        // interpolated by `work_branch::BusyCheckout::reason`.
        ("workbranch.busy.census_off_base", Locale::PtBr) => {
            "O checkout está na branch '{current}' e a única coisa não commitada na árvore são \
             artefatos do censo que o próprio Mustard escreveu: {paths}{more}. O censo só é \
             gravado na base '{base}', e criar '{target}' aqui o levaria para dentro da unidade \
             nova. Volte para '{base}' e abra a unidade de lá, ou commite/guarde (`git stash`) \
             esses arquivos antes."
        }
        ("workbranch.busy.census_off_base", Locale::EnUs) => {
            "The checkout is on branch '{current}' and the only uncommitted thing in the tree is \
             census output Mustard itself wrote: {paths}{more}. The census is recorded on the \
             base '{base}' only, and cutting '{target}' here would carry it into the new unit. \
             Go back to '{base}' and open the unit from there, or commit/stash (`git stash`) \
             those files first."
        }

        // …and when the checkout IS the base, dirty only with the census, but
        // the base is PROTECTED and the door asking is a cut or a hook — which
        // may not write a commit on a protected branch behind the operator's
        // back. The explicit open is the one door that records there, so it is
        // named as the way through.
        ("workbranch.busy.census_protected", Locale::PtBr) => {
            "A base protegida '{current}' está suja só com artefatos do censo que o próprio \
             Mustard escreveu: {paths}{more}. Esta porta não cria commit numa base protegida, e \
             criar '{target}' aqui levaria o censo para dentro da unidade nova. Reabra a unidade \
             pela porta explícita (`emit-pipeline --kind pipeline.kind`), que grava o censo na \
             base, ou commite esses arquivos você mesmo."
        }
        ("workbranch.busy.census_protected", Locale::EnUs) => {
            "The protected base '{current}' is dirty only with census output Mustard itself \
             wrote: {paths}{more}. This door does not write a commit on a protected base, and \
             cutting '{target}' here would carry the census into the new unit. Re-open the unit \
             through the explicit door (`emit-pipeline --kind pipeline.kind`), which records the \
             census on the base, or commit those files yourself."
        }

        // The base this move is about TRAILS its remote and could not be
        // fast-forwarded — a diverged base, one checked out elsewhere, a dirty
        // file in the way that is the operator's. Nothing is cut and nothing is
        // recorded: a unit cut from a stale base re-does merged work, and a
        // census commit written on it would make it diverge for good. Git's own
        // words travel in `{error}`; `{base}` is interpolated by
        // `work_branch::BusyCheckout::reason`.
        ("workbranch.busy.base_stale", Locale::PtBr) => {
            "A base '{base}' está atrás de origin/{base} e não pôde ser avançada — git disse: \
             {error}. Nada foi cortado nem gravado: uma unidade cortada de uma base velha refaz \
             trabalho já integrado. Coloque '{base}' em dia (`git pull --ff-only origin {base}` \
             parado nela; se ela divergiu, resolva a divergência primeiro) e tente de novo."
        }
        ("workbranch.busy.base_stale", Locale::EnUs) => {
            "The base '{base}' is behind origin/{base} and could not be advanced — git said: \
             {error}. Nothing was cut and nothing recorded: a unit cut from a stale base re-does \
             work that is already merged. Bring '{base}' up to date (`git pull --ff-only origin \
             {base}` while on it; if it has diverged, resolve that first) and try again."
        }

        // The base trails its remote, the advance IS a fast-forward, and the
        // only thing in its way is the OPERATOR's uncommitted work in files
        // origin also changed. The census beside it is the tool's and is set
        // aside on its own; their files are named, and the remedy is the stash
        // that unblocks the advance — a `git pull` would fail on the very same
        // files. `{base}`/`{paths}`/`{more}` are interpolated by
        // `work_branch::BusyCheckout::reason`.
        ("workbranch.busy.base_blocked", Locale::PtBr) => {
            "A base '{base}' está atrás de origin/{base}, e avançá-la sobrescreveria trabalho \
             NÃO commitado seu em: {paths}{more}. Nada foi cortado nem gravado, e nada foi \
             tocado. Guarde esse trabalho (`git stash push -- <caminhos>`), coloque '{base}' em \
             dia (`git pull --ff-only origin {base}`), traga-o de volta (`git stash pop`) e \
             tente de novo."
        }
        ("workbranch.busy.base_blocked", Locale::EnUs) => {
            "The base '{base}' is behind origin/{base}, and advancing it would overwrite \
             UNCOMMITTED work of yours in: {paths}{more}. Nothing was cut, nothing recorded, \
             and nothing touched. Stash that work (`git stash push -- <paths>`), bring '{base}' \
             up to date (`git pull --ff-only origin {base}`), take it back (`git stash pop`) \
             and try again."
        }

        // Work-branch BASE UNKNOWN — an emergency unit whose base nothing ever
        // recorded, in a project declaring several it could have been cut from.
        // Nothing is cut, and the operator is told: the harness used to take the
        // outermost candidate and mention it on stderr, which a PreToolUse hook
        // says to nobody (it exits 0). `{target}`/`{candidates}` are
        // interpolated by the gate.
        ("workbranch.base.unknown", Locale::PtBr) => {
            "Não dá para saber de qual base '{target}' deve sair: este projeto declara várias \
             candidatas ({candidates}) e nada registrou a escolha, então a branch NÃO foi criada. \
             Reabra a unidade com a base explícita (--base) — chutar aqui aponta o trabalho para \
             uma base que ninguém escolheu."
        }
        ("workbranch.base.unknown", Locale::EnUs) => {
            "There is no telling which base '{target}' should be cut from: this project declares \
             several candidates ({candidates}) and nothing recorded the choice, so the branch was \
             NOT created. Re-open the unit with an explicit base (--base) — guessing here aims the \
             work at a base nobody chose."
        }

        // BASE GATE — o censo que sobrou sujo na árvore e o portão acabou de
        // gravar por conta própria, em vez de deixá-lo para o corte da próxima
        // unidade recusar como se fosse trabalho do operador. Frase de usuário
        // (sai no stderr da abertura do pipeline), então mora no catálogo como
        // manda a nota do topo deste arquivo, e não embutida no portão em um
        // idioma só. `{paths}` é interpolado por
        // `base_gate::commit_census`, chamado só de
        // `census_settlement::settle`.
        ("basegate.census.recorded", Locale::PtBr) => {
            "base-gate: os artefatos do censo ({paths}) eram a única coisa não commitada na \
             árvore — foram gravados aqui mesmo, na base, para que o corte da próxima unidade \
             não cobre de você uma escrita que é da ferramenta."
        }
        ("basegate.census.recorded", Locale::EnUs) => {
            "base-gate: the census artifacts ({paths}) were the only uncommitted thing in the \
             tree — they were recorded right here, on the base, so the next unit's branch cut \
             does not charge you for a write that is the tool's."
        }
        // …and the three ways the recording can NOT happen, one line each. A
        // recording that fails in silence leaves the census dirty, and the next
        // cut then refuses naming it as the operator's uncommitted work with no
        // prior notice the tool left it there. `Proceed` after a failed
        // recording is a different fact from `Proceed` with nothing owed, and
        // these lines are what says which.
        ("basegate.census.nothing", Locale::PtBr) => {
            "base-gate: os artefatos do censo ({paths}) não deixaram nada para o git gravar — \
             já estão iguais ao que a base tem, ou o git não os enxerga."
        }
        ("basegate.census.nothing", Locale::EnUs) => {
            "base-gate: the census artifacts ({paths}) left nothing for git to record — they \
             already match what the base has, or git does not see them."
        }
        ("basegate.census.not_clean", Locale::PtBr) => {
            "base-gate: os artefatos do censo ({paths}) NÃO foram gravados — a árvore carrega \
             outras mudanças, e um commit aqui as varreria junto. Eles ficam para você commitar \
             ao lado do seu trabalho."
        }
        ("basegate.census.not_clean", Locale::EnUs) => {
            "base-gate: the census artifacts ({paths}) were NOT recorded — the tree carries other \
             changes, and a commit here would sweep them up. They are left for you to commit \
             beside your work."
        }
        ("basegate.census.unavailable", Locale::PtBr) => {
            "base-gate: os artefatos do censo ({paths}) NÃO foram gravados — o git não aceitou o \
             commit (sem `user.email`, um hook que recusou, um erro ao indexar). Eles ficam sujos \
             na árvore, e o próximo corte vai nomeá-los; commite-os você mesmo ou conserte o git."
        }
        ("basegate.census.unavailable", Locale::EnUs) => {
            "base-gate: the census artifacts ({paths}) were NOT recorded — git would not take the \
             commit (no `user.email`, a hook that declined, a staging error). They stay dirty in \
             the tree and the next cut will name them; commit them yourself or repair git."
        }

        // Work-unit SURFACING — the three places the harness says out loud that
        // a work unit is somewhere other than the checkout, or that the exit
        // ritual is still owed. All three are user-facing (a listing legend, a
        // status-bar label, a session-start advisory), so they are
        // config-language and live here rather than inline at the surface.
        //
        // `specs.location.remote_only` explains the third value of the listing's
        // location column: a unit alive only on a remote, which the ref sweep
        // now reaches. `{count}` / `{branches}` in the advisory are
        // interpolated by the caller.
        ("specs.location.remote_only", Locale::PtBr) => {
            "Onde: {remoto}/{branch}=spec só no remoto, nenhuma branch local carrega o \
             diretório (busque a branch antes de agir)"
        }
        ("specs.location.remote_only", Locale::EnUs) => {
            "Where: {remote}/{branch}=spec only on the remote, no local branch carries the \
             directory (fetch the branch before acting)"
        }
        ("statusline.prune.label", Locale::PtBr) => "a podar",
        ("statusline.prune.label", Locale::EnUs) => "to prune",
        ("statusline.harness.inert", Locale::PtBr) => "harness inerte",
        ("statusline.harness.inert", Locale::EnUs) => "harness inert",
        // Dormant is NOT inert: inert means someone switched the plugin off,
        // dormant means its binary never downloaded. Same consequence (no hook
        // runs), opposite remedy — so they must never share a label.
        ("statusline.harness.dormant", Locale::PtBr) => "harness dormente",
        ("statusline.harness.dormant", Locale::EnUs) => "harness dormant",
        ("prune.pending.notice", Locale::PtBr) => {
            "[Mustard] {count} unidade(s) de trabalho já mergeada(s) ainda têm branch viva: \
             {branches}. Diga ao usuário que o ritual de saída ficou pendente e ofereça \
             `mustard-rt run git-settle --report` para conferir o estado de cada uma e \
             `mustard-rt run git-settle --unit <branch>` para podar. Aviso, nunca bloqueio."
        }
        ("prune.pending.notice", Locale::EnUs) => {
            "[Mustard] {count} merged work unit(s) still have a live branch: {branches}. \
             Tell the user the exit ritual is outstanding and offer \
             `mustard-rt run git-settle --report` to check each one's state and \
             `mustard-rt run git-settle --unit <branch>` to prune. Advisory, never blocking."
        }
        // Aviso de sobras do início da sessão: `{total}` e `{count}` são
        // preenchidos pelo chamador (`session_start_inject::scratch_notice`).
        ("scratch.residue.notice", Locale::PtBr) => {
            "[Mustard] As cópias descartáveis antigas no diretório temporário somam {total} \
             em {count} pasta(s). Diga ao usuário que o disco está sendo gasto com sobras e \
             ofereça `mustard-rt run scratch-gc` para listar o que sai e \
             `mustard-rt run scratch-gc --apply` para apagar. Aviso, nunca bloqueio."
        }
        ("scratch.residue.notice", Locale::EnUs) => {
            "[Mustard] Old throwaway copies in the temp directory add up to {total} across \
             {count} folder(s). Tell the user the disk is being spent on leftovers and offer \
             `mustard-rt run scratch-gc` to list what would go and \
             `mustard-rt run scratch-gc --apply` to delete it. Advisory, never blocking."
        }

        // Scope-classify `## Files` diagnostics — the three ZERO-PATH shapes,
        // each named for what was actually measured (a diagnostic must never
        // assert "empty" about a section that has content). Config-language:
        // the warning is user-facing feedback in the spec's own language.
        ("scope.files.absent", Locale::PtBr) => {
            "## Arquivos ausente — fileCount=0; scope=abstain até autorar o censo \
             (adicione ## Arquivos e re-rode)"
        }
        ("scope.files.absent", Locale::EnUs) => {
            "## Files section absent — fileCount=0; scope=abstain until the census is \
             authored (add ## Files and re-run)"
        }
        ("scope.files.empty", Locale::PtBr) => {
            "## Arquivos vazio/placeholder — fileCount=0; scope=abstain até autorar o \
             censo (preencha ## Arquivos e re-rode)"
        }
        ("scope.files.empty", Locale::EnUs) => {
            "## Files section empty/placeholder — fileCount=0; scope=abstain until the \
             census is authored (fill ## Files and re-run)"
        }
        ("scope.files.unrecognised", Locale::PtBr) => {
            "## Arquivos tem conteúdo, mas nenhum caminho foi reconhecido — fileCount=0; \
             scope=abstain; declare cada arquivo como bullet `- caminho` ou linha de \
             tabela com coluna de caminho, e re-rode"
        }
        ("scope.files.unrecognised", Locale::EnUs) => {
            "## Files has content, but no path was recognised — fileCount=0; \
             scope=abstain; declare each file as a `- path` bullet or a table row with \
             a path column, then re-run"
        }

        // Resumo da spec em HTML (`apps/rt/src/commands/spec/spec_doc.rs`) — o
        // documento que o usuário lê ANTES de aprovar. Texto de usuário, então
        // segue o `language.text` e mora aqui, não embutido no comando. `{wave}` e
        // `{term}` são preenchidos pelo chamador.
        ("doc.kind.approval", Locale::PtBr) => "spec para aprovar",
        ("doc.kind.approval", Locale::EnUs) => "spec awaiting approval",
        ("doc.kind.summary", Locale::PtBr) => "resumo da spec",
        ("doc.kind.summary", Locale::EnUs) => "spec summary",
        ("doc.meta.spec", _) => "spec",
        ("doc.meta.branch", _) => "branch",
        ("doc.meta.base", Locale::PtBr) => "sai de",
        ("doc.meta.base", Locale::EnUs) => "cut from",
        ("doc.stage.analyze", Locale::PtBr) => "em análise",
        ("doc.stage.analyze", Locale::EnUs) => "under analysis",
        ("doc.stage.awaiting", Locale::PtBr) => "aguardando aprovação",
        ("doc.stage.awaiting", Locale::EnUs) => "awaiting approval",
        ("doc.stage.approved", Locale::PtBr) => "aprovada, execução por começar",
        ("doc.stage.approved", Locale::EnUs) => "approved, execution not started",
        ("doc.stage.execute", Locale::PtBr) => "em execução",
        ("doc.stage.execute", Locale::EnUs) => "executing",
        ("doc.stage.review", Locale::PtBr) => "em revisão",
        ("doc.stage.review", Locale::EnUs) => "under review",
        ("doc.stage.verify", Locale::PtBr) => "em verificação",
        ("doc.stage.verify", Locale::EnUs) => "under verification",
        ("doc.stage.close", Locale::PtBr) => "fechando",
        ("doc.stage.close", Locale::EnUs) => "closing",
        ("doc.stage.completed", Locale::PtBr) => "fechada",
        ("doc.stage.completed", Locale::EnUs) => "closed",
        ("doc.summary.lead", Locale::PtBr) => "Resumo.",
        ("doc.summary.lead", Locale::EnUs) => "Summary.",
        ("doc.section.where", Locale::PtBr) => "Onde estamos",
        ("doc.section.where", Locale::EnUs) => "Where we are",
        ("doc.section.clarified", Locale::PtBr) => "O que foi esclarecido",
        ("doc.section.clarified", Locale::EnUs) => "What was clarified",
        ("doc.section.decisions", Locale::PtBr) => "Decisões",
        ("doc.section.decisions", Locale::EnUs) => "Decisions",
        ("doc.section.risks", Locale::PtBr) => "Riscos",
        ("doc.section.risks", Locale::EnUs) => "Risks",
        ("doc.section.flow", Locale::PtBr) => "Antes e depois",
        ("doc.section.flow", Locale::EnUs) => "Before and after",
        ("doc.section.spec", Locale::PtBr) => "A spec",
        ("doc.section.spec", Locale::EnUs) => "The spec",
        ("doc.section.criteria", Locale::PtBr) => "Critérios de aceite",
        ("doc.section.criteria", Locale::EnUs) => "Acceptance criteria",
        ("doc.section.waves", Locale::PtBr) => "Ondas e skills",
        ("doc.section.waves", Locale::EnUs) => "Waves and skills",
        ("doc.section.evidence", Locale::PtBr) => "Evidências",
        ("doc.section.evidence", Locale::EnUs) => "Evidence",
        ("doc.section.pending", Locale::PtBr) => "Pendências abertas",
        ("doc.section.pending", Locale::EnUs) => "Open pending items",
        ("doc.section.next", Locale::PtBr) => "Próximo passo",
        ("doc.section.next", Locale::EnUs) => "Next step",
        // Os sete passos do processo, na ordem em que acontecem.
        ("doc.step.analyze.name", Locale::PtBr) => "Análise",
        ("doc.step.analyze.name", Locale::EnUs) => "Analysis",
        ("doc.step.analyze.desc", Locale::PtBr) => "ler o código e a conversa.",
        ("doc.step.analyze.desc", Locale::EnUs) => "read the code and the conversation.",
        ("doc.step.plan.name", Locale::PtBr) => "Plano",
        ("doc.step.plan.name", Locale::EnUs) => "Plan",
        ("doc.step.plan.desc", Locale::PtBr) => {
            "spec e ondas escritas, e cada critério rodado para provar que hoje ele falha."
        }
        ("doc.step.plan.desc", Locale::EnUs) => {
            "spec and waves written, and each criterion run to prove it fails today."
        }
        ("doc.step.approval.name", Locale::PtBr) => "Aprovação",
        ("doc.step.approval.name", Locale::EnUs) => "Approval",
        ("doc.step.approval.desc", Locale::PtBr) => "você aprova com `/mustard:spec`.",
        ("doc.step.approval.desc", Locale::EnUs) => "you approve with `/mustard:spec`.",
        ("doc.step.execute.name", Locale::PtBr) => "Execução",
        ("doc.step.execute.name", Locale::EnUs) => "Execution",
        ("doc.step.execute.desc", Locale::PtBr) => {
            "um agente por onda escreve o código, uma onda depois da outra."
        }
        ("doc.step.execute.desc", Locale::EnUs) => {
            "one agent per wave writes the code, one wave after another."
        }
        ("doc.step.review.name", Locale::PtBr) => "Revisão",
        ("doc.step.review.name", Locale::EnUs) => "Review",
        ("doc.step.review.desc", Locale::PtBr) => "um agente revisor tenta achar falhas.",
        ("doc.step.review.desc", Locale::EnUs) => "a reviewer agent tries to find flaws.",
        ("doc.step.verify.name", Locale::PtBr) => "Verificação",
        ("doc.step.verify.name", Locale::EnUs) => "Verification",
        ("doc.step.verify.desc", Locale::PtBr) => "os critérios rodam de novo e agora precisam passar.",
        ("doc.step.verify.desc", Locale::EnUs) => "the criteria run again and must now pass.",
        ("doc.step.close.name", Locale::PtBr) => "Fechamento",
        ("doc.step.close.name", Locale::EnUs) => "Closing",
        ("doc.step.close.desc", Locale::PtBr) => "pull request e merge na base.",
        ("doc.step.close.desc", Locale::EnUs) => "pull request and merge into the base.",
        ("doc.step.here", Locale::PtBr) => "Estamos aqui.",
        ("doc.step.here", Locale::EnUs) => "We are here.",
        ("doc.col.question", Locale::PtBr) => "Pergunta",
        ("doc.col.question", Locale::EnUs) => "Question",
        ("doc.col.answer", Locale::PtBr) => "Resposta",
        ("doc.col.answer", Locale::EnUs) => "Answer",
        ("doc.clarified.definition", Locale::PtBr) => "O que quer dizer {term} aqui?",
        ("doc.clarified.definition", Locale::EnUs) => "What does {term} mean here?",
        ("doc.clarified.terms", Locale::PtBr) => "Termos definidos na rodada de termos",
        ("doc.clarified.terms", Locale::EnUs) => "Terms settled in the term round",
        ("doc.clarified.reason", Locale::PtBr) => "Por que não houve rodada de termos?",
        ("doc.clarified.reason", Locale::EnUs) => "Why was there no term round?",
        ("doc.clarified.notes", Locale::PtBr) => "Nota:",
        ("doc.clarified.notes", Locale::EnUs) => "Note:",
        ("doc.col.severity", Locale::PtBr) => "Gravidade",
        ("doc.col.severity", Locale::EnUs) => "Severity",
        ("doc.col.risk", Locale::PtBr) => "Risco",
        ("doc.col.risk", Locale::EnUs) => "Risk",
        ("doc.col.mitigation", Locale::PtBr) => "O que atenua",
        ("doc.col.mitigation", Locale::EnUs) => "What mitigates it",
        ("doc.severity.alta", Locale::PtBr) => "Alta",
        ("doc.severity.alta", Locale::EnUs) => "High",
        ("doc.severity.media", Locale::PtBr) => "Média",
        ("doc.severity.media", Locale::EnUs) => "Medium",
        ("doc.severity.baixa", Locale::PtBr) => "Baixa",
        ("doc.severity.baixa", Locale::EnUs) => "Low",
        ("doc.criteria.lead", Locale::PtBr) => {
            "Cada critério é um comando. Antes de o código existir ele precisa falhar, e é \
             essa falha que prova que ele mede algo novo; depois da entrega, precisa passar."
        }
        ("doc.criteria.lead", Locale::EnUs) => {
            "Each criterion is a command. Before the code exists it must fail, and that \
             failure is what proves it measures something new; after delivery it must pass."
        }
        ("doc.col.id", _) => "Id",
        ("doc.col.criterion", Locale::PtBr) => "Quando… então…",
        ("doc.col.criterion", Locale::EnUs) => "When… then…",
        ("doc.col.wave", Locale::PtBr) => "Onda",
        ("doc.col.wave", Locale::EnUs) => "Wave",
        ("doc.col.proof", Locale::PtBr) => "Hoje",
        ("doc.col.proof", Locale::EnUs) => "Today",
        ("doc.proof.red", Locale::PtBr) => "falha provada",
        ("doc.proof.red", Locale::EnUs) => "failure proven",
        ("doc.proof.confirmed", Locale::PtBr) => "confirmado",
        ("doc.proof.confirmed", Locale::EnUs) => "confirmed",
        ("doc.proof.exempt", Locale::PtBr) => "isento",
        ("doc.proof.exempt", Locale::EnUs) => "exempt",
        ("doc.proof.none", Locale::PtBr) => "sem prova",
        ("doc.proof.none", Locale::EnUs) => "not proven",
        ("doc.waves.lead", Locale::PtBr) => {
            "As skills são os moldes que o agente de cada onda carrega antes de escrever os \
             arquivos que elas governam. A lista sai do cruzamento dos arquivos da onda com \
             as pastas de cada molde."
        }
        ("doc.waves.lead", Locale::EnUs) => {
            "Skills are the molds each wave's agent loads before writing the files they \
             govern. The list comes from crossing the wave's files with each mold's folders."
        }
        ("doc.wave.done", Locale::PtBr) => "concluída",
        ("doc.wave.done", Locale::EnUs) => "done",
        ("doc.wave.skills", _) => "Skills",
        ("doc.wave.covers", Locale::PtBr) => "cobre",
        ("doc.wave.covers", Locale::EnUs) => "covers",
        ("doc.wave.criteria", Locale::PtBr) => "Critérios",
        ("doc.wave.criteria", Locale::EnUs) => "Criteria",
        ("doc.wave.obligations", Locale::PtBr) => "Obrigação externa",
        ("doc.wave.obligations", Locale::EnUs) => "External obligation",
        ("doc.col.seen", Locale::PtBr) => "O que foi visto no código",
        ("doc.col.seen", Locale::EnUs) => "What was seen in the code",
        ("doc.col.where", Locale::PtBr) => "Onde",
        ("doc.col.where", Locale::EnUs) => "Where",
        ("doc.col.pending", Locale::PtBr) => "Pendência",
        ("doc.col.pending", Locale::EnUs) => "Pending item",
        ("doc.next.analyze", Locale::PtBr) => "A análise segue; a spec é escrita no passo seguinte.",
        ("doc.next.analyze", Locale::EnUs) => "Analysis continues; the spec is written in the next step.",
        ("doc.next.approve", Locale::PtBr) => {
            "Para aprovar, digite `/mustard:spec` neste branch. A onda 1 começa."
        }
        ("doc.next.approve", Locale::EnUs) => {
            "To approve, type `/mustard:spec` on this branch. Wave 1 starts."
        }
        ("doc.next.adjust", Locale::PtBr) => "Para ajustar, diga o que mudar.",
        ("doc.next.adjust", Locale::EnUs) => "To adjust, say what to change.",
        ("doc.next.approved", Locale::PtBr) => "A spec está aprovada. `/mustard:spec` começa a onda 1.",
        ("doc.next.approved", Locale::EnUs) => "The spec is approved. `/mustard:spec` starts wave 1.",
        ("doc.next.execute", Locale::PtBr) => "A execução segue na onda {wave}, com `/mustard:spec`.",
        ("doc.next.execute", Locale::EnUs) => "Execution continues with wave {wave}, via `/mustard:spec`.",
        ("doc.next.execute_done", Locale::PtBr) => "Todas as ondas terminaram; a revisão vem a seguir.",
        ("doc.next.execute_done", Locale::EnUs) => "Every wave is done; review comes next.",
        ("doc.next.review", Locale::PtBr) => {
            "A revisão e a verificação estão rodando; o resultado aparece nesta página."
        }
        ("doc.next.review", Locale::EnUs) => {
            "Review and verification are running; the result shows up on this page."
        }
        ("doc.next.close", Locale::PtBr) => "Falta fechar: pull request e merge na base.",
        ("doc.next.close", Locale::EnUs) => "What is left is closing: pull request and merge into the base.",
        ("doc.next.completed", Locale::PtBr) => "A unidade está fechada. Não há mais nada a fazer aqui.",
        ("doc.next.completed", Locale::EnUs) => "The unit is closed. Nothing else to do here.",
        ("doc.footer", Locale::PtBr) => {
            "Montado pelo Mustard a partir da spec, das ondas, do material da conversa, da \
             prova dos critérios e da lista de pendências."
        }
        ("doc.footer", Locale::EnUs) => {
            "Built by Mustard from the spec, the waves, the conversation material, the \
             criteria proof and the pending list."
        }

        // Pendências abertas (`apps/rt/src/hooks/session/session_start_inject.rs`
        // e a regra das pendências do fim da resposta,
        // `apps/rt/src/hooks/task/pending_gate.rs`). `{count}` e `{items}` vêm
        // do chamador; a lista usa a grafia de `format_pending_items`. O
        // bloqueio pede o título, nunca o número: a regra de escrita barra o
        // código interno ("P-3") na conversa.
        ("pending.notice", Locale::PtBr) => {
            "[Mustard] Trabalho combinado ainda aberto ({count}): {items}. Esses itens vivem \
             fora de toda unidade e sobrevivem à que os entrega: quando uma unidade fecha \
             (pull request mergeado ou spec concluída), a mensagem final cita cada item aberto \
             pelo id ou pelo título. Grave trabalho combinado novo com \
             `mustard-rt run pending --add`; um item só sai da lista com um motivo \
             (`--close <id>` ou `--drop <id>`, mais `--reason`)."
        }
        ("pending.notice", Locale::EnUs) => {
            "[Mustard] Agreed work still open ({count}): {items}. These items live outside \
             every unit and outlive the one that delivers them: when a unit closes (pull \
             request merged or spec completed), the final message names each open item by id \
             or title. Record new agreed work with `mustard-rt run pending --add`; an item \
             leaves the list only with a reason (`--close <id>` or `--drop <id>`, plus \
             `--reason`)."
        }
        ("pending.gate.block", Locale::PtBr) => {
            "[Mustard] Uma unidade fechou neste turno, e a mensagem final não cita {count} \
             pendência(s) aberta(s): {items}. O trabalho combinado sobrevive à unidade que \
             fechou — reescreva a mensagem de fechamento citando cada uma pelo título, sem o \
             número. Uma pendência que não vale mais só sai da lista com um motivo: \
             `mustard-rt run pending --close <id> --reason \"…\"` (entregue) ou \
             `mustard-rt run pending --drop <id> --reason \"…\"` (desistência)."
        }
        ("pending.gate.block", Locale::EnUs) => {
            "[Mustard] A unit closed in this turn, and the final message does not name {count} \
             open pending item(s): {items}. Agreed work outlives the unit that closed — rewrite \
             the closing message naming each one by title, without the number. An item that no \
             longer stands \
             leaves the list only with a reason: `mustard-rt run pending --close <id> --reason \
             \"…\"` (delivered) or `mustard-rt run pending --drop <id> --reason \"…\"` (given up)."
        }
        // Recusa do `run pending --add` (`apps/rt/src/commands/event/pending.rs`):
        // o título repetido, sem ligar para maiúscula nem acento.
        ("pending.duplicate", Locale::PtBr) => {
            "Já existe uma pendência aberta com esse título: {id} \"{title}\". Nada foi gravado. \
             Para mudar o combinado, feche a antiga com `mustard-rt run pending --close {id} \
             --reason \"…\"` ou use outro título."
        }
        ("pending.duplicate", Locale::EnUs) => {
            "An open pending item already has this title: {id} \"{title}\". Nothing was written. \
             To change what was agreed, close the old one with `mustard-rt run pending --close \
             {id} --reason \"…\"` or pick another title."
        }

        // Recusas e avisos do arquivo de eventos da spec (`domain::spec_events`,
        // comandos `run write` e `run read`). As vagas vêm do chamador.
        ("spec_events.not_an_object", Locale::PtBr) => {
            "Os campos do evento precisam vir num objeto JSON, como {\"text\": \"…\"}, e o que \
             veio não serve: {detail}. Nada foi gravado."
        }
        ("spec_events.not_an_object", Locale::EnUs) => {
            "The event's fields must come as one JSON object, like {\"text\": \"…\"}, and what \
             came does not parse: {detail}. Nothing was written."
        }
        ("spec_events.unknown_type", Locale::PtBr) => {
            "O tipo {type} não existe no arquivo da spec. Nada foi gravado. Tipos aceitos: {types}."
        }
        ("spec_events.unknown_type", Locale::EnUs) => {
            "The spec file has no {type} event type. Nothing was written. Accepted types: {types}."
        }
        ("spec_events.missing_field", Locale::PtBr) => {
            "O evento {type} precisa do campo {field}, que faltou ou veio vazio. Nada foi gravado."
        }
        ("spec_events.missing_field", Locale::EnUs) => {
            "The {type} event needs the {field} field, which is missing or empty. Nothing was \
             written."
        }
        ("spec_events.invalid_value", Locale::PtBr) => {
            "O campo {field} do evento {type} precisa ser {expected}. Nada foi gravado."
        }
        ("spec_events.invalid_value", Locale::EnUs) => {
            "The {field} field of the {type} event must be {expected}. Nothing was written."
        }
        ("spec_events.wrong_count", Locale::PtBr) => {
            "O campo {field} do evento {type} leva de {min} a {max} itens, e vieram {count}. Nada \
             foi gravado."
        }
        ("spec_events.wrong_count", Locale::EnUs) => {
            "The {field} field of the {type} event takes {min} to {max} items, and {count} came. \
             Nothing was written."
        }
        ("spec_events.fact_without_source", Locale::PtBr) => {
            "O fato {fact} do ponto não tem fonte. Diga de onde ele saiu: o arquivo e a linha, o \
             comando com o resultado, ou o número da mensagem do usuário. Nada foi gravado."
        }
        ("spec_events.fact_without_source", Locale::EnUs) => {
            "Fact {fact} of the point has no source. Say where it came from: the file and line, \
             the command with its result, or the number of the user's message. Nothing was \
             written."
        }
        ("spec_events.cited_file_missing", Locale::PtBr) => {
            "O fato {fact} cita {path}, e esse arquivo não existe. Confira o caminho antes de \
             afirmar. Nada foi gravado."
        }
        ("spec_events.cited_file_missing", Locale::EnUs) => {
            "Fact {fact} cites {path}, and that file does not exist. Check the path before stating \
             it. Nothing was written."
        }
        ("spec_events.cited_line_missing", Locale::PtBr) => {
            "O fato {fact} cita a linha {line} de {path}, mas o arquivo tem {lines} linhas. Confira \
             a linha antes de afirmar. Nada foi gravado."
        }
        ("spec_events.cited_line_missing", Locale::EnUs) => {
            "Fact {fact} cites line {line} of {path}, but the file has {lines} lines. Check the \
             line before stating it. Nothing was written."
        }
        ("spec_events.unknown_target", Locale::PtBr) => {
            "O evento {id} não existe nesta spec. Nada foi gravado."
        }
        ("spec_events.unknown_target", Locale::EnUs) => {
            "Event {id} does not exist in this spec. Nothing was written."
        }
        ("spec_events.unknown_code", Locale::PtBr) => {
            "O item {code} não existe nesta spec. Confira o código na página ou no read. Nada foi \
             gravado."
        }
        ("spec_events.unknown_code", Locale::EnUs) => {
            "Item {code} does not exist in this spec. Check the code on the page or in read. \
             Nothing was written."
        }
        ("spec_events.binary_only_field", Locale::PtBr) => {
            "O campo {field} é gravado só pelo binário e não pode vir no --json. Para apontar um \
             item pelo código, use replaces ou os alvos de remove e purge. Nada foi gravado."
        }
        ("spec_events.binary_only_field", Locale::EnUs) => {
            "The {field} field is written only by the binary and cannot come in --json. To point \
             at an item by its code, use replaces or the targets of remove and purge. Nothing was \
             written."
        }
        ("spec_events.replaces_other_type", Locale::PtBr) => {
            "O evento {id} é do tipo {found}, e a versão nova veio como {type}; ela precisa ser do \
             mesmo tipo. Nada foi gravado."
        }
        ("spec_events.replaces_other_type", Locale::EnUs) => {
            "Event {id} is a {found}, and the new version came as {type}; it must have the same \
             type. Nothing was written."
        }
        ("spec_events.filter_matches_nothing", Locale::PtBr) => {
            "Nenhum evento {type} entre {from} e {to}. Nada foi gravado."
        }
        ("spec_events.filter_matches_nothing", Locale::EnUs) => {
            "No {type} event between {from} and {to}. Nothing was written."
        }
        ("spec_events.unknown_block", Locale::PtBr) => "O bloco {block} não existe. Blocos: {blocks}.",
        ("spec_events.unknown_block", Locale::EnUs) => "There is no {block} block. Blocks: {blocks}.",
        ("spec_events.bad_spec_name", Locale::PtBr) => {
            "{spec} não serve como nome de spec: use um nome sem barra e sem \"..\"."
        }
        ("spec_events.bad_spec_name", Locale::EnUs) => {
            "{spec} cannot name a spec: use a name with no slash and no \"..\"."
        }
        ("spec_events.no_spec_file", Locale::PtBr) => "A spec {spec} ainda não tem arquivo de eventos.",
        ("spec_events.no_spec_file", Locale::EnUs) => "The spec {spec} has no event file yet.",
        ("spec_events.io_failed", Locale::PtBr) => "Não consegui usar o arquivo da spec: {detail}.",
        ("spec_events.io_failed", Locale::EnUs) => "Could not use the spec file: {detail}.",
        ("spec_events.skipped_line", Locale::PtBr) => {
            "A linha {line} do spec.ndjson não se entende e foi pulada; o resto do arquivo foi lido."
        }
        ("spec_events.skipped_line", Locale::EnUs) => {
            "Line {line} of spec.ndjson could not be understood and was skipped; the rest of the \
             file was read."
        }
        ("spec_events.duplicate_id", Locale::PtBr) => {
            "A linha {line} do spec.ndjson repete o número {id}, que já apareceu antes, e foi \
             pulada."
        }
        ("spec_events.duplicate_id", Locale::EnUs) => {
            "Line {line} of spec.ndjson repeats number {id}, already used above, and was skipped."
        }
        ("spec_events.kind.text", Locale::PtBr) => "um texto",
        ("spec_events.kind.text", Locale::EnUs) => "a text",
        ("spec_events.kind.int", Locale::PtBr) => "um número inteiro",
        ("spec_events.kind.int", Locale::EnUs) => "a whole number",
        ("spec_events.kind.bool", Locale::PtBr) => "true ou false",
        ("spec_events.kind.bool", Locale::EnUs) => "true or false",
        ("spec_events.kind.object", Locale::PtBr) => "um objeto JSON",
        ("spec_events.kind.object", Locale::EnUs) => "a JSON object",
        ("spec_events.kind.ints", Locale::PtBr) => "uma lista de números inteiros",
        ("spec_events.kind.ints", Locale::EnUs) => "a list of whole numbers",
        ("spec_events.kind.texts", Locale::PtBr) => "uma lista de textos",
        ("spec_events.kind.texts", Locale::EnUs) => "a list of texts",
        ("spec_events.kind.objects", Locale::PtBr) => "uma lista de objetos JSON",
        ("spec_events.kind.objects", Locale::EnUs) => "a list of JSON objects",
        ("spec_events.kind.list", Locale::PtBr) => "uma lista",
        ("spec_events.kind.list", Locale::EnUs) => "a list",
        ("spec_events.kind.one_of", Locale::PtBr) => "uma destas palavras: {values}",
        ("spec_events.kind.one_of", Locale::EnUs) => "one of these words: {values}",
        ("spec_events.kind.many_of", Locale::PtBr) => "uma lista só com estas palavras: {values}",
        ("spec_events.kind.many_of", Locale::EnUs) => "a list with only these words: {values}",
        ("spec_events.kind.text_or_object", Locale::PtBr) => "um texto ou um objeto JSON",
        ("spec_events.kind.text_or_object", Locale::EnUs) => "a text or a JSON object",
        ("spec_events.kind.time", Locale::PtBr) => "uma data e hora como 2026-09-11T21:03",
        ("spec_events.kind.time", Locale::EnUs) => "a date and time like 2026-09-11T21:03",
        ("spec_events.kind.ref", Locale::PtBr) => {
            "o número de um evento ou o código de um item, como MSTD-RULE-0002"
        }
        ("spec_events.kind.ref", Locale::EnUs) => "an event number or an item code, like MSTD-RULE-0002",
        ("spec_events.kind.refs", Locale::PtBr) => {
            "uma lista de números de evento ou de códigos de item, como MSTD-RULE-0002"
        }
        ("spec_events.kind.refs", Locale::EnUs) => {
            "a list of event numbers or item codes, like MSTD-RULE-0002"
        }

        // O índice das specs (`io::spec_index`, o comando `run index` e a
        // conferência do `doctor`). As vagas vêm do chamador.
        ("spec_index.write_warning", Locale::PtBr) => {
            "O evento foi gravado, mas a linha da spec no índice não foi refeita: {detail}. Rode \
             `mustard-rt run index` para refazer o índice."
        }
        ("spec_index.write_warning", Locale::EnUs) => {
            "The event was written, but the spec's line in the index was not rebuilt: {detail}. \
             Run `mustard-rt run index` to rebuild the index."
        }
        ("spec_index.missing", Locale::PtBr) => {
            "O índice das specs (.claude/spec/index.ndjson) não existe, e há {count} spec(s) com \
             arquivo de eventos. Rode `mustard-rt run index` para refazê-lo."
        }
        ("spec_index.missing", Locale::EnUs) => {
            "The spec index (.claude/spec/index.ndjson) does not exist, and {count} spec(s) have an \
             event file. Run `mustard-rt run index` to rebuild it."
        }
        ("spec_index.diverged", Locale::PtBr) => {
            "O índice das specs difere dos arquivos de eventos em {count} linha(s): {specs}. Rode \
             `mustard-rt run index` para refazê-lo."
        }
        ("spec_index.diverged", Locale::EnUs) => {
            "The spec index differs from the event files in {count} line(s): {specs}. Run \
             `mustard-rt run index` to rebuild it."
        }
        ("spec_index.stale_search", Locale::PtBr) => {
            "{count} linha(s) dos arquivos de eventos têm o campo search calculado por outro \
             redutor. Rode `mustard-rt run index` para recalculá-lo."
        }
        ("spec_index.stale_search", Locale::EnUs) => {
            "{count} line(s) of the event files have a search field computed by another stemmer. \
             Run `mustard-rt run index` to recompute it."
        }
        ("spec_index.no_specs", Locale::PtBr) => {
            "Nenhuma spec tem arquivo de eventos: não há índice a conferir."
        }
        ("spec_index.no_specs", Locale::EnUs) => "No spec has an event file: there is no index to check.",

        // Defeitos de clareza de uma resposta (`domain::clarity`) — cada um é
        // uma linha curta que o assistente recebe no bloqueio do fim da
        // resposta, ou que o usuário lê no aviso. Sem parênteses: o tom técnico
        // os apagaria. `{words}`, `{opening}`, `{acronym}`, `{term}`, `{code}`,
        // `{lines}`, `{limit}`, `{score}`, `{min}`, `{found}` e `{expected}` vêm
        // do chamador.
        ("clarity.long_sentence", Locale::PtBr) => "frase com {words} palavras: \"{opening}…\"",
        ("clarity.long_sentence", Locale::EnUs) => "sentence with {words} words: \"{opening}…\"",
        ("clarity.unexpanded_acronym", Locale::PtBr) => "{acronym} sem as palavras por extenso",
        ("clarity.unexpanded_acronym", Locale::EnUs) => "{acronym} without its full words",
        ("clarity.unexplained_term", Locale::PtBr) => "{term} usado sem tradução",
        ("clarity.unexplained_term", Locale::EnUs) => "{term} used without a translation",
        ("clarity.internal_code", Locale::PtBr) => {
            "{code} é um código interno; diga o assunto pelo nome"
        }
        ("clarity.internal_code", Locale::EnUs) => "{code} is an internal code; name the subject instead",
        ("clarity.too_long", Locale::PtBr) => "resposta com {lines} linhas; o limite é {limit}",
        ("clarity.too_long", Locale::EnUs) => "reply with {lines} lines; the limit is {limit}",
        ("clarity.hard_to_read", Locale::PtBr) => {
            "texto difícil de ler: nota {score} no índice de Flesch, e o mínimo é {min}; use \
             frases e palavras mais curtas"
        }
        ("clarity.hard_to_read", Locale::EnUs) => {
            "hard to read: {score} on the Flesch reading-ease index, and the minimum is {min}; \
             use shorter sentences and words"
        }
        // A prosa saiu num idioma que não é o do projeto, que é o do usuário.
        // `{found}` e `{expected}` são códigos de idioma: pt-BR, en-US.
        ("clarity.wrong_language", Locale::PtBr) => {
            "resposta em {found}; o idioma do projeto e do usuário é {expected}"
        }
        ("clarity.wrong_language", Locale::EnUs) => {
            "reply in {found}; the language of the project and the user is {expected}"
        }
        // O fim da resposta (`apps/rt/src/hooks/task/end_of_turn_check.rs`):
        // a primeira resposta que reprova é barrada, e o assistente recebe o
        // bloqueio para reescrever; a reescrita que ainda reprova só gera o
        // aviso ao usuário. Os defeitos vêm abaixo, um por linha.
        ("clarity.block.head", Locale::PtBr) => {
            "[Mustard] A resposta fugiu da regra de escrita. Reescreva-a em linguagem simples, \
             corrigindo estes pontos:"
        }
        ("clarity.block.head", Locale::EnUs) => {
            "[Mustard] The reply missed the writing rule. Rewrite it in plain language, fixing \
             these points:"
        }
        ("clarity.note.head", Locale::PtBr) => {
            "Mustard · clareza: a resposta acima ainda foge da regra de escrita:"
        }
        ("clarity.note.head", Locale::EnUs) => {
            "Mustard · clarity: the reply above still misses the writing rule:"
        }
        // A última linha da lista quando há mais defeitos do que ela mostra.
        ("clarity.more", Locale::PtBr) => "e mais {count}",
        ("clarity.more", Locale::EnUs) => "and {count} more",

        // A página e o `.md` de uma spec (`view::document`): os títulos dos
        // blocos, os nomes dos tipos, os rótulos dos campos e dos valores.
        ("page.block.state", Locale::PtBr) => "Estado",
        ("page.block.state", Locale::EnUs) => "State",
        ("page.block.metrics", Locale::PtBr) => "Painel de medição",
        ("page.block.metrics", Locale::EnUs) => "Measurement panel",
        ("page.block.agreed", Locale::PtBr) => "Combinado",
        ("page.block.agreed", Locale::EnUs) => "Agreed",
        ("page.block.specification", Locale::PtBr) => "Especificação",
        ("page.block.specification", Locale::EnUs) => "Specification",
        ("page.block.criteria", Locale::PtBr) => "Critérios",
        ("page.block.criteria", Locale::EnUs) => "Criteria",
        ("page.block.waves", Locale::PtBr) => "Ondas",
        ("page.block.waves", Locale::EnUs) => "Waves",
        ("page.block.review", Locale::PtBr) => "Revisão e QA",
        ("page.block.review", Locale::EnUs) => "Review and QA",
        ("page.block.progress", Locale::PtBr) => "Andamento",
        ("page.block.progress", Locale::EnUs) => "Progress",
        ("page.block.notes", Locale::PtBr) => "Anotações",
        ("page.block.notes", Locale::EnUs) => "Notes",
        ("page.block.conversation", Locale::PtBr) => "Conversa",
        ("page.block.conversation", Locale::EnUs) => "Conversation",

        ("page.kind.spec", _) => "spec",
        ("page.kind.project", Locale::PtBr) => "projeto",
        ("page.kind.project", Locale::EnUs) => "project",
        ("page.meta.spec", _) => "spec",
        ("page.meta.phase", Locale::PtBr) => "fase",
        ("page.meta.phase", Locale::EnUs) => "phase",
        ("page.meta.branch", _) => "branch",
        ("page.meta.base", Locale::PtBr) => "sai de",
        ("page.meta.base", Locale::EnUs) => "cut from",
        ("page.empty", Locale::PtBr) => "Nada registrado ainda.",
        ("page.empty", Locale::EnUs) => "Nothing recorded yet.",
        ("page.replaced", Locale::PtBr) => "versão substituída",
        ("page.replaced", Locale::EnUs) => "replaced version",
        ("page.wave.heading", Locale::PtBr) => "Onda {n}",
        ("page.wave.heading", Locale::EnUs) => "Wave {n}",
        ("page.conversation.summary", Locale::PtBr) => "{count} registros",
        ("page.conversation.summary", Locale::EnUs) => "{count} entries",

        ("page.type.message", Locale::PtBr) => "mensagem",
        ("page.type.message", Locale::EnUs) => "message",
        ("page.type.response", Locale::PtBr) => "resposta",
        ("page.type.response", Locale::EnUs) => "reply",
        ("page.type.injection", Locale::PtBr) => "injeção",
        ("page.type.injection", Locale::EnUs) => "injection",
        ("page.type.hook", Locale::PtBr) => "gancho",
        ("page.type.hook", Locale::EnUs) => "hook",
        ("page.type.call", Locale::PtBr) => "chamada",
        ("page.type.call", Locale::EnUs) => "call",
        ("page.type.state", Locale::PtBr) => "estado",
        ("page.type.state", Locale::EnUs) => "state",
        ("page.type.publish", Locale::PtBr) => "publicação",
        ("page.type.publish", Locale::EnUs) => "publish",
        ("page.type.work_type", Locale::PtBr) => "tipo de trabalho",
        ("page.type.work_type", Locale::EnUs) => "work type",
        ("page.type.point", Locale::PtBr) => "ponto",
        ("page.type.point", Locale::EnUs) => "point",
        ("page.type.rule", Locale::PtBr) => "regra",
        ("page.type.rule", Locale::EnUs) => "rule",
        ("page.type.limit", Locale::PtBr) => "limite",
        ("page.type.limit", Locale::EnUs) => "limit",
        ("page.type.contract", Locale::PtBr) => "contrato",
        ("page.type.contract", Locale::EnUs) => "contract",
        ("page.type.error", Locale::PtBr) => "erro",
        ("page.type.error", Locale::EnUs) => "error",
        ("page.type.edge_case", Locale::PtBr) => "caso de borda",
        ("page.type.edge_case", Locale::EnUs) => "edge case",
        ("page.type.out_of_scope", Locale::PtBr) => "fora do escopo",
        ("page.type.out_of_scope", Locale::EnUs) => "out of scope",
        ("page.type.decision", Locale::PtBr) => "decisão",
        ("page.type.decision", Locale::EnUs) => "decision",
        ("page.type.context", Locale::PtBr) => "contexto",
        ("page.type.context", Locale::EnUs) => "context",
        ("page.type.concern", Locale::PtBr) => "preocupação",
        ("page.type.concern", Locale::EnUs) => "concern",
        ("page.type.criterion", Locale::PtBr) => "critério",
        ("page.type.criterion", Locale::EnUs) => "criterion",
        ("page.type.criterion_run", Locale::PtBr) => "execução de critério",
        ("page.type.criterion_run", Locale::EnUs) => "criterion run",
        ("page.type.wave", Locale::PtBr) => "onda",
        ("page.type.wave", Locale::EnUs) => "wave",
        ("page.type.task", Locale::PtBr) => "tarefa",
        ("page.type.task", Locale::EnUs) => "task",
        ("page.type.skill", _) => "skill",
        ("page.type.send", Locale::PtBr) => "envio",
        ("page.type.send", Locale::EnUs) => "send",
        ("page.type.delivered", Locale::PtBr) => "entregou",
        ("page.type.delivered", Locale::EnUs) => "delivered",
        ("page.type.verdict", Locale::PtBr) => "veredito",
        ("page.type.verdict", Locale::EnUs) => "verdict",
        ("page.type.commit", _) => "commit",
        ("page.type.pr_summary", Locale::PtBr) => "resumo do pull request",
        ("page.type.pr_summary", Locale::EnUs) => "pull request summary",
        ("page.type.request", Locale::PtBr) => "pedido",
        ("page.type.request", Locale::EnUs) => "request",
        ("page.type.deferred", Locale::PtBr) => "pedido adiado",
        ("page.type.deferred", Locale::EnUs) => "deferred request",
        ("page.type.note", Locale::PtBr) => "anotação",
        ("page.type.note", Locale::EnUs) => "note",
        ("page.type.remove", Locale::PtBr) => "remoção",
        ("page.type.remove", Locale::EnUs) => "removal",
        ("page.type.purge", Locale::PtBr) => "expurgo",
        ("page.type.purge", Locale::EnUs) => "purge",

        ("page.group.work_type", Locale::PtBr) => "Tipo de trabalho",
        ("page.group.work_type", Locale::EnUs) => "Work type",
        ("page.group.point", Locale::PtBr) => "Pontos do levantamento",
        ("page.group.point", Locale::EnUs) => "Survey points",
        ("page.group.rule", Locale::PtBr) => "Regras",
        ("page.group.rule", Locale::EnUs) => "Rules",
        ("page.group.limit", Locale::PtBr) => "Limites",
        ("page.group.limit", Locale::EnUs) => "Limits",
        ("page.group.contract", Locale::PtBr) => "Contratos",
        ("page.group.contract", Locale::EnUs) => "Contracts",
        ("page.group.error", Locale::PtBr) => "Erros e mensagens",
        ("page.group.error", Locale::EnUs) => "Errors and messages",
        ("page.group.edge_case", Locale::PtBr) => "Casos de borda",
        ("page.group.edge_case", Locale::EnUs) => "Edge cases",
        ("page.group.out_of_scope", Locale::PtBr) => "Fora do escopo",
        ("page.group.out_of_scope", Locale::EnUs) => "Out of scope",
        ("page.group.decision", Locale::PtBr) => "Decisões",
        ("page.group.decision", Locale::EnUs) => "Decisions",
        ("page.group.context", Locale::PtBr) => "Contexto",
        ("page.group.context", Locale::EnUs) => "Context",
        ("page.group.concern", Locale::PtBr) => "Preocupações",
        ("page.group.concern", Locale::EnUs) => "Concerns",
        ("page.group.criterion_run", Locale::PtBr) => "Execuções",
        ("page.group.criterion_run", Locale::EnUs) => "Runs",
        ("page.group.skill", _) => "Skills",

        ("page.field.reply_to", Locale::PtBr) => "Responde a",
        ("page.field.reply_to", Locale::EnUs) => "Replies to",
        ("page.field.hook", Locale::PtBr) => "Gancho",
        ("page.field.hook", Locale::EnUs) => "Hook",
        ("page.field.chars", Locale::PtBr) => "Caracteres",
        ("page.field.chars", Locale::EnUs) => "Characters",
        ("page.field.action", Locale::PtBr) => "Ação",
        ("page.field.action", Locale::EnUs) => "Action",
        ("page.field.tool", Locale::PtBr) => "Ferramenta",
        ("page.field.tool", Locale::EnUs) => "Tool",
        ("page.field.reason", Locale::PtBr) => "Motivo",
        ("page.field.reason", Locale::EnUs) => "Reason",
        ("page.field.command", Locale::PtBr) => "Comando",
        ("page.field.command", Locale::EnUs) => "Command",
        ("page.field.ms", Locale::PtBr) => "Tempo (ms)",
        ("page.field.ms", Locale::EnUs) => "Time (ms)",
        ("page.field.result", Locale::PtBr) => "Resultado",
        ("page.field.result", Locale::EnUs) => "Result",
        ("page.field.refusal", Locale::PtBr) => "Recusa",
        ("page.field.refusal", Locale::EnUs) => "Refusal",
        ("page.field.phase", Locale::PtBr) => "Fase",
        ("page.field.phase", Locale::EnUs) => "Phase",
        ("page.field.branch", _) => "Branch",
        ("page.field.base", _) => "Base",
        ("page.field.witness", Locale::PtBr) => "Pergunta e resposta",
        ("page.field.witness", Locale::EnUs) => "Question and answer",
        ("page.field.pr", _) => "Pull request",
        ("page.field.page", Locale::PtBr) => "Página",
        ("page.field.page", Locale::EnUs) => "Page",
        ("page.field.milestone", Locale::PtBr) => "Marco",
        ("page.field.milestone", Locale::EnUs) => "Milestone",
        ("page.field.ok", Locale::PtBr) => "Deu certo",
        ("page.field.ok", Locale::EnUs) => "Succeeded",
        ("page.field.url", Locale::PtBr) => "Endereço",
        ("page.field.url", Locale::EnUs) => "Address",
        ("page.field.kinds", Locale::PtBr) => "Tipos",
        ("page.field.kinds", Locale::EnUs) => "Kinds",
        ("page.field.block", Locale::PtBr) => "Grupo de lacunas",
        ("page.field.block", Locale::EnUs) => "Gap group",
        ("page.field.gap", Locale::PtBr) => "Lacuna",
        ("page.field.gap", Locale::EnUs) => "Gap",
        ("page.field.from", Locale::PtBr) => "De onde veio",
        ("page.field.from", Locale::EnUs) => "Came from",
        ("page.field.status", Locale::PtBr) => "Situação",
        ("page.field.status", Locale::EnUs) => "Status",
        ("page.field.facts", Locale::PtBr) => "Fatos",
        ("page.field.facts", Locale::EnUs) => "Facts",
        ("page.field.closes", Locale::PtBr) => "Fecha",
        ("page.field.closes", Locale::EnUs) => "Closes",
        ("page.field.reminders", Locale::PtBr) => "Lembretes",
        ("page.field.reminders", Locale::EnUs) => "Reminders",
        ("page.field.example", Locale::PtBr) => "Exemplo",
        ("page.field.example", Locale::EnUs) => "Example",
        ("page.field.applies_to", Locale::PtBr) => "Vale para",
        ("page.field.applies_to", Locale::EnUs) => "Applies to",
        ("page.field.value", Locale::PtBr) => "Valor",
        ("page.field.value", Locale::EnUs) => "Value",
        ("page.field.message", Locale::PtBr) => "Mensagem",
        ("page.field.message", Locale::EnUs) => "Message",
        ("page.field.expected", Locale::PtBr) => "O que acontece",
        ("page.field.expected", Locale::EnUs) => "What happens",
        ("page.field.why", Locale::PtBr) => "Por quê",
        ("page.field.why", Locale::EnUs) => "Why",
        ("page.field.when", Locale::PtBr) => "Quando",
        ("page.field.when", Locale::EnUs) => "When",
        ("page.field.then", Locale::PtBr) => "Então",
        ("page.field.then", Locale::EnUs) => "Then",
        ("page.field.proof", Locale::PtBr) => "Prova",
        ("page.field.proof", Locale::EnUs) => "Proof",
        ("page.field.contracts", Locale::PtBr) => "Contratos",
        ("page.field.contracts", Locale::EnUs) => "Contracts",
        ("page.field.criterion", Locale::PtBr) => "Critério",
        ("page.field.criterion", Locale::EnUs) => "Criterion",
        ("page.field.exit", Locale::PtBr) => "Código de saída",
        ("page.field.exit", Locale::EnUs) => "Exit code",
        ("page.field.output", Locale::PtBr) => "Saída",
        ("page.field.output", Locale::EnUs) => "Output",
        ("page.field.n", Locale::PtBr) => "Número",
        ("page.field.n", Locale::EnUs) => "Number",
        ("page.field.criteria", Locale::PtBr) => "Critérios",
        ("page.field.criteria", Locale::EnUs) => "Criteria",
        ("page.field.done_when", Locale::PtBr) => "Pronta quando",
        ("page.field.done_when", Locale::EnUs) => "Done when",
        ("page.field.depends_on", Locale::PtBr) => "Depende das ondas",
        ("page.field.depends_on", Locale::EnUs) => "Depends on waves",
        ("page.field.wave", Locale::PtBr) => "Onda",
        ("page.field.wave", Locale::EnUs) => "Wave",
        ("page.field.files", Locale::PtBr) => "Arquivos",
        ("page.field.files", Locale::EnUs) => "Files",
        ("page.field.skill", _) => "Skill",
        ("page.field.covers", Locale::PtBr) => "Cobre",
        ("page.field.covers", Locale::EnUs) => "Covers",
        ("page.field.must_read", Locale::PtBr) => "Precisa ler",
        ("page.field.must_read", Locale::EnUs) => "Must read",
        ("page.field.name", Locale::PtBr) => "Nome",
        ("page.field.name", Locale::EnUs) => "Name",
        ("page.field.sha", Locale::PtBr) => "Identificador",
        ("page.field.sha", Locale::EnUs) => "Identifier",
        ("page.field.examples", Locale::PtBr) => "Exemplos",
        ("page.field.examples", Locale::EnUs) => "Examples",
        ("page.field.role", Locale::PtBr) => "Papel",
        ("page.field.role", Locale::EnUs) => "Role",
        ("page.field.lines", Locale::PtBr) => "Linhas",
        ("page.field.lines", Locale::EnUs) => "Lines",
        ("page.field.items", Locale::PtBr) => "Itens enviados",
        ("page.field.items", Locale::EnUs) => "Items sent",
        ("page.field.mustard", Locale::PtBr) => "Versão do Mustard",
        ("page.field.mustard", Locale::EnUs) => "Mustard version",
        ("page.field.lessons", Locale::PtBr) => "Lições",
        ("page.field.lessons", Locale::EnUs) => "Lessons",
        ("page.field.skills", _) => "Skills",
        ("page.field.title", Locale::PtBr) => "Título",
        ("page.field.title", Locale::EnUs) => "Title",
        ("page.field.waves", Locale::PtBr) => "Ondas",
        ("page.field.waves", Locale::EnUs) => "Waves",
        ("page.field.repo", Locale::PtBr) => "Repositório",
        ("page.field.repo", Locale::EnUs) => "Repository",
        ("page.field.effect", Locale::PtBr) => "Efeito",
        ("page.field.effect", Locale::EnUs) => "Effect",
        ("page.field.pending", Locale::PtBr) => "Pendência",
        ("page.field.pending", Locale::EnUs) => "Pending item",
        ("page.field.targets", Locale::PtBr) => "Itens",
        ("page.field.targets", Locale::EnUs) => "Items",
        ("page.field.filter", Locale::PtBr) => "Filtro",
        ("page.field.filter", Locale::EnUs) => "Filter",
        ("page.field.origin", Locale::PtBr) => "Origem",
        ("page.field.origin", Locale::EnUs) => "Origin",
        ("page.field.label", Locale::PtBr) => "Rótulo no rascunho",
        ("page.field.label", Locale::EnUs) => "Draft label",
        ("page.field.last_run", Locale::PtBr) => "Última execução",
        ("page.field.last_run", Locale::EnUs) => "Last run",
        ("page.field.wave_state", Locale::PtBr) => "Estado da onda",
        ("page.field.wave_state", Locale::EnUs) => "Wave state",

        ("page.value.warn", Locale::PtBr) => "aviso",
        ("page.value.warn", Locale::EnUs) => "warning",
        ("page.value.block", Locale::PtBr) => "bloqueio",
        ("page.value.block", Locale::EnUs) => "block",
        ("page.value.ok", _) => "ok",
        ("page.value.refused", Locale::PtBr) => "recusada",
        ("page.value.refused", Locale::EnUs) => "refused",
        ("page.value.spec", _) => "spec",
        ("page.value.approval", Locale::PtBr) => "aprovação",
        ("page.value.approval", Locale::EnUs) => "approval",
        ("page.value.round", Locale::PtBr) => "rodada",
        ("page.value.round", Locale::EnUs) => "round",
        ("page.value.close", Locale::PtBr) => "fechamento",
        ("page.value.close", Locale::EnUs) => "close",
        ("page.value.feature", Locale::PtBr) => "funcionalidade",
        ("page.value.feature", Locale::EnUs) => "feature",
        ("page.value.fix", Locale::PtBr) => "correção",
        ("page.value.fix", Locale::EnUs) => "fix",
        ("page.value.refactor", Locale::PtBr) => "refatoração",
        ("page.value.refactor", Locale::EnUs) => "refactor",
        ("page.value.gap", Locale::PtBr) => "lacuna",
        ("page.value.gap", Locale::EnUs) => "gap",
        ("page.value.lesson", Locale::PtBr) => "lição",
        ("page.value.lesson", Locale::EnUs) => "lesson",
        ("page.value.prior_spec", Locale::PtBr) => "spec anterior",
        ("page.value.prior_spec", Locale::EnUs) => "prior spec",
        ("page.value.code_conflict", Locale::PtBr) => "conflito no código",
        ("page.value.code_conflict", Locale::EnUs) => "code conflict",
        ("page.value.outside_review", Locale::PtBr) => "revisor de fora",
        ("page.value.outside_review", Locale::EnUs) => "outside review",
        ("page.value.open", Locale::PtBr) => "pendente",
        ("page.value.open", Locale::EnUs) => "open",
        ("page.value.closed", Locale::PtBr) => "✓ fechado",
        ("page.value.closed", Locale::EnUs) => "✓ closed",
        ("page.value.not_applicable", Locale::PtBr) => "não se aplica",
        ("page.value.not_applicable", Locale::EnUs) => "not applicable",
        ("page.value.pass", Locale::PtBr) => "passou",
        ("page.value.pass", Locale::EnUs) => "passed",
        ("page.value.fail", Locale::PtBr) => "falhou",
        ("page.value.fail", Locale::EnUs) => "failed",
        ("page.value.create", Locale::PtBr) => "criação",
        ("page.value.create", Locale::EnUs) => "created",
        ("page.value.change", Locale::PtBr) => "mudança",
        ("page.value.change", Locale::EnUs) => "changed",
        ("page.value.drop", Locale::PtBr) => "remoção",
        ("page.value.drop", Locale::EnUs) => "dropped",
        ("page.value.wave", Locale::PtBr) => "agente de onda",
        ("page.value.wave", Locale::EnUs) => "wave agent",
        ("page.value.review", Locale::PtBr) => "revisor",
        ("page.value.review", Locale::EnUs) => "reviewer",
        ("page.value.skill", Locale::PtBr) => "autor de skill",
        ("page.value.skill", Locale::EnUs) => "skill author",
        ("page.value.approved", Locale::PtBr) => "aprovada",
        ("page.value.approved", Locale::EnUs) => "approved",
        ("page.value.rejected", Locale::PtBr) => "reprovada",
        ("page.value.rejected", Locale::EnUs) => "rejected",
        ("page.value.new_waves", Locale::PtBr) => "ondas novas",
        ("page.value.new_waves", Locale::EnUs) => "new waves",
        ("page.value.adjust_waves", Locale::PtBr) => "ajusta as ondas",
        ("page.value.adjust_waves", Locale::EnUs) => "adjusts the waves",
        ("page.value.secret", Locale::PtBr) => "segredo",
        ("page.value.secret", Locale::EnUs) => "secret",
        ("page.value.client_data", Locale::PtBr) => "dado de cliente",
        ("page.value.client_data", Locale::EnUs) => "client data",
        ("page.value.yes", Locale::PtBr) => "sim",
        ("page.value.yes", Locale::EnUs) => "yes",
        ("page.value.no", Locale::PtBr) => "não",
        ("page.value.no", Locale::EnUs) => "no",
        ("page.value.new", Locale::PtBr) => "novo",
        ("page.value.new", Locale::EnUs) => "new",
        ("page.value.tests_rule", Locale::PtBr) => "confere a regra",
        ("page.value.tests_rule", Locale::EnUs) => "tests the rule",
        ("page.value.not_tests_rule", Locale::PtBr) => "não confere a regra",
        ("page.value.not_tests_rule", Locale::EnUs) => "does not test the rule",
        ("page.value.repeated", Locale::PtBr) => "repetiu",
        ("page.value.repeated", Locale::EnUs) => "repeated",
        ("page.value.not_repeated", Locale::PtBr) => "não repetiu",
        ("page.value.not_repeated", Locale::EnUs) => "did not repeat",
        ("page.value.wave_todo", Locale::PtBr) => "a fazer",
        ("page.value.wave_todo", Locale::EnUs) => "to do",
        ("page.value.wave_running", Locale::PtBr) => "em execução",
        ("page.value.wave_running", Locale::EnUs) => "running",
        ("page.value.wave_done", Locale::PtBr) => "pronta",
        ("page.value.wave_done", Locale::EnUs) => "done",
        ("page.value.wave_reviewed", Locale::PtBr) => "revisada",
        ("page.value.wave_reviewed", Locale::EnUs) => "reviewed",

        ("page.phase.survey", Locale::PtBr) => "levantamento",
        ("page.phase.survey", Locale::EnUs) => "survey",
        ("page.phase.plan", Locale::PtBr) => "plano",
        ("page.phase.plan", Locale::EnUs) => "plan",
        ("page.phase.approved", Locale::PtBr) => "aprovada",
        ("page.phase.approved", Locale::EnUs) => "approved",
        ("page.phase.running", Locale::PtBr) => "em execução",
        ("page.phase.running", Locale::EnUs) => "running",
        ("page.phase.closed", Locale::PtBr) => "fechada",
        ("page.phase.closed", Locale::EnUs) => "closed",
        ("page.phase.pr_open", Locale::PtBr) => "pull request aberto",
        ("page.phase.pr_open", Locale::EnUs) => "pull request open",
        ("page.phase.delivered", Locale::PtBr) => "entregue",
        ("page.phase.delivered", Locale::EnUs) => "delivered",
        ("page.phase.discarded", Locale::PtBr) => "descartada",
        ("page.phase.discarded", Locale::EnUs) => "discarded",

        ("page.author.user", Locale::PtBr) => "usuário",
        ("page.author.user", Locale::EnUs) => "user",
        ("page.author.assistant", Locale::PtBr) => "assistente",
        ("page.author.assistant", Locale::EnUs) => "assistant",
        ("page.author.hook", Locale::PtBr) => "gancho",
        ("page.author.hook", Locale::EnUs) => "hook",
        ("page.author.binary", Locale::PtBr) => "binário",
        ("page.author.binary", Locale::EnUs) => "binary",
        ("page.author.wave", Locale::PtBr) => "agente de onda",
        ("page.author.wave", Locale::EnUs) => "wave agent",
        ("page.author.review", Locale::PtBr) => "revisor",
        ("page.author.review", Locale::EnUs) => "reviewer",
        ("page.author.skill", Locale::PtBr) => "autor de skill",
        ("page.author.skill", Locale::EnUs) => "skill author",

        ("page.metrics.col.measure", Locale::PtBr) => "Medida",
        ("page.metrics.col.measure", Locale::EnUs) => "Measure",
        ("page.metrics.col.value", Locale::PtBr) => "Valor",
        ("page.metrics.col.value", Locale::EnUs) => "Value",
        ("page.metrics.calls", Locale::PtBr) => "Comandos do Mustard",
        ("page.metrics.calls", Locale::EnUs) => "Mustard commands",
        ("page.metrics.calls.value", Locale::PtBr) => "{count} chamadas, {refused} recusadas",
        ("page.metrics.calls.value", Locale::EnUs) => "{count} calls, {refused} refused",
        ("page.metrics.hooks", Locale::PtBr) => "Ganchos",
        ("page.metrics.hooks", Locale::EnUs) => "Hooks",
        ("page.metrics.hooks.value", Locale::PtBr) => "{blocks} bloqueios, {warns} avisos",
        ("page.metrics.hooks.value", Locale::EnUs) => "{blocks} blocks, {warns} warnings",
        ("page.metrics.injected", Locale::PtBr) => "Texto colocado pelos ganchos",
        ("page.metrics.injected", Locale::EnUs) => "Text added by hooks",
        ("page.metrics.injected.value", Locale::PtBr) => "{chars} caracteres, cerca de {tokens} tokens",
        ("page.metrics.injected.value", Locale::EnUs) => "{chars} characters, about {tokens} tokens",
        ("page.metrics.sends", Locale::PtBr) => "Pedidos enviados aos agentes",
        ("page.metrics.sends", Locale::EnUs) => "Requests sent to agents",
        ("page.metrics.sends.value", Locale::PtBr) => "{count}, o maior com {lines} linhas",
        ("page.metrics.sends.value", Locale::EnUs) => "{count}, the largest with {lines} lines",
        ("page.metrics.verdicts", Locale::PtBr) => "Revisões",
        ("page.metrics.verdicts", Locale::EnUs) => "Reviews",
        ("page.metrics.verdicts.value", Locale::PtBr) => "{approved} aprovadas, {rejected} reprovadas",
        ("page.metrics.verdicts.value", Locale::EnUs) => "{approved} approved, {rejected} rejected",
        ("page.metrics.points", Locale::PtBr) => "Pontos do levantamento",
        ("page.metrics.points", Locale::EnUs) => "Survey points",
        ("page.metrics.points.value", Locale::PtBr) => "{open} pendentes, {closed} fechados",
        ("page.metrics.points.value", Locale::EnUs) => "{open} open, {closed} closed",

        ("page.project.specs", _) => "Specs",
        ("page.project.col.spec", _) => "Spec",
        ("page.project.col.phase", Locale::PtBr) => "Fase",
        ("page.project.col.phase", Locale::EnUs) => "Phase",
        ("page.project.col.page", Locale::PtBr) => "Página",
        ("page.project.col.page", Locale::EnUs) => "Page",
        ("page.project.open", Locale::PtBr) => "abrir",
        ("page.project.open", Locale::EnUs) => "open",
        ("page.project.none", Locale::PtBr) => "sem página",
        ("page.project.none", Locale::EnUs) => "no page",
        ("page.project.index", Locale::PtBr) => "Índice das specs: {path}",
        ("page.project.index", Locale::EnUs) => "Spec index: {path}",

        // As recusas do comando `page`.
        ("page.missing_body", Locale::PtBr) => {
            "Diga o que gerar: --spec <nome> para a página de uma spec, ou --body \
             <arquivo.md> com --out <página.html> para uma página avulsa."
        }
        ("page.missing_body", Locale::EnUs) => {
            "Say what to build: --spec <name> for a spec's page, or --body <file.md> \
             with --out <page.html> for a standalone page."
        }
        ("page.unreadable_body", Locale::PtBr) => {
            "Não consegui ler {path}. Passe em --body um arquivo de texto UTF-8 com o \
             markdown da página."
        }
        ("page.unreadable_body", Locale::EnUs) => {
            "Could not read {path}. Pass --body a readable UTF-8 file holding the \
             page's markdown."
        }
        ("page.empty_title", Locale::PtBr) => {
            "A página não tem título. Passe --title ou comece o markdown com uma linha \
             \"# Título\"."
        }
        ("page.empty_title", Locale::EnUs) => {
            "The page has no title. Pass --title or start the markdown with a \
             \"# Title\" line."
        }
        ("page.write_failed", Locale::PtBr) => "Não consegui gravar {path}: {detail}.",
        ("page.write_failed", Locale::EnUs) => "Could not write {path}: {detail}.",

        // Fail-open: unknown key returns the key itself so callers always have
        // *something* to render. This is what `karpathy-guidelines` calls a
        // "safe default" — never panic on a typo in a hook.
        _ => key_as_static(key),
    }
}

/// Promote a `&str` to `&'static str` *only* for the fail-open path of
/// [`translate`]. Returns the well-known literal `<missing-key>` so we never
/// leak arbitrary unbounded `&str` into a static slot.
#[must_use]
fn key_as_static(_key: &str) -> &'static str {
    "<missing-key>"
}

/// Slugify `text` to a kebab-case identifier, lang-aware.
///
/// PT locale strips Latin diacritics (`ç → c`, `ã → a`, …) before kebab-casing
/// so spec slugs round-trip cleanly. EN locale keeps the input as-is (no
/// Unicode normalisation): accents are removed only in PT.
/// Stopword lists differ per locale (basic articles/prepositions are dropped).
///
/// The output never contains leading/trailing dashes and never collapses to an
/// empty string — fully non-alphanumeric input degrades to `"x"`, mirroring
/// the existing `apps/rt/src/run/scan/interpret.rs::slugify` contract.
#[must_use]
pub fn slugify(text: &str, lang: Locale) -> String {
    let normalised = match lang {
        Locale::PtBr => crate::domain::text::fold_accents(text),
        Locale::EnUs => text.to_string(),
    };
    let stopwords: &[&str] = match lang {
        Locale::PtBr => crate::domain::text::SLUG_STOPWORDS_PT,
        Locale::EnUs => crate::domain::text::SLUG_STOPWORDS_EN,
    };
    // 1. lowercase + split on non-alphanumeric.
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in normalised.chars() {
        let lc = ch.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() {
            cur.push(lc);
        } else if !cur.is_empty() {
            tokens.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    // 2. drop stopwords — but only when at least one token in the input is
    //    longer than a single character. Pure single-char inputs (e.g.
    //    `"ç ã õ"`) and pure-stopword inputs keep every token so callers
    //    always get *something* slug-shaped back. After filtering, if nothing
    //    is left, fall back to the original tokens.
    let has_long = tokens.iter().any(|tok| tok.chars().count() > 1);
    let kept: Vec<String> = if has_long {
        let filtered: Vec<String> = tokens
            .iter()
            .filter(|tok| !stopwords.contains(&tok.as_str()))
            .cloned()
            .collect();
        if filtered.is_empty() { tokens } else { filtered }
    } else {
        tokens
    };
    let joined = kept.join("-");
    if joined.is_empty() {
        "x".to_string()
    } else {
        joined
    }
}

// ---------------------------------------------------------------------------
// Type aliases — `SupportedLocale` (catalogue) + `UserLocale` (open BCP-47)
// ---------------------------------------------------------------------------

/// Catalogue-backed locale — the closed set Mustard ships translations for.
///
/// `SupportedLocale` is a type alias for the original [`Locale`] enum, so each
/// callsite could move to the new name without breaking every consumer at once.
pub type SupportedLocale = Locale;

/// User-declared BCP-47 locale, as a spec records it.
///
/// Unlike [`SupportedLocale`] (closed, two variants), `UserLocale` accepts any
/// syntactically valid BCP-47 code so users can write specs in `fr-FR`, `de-DE`,
/// etc. Parse the raw tag into a [`SupportedLocale`] when a banner needs to
/// render, falling back to the default when the locale is not in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserLocale {
    /// The raw BCP-47 tag as supplied by the user.
    pub raw: String,
}

impl UserLocale {
    /// Construct a `UserLocale` from a BCP-47 string.  No validation is
    /// performed — any non-empty string is accepted so fail-open callers never
    /// have to handle an error for syntactically arbitrary user input.
    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }
}

impl fmt::Display for UserLocale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for UserLocale {
    type Err = UserLocaleError;

    /// Parse a BCP-47 string into a `UserLocale`. Rejects empty strings and
    /// shapes that are not `<lang>-<REGION>` (2-3 lowercase letters, hyphen,
    /// 2 uppercase letters). Short forms like `pt`/`en` and unhyphenated
    /// blobs like `ptbr` are rejected so callers can rely on a canonical tag.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(UserLocaleError::Empty);
        }
        let (lang, region) = trimmed
            .split_once('-')
            .ok_or_else(|| UserLocaleError::Malformed(trimmed.to_string()))?;
        let lang_ok = (2..=3).contains(&lang.len())
            && lang.chars().all(|c| c.is_ascii_lowercase());
        let region_ok = region.len() == 2 && region.chars().all(|c| c.is_ascii_uppercase());
        if !lang_ok || !region_ok {
            return Err(UserLocaleError::Malformed(trimmed.to_string()));
        }
        Ok(Self { raw: trimmed.to_string() })
    }
}

/// Errors returned by [`UserLocale::from_str`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserLocaleError {
    /// Empty or whitespace-only input.
    Empty,
    /// Input does not match the `<lang>-<REGION>` shape.
    Malformed(String),
}

impl fmt::Display for UserLocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("locale string is empty"),
            Self::Malformed(raw) => write!(f, "locale `{raw}` is not BCP-47 `<lang>-<REGION>`"),
        }
    }
}

impl std::error::Error for UserLocaleError {}

/// Render a wave label given a locale + 1-based wave index.
///
/// `Locale::PtBr` → `"Onda 3"`, `Locale::EnUs` → `"W3"`. Reused by the rt
/// dispatch layer and the dashboard banners so the format stays in sync.
#[must_use]
pub fn wave_label(n: u32, lang: Locale) -> String {
    match lang {
        Locale::PtBr => format!("{} {n}", translate("wave.label", lang)),
        // EN uses the compact `W3` form — no separating space.
        Locale::EnUs => format!("{}{n}", translate("wave.label", lang)),
    }
}

// ---------------------------------------------------------------------------
// File-operation markers (`## Files` bullet annotations)
// ---------------------------------------------------------------------------

/// Every catalogue locale, EN canonical first — the iteration order of
/// [`file_marker_synonyms`], so the EN spelling is always `synonyms[0]`.
const CATALOGUE_LOCALES: &[Locale] = &[Locale::EnUs, Locale::PtBr];

/// A file-operation marker recognised in a spec's `## Files` bullet lines —
/// e.g. ``- `src/Payable.cs` (create)``. `Create` declares a net-new file
/// (validators must not flag it as missing); `Edit` declares a change to an
/// existing file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileMarker {
    /// Net-new file — `(create)` / `(new)` / `(novo)` / `(criar)`.
    Create,
    /// Existing file to change — `(edit)` / `(editar)`.
    Edit,
}

impl FileMarker {
    /// Catalogue key carrying this marker's per-locale synonyms.
    fn catalogue_key(self) -> &'static str {
        match self {
            Self::Create => "marker.create",
            Self::Edit => "marker.edit",
        }
    }
}

/// Every accepted spelling of `marker`, across ALL catalogue locales, deduped,
/// EN canonical first (`(create)` for [`FileMarker::Create`]). The synonyms
/// are data in the [`translate`] catalogue (`marker.*` keys, `|`-separated per
/// locale) — the SINGLE origin shared by the drafter and every validator
/// (`analyze-validation`, scope-classify), so a localized marker like the
/// pt-BR `(novo)` can never drift out of recognition.
///
/// Spellings are lowercase literals including the surrounding parentheses;
/// match with [`line_has_file_marker`] (case-insensitive `contains`).
#[must_use]
pub fn file_marker_synonyms(marker: FileMarker) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for lang in CATALOGUE_LOCALES {
        for syn in translate(marker.catalogue_key(), *lang).split('|') {
            let syn = syn.trim();
            if !syn.is_empty() && !out.contains(&syn) {
                out.push(syn);
            }
        }
    }
    out
}

/// Whether `line` carries `marker` in ANY of its accepted spellings
/// (case-insensitive substring, like the historical `(create)` check).
/// Fail-open helper for `## Files` bullet validation: a line such as
/// ``- `src/Payable.cs` (novo)`` matches [`FileMarker::Create`].
#[must_use]
pub fn line_has_file_marker(line: &str, marker: FileMarker) -> bool {
    let lower = line.to_lowercase();
    file_marker_synonyms(marker).iter().any(|syn| lower.contains(syn))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Short forms are rejected with a typed error.
    #[test]
    fn i18n_rejects_short_form() {
        assert_eq!(
            Locale::from_str("pt").unwrap_err(),
            LocaleError::ShortForm("pt".to_string())
        );
        assert_eq!(
            Locale::from_str("en").unwrap_err(),
            LocaleError::ShortForm("en".to_string())
        );
        // Trim + case-insensitive still rejects.
        assert_eq!(
            Locale::from_str("  PT ").unwrap_err(),
            LocaleError::ShortForm("PT".to_string())
        );
    }

    #[test]
    fn locale_parses_bcp47() {
        assert_eq!(Locale::from_str("pt-BR").unwrap(), Locale::PtBr);
        assert_eq!(Locale::from_str("en-US").unwrap(), Locale::EnUs);
        // Case-insensitive on the region tag.
        assert_eq!(Locale::from_str("PT-br").unwrap(), Locale::PtBr);
        assert_eq!(Locale::from_str("EN-US").unwrap(), Locale::EnUs);
        // Foreign / unsupported codes → Unknown, not ShortForm.
        assert!(matches!(
            Locale::from_str("es-MX").unwrap_err(),
            LocaleError::Unknown(_)
        ));
    }

    // Known keys translate to the canonical literals.
    #[test]
    fn i18n_translates_known_keys() {
        assert_eq!(
            translate("banner.close.success", Locale::PtBr),
            "Pipeline fechado com sucesso."
        );
        assert_eq!(
            translate("banner.close.success", Locale::EnUs),
            "Pipeline closed successfully."
        );
        assert_eq!(translate("wave.label", Locale::PtBr), "Onda");
        assert_eq!(translate("wave.label", Locale::EnUs), "W");
        assert_eq!(translate("ac.label", Locale::PtBr), "CA");
        assert_eq!(translate("ac.label", Locale::EnUs), "AC");
    }

    /// Work-unit surfacing copy is catalogue-driven in BOTH locales: the
    /// listing legend, the status-bar label and the session-start advisory
    /// carry no language literal at their surface.
    #[test]
    fn i18n_translates_work_unit_surfacing_keys() {
        for key in ["specs.location.remote_only", "statusline.prune.label", "prune.pending.notice"] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_ne!(translate(key, lang), "<missing-key>", "{key} missing for {lang}");
            }
            assert_ne!(
                translate(key, Locale::PtBr),
                translate(key, Locale::EnUs),
                "{key} must differ per locale (proof it is catalogue-driven)"
            );
        }
        // The advisory's slots are the caller's contract.
        for lang in [Locale::PtBr, Locale::EnUs] {
            let notice = translate("prune.pending.notice", lang);
            assert!(notice.contains("{count}"), "the advisory interpolates the count: {notice}");
            assert!(notice.contains("{branches}"), "and names the units: {notice}");
        }
    }

    /// Os avisos de pendência saem do catálogo nos dois idiomas, e cada um
    /// carrega as vagas que o chamador preenche. Os textos dos ganchos do fim
    /// da resposta que saíram (a entrega do resumo, o QA no `Stop`, o lembrete
    /// de gravar a conversa) e o aviso da mensagem seguinte saíram com eles.
    #[test]
    fn i18n_translates_doc_and_pending_keys() {
        for (key, slots) in [
            ("doc.section.flow", &[][..]),
            ("pending.notice", &["{count}", "{items}"][..]),
            ("pending.gate.block", &["{count}", "{items}"][..]),
            ("pending.duplicate", &["{id}", "{title}"][..]),
            ("scratch.residue.notice", &["{total}", "{count}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
        for key in [
            "deliver.order",
            "deliver.publish",
            "stopgate.block.reason",
            "crystallise.nudge",
            "clarity.next.head",
        ] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_eq!(translate(key, lang), "<missing-key>", "{key} left with its hook");
            }
        }
    }

    /// As recusas e os avisos do arquivo de eventos da spec saem do catálogo
    /// nos dois idiomas, cada um com as vagas que o chamador preenche.
    #[test]
    fn i18n_translates_spec_event_keys() {
        for (key, slots) in [
            ("spec_events.not_an_object", &["{detail}"][..]),
            ("spec_events.unknown_type", &["{type}", "{types}"][..]),
            ("spec_events.missing_field", &["{type}", "{field}"][..]),
            ("spec_events.invalid_value", &["{type}", "{field}", "{expected}"][..]),
            ("spec_events.wrong_count", &["{type}", "{field}", "{min}", "{max}", "{count}"][..]),
            ("spec_events.fact_without_source", &["{fact}"][..]),
            ("spec_events.cited_file_missing", &["{fact}", "{path}"][..]),
            ("spec_events.cited_line_missing", &["{fact}", "{path}", "{line}", "{lines}"][..]),
            ("spec_events.unknown_target", &["{id}"][..]),
            ("spec_events.unknown_code", &["{code}"][..]),
            ("spec_events.binary_only_field", &["{field}"][..]),
            ("spec_events.replaces_other_type", &["{id}", "{found}", "{type}"][..]),
            ("spec_events.filter_matches_nothing", &["{type}", "{from}", "{to}"][..]),
            ("spec_events.unknown_block", &["{block}", "{blocks}"][..]),
            ("spec_events.bad_spec_name", &["{spec}"][..]),
            ("spec_events.no_spec_file", &["{spec}"][..]),
            ("spec_events.io_failed", &["{detail}"][..]),
            ("spec_events.skipped_line", &["{line}"][..]),
            ("spec_events.duplicate_id", &["{line}", "{id}"][..]),
            ("spec_events.kind.text", &[][..]),
            ("spec_events.kind.int", &[][..]),
            ("spec_events.kind.bool", &[][..]),
            ("spec_events.kind.object", &[][..]),
            ("spec_events.kind.ints", &[][..]),
            ("spec_events.kind.texts", &[][..]),
            ("spec_events.kind.objects", &[][..]),
            ("spec_events.kind.list", &[][..]),
            ("spec_events.kind.one_of", &["{values}"][..]),
            ("spec_events.kind.many_of", &["{values}"][..]),
            ("spec_events.kind.text_or_object", &[][..]),
            ("spec_events.kind.time", &[][..]),
            ("spec_events.kind.ref", &[][..]),
            ("spec_events.kind.refs", &[][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }

    /// Os avisos do índice das specs e as recusas do banco de lições saem do
    /// catálogo nos dois idiomas, cada um com as vagas que o chamador
    /// preenche.
    #[test]
    fn i18n_translates_spec_index_and_lesson_keys() {
        for (key, slots) in [
            ("spec_index.write_warning", &["{detail}"][..]),
            ("spec_index.missing", &["{count}"][..]),
            ("spec_index.diverged", &["{count}", "{specs}"][..]),
            ("spec_index.stale_search", &["{count}"][..]),
            ("spec_index.no_specs", &[][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }

    /// Os defeitos de clareza, o bloqueio e o aviso do fim da resposta saem do
    /// catálogo nos dois idiomas, cada um com as vagas que o medidor preenche.
    #[test]
    fn i18n_translates_clarity_defect_keys() {
        for (key, slots) in [
            ("clarity.long_sentence", &["{words}", "{opening}"][..]),
            ("clarity.unexpanded_acronym", &["{acronym}"][..]),
            ("clarity.unexplained_term", &["{term}"][..]),
            ("clarity.internal_code", &["{code}"][..]),
            ("clarity.too_long", &["{lines}", "{limit}"][..]),
            ("clarity.hard_to_read", &["{score}", "{min}"][..]),
            ("clarity.wrong_language", &["{found}", "{expected}"][..]),
            ("clarity.block.head", &[][..]),
            ("clarity.note.head", &[][..]),
            ("clarity.more", &["{count}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }

    /// O resumo da spec em HTML tira todo o texto do catálogo, nos dois idiomas,
    /// e o próximo passo da execução carrega o número da onda.
    #[test]
    fn i18n_translates_spec_doc_keys() {
        for key in [
            "doc.section.where",
            "doc.step.plan.name",
            "doc.proof.red",
            "doc.next.approve",
            "doc.footer",
        ] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_ne!(translate(key, lang), "<missing-key>", "{key} missing for {lang}");
            }
            assert_ne!(
                translate(key, Locale::PtBr),
                translate(key, Locale::EnUs),
                "{key} must differ per locale (proof it is catalogue-driven)"
            );
        }
        for lang in [Locale::PtBr, Locale::EnUs] {
            assert!(translate("doc.next.execute", lang).contains("{wave}"));
            assert!(translate("doc.clarified.definition", lang).contains("{term}"));
        }
    }

    #[test]
    fn translate_unknown_key_is_failopen() {
        // Missing keys return a stable sentinel rather than panicking.
        assert_eq!(translate("banner.missing.xyz", Locale::PtBr), "<missing-key>");
        assert_eq!(translate("banner.missing.xyz", Locale::EnUs), "<missing-key>");
    }

    #[test]
    fn slugify_pt_strips_accents() {
        assert_eq!(slugify("Configuração do Idioma", Locale::PtBr), "configuracao-idioma");
        assert_eq!(slugify("São Paulo é grande", Locale::PtBr), "sao-paulo-grande");
        assert_eq!(slugify("ç ã õ", Locale::PtBr), "c-a-o");
    }

    #[test]
    fn slugify_pt_drops_em_a_contractions() {
        // `no` ("em o") is a stopword now: it must not eat a token slot and leave
        // a `...-erro-no` tail — the meaningful word (`nome`) survives instead.
        assert_eq!(slugify("erro no nome", Locale::PtBr), "erro-nome");
        assert_eq!(slugify("tratamento na base", Locale::PtBr), "tratamento-base");
        assert_eq!(slugify("volta ao topo", Locale::PtBr), "volta-topo");
    }

    #[test]
    fn slugify_en_keeps_input_keeps_no_accents() {
        // EN never had accents to strip in the first place; stopwords differ.
        assert_eq!(slugify("The Quick Brown Fox", Locale::EnUs), "quick-brown-fox");
        // PT stopwords are NOT applied in EN mode.
        assert_eq!(slugify("de para", Locale::EnUs), "de-para");
    }

    #[test]
    fn slugify_handles_empty_and_punctuation() {
        // Mirror the existing `interpret::slugify` floor — degrade to "x".
        assert_eq!(slugify("///", Locale::PtBr), "x");
        assert_eq!(slugify("", Locale::EnUs), "x");
        // A single-token input is preserved even if it would be a stopword,
        // so callers always get *something* slug-shaped back.
        assert_eq!(slugify("the", Locale::EnUs), "the");
    }

    #[test]
    fn i18n_render_is_the_translation() {
        let i = I18n::new(Locale::EnUs);
        assert_eq!(i.render("banner.close.success"), "Pipeline closed successfully.");
    }

    #[test]
    fn wave_label_formats_per_locale() {
        assert_eq!(wave_label(3, Locale::PtBr), "Onda 3");
        assert_eq!(wave_label(3, Locale::EnUs), "W3");
    }

    /// `heading.spec.ac` is the ONLY AC
    /// heading key — the byte-identical `heading.spec.ac_list` twin is gone
    /// (a second key for the same heading let the scaffold emit it twice).
    #[test]
    fn ac_heading_key_is_single() {
        assert_eq!(translate("heading.spec.ac", Locale::PtBr), "Critérios de Aceitação");
        assert_eq!(translate("heading.spec.ac", Locale::EnUs), "Acceptance Criteria");
        assert_eq!(translate("heading.spec.ac_list", Locale::PtBr), "<missing-key>");
        assert_eq!(translate("heading.spec.ac_list", Locale::EnUs), "<missing-key>");
        // `placeholder.see_below` retired with the same fix (dead copy).
        assert_eq!(translate("placeholder.see_below", Locale::PtBr), "<missing-key>");
    }

    #[test]
    fn file_marker_synonyms_merge_locales_en_canonical_first() {
        let create = file_marker_synonyms(FileMarker::Create);
        assert_eq!(create[0], "(create)", "EN canonical leads: {create:?}");
        for syn in ["(create)", "(new)", "(novo)", "(criar)"] {
            assert!(create.contains(&syn), "{syn} accepted: {create:?}");
        }
        let edit = file_marker_synonyms(FileMarker::Edit);
        assert_eq!(edit[0], "(edit)");
        assert!(edit.contains(&"(editar)"));
        // Deduped — no spelling twice.
        let mut sorted = create.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), create.len(), "no duplicates: {create:?}");
    }

    #[test]
    fn line_has_file_marker_matches_localized_and_case_insensitive() {
        assert!(line_has_file_marker("- `a.rs` (create)", FileMarker::Create));
        assert!(line_has_file_marker("- `a.rs` (novo)", FileMarker::Create));
        assert!(line_has_file_marker("- `a.rs` (criar)", FileMarker::Create));
        assert!(line_has_file_marker("- `a.rs` (NOVO)", FileMarker::Create));
        assert!(line_has_file_marker("- `a.rs` (editar)", FileMarker::Edit));
        // No marker / wrong marker → no match.
        assert!(!line_has_file_marker("- `a.rs`", FileMarker::Create));
        assert!(!line_has_file_marker("- `a.rs` (editar)", FileMarker::Create));
        // A prose parenthetical is not a marker.
        assert!(!line_has_file_marker("- `a.rs` (new format)", FileMarker::Create));
    }
}
