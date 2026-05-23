# Review — project-profiler

### Stage: Plan
### Outcome: Active
### Flags:
### Lang: pt
### Checkpoint: 2026-05-22T00:00:00Z
### Parent: 2026-05-22-project-profiler

## PRD

## Contexto

Revisão consolidada das cinco waves antes do QA. Foca nos pontos de maior risco do programa: determinismo da saída sob paralelismo (W1), fronteira conhecimento×plumbing do grafo (W3), e a honestidade da distinção injected/applied (W5).

## Critérios de Aceitação

- [ ] AC-1: build + clippy limpos nos subprojetos afetados — Command: `cargo clippy -p mustard-rt -- -D warnings`
- [ ] AC-2: nenhum `*_scanner.rs` por linguagem permanece — Command: `node -e "const fs=require('fs');const n=fs.readdirSync('apps/rt/src/run/scan').filter(f=>/_scanner\.rs$/.test(f)).length;process.exit(n===0?0:1)"`
- [ ] AC-3: o grafo não contém arestas de plumbing (config/wiring/imports) — Command: `cargo test -p mustard-rt graph_no_plumbing_edges`

## Plano

## Summary

Dispatch de review agent por subprojeto (rt, templates). REJECTED → fix loop (máx 2). Re-reviews em `sonnet`. Emitir `review-result` por subprojeto.

## Tarefas

### Review Agent

- [ ] Review `apps/rt/` — determinismo, fail-open, JSON byte-estável, cache por SHA.
- [ ] Review `apps/cli/templates/` — vault config, write-back nas SKILLs.
- [ ] Verificar fronteira conhecimento×plumbing no grafo gerado.

## Limites

- `.claude/spec/2026-05-22-project-profiler/review/`
