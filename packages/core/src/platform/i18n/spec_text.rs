//! O texto da spec em markdown: os títulos das seções, as lacunas a preencher,
//! os moldes dos critérios, os marcadores e o diagnóstico da seção de arquivos,
//! os títulos do resumo e do contexto da onda, a nota de memória e os rótulos
//! curtos da onda e do critério.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["heading", "placeholder", "checklist", "ac", "context", "marker", "memory", "scope", "wave"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Short wave label (e.g. "W3" vs "Onda 3"). The numeric suffix is
        // interpolated by the caller via `format!("{} {n}", translate(...))`.
        ("wave.label", Locale::PtBr) => "Onda",
        ("wave.label", Locale::EnUs) => "W",

        // Acceptance-criterion label (used as a prefix before the AC id).
        ("ac.label", Locale::PtBr) => "CA",
        ("ac.label", Locale::EnUs) => "AC",

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
        // identical strings) was collapsed into this one: two keys for the
        // same heading let the scaffold emit the AC section twice, shadowing
        // the real list for every `section_block` reader.
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
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::translate;

    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("spec_text.rs"),
            super::PREFIXES,
            60,
            0xf2f4_1f3e_4973_51f1,
        );
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
}
