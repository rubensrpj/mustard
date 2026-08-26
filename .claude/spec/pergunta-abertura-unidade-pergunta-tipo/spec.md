---
id: spec.pergunta-abertura-unidade-pergunta-tipo
---

# a pergunta de abertura de unidade pergunta o tipo antes da base e perde hotfix

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**O que acontece hoje.** Toda unidade de trabalho abre com uma pergunta que decide
duas coisas: o **tipo** (o prefixo do nome da branch — `fix/`, `hotfix/`, `feature/`)
e o **sai de** (a branch base de onde a unidade é cortada). O bloco-modelo que ensina
essa pergunta vive num único arquivo — a semente
que o instalador entrega a cada projeto — e mostra o `tipo` na primeira linha e o
`sai de` na segunda. Nesta sessão, ao abrir uma correção, o que chegou ao
operador foi isto:

```
  1. fix, saindo de dev
  2. hotfix, saindo de main
```

**Por que isso é um problema.** Três defeitos de uma vez, e nenhum deles é descuido
de quem renderizou — os três são lacunas do texto que ele seguiu:

1. **A ordem está invertida.** Quem abre uma unidade decide primeiro DE ONDE ela
   parte, e só depois COMO ela se chama. Perguntar o tipo primeiro faz a base parecer
   consequência do tipo — exatamente a implicação que este produto já removeu quando
   passou a escolher a base contra o catálogo real do `origin`.
2. **Os dois campos foram pareados.** O texto manda perguntar "as duas coisas juntas",
   e isso foi lido como *combiná-las*. O resultado é o produto cartesiano de duas
   escolhas independentes: quem quer `hotfix` saindo de `dev` simplesmente não acha a
   linha. A implicação tipo→base volta pela porta dos fundos.
3. **`hotfix` sumiu da lista.** O seletor do harness aceita no máximo 4 opções por
   pergunta. O texto sugere seis tipos e nunca menciona esse teto, então o
   renderizador cortou a sugestão que sobrava — e foi justamente `hotfix` que caiu.
4. **O nome da unidade não pode ser corrigido — por ninguém.** A linha `branch:` do
   bloco é só um aviso: ela MOSTRA o nome, e não há como recusá-lo. O portão
   (`emit-pipeline`) deriva o nome do texto do pedido e descarta qualquer nome que o
   chamador tenha sugerido, dizendo em voz alta "uma unidade tem um nome só".
   A regra em si está certa e existe por um motivo real —
   antes dela, uma unidade carregava dois nomes ao mesmo tempo. O que está errado é o
   alvo: ela foi escrita para calar o CHAMADOR que inventava um nome em silêncio, e
   acabou calando também o OPERADOR, que é a única pessoa que sabe como a unidade
   deveria se chamar. O próprio texto do código diz onde está a linha: *"o que não
   está em jogo é o silêncio"*. Um operador que vê o nome derivado e o corrige de
   propósito é o contrário de silêncio.

**O que muda.** A pergunta de abertura vira um formulário de tres campos, todos
corrigiveis, e o portao passa a aceitar um nome escolhido pelo operador.

A prosa do roteador passa a mostrar a base antes do tipo; a dizer, com todas as letras,
que os campos sao independentes e nunca opcoes pre-combinadas; a nomear o teto de
quatro opcoes da superficie de pergunta, do qual `hotfix` nunca pode ser cortado e no
qual sempre sobra campo livre para outro rotulo; e a apresentar o nome da branch para
confirmacao ou correcao, nao como aviso.

O portao ganha um sinal novo e explicito pelo qual o nome do operador vence a
derivacao — distinto do palpite de hoje, que perde de proposito. Editar o nome da
branch e editar o tipo e o nome numa string so, entao a juncao continua com uma grafia
unica e a lei "uma unidade, um nome" segue de pe.

As catracas prendem cada uma dessas leis. Hoje elas exigem que as linhas da pergunta
existam, mas nao exigem ordem entre elas — e foi por isso que nada impediu a regressao
que acabou de acontecer.

```
ANTES                              DEPOIS
┌──────────────────────────┐       ┌────────────────────────────────────┐
│ tipo:   [fix] feature …  │       │ sai de: [dev]  main  release/…     │
│ sai de: [dev] main …     │  ──►  │ tipo:   [fix]  hotfix  feature  ✎  │
│ branch: fix/o-botao-…    │       │ branch: fix/o-botao-…           ✎  │
└──────────────────────────┘       └────────────────────────────────────┘
 renderizado como pares:            3 campos independentes; ✎ = corrigível
 "fix saindo de dev"                o nome do operador chega ao portão
 "hotfix saindo de main"            e ganha da derivação
 branch: só aviso, não editável
```

**Como termina.** A próxima unidade — em qualquer projeto que receba a atualização —
abre perguntando a base primeiro, contra as branches que existem de verdade; depois o
tipo, com `hotfix` sempre entre as sugestões e campo livre para o resto; e por fim o
nome da branch já preenchido com a sugestão, que o operador aceita com um Enter ou
reescreve ali mesmo. Um nome ruim é corrigido ANTES de a branch existir — que é o que
a prosa sempre prometeu e só o `tipo` cumpria.

## Usuários/Stakeholders

Quem abre unidades de trabalho — a pergunta é a primeira coisa que o harness diz e a
única que exige resposta antes de qualquer código existir.

## Métrica de sucesso

A pergunta de abertura é respondida numa passada, sem correção nem reabertura: as três
respostas do operador chegam ao portão como foram dadas — a base, o tipo e o nome — e
nenhuma delas é descartada em silêncio.

## Não-Objetivos

- Não mexe em `base-candidates` nem no cálculo `{tipo}/{nome}` da branch: o catálogo
  de bases já é medido de verdade e a junção do nome tem uma grafia só, que continua
  sendo a única.
- Não afrouxa a lei "uma unidade, um nome". O nome do operador substitui a derivação,
  não convive com ela: continua havendo exatamente um nome, e a troca é registrada.
- Não fecha o vocabulário de tipos: continua rótulo aberto, com sugestões.
- Não trata o defeito da barra de status (binário antigo após `/upsert`) — é outra
  unidade, com outra causa.

## Arquivos

| arquivo | papel nesta unidade |
|---|---|
| `packages/core/templates/mustard/orchestrator.md` | a semente: o bloco-modelo da pergunta e a regra ao lado dele |
| `.claude/mustard/orchestrator.md` | a cópia entregue neste projeto, que uma catraca exige byte-idêntica à semente |
| `apps/rt/src/commands/event/emit_pipeline.rs` | o portão: onde o nome é cunhado hoje e onde o nome do operador passa a ganhar |
| `apps/rt/src/commands/event/cli.rs` | a declaração do sinal novo na linha de comando do portão |
| `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs` | as catracas que prendem prosa e comportamento juntos |

## Critérios de Aceitação

- **AC-1** — when o bloco-modelo do roteador é lido, then a linha `sai de:` aparece
  ANTES da linha `tipo:`, e a linha `tipo:` contém `hotfix`
  Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_asks_the_base_before_the_type`
  Expect: `1 passed`
  Control: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_prose_teaches_the_kind_named_branch_and_its_one_question`
- **AC-2** — when a regra ao lado do bloco é lida, then ela nomeia o teto de opções da
  superfície de pergunta, proíbe parear os campos e prende `hotfix` na lista
  Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_forbids_pairing_and_pins_hotfix`
  Expect: `1 passed`
  Control: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_prose_teaches_the_kind_named_branch_and_its_one_question`
- **AC-3** — when a cópia entregue neste projeto é comparada com a semente compilada,
  then as duas coincidem também na linha `sai de:` (hoje a catraca só compara `tipo:`,
  `branch:` e a chamada do portão)
  Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour delivered_copy_matches_the_seed_at_the_base_row`
  Expect: `1 passed`
  Control: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_prose_teaches_the_kind_named_branch_and_its_one_question`
- **AC-4** — when o portão recebe um nome escolhido pelo operador pelo sinal explícito
  de renomeação, then é ESSE nome que a unidade passa a ter — ele nomeia a branch, os
  eventos e o diretório da spec — e o relatório registra de onde ele veio; um `--spec`
  comum continua perdendo para a derivação, como hoje
  Command: `cargo test -p mustard-rt operator_name_wins_over_the_derivation`
  Expect: `1 passed`
  Control: `cargo test -p mustard-rt mint_unit_name`
- **AC-5** — when o bloco-modelo é lido, then a linha `branch:` se apresenta como campo
  corrigível (sugestão + edição), e não como aviso
  Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_offers_the_name_for_correction`
  Expect: `1 passed`
  Control: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_prose_teaches_the_kind_named_branch_and_its_one_question`
- **AC-6** — quando os injetaveis do roteador sao medidos, entao cada arquivo cabe embutido no contexto sem virar arquivo com previa — a secao ## Dispatch mora num injetavel proprio, pendurado em sessionStart, e o restante do roteador segue em userPromptSubmit
  Command: `cargo test -p mustard-cli --test template_budget` Expect: `2 passed`

## Checklist

- [ ] T1 — reordenar o bloco-modelo (`sai de` antes de `tipo`) e pôr `hotfix` na linha
      de sugestões, em `packages/core/templates/mustard/orchestrator.md`.
- [ ] T2 — escrever na regra vizinha as quatro leis: ordem, campos independentes (nunca
      pareados), teto de 4 opções com `hotfix` pinado + campo livre, e `branch` como
      campo corrigível.
- [ ] T3 — no portão (`apps/rt/src/commands/event/emit_pipeline.rs`), aceitar um nome
      escolhido pelo operador por um sinal EXPLÍCITO, distinto do `--spec` de hoje: o
      nome do operador ganha da derivação, o palpite continua perdendo, e o relatório
      diz qual dos dois venceu.
- [ ] T4 — ensinar a linha de despacho do roteador a repassar esse sinal quando — e
      somente quando — o operador tiver corrigido o nome.
- [x] T5 — replicar byte a byte em `.claude/mustard/orchestrator.md` (a catraca exige).
- [ ] T6 — prender as leis em catracas novas
      (`router_asks_the_base_before_the_type`, `router_forbids_pairing_and_pins_hotfix`,
      `router_offers_the_name_for_correction`,
      `operator_name_wins_over_the_derivation`) e estender a comparação da cópia
      entregue para incluir `  sai de:`.

## Definitions

- **tipo** — O prefixo do nome da branch (`fix/`, `hotfix/`, `feature/`...). E um rotulo ABERTO: aceita qualquer token valido como segmento de ref do git, e nao decide mais nada alem do prefixo — desde que a base passou a ser escolhida a parte, `hotfix/` nao move mais o destino da unidade.
- **sai de** — A branch base de onde a unidade e cortada, escolhida contra o catalogo REAL do `origin` que `mustard-rt run base-candidates` mede (cada linha marcada `protected` e `preselected`).
- **superficie de pergunta** — O seletor que o harness renderiza para uma pergunta do orquestrador. Aceita no maximo 4 opcoes por pergunta, mais um campo livre (`Other`) — um limite que a prosa do roteador nunca mencionou.

## Decisions

- A base (`sai de`) e perguntada ANTES do tipo, e o bloco-modelo do roteador passa a mostrar essa ordem.
  Reason: O operador decide primeiro DE ONDE a unidade parte e so depois COMO ela se chama. Mostrar o tipo primeiro faz a base parecer consequencia do tipo — exatamente a implicacao que este produto ja removeu quando passou a escolher a base contra um catalogo real.
- As duas perguntas sao campos INDEPENDENTES; nunca podem ser renderizadas como opcoes ja pareadas.
  Reason: Parear ('fix saindo de dev' / 'hotfix saindo de main') devolve ao operador o produto cartesiano de duas escolhas e ressuscita a implicacao tipo->base pela porta dos fundos: o operador que quer `hotfix` saindo de `dev` nao encontra a linha.
- `hotfix` nunca pode ser cortado da lista de sugestoes de tipo, e a prosa passa a nomear o teto de 4 opcoes da superficie.
  Reason: Sem regra explicita, o renderizador corta a sugestao que sobra para caber no teto — e foi `hotfix` que caiu. Um teto que a prosa nao nomeia e um teto que o leitor descobre errando na frente do operador.

- O nome da unidade passa a ser CORRIGIVEL pelo operador, por um sinal explicito e distinto do `--spec` de hoje.
  Reason: A lei "uma unidade tem um nome so" foi escrita contra o CHAMADOR que inventava um nome em silencio, e acabou calando tambem o OPERADOR — a unica pessoa que sabe como a unidade deveria se chamar. O proprio codigo diz onde esta a linha: "o que nao esta em jogo e o silencio". Um operador que ve o nome derivado e o corrige de proposito e o contrario de silencio, entao o sinal novo tem de ser explicito o bastante para que os dois casos nunca se confundam.
- Editar o campo `branch` e editar `tipo` + nome numa string so; nao existe um terceiro nome livre.
  Reason: A juncao `{tipo}/{nome}` tem uma grafia unica no codigo, guardada por catraca. Um campo de branch que pudesse discordar de `tipo` e do nome ressuscitaria exatamente o defeito dos dois nomes que a lei acima existe para impedir.

## Evidence

- O bloco-modelo da pergunta e autorado num unico lugar e mostra a linha `tipo:` antes da linha `sai de:`.
  Evidence: `packages/core/templates/mustard/orchestrator.md:27`
- A prosa manda perguntar as duas coisas juntas ('ask both together'), mas nao diz que os campos sao independentes nem o que fazer quando a superficie limita o numero de opcoes — as duas lacunas que produziram o defeito.
  Evidence: `packages/core/templates/mustard/orchestrator.md:22`
- A copia entregue neste projeto e byte-identica ao seed compilado, e uma catraca exige que continue sendo: editar so o template deixaria este projeto fazendo a pergunta velha.
  Evidence: `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs:672`
- A catraca existente exige a presenca das linhas `tipo:` e `sai de:` mas NAO exige ordem entre elas — reordenar nao a quebra, e por isso nada hoje impede a regressao que este defeito acabou de mostrar.
  Evidence: `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs:618`
- O portao deriva o nome do texto do pedido e descarta o nome sugerido pelo chamador, avisando em stderr que "uma unidade tem um nome so" — nao existe hoje nenhum caminho pelo qual um nome escolhido ganhe da derivacao.
  Evidence: `apps/rt/src/commands/event/emit_pipeline.rs:588`
- A propria documentacao da funcao registra que preferir a grafia do chamador EM SILENCIO foi o que criou o defeito dos dois nomes, e que "o que nao esta em jogo e o silencio" — o que deixa a porta aberta para uma escolha explicita.
  Evidence: `apps/rt/src/commands/event/emit_pipeline.rs:549`
