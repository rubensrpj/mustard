---
id: spec.harness-ve-toda-branch-trabalho
---

# o harness ve toda branch de trabalho: um enumerador sobre refs locais e remotas, um classificador que as cruza com a consulta de PR do provedor, e consumidores somente-leitura que nao podem apagar

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Uma unidade de trabalho (o pedaço de trabalho que nasce na base de integração, ganha
uma branch própria e deve voltar para a base) hoje **abre e não fecha**. O portão de
branch corta bem; o que não existe é o retorno. Nada detecta que o PR foi mergeado,
nada poda a branch, e — o mais grave — nada avisa que o ritual de saída está devendo.

Por que agora: em 2026-07-30 o repositório carregava **seis branches locais e seis
remotas** de unidades já mergeadas, duas delas de PRs mergeados no dia anterior. O
comando que as podaria existe, funciona, e ninguém o chamou. Não faltou comando —
faltou alguém enxergar e avisar.

Esta spec entrega **apenas a visão**: nada é apagado, nada é publicado. Ela existe
porque toda automação de poda que se construa depois precisa de uma fonte de verdade
sobre o estado de cada branch, e hoje essa fonte não existe: duas varreduras
diferentes respondem à mesma pergunta e nenhuma responde direito.

## Usuários/Stakeholders

Quem opera o Mustard no dia a dia e hoje precisa lembrar de rodar o ritual de saída
de cabeça. E quem chega numa máquina nova ou depois de apagar uma branch local, e
hoje simplesmente não consegue ver que a unidade de trabalho existe.

## Métrica de sucesso

Uma pessoa abre a sessão e lê, sem perguntar nada, quantas unidades estão devendo
poda. As sete situações possíveis de uma branch têm nome, incluindo as duas que hoje
são invisíveis: o rascunho abandonado e o PR fechado sem merge.

## Não-Objetivos

- **Apagar qualquer coisa.** Esta spec é somente leitura, e isso é garantido por
  tipo: os consumidores de relatório não recebem a capacidade de apagar. A poda é
  fase seguinte, com aprovação própria.
- **Comando novo na superfície travada.** O relatório é uma flag no `git-settle`
  que já existe. Um subcomando novo exigiria quatro registros e um chamador, e a
  decisão registrada é não crescer superfície para algo que o CLI do provedor
  quase resolve.
- **Consertar a identidade de sessão.** A constatação de que o `session_id` chuta
  o diretório mais recente por mtime está registrada em `## Evidence`, e ela
  **bloqueia a fase de automação**, não esta. Corrigi-la aqui seria widening.
- **Alterar configuração do repositório remoto.** Ligar o auto-delete de branch e
  a proteção de base é ação do dono do repositório; o harness observa e aconselha.

## Critérios de Aceitação

Cada critério nomeia seu teste e exige contagem não-zero. Um filtro que não casa nada
sai 0 dizendo `0 passed`, o que se lê como verde — então a CONTAGEM, não o código de
saída, é o que cada um afirma. O `Control:` de cada critério é um teste irmão que
passa hoje: ele prova que a expressão consegue casar algo.

- **AC-1** — when o enumerador é consultado num repositório com branches locais e branches que existem só no remoto, then ele devolve as duas famílias, filtradas por prefixo de base, e uma base sem underscore nunca entra no resultado
  Command: `cargo test -p mustard-rt branch_enumerator_sees_local_and_remote_refs` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` Expect: `[1-9][0-9]* passed`
- **AC-2** — when o ritual de saída e o inventário de specs precisam saber quais branches existem, then ambos consultam o mesmo enumerador, e nenhuma das duas varreduras anteriores sobrevive
  Command: `cargo test -p mustard-rt settle_and_active_specs_share_one_enumerator` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` Expect: `[1-9][0-9]* passed`
- **AC-3** — when uma unidade foi mergeada mas cortada in-place, sem worktree, then ela aparece na lista de pendentes de poda, que hoje responde vazio nesse caso
  Command: `cargo test -p mustard-rt in_place_merged_unit_is_reported_pending` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` Expect: `[1-9][0-9]* passed`
- **AC-4** — when o classificador recebe uma branch cuja remota desapareceu mas cujo merge não foi verificado, then ele a marca como perigo e nunca como pendente de poda
  Command: `cargo test -p mustard-rt gone_alone_never_authorises_deletion` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` Expect: `[1-9][0-9]* passed`
- **AC-5** — when o CLI do provedor está ausente ou não autenticado, then a coluna de PR responde desconhecido com o motivo, jamais sem-PR
  Command: `cargo test -p mustard-rt absent_provider_answers_unknown_never_absent` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` Expect: `[1-9][0-9]* passed`
- **AC-6** — when o módulo de relatório é compilado, then ele não alcança nenhuma função de exclusão de branch — a segurança da fase é estrutural, não disciplinar
  Command: `cargo test -p mustard-rt report_module_cannot_reach_deletion` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` Expect: `[1-9][0-9]* passed`
- **AC-7** — when uma spec existe apenas numa branch remota, then o inventário de specs a lista e nomeia onde ela vive
  Command: `cargo test -p mustard-rt active_specs_sees_a_spec_on_a_remote_only_branch` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` Expect: `[1-9][0-9]* passed`
- **AC-8** — when há unidades devendo poda, then a statusline informa a contagem, na língua configurada do projeto e sem nome de base literal no código
  Command: `cargo test -p mustard-rt statusline_names_units_awaiting_prune` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt model_segment_strips_prefixes` Expect: `[1-9][0-9]* passed`
- **AC-9** — when o ritual de saída documenta por que consulta o provedor, then a prosa não afirma um método de merge que ninguém mediu
  Command: `cargo test -p mustard-rt settle_doc_states_no_unmeasured_merge_method` Expect: `[1-9][0-9]* passed`
  Control: `cargo test -p mustard-rt base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` Expect: `[1-9][0-9]* passed`
- **AC-11** — when uma branch de trabalho nao tem nenhum commit a frente da sua base, then ela nunca e classificada como pendente de poda — cortar a branch nao e entregar trabalho, e o portao de branch corta toda unidade nova exatamente nessa forma
  Command: `cargo test -p mustard-rt a_branch_with_no_commits_ahead_is_never_awaiting_prune`
  Expect: `[1-9][0-9]* passed`
- **AC-12** — when uma unidade cujo merge foi verificado perdeu a ref local mas a remota segue viva, then ela entra na lista de pendentes de poda em vez de sair apenas como so-no-remoto
  Command: `cargo test -p mustard-rt merged_unit_alive_only_on_the_remote_is_awaiting_prune`
  Expect: `[1-9][0-9]* passed`
- **AC-10** — o build e os testes do projeto passam verdes
  Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

- `apps/rt/src/shared/branch_state.rs` — o enumerador, o classificador e a porta de consulta de PR (novo)
- `apps/rt/src/commands/git_settle.rs` — consome o enumerador; a lista de pendentes deixa de ser cega a unidade in-place; a prosa para de afirmar método de merge não medido
- `apps/rt/src/commands/git_cli.rs` — a flag de relatório no subcomando que já existe, sem superfície nova
- `apps/rt/src/commands/spec/active_specs.rs` — consome o enumerador; a coluna de localização ganha o terceiro valor, só-no-remoto
- `apps/rt/src/commands/statusline/segment.rs` — o segmento que informa a contagem pendente
- `apps/rt/src/commands/statusline/mod.rs` — liga o segmento na barra
- `apps/rt/src/hooks/session/session_start_inject.rs` — a linha de início de sessão quando houver pendência
- `packages/core/src/platform/i18n.rs` — as chaves de catálogo dos textos novos
- `plugin/commands/review.md` — remove a instrução para uma ação de merge que não existe

## Limites

IN: enumerar refs locais e remotas por prefixo de base; classificar cada branch
cruzando ancestralidade local com a consulta de PR atrás de uma porta; relatar por
repositório; informar na statusline e no início de sessão; convergir as duas
varreduras existentes numa só.

OUT: apagar branch local ou remota; mergear; abrir ou fechar PR; alterar
configuração do repositório remoto; corrigir a identidade de sessão (registrada em
`## Evidence`, bloqueia a fase seguinte); mover o corte da branch para o
`spec-draft`; transformar o acoplamento QA-integração em recusa.

## Definitions

- **unidade de trabalho** — um pedaço de trabalho que nasce na base de integração, vive numa branch {base}_{slug} e termina de volta na base — a branch é excursão temporária, nunca moradia
- **enumerador** — quem responde QUAIS refs de branch existem (locais e remotas, filtradas por prefixo de base) — não sabe nada sobre estado
- **classificador** — quem responde EM QUE ESTADO cada branch está, cruzando o enumerador com a consulta de PR — não enumera e não age
- **branch morta** — branch cujo trabalho já foi mergeado e cuja remota não existe mais; editar nela produz trabalho que não chega a lugar nenhum
- **rascunho abandonado** — branch local sem remota e sem PR — plano que nunca foi aprovado; a varredura nunca pode apagá-la porque não foi mergeada
- **poda** — apagar a branch local e a remota de uma unidade cujo merge foi VERIFICADO, depois de sair dela e avançar a base
- **gone** — marca que o git põe no upstream de uma branch local cuja remota deixou de existir; indica remota ausente, NUNCA merge

## Decisions

- um enumerador, um classificador, N consumidores
  Reason: hoje DUAS varreduras respondem à mesma pergunta e nenhuma responde direito — scan_work_branches vê só refs/heads/ e o parser de worktree é cego a unidade in-place, que é o padrão. Um terceiro varredor seria o defeito, não a solução; os dois existentes convergem para o novo
- consumidores de leitura NAO recebem a capacidade de apagar
  Reason: segregação de interface fazendo trabalho real: o relatório e a statusline não podem apagar nada porque o tipo que recebem não expõe isso. A fase de leitura fica provadamente segura por construção, não por disciplina
- o classificador depende de uma porta PrLookup, nunca do CLI do provedor
  Reason: inversão de dependência com ganho concreto: o provedor vem de mustard.json#git.provider, então um provedor novo é um adaptador novo sem tocar no classificador
- gone NAO prova merge; só a conjunção com merge verificado autoriza apagar
  Reason: uma branch apagada SEM mergear também aparece gone, então deletar com base nesse sinal apaga trabalho que nunca entrou em lugar nenhum
- a identidade da sessão deve ser RECEBIDA, nunca adivinhada
  Reason: o passo 3 do session_id escolhe o diretório de sessão mais recente por mtime — uma heurística. Com duas sessões abertas o vencedor é sorteio, e um chute não pode fundamentar uma ligação. A face run precisa receber o id explicitamente ou ler MUSTARD_SESSION_ID, que o passo 1 já consulta
- a configuração do provedor é ACONSELHADA, nunca alterada pelo harness
  Reason: apagar branch remota no merge é trabalho que a plataforma faz para todo cliente, inclusive com o harness quebrado; escrever isso em código é reinventar a roda com menos alcance. Mas um harness que muda configuração de repositório por conta própria excede seu mandato
- CLI do provedor ausente responde desconhecido, nunca sem-PR
  Reason: reportar estado não medido como negativo é exatamente a classe de defeito que o PR #130 fechou; e a varredura recusa agir sobre linha cujo estado de PR não foi medido
- nenhum nome de base, provedor, remoto ou texto de usuário literal no código
  Reason: base vem de git.integration_bases() e primary_base(), provedor de git.provider, texto do catálogo i18n::translate() — a lei de agnosticismo do projeto, e o work_branch_gate acabou de ser corrigido por violá-la
- não construir comando de listar PR, podar remota nem mergear
  Reason: o CLI do provedor e a configuração da plataforma já fazem os três com mais alcance. O que justifica código é o CRUZAMENTO local x remoto x PR e a ligação com a spec — nada mais

## Evidence

- o passo 3 do session_id devolve o diretorio de sessao mais recente por mtime, uma heuristica: rodando emit-pipeline pelo CLI nesta sessao 4e39eb9c o marcador pending-work-branch aterrissou em aee732a2, e o portao entao NEGOU a edicao por nao achar marcador. A Onda 3 removeu o balde placeholder do sorteio mas deixou o sorteio
  Evidence: `apps/rt/src/shared/context.rs:192`
- scan_work_branches enumera apenas refs/heads/, então uma spec que vive só numa branch remota é invisível ao inventário; a enumeração de refs/remotes e origin/ no arquivo retorna zero ocorrências
  Evidence: `apps/rt/src/commands/spec/active_specs.rs:429`
- o campo alsoMergeable enumera entradas de worktree, e o work_branch_gate corta in-place por padrão — então ele responde lista vazia havendo unidades mergeadas pendentes, mentindo por omissão
  Evidence: `apps/rt/src/commands/git_settle.rs:545`
- base_of_branch faz split_once underscore com propagação de None — uma base sem underscore devolve None, e há teste fixando isso; a varredura portanto nunca pode oferecer para apagar uma base de integração nem o PR de promoção base para base
  Evidence: `apps/rt/src/commands/git_settle.rs:123`
- o docstring afirma que este repo faz squash-merge e que isso quebra ancestralidade pura — medido FALSO: os três merges de 2026-07-30 têm dois pais cada e git branch --merged reconhece todas as seis branches. O fallback via provedor é certo existir, mas a justificativa escrita é afirmação não medida
  Evidence: `apps/rt/src/commands/git_settle.rs:11`
- o acoplamento QA para integração responde Warn e nunca Deny, embora leia a mesma fonte de verdade que o portão do CLOSE usa para bloquear — é a única peça destes pedidos que a plataforma não pode fazer no lugar do harness
  Evidence: `apps/rt/src/hooks/bash/pr_qa_gate.rs:68`
- toda edição em .claude/spec/ passa sem cortar branch, e o comentário declara a razão: no instante da escrita da spec não existe nome de branch para cortar, então forçar ali nega ou colide com worktree
  Evidence: `apps/rt/src/hooks/write/work_branch_gate.rs:424`
- refresh_integration_bases já roda ANTES do corte — fetch origin e depois ff-only na base corrente ou fetch base para base nas outras — de modo que a branch já nasce da base mais recente; a exigência de frescor no corte está satisfeita hoje
  Evidence: `apps/rt/src/hooks/write/work_branch_gate.rs:219`
- review_result exige spec obrigatória e escreve em review dentro do diretório da spec, então revisar o PR de um terceiro aterrissa o veredito no ledger de uma spec que não o gerou e mexe no estado do pipeline dela
  Evidence: `apps/rt/src/commands/review/review_result.rs:38`
- o slug nasce no passo 2, ou seja é o spec-draft que cria o nome da spec — logo nada antes dele pode nomear a branch base underscore slug
  Evidence: `plugin/commands/feature.md:19`
- o fluxo declara que não existe ação de merge e que PR é o único caminho de integração; o merge é feito à mão pelo CLI do provedor ou pelo portal
  Evidence: `plugin/commands/git.md:28`
- push é definido como sync-first, rebase sobre a base remota antes de empurrar, e pr chama push antes; a exigência de atualizar a branch com a base fresca na subida e no PR já está montada, só não dispara sozinha
  Evidence: `plugin/commands/git.md:21`
- a lei de operações destrutivas mantém force-with-lease PERMITIDO, o que viabiliza rebase de branch com PR aberto pela forma segura, que recusa se alguém empurrou depois
  Evidence: `plugin/pipeline-config.md:100`
- a statusline tem dez segmentos (module, git, context, duration, savings, diff, cost, model, version, mustard) e nenhum informa estado de branch ou de PR — não há onde ler que o ritual de saída está devendo
  Evidence: `apps/rt/src/commands/statusline/segment.rs:69`
- o comando de review manda o usuário rodar uma ação de merge do fluxo git quando não acha PR — ação que não existe e que o próprio fluxo declara inexistente
  Evidence: `plugin/commands/review.md:18`
- medido no repositório em 2026-07-30: delete_branch_on_merge está false e a base de integração não tem branch protection (a API responde 404), enquanto seis branches locais e seis remotas de unidades já mergeadas seguem pendentes de poda
  Evidence: `.claude/plans/mustard-git-cycle.md:24`
- medido em repo git descartável: estando na branch o delete responde erro nomeando a worktree que a usa; fora dela responde Deleted branch. Após fetch --prune o upstream vira gone. Não existe corrupção possível, existe uma ordem obrigatória
  Evidence: `.claude/plans/mustard-git-cycle.md:232`