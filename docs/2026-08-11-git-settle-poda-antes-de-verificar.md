# `git-settle` poda antes de verificar — e o laudo chega tarde

Proposta de correção no ritual de saída (`mustard-rt run git-settle`), a partir de um `pr close` real numa unidade de dois repositórios (monorepo + submódulo).

Confiança: **alta** no diagnóstico — reproduzido nos dois repositórios da mesma unidade, com causas de sujeira diferentes e desfecho idêntico, e confirmado lendo `apps/rt/src/commands/git_settle.rs`. **Média** na terceira proposta, que muda um critério de segurança e merece revisão de quem escreveu a exceção original.

Base empírica: unidade `busca-parceiros-ignora-acento-termo`, 11/08/2026, monorepo `sialia` com o submódulo `Sialia.Backend`. Dois PRs mergeados, `pr close` executado na ordem do protocolo. A memória do projeto registra o mesmo desfecho em 30/07/2026, uma das vezes com árvore limpa — não é incidente isolado.

## O que aconteceu

Depois do `pr close` no monorepo, o arquivo que eu acabara de corrigir apareceu na árvore de trabalho **sem a correção**. A leitura imediata — a única disponível para quem está olhando o editor — é que o trabalho evaporou.

Não tinha evaporado. O `dev` local ficou no commit anterior ao merge; a correção estava em `origin/dev`. Um `git merge --ff-only origin/dev` resolveu, sem conflito e sem stash.

O relatório do comando trazia `complete: true` no meio do JSON e, mais abaixo, `baseCheckout: {updated: false, reason: "dirty-tree"}`. O `ok: false` com `reason: "base-behind"` estava lá — o comando **não** escondeu o fato. O problema é outro, e é de ordem.

## O defeito: a poda precede a verificação

A sequência real (`git_settle.rs`):

```
593  in_place_exited = checkout(base)        ← a árvore já mostra a base atrasada
604  update_bases(...)                       ← tenta o ff; FALHA por dirty-tree
617  ...prune: branch -D + push --delete     ← executa mesmo assim
707  ok = pass_is_ok(action, base_advanced)  ← só agora nota que a base ficou atrás
730  report["reason"] = "base-behind"        ← laudo pós-morte
```

O `ok: false` é honesto e chega **depois** da ação irreversível. A branch local e a remota já foram apagadas quando o comando conclui que o passo anterior não deu certo.

Um `ok: false` que descreve um estado já consumado não é um portão — é uma certidão. E o estado que ele certifica é o pior possível: a base local não tem o trabalho, e a branch que o tinha não existe mais em lugar nenhum local.

Nada se perde de verdade, porque o merge está no remoto. Mas "recuperável por quem souber que `origin/dev` existe" é uma garantia bem mais fraca do que a que o ritual deveria oferecer, e o susto é real.

**Proposta 1 — inverter, e transformar a certidão em portão.** A poda só acontece depois de confirmar, por ancestralidade local, que a base contém o commit da unidade. Não "o PR foi mergeado no provedor" — isso é fato remoto e já é verificado antes. O que autoriza apagar a branch local é o fato **local**.

```
hoje                              proposto
  checkout base                     checkout base
  tenta avançar   ← falha           tenta avançar
  poda            ← irreversível    a base contém o commit da unidade?
  ok:false                            sim → poda → ok:true
                                      não → NÃO poda → ok:false + nextAction
```

No caminho de falha, o desfecho honesto é: a base ficou atrás, a branch continua onde estava, e o relatório diz o que rodar. O usuário perde um comando de tempo, não a referência ao próprio trabalho.

## A causa imediata: a guarda é mais rigorosa que a operação que protege

`update_bases` (416-421) decide se pode avançar com uma checagem própria antes de tentar:

```rust
let clean = git_out(main, &["status", "--porcelain"])
    .map(|s| !s.lines().any(|l| blocks_fast_forward(l, submodules)))
    .unwrap_or(false);
if !clean { /* dirty-tree */ }
else if git_ok(main, &["merge", "--ff-only", ...]) { /* updated */ }
else { /* non-ff-or-no-remote */ }
```

E `blocks_fast_forward` (401-407) trata como impedimento **qualquer** linha de status que não seja `.claude/worktrees/` nem um gitlink de submódulo.

O `merge --ff-only` do git é mais fino que isso: ele recusa quando o avanço tocaria um caminho modificado, não quando existe modificação em algum lugar. Foi exatamente a diferença no caso real — o que estava sujo era `documentacao/` e `scripts/`, o que o avanço trazia era `partners.graphql.ts`, a spec e o gitlink. Interseção vazia. Rodei o mesmo `merge --ff-only` na mão, com a árvore igualmente suja, e ele fast-forwardou sem reclamar.

Ou seja: a guarda barra um avanço que a operação guardada faria com segurança, e o preço desse excesso de zelo é o estado ruim da seção anterior — a base fica atrás **porque** a guarda disparou.

**Proposta 2 — deixar o git ser a autoridade.** O ramo `else if` já tenta o `merge --ff-only` e já trata a recusa em `non-ff-or-no-remote`. A operação é atômica e falha sem efeito colateral. A checagem prévia, portanto, não protege de nada que o git não proteja sozinho — ela apenas recusa mais casos.

Se a intenção de `dirty-tree` era produzir um diagnóstico melhor que `non-ff-or-no-remote`, o mesmo diagnóstico pode sair **depois** da tentativa: recusou e a árvore estava suja → `dirty-tree`; recusou e estava limpa → divergência real.

**Proposta 3 (mínima, se a 2 for ampla demais) — untracked nunca deveria contar.** `status --porcelain` lista arquivos não rastreados, e `blocks_fast_forward` não os distingue. Um arquivo que o git não rastreia não bloqueia fast-forward algum, salvo colisão de caminho — e nessa colisão o próprio git recusa. Ignorar as linhas `??` já teria evitado o caso do submódulo, onde o **único** impedimento era um untracked.

## O agravante: o harness suja a própria árvore

No submódulo, o que travou o avanço foi um arquivo: `.claude/feature-digest.json`. Gerado pelo Mustard.

No monorepo, o mesmo `pr close` deixou para trás `.claude/spec/<slug>/qa/` e `.claude/spec/<slug>/qa-report.json`, também untracked, também do Mustard, prontos para travar o próximo `pr close` pelo mesmo motivo.

A exceção de `.claude/worktrees/` em `blocks_fast_forward` mostra que o problema já foi reconhecido uma vez — mas foi resolvido para um diretório, não para a classe. Quanto mais o harness trabalha, mais artefatos ele deixa, e mais provável fica travar o próprio ritual de saída.

**Proposta 4 — os artefatos do harness saem da conta.** Ou pela proposta 3 (que os cobre por serem untracked), ou estendendo a isenção a `.claude/` gerado, ou fazendo esses artefatos nascerem ignorados. Qualquer uma serve; o que não serve é o harness competir consigo mesmo.

## Ordem sugerida

1. **Proposta 1** (ordem) — é a única que muda o estado do repositório, e não depende das outras. Mesmo mantendo a guarda conservadora, o pior desfecho passa a ser "não avancei, sua branch continua aí": honesto e reversível.
2. **Proposta 3 ou 4** (untracked / artefatos) — barata, escopo pequeno, elimina a causa mais frequente.
3. **Proposta 2** (deixar o git decidir) — a mais correta e a que eu deixaria por último, porque mexe no critério de segurança de uma ferramenta de git e merece mais cuidado que as outras somadas.

## O que não mudar

O `reason: "base-behind"` de 730-751 e a distinção que o comentário de 731-737 defende — `updated:false` cobrindo "atrás", "à frente" e "recusado por outro worktree" — estão certos, e o comentário registra que a indistinção já foi achada em revisão. O problema nunca foi a qualidade do laudo. Foi o laudo ser laudo.

Também não mudar a verificação de merge como portão duro (fail-open em tudo, menos nela). É a regra que faz a poda ser segura no caminho feliz; a proposta 1 apenas estende o mesmo princípio ao passo seguinte.

## Nota sobre o protocolo, não sobre o código

O `pr close` documentado manda rodar `git-settle`, depois `ExitWorktree`, depois `git-settle --unit`. Para uma unidade **in-place** (sem worktree), o primeiro `git-settle` já faz tudo — checkout da base, poda — como o próprio doc de módulo descreve. Quem segue o protocolo passo a passo executa o ritual inteiro no primeiro comando e fica com dois passos que não têm mais o que fazer. Vale o protocolo dizer isso na tabela, e não só no comentário do módulo.
