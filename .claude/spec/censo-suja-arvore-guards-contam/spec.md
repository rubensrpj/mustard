---
id: spec.censo-suja-arvore-guards-contam
---

# Duas tarefas que o produto cria para o operador e nao termina: o portao de base re-minera o censo num arquivo versionado e deixa sujo, travando a unidade seguinte; e a varredura de guards conta fixtures de teste como subprojetos pendentes

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Dois defeitos, uma forma: o produto cria uma tarefa para o operador e não a termina.

**A. O portão de base suja a árvore e a unidade seguinte leva a culpa.** Ao abrir uma unidade, o portão re-minera o censo (`.claude/grain.model.json`), que é versionado. Ele nem esconde o que fez — a mensagem diz que o resultado *"pode ser commitado à parte desta unidade"*, admitindo a dívida em vez de quitá-la. O corte da branch seguinte então recusa, atribuindo a escrita a *outra unidade de trabalho do operador*. Aconteceu cinco vezes nesta sessão, cada uma exigindo um commit manual.

É literalmente o defeito que o selo do instalador tinha, com outro arquivo — e o mecanismo do conserto já está escrito.

**B. A varredura de guards conta fixture de teste como subprojeto pendente.** Ela reporta 14 pendentes aqui; 6 são projetos falsos sob `apps/scan/tests/fixtures/`, que existem só para o minerador ter o que ler nos testes. A varredura irmã, a dos molds, já descarta terreno de teste; a das guards não. As duas metades do mesmo enriquecimento discordam.

## Usuários/Stakeholders

Quem abre unidades de trabalho neste produto: hoje leva um commit manual por abertura (A) e uma contagem inflada de dívida a cada aviso (B).

## Métrica de sucesso

Abrir duas unidades seguidas não exige nenhum commit manual entre elas, e a contagem de guards pendentes não inclui nenhum diretório de teste.

## Não-Objetivos

- **Escrever as guards dos 8 subprojetos reais.** É o enriquecimento, unidade própria — esta só para de contar os 6 fantasmas.
- **Mudar o texto da mensagem do portão.** Ela melhora por consequência de a árvore ficar limpa.
- **Tornar o censo não-versionado.** Ele é útil no histórico; o problema é quem escreve não terminar.

## Critérios de Aceitação

AC = critério de aceitação: uma frase verificável por um comando.

- **AC-1** — when a varredura de guards encontra um arquivo de instruções sob um segmento de teste, then ela não desce ali e o diretório não entra na lista de pendentes
  Command: `cargo test -p mustard-rt guards_walk_skips_test_terrain 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt scan_guards_list_finds_pending_and_excludes_root 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — when o portão de base re-minera o censo numa árvore de git que ele encontrou limpa, then ao fim a árvore está limpa de novo, sem passo manual
  Command: `cargo test -p mustard-rt census_refresh_leaves_the_tree_clean 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt accepts_any_real_branch_as_base 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-3** — when a árvore já carregava trabalho do operador, then o portão registra nada e o trabalho dele fica intocado
  Command: `cargo test -p mustard-rt census_refresh_never_commits_over_the_operators_work 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt accepts_any_real_branch_as_base 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-4** — o build do workspace passa verde
  Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

| arquivo | o que muda |
|---|---|
| `packages/core/src/platform/project_seed.rs` | generalizar o mecanismo do selo para qualquer caminho que o produto escreveu; `record_version_stamp` vira invólucro fino |
| `packages/core/src/lib.rs` | cascata: re-exportar a função generalizada |
| `apps/rt/src/commands/event/base_gate.rs` | amostrar a árvore antes do scan; registrar o censo quando ela estava limpa; testes AC-2/3 |
| `apps/rt/src/commands/scan_guards/list.rs` | não descer em terreno de teste, com a mesma lista de segmentos dos molds; teste AC-1 |
| `apps/rt/src/commands/scan_patterns/list.rs` | cascata: `TEST_SEGMENTS` passa a `pub(crate)` para ser a lista única (só visibilidade) |
| `apps/cli/src/commands/init.rs` | cascata da mesclagem: o relatório do selo passa a falar `RecordOutcome`, o tipo único |
| `packages/core/tests/private_install.rs` | cascata: o controle negativo pergunta se o git RASTREIA o config, em vez de medir sujeira como proxy |

## Limites

IN: os dois defeitos acima, nos arquivos da tabela.
OUT: tudo em `## Não-Objetivos`; qualquer mudança no que o scan minera.

## Definitions

- **terreno de teste** — diretorio sob um segmento convencional de teste (`tests`, `fixtures`, `__tests__`, `spec`, `mocks`). A varredura dos molds ja o reconhece e descarta; a das guards nao.
- **censo** — `.claude/grain.model.json`, o modelo deterministico que o scan minera. E versionado neste repositorio, entao re-minerar deixa a arvore suja.

## Decisions

- As duas coisas vao na mesma unidade.
  Reason: Sao a mesma forma de defeito: o produto cria trabalho para o operador e nao termina. O censo repete literalmente o que o selo do instalador fazia — inclusive a mensagem que diz `pode ser commitado a parte`, admitindo a divida em vez de quita-la.
- Generalizar o mecanismo que ja foi escrito para o selo em vez de escrever um segundo.
  Reason: `record_version_stamp` ja resolve exatamente isto — amostra a arvore antes, commita so o proprio caminho, nao toca no index e recusa quando a arvore ja tinha trabalho do operador. Duplicar essa logica criaria duas politicas para a mesma pergunta.
- Reusar a MESMA lista de segmentos de teste que a varredura dos molds usa.
  Reason: As duas metades do enriquecimento respondem a mesma pergunta. Duas listas divergiriam na primeira vez que alguem acrescentasse um segmento a uma so.

## Evidence

- DEFEITO A — `refresh_census_if_stale` roda o scan, grava o modelo versionado e anuncia que deixou trabalho: a propria mensagem diz que o resultado `pode ser commitado a parte desta unidade`.
  Evidence: `apps/rt/src/commands/event/base_gate.rs:238`
- O corte da branch da unidade seguinte entao recusa, atribuindo a escrita a OUTRA unidade de trabalho do operador — o mesmo texto e a mesma atribuicao errada que o selo do instalador provocava.
  Evidence: `packages/core/src/platform/i18n.rs:596`
- Reproduzido cinco vezes nesta sessao: cada abertura de unidade re-minerou o censo e a abertura seguinte foi recusada ate um commit manual.
  Evidence: `apps/rt/src/commands/event/base_gate.rs:220`
- O mecanismo do conserto ja existe e trata o caso privado: `record_version_stamp` curto-circuita para `Nothing` quando o caminho nao e rastreado, amostra a arvore ANTES da escrita e usa a forma com pathspec, que nao toca no index.
  Evidence: `packages/core/src/platform/project_seed.rs:1181`
- DEFEITO B — `IGNORE_DIRS` da varredura de guards nao inclui nenhum segmento de teste, so build/vendor, entao o walk desce em `apps/scan/tests/fixtures/`.
  Evidence: `apps/rt/src/commands/scan_guards/list.rs:29`
- A varredura IRMA, a dos molds, tem `TEST_SEGMENTS` e descarta o cluster sob um deles — as duas metades do mesmo enriquecimento discordam sobre o que e terreno de teste.
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:60`
- Medido neste repositorio: `scan-guards-list` devolve 14 pendentes, dos quais 6 sao fixtures (`flutter_app`, `graph_dart`, `graph_go`, `monorepo_mix/api`, `monorepo_mix/web`, `php_laravel`).
  Evidence: `apps/rt/src/commands/scan_guards/list.rs:111`
- O aviso `base-gate: enrichment stale` le esse mesmo coletor, entao os 6 fantasmas entram na contagem que o operador ve a cada abertura de unidade.
  Evidence: `apps/rt/src/commands/event/enrichment_gap.rs:104`
