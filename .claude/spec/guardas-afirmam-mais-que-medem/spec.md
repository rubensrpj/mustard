---
id: spec.guardas-afirmam-mais-que-medem
---

# seis guardas do selo de versao e da catraca de frontmatter verificam menos do que declaram

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O bump automático grava a versão em quatro arquivos, e o próprio workflow chama esses arquivos de "as pernas do selo". Cada perna tem um comando que a faz andar e uma guarda que deveria reprovar quando ela não andou. Seis dessas guardas — mais a catraca que tranca o frontmatter dos agentes — verificam menos do que a própria mensagem delas declara.

Nenhuma quebra comportamento hoje. Todas foram medidas, e é por isso que esta unidade existe: uma guarda que aprova sem medir só é descoberta no dia em que precisava ter reprovado, e nesse dia o release já saiu quebrado. Foi assim com a v0.1.29, cujo comentário ainda está no workflow.

Por que agora: os catorze achados abaixo vêm de três rodadas de revisão do PR #200 e estão todos com arquivo e linha. Tentar consertá-los dentro daquela promoção falhou — sem árvore limpa e sem ciclo próprio, cada conserto precisou do seguinte, e um deles quebrou o critério de aceite de uma spec já fechada. O material está fresco; a janela para usá-lo é agora.

## Usuários/Stakeholders

Quem publica um release. Hoje a pessoa que roda o bump recebe verde de guardas que não mediram o que dizem medir, então um lock parado pode chegar à tag e derrubar todo job de build com `--locked`. Quem escreve um arquivo de agente também: a catraca reprova uma anotação em YAML válido e ensina a apagá-la.

## Métrica de sucesso

Cada guarda tocada passa a REPROVAR pelo menos um caso que hoje ela aprova, e esse caso é demonstrado por um teste que roda. Nenhuma guarda passa a reprovar entrada válida — a catraca do frontmatter tem de aceitar comentário e continuar rejeitando valor corrompido.

## Não-Objetivos

- Acrescentar `--locked` ao build do dashboard no release. É a trava que transformaria a garantia em bloqueio, mas é outra unidade e tem risco próprio.
- Fazer a integração contínua compilar o dashboard. Ela o exclui de propósito, por causa das bibliotecas de sistema de cada sistema operacional.
- Reescrever `bump-on-main.yml` além das guardas. O que faz cada perna ANDAR está correto e medido; o alvo aqui é só o que VERIFICA.

## Critérios de Aceitação

- **AC-1** — when o `Cargo.lock` da raiz não andou mas uma dependência de terceiros está no número alvo, then a guarda da terceira perna reprova
  Command: `cargo test -p mustard-core --test version_line bump_guard_rejects_a_lock_whose_local_crates_did_not_move 2>&1 | grep -q '1 passed'`
  Control: `cargo test -p mustard-core --test version_line 2>&1 | grep -q 'test result: ok'`
- **AC-2** — when um lock fixa mais de um crate nosso, then a guarda confere TODOS eles, e não um nome escolhido à mão
  Command: `cargo test -p mustard-core --test version_line bump_guard_checks_every_local_crate_of_each_lock 2>&1 | grep -q '1 passed'`
  Control: `cargo test -p mustard-core --test version_line 2>&1 | grep -q 'test result: ok'`
- **AC-3** — when um crate nosso SOME de um lock, then a guarda reprova nomeando qual sumiu, em vez de aprovar o conjunto reduzido que sobrou
  Command: `cargo test -p mustard-core --test version_line bump_guard_rejects_a_lock_that_lost_one_of_our_crates 2>&1 | grep -q '1 passed'`
  Control: `cargo test -p mustard-core --test version_line 2>&1 | grep -q 'test result: ok'`
- **AC-4** — when a perna do dev decide pular a propagação, then ela consulta as mesmas pernas que o bloco de trabalho conserta
  Command: `cargo test -p mustard-core --test version_line dev_leg_decision_consults_what_the_work_block_repairs 2>&1 | grep -q '1 passed'`
  Control: `cargo test -p mustard-core --test version_line 2>&1 | grep -q 'test result: ok'`
- **AC-5** — when um agente declara `model` ou `effort` com comentário ou entre aspas, then a catraca lê o valor e o aceita; e continua reprovando valor com sobra depois do id
  Command: `cargo test -p mustard-rt --test plugin_agents scalar_ 2>&1 | grep -q '2 passed'`
  Control: `cargo test -p mustard-rt --test plugin_agents 2>&1 | grep -q 'test result: ok'`
- **AC-6** — o build do projeto passa verde
  Command: `cargo build --workspace`

## Arquivos

| arquivo | o que muda |
|---|---|
| `.github/scripts/check-lock-pins.sh` | **NOVO** — a guarda em si, extraída para um roteiro que o workflow e os testes rodam |
| `.github/workflows/bump-on-main.yml` | as guardas das quatro pernas, nas duas pernas do workflow, e a decisão da perna do dev |
| `packages/core/tests/version_line.rs` | testes novos que exercitam as guardas do workflow; e a justificativa falsa do interpretador de TOML escrito à mão |
| `apps/rt/tests/plugin_agents.rs` | a leitura de valor do frontmatter e a aceitação de modelo |

**O roteiro é acréscimo à tabela, feito durante a execução, e não alargamento de escopo.** A tabela original previa três arquivos porque eu imaginei as guardas continuando dentro do YAML. Elas não podiam: o achado que gerou o AC-2 é que o teste DERIVA o conjunto de crates do lock enquanto o workflow lê um nome escolhido à mão — dois critérios para a mesma coisa. Reimplementar a guarda em Rust para testá-la criaria o terceiro. Extraí-la para um roteiro que os dois rodam é a única forma de o teste exercitar **a guarda que o release realmente executa**. Continua dentro do `IN` declarado abaixo: é o que VERIFICA cada perna.

## Limites

IN: as guardas que VERIFICAM cada perna do selo; a decisão da perna do dev; a leitura de valor do frontmatter dos agentes; a justificativa do interpretador de TOML escrito à mão.

OUT: o comando que faz cada perna ANDAR (`cargo update`, os `sed`) — está correto e medido. O `--locked` no build do dashboard. A reunião de `agent_frontmatter` com o leitor canônico — é achado registrado abaixo, mas mexer no leitor compartilhado tem alcance próprio e vira unidade separada.

**Restrição dura, e é a que derrubou a tentativa anterior.** O critério AC-2 da spec FECHADA `cargo-lock-src-tauri-fica` procura a variável `dash_pin` no workflow por expressão regular. Qualquer unificação das guardas que remova essa variável deixa aquela spec vermelha, ainda que a intenção do critério — a perna do dev consultar o lock do dashboard antes de decidir — continue satisfeita.

Ao fim desta unidade, uma das duas coisas tem de valer: a variável sobreviveu à refatoração, ou o critério daquela spec foi amendado de propósito por `mustard-rt run ac-amend`, com razão declarada. O que não pode acontecer é descobrir isso depois, que foi como a tentativa anterior morreu.

Isto está aqui e **não** entre os critérios de aceitação, de propósito. Um critério precisa saber distinguir feito de não-feito, e este comando já é verde hoje — vira decoração se entrar na lista. A verificação existe, e é o próprio AC-2 daquela spec:

```sh
test "$(grep -c 'cargo update --workspace --manifest-path apps/dashboard/src-tauri/Cargo.toml' .github/workflows/bump-on-main.yml)" = 2 \
  && test "$(grep -c 'git add .*apps/dashboard/src-tauri/Cargo.lock' .github/workflows/bump-on-main.yml)" = 2 \
  && grep -qE '^[[:space:]]*if \[.*dash_pin.*\]; then' .github/workflows/bump-on-main.yml
```

## Definitions

- **perna do selo** — Cada um dos quatro arquivos em que o bump automatico grava a versao: plugin/.claude-plugin/plugin.json, o Cargo.toml da raiz, o Cargo.lock da raiz e o Cargo.lock do dashboard. O proprio workflow usa esse nome.
- **guarda** — A linha do workflow cuja unica funcao e REPROVAR quando uma perna nao andou. E distinta do comando que faz a perna andar: o comando trabalha, a guarda verifica. Uma guarda que aprova sem medir e decoracao.
- **catraca** — Teste que tranca uma superficie ja entregue para que ela nao regrida em silencio. Aqui, a que cobra `model` e `effort` no frontmatter dos agentes de plugin/agents/.

## Decisions

- Consertar estas guardas como unidade propria, nunca dentro de uma promocao.
  Reason: Foi tentado dentro do PR #200 e falhou em tres rodadas de revisao. Sem arvore limpa e sem ciclo proprio, cada conserto precisou do seguinte: o segundo reintroduziu um nivel abaixo dois dos defeitos que ia resolver, quebrou o criterio de aceite de uma spec ja fechada e deixou a catraca aprovando YAML invalido. Os dois commits foram revertidos e a arvore voltou byte a byte ao estado revisado.
- Nao trocar a lista nomeada de crates por um conjunto puramente derivado do lock sem manter a deteccao de crate AUSENTE.
  Reason: Medido na tentativa anterior: um conjunto derivado do lock nao percebe um crate que sumiu dele. A guarda passa verde justamente no caso em que uma dependencia foi removida por engano, que e o oposto do que ela existe para fazer. version_line.rs tem a mesma lacuna, cujo unico piso e `!ours.is_empty()`.
- Decidir o destino do AC-2 de cargo-lock-src-tauri-fica ANTES de unificar as guardas.
  Reason: Aquele criterio de aceite procura a variavel `dash_pin` no workflow. Qualquer unificacao das guardas a remove e deixa uma spec ja fechada vermelha, sem que a intencao do criterio tenha deixado de ser satisfeita. Contorcer o codigo para satisfazer um grep e mexer numa spec congelada sao escolhas diferentes, e nenhuma das duas e neutra.
- A colisao de versao com dependencia de terceiros e descrita como CLASSE, nao como estado presente.
  Reason: A colisao foi medida em v0.1.44, quando `tracing` estava exatamente nesse numero. Em v0.1.45 ela nao esta viva, porque `tracing` continua em 0.1.44. Descrever como se fosse presente seria falso; descrever como resolvida seria pior, porque ela volta toda vez que um crate de terceiros cair no nosso numero de patch.

## Evidence

- A guarda da TERCEIRA perna casa por NUMERO e nao por pacote. Um `grep -q '^version = "$nv"$' Cargo.lock` e satisfeito por qualquer dependencia de terceiros que esteja naquele numero. Medido em v0.1.44: `tracing` estava em 0.1.44 com o repositorio tambem em 0.1.44, e a guarda passava sozinha por causa dela. Provado tambem em lock forjado, onde a guarda aprova um lock que nao andou.
  Evidence: `.github/workflows/bump-on-main.yml:88`
- O comentario que fica tres linhas acima dessa guarda ja avisa contra exatamente essa confusao: diz preferir `cargo update` a um `sed` no lock porque o `sed` 'nao distingue a nossa versao da de uma dependencia que por acaso tenha o mesmo numero'. O aviso valia para o comando de trabalho e nunca foi aplicado a guarda.
  Evidence: `.github/workflows/bump-on-main.yml:84`
- A mesma guarda por numero solto se repete na perna do dev.
  Evidence: `.github/workflows/bump-on-main.yml:145`
- A guarda da QUARTA perna le UM dos dois crates nossos que o lock do dashboard fixa. Medido: aquele lock fixa `mustard-core` e `mustard-cli`, e a guarda le so `mustard-core`. Se o outro atrasar, a guarda aprova, o commit e tagueado, e o teste fica vermelho num commit que a integracao continua nao roda por anti-recursao do GITHUB_TOKEN.
  Evidence: `.github/workflows/bump-on-main.yml:102`
- A mesma guarda parcial se repete na perna do dev.
  Evidence: `.github/workflows/bump-on-main.yml:159`
- A DECISAO da perna do dev consulta TRES pernas enquanto a mensagem do ramo `then` diz 'nas quatro pernas'. Falta o Cargo.lock da raiz. Com ele atrasado e os outros tres em dia, o bloco inteiro e pulado e ele nunca anda.
  Evidence: `.github/workflows/bump-on-main.yml:133`
- O teste que cobra o lock do dashboard DERIVA o conjunto de pacotes locais do proprio lock (`source` ausente, menos o proprio dashboard), enquanto a guarda do workflow le um nome escolhido a mao. Sao dois criterios diferentes para a mesma coisa.
  Evidence: `packages/core/tests/version_line.rs:210`
- `declared()` devolve todo o rabo da linha, entao `model: sonnet   # comentario` — YAML valido que o runtime resolve para `sonnet` — e reprovado com a mensagem 'which Claude Code does not resolve'. Provado por execucao: o arquivo de agente real foi editado com um comentario e a catraca reprovou.
  Evidence: `apps/rt/tests/plugin_agents.rs:150`
- `model_is_accepted` aceita qualquer valor que apenas COMECE com `claude-`, sem olhar o resto. Provado por execucao: `model: "claude-opus-5"  # papel` passa na catraca, certificando um valor que nenhum runtime resolve.
  Evidence: `apps/rt/tests/plugin_agents.rs:123`
- A justificativa do interpretador de TOML escrito a mao e falsa. O comentario diz existir para nao puxar um interpretador de TOML para as dependencias de teste desta crate.
  Evidence: `packages/core/tests/version_line.rs:159`
- `toml = "1"` ja e dependencia REGULAR do mustard-core, entao o interpretador que o comentario acima diz evitar ja esta disponivel para aquele teste a custo zero.
  Evidence: `packages/core/Cargo.toml:39`
- `agent_frontmatter` e um terceiro leitor de frontmatter escrito a mao, que usa `strip_prefix("---")` e por isso entra em panico diante de um arquivo salvo com marca de ordem de bytes, dizendo que falta a cerca que esta la.
  Evidence: `apps/rt/tests/plugin_agents.rs:135`
- O projeto DECLARA um leitor de frontmatter canonico, tolerante a marca de ordem de bytes e com quebra de linha normalizada, e diz explicitamente que esta familia e o `mold_gate` compartilham UM leitor so.
  Evidence: `apps/rt/src/commands/scan_patterns/origin.rs:29`
- O criterio de aceite AC-2 desta spec, ja FECHADA, procura a variavel `dash_pin` no workflow por expressao regular. Qualquer unificacao das guardas que remova essa variavel deixa o criterio vermelho, embora a intencao dele — a perna do dev consultar o lock do dashboard antes de decidir — continue satisfeita.
  Evidence: `.claude/spec/cargo-lock-src-tauri-fica/spec.md:68`

<!-- wikilinks-footer-start -->
- [:space:](?) ⚠ unresolved
<!-- wikilinks-footer-end -->