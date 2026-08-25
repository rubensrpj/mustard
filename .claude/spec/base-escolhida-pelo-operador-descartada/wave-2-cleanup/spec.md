---
id: wave.base-escolhida-pelo-operador-descartada.2-cleanup
---

# wave-2-cleanup

## Summary

Os cinco pontos restantes deixam de consultar a lista de configuração para recusar, e os textos de ajuda deixam de ensinar o modelo apagado.

## Network

- Parent: [[spec.base-escolhida-pelo-operador-descartada]]
- Depends on: [[wave.base-escolhida-pelo-operador-descartada.1-backend]]

## Tasks

- [ ] apps/rt/src/commands/work_unit_open.rs (~627): remover a recusa 'is not an integration base of this project ... Declare it in mustard.json#git.flow'. Um nome no formato `<prefixo>_<resto>` deve ser resolvido pelo catálogo real; a saída oferecida nunca volta a ser 'edite a configuração'. Ver também o fallback primary_base() em ~610.
- [ ] apps/rt/src/commands/review/pr_door.rs (~314): a recusa 'not-on-integration-base' de `pr list` deve testar o conjunto PROTEGIDO (protected_branches) ou a base registrada da unidade — que é a pergunta que o comando realmente faz — e não flow.bases(). E o alvo do PR (~319) deve cair em origin/HEAD (mustard_core::default_branch, hoje sem nenhum chamador fora do próprio módulo) como último recurso, em vez de primary_base(), que termina no literal 'main'.
- [ ] apps/rt/src/commands/git_delete.rs (~101): mesma recusa, mesmo tratamento.
- [ ] apps/rt/src/commands/doctor/doctor.rs (~600-630): o aviso de 'git.flow vazio' dispara hoje em toda instalação correta, porque o instalador deixou de gravar a chave, e sua afirmação sobre o que está protegido é falsa perante protected_branches. Remover ou reescrever para relatar o que está REALMENTE protegido, medido.
- [ ] plugin/refs/git/git-flow.md: a referência que /git manda ler ensina o modelo apagado — bases de integração derivadas do flow (~23), tabela de tipo-decide-base (~33-35), alvo do PR resolvido pelo flow (~62), base lida do prefixo do branch (~104-110) e recusa por estar numa base de integração (~116). Reescrever para o modelo atual. Conferir também plugin/refs/git/submodule-rules.md (~20) e plugin/commands/upsert.md (~38).
- [ ] Comentários de documentação que hoje contradizem o próprio código, e que são o mapa que o próximo leitor usa: apps/rt/src/commands/event/base_gate.rs (~15-21) ainda lista 'Not an integration base -> Refuse' como uma das respostas do portão, num arquivo cujo corpo apagou exatamente isso; apps/rt/src/commands/event/emit_pipeline.rs (~154, ~451, ~476) ainda diz que --base precisa nomear uma base do git.flow; apps/rt/src/commands/event/work_branch.rs (~120) ainda cita BaseFlow::base_of_kind, função que não existe mais.
- [ ] Escrever as catracas: nothing_refuses_for_absence_from_the_preselected_list (nenhum dos três comandos recusa por ausência na lista), doctor_does_not_ask_for_a_flow_that_the_installer_no_longer_writes, e the_git_reference_teaches_the_measured_model em apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs.
- [ ] Se, ao fim, preselected_bases() não tiver mais nenhum chamador que RECUSE, dizer isso no relatório. A função continua legítima como pré-seleção; o que não pode sobreviver é a contradição entre sua documentação e seu uso. E integration_bases() (packages/core/src/domain/config.rs:150) é um encaminhamento marcado como obsoleto sem nenhum chamador de produção — remover, se nada quebrar.

## Files

- `apps/rt/src/commands/work_unit_open.rs`
- `apps/rt/src/commands/review/pr_door.rs`
- `apps/rt/src/commands/git_delete.rs`
- `apps/rt/src/commands/doctor/doctor.rs`
- `plugin/refs/git/git-flow.md`
- `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs`
