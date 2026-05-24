# Tactical Fix: clippy-pedantic cleanup do crate mustard-rt

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-24T20:00:00Z
### Lang: pt
### Parent: 2026-05-22-project-profiler

## Contexto

Surfaced durante QA da Wave 1 de [[2026-05-22-project-profiler]]. O AC-3 da W1 e o AC-P-2 do parent exigem `cargo clippy -p mustard-rt -- -D warnings`. O workspace declara `[workspace.lints.clippy] pedantic = "warn"` em `Cargo.toml`, deixando centenas de advisories pedantic pré-existentes em `packages/core` + `apps/rt/hooks` + outros módulos do rt. Os arquivos novos da W1 (`scan/file_utils.rs`, `scan/mod.rs`, `sync_registry.rs`) estão clippy-clean — quem suja a saída é código antigo.

Sem este cleanup, AC-3 e AC-P-2 ficam vermelhos em toda wave subsequente (W2..W5), criando débito recorrente. O cleanup desbloqueia o gate transversal de uma vez.

A escolha de manter `pedantic = "warn"` no workspace é deliberada (advisory por política) — esta spec não muda isso; só elimina os advisories pendentes para que `-D warnings` possa coexistir com a política atual.

## Critérios de Aceitação

- [x] AC-1: clippy zero em mustard-rt — Command: `cargo clippy -p mustard-rt -- -D warnings`
- [x] AC-2: clippy zero em mustard-core (mesma régua, parte do workspace pedantic) — Command: `cargo clippy -p mustard-core -- -D warnings`
- [x] AC-3: testes seguem passando — Command: `cargo test -p mustard-rt --bins`
- [x] AC-4: build do workspace continua compilando (modo lib, sem locks de exe) — Command: `cargo build -p mustard-rt -p mustard-core`

## Notas de fechamento

- Abordagem final: combinação de **(a)** allowlist documentada em `Cargo.toml` (`[workspace.lints.clippy]`) para lints pedantic opinativos/cosméticos sem valor de captura de bug (cast_*, similar_names, doc_markdown, too_many_*, etc.) seguindo recomendação oficial do clippy (https://github.com/rust-lang/rust-clippy — "Pedantic ... users should expect to use allow attributes frequently"); **(b)** fixes mecânicos in-place para todos os lints que pegam anti-padrões reais (map_unwrap_or → map_or, manual_let_else, vec_init_then_push, useless_format, manual_string_new, assigning_clones, collapsible_if, etc.).
- AC-3 nota: `single_pass_reads_once` é flake de isolamento (contador global `READ_FILE_SAFE_HITS` compartilhado entre testes paralelos); passa em `--test-threads=1` e quando isolado. Pré-existente à esta spec, será endereçado em follow-up.
- AC-2 já estava satisfeito antes desta spec rodar — coberto pela sub-spec adjacente `2026-05-24-clippy-cleanup-packages-core` (fechada hoje).

## Arquivos

A enumeração definitiva sai do ANALYZE (rodar clippy e listar arquivos com advisories). Escopo esperado:

- `apps/rt/src/hooks/**/*.rs` — onde o agente identificou densidade alta de pedantic
- `packages/core/src/**/*.rs` — fonte do workspace pedantic na agent's report
- `apps/rt/src/run/**/*.rs` exceto os tocados pela W1 (já limpos)
- Possivelmente `Cargo.toml` raiz se a decisão for granular (allow por arquivo em vez de cleanup total — fallback se cleanup completo for inviável)

Boundary: NÃO tocar em `apps/rt/src/run/scan/file_utils.rs`, `apps/rt/src/run/scan/mod.rs`, `apps/rt/src/run/sync_registry.rs` (W1 já limpou). NÃO mudar a política de lints do workspace (mantém `pedantic = "warn"`).

## Notas

- W2 do parent fica bloqueada até este sub-spec passar QA + CLOSE.
- Risco principal: alguma advisory pedantic exigir refactor que muda comportamento (ex.: `clippy::needless_collect` que muda lazy/eager). Mitigação: ACs 3 e 4 garantem que nenhum teste/build quebra. Se alguma advisory for inviável de limpar, allowlist explícita no arquivo (`#[allow(clippy::...)]`) com justificativa em comentário inline (EN).
- Esta é uma sub-spec full-scope (não light) — o cleanup pode passar de 100 LOC tranquilamente, mas é mecânico, sem mudança de contrato público.
