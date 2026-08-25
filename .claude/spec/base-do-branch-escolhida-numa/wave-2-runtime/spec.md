---
id: wave.base-do-branch-escolhida-numa.2-runtime
---

# wave-2-runtime

## Summary

os portões param de recusar branch real, o tipo abre e nasce o comando que lista as candidatas

## Network

- Parent: [[spec.base-do-branch-escolhida-numa]]
- Depends on: [[wave.base-do-branch-escolhida-numa.1-core]]

## Tasks

- [ ] base_gate.rs: remover a recusa 'não é uma base de integração'; manter intacta a recusa por base atrasada em relação ao remoto
- [ ] work_branch.rs: is_protected() passa a consultar protected_branches() em vez de integration_bases()
- [ ] work_kind.rs: WorkKind deixa de ser enum fechado — vira rótulo validado por sanitize_git_ref com lista de sugestões sobrescrevível; base_of_kind deixa de existir porque a base agora é escolhida, não derivada do tipo
- [ ] novo comando run base-candidates: faz git fetch, lista os branches do remoto ordenados por recência do último commit, marca qual é o protegido e qual está pré-selecionado; saída JSON determinística
- [ ] registrar o comando nos QUATRO lugares que o guard do rt exige: variante no enum da família, braço no dispatch(), lista em tests/run_command_surface.rs e um chamador real na prosa
- [ ] hooks/bash/safety.rs: o guard de comando destrutivo passa a ler protected_branches()
- [ ] review/pr_door.rs: o alvo do PR passa a ser a base registrada da unidade, com origin/HEAD como último recurso
- [ ] work_unit_open.rs e doctor/doctor.rs acompanham a nova origem da lista

## Files

- `apps/rt/src/commands/event/base_gate.rs`
- `apps/rt/src/commands/event/work_branch.rs`
- `apps/rt/src/shared/work_kind.rs`
- `apps/rt/src/commands/event/base_candidates.rs`
- `apps/rt/src/commands/event/cli.rs`
- `apps/rt/src/hooks/bash/safety.rs`
- `apps/rt/src/commands/review/pr_door.rs`
- `apps/rt/src/commands/work_unit_open.rs`
- `apps/rt/src/commands/doctor/doctor.rs`
- `apps/rt/tests/run_command_surface.rs`
