//! `i18n` — central language + tone module for Mustard banners.
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
//! - apply a tone (didactic / technical / concise) on top of a translation;
//! - slugify free-form text in a way that respects PT-vs-EN accent rules.
//!
//! Wave 4 of the `mustard-unification` mega-spec consolidates that into a
//! single boundary-typed module exported from `mustard_core`.
//!
//! ## Locale + tone vocabulary
//!
//! - [`Locale`] — BCP-47 typed locale. Only `pt-BR` and `en-US` are accepted;
//!   the legacy short forms `pt` / `en` are rejected with
//!   [`LocaleError::ShortForm`] (see memory `project_locale_codes`).
//! - [`Tone`] — `didactic` (expand abbreviations, prefer plain words),
//!   `technical` (keep abbreviations + jargon), `concise` (strip filler).
//! - [`I18n`] — the pair `{ lang, tone }` callers thread through banner
//!   rendering.
//!
//! ## Canonical banner keys
//!
//! Banners are keyed by dotted-namespace identifiers. Every key is documented
//! in [`translate`] and surfaced verbatim when the key is unknown (fail-open).
//! Known keys at Wave 4:
//!
//! - `banner.close.success` — "Pipeline closed successfully." (CLOSE phase)
//! - `banner.amend.drift` — drift-warning message body (see W4 spec).
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

/// Banner tone selector.
///
/// `Didactic` expands abbreviations on first use and prefers common words —
/// the default for user-facing chat output. `Technical` keeps jargon and
/// abbreviations as written. `Concise` strips parenthetical clarifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tone {
    /// Expand abbreviations, prefer plain words. The Mustard default.
    #[default]
    Didactic,
    /// Keep jargon + abbreviations as written.
    Technical,
    /// Strip filler / parentheticals.
    Concise,
}

impl Tone {
    /// Canonical lowercase string for `mustard.json#tone`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Didactic => "didactic",
            Self::Technical => "technical",
            Self::Concise => "concise",
        }
    }

    /// Parse a free-form tone string. Returns `None` for unknown values so
    /// callers fail open to [`Tone::Didactic`].
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "didactic" | "didatico" | "didático" => Some(Self::Didactic),
            "technical" | "tecnico" | "técnico" => Some(Self::Technical),
            "concise" | "conciso" => Some(Self::Concise),
            _ => None,
        }
    }
}

impl fmt::Display for Tone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Banner-rendering context: locale + tone.
///
/// Threaded through hook / CLI banner code so a single struct call replaces
/// the bilingual lookup tables that used to sit in each module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct I18n {
    /// User locale (drives [`translate`]).
    pub lang: Locale,
    /// User tone (drives [`apply_tone`]).
    pub tone: Tone,
}

impl I18n {
    /// Build an `I18n` from typed values.
    #[must_use]
    pub fn new(lang: Locale, tone: Tone) -> Self {
        Self { lang, tone }
    }

    /// Translate `key` and immediately apply the configured tone.
    #[must_use]
    pub fn render(&self, key: &str) -> String {
        apply_tone(translate(key, self.lang), self.tone)
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
        // developer and injected into the session, so they follow
        // `mustard.json#lang` (finding #1 of the 2026-07 SOLID audit) — unlike
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
        // `placeholder.see_below` was retired with the single-AC-heading fix
        // (TF 2026-06-10-ac-heading-unico): the AC PRD entry is no longer
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
        ("ac.safety.build_green", Locale::PtBr) => "o build e os testes do projeto passam verdes",
        ("ac.safety.build_green", Locale::EnUs) => "the project build and tests pass green",

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

        // Spec A v4 / W3 — wave _summary.md section headings.
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

        // Spec A v4 / W3 — wave _context.md section headings.
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

        // Spec A v4 / W4 — regression gate verdict labels + messages. These are
        // MACHINE / log strings (gate verdicts consumed by the orchestrator and
        // written to telemetry), so they are ENGLISH regardless of the user's
        // configured locale — only `gate.askuser.*` below stays config-lang.
        ("gate.verdict.green.label", _) => "Green",
        ("gate.verdict.amber.label", _) => "Amber",
        ("gate.verdict.red.label", _) => "Red",
        ("gate.verdict.green.message", _) => "No regression signals.",
        ("gate.verdict.amber.message", _) => "Ambiguous signals detected. Confirmation required.",
        ("gate.verdict.red.message", _) => "Regression detected. Consolidation blocked.",

        // Spec A v4 / W4 — gate signal layer labels (MACHINE / log, English
        // regardless of locale). Use the `{slot}` placeholders to let callers
        // interpolate the matched term, function name, etc.
        ("gate.signal.vocabulary", _) => "Vocabulary matched: {term} (layer {layer})",
        ("gate.signal.stub", _) => "Stub pattern: {pattern} in {function}",
        ("gate.signal.snapshot", _) => "Function {function} emptied ({before_lines} → {after_lines} lines)",

        // Spec A v4 / W4 — Amber AskUserQuestion (printed as JSON, consumed by orchestrator).
        ("gate.askuser.amber.question", Locale::PtBr) => "O gate detectou sinais ambíguos. Autorizar a consolidação?",
        ("gate.askuser.amber.question", Locale::EnUs) => "The gate detected ambiguous signals. Authorize consolidation?",
        ("gate.askuser.amber.option_authorize", Locale::PtBr) => "Autorizar",
        ("gate.askuser.amber.option_authorize", Locale::EnUs) => "Authorize",
        ("gate.askuser.amber.option_block", Locale::PtBr) => "Bloquear",
        ("gate.askuser.amber.option_block", Locale::EnUs) => "Block",
        ("gate.askuser.amber.option_block_desc", Locale::PtBr) => "Bloqueia a consolidação até resolução.",
        ("gate.askuser.amber.option_block_desc", Locale::EnUs) => "Block consolidation until resolved.",

        // W5 — span-level review (subagent_inject + agent_prompt_render).
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

        // W8.5 — install-grammars CLI helper.
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

/// Apply `tone` to `text`. `Didactic` is the identity (the catalog is already
/// authored in didactic tone); `Technical` strips parenthetical clarifications
/// of the shape `(meaning ...)`; `Concise` additionally collapses double
/// spaces and trims.
#[must_use]
pub fn apply_tone(text: &str, tone: Tone) -> String {
    match tone {
        Tone::Didactic => text.to_string(),
        Tone::Technical => strip_parentheticals(text),
        Tone::Concise => {
            let stripped = strip_parentheticals(text);
            // Collapse runs of whitespace into a single space and trim.
            let mut out = String::with_capacity(stripped.len());
            let mut prev_ws = false;
            for ch in stripped.chars() {
                if ch.is_whitespace() {
                    if !prev_ws {
                        out.push(' ');
                        prev_ws = true;
                    }
                } else {
                    out.push(ch);
                    prev_ws = false;
                }
            }
            out.trim().to_string()
        }
    }
}

/// Drop `( ... )` segments. Naïve but bounded — no nested parens; we treat the
/// first `)` after a `(` as the close. Keeps the cost predictable.
fn strip_parentheticals(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_paren = false;
    for ch in text.chars() {
        match ch {
            '(' => in_paren = true,
            ')' => in_paren = false,
            _ if !in_paren => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Slugify `text` to a kebab-case identifier, lang-aware.
///
/// PT locale strips Latin diacritics (`ç → c`, `ã → a`, …) before kebab-casing
/// so spec slugs round-trip cleanly. EN locale keeps the input as-is (no
/// Unicode normalisation) — the W4 spec calls out "acentos removidos só do PT".
/// Stopword lists differ per locale (basic articles/prepositions are dropped).
///
/// The output never contains leading/trailing dashes and never collapses to an
/// empty string — fully non-alphanumeric input degrades to `"x"`, mirroring
/// the existing `apps/rt/src/run/scan/interpret.rs::slugify` contract.
#[must_use]
pub fn slugify(text: &str, lang: Locale) -> String {
    let normalised = match lang {
        Locale::PtBr => strip_pt_accents(text),
        Locale::EnUs => text.to_string(),
    };
    let stopwords: &[&str] = match lang {
        Locale::PtBr => &["a", "o", "as", "os", "de", "da", "do", "das", "dos", "e", "em"],
        Locale::EnUs => &["a", "an", "the", "of", "and", "or", "in"],
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

/// Map common Portuguese diacritics to ASCII. Surgical — not a full Unicode
/// NFD normaliser (that would pull `unicode-normalization` into core just for
/// slugs). Covers `ç ã á â à é ê í õ ó ô ú ñ` and their uppercase peers.
fn strip_pt_accents(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        let replacement = match ch {
            'ç' => 'c',
            'Ç' => 'C',
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'ñ' => 'n',
            'Ñ' => 'N',
            other => other,
        };
        out.push(replacement);
    }
    out
}

// ---------------------------------------------------------------------------
// W7 type aliases — `SupportedLocale` (catalogue) + `UserLocale` (open BCP-47)
// ---------------------------------------------------------------------------

/// Catalogue-backed locale — the closed set Mustard ships translations for.
///
/// `SupportedLocale` is a type alias for the original [`Locale`] enum.  Wave 7
/// of the deep-refactor renames the type at every callsite; this alias lets the
/// migration land in a single wave without breaking every consumer at once.
pub type SupportedLocale = Locale;

/// User-declared BCP-47 locale from `mustard.json#specLang` or `### Lang:`.
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

    // AC-W4-3: short forms rejected with a typed error.
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

    // AC-W4-6: known keys translate to the canonical literals.
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

    #[test]
    fn translate_unknown_key_is_failopen() {
        // Missing keys return a stable sentinel rather than panicking.
        assert_eq!(translate("banner.missing.xyz", Locale::PtBr), "<missing-key>");
        assert_eq!(translate("banner.missing.xyz", Locale::EnUs), "<missing-key>");
    }

    #[test]
    fn apply_tone_didactic_is_identity() {
        let input = "Hello (world, expanded).";
        assert_eq!(apply_tone(input, Tone::Didactic), input);
    }

    #[test]
    fn apply_tone_technical_strips_parens() {
        assert_eq!(
            apply_tone("Hello (world, expanded).", Tone::Technical),
            "Hello ."
        );
    }

    #[test]
    fn apply_tone_concise_collapses_whitespace() {
        assert_eq!(
            apply_tone("Hello   (extra)   world.", Tone::Concise),
            "Hello world."
        );
    }

    #[test]
    fn slugify_pt_strips_accents() {
        assert_eq!(slugify("Configuração do Idioma", Locale::PtBr), "configuracao-idioma");
        assert_eq!(slugify("São Paulo é grande", Locale::PtBr), "sao-paulo-grande");
        assert_eq!(slugify("ç ã õ", Locale::PtBr), "c-a-o");
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
    fn tone_parse_accepts_pt_and_en_spellings() {
        assert_eq!(Tone::parse("didactic"), Some(Tone::Didactic));
        assert_eq!(Tone::parse("didatico"), Some(Tone::Didactic));
        assert_eq!(Tone::parse("Técnico"), Some(Tone::Technical));
        assert_eq!(Tone::parse("conciso"), Some(Tone::Concise));
        assert_eq!(Tone::parse("loud"), None);
    }

    #[test]
    fn i18n_render_pipes_translate_through_tone() {
        let i = I18n::new(Locale::EnUs, Tone::Didactic);
        assert_eq!(i.render("banner.close.success"), "Pipeline closed successfully.");
    }

    #[test]
    fn wave_label_formats_per_locale() {
        assert_eq!(wave_label(3, Locale::PtBr), "Onda 3");
        assert_eq!(wave_label(3, Locale::EnUs), "W3");
    }

    /// TF 2026-06-10-ac-heading-unico: `heading.spec.ac` is the ONLY AC
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
