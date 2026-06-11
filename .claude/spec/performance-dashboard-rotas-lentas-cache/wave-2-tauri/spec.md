# wave-2-tauri

## Resumo

Thread de fundo: o watcher reconstrói o snapshot agregado fora da thread principal (partindo do cache incremental) e empurra pronto para o front-end

## Rede

- Pai: [[performance-dashboard-rotas-lentas-cache]]
- Depende de: [[wave-1-core]]

## Tarefas

- [ ] No callback do watcher (watcher.rs:129-164), após a invalidação incremental, reconstruir o snapshot agregado (lista de specs + projeções da spec ativa) dentro de tauri::async_runtime::spawn_blocking — nunca na thread do callback do debouncer; a reconstrução parte do cache incremental (milissegundos), não de um walk completo
- [ ] Emitir dashboard:specs-snapshot com o payload pronto (tipo serde dedicado) — no máximo uma emissão por rajada, aproveitando o debounce de 200 ms e o throttle de 100 ms já existentes
- [ ] Manter dashboard:fs-change para os demais kinds (compatibilidade com as outras páginas)
- [ ] Teste: uma rajada de escritas NDJSON produz uma única reconstrução e uma única emissão de snapshot; tocar 1 arquivo relê somente esse arquivo

## Arquivos

- `apps/dashboard/src-tauri/src/watcher.rs`
- `apps/dashboard/src-tauri/src/lib.rs`
