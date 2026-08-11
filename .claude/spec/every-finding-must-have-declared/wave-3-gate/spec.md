---
id: wave.every-finding-must-have-declared.3-gate
---

# wave-3-gate

## Summary

A porta que declara o destino e o portao que recusa o fecho enquanto sobrar achado aberto.

## Network

- Parent: [[spec.every-finding-must-have-declared]]
- Depends on: [[wave.every-finding-must-have-declared.2-collect]]

## Tasks

- [ ] Criar a porta `mark-finding --spec <slug> --id <id> --to <criterion|change-request|queued|dropped> --reason "<motivo>"`, espelhando mark-checklist-item --drop --reason. Sem `--reason` ela recusa: um destino sem motivo nao e destino.
- [ ] Em apps/rt/src/commands/pipeline/close_gates.rs, adicionar o sub-gate de achados ENTRE o checklist e o QA: roda o coletor da onda 2 in-process, e recusa quando FindingItem::is_open() vale para qualquer achado.
- [ ] A mensagem de recusa segue format_gate_message e nomeia, por achado: a fonte, o statement, e o comando `mark-finding` exato que o resolve — um portao que recusa sem dizer a acao ensina o leitor a contorna-lo.
- [ ] Adicionar o campo `findings` a CloseGateModes e resolve-lo por MUSTARD_FINDINGS_GATE_MODE com default Strict, pela mesma cascata dos irmaos (resolve_mode).
- [ ] Registrar `mark-finding` nos quatro lugares exigidos pelo crate.
- [ ] Testes nomeados `findings_gate_*` e `mark_finding_*`.

## Files

- `apps/rt/src/commands/spec/mark_finding.rs`
- `apps/rt/src/commands/spec/cli.rs`
- `apps/rt/src/commands/pipeline/close_gates.rs`
- `apps/rt/tests/run_command_surface.rs`
