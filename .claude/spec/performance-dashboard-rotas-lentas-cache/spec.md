# Performance do dashboard: rotas lentas, cache unico de eventos e atualizacao continua em thread de fundo com push

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Performance do dashboard: rotas lentas, cache unico de eventos e atualizacao continua em thread de fundo com push.

Âncoras (do scan — BAIXA CONFIANÇA: casamento fraco, confirme lendo antes de usar):
- apps/rt/src/hooks/write/size_gate.rs
- packages/core/src/domain/ast/wasm_acquire.rs
- apps/dashboard/src-tauri/src/lib.rs
- packages/core/src/io/claude_paths.rs
- apps/dashboard/src/features/specs/SpecDetailDashboard/index.tsx
- apps/dashboard/src/lib/dashboard.ts
- apps/rt/tests/fixtures/regression-w6/w6-post/telemetry.rs
- apps/dashboard/src-tauri/test_cache_mismatch.rs
- apps/rt/src/commands/review/bugfix_cache.rs
- apps/rt/tests/fixtures/regression-w6/w6-pre/telemetry.rs
- apps/dashboard/src-tauri/src/telemetry.rs
- apps/dashboard/src-tauri/src/spec_views.rs

Fatias recorrentes (precedente a espelhar): Action+Card+Children+Events+Quality+Timeline+Waves (×2), Agents+Criteria+Effort+Heatmap+History+Phases+Timeline (×2), Files+Planned (×2)

O dashboard (aplicativo Tauri com front-end React) está lento para abrir qualquer rota, principalmente a rota de specs. O diagnóstico nas âncoras confirmou três causas que se somam:

1. A rota de detalhe de spec dispara 5 consultas em paralelo (card, ondas, qualidade, linha do tempo, markdown) e cada uma chama `read_workspace_events` (`packages/core/src/view/projection/mod.rs:83`), que varre e re-parseia ~10 mil arquivos NDJSON (o log de eventos do harness) do disco a cada chamada — cinco varreduras completas por render.
2. Já existe um cache em memória no lado Tauri (`walk_ndjson_events_cached`, `apps/dashboard/src-tauri/src/telemetry.rs:1637`), mas a projeção do core não passa por ele; o cache só atende parte dos comandos de telemetria.
3. A cada escrita de evento, o watcher de arquivos emite `dashboard:fs-change` e o front-end invalida 13 chaves de consulta de uma vez (`apps/dashboard/src/lib/watcher.ts:14-56`), disparando 13 refetches simultâneos que re-leem o disco inteiro. Essa é a "atualização constante" que o cliente percebe.

O pedido do cliente — mover a atualização contínua para uma thread — tem precedente pronto no código: o watcher já roda em thread própria (`apps/dashboard/src-tauri/src/watcher.rs:123`), comandos pesados já usam `spawn_blocking` (`lib.rs:188`) e o push para o front-end já existe via `app.emit`. A solução compõe esses mecanismos: um cache único de eventos compartilhado entre o core e o Tauri, e uma thread de fundo que reconstrói os snapshots quando o watcher detecta mudança e os empurra prontos para o front-end, no lugar das invalidações em massa.

## Usuários/Stakeholders

Desenvolvedores que acompanham pipelines pelo dashboard (rotas de specs, telemetria e atividade). O ganho é maior em máquinas com histórico grande de eventos (~10 mil arquivos NDJSON neste workspace).

## Métrica de sucesso

- Abrir a rota de detalhe de spec executa no máximo 1 varredura de disco (hoje: 5) — as demais consultas atendem do cache em memória.
- Uma escrita de evento não dispara mais refetch em massa no front-end: a atualização chega por push da thread de fundo (evento Tauri), com no máximo 1 reconstrução de snapshot por rajada (aproveitando o debounce de 200 ms já existente).
- Com o cache quente, o carregamento perceptível das rotas volta a ser sub-segundo.
- A reconstrução depois de um evento é incremental: somente os arquivos NDJSON alterados são relidos — o re-parse completo dos ~10 mil arquivos não acontece em regime normal (NDJSON é append-only; o custo por evento fica na casa de milissegundos). Specs são markdown pequenos e nunca justificam lentidão; o único volume real são os eventos, e mesmo eles têm que ser rápidos.

## Não-Objetivos

- Não muda o formato NDJSON nem a forma de gravação dos eventos (o lado `mustard-rt`/hooks fica intocado).
- Não troca TanStack Query nem o roteador do front-end.
- Não remove o watcher de arquivos — ele continua sendo o gatilho; muda o que ele dispara.
- Não persiste snapshots em disco (SQLite continua fora do dashboard, conforme decisão anterior do projeto).

## Critérios de Aceitação

- **AC-1** — Build do core e do dashboard verdes
  Command: `cargo build -p mustard-core -p mustard-dashboard`
- **AC-2** — Testes do core verdes (inclui teste novo: a segunda leitura da mesma workspace atende do cache, sem nova varredura de disco)
  Command: `cargo test -p mustard-core`
- **AC-3** — Testes do dashboard verdes (inclui o snapshot reconstruído em thread de fundo, o push emitido uma única vez por rajada e a invalidação incremental: tocar 1 arquivo NDJSON relê somente esse arquivo)
  Command: `cargo test -p mustard-dashboard`
- **AC-4** — Front-end compila e passa a checagem de tipos
  Command: `npm --prefix apps/dashboard run build`

<!-- PLAN -->

## Arquivos

- `packages/core/src/view/projection/mod.rs` — `read_workspace_events` ganha um caminho que recebe eventos já carregados (ou uma camada de cache injetável), em vez de varrer o disco a cada chamada
- `apps/dashboard/src-tauri/src/telemetry.rs` — o cache de eventos (`walk_ndjson_events_cached` + invalidação) vira a fonte única de leitura
- `apps/dashboard/src-tauri/src/lib.rs` — os 5 comandos de detalhe de spec e `dashboard_specs` passam pelo cache; snapshot agregado no estado do aplicativo
- `apps/dashboard/src-tauri/src/watcher.rs` — o callback reconstrói o snapshot em `spawn_blocking` (thread de fundo) e emite o push pronto
- `apps/dashboard/src-tauri/src/spec_views.rs` — a lista de specs (`specs_from_fs`) deixa de re-varrer `.claude/spec/` a cada chamada
- `apps/dashboard/src/lib/watcher.ts` — sai a invalidação em massa das 13 chaves; entra o consumo do push granular
- `apps/dashboard/src/lib/dashboard.ts` — binding tipado do novo evento/snapshot
- `apps/dashboard/src/hooks/useSpecActions.ts` — invalidações pontuais alinhadas ao push

## Limites

IN: leitura e cacheamento dos eventos NDJSON no dashboard; thread de fundo para reconstruir snapshots; push para o front-end; invalidações granulares.
OUT: gravação de eventos (`mustard-rt`), formato NDJSON, reintrodução de SQLite, páginas que já atendem do cache de telemetria sem regressão observada.