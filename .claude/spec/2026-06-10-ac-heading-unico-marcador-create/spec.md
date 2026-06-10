# Tactical Fix: heading de AC único no scaffold + marcador (create) localizado no validador

## Contexto

Auditoria 2026-06-10 (memória `mustard-sialia-payables-audit`): write_spec_md emite o heading de AC DUAS vezes (loop de PRD sections com a entrada "acceptance-criteria" de corpo "Ver abaixo." + bloco da lista via heading.spec.ac_list; as duas chaves i18n são byte-idênticas em i18n.rs) → todo draft virgem, Light E Full (prd_sections é montado sem guard de scope), reprova no próprio analyze-validation: section_block pega a primeira seção (placeholder), parse_ac_items vazio, WARN unparseable-ac, ok:false. Reproduzido com binário g4327b44 sem nenhum toque de LLM. Gap adjacente da mesma validação: analyze_validation só reconhece o literal EN "(create)" → os 7 missing-file da run vieram do drafter pt-BR emitindo "(novo)"/"(editar)". Nenhum teste roundtrip draft→validate existe (fixtures manuscritas desviam da colisão).

Fix: (1) emissor único — pular "acceptance-criteria" no loop PRD (padrão do skip is_wave_plan/tasks já existente), bloco da lista permanece como único emissor; SpecInput segue carregando a entrada (check_sections exige presença+ordem — fix só de render); (2) colapsar heading.spec.ac_list em heading.spec.ac e aposentar placeholder.see_below; (3) leitor defensivo: section_block prefere a seção homônima com itens parseáveis (specs legadas já duplicadas em disco); (4) validador aceita marcadores localizados ((novo)/(criar)/(editar)) via catálogo i18n, não literal hardcoded; (5) teste roundtrip draft→validate (Light+Full, pt-BR+en): exatamente 1 heading de AC no spec.md draftado e analyze-validation ok:true.

## Critérios de Aceitação

- **AC-1** — Roundtrip: spec-draft virgem (Light e Full, pt-BR e en) contém exatamente 1 heading de AC e passa analyze-validation com ok:true; marcador (novo) reconhecido como (create).
  Command: `cargo test -p mustard-rt roundtrip`
- **AC-2** — Workspace verde.
  Command: `cargo test --workspace`

## Arquivos

- apps/rt/src/commands/spec/spec_scaffold.rs — skip de acceptance-criteria no loop PRD (write_spec_md)
- apps/rt/src/commands/spec/spec_draft.rs — aposentar placeholder.see_below para acceptance-criteria
- apps/rt/src/commands/spec/spec_sections.rs — section_block defensivo (preferir seção com itens parseáveis)
- apps/rt/src/commands/review/analyze_validation.rs — marcador (create) localizado via i18n
- packages/core/src/platform/i18n.rs — colapsar heading.spec.ac_list
- testes roundtrip em apps/rt