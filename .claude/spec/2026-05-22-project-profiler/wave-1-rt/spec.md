# Wave 1 (rt) — Passada única e paralela

## PRD

## Contexto

O motor de scan relê cada arquivo de um subprojeto cerca de seis vezes — uma varredura completa por faceta (entidades, enums, rotas, DTOs, serviços) mais a descoberta de clusters — e ainda reconstrói o ignore-set e relê o `.gitignore` a cada varredura, tudo em uma única thread. Para um subprojeto com mil arquivos isso são milhares de leituras redundantes, e é a maior causa da lentidão que o usuário sente. Esta wave colapsa todas as facetas em uma única passada que lê cada arquivo uma vez, reaproveita o ignore-set, e paraleliza a leitura por arquivo. O comportamento observável (o conteúdo do registry) não muda — só a velocidade. Isso entrega valor sozinho, antes de qualquer modelo entrar na história.

## Métrica de sucesso

O scan de um subprojeto faz 1 passada em vez de ~6, e o `entity-registry.json` resultante é idêntico ao baseline atual no mesmo repo.

## Critérios de Aceitação

- [x] AC-1: paridade do registry — a passada única produz o mesmo registry que o motor antigo num fixture — Command: `cargo test -p mustard-rt single_pass_parity`
- [x] AC-2: cada arquivo é lido uma única vez por scan (contador de leituras instrumentado no teste) — Command: `cargo test -p mustard-rt single_pass_reads_once`
- [x] AC-3: workspace compila e clippy limpo — Command: `cargo clippy -p mustard-rt -- -D warnings`

## Plano

## Summary

Introduzir um `FileVisitor`/passada única que, ao ler cada arquivo uma vez, alimenta todos os extratores de faceta + o `cluster_discovery`. Coletar em paralelo (rayon) e reduzir em estrutura ordenada para preservar determinismo. Mover a construção do ignore-set/`.gitignore` para fora do loop de faceta.

## Arquivos

- `apps/rt/src/run/scan/mod.rs` — trait `Scanner::scan()`: trocar as 5 chamadas separadas por uma passada única que distribui o conteúdo lido para os extratores.
- `apps/rt/src/run/scan/file_utils.rs` — `collect_files` + novo `walk_once`/`visit` que retorna (path, conteúdo) uma vez; ignore-set computado uma vez por root.
- `apps/rt/src/run/scan/cluster_discovery.rs` — consumir o conteúdo já lido pela passada em vez de reabrir.
- `apps/rt/src/run/sync_registry.rs` — `enrich_descriptions` reusa o conteúdo da passada (não reabre arquivos de entidade).
- `apps/rt/Cargo.toml` — dependência `rayon`.

## Tarefas

### rt Agent (Wave 1)

- [x] Adicionar `rayon` ao `mustard-rt`; computar ignore-set uma vez por root e passar por referência.
- [x] Implementar passada única `visit(root) -> Vec<(RelPath, String)>` que lê cada arquivo uma vez, paralelizada com rayon, resultado ordenado por path.
- [x] Refatorar `Scanner::scan()` para receber o vetor da passada e rodar os extratores de faceta sobre o conteúdo em memória (sem novo `collect_files`).
- [x] Adaptar `cluster_discovery` e `enrich_descriptions` para consumir o conteúdo já lido.
- [x] Teste de paridade (`single_pass_parity`): registry idêntico (byte-estável) ao baseline num fixture multi-stack.
- [x] Teste de contagem de leitura (`single_pass_reads_once`): instrumentar `mustard-core::fs` no teste e afirmar 1 leitura por arquivo.
- [x] `cargo build --workspace` + `cargo clippy -p mustard-rt -- -D warnings` + `cargo test -p mustard-rt`.

## Limites

- `.claude/spec/2026-05-22-project-profiler/wave-1-rt/`
- `apps/rt/src/run/scan/**`
- `apps/rt/src/run/sync_registry.rs`
- `apps/rt/Cargo.toml`
- NÃO tocar nos `*_scanner.rs` individuais quanto à lógica de detecção (isso é W2) — só adaptar a assinatura para receber conteúdo já lido.
- NÃO introduzir modelo/LLM nesta wave.
