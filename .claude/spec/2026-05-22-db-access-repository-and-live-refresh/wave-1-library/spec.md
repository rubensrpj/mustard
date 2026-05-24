# wave-1-library — Repository único + schema fast-path

## Resumo

O ponto único de acesso a dados **já existe**: `store::SqliteEventStore` é "the
single store the harness reads from and writes to" (doc de `store/mod.rs`), já
trait-backed via `EventSink` (DIP). Esta wave NÃO cria módulo novo — ela torna
esse store (a) barato de abrir (fast-path via `user_version`) e (b) reusável em
vez de reconstruído a cada chamada, e move para dentro dele as consultas que hoje
abrem conexão própria no leitor. Para o consumo multi-projeto (dashboard),
acrescenta um cache fino por caminho dentro do próprio módulo `store`.

## Causa raiz

`SqliteEventStore::new` (`packages/core/src/store/sqlite_store.rs:195-218`) roda
incondicionalmente, a cada construção: `PRAGMA journal_mode=WAL`,
`execute_batch(SCHEMA_SQL)` (todo o DDL) e `migrations::apply`. O leitor
(`packages/core/src/reader/sqlite.rs`) constrói um `SqliteEventStore` por método
e, em `list_specs` (linha ~289), chama `spec_view` dentro de um laço (N+1).
`workspace_summary` chama `replay()` sem filtro (full scan).

## Arquivos

- `packages/core/src/store/sqlite_store.rs` — fast-path no open de `SqliteEventStore::new` via `user_version`; `synchronous=NORMAL` (WAL e `busy_timeout` já existem nas linhas 205-210)
- `packages/core/src/store/migrations.rs` — gravar `PRAGMA user_version = SCHEMA_VERSION` ao final; ler versão uma vez na escada (evitar re-query por passo)
- `packages/core/src/store/sqlite_schema.sql` — `idx_events_session_id`; índice composto para o sort de `knowledge_patterns (confidence, last_seen)`
- `packages/core/src/store/db_cache.rs` (novo, dentro do módulo `store`) — cache fino `Mutex<HashMap<PathBuf, SqliteEventStore>>` reusável por consumidores multi-projeto; declarar em `store/mod.rs`
- `packages/core/src/reader/sqlite.rs` — `list_specs` agregado (uma query, sem laço de `spec_view`); reusar um `SqliteEventStore` em vez de abrir por método; `replay()` com janela de tempo opcional

## Tarefas

### Library Agent (Wave 1)

- [ ] Adicionar fast-path no open: ler `PRAGMA user_version`; se == `SCHEMA_VERSION`, pular DDL + migrações; caso contrário aplicar DDL + migrações e setar `user_version`. Manter pragmas por conexão (`busy_timeout`, `synchronous=NORMAL`) sempre.
- [ ] `migrations::apply`: capturar a versão uma vez e incrementar localmente; gravar `PRAGMA user_version` ao concluir a versão mais recente.
- [ ] Schema SQL: criar `idx_events_session_id` e índice composto para a ordenação de `knowledge_patterns`. Bump de `SCHEMA_VERSION` + passo de migração para os índices.
- [ ] Adicionar `store/db_cache.rs`: cache `Mutex<HashMap<PathBuf, SqliteEventStore>>` que abre-uma-vez-e-reusa por caminho (para o dashboard multi-projeto da Wave 3). Reaproveita o trait `EventSink` já existente — sem novo trait paralelo.
- [ ] Reescrever `list_specs` como uma query agregada única (sem laço de `spec_view`); fazer o reader reusar um `SqliteEventStore` em vez de abrir por método; adicionar parâmetro de janela de tempo a `replay()` usado por `workspace_summary`.
- [ ] Adicionar método de prune (`prune_events_older_than(days)`) para retenção da tabela `events`.
- [ ] Testes: (a) segundo open de um DB já na versão atual NÃO roda migrações; (b) `list_specs` usa contagem de aberturas constante via fake; (c) prune remove o esperado. Rodar `cargo test -p mustard-core`.

## Critérios de Aceitação

- [ ] AC-1: `cargo build -p mustard-core` passa — Command: `cargo build -p mustard-core`
- [ ] AC-2: `cargo test -p mustard-core` passa — Command: `cargo test -p mustard-core`
- [ ] AC-3: `user_version` usado como gate — Command: `bash -c "grep -rq 'user_version' packages/core/src/store && echo ok"`

## Limites

- `packages/core/src/store/**` (incl. novo `db_cache.rs`), `packages/core/src/reader/sqlite.rs`
- NÃO criar módulo de acesso paralelo — consolidar no módulo `store` existente (o ponto único já é `SqliteEventStore`)
- NÃO tocar em `apps/rt` nem `apps/dashboard` nesta wave (são 2 e 3)
- NÃO adicionar dependência de pool externo (`r2d2`/`deadpool`)
