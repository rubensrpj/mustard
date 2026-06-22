---
id: wave.matar-prd-standalone-fazer-feature.1-grill
---

# wave-1-grill

## Resumo

/feature grelha inline: glossary-coverage expõe termos fracos, mini-grill focado, escritor map-aware grava no CONTEXT.md do subprojeto

## Rede

- Pai: [[matar-prd-standalone-fazer-feature]]

## Tarefas

- [ ] Estender glossary_coverage.rs para expor os termos de domínio fracos/ausentes (não só o verdict), para o orquestrador agir
- [ ] Criar grill_capture.rs: grava um bloco de termo confirmado no CONTEXT.md resolvido por resolve_context_files (map-aware); glossário-só, atualiza-não-duplica, fail-open quando ausente
- [ ] Registrar grill-capture em commands/mod.rs
- [ ] Tornar subagent_inject.rs::read_context_md ciente do CONTEXT-MAP (hoje lê um CONTEXT.md único) reusando resolve_context_files
- [ ] Reescrever o passo de glossário no feature/SKILL.md e glossary-nudge.md: de nudge-só para o grill inline leve que grava confirmados

## Arquivos

- `apps/rt/src/commands/glossary_coverage.rs`
- `apps/rt/src/commands/grill_capture.rs`
- `apps/rt/src/commands/mod.rs`
- `apps/rt/src/commands/economy/context_slice.rs`
- `apps/rt/src/hooks/task/subagent_inject.rs`
- `apps/cli/templates/commands/mustard/feature/SKILL.md`
- `apps/cli/templates/refs/feature/glossary-nudge.md`
