---
id: spec.aprovacao-moldes-padrao
---

# O portao de aprovacao promete um gesto que ele nao aceita, e o validador de moldes recusa o paths que a propria instrucao manda copiar

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Duas máquinas deste projeto recusam o gesto que elas mesmas pediram.

A primeira é o portão de aprovação. Um operador dentro do branch da própria unidade digitou
`/mustard:spec`, leu o plano e respondeu "Aprovar e implementar agora" no modal que o fluxo
levantou. O `approve-spec` recusou em seguida por falta do carimbo `<spec>/.approved-by-user` —
e listou, como caminho válido, exatamente o gesto que o operador acabara de fazer. Numa sessão
sem plan mode isso deixa um plano `full` sem caminho de aprovação que funcione: sobra digitar a
forma do seletor por letra, ou relaxar a trava com `MUSTARD_APPROVAL_MODE=warn`, que é
auto-aprovação com outro nome.

A leitura do código refutou o diagnóstico óbvio. A porta do modal existe, está registrada em
`PostToolUse(AskUserQuestion)` e cunha o carimbo. O que falha é a pergunta ANTERIOR a ela — de
qual spec estamos falando. A fila que responde isso tem três degraus e fica com o primeiro que
responder qualquer coisa, sem conferir se a resposta é um plano esperando aprovação. Um palpite
obsoleto no segundo degrau sombreia o terceiro, que foi escrito exatamente para esse caso. E o
recuo acontece ANTES do aviso que o explicaria, então o operador gasta o gesto sem saber que ele
não contou, e só descobre na recusa, tarde demais.

A segunda máquina é o autor de moldes de padrão. Numa passada de campo, 19 dos 79 moldes foram
recusados na primeira tentativa, sempre pelo mesmo detalhe: o campo `paths:` escrito em uma linha
só em vez de lista. Custou três re-execuções e cerca de 110 mil tokens. A causa não foi desleixo
do agente: o worklist entregue a ele imprime o valor de `paths` juntado por vírgula numa linha só,
sob a instrução "copie ao pé da letra". O agente obedeceu; o validador recusou. Dois outros moldes
passaram com as seções fora de forma, porque o validador confere o conteúdo e nunca conferiu os
títulos — e um molde é escrito uma vez e depois carrega sozinho em toda edição da pasta, então um
defeito de forma é permanente. E o relay respondeu `ok:true, blocks:0` para um arquivo que leu e
não entendeu, o que se lê como "não havia nada a fazer".

## Usuários/Stakeholders

O operador que aprova um plano `full` numa sessão sem plan mode — hoje ele não tem caminho que
funcione, e o que existe é ou digitar um seletor que só existe por causa de uma tabela que ele não
precisava, ou desligar a trava.

Quem roda o `/scan` num repositório grande — hoje paga um quarto das recusas em re-execução por um
detalhe de forma que a própria instrução causou, e recebe "deu certo" para arquivos que a ferramenta
não conseguiu ler.

## Métrica de sucesso

Aprovação: numa sessão sem plan mode, dentro do branch da unidade, UM gesto do operador basta —
nenhum segundo gesto é pedido e `MUSTARD_APPROVAL_MODE` não é tocado. Quando ainda assim nada for
cunhado, uma linha em stderr diz qual condição falhou.

Moldes: zero recusas por forma do campo `paths:` numa passada de scan; nenhum molde gravado com os
títulos fora de forma; e nenhum relatório `ok:true` sobre um envelope que a ferramenta não conseguiu
interpretar.

## Não-Objetivos

- **Remover ou afrouxar a trava do `approve-spec`.** Ela existe por um incidente real de modelo se
  auto-aprovando. O defeito é a porta que ela anuncia estar trancada, não a tranca existir.
- **Afrouxar a regra de prompt inteiro do observador digitado.** Uma regra que casasse com trecho
  deixaria uma mensagem que apenas cita a forma cunhar o carimbo — que é a forma da falsificação que
  já aconteceu uma vez.
- **Mexer nos stems de aprovação (`approv`/`aprov`) ou na regra de opção oferecida.** São eles que
  carregam o peso de segurança do fato 2 e do fato 3; nada aqui os toca.
- **Reescrever os dois moldes já gravados fora de forma no repositório de campo.** Isso se resolve
  lá, re-executando o autor para aquelas duas pastas.
- **Escrever Guards por área naquele repositório.** É um projeto npm único sem subprojetos, então a
  metade Guards do scan não tem onde escrever. Não é falha: é o formato do repositório.
- **A contradição entre o texto do fluxo do scan ("faça commit como unidade própria") e o instalador,
  que esconde a saída via `.git/info/exclude`.** Fica registrada; é unidade própria.

## Critérios de Aceitação

- AC-1 — quando o palpite mais recente de .pipeline-states/ nomeia uma spec que NAO esta na janela full+Plan e existe um unico plano full+Plan sem aprovar, a resolucao entrega o plano pendente e a aprovacao e cunhada. Command: `cargo test -p mustard-rt a_stale_hint_never_shadows_the_pending_full_plan 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-2 — quando o operador seleciona uma opcao de aprovacao oferecida e o fato 1 e que recusa, o observador nomeia em stderr qual condicao falhou, em vez de sair calado. Command: `cargo test -p mustard-rt a_fact_one_decline_names_its_reason 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-3 — quando o prompt inteiro e /mustard:spec r e o checkout E o branch da unidade cujo plano full esta em Plan sem aprovar, o marcador .approved-by-user e cunhado; fora desse branch, nada e cunhado. Command: `cargo test -p mustard-rt a_bare_r_inside_the_units_branch_mints_the_marker 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-4 — o worklist entregue ao autor mostra paths como o bloco YAML que o molde deve carregar, e o valor copiado ao pe da letra dele passa no validador. Command: `cargo test -p mustard-rt the_worklist_prints_paths_as_the_yaml_the_mold_must_carry 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-5 — um molde que declara paths na forma inline e aceito, e o arquivo gravado carrega paths em lista em bloco. Command: `cargo test -p mustard-rt an_inline_paths_value_is_accepted_and_written_as_a_list 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-6 — um molde cujos quatro titulos faltem, dupliquem ou estejam fora de ordem e recusado sem ser escrito, e um molde com os quatro na ordem certa e aceito. Command: `cargo test -p mustard-rt a_mold_whose_headings_are_wrong_is_refused 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-7 — um arquivo de envelope que foi lido, nao e JSON e nao demarca nenhum bloco volta ok:false nomeando o arquivo, nunca ok:true blocks:0. Command: `cargo test -p mustard-rt a_read_file_that_demarcates_nothing_is_never_a_silent_ok 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-8 — resume-loop secao A deixa de prometer que a resposta ao modal cunha o marcador sem dizer o que conta, e commands/spec.md registra a forma /mustard:spec r. Command: `! grep -q 'the answer mints the same marker' plugin/refs/spec/resume-loop.md && grep -q '/mustard:spec r' plugin/commands/spec.md`
- AC-9 — a recusa do approve-spec por marcador ausente nomeia os tres gestos que cunham, inclusive a forma digitada. Command: `cargo test -p mustard-rt the_refusal_names_the_gestures_that_actually_mint 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-10 — a arvore compila inteira depois das tres ondas. Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

**Onda 1 — aprovação**

- `apps/rt/src/hooks/observe/approval_marker_observer.rs` — a fila que resolve a spec (`active_spec`) e o recuo calado
- `apps/rt/src/hooks/observe/picker_approval_observer.rs` — a forma `r` seca
- `apps/rt/src/hooks/observe/plan_approval_observer.rs` — herda a mesma fila, então herda o conserto
- `apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs` — `slug_of_work_branch`, o leitor do nome do branch
- `apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs` — a visibilidade desse leitor

**Onda 2 — moldes**

- `apps/rt/src/commands/agent/render/role.rs` — o worklist que imprime `paths` e o contrato dos quatro títulos
- `apps/rt/src/commands/scan_patterns/apply.rs` — `declared_paths`, a normalização na escrita, a checagem de títulos
- `apps/rt/src/commands/scan_patterns/relay.rs` — o relatório de lido-e-sem-blocos
- `apps/rt/src/commands/scan_patterns/mod.rs` — `read_envelope`, onde o canal de arquivo se perde em `Raw`

**Onda 3 — prosa**

- `plugin/refs/spec/resume-loop.md` — § A, o parágrafo do caminho alternativo
- `plugin/commands/spec.md` — § 1 e § 3, a forma `r` sem letra
- `apps/rt/src/commands/spec/approve_spec.rs` — a mensagem de recusa
- `apps/rt/tests/spec_flow_prose.rs` e `apps/rt/tests/approval_refusal_explains.rs` — as travas que prendem a prosa ao código

## Limites

IN: a resolução de QUAL spec as portas de aprovação decidem; o aviso quando essa resolução recusa
uma aprovação legítima; a forma `/mustard:spec r` dentro do branch da unidade; a instrução de `paths`
entregue ao autor de moldes; a tolerância e a normalização de `paths` no validador; a checagem dos
quatro títulos canônicos; o relatório do relay para o canal de arquivo inteiro; e a prosa e a mensagem
de recusa que descrevem tudo isso.

OUT: a trava do `approve-spec` em si; a exatidão de prompt inteiro no observador digitado; os stems de
aprovação e a regra de opção oferecida; a metade Guards do scan; o `.git/info/exclude` do instalador;
e os dois moldes já gravados no repositório de campo.

## Definitions

- **porta de aprovacao** — um gesto que o modelo nao consegue autorar e do qual nasce o carimbo <spec>/.approved-by-user; existem tres — aceitar o ExitPlanMode, responder o AskUserQuestion de aprovacao, e digitar /mustard:spec {letra}r como prompt inteiro
- **fato 1** — a janela de estado que os observadores de aprovacao exigem antes de decidir qualquer coisa: a spec tem scope=full, stage=Plan e ainda nao carrega nenhum pipeline.status{to:approved}
- **molde de padrao** — o arquivo {subprojeto}/.claude/skills/{slug}-pattern/SKILL.md que descreve como se escreve aquele tipo de arquivo naquela pasta, e que carrega sozinho quando alguem edita a pasta
- **envelope** — o retorno inteiro do agente de moldes, que o scan-patterns-relay fatia nos demarcadores === FILE: <caminho> === ... === END ===

## Decisions

- consertar QUAL spec a porta decide, em vez de ensinar um gesto novo a ela
  Reason: a porta do AskUserQuestion ja existe, esta registrada em PostToolUse(AskUserQuestion) e cunha o marcador; o relatorio de campo atribuiu a falha a porta e a leitura do codigo refutou isso
- a fila que resolve a spec passa a parar na primeira resposta que SATISFAZ o fato 1
  Reason: hoje ela para na primeira resposta qualquer, entao um palpite obsoleto do .pipeline-states/ sombreia o terceiro degrau, que foi escrito exatamente para o caso em que os dois primeiros nao servem
- o recuo por fato 1 passa a falar em vez de sair calado
  Reason: o aviso que explica a recusa mora depois do fato 1 e nunca e alcancado; o operador gasta o gesto sem saber que ele nao contou e so descobre na recusa do approve-spec, tarde demais
- aceitar /mustard:spec r seco como aprovacao quando o checkout E o branch da unidade
  Reason: o branch ja nomeia a spec, entao o gesto nao precisa de letra nem de tabela; a regra de prompt inteiro e mantida, porque uma regra que casasse com trecho deixaria uma mensagem que apenas cita a forma forjar o marcador
- corrigir a instrucao do worklist antes de afrouxar o validador
  Reason: o render imprime o valor de paths juntado por virgula numa linha so e manda copiar ao pe da letra, enquanto o validador so le a forma de lista — a instrucao e a causa das 19 recusas, nao o desleixo do agente
- o validador passa a aceitar as duas formas de paths: e a escrita normaliza para lista em bloco
  Reason: a checagem existe para provar o VALOR copiado do worklist, nao a forma YAML; normalizar na escrita mantem o arquivo em disco na forma canonica que a plataforma le
- a checagem de titulos exige os quatro canonicos, exatamente uma vez cada, nesta ordem
  Reason: os moldes gerados que este repositorio carrega hoje ja satisfazem essa forma, entao a versao estrita nasce verde sobre o corpus inteiro em vez de custar uma enxurrada de recusas
- o relatorio de lido-e-sem-blocos passa a valer para o canal de ARQUIVO inteiro, nao so para o envelope reconhecido como JSON
  Reason: um arquivo legivel que nao e JSON cai em Envelope::Raw e volta a imprimir ok:true blocks:0, que e exatamente o sintoma relatado; o envelope literal em --content mantem seu relatorio vazio fail-open

## Evidence

- active_spec devolve a primeira resposta nao-vazia da fila (vinculo de sessao, depois current_spec, depois unique_pending_full_plan) sem conferir se ela satisfaz o fato 1
  Evidence: `apps/rt/src/hooks/observe/approval_marker_observer.rs:103`
- current_spec devolve o .pipeline-states/*.json mais recente por mtime, de qualquer spec, sem filtro de estagio nem de escopo
  Evidence: `apps/rt/src/shared/context.rs:456`
- unique_pending_full_plan e o unico degrau da fila que aplica o fato 1, e so e consultado quando os dois anteriores devolvem nada
  Evidence: `apps/rt/src/hooks/observe/approval_marker_observer.rs:122`
- o recuo por fato 1 retorna antes de qualquer aviso: unrecognised_answer_notice so e alcancado depois que o fato 1 ja passou
  Evidence: `apps/rt/src/hooks/observe/approval_marker_observer.rs:356`
- hipotese REFUTADA — a porta do AskUserQuestion nao cunhar o marcador: o modulo existe, cunha nos fatos 1+2+3 e esta registrado em PostToolUse(AskUserQuestion)
  Evidence: `apps/rt/src/registry.rs:481`
- plan_approval_observer compartilha a mesma active_spec, entao a porta do plan mode carrega o mesmo sombreamento
  Evidence: `apps/rt/src/hooks/observe/plan_approval_observer.rs:1`
- approve_and_implement_letter exige letra seguida de r; um r seco nao e reconhecido como gesto
  Evidence: `apps/rt/src/hooks/observe/picker_approval_observer.rs:130`
- o worklist imprime paths (copy verbatim into the frontmatter) com os valores juntados por virgula numa linha so
  Evidence: `apps/rt/src/commands/agent/render/role.rs:250`
- o prompt contrata exatamente os titulos ## Purpose, ## Convention, ## How to apply e ## Examples, nessa ordem
  Evidence: `apps/rt/src/commands/agent/render/role.rs:164`
- declared_paths so reconhece a forma de lista em bloco; a forma inline devolve vetor vazio e a comparacao com o worklist recusa o molde
  Evidence: `apps/rt/src/commands/scan_patterns/apply.rs:284`
- grounding_defects confere Ref:, paths: e a linha de censo, e nao confere os quatro titulos que o proprio prompt contrata
  Evidence: `apps/rt/src/commands/scan_patterns/apply.rs:208`
- o relatorio de lido-e-sem-blocos e condicionado a from_json, entao um arquivo lido que nao e JSON escapa dele
  Evidence: `apps/rt/src/commands/scan_patterns/relay.rs:160`
- read_envelope cai em Envelope::Raw quando harness_json nao reconhece o arquivo lido, que e o caminho pelo qual o silencio sobrevive
  Evidence: `apps/rt/src/commands/scan_patterns/mod.rs:83`
- a recusa do approve-spec cita responder o AskUserQuestion como caminho valido de aprovacao
  Evidence: `apps/rt/src/commands/spec/approve_spec.rs:542`
- resume-loop §A promete que a resposta ao modal cunha o mesmo marcador (the answer mints the same marker)
  Evidence: `plugin/refs/spec/resume-loop.md:35`
