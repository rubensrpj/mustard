---
id: spec.o-branch-de-trabalho-passa
---

# o branch de trabalho passa a ser nomeado pelo tipo (feature, fix, hotfix) e o tipo mais a base sao perguntados numa unica pergunta com padrao pre-marcado, mantendo os nomes antigos funcionando

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Hoje, quando você pede qualquer coisa e o harness abre uma unidade de trabalho, o branch nasce chamado `dev_o-que-voce-pediu`. O prefixo registra de onde a unidade foi cortada, e o programa depois lê esse mesmo prefixo de volta para descobrir a base — é assim que o pull request sabe para onde ir e que a verificação de "já foi integrado" sabe contra o quê comparar.

Isso funciona, e mesmo assim é a informação errada no lugar mais visível. Olhando a lista de branches, o que o operador precisa saber é o que cada unidade É — funcionalidade, correção, emergência — e não de onde ela saiu, que é quase sempre a mesma base. O convite do git-flow (`feature/`, `fix/`, `hotfix/`) diz exatamente isso, e é o que qualquer pessoa que chega ao projeto já espera ler.

Trocar o prefixo, porém, apaga a informação que o programa lia dali. Ela precisa vir de outro lugar — e já vem: a configuração do projeto declara o fluxo entre as bases, quem desemboca em quem. Então o prefixo passa a dizer o tipo, e a base vira consequência do tipo, lida da configuração em vez de recuperada de um pedaço de texto.

Falta a parte que o programa não consegue adivinhar. Uma correção que espera a próxima entrega e uma que vai direto para produção são a MESMA mudança de código; o que difere está na cabeça de quem pediu, não no pedido. Por isso o tipo e a base não são inferidos: são perguntados, uma vez, no começo da unidade, com a resposta provável já marcada — quem aceita gasta um Enter, quem quer outra coisa troca ali.

Depois desta unidade: o branch se chama pelo que é, o nome aparece antes de existir, a base é escolha sua com sugestão, e nada disso exige decorar palavra nenhuma.

## Usuários/Stakeholders

Quem abre unidades de trabalho neste harness — hoje uma pessoa só, o operador do projeto, mas o nome do branch é a primeira coisa que qualquer pessoa nova lê num repositório, e `feature/` diz sozinho o que `dev_` não diz. O caso da emergência é sentido por quem precisa mandar uma correção para produção sem ela passar pela fila normal.

## Métrica de sucesso

Abrir uma unidade custa uma pergunta com a resposta já marcada, e o branch resultante se chama pelo tipo. Nenhuma unidade que já estava em voo com o nome antigo é perdida: o pull request continua achando a base, e a recusa da segunda unidade continua reconhecendo a primeira.

## Não-Objetivos

- Renomear branches que já existem. Os nomes antigos continuam sendo entendidos; nenhum é reescrito.
- Guardar a resposta da pergunta em configuração. Foi considerado e recusado: cria mais um campo para manter e para ficar desatualizado, e a resposta muda de tarefa para tarefa.
- Rotular bases como "homologação" ou "produção" no `mustard.json`. O fluxo declarado já diz quem desemboca em quem; quando sobra mais de uma candidata, quem escolhe é o operador na hora, não um campo novo.
- Inferir emergência do texto do pedido. A mesma frase descreve as duas coisas; adivinhar aqui é decidir sozinho para que lado a correção vai.
- Mudar como a unidade é NOMEADA (o slug). Só o prefixo muda; a derivação do nome a partir do pedido continua onde está.

## Critérios de Aceitação

- **AC-1** — when a work unit is opened, then its branch is named by the unit's kind as feature/, fix/ or hotfix/, never by the base it was cut from
  Command: `cargo test -p mustard-rt a_unit_branch_is_named_by_its_kind` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt compute_work_branch`
- **AC-2** — when a branch name no longer carries the base, then the base is still resolved from the flow declared in mustard.json and never from the branch string
  Command: `cargo test -p mustard-rt the_base_comes_from_the_declared_flow_not_from_the_branch_name` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_for`
- **AC-4** — when a branch carries the old base-prefixed shape, then it is still recognised as that unit's branch and still resolves to its base
  Command: `cargo test -p mustard-rt an_old_shape_branch_is_still_understood`
  Expect: `[1-9][0-9]* passed`
- **AC-5** — when the unit is a hotfix, then it is cut from an integration base that is not the default work base, and the operator chooses when more than one candidate exists
  Command: `cargo test -p mustard-rt a_hotfix_is_cut_from_a_base_that_is_not_the_work_base`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — o build do projeto passa verde
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — primeira tarefa rastreável.

## Definitions

- **hotfix** — Not a KIND of work but a DESTINATION. The same code change is a fix or a hotfix depending only on whether it goes straight to production or waits for the next release. Nothing in the request text distinguishes them, which is why the harness cannot infer it and must ask.
- **base padrão** — The integration base a normal work unit is cut from: the value of the `*` key in `mustard.json#git.flow`. It is `dev` in this project and `develop` in another; no branch name is ever hardcoded.
- **base de emergência** — An integration base that is NOT the default work base — where a hotfix is cut from. With two bases declared there is exactly one candidate and nothing is asked; with three or more (e.g. a `qas` between `dev` and `main`) there are several and the operator picks at that moment.

## Decisions

- the branch prefix names WHAT the unit is (feature/, fix/, hotfix/), and the base becomes a consequence of that
  Reason: Today the prefix records where the unit was cut FROM and the base is recovered by parsing the name back. Naming the kind is what the operator actually wants to see, and deriving the base from configuration instead of from the string removes the parsing entirely.
- the type and the base are ALWAYS asked, in one question, with the likely answer pre-marked
  Reason: The user rejected both alternatives explicitly. Inferring silently decides for them something that changes day to day; persisting the answer in mustard.json creates one more field to maintain and to go stale. A pre-marked question costs one Enter, stores nothing, and leaves the choice with the operator — including a base they can override.
- the resulting branch name is shown in that same question, before the branch exists
  Reason: A name that is wrong is corrected there, at zero cost, instead of after the unit is cut.
- branches already in the old {base}_{slug} shape must keep being understood
  Reason: Units in flight would be orphaned otherwise: the PR target, the merged-ancestry check and the second-unit refusal all resolve a unit through its branch name.
- nothing is asked when there is only one candidate base
  Reason: The user's standing constraint for this whole area is that the process must be simple and frictionless. A question with a single possible answer is pure ceremony.

## Evidence

- compute_work_branch builds the branch as {base}_{slug}, so the integration base is encoded in the name the operator reads
  Evidence: `apps/rt/src/commands/event/work_branch.rs:96`
- base_for recovers the base by longest-match of the branch name against the declared integration bases — this is the consumer that breaks if the prefix stops naming the base
  Evidence: `apps/rt/src/commands/event/work_branch.rs:245`
- is_protected treats a branch as protected only when it IS an integration base, so it is unaffected by the prefix change
  Evidence: `apps/rt/src/commands/event/work_branch.rs:287`
- the project declares flow as {"*": "dev", "dev": "main"}, so the default work base and the single emergency candidate are both derivable without hardcoding either name
  Evidence: `mustard.json:3`
- the base gate derives the canonical slug from --intent and echoes the unit's branch, so the naming change lands at the same place the unit is already named once
  Evidence: `apps/rt/src/commands/event/work_branch.rs:335`