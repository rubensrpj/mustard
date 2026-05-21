# Wave 3 — Ingestão externa: adapters OTEL + JSONL + RTK

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: queued
### Phase: PLAN
### Scope: full (wave)
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt

## PRD

Três fontes externas trazem dado que hooks internos não conseguem: (a) OTEL collector entrega custo USD oficial Anthropic via stream OTLP do Claude Code — código pronto em `apps/rt/src/run/otel/`, mas spawn nunca foi reativado após migração JS→Rust (gap explícito em `session_start.rs:29-34`); (b) JSONL local do Claude Code em `~/.claude/projects/<encoded>/<session>.jsonl` carrega `usage.input_tokens`/`output_tokens`/`cache_*` + conteúdo bruto das requisições (tool calls, diffs) — nunca parseado; (c) `rtk gain --json` reporta economia do binário RTK — hoje lido via `Command::new("rtk")` pull-on-demand sem persistência. Esta wave consolida os três como **adapters paralelos** em `packages/core/src/economy/sources/`, cada um traduzindo sua fonte para os tipos de domínio da W1 e chamando o writer único.

## Acceptance Criteria

- [ ] AC-1: Build do rt + core passa — Command: `cargo check -p mustard-rt && cargo check -p mustard-core`
- [ ] AC-2: Adapter OTEL existe — Command: `node -e "if(!require('fs').existsSync('packages/core/src/economy/sources/otel.rs'))throw new Error('otel.rs missing')"`
- [ ] AC-3: Adapter JSONL existe — Command: `node -e "if(!require('fs').existsSync('packages/core/src/economy/sources/transcript.rs'))throw new Error('transcript.rs missing')"`
- [ ] AC-4: Adapter RTK existe — Command: `node -e "if(!require('fs').existsSync('packages/core/src/economy/sources/rtk.rs'))throw new Error('rtk.rs missing')"`
- [ ] AC-5: `session_start.rs` faz spawn do collector — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/session_start.rs','utf8');if(!t.includes('otel-collector'))throw new Error('session_start missing collector spawn')"`
- [ ] AC-6: Hook `SessionEnd` parseia JSONL — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/session_cleanup.rs','utf8');if(!t.includes('transcript_parser')&&!t.includes('parse_transcript'))throw new Error('SessionEnd missing transcript parse call')"`

## Plano

Três adapters em `packages/core/src/economy/sources/{otel,transcript,rtk}.rs`, cada um expondo `pub fn ingest(...) -> Result<Vec<ApiCostFrame|SavingsRecord>>` que retorna records traduzidos. Hooks no `rt` chamam:
- `session_start.rs` — spawn detachado do `mustard-rt run otel-collector` + escreve PID em `.claude/.harness/.otel-collector.pid` (~20 linhas, fecha gap da migração b3).
- `session_cleanup.rs` (`SessionEnd`) — invoca `sources::transcript::ingest(session_jsonl_path)` e chama `writer::record_api_cost()` para cada frame retornado.
- Adapter OTEL substitui a leitura ad-hoc em `dashboard/src-tauri/src/telemetry.rs` — passa a ler via `sources::otel::ingest()` (mesmo schema, fonte centralizada).
- Adapter RTK substitui `apps/rt/src/run/rtk_gain.rs` (que vira thin wrapper sobre `sources::rtk::ingest()`).
- Watcher opcional (file-system watcher em `~/.claude/projects/`) fica como sub-task — implementar como `mustard-rt run transcript-watcher` daemon, spawned por `session_start` se env `MUSTARD_TRANSCRIPT_WATCH=1`.

## Dependências

- [[wave-1-core-economy]]: writer API + tipos de record + facade.

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Depende de: [[wave-1-core-economy]]
- Paralela a: [[wave-2-hooks-real]] (independentes — uma instrumenta internamente, outra absorve externamente)
- Desbloqueia: [[wave-4-attribution]]
- Grava memória: `{adapters: ['otel','transcript','rtk'], pid_path: '.claude/.harness/.otel-collector.pid', watcher_env: 'MUSTARD_TRANSCRIPT_WATCH', records_per_source: {...}}` para [[wave-4-attribution]]

## Limites

Em escopo: `packages/core/src/economy/sources/{otel,transcript,rtk}.rs`, `apps/rt/src/hooks/{session_start,session_cleanup}.rs`, `apps/rt/src/run/otel/` (refactor para usar sources), `apps/rt/src/run/rtk_gain.rs` (vira wrapper), `apps/rt/src/run/transcript_watcher.rs` (novo, opcional).

Fora de escopo: outros hooks, dashboard, qualquer alteração de schema.
