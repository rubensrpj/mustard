# Plano de ondas — Mustard Deep Refactor

## Contexto

Consolida resíduo da `2026-05-24-mustard-unification` + reescreve os 3 pilares (spec creation + injeção entre agentes + montagem de skills) + faxina arquitetural. 136 specs históricas foram movidas para `~/.mustard-backups/2026-05-25-specs-archive/`. Apenas esta spec é ativa.

## Diagrama de dependências

```
W0 residual-w5 (encerra T5.2-T5.8 da mega-spec)
  ↓
W1 spec-injection-skills-refactor (gargalo arquitetural — os 3 pilares)
  ↓
W2 claude-dir-prune  ||  W3 scan-rust-first-agnostic  ||  W4 archive-completed-specs
  ↓
W5 rt-new-subcommands
  ↓
W6 templates-cuts  ||  W7 shared-memory-hardening
  ↓
W8 context-injection-optimization
  ↓
W9 stop-notification-triggers  ||  W10 verify-pipeline-multistack + wave-integrity-doctor
  ↓
W11 telemetry-perf + economy-wiring
  ↓
W12 close-and-archive
```

W1 é o gargalo principal — destrava W6, W7, W8 reduzindo escopo delas.
W4 (archive) pode rodar em paralelo a tudo — só emite eventos.

## Tabela de ondas

| # | Spec | Role | Depende de | Resumo |
|---|---|---|---|---|
| 0 | [[wave-0-mixed]] | mixed | — | Encerra T5.2 (core reader NDJSON), T5.3 (dashboard timeline claude-devtools), T5.4 (sessions table + sidebar), T5.5 (spec-clear cmd), T5.6 (mustard.db schema refeito), T5.7 (remover grafo + wikilinks obsidian — parcial), T5.8 (economy events). T5.1 (EventSink) já entregue. |
| 1 | [[wave-1-mixed]] | mixed | [[0]] | **3 pilares**: (A) `spec-draft` Rust gera spec.md+meta.json+wave-plan; (B) `skill-resolve` Rust matching determinístico; (C) frontmatter padronizado de skill. Refatorar `agent-prompt-render` para usar skill-resolve. Migrar 13 skills foundation. Validators `spec-validate`/`skill-validate --strict-frontmatter`. |
| 2 | [[wave-2-mixed]] | mixed | [[0]] | Limpeza profunda da `.claude/` raiz. T2.1 já feito manualmente nesta sessão (8 paths movidos). Restam: subcomando `claude-dir-prune`, janitor SessionStart, contrato canônico em CLAUDE.md. |
| 3 | [[wave-3-mixed]] | mixed | [[0]] | `/scan` Rust-first agnóstico. Etapa 1 `scan-structural` (Rust puro): stack.md de manifests, cluster_discovery cobertura ≥99% subprojetos, entity-registry expandido via parsers AST-leve, recipes derivadas mecanicamente, refs stack-aware install, graph nodes pipeline. Etapa 2 `scan-interpret` (IA enxuta, prompt ≤80 linhas). Etapa 3 `scan-md-validate` + `scan-recipes-validate` (Rust gate pós-IA). Zero hardcode de stack. |
| 4 | [[wave-4-rt]] | rt | — | Adicionar variante `Absorbed` ao enum `Outcome` em `packages/core/src/meta.rs`. Emit `pipeline.status` events para arquivar todas as 136 specs movidas (catalogadas em MANIFEST). |
| 5 | [[wave-5-rt]] | rt | [[1]], [[3]] | Subcomandos `mustard-rt run`: `spec-scaffold` (resíduo W1), `close-orchestrate`, `review-dispatch`, `tactical-fix-create`, `prd-build`, `skill-fetch`, `skill-cache`, `adapt-cursor`, `maint-deps`, `maint-validate`, `task-checklist`, `bugfix-cache`, `context-budget`, `backup-specs`, `migrate-to-meta`, `economy capture-baseline/reconcile/report`. |
| 6 | [[wave-6-cli]] | cli | [[1]] | Cortes nos `commands/mustard/*/SKILL.md` (≤67 linhas média; total ≤800). Cortar `pipeline-config.md` 489→200, `refs/scan/scan-protocol.md` 368→180, `refs/git/merge-protocol.md` 277→150. Sweep refs antigas (`### Stage:`, `Lang: pt`, `spec/active/`, scripts JS). Skills opt-in (`hallmark`, `design-craft`, `react-best-practices`, `grill-me`) para `templates-extras/`. Refs stack-aware (`browser-debug`, `fe-craft-check`) para `templates/refs/stack-templates/`. |
| 7 | [[wave-7-rt]] | rt | [[1]], [[5]] | Memória cross-session: tabela `agent_memory` + FTS5. Tabela `memory_feedback`. Subcomandos `memory search/feedback/write --verify`. Migração de `.claude/.agent-memory/` (legacy). Lazy decay. Filtro `spec=current OR (spec IS NULL AND confidence>=0.8)`. **Aproveita W1**: scope-by-cluster (não só por spec) usando `appliesTo` do frontmatter padronizado. |
| 8 | [[wave-8-rt]] | rt | [[1]], [[5]], [[7]] | Injeção otimizada: `SessionStart` scope-by-spec; `UserPromptSubmit` 1 linha "Pipeline em curso"; novo hook `subagent_inject`; `PostToolUse(Task)` observer `auto_capture_summary`; `SubagentStop` bump `last_used`; `SessionEnd` consolida memória; `PreCompact` inclui 3 `agent_memory` recentes. **Aproveita W1**: usa `skill-resolve` como signal de relevância no `context-slice`. |
| 9 | [[wave-9-rt]] | rt | [[5]] | Triggers `Stop` e `Notification` modelados em `Trigger` enum (`packages/core/src/model/contract.rs`). Hook `stop` persiste `agent_memory` `summary="interrupted at wave N"` (anti-spam 5min). Hook `notification` registra `notification.received`. |
| 10 | [[wave-10-mixed]] | mixed | [[0]], [[5]] | `verify-pipeline` multi-stack paralelo (rayon, JSON `{overall, per_subproject, total_duration_ms}`, timeouts por env). **Absorve `wave-integrity-and-doctor-check`**: hard gate em `wave-scaffold`, novo `plan-from-spec` (Rust monta JSON), check `wave-integrity` no doctor, flag `--json`, Tauri command `doctor_status`, `DoctorBadge` na Sidebar do dashboard. |
| 11 | [[wave-11-mixed]] | mixed | [[5]], [[8]] | Fechar review+qa pendentes de `telemetry-separation` (movida para backup). Audit queries hot do dashboard (`EXPLAIN QUERY PLAN`). Estender `db-maintain` com `--telemetry-only` e `--prune-older-than`. Tabelas `economy_baselines` + `economy_savings` em `telemetry.db`. Subcomandos `economy` (W5) ganham wire ao dashboard. Página `/economia` recebe aba "Deep Refactor Savings". |
| 12 | [[wave-12-mixed]] | mixed | [[0]]..[[11]] | Backup `~/.mustard-backups/2026-05-25-pre-refactor/` com SHA-256. ADR única em `docs/adr/2026-05-25-mustard-deep-refactor.md`. Vault Obsidian `.claude/graph/index.md` resync. Relatório final via `/economia`: tokens economizados + tamanho final `mustard.db`. Memory consolidation. |

## Paralelização

| Janela | Pode rodar em paralelo |
|---|---|
| Após W0 | W1 + W2 + W3 + W4 (escopos disjuntos) |
| Após W1 | W5 + W6 |
| Após W5 | W7 + W6 (se ainda não fechou) |
| Após W7 | W8 |
| Após W8 | W9 + W10 |
| Após W10 | W11 |
| Sequencial | W11 → W12 |

W1 é gargalo (gate de sincronização para W6/W7/W8).
W5 é gargalo (gate de sincronização para W7/W8/W9/W10).
W12 é sequencial (fechamento).

## Cobertura — críticas e pedidos do usuário

| Pedido / crítica | Onde resolve |
|---|---|
| Mustard agnóstico — nada hardcoded de stack | W1 (skill-resolve agnóstico), W3 (scan-structural sem catálogos) — [[feedback_no_hardcoded_stack_patterns]] |
| Cobertura entity-registry de 6% para 99% | W3 (cluster_discovery em todos subprojetos + parsers AST-leve agnósticos) |
| LLM call sempre via `claude` CLI, nunca SDK | Herdado da mega-spec W2 ✅ |
| `mustard.db` redesenhado, sem inchaço | W0 (T5.6 residual da mega-spec) |
| Dashboard sem grafo interno | W0 (T5.7 residual) |
| Timeline claude-devtools-style | W0 (T5.3 residual) |
| Recipes geradas pelo scan, nunca hardcoded | W3 (`scan-recipes-extract` derivado) — [[feedback_recipes_from_scan]] |
| Graph com escopo de pipeline (spec/skill/command/ref/recipe/conv) | W3 — [[feedback_graph_pipeline_knowledge]] |
| Templates `.md` enxutos sem refs legadas (bun/JS) | W6 (sweep + cortes) — [[feedback_templates_md_moat]] |
| Limpeza profunda da `.claude/` raiz | W2 (parte feita manualmente nesta sessão) — [[feedback_claude_dir_audit]] |
| Apenas spec nova ativa no picker | Backup massivo executado nesta sessão; W4 emite eventos archived |
| Memória cross-session com escopo por cluster | W7 (agent_memory + appliesTo) |
| Injeção entre agentes determinística | W1 (skill-resolve) + W8 (context-slice usando W1) |
| Comandos enxutos | W1 (spec-draft remove ~80 linhas de template) + W6 (cortes finais) |
| Skill frontmatter padronizado | W1 (T1.3 schema + migração foundation) |
| Validação Rust pós-IA | W3 (scan-md-validate) + W1 (spec-validate, skill-validate --strict) |
| Tudo metrificável em `/economia` | W11 (tabelas + wire) + W5 (subcomandos economy) |
| Hooks Stop/Notification cobertos | W9 |
| Doctor com check de integridade de wave | W10 (absorve `wave-integrity-and-doctor-check`) |
| Specs históricas fora do picker | Backup executado; W4 catalogadas em MANIFEST |

## Não-Objetivos (ondas)

- Reescrever cold-path `scan/interpret.rs` (W2 mega-spec ✅).
- Migrar dados de `mustard.db` antigos — drop limpo.
- Tocar UI do PRD lapidador (`dashboard-prd-ai-lapidator` — Cancelled nesta sessão).
- Tocar instalador multi-SO (`mustard-v1-installer-and-update` — Cancelled nesta sessão).
- Hardcodear catálogo de padrões esperados.
- Estimar economia em prosa — `economy_savings` real ou nada.

## Riscos eliminados por design

| Risco | Eliminação |
|---|---|
| W7/W8 memória/injeção dessincronizados com nova arquitetura de W1 | W1 entrega ANTES; W7/W8 dependem dela e aproveitam |
| Conflito de schema SQLite multi-wave | W0.T5.6 reescreve do zero ANTES de W7 |
| Specs históricas continuam aparecendo no picker | Backup massivo executado nesta sessão (136 → backup); W4 catalogadas |
| Cluster_discovery deixar subprojetos invisíveis | W3 AC mede cobertura por subprojeto detectado |
| Recipes degenerarem em hardcoded de novo | W3 valida ausência de placeholders literais (`{Entity}`, "find by entity name") |
| Refs ficarem defasados com cabeçalhos extintos | W6 sweep com validator Rust |
