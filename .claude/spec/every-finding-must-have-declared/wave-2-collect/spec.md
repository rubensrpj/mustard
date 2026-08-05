---
id: wave.every-finding-must-have-declared.2-collect
---

# wave-2-collect

## Summary

O coletor deterministico le as duas fontes que hoje ninguem le e semeia os achados no sidecar.

## Network

- Parent: [[spec.every-finding-must-have-declared]]
- Depends on: [[wave.every-finding-must-have-declared.1-core]]

## Tasks

- [ ] Criar apps/rt/src/commands/review/finding_collect.rs: le `<spec>/review/findings*.md` (um achado por arquivo, id derivado do nome do arquivo) e `<spec>/ac-proof.json` (um achado por criterio cuja coluna `removal` seja `survived` ou `evidence-removed`, carregando o `reason` que o ledger ja escreveu por extenso).
- [ ] Reusar ac_negative_check::load_ledger — e o UNICO leitor de ac-proof.json no crate, e um segundo leitor e como as duas leituras divergem.
- [ ] Semear meta.json#findings de forma IDEMPOTENTE: um achado cujo destino ja foi declarado sobrevive a uma nova coleta; um achado que sumiu da fonte e removido; um novo entra aberto.
- [ ] Registrar o subcomando `finding-collect` nos QUATRO lugares que o crate exige: variante no enum ReviewCmd e braco no dispatch() de commands/review/cli.rs, a lista trancada em tests/run_command_surface.rs, e um chamador real (a onda 3 e esse chamador).
- [ ] Testes nomeados `finding_collect_*`: coleta das duas fontes; idempotencia do destino ja declarado; spec sem nenhuma das fontes coleta zero e nao escreve chave.

## Files

- `apps/rt/src/commands/review/finding_collect.rs`
- `apps/rt/src/commands/review/mod.rs`
- `apps/rt/src/commands/review/cli.rs`
- `apps/rt/tests/run_command_surface.rs`
