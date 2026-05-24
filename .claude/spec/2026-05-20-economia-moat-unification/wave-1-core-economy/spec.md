# Wave 1 — Core economy domain: writer/reader/scope/estimator

## PRD

## Contexto

A economia de tokens do Mustard hoje vive espalhada: cada hook em `apps/rt/src/hooks/` monta seu próprio JSON com a forma do payload cravada inline; o dashboard em `apps/dashboard/src-tauri/src/telemetry.rs` lê o evento com queries SQL diretas; a página `Economia.tsx` junta tudo na mão. Quando alguém precisa adicionar um sinal novo, mexe em três crates ao mesmo tempo e descobre tarde que `bash_guard.rs:1417` e `model_routing.rs:417` cravam `tokens_saved: 0` desde sempre, ou que a tabela `spans` (que carrega o custo real Anthropic) está vazia em produção porque só fixtures de teste fazem INSERT. Sem uma fonte de domínio única, qualquer próxima evolução (revival OTEL, parser JSONL, atribuição por agente, scope multi-projeto) só amplifica o problema. Esta wave consolida o domínio em `packages/core/src/economy/` espelhando o padrão que `core::store::sqlite_store` aplicou para SQLite: writer único, reader único, scope (Projeto/Spec/Wave/Comparar Projetos) como cidadão de primeira classe, e `tiktoken-rs` como estimador para preview enquanto fontes externas não respondem.

## Usuários/Stakeholders

Desenvolvedores que estendem o Mustard (você + futuros contribuidores): hoje pra adicionar um sinal de economia novo precisa modificar 3 crates; depois disso modifica 1 módulo e o resto consome via API estável.

## Métrica de sucesso

`cargo test -p mustard-core` passa cobrindo: writer grava cada um dos 4 tipos de record em SQLite, reader devolve a forma esperada para os 4 scopes (Project/Spec/Wave/AllProjects), estimator devolve contagem ±5% via `tiktoken-rs`, `MultiProjectReader` faz fan-out funcional sobre 2 bancos de teste e retorna merge.

## Não-Objetivos

- Não tocar hooks ainda (W2 instrumenta).
- Não implementar adapters OTEL/JSONL/RTK (W3 entrega).
- Não fazer atribuição por agente (W4 entrega).
- Não tocar UI (W5/6/7 entregam).
- Não criar `mustard-economy` como crate separada — fica como `pub mod economy` dentro de `mustard-core`.
- Não alterar tabelas existentes (`events`, `spans`) — apenas adicionar novas via migration adicional.
- Não fazer benchmark de performance do `tiktoken-rs` — basta funcionar; otimização é debt futuro.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build do core passa — Command: `cargo check -p mustard-core`
- [x] AC-2: Testes do core passam — Command: `cargo test -p mustard-core`
- [x] AC-3: Módulo `economy` re-exportado em lib.rs — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/lib.rs','utf8');if(!t.includes('pub mod economy'))throw new Error('missing pub mod economy')"`
- [x] AC-4: `EconomyScope` tem 4 variantes — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/scope.rs','utf8');['Project','Spec','Wave','AllProjects'].forEach(v=>{if(!t.includes(v))throw new Error('missing variant '+v)})"`
- [x] AC-5: Writer expõe 4 métodos — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/writer.rs','utf8');['record_span','record_savings','record_context_cost','record_api_cost'].forEach(f=>{if(!t.includes('fn '+f))throw new Error('missing fn '+f)})"`
- [x] AC-6: Reader expõe 6 métodos — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/reader.rs','utf8');['economy_summary','per_agent_costs','per_spec_costs','per_wave_costs','savings_breakdown','context_routing_quality'].forEach(f=>{if(!t.includes('fn '+f))throw new Error('missing fn '+f)})"`
- [x] AC-7: `tiktoken-rs` adicionado em Cargo.toml — Command: `node -e "const t=require('fs').readFileSync('packages/core/Cargo.toml','utf8');if(!t.includes('tiktoken'))throw new Error('tiktoken-rs not added')"`
- [x] AC-8: MultiProjectReader implementado (não só assinatura) — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/multi_project.rs','utf8');if(!t.includes('pub fn') || !t.match(/for\\s+\\w+\\s+in/))throw new Error('MultiProjectReader missing fan-out loop')"`

## Plano

## Informações da Entidade

Nova área de domínio — sem entidade pré-existente no `entity-registry.json` para esta região. As 4 entidades-chave novas:

| Tipo | Onde | Propósito |
|---|---|---|
| `SpanRecord` | `economy::model` | Frame de custo da API Anthropic por request: `model`, `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`, `cost_usd_micros`, `session_id`, `request_id`, `ts` |
| `SavingsRecord` | `economy::model` | Economia atribuída a uma intervenção do Mustard: `source` (`SavingsSource` enum: `RtkRewrite`/`ModelRoutingDowngrade`/`BashGuardBlock`/`BudgetOutputCut`/`RecipeInjection`), `tokens_saved`, `model_target`, `project_path`, `spec_id?`, `wave_id?`, `agent_id?`, `ts` |
| `ContextCostFrame` | `economy::model` | Composição do contexto enviado a um agente: `prompt_size_bytes`, `prefix_stable_bytes`, `slice_bytes`, `recipe_bytes`, `wave_slice_bytes`, `return_size_bytes`, `retry_overhead_bytes`, `agent_id`, `wave_id?`, `spec_id?`, `project_path`, `ts` |
| `ApiCostFrame` | `economy::model` | Alias semântico de `SpanRecord` para a API pública do writer — distingue "vim de adapter externo (OTEL/JSONL)" de "vim de estimativa interna" |

## Arquivos (~11)

```
packages/core/Cargo.toml                              (modify — +tiktoken-rs workspace dep)
packages/core/src/lib.rs                              (modify — pub mod economy)
packages/core/src/economy/mod.rs                      (new — facade re-export)
packages/core/src/economy/scope.rs                    (new — EconomyScope + newtypes)
packages/core/src/economy/model.rs                    (new — domain types)
packages/core/src/economy/writer.rs                   (new — 4 record_* fns)
packages/core/src/economy/reader.rs                   (new — 6 query fns)
packages/core/src/economy/estimator.rs                (new — tiktoken-rs wrapper)
packages/core/src/economy/multi_project.rs            (new — fan-out reader)
packages/core/src/economy/sources/mod.rs              (new — placeholder, populado em W3)
packages/core/src/store/migrations.rs                 (modify — ADD migration N+1 com tabelas savings_records + context_cost_frames; spans já existe)
packages/core/tests/economy_basic.rs                  (new — round-trip + multi-project)
```

## Tarefas

### Core Library Agent

- [ ] Verificar via web a versão mais recente estável de `tiktoken-rs` no crates.io (deve ser ≥ 0.6) e adicionar em `Cargo.toml` raiz `[workspace.dependencies]` + referenciar em `packages/core/Cargo.toml` via `{ workspace = true }`. Seguir o padrão da `sha2` existente que comenta data e razão de inclusão.
- [ ] Criar `packages/core/src/economy/mod.rs` como facade. Re-exportar todos os tipos públicos de `model`, `scope`, `writer`, `reader`, `estimator`, `multi_project`. Seguir padrão de `store/mod.rs` existente.
- [ ] Criar `packages/core/src/economy/scope.rs` com `EconomyScope` enum (4 variantes: `Project(ProjectPath)`, `Spec { project, spec }`, `Wave { project, spec, wave }`, `AllProjects(Vec<ProjectPath>)`) + newtypes `ProjectPath(PathBuf)`, `SpecId(String)`, `WaveId(String)`, `AgentId(String)`. Newtypes derivam `Serialize/Deserialize/Clone/Debug/PartialEq/Eq/Hash`. Document inline (doc-comments EN) por que cada variant existe.
- [ ] Criar `packages/core/src/economy/model.rs` com os 4 records de domínio (`SpanRecord`, `SavingsRecord`, `ContextCostFrame`, `ApiCostFrame`) + tipos agregados (`EconomySummary`, `AgentCost`, `SpecCost`, `WaveCost`, `SavingsBreakdown`, `ContextRoutingMetrics`). Use lenient serde pattern (`#[serde(flatten)] raw: Value` se aceitar JSON externo). Todos derivam Serialize/Deserialize. Custo em micro-USD (`i64`) — evita float drift; conversão pra display fica na UI.
- [ ] Criar `packages/core/src/economy/writer.rs` com 4 funções públicas: `record_span(conn: &Connection, rec: SpanRecord) -> Result<()>`, `record_savings(conn, SavingsRecord) -> Result<()>`, `record_context_cost(conn, ContextCostFrame) -> Result<()>`, `record_api_cost(conn, ApiCostFrame) -> Result<()>`. Cada uma valida shape, abre transação, faz INSERT na tabela alvo (`spans` existente para spans/api_cost; novas tabelas `savings_records` e `context_cost_frames` para os outros dois), retorna Result. Fail-open: erro de IO loga via `tracing` mas não panica.
- [ ] Criar `packages/core/src/economy/reader.rs` com 6 funções públicas, todas com assinatura `(conn: &Connection, scope: EconomyScope) -> Result<T>`:
  - `economy_summary(scope) -> EconomySummary` — agregado top-level (total USD, total tokens, savings total, top 3 agentes por custo)
  - `per_agent_costs(scope) -> Vec<AgentCost>` — ordenado por custo desc
  - `per_spec_costs(scope) -> Vec<SpecCost>` — só faz sentido em scope Project/AllProjects
  - `per_wave_costs(scope) -> Vec<WaveCost>` — só faz sentido em scope Spec/Project/AllProjects
  - `savings_breakdown(scope) -> SavingsBreakdown` — por `SavingsSource`
  - `context_routing_quality(scope) -> ContextRoutingMetrics` — ratio cache hit, ratio prefix-stable, retry overhead
  - Cada função: match no `scope` — single-project é query SQL direta, `AllProjects` delega para `multi_project::MultiProjectReader`.
- [ ] Criar `packages/core/src/economy/estimator.rs`: wrapper sobre `tiktoken-rs`. Exporta `estimate_input_tokens(text: &str, model: &str) -> u32` e `estimate_output_tokens(text: &str, model: &str) -> u32`. Cache do encoder por modelo via `OnceLock<HashMap<String, CoreBPE>>`. Suporta pelo menos `claude-3-5-sonnet`, `claude-3-5-haiku`, `claude-opus-4` mapeando para o tokenizer apropriado (cl100k_base é a aproximação aceita para modelos Claude — documentar como ±5% precisão, não bit-exact). Função `model_pricing_usd_micros_per_million(model: &str) -> (input: i64, output: i64)` retorna preço estático lookup-table para conversão de tokens em USD.
- [ ] Criar `packages/core/src/economy/multi_project.rs::MultiProjectReader`. Struct vazio com métodos estáticos que recebem `Vec<ProjectPath>` + função de query single-project. Abre cada DB read-only (via `Connection::open_with_flags(SQLITE_OPEN_READ_ONLY)`), executa a query, agrega resultados em `HashMap<ProjectPath, T>` + opcional merge em `T` agregado. Não usa Rayon ainda (paralelismo é otimização futura) — loop sequencial sobre projetos é aceitável em W1.
- [ ] Criar `packages/core/src/economy/sources/mod.rs` como módulo vazio com comentário `//! Adapters injetam aqui em W3. Vide spec wave-3-ingestion.`
- [ ] Adicionar migration nova em `packages/core/src/store/migrations.rs` (apenas APPEND ao array de migrations existentes — nunca alterar uma migration já aplicada). Migration cria:
  - tabela `savings_records` com colunas `(id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, source TEXT NOT NULL, tokens_saved INTEGER NOT NULL, model_target TEXT, project_path TEXT NOT NULL, spec_id TEXT, wave_id TEXT, agent_id TEXT, payload TEXT)` + índices em `(project_path, ts)` e `(spec_id, ts)`
  - tabela `context_cost_frames` com colunas `(id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, agent_id TEXT NOT NULL, wave_id TEXT, spec_id TEXT, project_path TEXT NOT NULL, prompt_size_bytes INTEGER, prefix_stable_bytes INTEGER, slice_bytes INTEGER, recipe_bytes INTEGER, wave_slice_bytes INTEGER, return_size_bytes INTEGER, retry_overhead_bytes INTEGER)` + índices em `(project_path, ts)` e `(agent_id, ts)`
  - Tabela `spans` já existe — apenas confirma schema compatível (não recriar)
- [ ] Re-exportar em `packages/core/src/lib.rs`: adicionar `pub mod economy;` na lista de módulos públicos + adicionar re-exports relevantes na seção `pub use ...` (ex.: `pub use economy::{EconomyScope, EconomySummary, SavingsSource};`).
- [ ] Criar `packages/core/tests/economy_basic.rs` com testes integrados usando `tempfile::tempdir()`:
  - `test_writer_roundtrip_span`: grava 1 SpanRecord, lê de volta via reader em scope Project, valida igualdade
  - `test_writer_roundtrip_savings`: idem para SavingsRecord
  - `test_writer_roundtrip_context_cost`: idem para ContextCostFrame
  - `test_reader_scope_project_aggregates`: grava 3 spans + 2 savings em 2 specs diferentes, valida que scope Project soma tudo e scope Spec filtra
  - `test_multi_project_reader_fanout`: cria 2 DBs temp em diretórios diferentes, grava records em cada, chama scope AllProjects, valida merge correto
  - `test_estimator_within_tolerance`: estimate_input_tokens em string conhecida para `claude-3-5-sonnet`, valida que está em range esperado (±5%)
- [ ] Rodar `cargo check -p mustard-core` e `cargo test -p mustard-core` — ambos devem passar.

## Dependências

Nenhuma. Primeira wave, foundation.

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Paralela a: [[wave-5-ds-foundation]] (ambas podem iniciar dia 1)
- Desbloqueia: [[wave-2-hooks-real]], [[wave-3-ingestion]]
- Grava memória: `{types_added: [...], writer_methods: [...], reader_methods: [...], scope_variants: [...], pricing_table: "..."}` para [[wave-2-hooks-real]] e [[wave-3-ingestion]] consumirem.

## Limites

Em escopo: `packages/core/Cargo.toml`, `packages/core/src/lib.rs`, `packages/core/src/economy/**` (todos os arquivos novos), `packages/core/src/store/migrations.rs` (APPEND apenas), `packages/core/tests/economy_basic.rs`.

Fora de escopo: qualquer outro arquivo. NÃO alterar `core::store::sqlite_store`, `core::store::event_store`, `core::projection::*`, `core::reader::*`, `core::knowledge`, `core::metrics`. NÃO tocar em `apps/rt/**`, `apps/dashboard/**`, `apps/cli/**`. NÃO modificar migrations existentes (apenas adicionar).

## Concerns

- **ALTER TABLE `spans` em vez de tabela nova** — non-goal global do parent diz "Não alterar tabelas existentes (events, spans) — apenas adicionar novas via migration adicional". Implementação adicionou `cost_usd_micros`, `cache_read_input_tokens`, `cache_creation_input_tokens`, `project_path`, `ts_iso`, `session_id`, `wave_id` à `spans` via `add_column_if_missing` ALTER probes (re-runnable). Justificativa do agente: campos requisitados pelo `SpanRecord` em `economy::model` não existiam no schema legado. Alternativas a avaliar no REVIEW: (a) aceitar como interpretação válida de "adicionar via migration adicional" (a migration é nova, só toca colunas), (b) refatorar para `economy_spans` separada e deixar `spans` legado intacto, (c) escopar mais campos via JSON em `payload` em vez de colunas tipadas. Decisão final fica para REVIEW da entrega completa.
- **Custos per-agent/per-wave aproximados (W2 debt)** — schema W1 não liga `spans` ↔ agente diretamente. `reader::per_agent_costs` e `per_wave_costs` distribuem proporcionalmente por share de `ContextCostFrame`. Documentado em `reader.rs:147-152`. API shape estável; precisão real chega em W4 (Atribuição).
