@.claude/scan-map.md

# Server

> Parent: [../../../CLAUDE.md](../../../CLAUDE.md) | Orchestrator: [../../../.claude/mustard/orchestrator.md](../../../.claude/mustard/orchestrator.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=cargo; frameworks=tiny_http, serde, serde_json, notify, notify-debouncer-mini, chrono, dirs -->
- Todo comando novo precisa de UMA linha na tabela `COMMANDS` de `server.rs`; esquecer compila mas o `POST /api/{nome}` responde 404. A tabela é a fonte única — `GET /api/commands` a lista, e é assim que uma sonda distingue "comando ausente" de "comando quebrado".
- As chaves de argumento na tabela são camelCase (`repoPath`, `specName`) — é o contrato de serialização que o frontend já mandava pro `invoke()`. O extrator aceita a grafia snake_case também, mas a camelCase é a canônica: não renomeie.
- Toda struct de retorno usa `#[serde(rename_all = "snake_case")]`: as chaves espelham os tipos TypeScript do dashboard — renomear/trocar casing aqui desalinha o binding silenciosamente.
- Comandos são tolerantes a falha: nunca propague erro de IO ausente pra um toast — devolva vazio/zerado. Onde a versão de aplicativo de mesa degradava um `spawn_blocking(..).await` em pânico, use `crate::catch_panic`; o dispatch também absorve pânico, mas o fallback específico mora no comando.
- Comandos são funções síncronas comuns: cada requisição já roda na sua thread de worker do `tiny_http` e não há thread de UI pra desocupar — não reintroduza `async` nem runtime.
- O watcher (`watcher.rs`) empurra pelo `EventBus`, nunca direto na conexão: o estrangulamento (`last_emit` + `EMIT_THROTTLE`) e o rebuild fora da thread do debouncer são o contrato, e a rota `GET /api/events` só drena o canal.
- Reaproveite dados via `mustard-core`/`mustard-cli` nativamente: leia o modelo com `read_projects`/`read_entity_names` em vez de parsear `grain.model.json`; a fonte de pipeline é o NDJSON por spec + walk de `spec.md`, não há SQLite compartilhado.
- Esta crate é membro normal do workspace: `cargo test -p mustard-dashboard` e `cargo build --workspace` a alcançam, e as dependências vêm de `[workspace.dependencies]`. Não recrie um `Cargo.lock` próprio.
<!-- /mustard:guards -->
