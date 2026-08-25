---
id: wave.base-do-branch-escolhida-numa.3-installer
---

# wave-3-installer

## Summary

o init para de perguntar qual é a produção e qual é o desenvolvimento

## Network

- Parent: [[spec.base-do-branch-escolhida-numa]]
- Depends on: [[wave.base-do-branch-escolhida-numa.1-core]]

## Tasks

- [ ] git_flow.rs: remover as perguntas de branch de produção e de branch de desenvolvimento; a sondagem de origin/HEAD e de branches remotos PERMANECE, porque agora serve à proteção e ao seletor
- [ ] init.rs: parar de gravar git.flow no mustard.json — um projeto novo nasce sem a chave, e o resto do fluxo já sabe viver sem ela
- [ ] escrever o teste init_does_not_ask_for_branches provando as duas metades: nenhuma pergunta de branch e nenhuma chave git.flow no arquivo gravado

## Files

- `apps/cli/src/commands/git_flow.rs`
- `apps/cli/src/commands/init.rs`
