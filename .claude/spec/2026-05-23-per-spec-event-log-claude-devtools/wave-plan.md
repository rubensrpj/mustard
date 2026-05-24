# Per-spec event log no estilo claude-devtools

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: full (wave plan)
### Lang: pt
### Total waves: 5

## PRD

### Contexto

Hoje cada `tool call` de cada subagent escreve uma linha na tabela global `events` do `mustard.db` (com `events_fts` FTS5 ao lado). Múltiplos subagents em waves paralelas brigam pelo lock único do SQLite, eventos ficam misturados entre todas as specs ao longo do tempo, e o dashboard só consegue mostrar `kind:label` agrupado por fase — sem ver os parâmetros que o agente mandou nem o retorno bruto que ele recebeu. O resultado é hot path lento, banco que só cresce, perda de visibilidade quando algo dá errado, e nenhuma forma de retomar uma pipeline interrompida com contexto visual do que já foi feito.

Esta entrega substitui a tabela `events` por arquivos NDJSON per-spec append-only (`.claude/spec/{name}/[wave-N-{role}/]events/{ts-ns}-{run-id}-{pid}.ndjson`) com blob spill content-addressed (`blobs/{ab}/{sha256}.bin`, threshold 4KB) para payloads grandes (Read de arquivo inteiro, transcript de subagent, output longo de Bash). Eventos de ciclo de vida da pipeline (`pipeline.status/phase/scope`) continuam em SQLite numa nova mini-tabela `pipeline_events` (necessária para queries rápidas tipo "listar specs ativas"). O dashboard recebe um rewrite: `SpecTimelineTab` e `PipelineTimeline` passam a mostrar lista flat de `tool calls` no padrão claude-devtools — ícone, label, tokens in/out, duração, status dot; expand revela `Input` e `Output` renderizados por tool (Bash → terminal, Read → Code/Preview toggle, Edit → diff, Glob/Grep → lista, Task → execution trace recursiva do subagent). Live tail via `notify-rs` no `src-tauri` emite eventos Tauri que o React consome para atualizar em tempo real. Sessões sem spec ativa moram em `.claude/.session/{slug}/events/` e têm sua própria área no dashboard com sidebar.

### Usuários/Stakeholders

Rubens (único usuário hoje), via dashboard Tauri rodando local. Hooks de `mustard-rt` que disparam em PreToolUse/PostToolUse/SubagentStart/SubagentStop durante cada pipeline. O usuário que revisa o que um subagent fez, retoma uma pipeline depois de Ctrl+C, ou audita se a wave está sendo feita corretamente.

### Métrica de sucesso

1. **Hot path mais rápido:** latência de escrita do hook cai de ~100-500µs (SQLite open+insert+commit) para <30µs (file append) — medível em `mustard-rt run metrics`.
2. **Visibilidade total:** dashboard mostra cada tool call de uma spec com input completo, output renderizado por tool, e recursão de subagent expansível.
3. **Recuperação após interrupção:** Ctrl+C → reabrir dashboard → timeline persistida com tudo que foi feito é visível, sem perder contexto.
4. **Sem inchaço:** `mustard.db` para de crescer com eventos; `/mustard:spec clear` libera o disco de specs antigas preservando documentação.

### Não-Objetivos

- Migração de dados da tabela `events` antiga — sem usuários em produção, drop limpo (`feedback_no_migration_dev_phase`).
- Janitor automático de retenção — substituído por `/mustard:spec clear` manual com `--dry-run` default.
- Cross-spec timeline (visão única atravessando todas as specs) — foco per-spec.
- Mudanças em `telemetria.db` — escopo separado, já está bem.
- LLM-driven label generation ou semantic search sobre eventos — zero custo de token AI.
- FTS5 textual sobre eventos crus — busca textual rara, não vale a complexidade; eventos lifecycle (em `pipeline_events`) ficam queryable em SQL.
- Indexes/queries SQL sobre o stream de eventos por-spec — leitura sequencial dos NDJSON é o caminho; merge por timestamp basta.

### Acceptance Criteria

AC autoritativos vivem em cada `wave-N-*/spec.md`. `/mustard:qa` agrega no momento da execução. AC globais agregados:

- [ ] AC-G1: `cargo build --workspace && cargo clippy --workspace -- -D warnings` passa após todas as waves — Command: `cargo build --workspace && cargo clippy --workspace -- -D warnings`
- [ ] AC-G2: `pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard lint` passa — Command: `pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard lint`
- [ ] AC-G3: Após uma pipeline real escrever eventos, `.claude/spec/{spec-ativa}/events/` contém pelo menos um `.ndjson` e o dashboard mostra a timeline em tempo real — Command: `node -e "const fs=require('fs');const p=require('path');const root='.claude/spec';const any=fs.readdirSync(root).some(s=>{const e=p.join(root,s,'events');return fs.existsSync(e)&&fs.readdirSync(e).some(f=>f.endsWith('.ndjson'))});process.exit(any?0:1)"`
- [ ] AC-G4: Tabela `events` foi dropada de `mustard.db` — Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema events\"',{encoding:'utf8'}).trim();process.exit(out===''?0:1)"`
- [ ] AC-G5: Tabela `pipeline_events` existe em `mustard.db` — Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema pipeline_events\"',{encoding:'utf8'});process.exit(out.includes('CREATE TABLE')?0:1)"`
- [ ] AC-G6: Tabela `sessions` existe em `mustard.db` — Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema sessions\"',{encoding:'utf8'});process.exit(out.includes('CREATE TABLE')?0:1)"`
- [ ] AC-G7: `mustard-rt run spec-clear --help` lista as flags `--dry-run`, `--apply`, `--all`, `--name`, `--age-days` — Command: `rtk mustard-rt run spec-clear --help`

## Plano

### Critique Coverage

Cada preocupação levantada na grelhada vira: (a) wave coberta, (b) Não-Objetivo justificado, ou (c) decisão de design já explicitada no Contexto.

| Crítica / pedido da grelhada | Onde está |
|---|---|
| Cache rápido pesquisável entre agents | Wave 1 (ndjson append) + Wave 2 (reader) |
| Persiste se fechar Claude Code ou Ctrl+C | Wave 1 (append linha-a-linha; durável por write atômico) |
| SQLite vai inchar e ficar ruim | Wave 1 (drop tabela `events`; só `pipeline_events` mini fica) |
| Por spec, performático | Wave 1 (per-spec layout + append direto; zero contenção) |
| Memória em disco por spec em Rust | Wave 1 (Rust pure, sem deps externas, sem KV embeddable) |
| Timeline por spec, ver fase | Wave 3 (rewrite com grouping por wave) |
| Input/output enviado e recebido | Wave 1 (shape) + Wave 3 (expand renderers) |
| Diff dos arquivos | Wave 3 (`EditRenderer` com old/new) |
| Estilo claude-devtools | Wave 3 (lista flat + expand + per-tool renderers) |
| Recursão de subagent (Task) | Wave 1 (`parent_id`) + Wave 3 (`TaskRenderer` recursivo) |
| Sessions sidebar (sem spec) | Wave 4 (`sessions` table + `.claude/.session/{slug}/`) |
| 15 dias retenção post-close | Wave 5 (`--age-days 15` default) |
| Manual via comando, sem janitor auto | Wave 5 (`--dry-run` default, `--apply` explícito) |
| Zero token AI cost | Não-Objetivo (constraint validada — Rust + libs client-side) |
| Sem migração (dev only user) | Não-Objetivo + Wave 1 (drop limpo) |
| `telemetria.db` separado | Não-Objetivo (sem mudança) |
| `mustard.db` leve | Wave 1 (events out) + Wave 4 (sessions tiny) |
| Live tail (realtime) | Wave 3 (`notify-rs` no src-tauri) |

## Tabela de Waves

| Wave | Spec | Role | Depende de | Resumo |
|------|------|------|------------|--------|
| 1 | [[wave-1-library]] | library | — | RT writer per-spec NDJSON + blob spill + `parent_id` + nova `pipeline_events` + drop tabela `events` |
| 2 | [[wave-2-library]] | library | [[1]] | Core reader NDJSON + atualização de `projection/timeline.rs` + shape novo no `model/view/timeline.rs` + `rebuild-specs` |
| 3 | [[wave-3-ui]] | ui | [[2]] | Dashboard claude-devtools-style: rewrite `SpecTimelineTab` + `PipelineTimeline` + per-tool renderers + recursão de Task + live tail `notify-rs` |
| 4 | [[wave-4-general]] | general | [[2]] | `sessions` table + `.claude/.session/{slug}/` + rota `Sessions.tsx` + sidebar |
| 5 | [[wave-5-general]] | general | [[1]] | `/mustard:spec clear` (manual, `--age-days 15` default) |
