# Wave 3 — Ingestão externa: adapters OTEL + JSONL + RTK

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: completed
### Phase: EXECUTE
### Scope: full (wave)
### Checkpoint: 2026-05-21T04:35:00Z
### Lang: pt

## PRD

Três fontes de telemetria de custo/economia hoje vivem desligadas do `economy::writer` unificado entregue na W1: (a) o coletor OTEL existe em `apps/rt/src/run/otel/` mas ninguém o spawna no `SessionStart`, então spans `gen_ai.*` nunca chegam ao SQLite; (b) o transcript JSONL de cada sessão Claude Code contém `message.usage` linha-a-linha, mas nenhum parser nosso o lê — perdemos a fonte mais barata de `ApiCostFrame`; (c) `mustard-rt run rtk-gain` já calcula tokens economizados via rewriter, mas imprime JSON pro stdout sem persistir em `savings_records`. A W3 fecha as três pontas: cria adapters puros em `mustard_core::economy::sources::{otel, transcript, rtk}` que traduzem o formato externo em records do W1, e fia-os nos hooks/subcomandos do rt para que cada fonte produza linhas reais no banco a cada sessão.

## Contexto

W1 fechou a API de escrita (`record_span`, `record_savings`, `record_api_cost`, `record_context_cost`) e deixou `sources/mod.rs` vazio como placeholder. W2 já provou o pattern de ler `MUSTARD_DB_PATH`/`.claude/.harness/mustard.db` em hooks via `SqliteEventStore::for_project`, mas espalhou 5 cópias do helper — W3 consolida em `economy::store::open_for` e usa nos adapters novos. Refactor dos 5 hooks W2 fica fora de escopo (já estão verdes).

## Usuários/Stakeholders

- Operador da pipeline (lê painel de economia e espera ver 3 fontes alimentando o banco).
- Dashboard de economia (consumidor downstream das tabelas `spans`, `savings_records`).
- Agentes Claude Code rodando localmente (geram o JSONL e os spans OTEL).

## Métrica de sucesso

Após uma sessão Claude Code real com `MUSTARD_TRANSCRIPT_WATCH=1` setado: `SELECT COUNT(*) FROM spans WHERE session_id=?` retorna >0 (OTEL alimentando), e `SELECT COUNT(*) FROM savings_records WHERE source='RtkRewrite'` cresce após `mustard-rt run rtk-gain`.

## Não-Objetivos

- Não refatorar os 5 hooks do W2 (já verdes; consolidação opcional via novo `open_for`).
- Não implementar nova UI de dashboard (consumidor downstream das waves 6-7).
- Não migrar OTLP/gRPC — apenas OTLP/JSON local que o collector existente já decoda.
- Não tocar pricing table (vem do W1 `estimator::model_pricing_usd_micros_per_million`).
- Não fazer backfill histórico de transcripts antigos (apenas sessão corrente).

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build do rt + core passa — Command: `cargo check -p mustard-rt && cargo check -p mustard-core`
- [x] AC-2: Adapter OTEL existe — Command: `node -e "if(!require('fs').existsSync('packages/core/src/economy/sources/otel.rs'))throw new Error('otel.rs missing')"`
- [x] AC-3: Adapter JSONL existe — Command: `node -e "if(!require('fs').existsSync('packages/core/src/economy/sources/transcript.rs'))throw new Error('transcript.rs missing')"`
- [x] AC-4: Adapter RTK existe — Command: `node -e "if(!require('fs').existsSync('packages/core/src/economy/sources/rtk.rs'))throw new Error('rtk.rs missing')"`
- [x] AC-5: `session_start.rs` faz spawn do collector — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/session_start.rs','utf8');if(!t.includes('otel-collector'))throw new Error('session_start missing collector spawn')"`
- [x] AC-6: Hook `SessionEnd` parseia JSONL — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/session_cleanup.rs','utf8');if(!t.includes('transcript')&&!t.includes('record_api_cost'))throw new Error('SessionEnd missing transcript parse call')"`
- [x] AC-7: Helper `economy::store::open_for` consolidado — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/store.rs','utf8');if(!t.includes('pub fn open_for'))throw new Error('open_for helper missing')"`
- [x] AC-8: Daemon `transcript_watcher` existe — Command: `node -e "if(!require('fs').existsSync('apps/rt/src/run/transcript_watcher.rs'))throw new Error('transcript_watcher.rs missing')"`
- [x] AC-9: Subcomando `transcript-watcher` registrado — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/main.rs','utf8');if(!t.includes('transcript-watcher')&&!t.includes('transcript_watcher'))throw new Error('subcommand not registered')"`
- [x] AC-10: Tests core economy::sources passam — Command: `cargo test -p mustard-core economy`

## Plano

Três adapters em `packages/core/src/economy/sources/{otel,transcript,rtk}.rs`, cada um expondo `pub fn ingest(...) -> Result<Vec<Record>>` que retorna records traduzidos sem tocar banco (caller gerencia escrita). Hooks no `rt` chamam:

- `session_start.rs` — spawn detachado do `mustard-rt run otel-collector` + escreve PID em `.claude/.harness/.otel-collector.pid` (fecha gap da migração b3). Spawn idempotente (skip se PID file existe e processo está vivo).
- `session_cleanup.rs` (`SessionEnd`) — invoca `sources::transcript::ingest(session_jsonl_path)` e chama `writer::record_api_cost()` para cada frame retornado.
- Adapter OTEL substitui a leitura ad-hoc em `apps/rt/src/run/otel/mod.rs` — passa a delegar tradução para `sources::otel::ingest()` e chamar `writer::record_span()` por record.
- Adapter RTK substitui implementação interna de `apps/rt/src/run/rtk_gain.rs` (que vira thin wrapper sobre `sources::rtk::ingest()` + persiste savings antes de imprimir o JSON legado).
- Watcher opcional em `apps/rt/src/run/transcript_watcher.rs` — daemon spawned por `session_start` quando `MUSTARD_TRANSCRIPT_WATCH=1`. Subcomando `mustard-rt run transcript-watcher`.

W3 será despachada em duas sub-waves seriais: **3a** entrega adapters em `mustard-core` (library agent), **3b** liga eles aos hooks/subcomandos em `mustard-rt` (backend agent) consumindo a API recém-fechada de 3a.

## Informações da Entidade

Consome API do módulo `economy` (W1) e adiciona 1 helper consolidado de conexão. Sem entidade nova.

## Arquivos (~12)

```
packages/core/src/economy/sources/otel.rs        (new)
packages/core/src/economy/sources/transcript.rs  (new)
packages/core/src/economy/sources/rtk.rs         (new)
packages/core/src/economy/sources/mod.rs         (modify — re-export os 3 adapters + IngestContext)
packages/core/src/economy/store.rs               (new — open_for helper consolidado)
packages/core/src/economy/mod.rs                 (modify — pub mod store + re-export)
apps/rt/src/hooks/session_start.rs               (modify — spawn collector + opcional spawn watcher)
apps/rt/src/hooks/session_cleanup.rs             (modify — parse transcript no SessionEnd + record_api_cost)
apps/rt/src/run/rtk_gain.rs                      (modify — vira wrapper sobre sources::rtk)
apps/rt/src/run/otel/mod.rs                      (modify — usa sources::otel ao traduzir spans)
apps/rt/src/run/transcript_watcher.rs            (new — daemon opcional)
apps/rt/src/main.rs                              (modify — registrar subcomando transcript-watcher)
```

## Tarefas

### Core Sources Agent (3a)

- [ ] Criar `packages/core/src/economy/store.rs` com `pub fn open_for(project_path: &str) -> Result<rusqlite::Connection>` — abre `MUSTARD_DB_PATH` ou `<project>/.claude/.harness/mustard.db`, garante schema via `SqliteEventStore::for_project(...)`, retorna `Connection` raw pronta pra writer. Documentar contrato (fail-open: caller deve `match` no Result e logar).
- [ ] Adicionar `pub mod store;` + re-export em `packages/core/src/economy/mod.rs`.
- [ ] Definir `pub struct IngestContext { pub project_path: String, pub session_id: Option<String> }` em `packages/core/src/economy/sources/mod.rs` — compartilhado pelos 3 adapters.
- [ ] Criar `packages/core/src/economy/sources/otel.rs` com `pub fn ingest(otlp_json: &str, ctx: &IngestContext) -> Result<Vec<SpanRecord>>` — parse OTLP/JSON traces (use `serde_json::Value` lenient), extract spans com atributo `gen_ai.usage.*`, traduz para `SpanRecord` (model, input_tokens, output_tokens, cache_*, cost_usd_micros via `estimator::model_pricing_usd_micros_per_million`, session_id, request_id, ts).
- [ ] Criar `packages/core/src/economy/sources/transcript.rs` com `pub fn ingest(transcript_path: &Path, ctx: &IngestContext) -> Result<Vec<ApiCostFrame>>` — abre arquivo JSONL line-by-line, parse cada linha como `serde_json::Value`, extrai `message.usage.{input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens}` quando presente, monta `ApiCostFrame` com `cost_usd_micros` calculado via pricing table do estimator. Tolerante a linhas malformadas (log warn + skip).
- [ ] Criar `packages/core/src/economy/sources/rtk.rs` com `pub fn ingest(ctx: &IngestContext) -> Result<Vec<SavingsRecord>>` — `Command::new(env::var("MUSTARD_RTK_BIN").unwrap_or_else(|_| "rtk".into())).args(["gain", "--json"]).output()`, parse stdout como JSON, mapeia cada entry para `SavingsRecord { source: SavingsSource::RtkRewrite, tokens_saved, model_target: None, project_path: ctx.project_path.clone(), spec_id: None, wave_id: None, agent_id: None, ts: now_epoch_ms() }`. Fail-open se `rtk` não existe (warn + Vec vazio).
- [ ] Atualizar `packages/core/src/economy/sources/mod.rs`: `pub mod otel; pub mod transcript; pub mod rtk;` + re-export de `IngestContext`.
- [ ] Adicionar 1 teste unitário por adapter (fixtures inline) em `packages/core/tests/economy_sources.rs` ou módulo `#[cfg(test)]` em cada adapter.
- [ ] Rodar `cargo check -p mustard-core` e `cargo test -p mustard-core` — passar.

### RT Ingestion Agent (3b — DEPENDE de 3a completa)

- [ ] **`apps/rt/src/hooks/session_start.rs`** — adicionar ao final do `on_session_start` (ou equivalente): spawn detachado do collector via `Command::new(env::current_exe()?).args(["run", "otel-collector"]).spawn()?`, escrever PID em `<project>/.claude/.harness/.otel-collector.pid`. Pre-check: se PID file existe e processo está vivo (helper local que testa `kill -0` em unix ou `OpenProcess` em windows), skip spawn. Adicionar spawn opcional do watcher se `env::var("MUSTARD_TRANSCRIPT_WATCH").as_deref() == Ok("1")`: `Command::new(env::current_exe()?).args(["run", "transcript-watcher"]).spawn()?`. Fail-open em todos os spawns (warn, não panic).
- [ ] **`apps/rt/src/hooks/session_cleanup.rs`** — no handler do `SessionEnd`, resolver transcript path via env `CLAUDE_TRANSCRIPT_PATH` (Claude Code injeta) ou fallback `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` (use `dirs::home_dir()` + URL-encode do cwd). Se path existe, chamar `mustard_core::economy::sources::transcript::ingest(&path, &ctx)?`. Para cada `ApiCostFrame` retornado, chamar `mustard_core::economy::writer::record_api_cost(&conn, frame)?`. Connection vem do novo `economy::store::open_for(project_path)`. Fail-open.
- [ ] **`apps/rt/src/run/rtk_gain.rs`** — refatorar implementação interna: manter CLI args/output identicos (`mustard-rt run rtk-gain` continua imprimindo JSON), mas internamente substituir o `Command::new("rtk").args(["gain","--json"])` ad-hoc por `let records = mustard_core::economy::sources::rtk::ingest(&ctx)?;` seguido de `for r in &records { writer::record_savings(&conn, r.clone())?; }` antes de imprimir. Saída JSON mantém compat backward.
- [ ] **`apps/rt/src/run/otel/mod.rs`** — onde hoje o collector loop lê spans, ao invés de traduzir inline, delegar para `mustard_core::economy::sources::otel::ingest(&otlp_json, &ctx)?`, e gravar cada `SpanRecord` via `writer::record_span`. Manter shape do collector daemon (loop, decoding HTTP/gRPC).
- [ ] **`apps/rt/src/run/transcript_watcher.rs`** (NEW) — daemon `pub fn run() -> Result<()>` que usa `notify::recommended_watcher` para vigiar `~/.claude/projects/`. Em evento `Modify`/`Create` em `*.jsonl`, chamar `sources::transcript::ingest(&path, &ctx)` + writer fan-out. Sair limpo no SIGINT. Confirmar via cargo que `notify` está em workspace deps; se não, adicionar `notify = "6"` no workspace `Cargo.toml`.
- [ ] **`apps/rt/src/main.rs`** — registrar novo subcomando `transcript-watcher` no match de `run` subcommands, mapeando para `transcript_watcher::run()`.
- [ ] Rodar `cargo check -p mustard-rt` e `cargo test -p mustard-rt` — passar.

## Dependências

- [[wave-1-core-economy]]: writer API + tipos de record + facade.
- [[wave-2-hooks-real]] (informativo): pattern de conexão SQLite via `SqliteEventStore::for_project` — agora encapsulado em `economy::store::open_for`.
- Externa: binário `rtk` no PATH (ou `MUSTARD_RTK_BIN`). Ausência é fail-open.
- Crate `notify` (verificar workspace deps; adicionar `notify = "6"` se faltar).

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Depende de: [[wave-1-core-economy]]
- Paralela a: [[wave-2-hooks-real]] (independentes — uma instrumenta internamente, outra absorve externamente)
- Desbloqueia: [[wave-4-attribution]]
- Grava memória: `{adapters: ['otel','transcript','rtk'], pid_path: '.claude/.harness/.otel-collector.pid', watcher_env: 'MUSTARD_TRANSCRIPT_WATCH', open_for_helper: 'mustard_core::economy::store::open_for'}` para [[wave-4-attribution]]

## Limites

Em escopo: `packages/core/src/economy/sources/{otel,transcript,rtk}.rs`, `packages/core/src/economy/sources/mod.rs`, `packages/core/src/economy/store.rs` (NEW — open_for helper), `packages/core/src/economy/mod.rs` (re-export), `apps/rt/src/hooks/{session_start,session_cleanup}.rs`, `apps/rt/src/run/otel/mod.rs` (refactor para usar sources), `apps/rt/src/run/rtk_gain.rs` (vira wrapper), `apps/rt/src/run/transcript_watcher.rs` (novo), `apps/rt/src/main.rs` (registrar subcomando).

Fora de escopo: outros hooks, dashboard, qualquer alteração de schema, refactor dos 5 hooks W2 para usar `open_for`, suporte OTLP/gRPC, backfill histórico de transcripts.

## Concerns

- **`notify` crate ausente do workspace `Cargo.toml`** — 3a não adicionou para preservar boundary (root Cargo.toml é território backend/3b). 3b precisa validar a versão estável na crates.io e declarar `notify = "6"` (ou current) em `[workspace.dependencies]` antes de implementar `transcript_watcher.rs`. Sem isso, `cargo check` quebra.
- **RTK adapter usa trait `RtkCommand` para testabilidade** — `RealRtkCommand` é a impl de produção; tests injetam `FakeRtk`. Live-process test fica `#[ignore]` para CI sem `rtk` no PATH. Decisão a confirmar no REVIEW: faz sentido expor o trait pública? Hoje é `pub` para permitir injection externa (W3b pode passar fake em testes integrados).
- **`open_for` constrói-e-descarta `SqliteEventStore`** para disparar migrations, depois reabre `Connection` raw. Trade-off: 2 `open` calls em vez de 1, em troca de não duplicar lógica de migration. REVIEW pode propor expor `SqliteEventStore::raw_connection()` na W4.
- **`notify = "6"` (não 8.x)** — dashboard `src-tauri` já pinava `notify-debouncer-mini` que arrasta `notify = 6`; subir workspace para 8 quebraria o grafo. 6.x cobre 100% da API consumida (`recommended_watcher`, `RecursiveMode::Recursive`, `EventKind::{Modify,Create}`). REVIEW final pode reavaliar quando dashboard puder bumpar.
- **`is_process_alive` sem `windows-sys`** — Mustard veta `unsafe` na crate; impl usa `tasklist /NH /FI "PID eq <pid>"` em Windows e `kill -0 <pid>` em Unix. Mais lento que `OpenProcess`, mas zero deps novas. Quando o probe falha (binários ausentes), degrada para `false` → spawn novo collector → segundo bind falha cleanly e sai. Seguro mas não ideal — REVIEW pode propor migrar para `sysinfo` ou similar se virar hot-path.
- **`/v1/traces` é rota nova no OTEL collector** — spec dizia "swap inline translation for sources::otel::ingest", mas o collector hoje só tinha `/v1/metrics` e `/v1/logs` (tabela `claude_code_otel`). 3b adicionou `/v1/traces` que alimenta `spans` via writer (canal correto para o pipeline W3 unificado). Decisão a confirmar no REVIEW: deveríamos migrar `metrics`/`logs` para também passar pelo `economy::writer`, ou eles ficam no caminho legado?
