# Feature: eliminate-bun-followups

### Status: draft | Phase: PLAN | Scope: full
### Checkpoint: 2026-05-19T22:30:00Z
### Lang: pt

> Follow-up direto da spec `eliminate-bun` (concluída 2026-05-19, ver `.claude/spec/completed/2026-05-19-eliminate-bun`). Aquela spec consolidou o storage do harness em SQLite único (`mustard.db`) e eliminou o `events.jsonl`, mas deixou explicitamente fora dos limites dois consumidores que ainda dependem do estado antigo. Esta spec fecha esses dois pontos. Informada pelos `CONCERN`s registrados no REVIEW da `eliminate-bun` e por uma sondagem de código de 2026-05-19.

## Contexto

A spec `eliminate-bun` trocou o store do harness: o `events.jsonl` (NDJSON append-only) deixou de ser escrito; o `mustard.db` (SQLite WAL) passou a ser o store único de escrita+leitura. Dois consumidores ficaram para trás:

**(1) Backend Tauri do dashboard.** `apps/dashboard/src-tauri/src/` lê o `events.jsonl` em vários pontos e — pior — o trata como **fonte canônica**: a lógica é "JSONL-first, SQLite como fallback stale" (ver comentários em `lib.rs:569-572`, `telemetry.rs:519-522`, `telemetry.rs:588-589`). Com a `eliminate-bun`, essa premissa inverteu: o `events.jsonl` não existe mais e o `mustard.db` é canônico. Hoje cada leitor `*_from_jsonl` lê um arquivo ausente, retorna vazio e cai no fallback SQLite — funciona por sorte onde há fallback, mas:

- leitores **sem** fallback SQLite (ex.: `agent_activity_from_jsonl`, `lib.rs:1255`) ficam permanentemente vazios;
- o `watcher.rs:33` observa `events.jsonl` para disparar refresh — o arquivo nunca muda, então a atualização ao vivo do dashboard quebra;
- toda sessão paga um stat de arquivo morto antes do fallback;
- os comentários e a ordem de prioridade do código estão factualmente errados agora.

**(2) Auto-update da CLI.** `apps/cli/src/npm.rs` ainda tem `PACKAGE_NAME = "mustard-claude"` e o subcomando `auto-update` (`apps/cli/src/commands/auto_update.rs`) roda `npm view mustard-claude version` + `npm install -g mustard-claude@latest`. A CLI agora é binário Rust nativo; esse pacote npm não é mais o canal de distribuição. O comando faz silenciosamente a coisa errada se invocado.

## Resumo

(1) Re-cablar o backend Tauri do dashboard para ler o harness **somente** do `mustard.db`: remover os leitores `*_from_jsonl`, inverter a lógica de prioridade (SQLite é canônico), garantir paridade SQLite para todo dado hoje servido só via JSONL, e repontar o `watcher` de `events.jsonl` para `mustard.db`. (2) Neutralizar o auto-update npm da CLI: depreciar o subcomando `auto-update` para que não execute `npm install` de um pacote inexistente — o updater real (GitHub releases / `cargo install`) é trabalho de distribuição B6+, fora de escopo aqui.

## Entidades

N/A — infraestrutura (consumidores de telemetria, distribuição da CLI).

## Component Contract

N/A.

## Arquivos

### Área 1 — dashboard `src-tauri`

- `apps/dashboard/src-tauri/src/telemetry.rs` — remover `workflow_by_phase_from_jsonl`, `tool_breakdown_from_jsonl`, `agent_activity_from_jsonl`, o derivador de cut-off de sessão e o tail-reader baseado em JSONL (~linhas 106-160, 519-664, 823-951); manter/estender os caminhos SQLite
- `apps/dashboard/src-tauri/src/lib.rs` — remover `recent_events_from_jsonl`, `summary_from_jsonl_value`, a lógica JSONL-first (`:569-575`), a chamada `agent_activity_from_jsonl` (`:1255`) e a "live activity" derivada de JSONL (`:1270`)
- `apps/dashboard/src-tauri/src/watcher.rs` — `:33` repontar a observação de `events.jsonl` → `mustard.db` (o ramo `mustard.db` já existe em `:35`)
- `apps/dashboard/src-tauri/src/discovery.rs` — `:49` a detecção de projeto-mustard via `events.jsonl` passa a usar `mustard.db` (`:37` já checa o `.db`)
- `apps/dashboard/src-tauri/src/db.rs` — fonte SQLite-only; estender com qualquer leitor que hoje só existe em versão `_from_jsonl` (candidatos: agent activity, tool breakdown, cut-off de sessão — confirmar a paridade no ANALYZE)
- `apps/dashboard/src-tauri/Cargo.toml` — decisão de PLAN: depender de `mustard-core` (reusar `SqliteEventStore`) ou manter o `db.rs` próprio do dashboard (`rusqlite 0.31`, já presente)

### Área 2 — auto-update da CLI

- `apps/cli/src/commands/auto_update.rs` — depreciar: o subcomando passa a imprimir uma mensagem de canal-de-distribuição e NÃO executa `npm install`
- `apps/cli/src/npm.rs` — marcar como legado/morto; remover se nada mais o usa após a depreciação do `auto_update`
- `apps/cli/src/cli.rs` — `:59-68` e `:118-120`: ajustar a declaração/dispatch do subcomando `AutoUpdate`

## Limites

- Área 1: somente `apps/dashboard/src-tauri/src/` (backend Rust do Tauri) + seu `Cargo.toml`. O frontend React (`apps/dashboard/src/`) só muda se um tipo retornado por comando Tauri mudar de forma — preferir manter os tipos estáveis.
- Área 2: somente `apps/cli/` (CLI Rust).
- **Fora dos limites:** construir o updater real da CLI (GitHub releases / self-update) — isso é distribuição B6+; aqui o `auto-update` apenas para de fazer a coisa errada. Não re-projetar o esquema do `mustard.db`. Não tocar no protocolo MCP nem no `mustard-rt`.

## Tarefas

### Impl Agent (Wave 1) — dashboard `src-tauri` lê só do SQLite

- [ ] ANALYZE: enumerar todo leitor `*_from_jsonl` em `telemetry.rs`/`lib.rs` e, para cada um, confirmar se já existe equivalente SQLite em `db.rs`. Listar os gaps de paridade.
- [ ] Decidir (PLAN): o dashboard reusa `mustard-core::SqliteEventStore` (passa a depender da crate; alinha o pin `rusqlite 0.31` automaticamente) ou mantém o `db.rs` próprio. Registrar a decisão.
- [ ] Fechar os gaps de paridade: implementar em `db.rs` (ou via `SqliteEventStore`) os leitores que hoje só existem em JSONL — agent activity, tool breakdown, cut-off de sessão, recent-events tail.
- [ ] Remover os leitores `*_from_jsonl` e a lógica "JSONL-first / SQLite-fallback"; o SQLite passa a ser o único caminho. Corrigir os comentários factualmente errados.
- [ ] `watcher.rs`: repontar a observação de mudança de `events.jsonl` para `mustard.db` (WAL — observar o `.db` e/ou o `-wal`).
- [ ] `discovery.rs`: detecção de projeto-mustard deixa de depender de `events.jsonl`.
- [ ] `cargo build -p mustard-dashboard` (ou o nome da crate `src-tauri`) verde; testes da crate verdes.

### Impl Agent (Wave 2) — depreciar o auto-update npm da CLI

- [ ] `auto_update.rs`: o subcomando `auto-update` deixa de chamar `npm`; imprime que a CLI agora é binário Rust e indica o canal de atualização correto (ex.: `cargo install --git …` / GitHub releases). `--check-only` reporta a depreciação; nunca executa `npm install`.
- [ ] `npm.rs`: remover `get_latest_version`/`update_global`/`PACKAGE_NAME` se ficarem sem uso após a depreciação; senão, marcar `#[deprecated]` com nota.
- [ ] `cli.rs`: manter o subcomando `AutoUpdate` reconhecível (não quebrar quem o digita) mas com o novo comportamento.
- [ ] `cargo build -p mustard-cli` + `cargo test -p mustard-cli` verdes.

## Dependências

- Wave 1 e Wave 2 são **independentes** (crates distintas, sem código compartilhado) — podem rodar em paralelo.
- Wave 1 depende do `mustard.db` já ser o store canônico — garantido pela `eliminate-bun` (concluída).

## Preocupações

- **Paridade de leitores é o risco da Wave 1:** se um dado hoje servido via JSONL não tiver fonte SQLite equivalente, removê-lo cega o dashboard. O ANALYZE precisa enumerar antes de remover — não deletar `*_from_jsonl` sem o equivalente SQLite pronto.
- **`watcher` em modo WAL:** com WAL, as escritas vão primeiro para `mustard.db-wal`; observar só o `mustard.db` pode perder eventos de refresh. Avaliar observar o diretório `.harness/` ou o `-wal`.
- **Pin `rusqlite 0.31`:** se a Wave 1 fizer o dashboard depender de `mustard-core`, o pin alinha sozinho (ambos em 0.31 — ver `## Preocupações` da `eliminate-bun`). Se mantiver o `db.rs` próprio, o pin 0.31 já está lá; não subir isoladamente.
- **Tipos de comando Tauri:** remover leitores não deve mudar a forma dos tipos retornados (`RecentEvent`, `MetricsSummary`, etc.) — senão o frontend React entra no escopo. Preferir paridade de forma.

## Critérios de Aceitação

- [ ] AC-1: O workspace compila — Command: `bash -c 'cargo build --workspace'`
- [ ] AC-2: Testes do workspace verdes — Command: `bash -c 'cargo test --workspace'`
- [ ] AC-3: Nenhum leitor de `events.jsonl` resta no backend do dashboard — Command: `bash -c '! grep -rln "events.jsonl" apps/dashboard/src-tauri/src'`
- [ ] AC-4: O auto-update da CLI não invoca npm — Command: `bash -c '! grep -rln "npm install" apps/cli/src'`

## Não-Objetivos

- Não construir o updater real da CLI (self-update via GitHub releases / `cargo install`) — distribuição é B6+; aqui o `auto-update` só para de fazer a coisa errada.
- Não re-projetar o esquema do `mustard.db` nem os tipos de comando Tauri.
- Não tocar no frontend React do dashboard, salvo se um tipo de retorno mudar de forma (a evitar).
- Não importar histórico — o `events.jsonl` legado foi descartado pela `eliminate-bun`; o dashboard começa a partir do `mustard.db`.
