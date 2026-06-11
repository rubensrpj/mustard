# Plano de Waves

## Tabela de Waves

| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-core]] | core | — | Cache único e incremental de eventos: comandos do dashboard atendem da memória; só o arquivo NDJSON alterado é relido |
| 2 | [[wave-2-tauri]] | tauri | [[wave-1-core]] | Thread de fundo: o watcher reconstrói o snapshot agregado fora da thread principal (partindo do cache incremental) e empurra pronto para o front-end |
| 3 | [[wave-3-frontend]] | frontend | [[wave-2-tauri]] | Front-end consome o push granular e abandona a invalidação em massa das 13 chaves de consulta |

## Critérios de Aceitação
- **AC-1** — Build do core e do dashboard verdes. Command: `cargo build -p mustard-core -p mustard-dashboard`
- **AC-2** — Testes do core verdes, incluindo: segunda leitura da mesma workspace atende do cache sem nova varredura de disco. Command: `cargo test -p mustard-core`
- **AC-3** — Testes do dashboard verdes, incluindo: snapshot reconstruído em thread de fundo, push emitido uma única vez por rajada e invalidação incremental (tocar 1 arquivo NDJSON relê somente esse arquivo). Command: `cargo test -p mustard-dashboard`
- **AC-4** — Front-end compila e passa a checagem de tipos. Command: `npm --prefix apps/dashboard run build`
