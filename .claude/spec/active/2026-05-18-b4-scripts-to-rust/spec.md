# Feature: b4-scripts-to-rust

### Status: implementing | Phase: EXECUTE | Scope: full
### Checkpoint: 2026-05-19T14:30:00Z
### Lang: pt

> Spec de backlog (Parte B, item B4). **ÉPICO** — porta os scripts JS para subcomandos do binário `mustard-rt`. Depende de B2; rodou em paralelo a B3 (concluído). Refinada 2026-05-19 no ANALYZE: inventário real (~48 arquivos), mapa de invocações (~40 sites), forma de invocação aninhada (`mustard-rt script <nome>`) e decomposição em 7 waves por família.

## Contexto

Os scripts em `packages/cli/templates/scripts/` — `sync-detect`, `sync-registry`, `diff-context`, `qa-run`, `metrics`, `spec-extract`, `event-projections`, `wave-tree` e os demais, mais os scanners de `registry/` e `scan/` — são invocados pelos comandos do pipeline via `bun`/`node`. Eles têm a mesma fragilidade de runtime dos hooks. Com os hooks já em Rust (B3 concluído), manter os scripts em JS deixa o Mustard com dois runtimes pela metade. Portar os scripts para subcomandos do mesmo binário `mustard-rt` completa a unificação: um binário, zero dependência de runtime, e os comandos do pipeline passam a invocar `mustard-rt run <nome>` em vez de `bun .claude/scripts/<nome>.js`.

## Resumo

Portar os ~48 arquivos de `packages/cli/templates/scripts/` (31 scripts de topo + 14 em `registry/` + 3 em `scan/`) para subcomandos do binário `mustard-rt` (B3), consumindo `mustard-core` (B2). Os scripts viram a **terceira face** do binário, sob um subcomando aninhado `run` (`mustard-rt run <nome>`), distinto das faces de hooks (`mustard-rt on <event>` / `mustard-rt check <id>`). Atualizar todos os comandos do pipeline e refs que invocam `bun .claude/scripts/*.js` para chamar `mustard-rt run <nome>`. Migração incremental, família por família, em 7 waves. Os scripts que produzem **relatório para humano** (`qa-run`, `metrics`, `event-projections`, `verify-pipeline`) ganham um modo de saída **HTML** além do JSON.

## Entidades

N/A — infraestrutura de scripts.

## Component Contract

N/A.

## Decisão de invocação — subcomando aninhado

O `Command` enum de `packages/rt/src/main.rs` ganha uma variante `Run { RunCmd }`. Forma escolhida: **aninhada** (`mustard-rt run <nome>`). Todos os scripts viram Rust compilado — `run` é apenas o rótulo da face, não uma linguagem.

- Mantém o enum de topo estável (3 variantes: `On`, `Check`, `Run`) — ~48 scripts não poluem o topo.
- `run` é verbo, coerente com `on`/`check`; lê-se "rode a utilidade X". Sem conotação de linguagem de script.
- Permite B5 (CLI) adicionar outra face sem conflito; sem colisão de nomes com a face de hooks.
- Custo: invocação 1 palavra mais longa — trivial.

## Relatórios HTML — quando e por quê

Decisão baseada na regra de Thariq Shihipar (Anthropic, *"The Unreasonable Effectiveness of HTML"*, mai/2026): **HTML para documento com leitor humano terceiro que não o edita; Markdown/JSON para o que pipeline/agente consome.** Aplicado aos scripts:

- **Saída de máquina (default, fica JSON):** `sync-detect`, `sync-registry`, `diff-context`, `spec-extract`, `wave-tree`, `scope-decompose` etc. — consumidos pelo pipeline e por agentes. Continuam JSON/markdown.
- **Relatório humano (ganha `--format html`):** `qa-run` (QA pass/fail visual), `metrics` (custo de token por agente), `event-projections` (timeline de `events.jsonl`), `verify-pipeline` (build/test). Arquivo HTML standalone, read-only, que o humano abre no browser e o dashboard linka. **JSON continua o default** — HTML é opt-in via flag, artefato adicional, nunca substituto.

## Arquivos

- `packages/rt/src/main.rs` — adicionar variante `Run { RunCmd }` ao `Command` enum
- `packages/rt/src/run/mod.rs` — `RunCmd` (clap) + dispatch por script
- `packages/rt/src/run/*.rs` — um módulo por família/script, consumindo `mustard-core`
- `packages/rt/src/report/` — gerador de relatório HTML compartilhado (template embutido, sem dependência de runtime externo, fail-open)
- `packages/cli/templates/scripts/*.js` (+ `registry/`, `scan/`, `_lib/event-store.js`) — removidos conforme portados
- `packages/cli/templates/hooks/_lib/{harness-event,hook-env,runtime-shim,event-store,metrics-emit}.js` — deletados na Wave 7 (órfãos após portar todos os consumidores)
- `packages/cli/templates/commands/mustard/*/SKILL.md` — atualizar invocações `bun .claude/scripts/*` → `mustard-rt run *`
- `packages/cli/templates/refs/**/*.md` — idem onde houver invocação de script

## Limites

- `packages/rt/`, `packages/cli/templates/scripts/`, `packages/cli/templates/hooks/_lib/`, e as invocações de script em `packages/cli/templates/commands/` e `packages/cli/templates/refs/`.
- **Fora dos limites:** hooks (B3, concluído), CLI (B5), o adapter Cursor (`templates/adapters/cursor/`), e a lógica de decisão dos scripts (porte fiel).

## Tarefas

> Decomposição em 7 waves por família de script. Cada wave porta sua família, **atualiza as invocações dos seus próprios scripts** nos comandos/refs (mantendo o pipeline funcional a cada wave) e remove os `.js` portados. A Wave 7 faz a varredura final e deleta os `_lib` órfãos.

### Impl Agent (Wave 1) — scaffold + scanners de linguagem

- [x] `main.rs`: adicionar variante `Run { RunCmd }` ao `Command` enum.
- [x] Criar `packages/rt/src/run/mod.rs` com o enum `RunCmd` (clap) e o dispatch.
- [x] Portar o contrato de scanner: `registry/scanner-contract.js`, `registry/scanner-loader.js`, `registry/pluralize.js`.
- [x] Portar os 7 scanners de linguagem: `typescript`, `python`, `go`, `rust`, `java`, `php`, `dotnet`.
- [x] Portar `sync-detect.js`; atualizar suas invocações em `bugfix`, `feature`, `refs/scan/scan-protocol.md`.

### Impl Agent (Wave 2) — montagem do registry

- [x] Portar `sync-registry.js` e os enriquecedores: `registry/cluster-discovery.js`, `registry/description-enricher.js`, `registry/project-conventions.js`, `registry/schema-builder.js`.
- [x] Escopo da orquestração `scan/`: `orchestrate.js`/`_precompute.js`/`finalize.js` são drivers do comando `/scan` (não da camada de dados do registry) — porte movido para a Wave 6.
- [x] Atualizar as invocações de `sync-registry` (9 sites em `commands/`, `refs/` e `skills/`).

### Impl Agent (Wave 3) — estado de pipeline + memória

- [ ] Portar `diff-context.js`, `emit-phase.js`, `complete-spec.js`, `context-slice.js`.
- [ ] Portar `memory.js` e `epic-fold.js` (consomem `_lib/harness-event.js` — porte fiel da emissão de eventos via `mustard-core`).
- [ ] Atualizar as invocações desses scripts em `feature`, `close`, `bugfix`, `refs/knowledge/evolve-report.md`, `refs/resume/fix-loop-wave.md`.

### Impl Agent (Wave 4) — parsing de spec + análise de waves

- [ ] Portar `spec-extract.js`, `spec-link.js`, `analyze-validation.js`, `mark-checklist-item.js`.
- [ ] Portar `wave-tree.js`, `wave-dependency.js`, `scope-decompose.js`, `exec-rewave-check.js`, `wave-size-check.js`.
- [ ] Portar `recipe-match.js`.
- [ ] Atualizar as invocações em `feature`, `approve`, `close`, `refs/feature/wave-decomposition.md`.

### Impl Agent (Wave 5) — relatórios + HTML

- [ ] Construir `packages/rt/src/report/` — gerador HTML standalone (template embutido, fail-open).
- [ ] Portar `qa-run.js`, `metrics.js`, `event-projections.js`, `verify-pipeline.js`, `pipeline-summary.js`, `review-result.js`.
- [ ] Adicionar `--format json|html` a `qa-run`, `metrics`, `event-projections`, `verify-pipeline` (JSON é o default).
- [ ] Ao portar `event-projections`: remover/ajustar o `buildSlopeReport` — após B3 deletar `duplication-check`/`convention-check`, ninguém emite `duplication.warn`/`convention.warn` (ver Preocupações).
- [ ] Atualizar as invocações em `bugfix`, `close`, `feature`, `refs/resume/fix-loop-wave.md`.

### Impl Agent (Wave 6) — telemetria + validação

- [ ] Portar `statusline.js`, `skills.js`, `security-scan.js`, `otel-collector.js`, `diagnose-otel.js`, `verify-emit.js`, `_rtk-gain.js`.
- [ ] Portar a orquestração do `/scan` (deferido da Wave 2): `scan/orchestrate.js`, `scan/_precompute.js`, `scan/finalize.js`.
- [ ] Atualizar as invocações em `refs/scan/scan-protocol.md`, `refs/scan/evidence-rules.md`, `refs/feature/ac-cross-shell.md`.

### Impl Agent (Wave 7) — limpeza + orfanização

- [ ] Varredura final: nenhum `bun/node .claude/scripts` nem `bun templates/scripts` resta em `commands/` ou `refs/` (AC-2).
- [ ] Deletar os 5 `_lib/*.js` órfãos (`harness-event`, `hook-env`, `runtime-shim`, `event-store`, `metrics-emit`), o `runtime-shim.d.ts` e o re-export `scripts/_lib/event-store.js` — confirmando que nenhum hook Rust nem script remanescente os consome.
- [ ] Confirmar que `rtk` reescreve/passa `mustard-rt script *` (ver Preocupações — RTK).
- [ ] Remover todos os `.js` portados restantes de `templates/scripts/`.

## Dependências

- B2 (`mustard-core`) — concluído.
- B3 (hooks → Rust) — concluído; compartilha o crate `packages/rt` e o binário `mustard-rt`.

## Preocupações

- **Volume real:** ~48 arquivos (31 de topo + 14 em `registry/` + 3 em `scan/`). Decomposto em 7 waves por família.
- **Invocações espalhadas:** ~40 sites em ~10 arquivos de `commands/`/`refs/`. Mais invocados: `sync-registry` (6), `memory` (5), `wave-tree`/`qa-run` (3). Cada wave atualiza as invocações dos seus scripts; a Wave 7 faz a varredura. AC-2 é o gate.
- **Ordem de orfanização:** os 5 `_lib` só podem ser deletados na Wave 7, depois de portar todos os 6 consumidores: `epic-fold` (W3), `memory` (W3), `spec-link` (W4), `qa-run` (W5), `review-result` (W5), e o proxy `scripts/_lib/event-store.js`. A Wave 7 valida que nenhum hook Rust nem script remanescente os consome antes de deletar.
- **Código morto herdado de B3:** `event-projections.js` (~linha 647, `buildSlopeReport`) projeta eventos `duplication.warn`/`convention.warn` que ninguém emite mais (B3 deletou os hooks `duplication-check`/`convention-check`). Ao portar (W5), remover esse trecho — não reproduzir código morto.
- **RTK:** invocações `rtk bun .claude/scripts/*` viram `rtk mustard-rt run *`. O `rtk-rewrite`/`bash_guard` (B3) precisa reconhecer/passar `mustard-rt` sem reescrita destrutiva — verificar na Wave 7.
- **HTML não vira default:** o relatório HTML é opt-in via `--format html`. JSON continua o default — quebrar o formato que o pipeline consome regrediria o pipeline inteiro.

## Concerns

> Registradas durante o EXECUTE. Surfaceadas no CLOSE.

- **W1 — gate de cache de `sync-detect` não portado:** o early-exit de cache (5 min) e a comparação de hash contra o cache anterior foram omitidos no porte. `hashChanged` emite sempre `true` e `moduleHashes` sempre `{}`. O shape do JSON e a corretude são preservados (sempre re-sincroniza), mas perde-se a otimização de skip incremental por SHA256 citada no CLAUDE.md. **Wave 2** (porte de `sync-registry`) decide: restaurar a comparação de hash ou aceitar a perda de performance. Não bloqueia.

## Critérios de Aceitação

- [ ] AC-1: O binário compila e os testes passam — Command: `bash -c 'cargo build -p mustard-rt && cargo test -p mustard-rt'`
- [ ] AC-2: Nenhuma invocação de script JS resta nos comandos/refs — Command: `bash -c '! grep -rlE "(claude|templates)/scripts" packages/cli/templates/commands packages/cli/templates/refs'`
- [ ] AC-3: Os scripts de relatório aceitam saída HTML — Command: `bash -c 'mustard-rt run qa-run --help | grep -qi html'`
- [ ] AC-4: Os 5 `_lib/*.js` órfãos foram removidos — Command: `bash -c '! ls packages/cli/templates/hooks/_lib/harness-event.js packages/cli/templates/hooks/_lib/hook-env.js packages/cli/templates/hooks/_lib/runtime-shim.js packages/cli/templates/hooks/_lib/event-store.js packages/cli/templates/hooks/_lib/metrics-emit.js 2>/dev/null'`
- [ ] AC-5: Nenhum script `.js` resta em `templates/scripts/` — Command: `bash -c '! ls packages/cli/templates/scripts/*.js 2>/dev/null'`

## Não-Objetivos

- Não portar hooks (B3, concluído) nem CLI (B5).
- Não mudar o comportamento de decisão de nenhum script — porte fiel.
- Não atualizar o adapter Cursor (`templates/adapters/cursor/`) — fora dos limites, nota para os mantenedores da camada de adapters.
- Não converter specs nem contexto de agente para HTML — esses são consumidos por pipeline/agente e ficam markdown (regra do Thariq).
