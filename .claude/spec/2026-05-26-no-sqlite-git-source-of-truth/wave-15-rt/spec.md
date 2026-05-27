# Tactical-fix: apps/rt/tests/ migration after W2 rename

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec tática descoberta durante W3/W4. **W2 renomeou `pipeline_state_for_spec → pipeline_state_from_events` em `apps/rt/src/run/event_projections.rs`, mas os tests em `apps/rt/tests/` ainda referenciam a assinatura antiga.** Resultado: `cargo test -p mustard-rt --no-run` falha com ~11 mismatched types antes mesmo de rodar qualquer caso. O fix é mecânico (renomear callsites + ajustar 1-2 closures de assinatura), mas é fora-de-escopo de W4 (cap apertado).

Goal binário: `cargo test -p mustard-rt --no-run` compila limpo (0 errors). Não exige tests passando — só compilando — porque sub-specs W11+ (delete-store-and-telemetry-modules) vão deletar grandes blocos de testes SQLite-coupled em massa.

**Files (≤5 esperado):** `apps/rt/tests/pipeline_state_projection_test.rs`, possivelmente 1-2 tests adicionais que importavam o nome antigo (`mcp.rs`, `spec_hygiene.rs`, `amend_finalize.rs`).

## Critérios de Aceitação

- [ ] AC-15-1: `cargo test -p mustard-rt --no-run` compila com 0 erros (warnings permitidos). Command: `cargo test -p mustard-rt --no-run`

## Plano

## Arquivos

- `apps/rt/tests/pipeline_state_projection_test.rs`
- (descobertos durante execução — provavelmente `apps/rt/tests/mcp.rs`, `apps/rt/tests/spec_hygiene.rs`, `apps/rt/tests/amend_finalize.rs`)

## Tarefas

1. Rodar `cargo test -p mustard-rt --no-run` e listar os ~11 erros agrupados por arquivo
2. Substituir cada `pipeline_state_for_spec` por `pipeline_state_from_events` (rename mecânico)
3. Ajustar assinatura: a nova função aceita `&[HarnessEvent]` como 1º arg; se algum caller passa `&SqliteEventStore` ou `&dyn EventStore`, fazer `store.replay()` ou equivalente antes
4. Build limpo: `cargo test -p mustard-rt --no-run`

## Dependências

Independente — pode rodar a qualquer momento após W2/W3/W4 (todos commitados). Sem dependência de outras sub-specs.

## Limites

- CAP RÍGIDO: ≤5 arquivos (provavelmente 3-4)
- Não migrar lógica de SQLite para NDJSON nestes tests — apenas atualizar callsite do rename. Tests SQLite-coupled completos ficam para W11+
- Commit message sugerido: `fix(wave-4/rt): W4-tactical — rename pipeline_state_for_spec callsites in apps/rt/tests/`
