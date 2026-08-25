---
id: wave.comandos-pr-passam-por-uma.2-commands
---

# wave-2-commands

## Summary

pr-open, pr-edit e pr-ready como comandos run, atrás da porta

## Network

- Parent: [[spec.comandos-pr-passam-por-uma]]
- Depends on: [[wave.comandos-pr-passam-por-uma.1-backend]]

## Tasks

- [ ] criar apps/rt/src/commands/review/pr_publish.rs com os três comandos: pr-open (--base, --head, --body-file, --draft), pr-edit (--number, --body-file), pr-ready (--number); cada um resolve o adaptador via provider_for e devolve um relatório JSON com ok/url/number/provider e o erro degradado em campo, nunca exit de pânico
- [ ] as QUATRO inscrições que todo comando run novo exige: variante no enum de review/cli.rs, braço de despacho, nome na lista travada de apps/rt/tests/run_command_surface.rs, e o chamador real na prosa (entra na onda 3)
- [ ] testes com um PrProvider fake em tabela, como FakePr de branch_state.rs — nenhum teste toca rede

## Files

- `apps/rt/src/commands/review/pr_publish.rs`
- `apps/rt/src/commands/review/cli.rs`
- `apps/rt/src/commands/review/mod.rs`
- `apps/rt/tests/run_command_surface.rs`
