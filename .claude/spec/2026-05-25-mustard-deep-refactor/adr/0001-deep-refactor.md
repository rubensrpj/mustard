# ADR 0001 — Mustard Deep Refactor (2026-05-25)

- Status: Aceito
- Data: 2026-05-25 → 2026-05-26 (fechamento)
- Spec canônica: [[../spec]] (`.claude/spec/2026-05-25-mustard-deep-refactor/spec.md`)
- Wave plan: [[../wave-plan]]
- Escopo: 13 ondas (W0 a W12) — [[../wave-0-rt/spec|W0]] · [[../wave-1-rt/spec|W1]] · [[../wave-2-mixed/spec|W2]] · [[../wave-3-mixed/spec|W3]] · [[../wave-4-rt/spec|W4]] · [[../wave-5-rt/spec|W5]] · [[../wave-6-cli/spec|W6]] · [[../wave-7-rt/spec|W7]] · [[../wave-8-rt/spec|W8]] · [[../wave-9-rt/spec|W9]] · [[../wave-10-mixed/spec|W10]] · [[../wave-11-mixed/spec|W11]] · [[../wave-12-mixed/spec|W12]]
- Idioma: pt-BR (narrativa) — código sempre EN
- Predecessores absorvidos: `2026-05-24-mustard-unification` (W0-W4 entregues; W5 residual + W6-W17 migrados); 136 specs históricas movidas para `~/.mustard-backups/2026-05-25-specs-archive/`

## Contexto

Após seis meses evoluindo em camadas, o Mustard chegou a um ponto em que três pilares precisavam ser reescritos juntos para o produto ficar coerente:

1. **Criação de spec** — `/feature` carregava ~80 linhas de template literal no `SKILL.md`, decisão de escopo por prosa do LLM e resolução de idioma com formato curto (`Lang: pt`) que já não tinha mais suporte na pipeline.
2. **Injeção entre agentes** — o `agent-prompt-render` em Rust preenchia `{recommended_skills}` e `{guards_summary}` por prosa do LLM e regex frágil em `CLAUDE.md`. Resultado: variabilidade entre execuções, e skills geradas pelo `/scan` jamais chegavam ao prompt.
3. **Montagem de skills** — 13 skills foundation em `templates/skills/` com frontmatter inconsistente; skills geradas pelo scan saíam em formato livre; sem validador Rust nem matching determinístico.

Soma-se a isso o resíduo arquitetural: a `2026-05-24-mustard-unification` tinha W5 em curso + W6-W17 ainda em plano; 136 specs históricas poluíam o picker; recipes hardcoded e refs com cabeçalhos extintos (`### Stage:` em `.md` em vez de `meta.json`); grafo com nós `entity.*`/`enum.*` fora do escopo de pipeline.

Decisão: encerrar a mega-spec, arquivar tudo, abrir uma única spec deep-refactor com 13 ondas e fechá-la em uma passada. Princípios consolidados (memórias): agnóstico, scan-Rust-first, nada hardcoded de stack, recipes-from-scan, graph-pipeline-knowledge, templates `.md` como moat enxuto.

## Decisões por onda

### W0 — Resíduo da W5 da mega-spec

- `mustard.db` reescrito do zero (drop `events`/`events_fts`/`knowledge` legacy/`metrics_projection`); CREATE direto das tabelas atuais (`pipeline_events`, `sessions`, `knowledge_patterns`, `memory_decisions`, `memory_lessons` + FTS5, `agent_memory`, `memory_feedback`).
- NDJSON per-spec como hot path do event-log (`packages/core/src/projection/timeline.rs`); leitor reaproveitado por `mustard-rt run rebuild-specs`.
- Dashboard ganhou timeline claude-devtools-style, tabela `sessions` + sidebar, e perdeu o componente de grafo interno: wikilinks `[[X]]` abrem no Obsidian via `obsidian://open?vault=...`.
- `mustard-rt run spec-clear` substitui o varredor JS antigo (`--dry-run` por padrão, `--all`, `--name`, `--age-days`).
- Deferred: `T0.7` (constante de event-kind `pipeline.economy.event.written` em core/rt) — não bloqueou /economia porque W11 fez wire pelo subcomando `economy`.

### W1 — Spec creation + injeção + skills (os 3 pilares)

- Contrato de spec em Rust (`packages/core/src/spec/contract.rs`): layout obrigatório PRD → AC → Plano → Limites; `Stage`/`Outcome`/`Phase`/`Scope` como enums; AC com `Command` runnable; Lang em BCP-47.
- `mustard-rt run spec-draft` gera `spec.md` + `meta.json` + (Full) `wave-plan.md` + `wave-N-{role}/spec.md` direto. Os ~80 linhas de template literal saíram do `feature/SKILL.md`.
- Schema de frontmatter de skill (`packages/core/src/skill/frontmatter.rs`): `name`, `description`, `tags`, `appliesTo` (cluster labels), `scope`, `entities`, `metadata.generated_by`.
- `mustard-rt run skill-resolve` faz matching agnóstico em Rust (parse leve de verbo + nouns + cross com `entity-registry.json`), top-K via score determinístico. Zero IA.
- `agent-prompt-render` refatorado para consumir `skill-resolve` para `{recommended_skills}` e extração estruturada de `CLAUDE.md` para `{guards_summary}`; cache por wave.
- 13 skills foundation migradas para o schema novo; validators Rust `spec-validate` e `skills validate --strict-frontmatter`.
- Subcomando `spec-memory create` para gerar `memory/{name}.md` com wirelinks automáticos.

### W2 — Limpeza profunda da `.claude/` raiz

- 8 paths removidos manualmente nesta sessão (`scripts/`, `adapters/`, `plans/`, `agent-memory/`, etc., total 797 KB de backup em `~/.mustard-backups/2026-05-25-claude-dir-prune-manual/`).
- `mustard-rt run claude-dir-prune` agnóstico: classifica cada subdir como KEEP/STALE/ORPHAN/LEGACY com base em cross-check com rt/cli/dashboard.
- Janitor no `SessionStart`: hook chama `check_orphans()` e emite WARN sem bloquear.
- Contrato canônico em `apps/cli/templates/CLAUDE.md`: todo path em `.claude/` precisa de consumidor declarado em pelo menos uma das três subprojects.

### W3 — `/scan` Rust-first agnóstico

- `mustard-rt run scan-structural`: parser agnóstico de manifests (Cargo.toml, package.json, requirements.txt, pyproject.toml, go.mod, pom.xml, composer.json, Gemfile, *.csproj, pubspec.yaml); gera `stack.md` ≤60 linhas.
- `cluster_discovery` agora roda em **todos** subprojetos retornados por `sync-detect`.
- Gerador de recipes derivado: para cada cluster, lê 2-3 amostras, extrai imports comuns + skeleton, gera `.claude/recipes/{sub}/add-{label}.json` com paths reais (zero hardcode).
- Graph nodes restritos a `spec.X`/`skill.X`/`command.X`/`ref.X`/`recipe.X`/`conv.X` — `entity.*`/`enum.*` nunca aparecem.
- Prompt do `scan-interpret` ≤80 linhas; 3 exemplos golden por classe genérica (compiled-strongly-typed / dynamic-scripting / transpiled-typed), sem nome de tecnologia.
- Validators Rust: `scan-md-validate` (tamanho + refs + wirelinks + fence + dedup) e `scan-recipes-validate` (shape + paths existem + sem placeholders literais).
- **Deferred** (entram como `Residuals from W3` no backlog):
  - `T3.3` — Parser AST-leve agnóstico em `scan/entity_extractor.rs` (cobertura do entity-registry ainda em 6%; alvo era ≥99%).
  - `T3.5` — `scan/refs_installer.rs` (cópia de `templates/refs/stack-templates/` quando signals batem com stack).
  - `T3.7` — Wirelinks canônicos `[[{sub}.{kind}.{slug}]]` em todo cross-ref.
  - `T3.12` — Re-dispatch de `scan_finalize` quando validate falha (fail-open sem retry).

### W4 — Arquivamento das 136 specs históricas

- Variante `Absorbed` no enum `Outcome` (`packages/core/src/meta.rs`) + parsers em `apps/rt/src/run/spec_sections.rs`.
- `pipeline.status` events emitidos para cada uma das 136 specs do MANIFEST com mapeamento:
  - `2026-05-24-mustard-unification` → `Completed`
  - `2026-05-21-mustard-v1-installer-and-update`, `2026-05-20-dashboard-prd-ai-lapidator` → `Cancelled`
  - Sufixo `-SUPERSEDED` → `Superseded`
  - `config-idioma-tom`, `meta-sidecar`, `per-spec-event-log` → `Absorbed`
  - Demais ~126 → `Completed`
- Dashboard renderiza badges `Absorbed` (cinza claro), `Cancelled` (vermelho discreto), `Superseded` (laranja).

### W5 — 16 subcomandos novos no `mustard-rt run`

`close-orchestrate`, `review-dispatch`, `tactical-fix-create`, `prd-build`, `skill-fetch`, `skill-cache`, `adapt-cursor`, `maint-deps`, `maint-validate`, `task-checklist`, `bugfix-cache`, `context-budget`, `backup-specs`, `i18n`, `spec-lang`, `economy` (capture-baseline/reconcile/report), `pipeline-prelude`. Cada um segue `rt-run-subcommand-pattern` (Options + parse + run + JSON byte-stable), tem teste happy+error+shape, emite `pipeline.economy.operation.invoked`, e doc-comments em EN.

- **Parcial — AC-W5.3**: clippy verde nos 16 arquivos novos da onda; restam **18 lints residuais** em arquivos pré-existentes fora dos Limites (`scan_structural.rs`, `claude_dir_prune.rs`, `scan/interpret.rs`, `scan_md_validate.rs`). Endereçar quando algum desses arquivos for tocado.

### W6 — Cortes nos templates `.md`

- 18 `commands/mustard/*/SKILL.md` totais: **812 linhas** (alvo ≤800 — passa apenas o `qa` deveria ter 40 e tem 54; média 45 linhas — alvo era ≤67). De ~2300 linhas iniciais para 812 = **~65% de corte**.
- `pipeline-config.md` 489 → 200 linhas.
- Refs grandes encurtadas: `scan-protocol.md` 368→180, `merge-protocol.md` 277→150, `spec-language.md` 263→140.
- Sweep refs antigas: zero hits de `### Stage:`/`Lang: pt`/`spec/active|completed|superseded/`/`node scripts/`/`.mjs` em `templates/refs/` e `templates/commands/`.
- Skills opt-in (`hallmark`, `design-craft`, `react-best-practices`, `grill-me`) movidas para `apps/cli/templates-extras/skills/`; `mustard add skill:nome` instala via `skill-fetch`.
- Refs stack-aware (`browser-debug.md`, `fe-craft-check.md`) movidas para `templates/refs/stack-templates/` com frontmatter `qualifyingSignals`.
- `apps/cli/templates/adapters/cursor/adapter.js` eliminado (substituído por `mustard-rt run adapt-cursor`).

### W7 — Shared memory hardening

- Tabelas `agent_memory` + `memory_feedback` (DDL em W0) ganham lógica de write/read em `apps/rt/src/run/memory.rs` (campos: session_id, spec, wave, role, summary, details, confidence, status, at, last_used + FTS5 mirror).
- Subcomandos novos: `memory search` (FTS5 + scope), `memory feedback --kind {deprecate|bump|supersede|use}`, `memory write --verify` (round-trip).
- `memory-ingest --agent-memory` migra `.claude/.agent-memory/_index.json` legacy para SQLite; remove diretório após sucesso.
- Lazy decay on read: `confidence * (1 - days_since_last_used / 30)`; memórias com confidence < 0.3 não retornam por default.
- Filtro padrão de injeção: `spec=current OR (spec IS NULL AND confidence>=0.8)`; extensão por `cluster` quando wave declara `appliesTo` (helper `default_injection_select`).
- `memory cross-wave --cluster` para scope por cluster.

### W8 — Context injection optimization

- `SessionStart` scope-by-spec: top-3 da spec atual + top-2 globais (em vez de top-15 indiscriminados).
- `UserPromptSubmit` adiciona 1 linha "Pipeline em curso" quando há spec ativa e não é `/mustard:*`.
- Hook novo `subagent_inject`: para Task sem SKILL declarada, injeta slice mínimo do `CONTEXT.md` + skills resolvidas via W1.T1.4.
- `PostToolUse(Task)` observer `auto_capture_summary`: parse `<MEMORY>` / `Resumo:` no output do Task, grava em `agent_memory`.
- `SubagentStop` bump em `last_used` da memória que apareceu no output.
- `SessionEnd` consolida `agent_memory` com confidence ≥ 0.85 em `memory_decisions/lessons` permanentes.
- `PreCompact` adiciona até 3 `agent_memory` recentes ao snapshot.
- `context-slice` estendido para `CLAUDE.md` (antes só `CONTEXT.md`).
- `agent-prompt-render --budget-tokens N` trunca placeholders por orçamento; `skill-resolve` é signal de relevância.
- `memory/` da spec não auto-injetada no SessionStart; carregamento via `subagent_inject` por dispatch.

### W9 — Stop e Notification triggers

- Variantes `Stop` e `Notification` no enum `Trigger` (`packages/core/src/model/contract.rs`).
- Hook `stop` persiste `agent_memory` `summary="interrupted at wave N"` se houve edit recente (anti-spam 5min).
- Hook `notification` registra `notification.received` no event-log (apenas observa).
- `settings.json` template inclui entradas `mustard-rt on Stop` e `mustard-rt on Notification`.

### W10 — Verify pipeline multi-stack + wave-integrity-doctor

- `verify-pipeline --json` paralelo via `rayon`; output `{ overall, per_subproject, total_duration_ms }`; timeouts por env (`MUSTARD_VERIFY_TIMEOUT_RUST=600`, `_TS=120`, `_PYTHON=180`).
- Comandos detectados via `stack.md` (W3.T3.1): `[scripts]` block → usa scripts; senão fallback Cargo/pnpm/python.
- Hard gate em `wave-scaffold`: `plan.waves.is_empty()` → exit !=0; mismatch `total_waves` vs `waves.length` → WARN.
- `mustard-rt run plan-from-spec --waves N --roles a,b,c --lang pt-BR`: monta JSON deterministicamente.
- Check `wave-integrity` em `doctor`: verifica wikilinks `[[wave-N-{role}]]` no `wave-plan.md` vs existência dos diretórios.
- `doctor --json`: shape `{ checks: [...], overall }`.
- Tauri `doctor_status` + `DoctorBadge` no footer da Sidebar (verde/amarelo/vermelho com tooltip).

### W11 — Telemetry perf + economy wiring + dashboard /economia

- Audit de queries hot do dashboard via `EXPLAIN QUERY PLAN`; índices criados em `packages/core/src/telemetry/schema.sql` onde havia full-scan.
- `db-maintain --telemetry-only` e `--prune-older-than {N}d`.
- Tabelas `economy_baselines (operation, baseline_tokens, captured_at)` e `economy_savings (wave_id, operation, savings_tokens, measured_at)` em telemetry.
- Tauri command `economy_summary` em `apps/dashboard/src-tauri/src/economy.rs`.
- Página `/economia` ganha aba "Deep Refactor Savings": card total + tabela per-wave (W0-W12) + sparkline.

### W12 — Close and archive (esta onda)

- Backup pré-refator em `~/.mustard-backups/2026-05-25-pre-deep-refactor/` com MANIFEST.json + SHA-256 por arquivo.
- `pipeline.status: archived` emitido para `2026-05-25-mustard-deep-refactor`.
- ADR aqui presente.
- `mustard-rt run graph-index` resincroniza `.claude/graph/index.md` (nós canônicos pós-W3 apenas).
- `meta.json` atualizado: `outcome: Completed`, `phase: CLOSE`, `closed_at` ISO-8601 UTC.
- Memória consolidada (no-op nesta sessão: `agent_memory` sem rows scoped à spec; consolidação já roda via SessionEnd em W8.T8.6 para confidence ≥ 0.85).

## Alternativas consideradas

- **Continuar a `2026-05-24-mustard-unification` em vez de abrir spec nova**: rejeitada — a mega-spec já tinha W5 em curso + W6-W17 em plano misturando temas (events, skills, memory, scan), e o `pipeline-states` herdado bloqueava encerramento limpo. Migrar para spec nova permitiu reordenar dependências (gargalo W1) e absorver pedidos descobertos durante o caminho (graph rescope, claude-dir-prune, recipes-from-scan).
- **Manter as 136 specs históricas no picker com filtro de UI**: rejeitada — `mustard-rt run active-specs` é fonte de verdade para o picker; filtrar na UI deixaria entries fantasmas em queries do dashboard. Backup + `pipeline.status` semântico é mais limpo.
- **Hardcodear catálogo de padrões esperados por stack** (ex.: "React tem cluster `components/`, Drizzle tem `schema/`"): rejeitada — viola [[feedback_no_hardcoded_stack_patterns]]; entropy do user já carrega o sinal via filesystem; emergir do `cluster_discovery` agnóstico é o moat.
- **SDK Anthropic em Rust para o cold-path do scan**: rejeitada — viola [[feedback_llm_via_claude_cli]]; user já paga a subscription do Claude Code, todo LLM call passa pelo subprocess `claude --print`.
- **Mover memória de specs antigas para o novo `mustard.db`**: rejeitada — fase dev, drop limpo ([[feedback_no_migration_dev_phase]]). Memória legacy fica em backup; acessível por Grep/Read se necessário.
- **Manter componente de grafo interno do dashboard**: rejeitada — wikilinks abrem no Obsidian via URI scheme; vault já é gerado pelo `mustard-rt run graph-index`. Menos código a manter, melhor UX (Obsidian tem grafo + busca + edição).
- **Migrar `agent-prompt-render` para template engine externo (Handlebars/Tera)**: rejeitada — substituição literal `{placeholder}` é suficiente; engine externa adiciona dependência sem ganho.
- **Estimar economia em prosa no ADR** (ex.: "~30% mais rápido"): rejeitada — todo delta passa por `economy_baselines`/`economy_savings` reais em `telemetry.db` ([[feedback_everything_measurable]]).

## Consequências

### Positivas

- **Pipeline determinístico**: `spec-draft` + `skill-resolve` + frontmatter padronizado removem três fontes de variabilidade entre execuções.
- **Comandos enxutos**: 18 `commands/mustard/*/SKILL.md` somam 812 linhas (vs ~2300 antes — ~65% de corte). Média 45 linhas por SKILL.
- **Templates `.md` como moat**: refs grandes encurtadas, refs stack-aware com `qualifyingSignals`, skills opt-in fora do default.
- **`/scan` Rust-first**: estrutural em Rust com `cluster_discovery` agnóstico; IA só para interpretação semântica; validators Rust pós-IA.
- **Picker limpo**: `mustard-rt run active-specs` retorna apenas a spec ativa do momento; 136 historical movidas para backup.
- **Memória cross-session**: scope-by-cluster via `appliesTo`; lazy decay; feedback bidirecional.
- **Observabilidade**: `verify-pipeline` multi-stack paralelo; `doctor --json`; `DoctorBadge` no dashboard; `/economia` aba "Deep Refactor Savings".

### Negativas / dívida assumida

- **AC-G5 não verificado**: cobertura do `entity-registry.json` ≥99% depende de W3.T3.3 (parser AST-leve), que ficou como deferred — cobertura permanece em ~6% por enquanto.
- **AC-W5.3 parcial**: 18 lints clippy em 4 arquivos pré-existentes do scan (`scan_structural.rs`, `claude_dir_prune.rs`, `scan/interpret.rs`, `scan_md_validate.rs`). Endereçar incrementalmente quando esses arquivos forem tocados.
- **W3 deferred (T3.3/T3.5/T3.7/T3.12)**: capturado como entry `Residuals from W3` no backlog.
- **`economy_savings` sem números absolutos por wave**: os 11 entries emitidos contêm apenas markers `wave-N-complete` (timestamps), sem `savings_tokens` populado — wire dos `capture-baseline` calls durante as waves não foi instrumentado retroativamente.

### Neutras

- `.claude/graph/index.md` ficou empty no fechamento — esperado, pois W3 restringe nós a `spec.X`/`skill.X`/`command.X`/`ref.X`/`recipe.X`/`conv.X` e essa indexação só se popula quando há cross-refs ativos.
- Memória `agent_memory` da spec vazia no fechamento — esperado: a consolidação automática (W8.T8.6) move entries para `memory_decisions/lessons` permanentes via `SessionEnd`, então não restam rows scoped à spec.

## Relatório final

| Métrica | Valor |
|---|---|
| Waves entregues | 13 (W0-W12) |
| Subcomandos `mustard-rt run` novos (W1+W5+W10) | ~22 |
| Linhas totais `commands/mustard/*/SKILL.md` (18 arquivos) | **812** (de ~2300 → ~65% de corte) |
| Média de linhas por SKILL.md | **45** (alvo ≤67) |
| Tamanho de `.claude/.harness/mustard.db` | **5.4 MB** (5 529 600 bytes) |
| Backup pré-refator | `~/.mustard-backups/2026-05-25-pre-deep-refactor/` (2 specs, 1 115 arquivos, 3.09 MB) |
| Specs históricas arquivadas | **136** (em `~/.mustard-backups/2026-05-25-specs-archive/`) |
| Tabelas drop do `mustard.db` | `events`, `events_fts`, `knowledge` legacy, `metrics_projection` |
| `economy_savings` entries (W0-W10) | 11 (markers; valores absolutos não instrumentados) |
| Linhas deste ADR | ≤300 (cap respeitado) |

## Referências

- Spec canônica: [[../spec]]
- Wave plan: [[../wave-plan]]
- Memória da spec: [[../memory/_index]]
- Backup pré-refator: `~/.mustard-backups/2026-05-25-pre-deep-refactor/MANIFEST.json`
- Backup histórico: `~/.mustard-backups/2026-05-25-specs-archive/MANIFEST.json`
- Memórias guiadoras: [[feedback_mustard_agnostic]], [[feedback_scan_rust_first]], [[feedback_no_hardcoded_stack_patterns]], [[feedback_recipes_from_scan]], [[feedback_graph_pipeline_knowledge]], [[feedback_templates_md_moat]], [[feedback_llm_via_claude_cli]], [[feedback_no_migration_dev_phase]], [[feedback_everything_measurable]], [[project_dashboard_no_graph_obsidian]].
