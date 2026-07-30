---
id: wave.harness-ve-toda-branch-trabalho.1-enumerator
---

# wave-1-enumerator

## Summary

Uma única fonte de verdade sobre branches: o enumerador varre refs locais e remotas por prefixo de base, o classificador cruza ancestralidade local com a consulta de PR atrás de uma porta, e o relatório sai por uma flag do ritual de saída — sem poder apagar nada.

## Network

- Parent: [[spec.harness-ve-toda-branch-trabalho]]

## Tasks

- [ ] shared/branch_state.rs (novo): o BranchEnumerator varre refs/heads/ E refs/remotes/<remoto>/ via for-each-ref, filtrando pelo conjunto de bases do projeto (git.integration_bases()). Uma ref sem underscore após o prefixo nunca entra — o base_of_branch do git_settle já garante isso com split_once e propagação de None, e há teste fixando; reuse esse predicado, não escreva um segundo.
- [ ] shared/branch_state.rs: o StateClassifier cruza o enumerador com a consulta de PR e devolve UMA variante por branch, cobrindo as sete situações: rascunho abandonado (local, sem remota, sem PR), subiu-sem-PR, em review, deve-poda (mergeada, remota viva), deve-poda-local (mergeada, remota ausente), PERIGO (remota ausente e merge NÃO verificado) e só-no-remoto. A ancestralidade é local e sem rede (branch --merged contra a base); a rede só confirma.
- [ ] shared/branch_state.rs: a porta PrLookup abstrai a consulta de PR. O provedor vem de mustard.json#git.provider — o classificador NUNCA nomeia um CLI. Porta ausente ou não autenticada responde Desconhecido com o motivo, jamais Ausente: reportar estado não medido como negativo é a classe de defeito que esta spec existe para não repetir.
- [ ] shared/branch_state.rs: separe por tipo a visão de LEITURA da capacidade de APAGAR. O relatório e a statusline recebem apenas os estados; nenhuma função de exclusão fica alcançável a partir do módulo de relatório. A segurança desta onda é estrutural, não disciplinar — e AC-6 a afirma lendo o próprio fonte, como o teste de prosa do plugin já faz.
- [ ] git_settle.rs: a lista de unidades mergeadas pendentes (hoje calculada em torno da linha 545 enumerando entradas de worktree) passa a consumir o enumerador. Isso é o conserto de um campo que MENTE POR OMISSÃO: o portão de branch corta in-place por padrão, sem worktree, então o campo responde lista vazia havendo unidades pendentes. Medido em campo: seis locais e seis remotas invisíveis.
- [ ] git_settle.rs: a prosa do módulo afirma que este repositório faz squash-merge e que isso quebra ancestralidade pura. Medido FALSO — os três merges de 2026-07-30 têm dois pais cada e branch --merged reconhece todas as seis branches. O fallback pelo provedor é certo EXISTIR (o portal pode squashar); o que sai é a afirmação não medida sobre o repositório.
- [ ] git_cli.rs: uma flag de relatório no subcomando git-settle que já existe. NÃO crie subcomando novo: o Guard do crate exige quatro registros e um chamador, e a decisão registrada é não crescer superfície para o que o CLI do provedor quase resolve. O relatório sai por repositório, como o campo repos do settle já faz.

## Files

- `apps/rt/src/shared/branch_state.rs`
- `apps/rt/src/commands/git_settle.rs`
- `apps/rt/src/commands/git_cli.rs`

## Reality Obligations

- **RO-1.1** — Consultar a documentação oficial do CLI do provedor configurado para a forma EXATA da consulta de PRs mergeados por branch de origem e do JSON que ela devolve — o adaptador da porta não deve adivinhar nomes de campo nem o comportamento quando não há resultado.
