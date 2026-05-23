# Tactical Fix: dedupe now_iso entre core e rt

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: touch
### Checkpoint: 2026-05-23T00:00:00Z
### Lang: pt
### Parent: [[2026-05-22-economia-didatica-e-economias-reais]]

## Contexto

Tactical fix derivado de [[2026-05-22-economia-didatica-e-economias-reais]] (wave-2).

A revisão da Wave 2 (rt-explorer) flagou que `apps/rt/src/run/recipe_match.rs:240-261` reimplementa o algoritmo civil-to-date inline para gerar `now_iso()` (~22 LOC), enquanto `packages/core/src/economy/sources/time.rs:20` já tem a mesma função, porém com visibilidade `pub(super)`. Duas cópias do algoritmo são um risco de drift futuro (cada lado pode receber um fix isolado).

Fix: promover `time::now_iso` para `pub` dentro de `mustard_core::economy::sources::time`, reexportar via `mustard_core::economy::sources::time` (já é um módulo público), e substituir a implementação inline em `recipe_match.rs` por um `use`.

## Arquivos

- `packages/core/src/economy/sources/time.rs` — `pub(super) fn now_iso` → `pub fn now_iso`
- `packages/core/src/economy/sources/mod.rs` — confirmar que `time` é `pub` (ou reexpor `pub use time::now_iso`)
- `apps/rt/src/run/recipe_match.rs` — substituir bloco inline `fn now_iso() -> String { ... }` por `use mustard_core::economy::sources::time::now_iso;` e remover os `#[allow(...)]` que vinham com ele

## Critérios de Aceitação

- [x] AC-1: build core verde — Command: `cargo build -p mustard-core`
- [x] AC-2: build rt verde — Command: `cargo build -p mustard-rt`
- [x] AC-3: testes core verdes — Command: `cargo test -p mustard-core --lib`
- [x] AC-4: implementação inline removida — Command: `bash -c "test $(grep -c 'fn now_iso' apps/rt/src/run/recipe_match.rs) -eq 0 && echo ok"`
- [x] AC-5: import do helper presente — Command: `bash -c "grep -q 'sources::time::now_iso' apps/rt/src/run/recipe_match.rs && echo ok"`

## Limites

- Mudança aditiva de visibilidade (`pub(super)` → `pub`) — não altera contrato; só amplia
- Não tocar a lógica interna do `now_iso` em nenhum dos dois lados
- Não tocar testes existentes (o teste `now_iso_is_iso8601_shape` em `time.rs` continua válido)
