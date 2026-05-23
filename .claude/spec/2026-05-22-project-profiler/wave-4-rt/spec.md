# Wave 4 (rt) — Resolver de injeção mínima

### Stage: Plan
### Outcome: Active
### Flags:
### Lang: pt
### Checkpoint: 2026-05-22T00:00:00Z
### Parent: 2026-05-22-project-profiler

## PRD

## Contexto

Com o grafo no lugar, o orquestrador pode parar de injetar arquivos inteiros e passar a injetar exatamente o fecho de nós que a tarefa exige. Hoje o Mustard tem quatro mecanismos separados e frágeis para decidir o que carregar — casamento de skill por descrição, `recipe-match` por entidade e operação, refs progressivos lidos sob demanda, e o `context-slice` do glossário — cada um com sua própria noção de relevância e sem deduplicação entre si. Esta wave os unifica numa primitiva só: dado o escopo da tarefa (entidade, operação, camada), um BFS determinístico anda pelas arestas `requires` a partir dos nós de entrada, para no fecho mínimo, corta pelo budget do papel, e dereferencia cada `id` para o conteúdo real via o índice `id→path`. O agente recebe só o necessário, com cada convenção compartilhada incluída uma única vez. O Claude Code nunca vê `[[ ]]` cru — recebe o conteúdo já resolvido.

## Métrica de sucesso

`context-resolve` devolve, para um escopo de teste, um fecho menor que o conjunto completo de nós; convenções compartilhadas aparecem uma vez; o resultado respeita o budget do papel; os 4 mecanismos antigos passam a chamar o resolver.

## Não-Objetivos

- Não inventar relevância nova — o escopo de entrada reusa a detecção de escopo já feita pelo pipeline.

## Critérios de Aceitação

- [ ] AC-1: fecho mínimo — resolve um escopo de teste e retorna menos nós que o total, com dedup — Command: `cargo test -p mustard-rt resolve_closure_is_minimal`
- [ ] AC-2: corte por budget — fecho que estoura o budget do papel é truncado por distância no grafo — Command: `cargo test -p mustard-rt resolve_respects_budget`
- [ ] AC-3: dereferência — saída traz conteúdo resolvido, sem `[[ ]]` cru — Command: `cargo test -p mustard-rt resolve_dereferences_ids`

## Plano

## Summary

Novo `mustard-rt run context-resolve --scope '{entities,operation,layer}'` que carrega o `.context-graph.json`, faz BFS a partir dos nós de entrada pelas arestas `requires`, deduplica, ordena por distância, corta por budget do papel, e emite a lista ordenada de conteúdos resolvidos (JSON byte-estável). Reescrever os 4 mecanismos atuais (skill-match, `recipe-match`, refs, `context-slice`) como chamadas a esse resolver.

## Arquivos

- `apps/rt/src/run/scan/resolve.rs` — novo: carrega grafo, BFS, dedup, ordenação por distância, corte por budget, dereferência `id→path`.
- `apps/rt/src/run/mod.rs` / dispatcher — registrar `run context-resolve`.
- `apps/rt/src/run/` (recipe-match, context-slice e correlatos) — passar a delegar ao resolver.
- `apps/rt/src/hooks/budget.rs` — reusar os budgets por papel já definidos como teto do fecho.

## Tarefas

### rt Agent (Wave 4)

- [ ] `resolve.rs`: BFS sobre `requires`, dedup por `id`, ordenação por distância do nó de entrada.
- [ ] Corte por budget do papel (reusar tabela do `budget.rs`); truncar os nós mais distantes primeiro.
- [ ] Dereferência `id→path` → conteúdo; garantir que nenhum `[[ ]]` cru sai na saída.
- [ ] `mustard-rt run context-resolve` (JSON byte-estável) + registro no dispatcher; cache por hash do escopo.
- [ ] Migrar `recipe-match`/`context-slice`/refs/skill-match para delegar ao resolver (unificação).
- [ ] Testes: `resolve_closure_is_minimal`, `resolve_respects_budget`, `resolve_dereferences_ids`.

## Limites

- `.claude/spec/2026-05-22-project-profiler/wave-4-rt/`
- `apps/rt/src/run/scan/resolve.rs`, dispatcher de `run`, subcomandos de carga existentes
- `apps/rt/src/hooks/budget.rs` (somente leitura da tabela de budget)
- NÃO alterar o write-back de specs (W5).
