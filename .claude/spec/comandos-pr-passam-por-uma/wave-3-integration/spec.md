---
id: wave.comandos-pr-passam-por-uma.3-integration
---

# wave-3-integration

## Summary

os consumidores existentes e a prosa passam pela porta

## Network

- Parent: [[spec.comandos-pr-passam-por-uma]]
- Depends on: [[wave.comandos-pr-passam-por-uma.2-commands]]

## Tasks

- [ ] trocar na prosa das portas: plugin/commands/pr.md deixa de mandar rodar rtk gh pr create/edit e passa a mandar mustard-rt run pr-open / pr-edit; plugin/commands/git.md troca rtk gh pr ready por mustard-rt run pr-ready — o chamador real que a inscrição da onda 2 exige
- [ ] review_prefetch.rs e pr_door.rs anotam no cabeçalho que o caminho de ESCRITA de PR agora é a porta (as leituras gh view/list deles migram numa unidade própria — mexer nos dois leitores aqui dobraria o raio da unidade)
- [ ] conferir que nenhuma outra prosa do plugin nomeia gh pr create/edit/ready

## Files

- `plugin/commands/pr.md`
- `plugin/commands/git.md`
- `apps/rt/src/commands/review/review_prefetch.rs`
- `apps/rt/src/commands/review/pr_door.rs`
