---
id: wave.give-the-approval-markers-timestamp.2-report
---

# wave-2-report

## Summary

Os leitores passam a mostrar a proveniencia que ja era gravada

## Network

- Parent: [[spec.give-the-approval-markers-timestamp]]
- Depends on: [[wave.give-the-approval-markers-timestamp.1-backend]]

## Tasks

- [ ] approve-spec ecoa a porta e a data ao aprovar, lendo por read_marker_provenance. A existencia do arquivo continua governando o portao: corpo ilegivel degrada para sem-proveniencia e a aprovacao SEGUE. Nunca um novo modo de falha.
- [ ] O comando de status (commands/pipeline/status.rs) mostra porta e data na linha da spec aprovada.
- [ ] ATENCAO ao Guard do rt: a saida de comando run deve ser deterministica e byte-estavel, sem timestamps volateis — ha snapshots insta e gates que comparam a saida. Um instante ecoado no stdout PODE quebrar isso. Verificar os snapshots antes; se quebrar, o instante fica fora do stdout do run e aparece so na face humana, ou o snapshot e atualizado deliberadamente no mesmo commit.
- [ ] Testes: approve_spec_echoes_provenance, unreadable_marker_body_still_approves (o caminho que prova que o portao nao endureceu) e status_shows_approval_provenance.

## Files

- `apps/rt/src/commands/spec/approve_spec.rs`
- `apps/rt/src/commands/pipeline/status.rs`
