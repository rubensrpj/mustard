# Auditoria Arquitetural — `core` / `cli` / `rt`

> Análise completa do código atual (sem histórico git). Objetivo: aplicar SOLID,
> modularizar e reaproveitar código após a migração TS/Bun → Rust. Foco em
> **separar o `apps/rt` por domínio/operação** (hoje ~115 arquivos num único
> diretório `run/`) e **eliminar a duplicação** deixada pela migração incompleta.

---

## 1. Sumário executivo

| Crate | Linhas | Estado | Veredito |
|---|---|---|---|
| `packages/core` | ~24.6k | **Saudável** — 23 seams bem definidos, SRP por módulo | É o *modelo-alvo*. Mantém-se; recebe a lógica extraída do `rt`. |
| `apps/cli` | ~4.0k | **Saudável** — thin shell `clap`, helpers próprios, usa `core` corretamente | Não é prioridade. Só 6 `std::fs` diretos. |
| `apps/rt` | ~84.3k | **Caótico no `run/`** — flat dir, duplicação, bypasses | **Alvo principal da refatoração.** |

**A parte difícil já está feita.** Os seams canônicos existem em `core`
(`fs`, `ClaudePaths`, `spec`, `meta`, `economy`, `events`, `projection`,
`atomic_md`, `skill`, `process`…). O problema é que a migração para *usar* esses
seams ficou pela metade no `apps/rt`, e os dois padrões coexistem (o que gera as
"várias chamadas com implementações diferentes"):

| Bypass | Ocorrências no `rt` | Deveria usar |
|---|---|---|
| `std::fs::` direto | **661** | `mustard_core::fs` |
| join de caminho `.claude`/`spec` inline | **~195** | `ClaudePaths` / `SpecPaths` |
| `serde_json::from_*` ad-hoc | **149** | helpers tipados |
| `Command::new` solto | **83** | `process::rtk_command` / `util::platform` |

E o `rt/src/lib.rs` carrega `#![allow(dead_code, unused_imports, unused_variables, unused_mut)]` — que mascara código morto e cópias esquecidas da migração.

**A arquitetura de dispatch do `rt` é boa** (ver §3). O caos é estrutural
(organização de arquivos) e de duplicação — não de design de runtime.

---

## 2. Arquitetura atual

### As 4 faces do `mustard-rt`
1. **`on <event>`** — enforcement: lê JSON do stdin, roda módulos aplicáveis, devolve `Outcome`.
2. **`check <id>`** — roda um único módulo de hook por ID.
3. **`run <name> [args]`** — porta dos scripts: argumentos `clap`, **nunca stdin**, imprime no stdout. **← onde está o caos (115 arquivos em `run/`).**
4. **`mcp`** — servidor MCP (JSON-RPC), read-only, 5 tools.

### Contrato de hook (bom, SOLID)
`Registry` estática mapeia `(Trigger, ToolMatch)` → `Module { id, triggers, check?, observer? }`.
`Check: fn(&HookInput, &Ctx) -> Verdict` e `Observer: fn(&HookInput, &Ctx)` vivem em
`core::model::contract`. O `dispatch.rs` é o ponto único de fail-open: roda observers
(fire-and-forget), aplica `Mode` (off/warn/strict) sobre cada `Verdict`, folda em `Outcome`,
e trata `Err` como `Allow` (degradação, sem panic). **Manter.**

---

## 3. Catálogo de seams do `core` (o que JÁ é compartilhável)

| Módulo | Seam / serviço | Principais tipos/fns |
|---|---|---|
| `ast` | parse AST agnóstico (tree-sitter) + detecção de stub fail-open | `GrammarLoader`, `TreeSitterParser`, `detect_stub_patterns`, `extract_function_signatures` |
| `atomic_md` | I/O atômico de markdown + frontmatter + wikilinks | `MarkdownStore`, `MarkdownDoc`, `Frontmatter`, `find_backlinks`, `render_footer`, `resolve` |
| `claude_paths` | **fonte única de caminhos `.claude/`** (anti double-nesting) | `ClaudePaths`, `SpecPaths`, `WavePaths` + accessors tipados |
| `config` | modo de enforcement (off/warn/strict) | `EnforcementConfig`, `Mode`, `resolve` |
| `economy` | **fonte única de custo/savings**: modelos, writers NDJSON, estimadores, ingest externo | `EconomyScope`, `EconomySummary`, `SpanRecord`, `SavingsRecord`, `estimate_*_tokens`, `reader`, `sources::{otel,rtk,transcript}` |
| `env` | runtime de hook (porta do `hook-env.js`): should-run, guards, depth | `Env` trait, `ProcessEnv`, `guarded_run`, `should_run` |
| `error` | erro tipado + helpers fail-open | `Error`, `Result`, `fail_open` |
| `events` | primitivos NDJSON: streaming/cache/filtro | `Event`, `EventReader` |
| `fs` | **seam canônico de filesystem** (fail-open, atomic, DIP via trait) | `Fs` trait, `RealFs`, `FakeFs`, `read_to_string`, `write_atomic`, `append_line` |
| `i18n` | idioma + tom (pt-BR/en-US, BCP-47) | `SupportedLocale`, `UserLocale`, `translate`, `apply_tone`, `slugify` |
| `knowledge` | extração de friction + seleção de contexto (trait plugável) | `extract_friction`, `ContextSelector` trait |
| `meta` | **sidecar `meta.json`** (schema + IO lenient) | `Meta`, `read_meta`, `write_meta`, `normalise_lang` |
| `metrics` | writer de métricas (porta do `metrics-emit.js`) | `MetricLine`, `emit_metric` |
| `model` | tipos serde puros: evento, contrato de hook, pipeline-state, ViewModels SDD | `HookInput`, `Verdict`, `Outcome`, `HarnessEvent`, `SpecView`, `WaveView`, `QualityRollup` |
| `process` | subprocess com Golden Rule | `rtk_command` |
| `projection` | folds puros sobre `&[HarnessEvent]` (1 fn por ViewModel) | `project_spec_view`, `project_quality`, `project_waves`, `project_workspace`, `ndjson_to_harness` |
| `regression_check` | snapshot+diff de funções tocadas (AST + fallback textual) | `Snapshot`, `compare_snapshots`, `Diff` |
| `skill` | schema canônico de frontmatter de skill | `SkillFrontmatter`, `SkillScope`, `SkillTag`, `parse`, `validate` |
| `spec` | **dono do header do spec.md** (parse/rewrite tolerante + atômico) | `parse_state`, `write_state`, `rewrite_header`, `header_field` |
| `summary` | artefato versionável `.summary.json` | `SpecSummaryDoc`, `writer::write` |
| `vocabulary` | matcher de 4 camadas (Aho-Corasick) p/ gate de regressão | `VocabularyMatcher`, `VocabLayer` |
| `workspace` | **fonte única da raiz do workspace** | `workspace_root`, `WorkspaceError` |

---

## 4. Catálogo de comandos do `rt` (`run/`) — o que cada um faz

Agrupado pela **estrutura de domínio proposta** (§6). Hoje todos estão soltos em `run/`.

### 4.1 `spec/` — ciclo de vida e materialização de specs
| Arquivo | O que faz |
|---|---|
| `active_specs` | descobre specs `Outcome=Active`, filtra por `Stage`, conta progresso de waves |
| `complete_spec` | marca spec `completed`/`closed-followup`, emite eventos, reconstrói `.summary.json` |
| `spec_children` | lista sub-specs via header `### Parent:` |
| `spec_children_tree` | projeta waves + ACs + sub-specs num round-trip |
| `spec_clear` | varre specs `Close+Completed` idle > 15d, remove/lista |
| `spec_draft` | gera layout completo (spec.md + meta.json + memory + wave-plan) |
| `spec_extract` | fatia wave N / bloco de AC; mede omissão em bytes |
| `spec_lang_resolve` | resolve locale (meta → header → mustard.json → default) |
| `spec_link` | liga child↔parent via evento `spec.link` + pipeline-states |
| `spec_memory` | cria `memory/{name}.md` com frontmatter |
| `spec_scaffold` | **helper compartilhado**: escreve spec.md/meta.json atomicamente |
| `spec_sections` | **helper puro**: heading ↔ chave canônica (PT/EN) |
| `spec_slug` | slug locale-aware (fachada fina sobre `core::slugify`) |
| `spec_status_backfill` | alinha header spec.md ↔ meta.json |
| `spec_validate` | valida layout contra `core::spec::contract` |
| `rebuild_specs` | re-materializa `.summary.json` |
| `backup_specs` | copia `.claude/spec/` com manifesto SHA-256 |
| `plan_from_spec` | emite JSON de plano de waves |
| `prd_build` | lapida intent em PRD JSON |
| `scope_decompose` | detecta sinais de roadmap no texto do spec |
| `tactical_fix_create` | scaffold de spec tático + `spec.link` |

### 4.2 `migrate/` — migrações terminais (audit-only/legacy)
| Arquivo | O que faz |
|---|---|
| `migrate_spec_headers` | `### Status:`/`### Phase:` → `### Stage:/Outcome:/Flags:` |
| `migrate_to_meta` | extrai headers → sidecar `meta.json` |
| `pipeline_state_ingest` | no-op de compatibilidade pós-migração SQLite→NDJSON |

### 4.3 `wave/` — composição e renderização de waves
| Arquivo | O que faz |
|---|---|
| `wave_context` | renderiza `_context.md` canônico (5 seções, cap 8k palavras) |
| `wave_summary` | renderiza `_summary.md` canônico (7 seções) |
| `wave_tree` | árvore ASCII/JSON de status das waves |
| `wave_dependency` | DAG de dependências (parse import/require) + topo-sort |
| `wave_files` | extrai contagem/markdown da seção `## Arquivos`/`## Files` |
| `wave_size_check` | audita waves oversized (file/layer count) |
| `wave_scaffold` | materializa layout SDD de wave via plano JSON |
| `wave_lib` | **helper compartilhado**: `detect_role`, `parse_files_section` |
| `exec_rewave_check` | re-decompõe spec em wave-plan se `layerCount>=2` |
| `epic_fold` | consolida eventos no fim do epic, emite `epic.complete` |

### 4.4 `event/` — emissão e armazenamento de eventos
| Arquivo | O que faz |
|---|---|
| `event_route` | **router único** de classificação/roteamento de todo evento |
| `event_writer_ndjson` | writer NDJSON (hot path) com blob spill + fail-open |
| `event_projections` | projeções read-only (6+ views) |
| `emit_event` | emite evento arbitrário com payload |
| `emit_phase` | emite transição `pipeline.phase` + writebacks |
| `emit_pipeline` | emite eventos pipeline tipados + QA gate + aliasing legacy |
| `blob_spill` | spill content-addressed p/ o log NDJSON |
| `verify_emit` | verifica que um evento foi emitido na janela |

### 4.5 `pipeline/` — orquestração e status
| Arquivo | O que faz |
|---|---|
| `pipeline_prelude` | warm-up ANALYZE/PLAN/EXECUTE (sync-detect + diff-context) |
| `pipeline_summary` | render Done/Left/Next/Follow-ups no CLOSE |
| `verify_pipeline` | build+test paralelo (rayon) com descoberta de stack |
| `close_orchestrate` | gate CLOSE (verify→qa→docs→summary) |
| `status` | snapshot git + pipelines + build + registry |
| `resume_bootstrap` | engine de decisão de resume (mode/stage/wave/model) |

### 4.6 `economy/` — custo, budget e telemetria
| Arquivo | O que faz |
|---|---|
| `economy_capture_baseline` | grava baseline de duração de operação |
| `economy_reconcile` | atualiza baselines (mediana das últimas 3) |
| `economy_report` | tabela ASCII/JSON das baselines |
| `rtk_gain` | normaliza `rtk gain --json` → `core::economy::sources::rtk` |
| `token_budget` | estimador de budget (4 chars/token) + prune greedy |
| `context_budget` | budget de char por role (lookup puro) |
| `context_slice` | recorta CONTEXT.md contra um spec |
| `transcript_watcher` | daemon que tail `~/.claude/projects/*.jsonl` |
| `metrics` | agrega telemetria por tipo de evento |
| `metrics_wave_status` | roll-up por wave (status/tokens/duração) |
| `otel/{collector,diagnose,project,mod}` | receiver OTLP + diagnose + projeção |

### 4.7 `scan/` — descoberta estrutural e grafo
| Arquivo | O que faz |
|---|---|
| `scan/file_utils` | visitor single-pass + cache thread-local |
| `scan/cluster_discovery` | clusters estruturais (sufixos, base classes) |
| `scan/entity_extractor` | extrai declarações públicas (agnostic) |
| `scan/graph` | índice de conceitos `.claude/graph/` + ciclos |
| `scan/resolve` | resolver BFS do grafo com budget |
| `scan/interpret` | interpreta perfil via subprocess `claude` |
| `scan/project_conventions` | detecta convenção de naming dominante |
| `scan/pluralize` | singularização de nomes EN |
| `scan/refs_installer` | instala refs progressivas por signal |
| `scan/mod` | orquestra scan + cluster + interpret |
| `scan_orchestrate` / `scan_precompute` / `scan_finalize` | pré/pós-dispatch |
| `scan_structural` | manifests + deps + extensions + clusters |
| `scan_md_validate` / `scan_recipes_validate` | validação de `.md`/recipes gerados |
| `sync_detect` | detecta subprojetos, roles, hashes SHA-256 |
| `sync_registry` | gera `entity-registry.json` v4 |
| `recipe_match` | busca recipe por entity+operation |

### 4.8 `knowledge/` — memória e grafo de conhecimento
| Arquivo | O que faz |
|---|---|
| `knowledge` | browser do glossário via `entity-registry.json` |
| `memory` | CLI unificada de persistência markdown (agent/decision/…) |
| `memory_cross_wave` | renderiza memórias de waves anteriores |
| `memory_ingest` | migração legacy JSON → markdown |
| `graph_index` / `graph_dead` | build do grafo / nós sem backlink |

### 4.9 `skill/` — descoberta e resolução de skills
| Arquivo | O que faz |
|---|---|
| `skill_resolve` | scorer determinístico de skills por intent |
| `skills` | validate/graph/orphans/list de SKILL.md |
| `skill_fetch` | instala skill (path:/github:/local) |
| `skill_cache` | checa cache `.skill-cache.json` |
| `skill_discovery_lint` | lint de antipatterns de discovery LLM |

### 4.10 `review/` — review, QA e gates
| Arquivo | O que faz |
|---|---|
| `review_dispatch` | orquestra fase REVIEW |
| `review_prefetch` | extrai PR via `gh pr view` |
| `review_result` | registra desfecho REVIEW |
| `review_spans` | ledger append-only de verdicts por wave |
| `qa_run` / `qa_run_all` | executa ACs do spec / itera ativos |
| `gate_regression_check` | gate W4 (vocab + AST + snapshot) |
| `analyze_validation` | validador WARN de spec |
| `security_scan` | scan de secrets + exposição |
| `dependency_precheck` | gate de dependências JSX/import cross-wave |
| `bugfix_cache` | cache durável de fixes por hash |

### 4.11 `doctor/` — diagnóstico e auditoria de linguagem
| Arquivo | O que faz |
|---|---|
| `doctor` | diagnóstico read-only de saúde |
| `doctor_claude_paths` / `doctor_i1` / `doctor_workspace_leaks` | auditorias específicas |
| `language_audit` | detecta PT-BR em arquivos EN-only |
| `docs_stale_check` | linter de narrative-drift |

### 4.12 `maint/` — manutenção e kill-switch
| Arquivo | O que faz |
|---|---|
| `maint_deps` / `maint_validate` | install/validate por stack |
| `refresh_claude` | sync `.claude/` vs templates por SHA-256 |
| `artifact_update` | freshness de artifacts (check/apply) |
| `claude_dir_prune` | poda de drift em `.claude/` |
| `worktree_gc` | GC de worktrees órfãos |
| `unhook` / `rehook` | kill-switch / restore do harness |
| `adapt_cursor` | gera `.cursorrules` a partir de CLAUDE.md |

### 4.13 demais (pequenos / fachadas)
`statusline/{mod,segment,theme,preview}` · `task_checklist` + `mark_checklist_item` ·
`agent_prompt_render` + `amend_finalize` · `i18n_translate` · `env` (contexto de run).

---

## 5. Hooks (`hooks/`) — já consolidados por família, ainda flat

27 módulos. A consolidação por família já ocorreu (`bash_guard` junta 5 gates de Bash;
`post_edit` junta 3; `size_gate` 3; `knowledge` 3; `session_start` 3; `tracker` 5 observers).
**O que falta é extrair os helpers de contrato repetidos** (ver §6.B).

Famílias: **bash/tool** (`bash_guard`) · **task** (`budget`, `model_routing`, `tracker`,
`skills_audit`, `subagent_inject`) · **write/edit** (`size_gate`, `path_guard`, `post_edit`,
`close_gate`, `pre_edit_intent_check`) · **session** (`session_start`, `session_cleanup`,
`knowledge`, `pre_compact`, `prompt_gate`, `spec_hygiene`, `enforce_registry`) ·
**observação** (`amend_capture`, `auto_capture_summary`, `stop`, `stop_observer`,
`tool_result`, `notification`, `wikilink_footer`).

---

## 6. Duplicação detectada → backlog de extração (big-bang por categoria)

### A. Para o `core` (lógica cross-cutting, sem dono claro hoje)

| # | Duplicação | Onde aparece | Destino proposto |
|---|---|---|---|
| A1 | **timestamp ISO↔epoch** com 3 impls divergentes (`parse_iso_millis`, `epoch_ms_from_iso`, `iso_from_epoch_ms`) | complete_spec, event_writer_ndjson, spec_clear, verify_emit, metrics_wave_status | `core::time` (nova) |
| A2 | **factory de evento de economia** (`HarnessEvent{event:"pipeline.economy.*"}`) | spec_lang_resolve, plan_from_spec, prd_build, pipeline_prelude, verify_pipeline, tactical_fix_create, bugfix_cache, review_dispatch (~8×) | `core::economy::emit_economy()` |
| A3 | **gestão de baselines** (walk NDJSON manual reinventando `EventReader`/`reader`) | economy_capture_baseline, economy_reconcile, economy_report | `core::economy::baselines` |
| A4 | **projeção `pipeline_state_from_events`** mora no `rt` mas é fold puro | event_projections | `core::projection` |
| A5 | **discovery de SKILL.md + parse de frontmatter** (1 usa `core::skill`, outro parseia YAML à mão — inconsistente) | skill_resolve, skills | `core::skill::discover` (unificar no `core::skill::frontmatter`) |
| A6 | **queries de entity-registry** | knowledge, skill_resolve, skills, prd_build | `core::entity` (novo) |
| A7 | **parse de frontmatter/wikilink** apesar de `core::atomic_md` existir | scan/graph, scan/resolve, scan_md_validate, scan/refs_installer | usar `core::atomic_md` |

### B. Para helpers do `rt` (específicos do binário)

| # | Duplicação | Onde aparece | Destino proposto |
|---|---|---|---|
| B1 | **`read_json` / `write_json`** + IO de `.pipeline-states/{spec}.json` ad-hoc | exec_rewave_check, epic_fold, spec_link, event_projections, pipeline_summary | `rt::util::json_io` |
| B2 | **shell por plataforma** (cmd.exe vs sh) | verify_pipeline, status, qa_run, close_gate | `rt::util::platform::{build_cmd,run_cmd}` |
| B3 | **render markdown** (`write_heading`, `write_wikilink_list`, `write_code_table`, `write_ac_rows`) | wave_context, wave_summary | `rt::run::wave::markdown_render` |
| B4 | **extração de seção** (`## Files`/`## Arquivos`, `## Summary`, `## Tasks`, wave-plan rows) | wave_files, wave_size_check, exec_rewave_check, dependency_precheck, qa_run, wave_tree | estender `spec_sections` (ou `core::spec::sections`) |
| B5 | **`slug_for` + `fnv1a8`** (cópias idênticas) | memory, memory_ingest | `rt::run::knowledge::slug` |
| B6 | **detecção de stack/signals** (3 cópias) | scan/mod, sync_detect, sync_registry | constante/enum único em `scan/` |
| B7 | **contrato de hook**: `project_dir(input,ctx)`, `extract_tool_field`, `resolve_content` (Write vs Edit), `path \\→/`, `resolve_mode(env,default)` | 15+ hooks | `rt::hooks::contract` |

### C. Migrações de bypass (mecânicas, por categoria)
- **C1** — `std::fs::` → `core::fs` (**661** sites; concentrados em hooks + scan).
- **C2** — joins inline `.claude`/`spec` → `ClaudePaths`/`SpecPaths` (**~195**).
- **C3** — `Command::new` → `process::rtk_command` ou `util::platform` (**83**).
- **C4** — remover `#![allow(dead_code, unused_imports, …)]` do `rt/lib.rs` e limpar o que aparecer.

---

## 7. Estrutura-alvo proposta (`apps/rt/src/run/`)

De **flat (115 arquivos)** para **domínios** (cada um com `mod.rs` reexportando a face pública):

```
apps/rt/src/
├── run/
│   ├── mod.rs              # só declara os domínios + roteia
│   ├── spec/              # §4.1  (21 arquivos)
│   ├── migrate/           # §4.2  (3)
│   ├── wave/              # §4.3  (+ markdown_render, section_parse) (10)
│   ├── event/            # §4.4  (8)
│   ├── pipeline/         # §4.5  (6)
│   ├── economy/          # §4.6  (+ otel/) (14)
│   ├── scan/             # §4.7  (consolida os scan_*/sync_* soltos) (19)
│   ├── knowledge/        # §4.8  (6)
│   ├── skill/            # §4.9  (5)
│   ├── review/           # §4.10 (11)
│   ├── doctor/           # §4.11 (6)
│   ├── maint/            # §4.12 (9)
│   ├── statusline/       # já existe
│   ├── checklist.rs      # task_checklist + mark_checklist_item
│   ├── agent.rs          # agent_prompt_render + amend_finalize
│   ├── i18n.rs           # i18n_translate
│   └── context.rs        # env.rs (contexto de run)
├── hooks/                # já por família; + contract.rs (B7)
└── util/                 # + json_io (B1), platform (B2), time→core (A1)
```

`core` ganha: `time` (A1), `economy::{emit_economy, baselines}` (A2/A3),
`projection::pipeline_state` (A4), `skill::discover` (A5), `entity` (A6).

---

## 8. Plano de execução (big-bang por categoria, trabalhando direto)

Ordem por **dependência + alavancagem** (cada passo compila e passa testes antes do próximo):

1. **Fundação no `core`** — A1 `time`, A2 `emit_economy`, A6 `entity`. (destrava o resto; sem mover arquivos)
2. **Helpers do `rt`** — B1 `json_io`, B2 `platform`, B7 `hooks::contract`.
3. **Reorganização física** — mover `run/*` para os domínios da §7 (puro `git mv` + ajuste de `mod.rs`/paths). Big-bang: 1 commit por domínio.
4. **Dedup intra-domínio** — B3/B4 (wave), B5 (knowledge), B6 (scan), A3/A4/A5/A7.
5. **Migração de bypass por categoria** — C2 (paths) → C1 (fs) → C3 (command).
6. **Limpeza final** — C4 (remover `allow` global, matar dead code exposto).

> **Risco/observação:** o passo 3 gera diffs enormes mas mecânicos. Os passos 1, 2 e 4
> são onde a engenharia real acontece. Cada passo é verificável por `cargo build` +
> `cargo test` do workspace.
