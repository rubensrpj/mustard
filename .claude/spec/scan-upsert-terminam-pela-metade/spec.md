---
id: spec.scan-upsert-terminam-pela-metade
---

# Tres pontos em que o mustard termina o trabalho pela metade sem dizer: o scan nao escreve mold algum em repositorio de manifesto unico, o upsert deixa o selo de versao nao commitado e trava o corte da branch seguinte, e a checagem de deriva compara o selo com o plugin em execucao em vez do disponivel

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Três defeitos com a mesma forma: em cada um o mustard **sabe** que uma etapa falta, não a executa e não a diz.

**1. O scan não escreve nenhum mold em repositório de manifesto único.** A resolução de dono de cluster lê `projects[]`, que é derivado dos manifestos encontrados na árvore. Num backend NestJS de campo com um único `package.json`, essa lista tem uma entrada só — a raiz — e ela é filtrada fora; os 34 clusters de papel que o minerador encontrou caem em `no_owner` e a worklist volta vazia. O mesmo modelo carrega 25 unidades arquiteturais em `skeleton[]` (`src/puzzle`, `src/mlplan`, `src/sira`, `src/core`), e a resolução de dono nunca as consulta.

**2. O instalador suja a árvore e a unidade seguinte leva a culpa.** Toda execução de `init` reescreve `mustard.json#version` — arquivo versionado — e não commita. O corte da branch da unidade seguinte reprova nessa árvore suja com um texto que atribui a escrita a *outra unidade de trabalho do operador*, quando quem escreveu foi o próprio instalador.

**3. Uma sessão rodando plugin velho parece alinhada.** O aviso de deriva compara o selo do projeto com o plugin **em execução**. Depois de instalar uma versão nova, a sessão continua com a antiga carregada até o operador ir recarregar à mão; nesse intervalo selo e plugin em execução são iguais, o aviso não dispara, e nada no produto menciona que recarregar é preciso.

**Por que agora.** Os três apareceram na mesma sessão: o primeiro num projeto de campo, e o segundo e o terceiro travando o trabalho sobre o primeiro. O padrão comum — trabalho terminado pela metade, em silêncio — é o que justifica tratá-los juntos.

## Usuários/Stakeholders

Quem instala o mustard num repositório de pacote único (defeito 1) e quem atualiza o plugin (defeitos 2 e 3). Hoje os dois descobrem a etapa que falta pela ausência do resultado, nunca por um aviso.

## Métrica de sucesso

Num repositório de manifesto único, `scan-patterns-list` deixa de devolver lista vazia. E uma instalação seguida da abertura de uma unidade de trabalho não exige nenhuma ação manual do operador entre as duas.

## Não-Objetivos

- **Estender os Guards à raiz do workspace.** A recusa ali é decisão declarada (`scan_claude.rs:555-558`): o arquivo da raiz pertence ao usuário.
- **Remover o teto de 25 entradas do `build_skeleton`.** A cobertura fica parcial em repositório com mais de 25 domínios, e isso é aceito nesta unidade.
- **Reviver a chave `subprojects` do `mustard.json`.** Está declarada e nunca lida; se for para existir, é unidade própria.
- **Reescrever apenas o texto da mensagem do portão.** A mensagem melhora como consequência de a árvore ficar limpa, não como conserto em si.
- **Mover o selo para fora do `mustard.json`.** Considerado e recusado: três leitores dependem dele (`session_start_inject.rs:411`, `statusline/segment.rs:280` e `project_overview.rs:133`, este último no crate `src-tauri`, que não compila nesta máquina).

## Critérios de Aceitação

AC = critério de aceitação: uma frase verificável por um comando.

- **AC-1** — when o modelo tem `projects[]` só com a unidade-raiz e `skeleton[]` com casas, then a resolução de dono usa as casas do esqueleto e a worklist sai com `subproject` real (`src/sira`) e `moldPath` sob `src/sira/.claude/skills/`
  Command: `cargo test -p mustard-rt skeleton_houses_own_clusters_when_no_manifest_unit_exists 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt owner_picks_longest_prefix 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — when o modelo tem ao menos uma unidade de manifesto com `dir` não-vazio, then o esqueleto não é consultado e a saída é a de hoje
  Command: `cargo test -p mustard-rt skeleton_fallback_stays_out_when_manifest_units_exist 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt a_role_spread_across_subprojects_teaches_each_of_them 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-3** — when não há unidade de manifesto E o modelo não traz `skeleton[]` (modelo antigo), then a worklist é `[]` e o comando sai 0
  Command: `cargo test -p mustard-rt no_skeleton_degrades_to_empty_worklist 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt owner_picks_longest_prefix 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-4** — when `init` roda sobre uma árvore de git limpa e o selo de versão muda, then ao fim da execução a árvore está limpa de novo, sem ação do operador
  Command: `cargo test -p mustard-cli install_leaves_the_git_tree_clean 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-cli init_seeds_harness_and_enables_plugin 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-5** — when o plugin carregado está atrás da versão registrada em `installed_plugins.json`, then o início de sessão diz em UMA linha que a sessão roda prosa antiga e que recarregar é preciso
  Command: `cargo test -p mustard-rt stale_plugin_is_announced_at_session_start 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt drift_notice_absent_when_stamp_matches 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-6** — o build do workspace passa verde
  Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

| arquivo | o que muda |
|---|---|
| `apps/rt/src/commands/scan_patterns/list.rs` | campo `skeleton` no `struct Model`; caminho alternativo na lista de donos quando ela sai vazia; testes AC-1/2/3 |
| `apps/cli/src/commands/init.rs` | após escrever o selo, deixar a árvore limpa em vez de deixá-lo solto |
| `packages/core/src/platform/project_seed.rs` | `upsert_mustard_json` — mesmo ponto de escrita do selo pelo lado do `rt` |
| `apps/rt/src/hooks/session/session_start_inject.rs` | comparar também o plugin carregado com o registrado como instalado; nova linha de aviso |
| `packages/core/src/platform/harness.rs` | resolver a versão INSTALADA (não só a carregada) |
| `packages/core/src/platform/git_branches.rs` | cascata: `current_branch`, para a linha do selo nomear onde o commit caiu |
| `packages/core/src/lib.rs` | cascata: re-exportar a API nova de `platform/` |

O mecanismo do AC-4 é: quando `init` roda numa árvore de git que ele encontrou limpa e a única mudança é o selo, ele registra essa mudança sozinho e anuncia o que fez. Numa árvore já suja ele não toca em nada e diz por quê — o trabalho do operador nunca é varrido junto.

## Limites

IN: os três defeitos descritos em `## Contexto`, nos arquivos da tabela acima.
OUT: tudo em `## Não-Objetivos`; qualquer mudança em `scan-guards-*`; qualquer mudança no crate `apps/dashboard`.

## Definitions

- **mold** — arquivo `{casa}-{papel}-pattern/SKILL.md` que ensina a convencao de um papel; carrega sozinho quando um agente edita um arquivo daquele papel. E o produto da metade escrita do scan.
- **skeleton[]** — array do `grain.model.json` com as unidades arquiteturais mineradas pela ESTRUTURA do caminho (dois primeiros segmentos), truncado nas 25 maiores. Distinto de `projects[]`, que vem dos MANIFESTOS encontrados na arvore.
- **no_owner** — motivo de descarte de `scan-patterns-list` para um cluster cujos exemplares nao caem sob nenhuma unidade conhecida: sem dono nao ha diretorio onde gravar o mold.
- **selo de versao** — a chave `version` do `mustard.json`, reescrita a cada execucao de `init` com o valor de `harness_version()` — o manifesto do plugin EM EXECUCAO.
- **deriva de versao** — o aviso de inicio de sessao que compara o selo do projeto com o harness em execucao e sugere `/mustard:upsert` quando eles diferem.

## Decisions

- Os tres defeitos vao num spec unico.
  Reason: Escolha do operador, e eles tem a mesma forma: em todos, o mustard SABE que uma etapa falta e nao a executa nem a diz — a worklist volta vazia, o selo fica solto na arvore, a sessao roda prosa velha. Tratar como tres unidades separadas esconderia o padrao.
- No scan, o caminho novo dispara SOMENTE quando `projects[]` nao oferece nenhuma unidade com `dir` nao-vazio.
  Reason: Assim o conjunto de repositorios afetados passa a ser exatamente aquele onde a saida de hoje e `[]` — nao existe comportamento anterior a regredir. Em qualquer monorepo o ramo novo nem e tomado (medido: o proprio mustard tem 14 unidades com dir nao-vazio).
- Reusar `skeleton[]` em vez de minerar uma segunda lista de casas dentro do `rt`.
  Reason: A lista ja esta no modelo, e deterministica e e a mesma que o censo de orientacao exibe. Minerar de novo criaria uma segunda verdade sobre quais sao as unidades do projeto.
- Nao criar filtro para a entrada agregada (`src`) nem regra de nome nova para a casa.
  Reason: `owner_of` ja resolve por prefixo mais longo, entao `src/puzzle` vence `src` e a agregada fica so com os arquivos soltos, morrendo em `house_below_exemplars`. E `basename("src/sira")` ja da `sira`, que e o prefixo do slug. So a entrada `(root)` precisa sair, por nao ser um diretorio.
- Guards (`scan-guards-list` / `scan-guards-apply`) ficam FORA de escopo.
  Reason: Ali a exclusao da raiz nao e descuido: e decisao declarada em `scan_claude.rs:555-558` — o arquivo da raiz pertence ao usuario. Misturar as duas transformaria um conserto de leitura de array numa discussao de propriedade de arquivo.
- Para o selo, corrigir a CAUSA (o instalador deixa arquivo versionado sujo) e nao apenas a mensagem.
  Reason: Reescrever o texto do portao para dizer `pode ter sido o instalador` continuaria exigindo que o operador limpe a arvore a mao a cada upsert. O trabalho que o produto sabe que falta e o produto que deve fazer.

## Evidence

- DEFEITO 1 — A lista de donos remove a unidade-raiz com `filter(|p| !p.dir.is_empty())`; num repositorio cujo unico manifesto esta na raiz essa lista fica VAZIA.
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:574`
- Com a lista de donos vazia, `owner_of` devolve None para todo exemplar e `group_by_project` devolve vazio, levando ao descarte `no_owner`.
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:646`
- O `struct Model` que a projecao desserializa nao declara o campo `skeleton` — por isso a lista de casas do esqueleto nunca chega ate a resolucao de dono.
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:67`
- O caminho do mold e montado como `{project_dir}/.claude/skills/{subproj}-{label}-pattern/SKILL.md`, entao uma casa `src/sira` produz `src/sira/.claude/skills/sira-service-pattern/SKILL.md` sem regra de nome nova.
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:694`
- `build_skeleton` agrupa os modulos pelos dois primeiros segmentos do caminho e TRUNCA em 25 entradas — limite conhecido da cobertura proposta.
  Evidence: `apps/scan/src/condense.rs:29`
- O monitor de lacuna de enriquecimento mede chamando os MESMOS dois coletores, entao num repositorio de manifesto unico a lacuna e considerada vazia e a linha `base-gate: enrichment stale` nunca dispara.
  Evidence: `apps/rt/src/commands/event/enrichment_gap.rs:104`
- Reproduzido em campo num backend NestJS de 1085 arquivos com um unico package.json: `scan-patterns-list` devolve `[]` e `--rejected` acusa 34 de 47 clusters em `no_owner` (Service x111 em src/sira/service, Strategy x7 em src/mlplan/strategies); o mesmo modelo carrega 25 entradas em skeleton[].
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:646`
- HIPOTESE REFUTADA: declarar `subprojects` no `mustard.json` nao contorna o defeito 1. A chave e um objeto `{exclude, include}` e esta declarada e NUNCA lida — a unica ocorrencia no workspace e a propria declaracao.
  Evidence: `packages/core/src/domain/config.rs:378`
- DEFEITO 2 — `init` reescreve `config.version` no `mustard.json` a cada execucao e nao commita; o arquivo e versionado, entao toda instalacao deixa a arvore suja.
  Evidence: `apps/cli/src/commands/init.rs:634`
- O corte da branch da unidade seguinte reprova nessa arvore suja com um texto que atribui a escrita a OUTRA unidade de trabalho do operador — atribuicao factualmente errada quando quem escreveu foi o instalador.
  Evidence: `packages/core/src/platform/i18n.rs:596`
- Reproduzido nesta sessao: apos instalar 0.1.42, `spec-draft` recusou cortar `fix/...` com `trabalho NAO commitado em: mustard.json`, e a linha suja era exatamente o selo escrito pelo proprio instalador.
  Evidence: `apps/cli/src/commands/init.rs:634`
- DEFEITO 3 — `version_drift_notice` compara o selo do projeto com `harness_version()`, que le o manifesto do plugin EM EXECUCAO. Uma sessao rodando plugin velho tem selo igual ao plugin velho e portanto NAO acusa deriva: parece alinhada.
  Evidence: `apps/rt/src/hooks/session/session_start_inject.rs:407`
- `harness_version()` resolve o manifesto do plugin carregado (`CLAUDE_PLUGIN_ROOT`), nunca o que o marketplace oferece — nao existe no codigo nenhuma comparacao entre a versao carregada e a disponivel.
  Evidence: `packages/core/src/platform/harness.rs:21`
- Nem `apps/cli/src` nem os arquivos de `plugin/commands/` mencionam recarregar o plugin: o passo que faz a versao nova valer de fato e o unico que o `upsert` nao automatiza nem anuncia.
  Evidence: `plugin/commands/upsert.md:19`