# wave-1-core

## Resumo

Cache único e incremental de eventos: comandos do dashboard atendem da memória; só o arquivo NDJSON alterado é relido

## Rede

- Pai: [[performance-dashboard-rotas-lentas-cache]]

## Tarefas

- [ ] Fazer os 5 comandos de detalhe de spec (dashboard_spec_card / _waves / _quality / _timeline / _markdown) e dashboard_specs lerem eventos via walk_ndjson_events_cached (telemetry.rs:1637) em vez de cada um disparar read_workspace_events com varredura própria de disco
- [ ] Ajustar a projeção do core (packages/core/src/view/projection/mod.rs:83) para aceitar eventos já carregados (slice/iterator) — a varredura de disco vira responsabilidade do chamador cacheado; não criar wrapper duplicado, mudar os call-sites
- [ ] Tornar o cache de eventos incremental por arquivo (chave: caminho + mtime/tamanho): a invalidação disparada pelo watcher marca somente os arquivos alterados e a reconstrução relê apenas esses — nunca re-parsear os ~10 mil NDJSON em regime; o custo por evento deve ficar na casa de milissegundos
- [ ] Cachear a lista de specs (specs_from_fs em spec_views.rs) com invalidação disparada pelo watcher (kind spec) — spec.md é markdown pequeno, a rota de lista nunca deve esperar disco
- [ ] Mover comandos pesados que ainda rodam síncronos para tauri::async_runtime::spawn_blocking, espelhando o precedente de dashboard_metrics (lib.rs:188)
- [ ] Teste: com cache quente, a segunda chamada de um comando de spec não relê o disco; e após tocar 1 arquivo NDJSON, apenas o arquivo alterado é relido (asserção via contador de aberturas)

## Arquivos

- `packages/core/src/view/projection/mod.rs`
- `apps/dashboard/src-tauri/src/telemetry.rs`
- `apps/dashboard/src-tauri/src/lib.rs`
- `apps/dashboard/src-tauri/src/spec_views.rs`
