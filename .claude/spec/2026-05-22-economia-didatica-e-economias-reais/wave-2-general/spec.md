# wave-2-general — Emissores de economia que faltam (RTK contínuo + injeção)

### Parent: [[2026-05-22-economia-didatica-e-economias-reais]]
### Stage: Execute
### Outcome: Active
### Flags:
### Lang: pt
### Checkpoint: 2026-05-22T17:35:00Z

## Resumo

Escrever os dois emissores de economia que hoje deixam a tela zerada: a **injeção
de recipe** (nunca codada) e o **RTK contínuo** (só ingere via comando manual).
Depende da Wave 1 (helper `injection_savings_tokens`).

## Causa raiz

`savings_records` só recebe `ModelRoutingDowngrade` (funciona). `RecipeInjection`
não tem nenhum call site de `record_savings`. RTK (`SavingsSource::RtkRewrite`)
só é gravado via `apps/rt/src/run/rtk_gain.rs` quando `mustard-rt run rtk-gain` é
chamado manualmente — nunca contínuo; e o caminho inline em `bash_guard.rs:1588`
fica atrás de um veredito `Rewrite` que não ocorre no modo strict (padrão).

## Arquivos

- `apps/rt/src/run/recipe_match.rs` — quando um recipe não-vazio é casado/injetado, emitir `economy::writer::record_savings(RecipeInjection, injection_savings_tokens(skeleton))` (helper da Wave 1)
- `apps/rt/src/hooks/session_cleanup.rs` — no `SessionEnd`, ingerir `rtk gain --json` (reusar `economy::sources::rtk::ingest` / o caminho de `run/rtk_gain.rs`) gravando `RtkRewrite` em `savings_records`; fail-open (`let _ = ...`)
- `packages/core/src/economy/writer.rs` — se necessário, expor a assinatura usada por `record_savings` para os dois call sites (já existe; só consumir)

## Tarefas

### General Agent (Wave 2)

- [ ] `recipe_match.rs`: ao retornar recipe não-vazio, calcular `injection_savings_tokens(skeleton)` (Wave 1) e `record_savings(RecipeInjection, ...)`. Idempotência/dedup razoável (não somar o mesmo match repetido na mesma invocação). Fail-open.
- [ ] `session_cleanup.rs`: no SessionEnd, rodar a ingestão do `rtk gain --json` (mesma lógica de `run/rtk_gain.rs`) para gravar a economia real do RTK em `savings_records`. Fail-open; nunca abortar o cleanup.
- [ ] Confirmar que `savings_breakdown` (reader) passa a retornar RTK e RecipeInjection não-zero quando houve atividade.
- [ ] `cargo build -p mustard-rt` + `cargo test -p mustard-rt` (+ `-p mustard-core` se tocar writer).

## Critérios de Aceitação

- [ ] AC-1: `cargo build -p mustard-rt` passa — Command: `cargo build -p mustard-rt`
- [ ] AC-2: `cargo test -p mustard-rt` passa — Command: `cargo test -p mustard-rt`
- [ ] AC-3: emissor de injeção existe — Command: `bash -c "grep -rq 'RecipeInjection' apps/rt/src/run/recipe_match.rs && echo ok"`
- [ ] AC-4: RTK ingerido no cleanup — Command: `bash -c "grep -riq 'rtk' apps/rt/src/hooks/session_cleanup.rs && echo ok"`

## Limites

- `apps/rt/src/run/recipe_match.rs`, `apps/rt/src/hooks/session_cleanup.rs`
- Telemetria/savings nunca load-bearing: fail-open, sem panic, sem abortar hooks
- NÃO alterar o cálculo da Wave 1 (consumir `injection_savings_tokens`)
- NÃO tocar dashboard (Wave 3)
