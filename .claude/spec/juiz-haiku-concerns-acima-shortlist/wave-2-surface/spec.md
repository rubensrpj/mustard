---
id: wave.juiz-haiku-concerns-acima-shortlist.2-surface
---

# wave-2-surface

## Resumo

Passo do juiz Haiku no SKILL do /feature: gate multi-concern -> render -> dispatch haiku -> parse -> decompor por concern, com fallback deterministico

## Rede

- Pai: [[juiz-haiku-concerns-acima-shortlist]]
- Depende de: [[wave-1-impl]]

## Tarefas

- [ ] Em apps/cli/templates/commands/mustard/feature/SKILL.md, adicionar o passo do juiz Haiku no ANALYZE/DECOMPOSE: gate de sinal multi-concern -> `mustard-rt run concern-judge-render` -> dispatch de agente Haiku (modelo haiku) com o prompt renderizado -> parse dos concerns -> decompor por concern.
- [ ] Documentar o fallback deterministico: se o juiz falhar ou estiver indisponivel, cair nos anchors planos sem quebrar o fluxo.
- [ ] Reforcar a invariante na prosa: o digest do scan segue 100% deterministico; o juiz e a unica etapa de IA e mora na orquestracao (espelha .memory-approved).

## Arquivos

- `apps/cli/templates/commands/mustard/feature/SKILL.md`
