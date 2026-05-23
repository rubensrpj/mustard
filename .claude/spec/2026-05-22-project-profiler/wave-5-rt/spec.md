# Wave 5 (rt + templates) — Write-back spec→skill + telemetria

### Stage: Plan
### Outcome: Active
### Flags:
### Lang: pt
### Checkpoint: 2026-05-22T00:00:00Z
### Parent: 2026-05-22-project-profiler

## PRD

## Contexto

O grafo fica completo quando a spec também é um nó. Quando o pipeline injeta o fecho de contexto num agente, ele já sabe exatamente quais nós entregou — então pode gravar essas arestas `[[ ]]` de volta na spec, automaticamente, no fim da fase. Isso fecha o ciclo: o Obsidian passa a mostrar backlinks ("esta convenção foi usada por estas specs"), o que dá análise de impacto antes de mexer numa convenção e torna visível o artefato morto (skill que nenhuma spec linka, candidata a deleção). É preciso ser honesto na distinção: o pipeline sabe com certeza o que foi *injetado*, mas se a skill *realmente influenciou* o código é inferência mais fraca — as duas arestas são marcadas diferente. Por fim, como o `budget` já mede o tamanho do prompt de cada Task, dá para provar o ganho: tokens injetados por agente caem sem queda na taxa de QA passando.

## Métrica de sucesso

Toda spec executada ganha arestas `[[skill]]` automáticas (`injected`); o Obsidian mostra os backlinks; a telemetria do `budget` registra queda nos tokens injetados por agente comparada ao baseline pré-W4, sem regressão de QA.

## Não-Objetivos

- Não vender a aresta `applied` como fato — ela é inferida e marcada como tal.

## Critérios de Aceitação

- [ ] AC-1: write-back automático — após uma fase EXECUTE simulada, a spec contém arestas `[[id]]` marcadas `injected` — Command: `cargo test -p mustard-rt writeback_injected_edges`
- [ ] AC-2: distinção injected vs applied presente no schema da aresta — Command: `cargo test -p mustard-rt edge_kind_injected_vs_applied`
- [ ] AC-3: detecção de morto — nó sem nenhum backlink de spec é listado por um comando — Command: `cargo test -p mustard-rt dead_node_detection`
- [ ] AC-4: telemetria de injeção comparável — o resolver emite o tamanho do fecho via a métrica de prompt do `budget` — Command: `cargo test -p mustard-rt resolve_emits_prompt_metric`

## Plano

## Summary

No fim da fase de execução, o pipeline grava na spec as arestas `[[id]]` do fecho que o resolver injetou (kind `injected`), e opcionalmente `applied` quando há sinal de que os arquivos tocados batem com o que o nó descrevia. Um `mustard-rt run graph-dead` lista nós sem backlink de spec. O resolver emite o tamanho do fecho pela mesma métrica de prompt do `budget`, permitindo o A/B antes/depois. As SKILLs/commands de pipeline nos templates passam a registrar o write-back.

## Arquivos

- `apps/rt/src/run/scan/graph.rs` — backlinks, `run graph-dead`, schema da aresta com `kind: injected|applied`.
- `apps/rt/src/hooks/budget.rs` ou telemetria de run — emitir o tamanho do fecho do resolver na métrica de prompt existente.
- `apps/rt/src/run/` — passo de write-back chamado no fim do EXECUTE.
- `apps/cli/templates/commands/mustard/**` — as SKILLs de pipeline registram as arestas `injected` ao injetar contexto.

## Tarefas

### rt Agent (Wave 5)

- [ ] Schema de aresta com `kind: injected|applied`; write-back grava `injected` a partir do fecho do resolver.
- [ ] Inferência opcional `applied` (arquivos tocados × nós descritos), claramente marcada como inferida.
- [ ] `mustard-rt run graph-dead` — lista nós-conceito sem backlink de spec.
- [ ] Resolver emite tamanho do fecho via métrica de prompt do `budget` (telemetria para o A/B).
- [ ] Testes: `writeback_injected_edges`, `edge_kind_injected_vs_applied`, `dead_node_detection`, `resolve_emits_prompt_metric`.

### templates Agent (Wave 5)

- [ ] SKILLs de pipeline (`feature`/`bugfix`/`task`) registram as arestas `injected` no fim do EXECUTE.
- [ ] Documentar backlinks e detecção de morto no `templates/CLAUDE.md`.

## Limites

- `.claude/spec/2026-05-22-project-profiler/wave-5-rt/`
- `apps/rt/src/run/scan/graph.rs`, telemetria de run/`budget`, passo de write-back
- `apps/cli/templates/commands/mustard/**`, `apps/cli/templates/CLAUDE.md`
- NÃO reabrir o resolver (W4) nem a interpretação (W2) — esta wave só fecha o ciclo de write-back e medição.
