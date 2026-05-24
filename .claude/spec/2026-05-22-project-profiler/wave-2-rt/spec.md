# Wave 2 (rt) — Interpretação por modelo (cold path)

### Stage: Execute
### Outcome: Active
### Flags:
### Lang: pt
### Checkpoint: 2026-05-24T09:05:08.646Z
### Parent: 2026-05-22-project-profiler

## PRD

## Contexto

A detecção de entidades hoje vive em oito arquivos Rust, um por linguagem, que reconhecem apenas convenções fixas — o scanner .NET, por exemplo, só acha entidade em pastas `Entities`/`Domain` e ignora `DbSet` e a pasta `Features` que muitos projetos usam. Adicionar uma linguagem nova exige escrever mais um arquivo Rust. Ao mesmo tempo, o `cluster_discovery` já extrai de graça, e de forma agnóstica, a estrutura, a nomenclatura e os clusters do projeto — mas entrega isso como dado cru, sem rótulo (um cluster de sufixo `Resolver` não vira "padrão de resolver GraphQL"), e às vezes desiste de eleger uma convenção dominante. Esta wave adiciona uma camada de interpretação no cold path: um modelo (Sonnet por padrão) lê o perfil compacto já produzido — não o repositório inteiro — mais algumas amostras, rotula os clusters, resolve as convenções que ficaram em aberto, identifica entidades, e escreve as arestas `[[ ]]` que ligam os conceitos. Roda uma vez por projeto, congelado por SHA. Com isso, os oito scanners por linguagem deixam de existir.

## Métrica de sucesso

Os 8 `*_scanner.rs` são removidos por completo (`dart`, `dotnet`, `java`, `go`, `rust`, `python`, `php`, `typescript`); a detecção de entidade deixa de depender de convenção fixa por linguagem e funciona em casos que os scanners hardcoded perdiam (ex.: .NET com `Features/`+`DbSet`, TypeScript com `mysqlTable`/`sqliteTable`, Go sem `gorm`, struct Rust sem derive de ORM); o perfil interpretado traz clusters rotulados e zero `dominant:null` quando há sinal suficiente.

## Não-Objetivos

- Não rodar modelo no hot path nem a cada sync — só 1ª vez ou quando o file-set muda além do threshold.
- Não remover o `cluster_discovery` — ele continua sendo a entrada (o perfil compacto) que o modelo interpreta.

## Critérios de Aceitação

- [x] AC-1: zero scanners por linguagem — todos os 8 `*_scanner.rs` removidos — Command: `node -e "const fs=require('fs');const n=fs.readdirSync('apps/rt/src/run/scan').filter(f=>/_scanner\.rs$/.test(f)).length;process.exit(n===0?0:1)"`
- [x] AC-2: cache congelado — segunda chamada sem mudança de file-set não dispara modelo (flag de cache hit no teste) — Command: `cargo test -p mustard-rt interpret_cache_frozen`
- [x] AC-3: detecção multi-stack — matriz de fixtures que os scanners hardcoded perdiam (.NET `Features/`+`DbSet`, TS `mysqlTable`, Go sem `gorm`, Rust sem derive ORM) detecta a entidade em cada uma — Command: `cargo test -p mustard-rt interpret_multistack_entities`
- [x] AC-4: modelo selecionável por env (default sonnet) — Command: `cargo test -p mustard-rt interpret_model_env_default`

## Plano

## Summary

Novo módulo `scan/interpret.rs` (cold path) que recebe o perfil compacto (`_patterns` + amostras) e devolve nós interpretados (clusters rotulados, convenções resolvidas, entidades, arestas `[[ ]]`). Chamada de modelo via a face existente do binário; resultado cacheado/congelado por SHA junto do `.cluster-cache.json`. Remover os 8 `*_scanner.rs` e o dispatch por linguagem em `mod.rs`; `load_scanner` passa a devolver um interpretador genérico sobre o perfil agnóstico.

## Arquivos

- `apps/rt/src/run/scan/interpret.rs` — novo: monta o prompt a partir do perfil compacto, chama o modelo, parseia a resposta em nós + arestas, cacheia por SHA.
- `apps/rt/src/run/scan/mod.rs` — remover `STACK_SIGNALS` por-linguagem e o `match` de `load_scanner`; manter detect agnóstico + interpretador genérico.
- `apps/rt/src/run/scan/{dart,dotnet,java,go,rust,python,php,typescript}_scanner.rs` — REMOVER.
- `apps/rt/src/run/sync_registry.rs` — `build_registry` consome os nós interpretados para a faceta entidade.
- `apps/rt/CLAUDE.md` — atualizar a seção de scan (não há mais scanner por linguagem).

## Tarefas

### rt Agent (Wave 2)

- [x] Definir o contrato do perfil compacto que entra no modelo (subset do `_patterns` + N amostras representativas por cluster).
- [x] Implementar `interpret.rs`: prompt, chamada de modelo, parse robusto (fail-open: erro → cai no piso agnóstico), cache por SHA do file-set.
- [x] Env `MUSTARD_SCAN_MODEL` (default `sonnet`); honrar a política de no-downgrade.
- [x] Remover os 8 `*_scanner.rs` e o dispatch por linguagem; `load_scanner` → interpretador genérico.
- [x] `build_registry` usa os nós interpretados; manter saída byte-estável.
- [x] Testes: `interpret_cache_frozen`, `interpret_multistack_entities`, `interpret_model_env_default`.
- [x] Atualizar `apps/rt/CLAUDE.md` (seção scan) e checar `docs-stale-check`.

## Limites

- `.claude/spec/2026-05-22-project-profiler/wave-2-rt/`
- `apps/rt/src/run/scan/**`
- `apps/rt/src/run/sync_registry.rs`
- `apps/rt/CLAUDE.md`
- NÃO definir ainda o resolver de injeção (W4) nem o layout do vault (W3) — esta wave só produz os nós e arestas.
