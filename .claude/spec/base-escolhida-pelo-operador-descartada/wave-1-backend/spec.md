---
id: wave.base-escolhida-pelo-operador-descartada.1-backend
---

# wave-1-backend

## Summary

A escolha de base gravada passa a ser conferida por EXISTÊNCIA no remoto, não por pertencimento à lista de configuração — nos dois pontos de leitura.

## Network

- Parent: [[spec.base-escolhida-pelo-operador-descartada]]

## Tasks

- [ ] Em apps/rt/src/commands/event/work_branch.rs, na função recorded_or_derived_base (linha ~359): trocar o filtro `declared.contains(b)`, que testa pertencimento a `preselected_bases()`, por um teste de EXISTÊNCIA do branch no remoto. Use o catálogo que já existe (mustard_core::branch_catalog / a leitura de refs de origin em packages/core/src/platform/git_branches.rs) — não escreva um segundo leitor de branches. Atenção ao custo: esta função roda no caminho do corte e também dentro de um hook; a medição não pode disparar um `git fetch` de rede a cada chamada. Prefira a leitura local de refs (sem fetch) e, se o catálogo só souber medir com fetch, acrescente uma variante sem fetch em vez de forçar rede aqui.
- [ ] Degrade fail-open na direção certa: quando a existência NÃO puder ser medida (não é repositório, sem remoto, git indisponível), OBEDEÇA a escolha gravada em vez de descartá-la. Descartar por não ter conseguido medir repete exatamente o defeito desta unidade — recusar uma escolha real por causa de uma fonte que não respondeu.
- [ ] Em apps/rt/src/shared/work_kind.rs, na função recorded_base_of (linha ~446): mesmo tratamento para o registro durável da unidade (`meta.json#base` / arquivo de base do corte). O teste passa a ser existência, não `self.bases.contains(...)`.
- [ ] Reescrever a documentação de recorded_or_derived_base (linhas ~332-352), que hoje justifica o descarte dizendo que 'o flow pode ter mudado desde o marcador'. A proteção continua; o que muda é o que ela mede. Diga isso com todas as letras, para que o próximo leitor não reintroduza o pertencimento.
- [ ] Substituir o teste de apps/rt/src/shared/work_kind.rs:774 — hoje ele afirma que 'um registro não declarado é descartado' e FIXA o defeito. No lugar, dois testes: the_recorded_base_survives_to_the_cut (a escolha fora da lista de configuração sobrevive até o branch cortado) e a_vanished_recorded_base_is_ignored (uma base que sumiu do remoto é ignorada e a derivação assume).
- [ ] Rodar a suíte inteira do workspace e conferir que nenhum outro teste dependia do descarte.

## Files

- `apps/rt/src/commands/event/work_branch.rs`
- `apps/rt/src/shared/work_kind.rs`
- `packages/core/src/platform/git_branches.rs`
