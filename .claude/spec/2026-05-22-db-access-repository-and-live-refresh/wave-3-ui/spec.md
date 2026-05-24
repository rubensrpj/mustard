# wave-3-ui — Estado compartilhado no dashboard + refresh enxuto

## Resumo

Manter o `store::DbCache` (Wave 1) vivo no estado gerenciado do Tauri e fazer
todos os comandos reusarem a conexão em cache por path, em vez de abrir/fechar a
cada chamada. Apoiar na thread de watcher já existente para o refresh ao vivo,
reduzindo o polling redundante por hook. Depende da Wave 1.

## Causa raiz

`db::with_db` (`apps/dashboard/src-tauri/src/db.rs:41-57`) abre uma `Connection`
nova a cada comando; nada é gerenciado via `.manage()` além do `WatcherState`
(`lib.rs:1675`). Em paralelo, ~15 hooks do front têm `refetchInterval` próprio
(5-12s) rodando por cima do watcher de filesystem (`watcher.rs`), que já invalida
as queries via `dashboard:fs-change`. Os paths de knowledge foram removidos da
classificação do watcher (Wave 6c), então a página Knowledge cai para polling.

## Arquivos

- `apps/dashboard/src-tauri/src/lib.rs` — `.manage(store::DbCache compartilhado)`; comandos recebem `State<DbCache>`
- `apps/dashboard/src-tauri/src/db.rs` — `with_db` pega o store do cache gerenciado (por `repo_path`) em vez de abrir conexão nova
- `apps/dashboard/src-tauri/src/watcher.rs` — reativar classificação dos paths de knowledge (`kind: "knowledge"`)
- `apps/dashboard/src/hooks/*.ts` + `src/pages/{Knowledge,Specs,Home}.tsx`, `src/components/layout/Topbar.tsx` — remover/alongar `refetchInterval` redundantes, deixando o watcher como gatilho primário; Knowledge passa a invalidar por evento

## Tarefas

### UI Agent (Wave 3)

- [ ] `lib.rs`: construir o `DbCache` no `.setup`/builder e registrar via `.manage()`.
- [ ] `db.rs`: `with_db` recebe `State<DbCache>` e pega o store por `repo_path`; remover `Connection::open_with_flags` por chamada. Comandos de escrita (`spec_complete/cancel/reactivate`) reusam o handle.
- [ ] `watcher.rs`: reativar `classify_kind` para os arquivos de knowledge, emitindo `kind: "knowledge"`.
- [ ] Front: registrar invalidação de knowledge no listener do watcher (`src/lib/watcher.ts`); remover os `refetchInterval` que apenas duplicam o watcher (manter um fallback longo onde fizer sentido).
- [ ] Rodar `cargo build -p mustard-dashboard` e `pnpm --filter mustard-dashboard build`.

## Critérios de Aceitação

- [ ] AC-1: `cargo build -p mustard-dashboard` passa — Command: `cargo build -p mustard-dashboard`
- [ ] AC-2: build do front passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-3: `with_db` não abre conexão nova (sem `open_with_flags` solto) — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('apps/dashboard/src-tauri/src/db.rs','utf8');const i=s.indexOf('fn with_db');const b=s.slice(i,i+1200);process.exit(/open_with_flags/.test(b)?1:0)"`
- [ ] AC-4: watcher classifica knowledge — Command: `bash -c "grep -q 'knowledge' apps/dashboard/src-tauri/src/watcher.rs && echo ok"`

## Limites

- `apps/dashboard/src-tauri/src/{lib,db,watcher}.rs`, `apps/dashboard/src/hooks/*.ts`, `apps/dashboard/src/lib/watcher.ts`, páginas citadas
- NÃO alterar a API do store/`DbCache` (Wave 1)
- NÃO remover o watcher nem criar nova thread
