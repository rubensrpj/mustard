# Plano de ondas — Unificação do Mustard

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: full (wave plan)
### Checkpoint: 2026-05-24T19:00:00Z
### Lang: pt-BR
### Total waves: 14

## Contexto

Esta unificação consolida três specs ativas (`2026-05-24-meta-sidecar`, `2026-05-24-config-idioma-tom`, `2026-05-23-per-spec-event-log-claude-devtools`), fecha review/qa pendentes de `2026-05-22-telemetry-separation`, e fecha o ciclo da refatoração Rust do Mustard. PRD em `spec.md`; plano completo em `C:\Users\ruben\.claude\plans\o-mustard-vem-passando-humble-whale.md`.

A topologia espelha a arquitetura: cada onda toca um eixo claro (clippy, scan, schema, idioma, eventos, subcomandos, templates, memória, contexto, triggers, validation, telemetria, fechamento). Onde duas ondas podem rodar em paralelo, está marcado.

## Diagrama de dependências

```
W0 stop-the-bleeding   (independente — desbloqueia tudo)
   ↓
W1 worktree-gc    ||    W2 scan-cold-path-bootstrap
                              ↓
W3 spec-meta-sidecar    (gargalo — entrega meta.json lateral)
                              ↓
W4 language-and-tone    ||    W5 mustard-db-redesign + per-spec-event-log + dashboard-fast
                              ↓
W6 rt-new-subcommands   (12+3 subcomandos novos — pré-requisito de W7)
                              ↓
W7 templates-cuts + opt-in-skills    ||    W8 shared-memory-hardening
                              ↓
W9 context-injection-optimization
                              ↓
W10 stop-and-notification-triggers    ||    W11 verify-pipeline-multistack
                              ↓
W12 telemetry-perf-followup + economy-dashboard-wiring
                              ↓
W13 close-and-archive
```

## Tabela de ondas

| # | Spec | Role | Depende de | Resumo |
|---|---|---|---|---|
| 0 | [[wave-0-mixed]] | mixed | — | Bloqueadores: clippy unused import (corrigido nesta sessão); `docs-stale-check` exclude `**/worktrees/**`; `verify-pipeline` stack-aware (Rust 600s, TS 120s, Python 180s); remoção do worktree órfão `.claude/worktrees/agent-a19b5122f2df4ee44/`. |
| 1 | [[wave-1-rt]] | rt | [[0]] | Novo subcomando `mustard-rt run worktree-gc` (enumera `agent-*`, remove >7d, emite `worktree.gc.run`). Hook `SessionStart` ganha chamada idempotente fail-open com warning se houver mais que N órfãos. |
| 2 | [[wave-2-rt]] | rt | [[0]] | Cold-path interpret reescrito para subprocess do `claude` CLI (sem SDK Anthropic, sem `ANTHROPIC_API_KEY`). `doctor.rs` valida presença do binário. Regression test com fake binary. Regenerar `entity-registry.json` com `entities[]` não-vazio. |
| 3 | [[wave-3-rt]] | rt | [[2]] | Absorve `2026-05-24-meta-sidecar` integralmente: schema `meta.json` em `packages/core/src/meta.rs`, leitor+escritor, `pipeline_state_ingest` lê JSON, `migrate-to-meta` one-shot, dashboard `read_spec_meta` Tauri command, remoção dos headers `### X:` do `.md`, simplificação `spec_sections.rs`. |
| 4 | [[wave-4-mixed]] | mixed | [[3]] | Absorve `2026-05-24-config-idioma-tom` integralmente: módulo `packages/core/src/i18n.rs` (enum `Locale { PtBr, EnUs }`, `Tone`, `translate`, `apply_tone`, `slugify`), schema `lang`+`tone` em `mustard.json` (BCP-47), refactor banners em `apps/rt/src/**`, refactor CLI em `apps/cli/src/commands/**`, dashboard Settings page, `spec_slug.rs` lang-aware, novo `i18n translate-heading` + `spec-lang resolve`. |
| 5 | [[wave-5-mixed]] | mixed | [[3]] | Absorve `2026-05-23-per-spec-event-log-claude-devtools` + redesign full do `mustard.db`. Tasks T5.1 (EventSink NDJSON + blob spill + `pipeline_events`), T5.2 (core reader), T5.3 (dashboard timeline claude-devtools-style), T5.4 (`sessions` table + sidebar), T5.5 (`spec-clear` cmd), T5.6 (mustard.db schema refeito do zero — drop `events`/`events_fts`/`knowledge`/`metrics_projection`; CREATE direto de `agent_memory`+`memory_feedback`; índices auditados; VACUUM), T5.7 (remover grafo interno do dashboard; wikilinks via `obsidian://` URI; lista virtualizada `<200ms`/100 specs), T5.8 (events `pipeline.economy.event.written` visíveis em `/economia`). |
| 6 | [[wave-6-rt]] | rt | [[4]], [[5]] | 12+3 subcomandos novos em `apps/rt/src/run/`: `spec-scaffold`, `close-orchestrate`, `review-dispatch`, `tactical-fix-create`, `prd-build`, `skill-fetch`, `skill-cache`, `adapt-cursor`, `maint-deps`, `maint-validate`, `task-checklist`, `bugfix-cache`, `context-budget`, `backup-specs`, `worktree-gc`, `migrate-to-meta`, `i18n translate-heading`, `spec-lang resolve`, `economy capture-baseline`, `economy reconcile`, `economy report`. Cada um segue `rt-run-subcommand-pattern`. |
| 7 | [[wave-7-cli]] | cli | [[6]] | Cortes nos `SKILL.md` (≤1100 linhas no total): `feature` 353→90, `bugfix` 240→70, `close` 220→50, `review` 178→60, `prd` 167→30, `skill` 161→60, `tactical-fix` 135→40, `task` 131→70, `qa` 115→40, `knowledge` 118→50, `maint` 104→40, `spec` 157→120. Tradução en-US consistente. Refs novos extraídos. Skills opt-in (`hallmark`, `design-craft`, `react-best-practices`, `grill-me` + skill-creator subdirs) movidas para `templates-extras/skills/`. `mustard add skill:nome` instala via `skill-fetch`. Mensagem final em `mustard init` lista extras. Adapter Cursor em Rust (W6 entregou; W7 remove o `.js`). |
| 8 | [[wave-8-rt]] | rt | [[5]], [[6]] | Memória cross-session: tabela `agent_memory` (id, session_id, spec, wave, role, summary, details, confidence, status, at, last_used) + FTS5. Tabela `memory_feedback` (depreciate/bump/supersede/use). Subcomandos `memory search`, `memory feedback`, `memory write --verify`. `memory-ingest --agent-memory` migra `.claude/.agent-memory/_index.json` para SQLite. Lazy decay on read. Filtro padrão `spec=current OR (spec IS NULL AND confidence>=0.8)`. |
| 9 | [[wave-9-rt]] | rt | [[8]] | Injeção otimizada: `SessionStart` scope-by-spec (3 spec + 2 globais); `UserPromptSubmit` adiciona 1 linha "Pipeline em curso" quando não é `/mustard:*`; novo hook `subagent_inject` para `Task` sem SKILL (slice mínimo); `PostToolUse(Task)` observer `auto_capture_summary` (parse `<MEMORY>` ou `Resumo:`); `SubagentStop` observer (bump `last_used`); `SessionEnd` consolidação `agent_memory → memory_decisions/lessons`; `PreCompact` adiciona até 3 `agent_memory` recentes. `context-slice` estendido para `CLAUDE.md`. Flag `--budget-tokens` em `agent-prompt-render`. |
| 10 | [[wave-10-rt]] | rt | [[6]] | Triggers `Stop` e `Notification` modelados em `Trigger` enum. Hook `stop` persiste `agent_memory` `summary="interrupted at wave N"` se houve edit recente (anti-spam 5min). Hook `notification` registra `notification.received` (sem auto-resolve). |
| 11 | [[wave-11-rt]] | rt | [[0]] | `verify-pipeline` multi-stack: lê `sync-detect`, dispara N verifications em paralelo (rayon). Saída JSON `{ overall, per_subproject, total_duration_ms }`. Timeouts por stack via env. |
| 12 | [[wave-12-mixed]] | mixed | [[6]], [[9]] | Fechar review+qa pendentes de `2026-05-22-telemetry-separation`. Audit de queries hot do dashboard (`EXPLAIN QUERY PLAN`). Estender `db-maintain` com `--telemetry-only` e `--prune-older-than`. Tabelas `economy_baselines` + `economy_savings` em `telemetry.db`. Subcomandos `economy capture-baseline/reconcile/report` (entregues em W6) ganham wire ao dashboard. Página `/economia` recebe aba "Mustard Unification Savings" (card total + tabela per-wave + sparkline). 5 ACs de metrification globais. |
| 13 | [[wave-13-mixed]] | mixed | [[0]]..[[12]] | Backup `~/.mustard-backups/2026-05-24-pre-unification/MANIFEST.json` com checksum SHA-256. Emit `pipeline.status: archived` para todas as ~55 specs fechadas + 4 absorvidas. ADR única em `docs/adr/2026-05-24-mustard-unification.md`. Vault Obsidian (`.claude/graph/index.md`) ganha index das ADRs. Relatório final de tokens economizados via `/economia` + tamanho final do `mustard.db`. |

## Paralelização

| Janela | Pode rodar em paralelo |
|---|---|
| Após W0 | W1 + W2 |
| Após W3 | W4 + W5 |
| Após W6 | W7 + W8 |
| Após W9 | W10 + W11 |

W3 e W6 são gargalos (gates de sincronização). W12 e W13 são sequenciais.

## Cobertura — críticas e pedidos do usuário (cross-check)

Lista exaustiva do que foi pedido durante a conversa de planejamento, mapeada para a onda que entrega:

| Pedido / crítica | Onde resolve |
|---|---|
| Backup de specs antes de mexer | W0 dispara `backup-specs`; subcomando entregue em W6; AC-G14 |
| Absorver `meta-sidecar` | W3 |
| Absorver `config-idioma-tom` | W4 |
| Absorver `per-spec-event-log-claude-devtools` | W5 (T5.1-T5.5) |
| Deixar `dashboard-prd-ai-lapidator` separada | declarado em PRD, não absorvido |
| Aposentar `economia-moat-unification` como superseded | W13 |
| Fix clippy unused import `session_start.rs:754` | W0 (aplicado nesta sessão; também corrigiu 3 collapsible-if em `unhook.rs`) |
| Remover worktree órfão | W0 (manual) + W1 (sistêmico via `worktree-gc`) |
| Fix `docs-stale-check` exclude worktrees | W0 |
| `verify-pipeline` stack-aware (`npm test` quebrando em Rust) | W0 (fix mínimo) + W11 (generalização multi-stack) |
| `entity-registry.json` vazio (cold-path sem chave) | W2 (subprocess `claude` CLI; sem `ANTHROPIC_API_KEY`) |
| LLM call sempre via `claude` CLI, nunca SDK | W2 (cold-path); regra padrão para qualquer LLM call futuro |
| `mustard.db` inchando | W5.T5.1 (drop `events`) + W5.T5.6 (schema refeito do zero) |
| `telemetry.db` perf + review/qa pendentes | W12 |
| Tudo metrificável em `/economia` | W5.T5.8 + W6 (3 subcomandos `economy *`) + W12 (tabelas + dashboard wiring); AC-G12 |
| Dashboard lento na área de specs | W5.T5.7 (remove grafo interno; wikilinks via `obsidian://`; lista virtualizada) |
| Timeline spec/wave estilo claude-devtools | W5.T5.3 (rewrite + per-tool renderers + recursão Task) |
| Locales pt-BR/en-US BCP-47 | W4 (enum `Locale { PtBr, EnUs }`); AC-G7 |
| Memória cross-session entre agentes | W8 (agent_memory + feedback + scope) + W9 (auto-capture + inject) |
| Otimizar carga de contexto inicial | W9 (`context-slice` em `CLAUDE.md`, `--budget-tokens`, `context-budget`) |
| Eventos Claude Code não-cobertos (Stop/Notification) | W10 |
| Reanálise + corte de todos os 18 commands + 13 skills + 24 refs + 5 recipes + settings.json + CLAUDE.md | W7 (cobertura total tabulada no plano aprovado) |
| Remover vinculos bun/JavaScript | W6 (adapt-cursor Rust) + W7 (limpeza final dos `.md`) |
| Padronizar idioma conforme `mustard.json#lang` | W4 (i18n.rs) + W7 (SKILL.md em en-US; `i18n.key()` para banners pt-BR) |
| Templates direcionados à própria IA | W7 (cortes; refs progressivos) + W9 (slicing) |
| Skills 3rdparty como auxílio opt-in | W7 (`mustard add skill:nome`); AC-G10 |
| Recursos Rust para evitar tokens | W6 (12+3 subcomandos) + W7 (cortes) + W9 (slicing); tabela "Economia de IA" no plano |
| Análise cruzada entre frentes | seção "Consistência cruzada" do plano (9 arquivos compartilhados, sequência segura) |
| Cada comando eficiente | W6 + W7 |

## Files

Lista canônica dos arquivos tocados (consolidado por onda; cada `wave-N-{role}/spec.md` detalha):

**packages/core/**:
- `src/store/sqlite_schema.sql` (W5.T5.6 — refeito; W8 — DDL adicionada)
- `src/store/sqlite_store.rs` (W5.T5.1)
- `src/projection/timeline.rs` (W5.T5.2)
- `src/model/view/timeline.rs` (W5.T5.2)
- `src/model/contract.rs` (W10 — Trigger enum)
- `src/meta.rs` (W3 — schema)
- `src/i18n.rs` (W4 — módulo central, novo)
- `src/telemetry/schema.sql` (W12 — `economy_baselines`+`economy_savings`)

**apps/rt/**:
- `src/hooks/session_start.rs` (W0 fix; W8 scope-by-spec; W9 resume block)
- `src/hooks/{subagent_inject,auto_capture_summary,memory_write_verify,stop,notification}.rs` (W8/W9/W10 — novos)
- `src/registry.rs` (W5/W8/W9/W10 — registrar)
- `src/run/scan/interpret.rs` (W2 — claude CLI subprocess)
- `src/run/doctor.rs` (W2 — check binário claude)
- `src/run/docs_stale_check.rs` (W0 — exclude worktrees)
- `src/run/verify_pipeline.rs` (W0 fix; W11 multi-stack)
- `src/run/unhook.rs` (W0 fix collapsible-if — aplicado)
- `src/run/db_maintain.rs` (W12 — flags)
- `src/run/memory.rs` (W8 — search/feedback/write-verify)
- `src/run/agent_prompt_render.rs` (W4/W7/W9)
- `src/run/migrate_spec_headers.rs` (W3 — referência para `migrate-to-meta`)
- `src/run/{spec_scaffold,close_orchestrate,review_dispatch,tactical_fix_create,prd_build,skill_fetch,skill_cache,adapt_cursor,maint_deps,maint_validate,task_checklist,bugfix_cache,context_budget,backup_specs,worktree_gc,migrate_to_meta,i18n_translate,spec_lang_resolve,economy_capture_baseline,economy_reconcile,economy_report,event_writer_ndjson,blob_spill,spec_clear}.rs` (W6/W5/W8 — novos)
- `src/run/context_slice.rs` (W9 — estender para CLAUDE.md)
- `src/run/mod.rs` (W6 — registrar subcomandos novos)

**apps/cli/**:
- `src/commands/add.rs` (W7 — tipo "skill")
- `src/commands/init.rs` (W7 — mensagem final extras)
- `templates/commands/mustard/{feature,bugfix,close,review,prd,skill,tactical-fix,task,qa,knowledge,maint,spec,scan,stats,status,git,unhook,rehook}/SKILL.md` (W7 — cortes)
- `templates/refs/feature/{analyze,plan,execute}-protocol.md` (W7 — novos)
- `templates/refs/{task/action-bridge,task/domain-checklists,knowledge/capture-at-close,bugfix/diagnose-protocol,bugfix/retry-cache}.md` (W7 — novos)
- `templates/skills/{hallmark,design-craft,react-best-practices,grill-me}/` → `templates-extras/skills/` (W7 — mover)
- `templates/skills/skill-creator/{scripts,agents,assets,eval-viewer,references}/` → `templates-extras/skills/skill-creator/` (W7 — mover)
- `templates/adapters/cursor/adapter.js` → eliminado (W6 entrega `adapt-cursor` Rust)
- `mustard.json` (W4 — schema `lang`+`tone`)

**apps/dashboard/**:
- `src/components/SpecTimelineTab.tsx` + `PipelineTimeline.tsx` + per-tool renderers + `Sessions.tsx` (W5.T5.3)
- `src/pages/Specs.tsx` (W5.T5.7 — sem grafo, virtualizada)
- `src/pages/Economia.tsx` (W12 — aba unification savings)
- `src-tauri/src/main.rs` (W5 — notify-rs + shell.open Obsidian)
- `src-tauri/src/commands/specs.rs` (W3 — `read_spec_meta`)
- `package.json` (W5.T5.7 — remover dep força-grafo; W7 — talvez adicionar `react-virtuoso`)

## Não-Objetivos (ondas)

- Não reabsorver código entregue de `2026-05-22-project-profiler` (citação consumida, não reimplementação).
- Não fazer migration formal de dados — fase dev, drop limpo.
- Não tocar UI do PRD lapidador (escopo separado).
- Não introduzir Anthropic SDK em Rust.

## Riscos eliminados por design

| Risco potencial | Eliminação |
|---|---|
| Conflito de schema SQLite (multi-wave editando `sqlite_schema.sql`) | W5.T5.6 reescreve do zero ANTES de W8 (consistência cruzada) |
| Quebra de specs em flight com migração de header | Padrão fase-dev: drop limpo; specs antigas viram backup |
| Dashboard ficar sem dado durante refactor | W5.T5.7 substitui grafo por lista mantendo todos os outros componentes |
| Cold-path scan parar de funcionar por falta de chave | W2 muda para `claude` CLI (sempre presente para o user) |
| Métrica de economia ficar estimada vs real | W12 exige `economy_savings` populado por wave (AC-G12 + 5 ACs Metric internos) |
