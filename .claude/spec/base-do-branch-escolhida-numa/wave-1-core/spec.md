---
id: wave.base-do-branch-escolhida-numa.1-core
---

# wave-1-core

## Summary

separa no modelo o que é ponto-de-corte do que é branch protegido

## Network

- Parent: [[spec.base-do-branch-escolhida-numa]]

## Tasks

- [ ] em packages/core/src/domain/config.rs, criar protected_branches(): resolve origin/HEAD via `git symbolic-ref refs/remotes/origin/HEAD` e une com a lista opcional git.protected do mustard.json; fail-open para {main, master} quando o remoto não responde
- [ ] manter integration_bases() vivo mas rebaixado: passa a significar APENAS 'bases pré-selecionadas', nunca 'bases permitidas' — documentar a mudança de sentido no doc do método
- [ ] adicionar o campo opcional git.protected ao ProjectConfig, mantendo o schema camelCase e o dono único do mustard.json
- [ ] escrever o teste flow_preselects_but_never_restricts provando que um git.flow preenchido pré-seleciona mas não recusa nenhum outro branch

## Files

- `packages/core/src/domain/config.rs`
- `packages/core/src/platform/git_branches.rs` (novo — o guard do core proíbe efeito colateral em `domain/`, então a sonda do git mora em `platform/`)
- `packages/core/src/platform/mod.rs`
- `packages/core/src/lib.rs`

## Reality Obligations

- **RO-1.1** — confirmar contra o git real o que `git symbolic-ref refs/remotes/origin/HEAD` devolve quando o clone nunca rodou `git remote set-head` — é o caso comum em clone raso de CI e decide o fail-open
