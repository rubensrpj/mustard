# Plano de ondas — W2 residuals (ClaudePaths sweep)

## Contexto

[[2026-05-26-claude-paths-single-source]] introduziu `ClaudePaths` (em `packages/core/src/claude_paths.rs`) + `workspace_root()` walker. A wave W2 dessa parent listou ~33 arquivos para migrar e fechou; um grep agnóstico identifica **89 callsites de `.join(".claude")` em 52 arquivos**, distribuídos em três diretórios (`hooks/`, `run/`, `mcp/`). A primeira tentativa flat (rt-impl único, 22min/246 tool calls) sweepou parcial e estourou contexto.

Esta spec divide o resíduo em **5 ondas mecânicas**, com lista de arquivos enumerada por wave (zero descoberta no agent — só Read+Edit por arquivo, com `rtk cargo check -p mustard-rt` ao fim de cada wave).

## Diagrama de dependências

```
W1 hooks + mcp
W2 run/ emit/memory/amend
W3 run/ skills + spec helpers + scan/
W4 run/ misc tail + scan_md_validate.rs
  ↓
W5 tests migration + one-shot cleanup + doctor verify
```

W1-W4 são disjuntas (sem overlap de arquivos) → **podem rodar em paralelo**. W5 sequencial (depende de todas).

## Tabela de ondas

| # | Spec | Role | Depende de | Arquivos | Violações | Resumo |
|---|------|------|------------|----------|-----------|--------|
| 1 | [[wave-1-rt]] | rt | — | 9 | 12 | Sweep `apps/rt/src/hooks/` (7 arquivos) + `apps/rt/src/mcp/` (2 arquivos). Cada `.join(".claude")` → método `ClaudePaths`. |
| 2 | [[wave-2-rt]] | rt | — | 8 | 16 | Sweep pipeline writers em `apps/rt/src/run/`: `emit_pipeline.rs`, `emit_phase.rs`, `event_writer_ndjson.rs`, `memory_cross_wave.rs`, `memory_ingest.rs`, `spec_memory.rs`, `amend_finalize.rs`, `resume_bootstrap.rs`. |
| 3 | [[wave-3-rt]] | rt | — | 13 | 24 | Sweep `apps/rt/src/run/`: skills (5 files), spec helpers (4), scan/ (3), wikilink + migrate_to_meta. |
| 4 | [[wave-4-rt]] | rt | — | 22 | 37 | Cauda longa em `apps/rt/src/run/`: status, unhook, sync_detect, db_maintain, transcript_watcher, scan_md_validate + 16 arquivos de 1 violação cada. |
| 5 | [[wave-5-rt]] | rt | [[1]], [[2]], [[3]], [[4]] | varies | — | Migrar testes de integração em `apps/rt/tests/**` para `common::test_workspace()`. Script PowerShell one-shot para apagar 7 legados em `.claude/`. Verificar `mustard-rt run doctor --check claude-paths`. |

## Paralelização

| Janela | Pode rodar em paralelo |
|---|---|
| Início | W1 + W2 + W3 + W4 (escopos disjuntos por arquivo) |
| Após W1-W4 | W5 (cleanup + verify) |

## Critérios de Aceitação (globais)

Iguais aos da spec umbrella ([[spec]]):

- **AC-G1.** Zero `.join(".claude")` em código não-test fora de `ClaudePaths`/`claude_paths.rs`.
- **AC-G2.** 10× `cargo test -p mustard-rt` não criam `apps/rt/.claude/`.
- **AC-G3.** `.claude/` raiz sem os 7 legados volatile.

Cada wave tem AC próprio (ver `wave-N-rt/spec.md`) que valida só seu sub-conjunto.

## Não-Objetivos (ondas)

- **Não migrar tests `dir.path().join(".claude")` em `#[cfg(test)]`** — esses são fixtures legítimos para `tempfile::TempDir`. ClaudePaths é para callsites de produção.
- **Não refatorar `claude_paths.rs` em si** — fonte canônica, sweepamos consumidores.
- **Não tocar `packages/core/`, `apps/cli/`, `apps/dashboard/`** — out-of-scope da parent W2.

## Riscos eliminados por design

- **Burn-out de contexto** — agent vê lista enumerada (zero Glob/Grep para descobrir), 8-22 arquivos por wave.
- **Drift entre waves** — escopos disjuntos por arquivo (verificável via diff por wave).
- **Build break** — cada wave roda `rtk cargo check -p mustard-rt` no final; falha ⇒ wave reaberta.
