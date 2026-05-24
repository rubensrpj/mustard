# wave-1-library

## Resumo

Hard gate no consumidor do `plan.json`. Quando o arquivo declara `waves: []` ou `total_waves != waves.len()`, o scaffolder hoje cria silenciosamente `wave-plan.md` + `review/spec.md` + `qa/spec.md` sem nenhum `wave-N-role/spec.md`, e o caller não tem como saber. Esta wave transforma esse silêncio em erro visível (stderr) e JSON com campo `error`, e impede a criação dos artefatos órfãos quando `waves` está vazio.

## Network

- Parent: [[2026-05-21-wave-integrity-and-doctor-check]]
- Depende de: —

## Arquivos

```
apps/rt/src/run/wave_scaffold.rs   — modify: validation gate antes do emit; novos testes
```

## Tarefas

- [ ] Adicionar validação no início de `run()` (após parse do `Plan`): se `plan.waves.is_empty()`, escrever em stderr `[wave-scaffold] plan.waves is empty — refusing to scaffold` e retornar JSON `{"created_files":[],"skipped":[],"error":"empty waves"}`. Não criar nenhum arquivo. Exit 0 (fail-open).
- [ ] Adicionar warning na sequência: se `plan.total_waves.is_some()` e `Some(plan.waves.len() as u32) != plan.total_waves`, escrever em stderr `[wave-scaffold] WARN: total_waves={declared} differs from waves.len()={actual}`. Seguir o fluxo normal de criação.
- [ ] Adicionar `#[test] fn empty_waves_returns_error_and_creates_nothing()`: escreve plan com `waves: []`, roda `run()`, valida que `spec_dir.join("wave-plan.md")` NÃO existe.
- [ ] Adicionar `#[test] fn total_waves_mismatch_warns_but_continues()`: escreve plan com `waves: [{n:1,...}]` e `total_waves: 3`, roda `run()`, valida que `wave-1-general/spec.md` foi criado (não bloqueou).
- [ ] `cargo build -p mustard-rt && cargo test -p mustard-rt -- wave_scaffold`

## Acceptance Criteria

- [ ] AC-1: `cargo test -p mustard-rt -- wave_scaffold::tests::empty_waves_returns_error_and_creates_nothing` passa — Command: `cargo test -p mustard-rt -- wave_scaffold::tests::empty_waves_returns_error_and_creates_nothing`
- [ ] AC-2: `cargo test -p mustard-rt -- wave_scaffold::tests::total_waves_mismatch_warns_but_continues` passa — Command: `cargo test -p mustard-rt -- wave_scaffold::tests::total_waves_mismatch_warns_but_continues`

## Limites

- `apps/rt/src/run/wave_scaffold.rs` (única modificação desta wave)

Out-of-boundary: qualquer outro arquivo. Esta wave entrega 1 fix cirúrgico ~25 LOC.
