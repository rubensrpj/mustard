# wave-1-library — Reader por-sessão + métrica de injeção

### Parent: [[2026-05-22-economia-didatica-e-economias-reais]]
### Stage: Done
### Outcome: Active
### Flags:
### Lang: pt
### Checkpoint: 2026-05-22T19:00:00Z

## Resumo

Enriquecer o reader de economia em `packages/core` para alimentar o card
por-sessão (custo medido + data/hora + spec(s) trabalhadas) e expor a métrica da
economia de injeção. A Wave 1 entrega os dados; Wave 3 desenha.

## Causa raiz

Hoje `EconomySummary.by_session` (adicionado na spec anterior) traz só
`session_id` + `usd`. Falta a data/hora e as specs daquela sessão. E não há
nenhum cálculo de economia de injeção (a fonte `RecipeInjection` existe no model
mas ninguém escreve).

## Arquivos

- `packages/core/src/economy/model.rs` — enriquecer `SessionCost`: + `last_at_ms: Option<i64>` (data/hora) + `specs: Vec<String>` (specs da sessão)
- `packages/core/src/economy/reader.rs` — popular `last_at_ms` (do `usage_totals.updated_at` por sessão) e `specs` (de `run_usage`: `SELECT DISTINCT spec FROM run_usage WHERE session_id=?1 AND spec IS NOT NULL`), apenas no escopo não-filtrado (projeto/all)
- `packages/core/src/telemetry/reader.rs` — métodos aditivos `specs_for_session(conn, session_id) -> Vec<String>` e `session_last_at(conn, session_id) -> Option<i64>`
- `packages/core/src/economy/writer.rs` — helper `injection_savings_tokens(skeleton_text) -> i64`: tokens do esqueleto (≈ chars/4) menos o input do próprio esqueleto, mínimo 0 (proxy "geração evitada"); reutilizado pelo emissor da Wave 2

## Tarefas

### Library Agent (Wave 1)

- [x] `model.rs`: adicionar `last_at_ms` e `specs` ao `SessionCost` (aditivo; não quebrar a serialização existente).
- [x] `reader.rs`: no ramo não-filtrado, para cada sessão do `cost_by_session`, preencher `last_at_ms` e `specs` via os novos métodos do `telemetry::reader` (mesma conexão do telemetry.db; sem cruzar mustard.db).
- [x] `telemetry/reader.rs`: métodos aditivos para specs-por-sessão e last_at-por-sessão (leem `run_usage`/`usage_totals`).
- [x] `economy/writer.rs`: `injection_savings_tokens` (proxy; puro, testável). Não emite nada ainda — só o cálculo (Wave 2 chama no emissor).
- [x] Testes: by_session traz specs+last_at no escopo projeto; `injection_savings_tokens` calcula o proxy esperado. `cargo test -p mustard-core`.

## Critérios de Aceitação

- [x] AC-1: `cargo build -p mustard-core` passa — Command: `cargo build -p mustard-core`
- [x] AC-2: `cargo test -p mustard-core` passa — Command: `cargo test -p mustard-core`
- [x] AC-3: by_session enriquecido — Command: `bash -c "grep -q 'specs' packages/core/src/economy/model.rs && grep -q 'last_at' packages/core/src/economy/model.rs && echo ok"`

## Limites

- `packages/core/src/economy/{model,reader,writer}.rs`, `packages/core/src/telemetry/reader.rs`
- Aditivo: não alterar assinaturas existentes nem a forma de retorno de `economy_summary` (só somar campos)
- Sem cruzar `mustard.db` na leitura de telemetria (tudo no `telemetry.db`)
- NÃO emitir savings aqui (Wave 2); NÃO tocar dashboard (Wave 3)
