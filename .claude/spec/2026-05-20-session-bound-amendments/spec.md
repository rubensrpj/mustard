# Janelas de emenda vinculadas à sessão (session-bound amendments)

## PRD

## Contexto

Hoje, quando uma pipeline Mustard fecha (com ou sem follow-up declarado) e o autor faz ajustes manuais imediatos via Edit/Write na mesma sessão Claude Code, nada é capturado. O painel `mustard-dashboard` não vê o trabalho, a spec não registra evidência de continuação, e nenhuma métrica reflete o tempo entre o fechamento e a resolução do que ficou pendente. Trabalho pós-fechamento no mesmo contexto de conversa é território cego — apesar de o `mustard-rt` já interceptar toda Write/Edit via PostToolUse e o harness do Claude Code expor `session_id` em cada lifecycle event. O efeito prático apareceu na pipeline anterior `2026-05-19-pipeline-state-from-sqlite`: fechada com `closed-followup` apontando para `apps/rt/src/run/complete_spec.rs`, o follow-up foi migrado na mesma sessão sem que nenhum evento, métrica ou rastreabilidade ficasse no sistema. Pesquisa em nove ferramentas SDD (GitHub Spec Kit, Kiro, Aider, Cursor, Continue, Devin, OpenHands, BMad, Cline) confirmou que nenhum tool resolve isso de forma passiva — Spec Kit reconhece o gap no issue #1191 mas não mergeou, Kiro tem Hooks file-event como primitiva mais próxima, Devin/BMad recomendam "fresh chat" como anti-padrão. O gap é real e Mustard pode fechá-lo combinando file-event hook passivo, `session_id` como chave de agrupamento, drift detection por path-match contra escopo declarado, e warning não-bloqueante.

## Usuários/Stakeholders

Mantenedores do Mustard que precisam visibilidade de continuação de trabalho pós-CLOSE como sinal de débito real (carry-over rate). Indiretamente, qualquer usuário do `mustard-dashboard` que abre Atividade ou Telemetria e quer ver a história completa de uma spec — não apenas o que aconteceu dentro do pipeline formal. Solicitado por Rubens em 2026-05-20 após observar a invisibilidade do ajuste de follow-up no painel imediatamente após o CLOSE da pipeline anterior.

## Métrica de sucesso

- Edit em arquivo dentro do escopo de uma spec recém-fechada, na mesma sessão, gera evento `pipeline.amend_activity` visível no dashboard ligado à spec original.
- Spec recém-fechada com atividade pós-CLOSE acumulada recebe seção `## Amendments` automática ao `SessionEnd`, com transição de status condicional ao build verde.
- Edits em escopo claramente diferente (subprojeto distinto, mais de 3 arquivos novos não-relacionados) disparam aviso não-bloqueante sugerindo abrir `/feature` ou `/task` novo.
- Métrica `amend_resolution_rate` exposta no Telemetry: ≥ 70% das janelas fechando como `archived` na mesma sessão indica follow-ups resolvidos a quente, não virando débito real.

## Não-Objetivos

- Comando manual `/mustard:amend` — o sistema é 100% passivo, sem invocação explícita.
- Auto-resolução cross-session — quando a sessão Claude Code termina, a janela morre; trabalho posterior exige nova spec/bugfix/task.
- Migração de specs `closed-followup` antigas — janelas só abrem para pipelines fechadas após o merge.
- Reabertura de spec já arquivada — auto-amend anexa seção e move status, mas não retorna a spec para EXECUTE.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Tabela `pipeline_amend_window` criada via schema; presença confirmada — Command: `cargo test -p mustard-core schema_amend_window_present`
- [x] AC-2: Projeção `amend_window_pipeline_file_set` retorna união dedupada de `files_modified` de 3 eventos sintéticos `pipeline.task.complete` — Command: `cargo test -p mustard-core amend_window_projection_union_dedupes`
- [x] AC-3: PostToolUse(Write) em arquivo dentro do escopo com `session_id` matching emite `pipeline.amend_activity` — Command: `cargo test -p mustard-rt amend_capture_activity`
- [x] AC-4: PostToolUse(Bash) com `cargo test` exit 0 atualiza `build_verde_at` da janela — Command: `cargo test -p mustard-rt amend_capture_build_verde`
- [x] AC-5: UserPromptSubmit dentro da janela emite `pipeline.amend_intent` com `prompt_text` literal preservado — Command: `cargo test -p mustard-rt amend_capture_intent`
- [x] AC-6: PostToolUse com `session_id` diferente do registrado na janela não emite eventos amend — Command: `cargo test -p mustard-rt amend_capture_session_isolation`
- [x] AC-7: Projeção retorna janela com `pipeline_file_set` correto após INSERT sintético — Command: `cargo test -p mustard-core amend_window_open_on_complete`
- [x] AC-8: 1 arquivo fora do escopo NÃO dispara drift (sob threshold) — Command: `cargo test -p mustard-rt amend_drift_under_threshold`
- [x] AC-9: 4 arquivos novos fora do escopo + fora do subprojeto disparam `pipeline.amend_drift` com warning não-bloqueante — Command: `cargo test -p mustard-rt amend_drift_triggers_warning`
- [x] AC-10: Edit em arquivo do mesmo subprojeto da spec, fora do `pipeline_file_set`, não dispara drift — Command: `cargo test -p mustard-rt amend_drift_same_subproject_ok`
- [x] AC-11: `SessionEnd` com janela ativa + `build_verde_at` posterior ao último `amend_activity` anexa `## Amendments` à spec.md, status `archived`, move para `archived/` — Command: `cargo test -p mustard-rt amend_session_end_archived`
- [x] AC-12: `SessionEnd` com atividade mas sem build verde cobrindo os edits marca status `closed-amend-pending`, mantém em `active/` — Command: `cargo test -p mustard-rt amend_session_end_pending`
- [x] AC-13: `SessionEnd` com `pipeline.amend_drift` emitido marca status `closed-amend-drift`, mantém em `active/` — Command: `cargo test -p mustard-rt amend_session_end_drift`
- [x] AC-14: Bloco `## Amendments` herda o idioma da spec original via `PipelineScopePayload.lang` (PT/EN); ausência defaulta para EN — Command: `cargo test -p mustard-rt amend_writer_lang`
- [x] AC-15: Componente AmendActivityBlock existe com filtro de eventos `pipeline.amend_*` e é importado em Activity.tsx — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('apps/dashboard/src/components/amend/AmendActivityBlock.tsx','utf8');const p=fs.readFileSync('apps/dashboard/src/pages/Activity.tsx','utf8');process.exit((c.includes('AmendActivityBlock')&&c.includes('pipeline.amend')&&p.includes('AmendActivityBlock'))?0:1)"`
- [x] AC-16: 4 Tauri commands (`amend_resolution_rate`, `amend_drift_rate`, `cross_session_amend_count`, `amend_window_duration`) registrados em lib.rs, expostos como wrappers camelCase em dashboard.ts, e consumidos por AmendMetricsCard — Command: `node -e "const fs=require('fs');const l=fs.readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');const d=fs.readFileSync('apps/dashboard/src/lib/dashboard.ts','utf8');const c=fs.readFileSync('apps/dashboard/src/components/amend/AmendMetricsCard.tsx','utf8');const rs=['amend_resolution_rate','amend_drift_rate','cross_session_amend_count','amend_window_duration'];const ts=['fetchAmendResolutionRate','fetchAmendDriftRate','fetchCrossSessionAmendCount','fetchAmendWindowDuration'];process.exit(rs.every(k=>l.includes(k))&&ts.every(k=>d.includes(k)&&c.includes(k))?0:1)"`

## Plano

## Informações da Entidade

Nova entidade transversal: **`pipeline_amend_window`** — projeção SQLite que rastreia a janela de captura passiva entre o `pipeline.complete` e o `SessionEnd` de uma spec, vinculada por `session_id` do Claude Code.

| Campo | Tipo | Origem |
|---|---|---|
| `spec_id` | TEXT NOT NULL | `pipeline.complete.spec_id` |
| `session_id` | TEXT NOT NULL | harness lifecycle event payload |
| `closed_at` | TEXT NOT NULL | `pipeline.complete.closed_at` |
| `pipeline_file_set` | TEXT (JSON array) NOT NULL | união de `pipeline.task.complete.files_modified` por spec |
| `subprojects` | TEXT (JSON array) NOT NULL | derivado de `pipeline_file_set` (prefixos `apps/X/`, `packages/X/`) |
| `status` | TEXT NOT NULL DEFAULT 'open' | `open` \| `amending` \| `resolved` \| `drift` \| `pending` |
| `last_activity_at` | TEXT | atualizado em cada `amend_activity` |
| `build_verde_at` | TEXT | atualizado em PostToolUse(Bash) com build/test exit 0 |
| `drift_unrelated_paths` | TEXT (JSON array) NOT NULL DEFAULT '[]' | set de paths novos fora-escopo acumulados |
| `drift_emitted` | INTEGER NOT NULL DEFAULT 0 | bit indicando se `amend_drift` já foi emitido |

Primary key: `(spec_id, session_id)` — re-emit de `pipeline.complete` é no-op idempotente. Índice: `(session_id, status)` para lookup O(1) em cada PostToolUse.

Eventos novos (constantes em `packages/core/src/model/event.rs`): `EVENT_PIPELINE_AMEND_OPEN`, `EVENT_PIPELINE_AMEND_ACTIVITY`, `EVENT_PIPELINE_AMEND_INTENT`, `EVENT_PIPELINE_AMEND_DRIFT`, `EVENT_PIPELINE_AMEND_CLOSE` — cada um com payload struct serde lenient correspondente.

## Arquivos

```
packages/core/src/io/sqlite_schema.sql                    — nova tabela + índice
packages/core/src/io/sqlite_store.rs                      — método amend_window_for_session()
packages/core/src/model/event.rs                          — 5 constantes EVENT_PIPELINE_AMEND_* + payloads
packages/core/tests/amend_window_projection.rs            — AC-1, AC-2, AC-7

apps/rt/src/hooks/amend_capture.rs                        — módulo novo (Check + Observer)
apps/rt/src/hooks/mod.rs                                  — registro do módulo
apps/rt/src/dispatch.rs                                   — Registry::new() append (não modificar dispatcher)
apps/rt/src/run/amend_finalize.rs                         — subcommand chamado por SessionEnd
apps/rt/src/run/mod.rs                                    — registro do subcommand
apps/rt/src/hooks/session_cleanup.rs                      — chama amend-finalize antes do cleanup
apps/rt/tests/amend_capture.rs                            — AC-3..AC-10
apps/rt/tests/amend_finalize.rs                           — AC-11..AC-14

apps/dashboard/src/pages/Activity.tsx                     — timeline absorve sub-eventos amend.*
apps/dashboard/src/pages/Telemetry.tsx                    — 4 séries novas
apps/dashboard/src-tauri/src/queries.rs                   — 4 funções Tauri amend.*
apps/dashboard/src/tests/amend.spec.ts                    — AC-15, AC-16

.claude/mustard.json                                      — campo amend.drift_threshold (default 3)
```

## Tarefas

### core Agent (Wave 1) — Schema + eventos + projeção

- [x] Adicionar tabela `pipeline_amend_window` + índice `(session_id, status)` em `sqlite_schema.sql`
- [x] Adicionar 5 constantes `EVENT_PIPELINE_AMEND_*` e payload structs serde lenient em `event.rs`
- [x] Implementar `SqliteEventStore::amend_window_for_session(session_id) -> Result<Option<AmendWindow>>`
- [x] Implementar `SqliteEventStore::amend_window_pipeline_file_set(spec_id) -> Result<Vec<String>>` (união de `pipeline.task.complete.files_modified`)
- [x] Test: schema migration roda limpa sobre banco existente (AC-1)
- [x] Test: projeção retorna janela completa com `pipeline_file_set` correto após 3 task.complete events sintéticos (AC-2, AC-7)
- [x] `cargo build -p mustard-core && cargo test -p mustard-core`

### rt Agent (Wave 2) — Hook amend_capture + drift detection

- [x] Criar `apps/rt/src/hooks/amend_capture.rs` implementando Check + Observer
- [x] No evento `pipeline.complete`: abrir janela (INSERT OR IGNORE) com `pipeline_file_set` derivado + `subprojects` parseados — emit `pipeline.amend_open`
- [x] PostToolUse(Write\|Edit): query janela aberta com `session_id` matching; se `file_path` ∈ `pipeline_file_set` ou subprojeto pertence a `subprojects`, emit `pipeline.amend_activity` e atualiza `last_activity_at`
- [x] PostToolUse(Bash): se comando casa regex `cargo (build|test|check)|pnpm (build|test)|npm (test|run build)|tsc` E exit_code 0, atualiza `build_verde_at`
- [x] UserPromptSubmit: se há janela aberta com session matching, emit `pipeline.amend_intent` com `prompt_text` literal preservado
- [x] Drift detection: edit em arquivo (a) fora `pipeline_file_set` E (b) subprojeto fora `subprojects` → adiciona path único ao `drift_unrelated_paths`; se `len(drift_unrelated_paths) ≥ amend.drift_threshold` (default 3) E `drift_emitted=0`, emit `pipeline.amend_drift` + retorna `Decision::Allow` com injection contendo warning em PT/EN herdado da spec; set `drift_emitted=1`
- [x] Registrar module em `Registry::new()` em `apps/rt/src/dispatch.rs` (append-only, NUNCA modificar dispatcher)
- [x] Estender `prompt_gate` para fechar janelas abertas antes de iniciar nova `/mustard:feature` ou `/mustard:bugfix`
- [x] Tests: AC-3, AC-4, AC-5, AC-6, AC-8, AC-9, AC-10
- [x] `cargo build -p mustard-rt && cargo test -p mustard-rt`

### rt Agent (Wave 3) — Auto-amend writer + SessionEnd

- [x] Criar subcommand `mustard-rt run amend-finalize --session-id {id}` em `apps/rt/src/run/amend_finalize.rs`
- [x] amend-finalize: para cada janela com `session_id` matching e `status != resolved/drift`:
  - Lê `lang` da spec original via `PipelineScopePayload` (default `en` se ausente)
  - Constrói bloco `## Amendments (session {short_id}, {closed_at}-{now})` no idioma resolvido
  - Lista no bloco: `amend_intent` (prompts user), `amend_activity` (Write/Edit por arquivo), build outcomes
  - Append à spec.md (cria seção se inexistente)
  - Decide status final: `archived` se `build_verde_at > last_activity_at` E `drift_emitted=0`; `closed-amend-drift` se `drift_emitted=1`; `closed-amend-pending` caso contrário
  - Se status=archived: move dir da spec de `active/` para `archived/`
  - Emit `pipeline.amend_close` com status final
- [x] Estender `apps/rt/src/hooks/session_cleanup.rs` para chamar `amend-finalize` ANTES de qualquer cleanup
- [x] Garantir ordenação no `Registry::new()`: amend_capture (SessionEnd) precede session_cleanup (SessionEnd)
- [x] Tests: AC-11, AC-12, AC-13, AC-14
- [x] `cargo build -p mustard-rt && cargo test -p mustard-rt`

### dashboard Agent (Wave 4) — Timeline + telemetry séries (parallel-safe com Wave 3)

- [x] Estender `apps/dashboard/src-tauri/src/queries.rs` com 4 funções:
  - `amend_resolution_rate() -> f64` (% janelas → archived na mesma sessão)
  - `amend_drift_rate() -> f64` (% janelas → closed-amend-drift)
  - `cross_session_amend_count() -> u64` (specs em closed-amend-pending)
  - `amend_window_duration() -> Vec<i64>` (histograma `closed_at` → `amend_close.timestamp`)
- [x] Expor via Tauri `#[command]`
- [x] Estender `Activity.tsx`: sub-eventos `pipeline.amend_*` aparecem agrupados dentro do card da spec original (não card separado); ícone distinto pra amend
- [x] Estender `Telemetry.tsx`: 4 séries novas como cards/charts na aba Atividade
- [x] Tests: AC-15, AC-16
- [x] `pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard test`

## Dependências

- Wave 1 (core) → Wave 2 (rt depende do schema + eventos)
- Wave 2 (rt capture) → Wave 3 (rt finalize depende dos eventos emitidos por capture)
- Wave 4 (dashboard) **(parallel-safe)** com Wave 3 — só consome SQLite/eventos, não depende do amend-finalize executar

## Limites

Modificações intencionalmente limitadas aos paths listados em `## Arquivos`. Edições fora surfaceiam `[BOUNDARY WARNING]`. Em particular: **NUNCA modificar** `apps/rt/src/dispatch.rs` além do append em `Registry::new()` — o dispatcher mecanismo permanece imutável (guard `rt-fail-open-dispatch-pattern`).
