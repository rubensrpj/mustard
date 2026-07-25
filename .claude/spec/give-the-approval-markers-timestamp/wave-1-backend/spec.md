---
id: wave.give-the-approval-markers-timestamp.1-backend
---

# wave-1-backend

## Summary

Um escritor so para o corpo do marcador, agora com data

## Network

- Parent: [[spec.give-the-approval-markers-timestamp]]

## Tasks

- [ ] Em shared/context.rs, ao lado de approval_marker_path e clarified_marker_path (que o proprio comentario chama de casa unica do caminho): criar marker_body(spec, via, session, ts) -> String e read_marker_provenance(path) -> Option<MarkerProvenance>. O formato continua chave=valor por linha, legivel em cat; ganha um campo de instante ISO-8601. read_marker_provenance NUNCA falha duro: corpo ilegivel, truncado ou com chave desconhecida devolve None ou campos vazios, jamais Err.
- [ ] Os tres escritores passam a chamar marker_body em vez de montar o texto: plan_approval_observer.rs:83 (via ExitPlanMode), approval_marker_observer.rs:274 (via AskUserQuestion) e grill_capture.rs:220 (via grill-finalize). O terceiro hoje NAO grava session= e passa a gravar, pela mesma funcao.
- [ ] Testes: marker_body_is_the_single_writer prova que as tres portas produzem o mesmo formato com os quatro campos; read_marker_provenance_round_trips_and_degrades prova o ida-e-volta E o caminho degradado (corpo vazio, sem separador, chave desconhecida).

## Files

- `apps/rt/src/shared/context.rs`
- `apps/rt/src/hooks/observe/plan_approval_observer.rs`
- `apps/rt/src/hooks/observe/approval_marker_observer.rs`
- `apps/rt/src/commands/grill_capture.rs`
