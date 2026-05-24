# Feature: eliminate-bun

> Continuação do roadmap Parte B. B3/B4/B5 portaram hooks, scripts e a CLI para Rust. Esta spec elimina o último resíduo de runtime não-Rust da ferramenta — o servidor MCP `mustard-memory` em TypeScript/`bun` — consolida o storage do harness em SQLite único, e fecha a migração de instalações antigas. Informada pela investigação de 2026-05-19 (auditoria de código morto + sondagem de `templates/` + decisão de arquitetura de storage).

## Contexto

O Mustard como ferramenta não deve ter nenhum runtime não-Rust (ver memória `project_no_bun_rust_only`). Resta um resíduo: o servidor MCP `mustard-memory` (`apps/cli/src/mcp/mustard-memory.ts`, 205 LOC) ainda é TypeScript e roda via `bun dist/mcp/mustard-memory.js`. Ele ancora toda a ilha TS de `apps/cli/` (`tsconfig.json`, `eslint.config.js`, `dist/`, `tests/*.cjs`, `runtime/event-store.ts`, `migrate/jsonl-to-sqlite.ts`).

Há também duplicação de storage: o harness usa `events.jsonl` (NDJSON append-only) como truth-source enquanto o MCP/OTEL usa `mustard.db` (SQLite) — dois stores + um sync. Decisão de 2026-05-19: **consolidar em SQLite único** (`mustard.db`, modo WAL) como store de escrita+leitura, eliminando o `events.jsonl`.

Além disso, o `.claude/` raiz **deste repo** é uma instalação stale: o `settings.json` ainda dispara hooks JS, não o binário `mustard-rt` do B3/B4.

## Resumo

(1) Adicionar ao `mustard-core` um `SqliteEventStore` (WAL) que substitui o `JsonlEventStore` como store único do harness; (2) re-cablear a emissão e a leitura de eventos do `mustard-rt` para esse store, eliminando o `events.jsonl`; (3) re-portar `mustard-memory` para Rust como face `mcp` do `mustard-rt`, consumindo o store; (4) deletar a ilha TS, eliminar `bun` de `templates/`, completar o RTK auto-installer, corrigir o `CLAUDE.md` raiz; (5) migrar o `.claude/` raiz deste repo para `mustard-rt` como validação.

## Entidades

N/A — infraestrutura (storage, runtime, MCP, distribuição).

## Component Contract

N/A.

## Arquivos

- `packages/core/src/io/sqlite_store.rs` (novo) — `SqliteEventStore` (WAL); substitui o `JsonlEventStore` como store do harness
- `packages/core/src/io/event_store.rs` — remover o `JsonlEventStore` (substituído pelo SQLite) na Wave 2
- `packages/core/src/io/mod.rs`, `packages/core/src/lib.rs`
- `Cargo.toml` raiz — promover `rusqlite` (bundled); adicionar `rmcp`, `tokio`; **bump de TODAS as deps a latest stable**
- `apps/rt/src/` — emissão de evento dos hooks + leitores (`harness-views`, `event-projections`, pre-compact snapshot, session-knowledge) passam a usar o `SqliteEventStore`
- `apps/rt/src/mcp/` (novo) + `apps/rt/src/main.rs` — face `mcp`
- `apps/rt/Cargo.toml` — opt-in de `rmcp`/`tokio`
- `apps/cli/src/commands/init.rs` — RTK auto-installer; `ensure_global_permissions` opt-out
- **Deletar** (ilha TS): `apps/cli/src/mcp/`, `apps/cli/src/migrate/`, `apps/cli/src/runtime/event-store.ts`, `apps/cli/src/runtime/{tsconfig.json,schema.sql}`, `apps/cli/{package.json,tsconfig.json,eslint.config.js}`, `apps/cli/dist/`, `apps/cli/tests/`, `apps/cli/.claude/`
- `apps/cli/templates/settings.json`, `apps/cli/templates/CLAUDE.md` — remover `bun`
- `CLAUDE.md` raiz — corrigir `## Structure` e frases stale
- `.claude/settings.json`, `.claude/hooks/`, `.claude/scripts/` (raiz) — migração

## Limites

- `packages/core`, `apps/rt`, `apps/cli` (código Rust + remoção da ilha TS), `templates/`, `CLAUDE.md` raiz, e o `.claude/` raiz deste repo.
- **Fora dos limites:** o frontend do dashboard (`apps/dashboard/src/` — React/Vite/pnpm; "sem bun" ≠ "sem node"); o protocolo MCP em si (paridade 1:1 com os 5 tools atuais); o bootstrap de distribuição do `mustard-rt`.

## Tarefas

### Impl Agent (Wave 1) — `SqliteEventStore` em `mustard-core`

- [x] Adicionar `packages/core/src/io/sqlite_store.rs`: store SQLite em **modo WAL** com `busy_timeout`. Schema vindo de `apps/cli/src/runtime/schema.sql` (tabelas de eventos, knowledge, specs, metrics, spans; FTS5 para knowledge).
- [x] API de **escrita**: `append` (INSERT-only — preserva a semântica append-only/auditoria).
- [x] API de **leitura**: `replay`, `query` (eventos), `search`/knowledge (FTS5 `bm25`), `specs`, `metrics`, `spans`.
- [x] Promover `rusqlite` (feature `bundled`) a `[workspace.dependencies]`; `mustard-core` opta-in. Respeitar `MUSTARD_DB_PATH` (default `.claude/.harness/mustard.db`).
- [x] Testes contra um `mustard.db` temporário (`tempfile`).

### Impl Agent (Wave 2) — re-cablear `mustard-rt` para SQLite

- [x] A emissão de evento de todos os hooks (`mustard-rt on <evento>`) passa a escrever via `SqliteEventStore` — fim do `append` em `events.jsonl`.
- [x] Os leitores (`harness-views`, `event-projections`, snapshot de pre-compact, extração de session-knowledge) passam a consultar o `SqliteEventStore`.
- [x] Remover a rotação de `events.jsonl` do `harness-init` (WAL não precisa de rotação de arquivo).
- [x] Remover o `JsonlEventStore` de `mustard-core` — código morto após o rewiring.
- [x] `cargo test -p mustard-rt` verde.

### Impl Agent (Wave 3) — face `mcp` em `mustard-rt`

- [x] Adicionar `rmcp` (`features = ["server","transport-io"]`) + `tokio` (`rt`,`io-std`,`macros`) a `[workspace.dependencies]`; `apps/rt` opta-in. **Latest stable verificado em crates.io no EXECUTE** — em 2026-05-19: `rmcp` 1.7.0, `tokio` 1.52. Atenção: `rmcp` 1.x tem API diferente da série 0.x.
- [x] Nova face `mcp` em `mustard-rt` (subcomando `clap`, padrão das faces `on`/`run`/`check`); `tokio` escopado só a essa face — `on`/`run`/`check` seguem síncronos.
- [x] Portar os 5 tool handlers de `mustard-memory.ts` (`search_knowledge`, `query_events`, `find_similar_specs`, `get_spec_metrics`, `get_span_summary`) consumindo o `SqliteEventStore`.
- [x] Testes de integração JSON-RPC (`initialize` + cada tool).

### Impl Agent (Wave 4) — eliminar `bun` + limpeza

- [x] `templates/settings.json`: `mcpServers.mustard-memory` → `{"command":"mustard-rt","args":["mcp"]}`.
- [x] Deletar a ilha TS de `apps/cli/` (lista em `## Arquivos`).
- [x] `templates/CLAUDE.md`: `## Stack` deixa de declarar "Bun (>=1.2.0)" — reflete o runtime Rust.
- [x] `CLAUDE.md` raiz: corrigir `## Structure` (sem `src/scanners`, `src/generators`, `dist/`; CLI é crate Rust), `## Build & Run` (sem `bun bin/mustard.js`), e frases que descrevem hooks/scripts como JavaScript.
- [x] `init.rs`: completar `ensure_rtk()` — auto-install quando ausente (Unix: `curl … install.sh | sh`; Windows: `cargo install --git` / Scoop). Fail-soft.
- [x] `init.rs`/`update.rs`: tornar `ensure_global_permissions()` opt-out — não escrever em `~/.claude/settings.json` por padrão.
- [x] Bump de TODAS as `[workspace.dependencies]` (`serde`, `serde_json`, `thiserror`, `anyhow`, `clap`, `jiff`, `insta`, `tempfile`, `ureq`, `tar`, `flate2`, `dialoguer`, `zip` + as novas) a latest stable verificada em crates.io; `cargo update`; build verde. (`rusqlite` mantido em 0.31 — ver `## Preocupações`.)

### Impl Agent (Wave 5) — migrar o `.claude/` raiz deste repo

- [x] Backup de `.claude/`; substituir `.claude/settings.json` pela versão `mustard-rt` de `templates/`.
- [x] Re-copiar `templates/commands/mustard/` → `.claude/commands/mustard/` (os SKILL.md passam a invocar `mustard-rt run`).
- [x] Deletar `.claude/hooks/` e `.claude/scripts/` (JS stale) e o `events.jsonl` legado (descartado — sem importação).
- [x] `mustard-rt` já instalado no PATH via `cargo install --path apps/rt` (feito em 2026-05-19).

## Dependências

- Wave 2 e Wave 3 dependem de Wave 1 (ambas consomem o `SqliteEventStore`).
- Wave 4 depende de Wave 3 (`templates/settings.json` aponta para `mustard-rt mcp`).
- Wave 5 depende de Wave 4.
- `rmcp` — SDK oficial de Rust do Model Context Protocol (crates.io); usar latest (1.7.0 em 2026-05-19).

## Preocupações

- **A consolidação de storage é um refactor de camada:** a Wave 2 toca tudo que lê/escreve `events.jsonl` no `mustard-rt`. Se ficar grande demais, o wave-size audit do `/approve` pode dividi-la (emissão / leitores).
- **Concorrência de hooks:** WAL + `busy_timeout` absorve hooks paralelos; escritas são um `INSERT` único (sub-ms). Hook é fail-open — se a escrita expirar, o evento se perde mas o trabalho do usuário continua.
- **`rmcp` 1.x:** API estável, diferente da série 0.x — o agente da Wave 3 deve usar a doc da 1.7.
- **`rmcp` exige `tokio`** (async-only) — escopar `tokio` à face `mcp` (runtime `current_thread` local); `main.rs` despacha `mcp` cedo, como já faz com `run`.
- **`ensure_global_permissions` escreve global:** o `update`/`init` portados mutam `~/.claude/settings.json` — contra a política do usuário. Wave 4 torna isso opt-out.
- **Os 9 `.ts` da CLI já foram deletados** no b5 EXECUTE (`git status` mostra `D`). A Wave 4 lida apenas com a ilha TS restante (`mcp/`, `migrate/`, `event-store.ts`, infra node).
- **A migração do `.claude/` raiz só surte efeito na próxima sessão** — o Claude Code carrega `settings.json`/hooks no início. Deletar `hooks/` no meio da sessão degrada o enforcement corrente (fail-open, tolerável).
- **Sem preservação de histórico:** o `events.jsonl` legado é descartado — a migração começa com um `mustard.db` limpo. Histórico de eventos é telemetria operacional, não dado de usuário; não há importador.
- **[CONCERN — Wave 1] `rusqlite` fixado em 0.31, não em "latest stable":** `libsqlite3-sys` carrega `links = "sqlite3"`, então o workspace Cargo só pode linkar UMA versão. `apps/dashboard/src-tauri/Cargo.toml` já fixa `rusqlite = "0.31"`. Subir o `mustard-core` para 0.39 isoladamente é um conflito de link irresolvível. A Wave 1 alinhou em `0.31` (código API-compatível com ambas as linhas). **Ação para a Wave 4:** o bump `rusqlite` 0.31→latest deve ser feito para `mustard-core` E `apps/dashboard/src-tauri` em lockstep — ou o feature fica em 0.31. O bump de deps da Wave 4 precisa tratar isso explicitamente.
- **[CONCERN — Wave 2] O backend Tauri do dashboard lê `events.jsonl` diretamente:** `apps/dashboard/src-tauri/{telemetry,lib,watcher,discovery}.rs` consome `.claude/.harness/events.jsonl` — arquivo que a Wave 2 deixou de escrever. O dashboard está fora dos limites desta spec, então a Wave 2 não re-cablou esses leitores: até serem migrados para `SqliteEventStore`, o dashboard lê dados vazios/stale. **Follow-up necessário** (spec separada): re-cablar o `src-tauri` do dashboard para o `SqliteEventStore`. Decisão a tomar no CLOSE.
- **[NOTE — Wave 2] Teste flaky:** `run::otel::collector::tests::unknown_route_is_404` falha intermitentemente no Windows (socket reset `code: 10054`); passa isolado, sem relação com a migração de storage. OTEL tem store SQLite próprio, intocado.
- **[REVIEW WARNING] `npm.rs`/`auto_update.rs` desatualizado:** `PACKAGE_NAME` ainda é `"mustard-claude"` e o auto-update roda `npm install -g mustard-claude@latest` — semanticamente quebrado agora que a CLI é binário Rust. **Fora do escopo desta spec** (distribuição = B6+, ver `## Não-Objetivos`). Follow-up: depreciar ou re-cablear o auto-update no B6.
- **[REVIEW WARNING] WAL pragma silencioso:** `sqlite_store.rs` descarta o resultado de `PRAGMA journal_mode = WAL`. Em FS que não suporta WAL (NFS, alguns paths Windows) o SQLite cai para DELETE mode sem erro — `busy_timeout` vira a única defesa de contenção. Baixo risco no path normal `.claude/.harness/`. Follow-up opcional: logar em stderr quando o mode efetivo ≠ WAL.
- **[REVIEW WARNING] `mcp query_events` faz full replay:** o tool da face `mcp` replaya o log inteiro e filtra em memória em vez de usar o `WHERE spec = ?` de `SqliteEventStore::query`. Aceitável para uso MCP (read-only, baixa frequência); otimização SQL é trivial se virar hot path.
- **[REVIEW] Correção do `CLAUDE.md` raiz não commitada:** a Wave 4 corrigiu o `CLAUDE.md` raiz na working tree, mas o branch ainda não foi commitado — o HEAD atual mantém referências stale a `bun`/`packages/cli`. Resolve-se ao commitar o branch (ver nota de CLOSE).

## Critérios de Aceitação

- [x] AC-1: O workspace compila — Command: `bash -c 'cargo build --workspace'`
- [x] AC-2: Testes de core e rt verdes — Command: `bash -c 'cargo test -p mustard-core -p mustard-rt'`
- [x] AC-3: A face `mcp` responde ao handshake/tools — Command: `bash -c 'cargo test -p mustard-rt mcp'`
- [x] AC-4: Sem `bun` no payload — Command: `bash -c 'cd apps/cli/templates && ! grep -rln "\"bun\"" settings.json CLAUDE.md'`
- [x] AC-5: Ilha TS removida — Command: `bash -c 'test ! -d apps/cli/src/mcp && test ! -f apps/cli/package.json'`

## Não-Objetivos

- Não mexer no frontend do dashboard — React/Vite/pnpm é node, não bun; permanece.
- Não re-projetar o protocolo MCP — paridade 1:1 com os 5 tools atuais.
- Não preservar o histórico do `events.jsonl` — a migração começa com um `mustard.db` limpo; não há importador.
- Não construir o bootstrap de distribuição do `mustard-rt` — decisão de B6+.
