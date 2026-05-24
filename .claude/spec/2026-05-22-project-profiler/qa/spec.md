# QA — project-profiler

## PRD

## Contexto

Fase de aceitação final: roda todos os Critérios de Aceitação transversais (AC-P-1..8 do spec parent) mais os ACs de cada wave, contra o próprio repo Mustard como consumidor real.

## Critérios de Aceitação

- [ ] AC-1: workspace compila — Command: `cargo build --workspace`
- [ ] AC-2: clippy limpo — Command: `cargo clippy -p mustard-rt -- -D warnings`
- [ ] AC-3: testes do rt passam — Command: `cargo test -p mustard-rt`
- [ ] AC-4: zero scanners por linguagem — Command: `node -e "const fs=require('fs');const n=fs.readdirSync('apps/rt/src/run/scan').filter(f=>/_scanner\.rs$/.test(f)).length;process.exit(n===0?0:1)"`
- [ ] AC-5: vault Obsidian presente — Command: `node -e "const fs=require('fs');process.exit(fs.existsSync('.claude/.obsidian')&&fs.existsSync('.claude/graph/index.md')?0:1)"`
- [ ] AC-6: fecho mínimo comprovado — Command: `cargo test -p mustard-rt resolve_closure_is_minimal`

## Plano

## Summary

`mustard-rt run qa-run --spec 2026-05-22-project-profiler`. Todos pass → CLOSE. Qualquer fail → volta para a wave responsável.

## Limites

- `.claude/spec/2026-05-22-project-profiler/qa/`
