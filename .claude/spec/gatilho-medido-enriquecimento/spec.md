---
id: spec.gatilho-medido-enriquecimento
---

# o portao de base passa a medir a lacuna do enriquecimento (Guards pendentes e moldes sem autor) e reportar em stderr a unidade propria que a fecha; o roteador ganha a regra que despacha o fluxo scan, e o comentario que ainda cita a porta /scan selada e corrigido

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Quando uma unidade abre, o portão de base remina o censo determinístico (`grain.model.json`) e
imprime em stderr uma linha dizendo que reminerou. Isso é METADE do que o fluxo `scan` faz. A outra
metade — a prosa dos `## Guards` de cada subprojeto e os moldes `{papel}-pattern` — é escrita por
agente, e um processo Rust não escreve prosa. A onda 6 (`surface-fold`) da spec
`work-unit-lives-on-its` selou a porta digitável e automatizou apenas a metade determinística; a
metade escrita ficou dependendo de alguém ACHAR que o modelo envelheceu. A descrição do próprio
fluxo admite: *weak fallback only… when the model is visibly stale*. "Visivelmente" não é medida
nenhuma.

Por que agora. Medido neste repositório em 21/08/2026: 6 moldes candidatos sem autor —
`core-entry`, `core-outcome`, `dashboard-section`, `rt-azure`, `rt-branch`, `rt-pr` — e nenhuma
linha, em lugar nenhum, dizendo isso. Todo agente despachado nesses subprojetos escreve sem o molde
que ensina a convenção da casa, e o silêncio se lê exatamente como "está tudo em dia". O operador
que tentou puxar a alavanca à mão foi recusado pela trava das quatro portas — recusa correta; o que
falta é o gatilho do outro lado.

## Usuários/Stakeholders

Quem abre unidades de trabalho: passa a saber que o modelo do projeto está pela metade ANTES de
despachar agentes que dependem dele. E quem mantém o portão: o comentário do código volta a
descrever um caminho que existe.

## Métrica de sucesso

A lacuna deixa de ser invisível. Num repositório com molde sem autor ou `## Guards` ainda em
esqueleto, a abertura de unidade imprime UMA linha em stderr que nomeia a contagem, alguns slugs e
a unidade própria que fecha. Onde a lacuna é vazia, o portão continua mudo — nenhuma linha nova, e
a linha JSON de stdout permanece idêntica byte a byte.

## Não-Objetivos

- Reabrir a porta digitável: a superfície de quatro portas fica como está, trancada por teste.
- Rodar o enriquecimento automaticamente — ele escreve arquivos versionados e exige árvore limpa,
  então continua sendo unidade PRÓPRIA, aberta por decisão e não como passo de outra unidade.
- Criar subcomando `run` novo: a medida é chamada em processo pelo portão.
- Tocar `dispatch.md`: o evento `sessionStart` já soma perto do teto, e a regra cabe no
  `orchestrator.md`, que roda em outro evento com folga.

## Critérios de Aceitação

- **AC-1** — when o repositório tem molde candidato que nenhum agente autorou, then a medida da
  lacuna conta esse molde em vez de devolver vazio.
  Command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::counts_molds_with_no_author -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-rt --lib base_gate 2>&1 | grep -q "test result: ok"`
- **AC-2** — when um subprojeto tem `## Guards` ainda no esqueleto pendente, then a medida nomeia
  esse subprojeto, mesmo com o censo recém-reminerado.
  Command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::names_a_subproject_whose_guards_are_still_a_scaffold -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-rt --lib scan_guards 2>&1 | grep -q "test result: ok"`
- **AC-3** — when não existe censo no projeto, then a lacuna volta vazia e o portão fica em
  silêncio, sem pânico e sem erro.
  Command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::no_census_means_an_empty_gap -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-rt --lib scan_patterns 2>&1 | grep -q "test result: ok"`
- **AC-4** — when a prosa semeada do roteador é comparada com o código do portão, then as duas
  metades carregam o MESMO literal de sinal, de modo que uma não pode mudar sozinha.
  Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour the_router_prose_names_the_signal_the_gate_emits -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-rt --test command_frontmatter 2>&1 | grep -q "test result: ok"`
- **AC-5** — o build do projeto passa verde
  Command: `cargo build --workspace`

## Checklist

- [x] T1 — `apps/rt/src/commands/event/enrichment_gap.rs` (novo): a medida `EnrichmentGap` e o
  repórter em stderr, reusando as duas travessias que já existem.
- [x] T2 — registrar o módulo em `event/mod.rs` e chamar o repórter no braço `BaseVerdict::Open` de
  `emit_pipeline.rs`, logo depois do refresh do censo.
- [x] T3 — corrigir o comentário de `base_gate.rs` que ainda manda o leitor à porta selada.
- [x] T4 — a linha de roteamento em `orchestrator.md`, a impressão digital superseded em
  `project_seed.rs` e a cópia entregue re-semeada.
- [x] T5 — a descrição de `plugin/commands/scan.md` passa a nomear o gatilho medido.
- [x] T6 — o teste de paridade que trava prosa e código no mesmo literal.

## Arquivos

**apps/rt** — a medida e o ponto onde ela entra.
- `apps/rt/src/commands/event/enrichment_gap.rs` (novo) — `EnrichmentGap { pending_guards, missing_molds }`, a função pura que mede e o repórter que imprime uma linha em stderr. Reusa `scan_guards::list::collect_pending` e `scan_patterns::list::collect`; nenhuma travessia nova.
- `apps/rt/src/commands/event/mod.rs` — registra o módulo.
- `apps/rt/src/commands/event/emit_pipeline.rs` — chama o repórter no braço `BaseVerdict::Open`, depois de `refresh_census_if_stale`; nada muda no `Abstain` nem no `Refuse`.
- `apps/rt/src/commands/event/base_gate.rs` — o comentário do passe completo passa a apontar para o fluxo e para este relato, não para a porta selada.
- `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs` — o teste que trava prosa e código no mesmo literal.

**packages/core** — a prosa semeada e sua entrega.
- `packages/core/templates/mustard/orchestrator.md` — a linha de roteamento: ao ler o sinal, dizer ao operador em uma frase e oferecer o fluxo como unidade própria depois que a corrente fechar.
- `packages/core/src/platform/project_seed.rs` — a impressão digital da versão superseded no catálogo, senão instalações existentes preservam prosa velha achando que é customização.

**plugin** — a descrição que o modelo lê para descobrir o fluxo.
- `plugin/commands/scan.md` — troca "weak fallback… visibly stale" pelo gatilho medido.

**raiz** — a cópia entregue.
- `.claude/mustard/orchestrator.md` — re-semeado a partir do template; o teste de deriva compara byte a byte.

## Limites

IN: a medida da lacuna do enriquecimento no portão de base, o relato em uma linha de stderr, a
regra de roteamento na prosa semeada do orquestrador com os dois acompanhamentos que os testes
cobram (impressão digital e cópia entregue), a descrição do fluxo e o comentário que ainda cita a
porta selada.

OUT: rodar o enriquecimento em si (continua sendo unidade própria, aberta por decisão); reabrir a
porta digitável; qualquer subcomando `run` novo; `dispatch.md` e o evento `sessionStart`; a linha
JSON de stdout do `emit-pipeline`, que permanece idêntica byte a byte; e a recusa por base atrasada
em relação ao remoto, que fica exatamente como está.

## Concerns

- **WARN `context-not-prose`** (validação estrutural) — a seção `## Contexto` cita o nome de um
  arquivo (`grain.model.json`) e o validador prefere que caminho de arquivo viva em
  `## Evidence` / `## Arquivos`. Mantido de propósito: ali o nome não é evidência, é o nome próprio
  da metade determinística do censo — sem ele a frase que separa as duas metades fica sem sujeito.
  Cada achado verificado deste trabalho está em `## Evidence` com seu `file:line`, que é o que o
  aviso protege.

## Definitions

- **enriquecimento** — a metade do scan escrita por agente — a prosa dos `## Guards` de cada subprojeto e os moldes `{papel}-pattern`; a outra metade e o censo deterministico (grain.model.json), que o portao de base ja remina sozinho
- **lacuna do enriquecimento** — os subprojetos cujo `## Guards` ainda e o esqueleto pendente, somados aos moldes candidatos que nenhum agente autorou
- **porta** — comando que o usuario digita; a onda 6 da spec work-unit-lives-on-its reduziu a superficie a quatro (git, pr, spec, upsert) e o scan deixou de ser uma delas

## Decisions

- a linha sai em stderr, nunca no stdout do emit-pipeline
  Reason: a unica linha JSON do emit-pipeline e comparada byte a byte por gates; o aviso de refresh do censo ja usa stderr exatamente por isso
- a medida dispara sempre que a lacuna existe, e nao apenas quando o censo foi reminerado
  Reason: Guards pendentes nascem da instalacao e sobrevivem a qualquer quantidade de censo fresco, entao amarrar o aviso ao re-minerio esconderia o caso mais comum
- a regra de roteamento vai em orchestrator.md e nao em dispatch.md
  Reason: dispatch.md roda no sessionStart, que ja soma 8072 caracteres com o censo (~950) e as advertencias dentro do teto de 10000; orchestrator.md roda no userPromptSubmit com 5927 caracteres e folga larga
- nenhum subcomando `run` novo e criado
  Reason: a medida e chamada em processo pelo portao; um subcomando exigiria os quatro registros do guard de run e faria a superficie crescer sem chamador real
- a lacuna e reportada como unidade PROPRIA, a ser despachada depois que a unidade corrente fechar
  Reason: o enriquecimento escreve arquivos versionados e exige arvore limpa, a mesma premissa que scan-clean-gate cobra
- a unidade sai de dev com tipo feature e nome gatilho-medido-enriquecimento
  Reason: escolhido pelo operador na pergunta de abertura, com o nome corrigido a mao na linha branch

## Evidence

- o portao de base remina apenas o censo deterministico e para ali; Guards pendentes e moldes faltando nao sao olhados por ninguem
  Evidence: `apps/rt/src/commands/event/base_gate.rs:217`
- o comentario do portao ainda manda o leitor a porta /scan explicita, que foi selada em 03/08/2026
  Evidence: `apps/rt/src/commands/event/base_gate.rs:213`
- o bracao BaseVerdict::Open e onde o refresh do censo roda hoje, e portanto onde a medida da lacuna entra
  Evidence: `apps/rt/src/commands/event/emit_pipeline.rs:510`
- a travessia unica que lista subprojetos com Guards em esqueleto ja existe e ja e reusada pelo doctor, entao a medida nao precisa de travessia nova
  Evidence: `apps/rt/src/commands/scan_guards/list.rs:73`
- o funil de moldes ja exclui molde presente no disco e slug declinado, entao collect() e exatamente a lista de faltantes
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:553`
- medido neste repositorio em 21/08/2026: 6 moldes sem autor (core-entry, core-outcome, dashboard-section, rt-azure, rt-branch, rt-pr) e 0 subprojetos com Guards pendentes, sem nenhum aviso em lugar nenhum
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:553`
- a superficie de quatro portas e trancada por teste, entao reabrir /scan como porta reprovaria a suite
  Evidence: `apps/rt/tests/command_frontmatter.rs:41`
- editar o seed do orquestrador exige anexar a impressao digital superseded ao catalogo, cobrado pelo teste the_fingerprint_catalog_covers_every_history
  Evidence: `packages/core/src/platform/project_seed.rs:808`
- a copia entregue .claude/mustard/orchestrator.md e comparada byte a byte com o seed, entao editar o template obriga a re-semear este repositorio
  Evidence: `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs:375`
- a onda 6 (surface-fold) decidiu literalmente tornar o scan passo automatico do portao de base em vez de porta, e automatizou apenas a metade deterministica
  Evidence: `.claude/spec/work-unit-lives-on-its/wave-6-surface-fold/spec.md:19`
