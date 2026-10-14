# Conferência do pedido do Mustard no código

Seu pedido ainda não está completamente atendido. Existem mecanismos úteis e correções implementadas, mas faltam partes do produto: um perfil persistido mais completo do projeto, uma transição explícita para preparar especializações quando necessárias, seleção de procedimentos pelo propósito da tarefa em mais situações e validação da família efetiva dos modelos. A economia total e o benefício das especializações também não foram demonstrados. Portanto, a conclusão anterior de que faltavam principalmente testes de host era insuficiente.

Conferência de 14/10/2026. Código publicado examinado: `main` em `230d55ac5a09b48a08873ed5a0b542616103e353`, versão 0.2.20. Candidato examinado: `codex/review-agnostic-orchestration` em `0edec4040a2917f79302c511164c705a8fd4a8b4`. As referências abaixo são caminhos e funções desse candidato. As correções existentes no candidato não estão publicadas em main. Esta conferência não abre PR, promove branch ou publica release.

A matriz abaixo preserva o estado encontrado antes das correções. O fechamento de implementação e seus limites estão na atualização ao final deste documento.

## Cada requisito do texto

**Implementado** identifica um comportamento encontrado no caminho de execução. **Parcial** significa que falta comportamento solicitado, além de eventual teste. **A validar no host** identifica uma dependência operacional externa ao teste local. **Sem prova de resultado** significa que configuração, existência de código e testes de contrato não demonstram o benefício pedido.

| Nº | Requisito do pedido | Situação encontrada | Código e limite concreto |
| --- | --- | --- | --- |
| 1 | Scan agnóstico ao projeto. | Implementado nos adaptadores examinados. | `apps/scan/src/extract.rs`, `apps/scan/build.rs`, `packages/core/src/domain/vocabulary/stacks.toml`. Extração por gramáticas/consultas e catálogo de ferramentas; sem roteamento pelos nomes dos projetos de exemplo nas ocorrências examinadas. |
| 2 | Identificar porte e tamanho. | Implementado como medidas; parcial como perfil de capacidade. | `apps/scan/src/model.rs::LanguageStat`, `Coverage`, `ProjectUnit`; `io/project_map.rs::CENSUS`. Grava arquivos, linhas, unidades e cobertura. Isso não é uma avaliação completa de capacidade, custo ou risco do projeto. |
| 3 | Identificar padrão de código e de convenção. | Parcial. | `domain/pattern.rs::learn` infere papéis e direções de importação. Não representa um catálogo abrangente de convenções de tipos, schemas, componentes, transações, erros e arquitetura. Exemplos atuais e regras escritas complementam esse mecanismo. |
| 4 | Identificar padrão de estrutura. | Implementado estruturalmente; parcial semanticamente. | `apps/scan/src/condense.rs::build_skeleton` e `model.rs`. Pastas, unidades, dependências e graus entram no mapa. Um papel como `L2` ou um grupo por nome não explica sozinho sua responsabilidade de negócio. |
| 5 | Identificar linguagem e suas semânticas. | Implementado para o registro suportado; parcial para comportamento. | `apps/scan/languages.toml`, consultas `.scm`, `extract.rs`, `extract/typed.rs`. Sintaxe, namespaces, membros e relações estáticas; não execução nem semântica completa das bibliotecas externas. |
| 6 | Identificar a forma como o projeto funciona. | Parcial. | `io/knowledge/relations.rs`, `navigation.rs`, `investigation.rs`, `knowledge_notes`. Há ligações e interpretações com fontes. O scan nativo não produz automaticamente uma explicação validada de todos os fluxos. |
| 7 | Registrar as identificações na base. | Implementado para o mapa; parcial para o perfil pedido. | `io/project_map.rs::CENSUS`, `FILES`, `KNOWLEDGE_NOTES`, `EXPERTISE` e `io/knowledge/catalog.rs`. Linguagens/estrutura/evidências ficam em SQLite. `domain/pattern.rs` recalcula o padrão em memória e declara que ele não é guardado separadamente; falta a projeção persistida completa de padrões e convenções. |
| 8 | Nada específico de um cliente ficar hardcoded. | Atendido nas ocorrências examinadas. | Busca por nomes e regras dos exemplos nos diretórios de produção encontrou referências em testes/comentários de regressão. Constantes genéricas de orçamento, gramáticas e contratos de ferramentas continuam necessárias; não são regras de negócio de um cliente. A busca textual não é prova universal sobre todo literal do repositório. |
| 9 | Claude/Codex usar Mustard em tudo que for possível para busca. | Parcial. | `shared/search_gateway.rs::before_bash`, `before_file`, `host.rs`, `plugin/hooks/register.js`. Há gateway, reescrita, MCP e cruzamento de leituras. Comandos desconhecidos, scripts e algumas formas de shell continuam no caminho nativo. Hooks clássicos de Grep/Glob podem exigir outra chamada. |
| 10 | Usar os comandos padrões quando o banco não resolver. | Implementado nos caminhos suportados. | `io/code_search.rs::execute_native`, `enrich`: executa a operação nativa primeiro e preserva saída/status. Índice ausente ou desatualizado não remove ocorrências. Uma busca literal vazia continua vazia; uma investigação diferente pode usar `knowledge` explicitamente. |
| 11 | Aproveitar comentários de código. | Implementado. | `apps/scan/src/extract.rs`, `io/project_map.rs` nas declarações/textos. Documentação, comentários e mensagens são extraídos com posição; comentário não é tratado como prova da execução. |
| 12 | Aproveitar Git e GitHub. | Implementado com cobertura condicionada. | `apps/scan/src/history.rs`, `shared/pr_history.rs::refresh`, `io/project_map.rs::pr_comments`. Histórico incremental e textos/comentários de PRs referenciados, conforme configuração/acesso. Não importa todo conteúdo do GitHub nem transforma revisão antiga em fato atual. |
| 13 | Aproveitar retorno e contratos das funções. | Parcial. | Extração de assinatura/contrato e relações de valores em `apps/scan/src/extract/typed.rs`, `graph/value.rs` e evidências de `domain/knowledge.rs`. Não conhece o valor retornado em execução nem efeitos dependentes de dados/ambiente apenas pelo AST. |
| 14 | Pesquisa por grafo. | Implementado estaticamente. | `apps/scan/src/graph.rs`, `io/knowledge/relations.rs` e operações Symbol/Trace. Ligações resolvidas e candidatas têm limites explícitos; relações dinâmicas permanecem desconhecidas. |
| 15 | Pesquisa vetorial. | Implementado, com uso condicionado à consulta. | `io/code_vectors.rs::fill_at`, `ranked`; `io/knowledge/catalog.rs::code_candidates`. Índice local incremental e busca por similaridade. `locate`/Read não usam esse caminho para uma leitura exata; nem toda consulta interceptada é uma investigação vetorial. |
| 16 | Modelo local pequeno para padrões/significado. | Implementado como embeddings; parcial como interpretação. | `io/code_vectors.rs` usa `model2vec_rs::StaticModel` e `potion-code-16M-v2`. É um codificador de similaridade embutido, não um gerador que compreenda e explique o projeto. Não depende de Ollama. |
| 17 | Haiku/Luna dar sentido às funções na sessão atual. | Parcial no fluxo; execução real a validar. | `io/knowledge/enrichment/session.rs::prepare_session`, `complete_session` e agente `analysis`. O binário monta o pedido e recebe a interpretação. Scan/busca não chamam automaticamente essas funções; a sessão precisa decidir e executar a delegação. |
| 18 | Última versão da família e effort médio. | Parcial. | Pedido contém política de família atual; recibo com esforço conhecido diferente de médio é recusado. `complete_session` valida o formato do nome, mas não sua família ou atualidade. Reprodução aceitou recibo declarando Opus para pedido Haiku. |
| 19 | Entrada fixa de 100 mil para Haiku/Luna. | Parcial como controle de execução. | `complete_session` recusa entrada conhecida acima de 100.000 antes de persistir. Isso ocorre depois da geração; não impede o host de consumir acima do limite. Telemetria ausente é aceita como desconhecida, não como conformidade. |
| 20 | Só analisar ponto novo/alterado e reaproveitar análises. | Implementado por identidade e validade. | `session.rs::analysis_identity`, `cached`, controle de pedido pendente; fontes e regras alteradas invalidam. Identidade não considera toda paráfrase como a mesma pergunta. |
| 21 | Jev classificar e escolher, seguindo sua documentação. | Implementado para decisões previstas; benefício sem prova. | `shared/judgement.rs::JudgementProvider/Service`, `shared/jev.rs`, perguntas Choice/Noul, confiança, abstenção, cache e recibos. Evidências nativas precedem as escolhas; Jev não produz prova de comportamento. |
| 22 | Jev ajudar na criação e orquestração das ondas. | Implementado como auxílio condicionado. | `round/backlog.rs::dispatch_backlog`, `shared/jev.rs::judge_backlog`, `round/item_choice.rs`, `shared/expertise_selection.rs`, `return_triage.rs`. Classifica tarefas/afinidade/contexto/procedimentos/relatos. Dependências, bloqueios, reservas e aprovação permanecem nativos. |
| 23 | Jev ser útil e econômico nesse processo. | Sem prova de resultado. | Ledger registra chamadas/custo/cache; não mede o trabalho que a escolha evitou nem compara a mesma entrega aceita com/sem Jev. Quantidade de chamadas não demonstra utilidade. |
| 24 | RTK reduzir consumo. | Implementado para comandos compatíveis; ganho total sem prova. | Claude: `project_seed/settings.rs::apply_rtk_hook`. Codex: `host.rs::rtk_rewrite`. `shared/rtk_gain.rs` lê histórico por projeto. Redução de saída estimada não é economia líquida do modelo. |
| 25 | Mustard ser rápido. | Parcial como resultado demonstrado. | Scan incremental, índices e cache existem. Algumas chamadas podem iniciar atualização e consultas LSP frias. Não existe SLO medido por consulta/tamanho/projeto nesta conferência, nem comparação final de latência de uma spec equivalente. |
| 26 | Ondas eficazes e de baixo input. | Parcial como resultado demonstrado. | `io/wave_prompt.rs`, `prepared.rs`, templates e `round::projection` entregam referências, contexto selecionado e saídas menores. Não demonstram que a sessão deixou de reler material ou que entregou com menos tokens/retrabalho. |
| 27 | Resumos ajudarem as próximas ondas. | Implementado. | `round/summary_wave.rs`, `backlog.rs::summary_waves`, `wave_prompt` e avisos de capacidade. Reutiliza continuidade compatível, sem tratar resumo como prova atual de acesso externo. |
| 28 | Regra de 150 mil adicionais após entrada e resumo. | Implementado no candidato e rechecado. | `hooks/session/conversation_size.rs::WaveContext`, `wave_notice_text`, `task_end_text`. Entrada de 50 mil + trabalho de 150 mil = sinal em 200 mil. Termina tarefa atual, testes mínimos e resumo; trava de nova implementação só após término aceito. Não é teto absoluto de consumo. |
| 29 | Orquestrador avisado em 200 mil e a cada 100 mil depois. | Implementado e rechecado. | Mesmo módulo: `CONDUCTOR_FIRST_NOTICE`, `CONDUCTOR_STEP`, deduplicação e reset confirmado. Uso de cobrança acumulado não é confundido com contexto atual. |
| 30 | Scan criar agentes/skills úteis por projeto e tarefa. | Parcial. | `io/expertise.rs::sync` gera catálogo de navegação; não exporta isso como especialista. `authoring.rs` converte interpretação em procedimento candidato que precisa de revisão. A redação real depende da sessão. |
| 31 | Preparar o que falta antes do despacho/quando a spec for aprovada. | Parcial. | `round/answer.rs` acrescenta `specialization_preparation` opcional e segue o despacho na mesma rodada. `workflow::dispatch_preparation` não sugere nada quando não há candidato no catálogo. Não há uma transição que complete preparação/revisão/seleção antes de despachar quando isso for necessário. |
| 32 | Reutilizar agentes/skills e atualizar só se mudarem. | Implementado para procedimentos registrados. | `expertise::active`, `review_fingerprint`, `authoring::prepare`, `workflow::restore` e ledger de exportação. Fontes/regras mudadas retiram validade; arquivos manuais são preservados. |
| 33 | Agente/skill adequado ao propósito da onda. | Parcial. | `workflow::candidates/select_many` prioriza origem e cobertura dos arquivos. `ambiguous` aciona Jev apenas entre procedimentos autorais de mesma cobertura. Um único procedimento do mesmo arquivo pode ser selecionado para uma tarefa diferente sem avaliação do propósito. |
| 34 | Especializações funcionarem no Claude e Codex. | Formatos implementados; descoberta/uso reais a validar. | `expertise/export.rs`, `platform/codex.rs`, `wave_prompt.rs`. Exporta e referencia arquivos por host, com fallback ao executor genérico. A exportação não prova que o host descobriu o agente ou aplicou a skill. |
| 35 | Integrar Codex com o mesmo cuidado. | Parcial. | MCP, normalização de comandos/patches/despacho, hooks locais e agentes existem. A configuração escreve `sandbox_mode=read-only` em um agente; a documentação Codex diz que isso não substitui permissões do pai. A restrição precisa de mecanismo suportado, além da instrução textual. |
| 36 | Mods mostrar informação relevante e legível. | Parcial como aceite do produto. | `panel-view.js`, `panel-content.js`, `panel-extras.js` mostram definições, pedidos/input, entregas/provas, consumo, configurações, ideias, perfis e impacto. Há Markdown/paginação e apresentação compacta no terminal. Não houve aceite visual no host reclamado. Chat lateral segue fora do escopo. |
| 37 | Layout ERPBrain, amarelo e sempre escuro. | Parcial. | `plugin/ui/theme.js` reproduz paleta escura com destaque amarelo e `panelPalette` usa preto no terminal. Campos `font`, `radius`, `control` não têm consumidores encontrados nos renderizadores Mods. Isso não comprova a tipografia/controle/layout completo da referência. |
| 38 | Mods abrir no Desktop Windows por SSH. | Diagnóstico implementado; correção operacional a validar. | `shared/host_installation.rs` identifica diretório/versionamento explícito do plugin; `register.js` corrige reabertura. Observar arquivo/processo e testar API com stubs não demonstra abertura no Desktop. Atualizar o CLI remoto sozinho não assegura atualização do plugin fornecido pelo cliente. |
| 39 | Painel permitir entender economia, scan e ondas. | Parcial. | `panel-economy.js` agora deixa claro “Economia total: não medida”, separa cobertura/subtotal, saída de busca, RTK e Jev. Ainda faltam resultados por tarefa aceita, utilidade dos cartões, exploração evitada e retrabalho para responder ao pedido. |
| 40 | Logs e erros compreensíveis. | Implementado para metadados observados; parcial para causas antigas. | `host_observation/failures.rs`, `panel/diagnostics.rs`, operações e recibos. Distingue recusa, proteção, erro e interrupção. Contagens antigas sem recibo não ganham causa inventada. |
| 41 | Toda instalação conter os recursos da versão. | Testes/diagnóstico existem; uniformidade operacional sem prova. | Instalador/boot e migrações têm validações. CLI, plugin fornecido por SSH e configuração do projeto são camadas distintas. O candidato ainda não passou pela distribuição/instalação real nos hosts. |
| 42 | Provar valor, precisão, economia e melhor código. | Sem prova suficiente. | Regressões, bytes entregues, recibos e testes locais cobrem mecanismos. Não existe avaliação equivalente de ponta a ponta por tarefa aceita, incluindo custo dos agentes, buscas, análises, revisão e retrabalho. |

## Lacunas de implementação que não se resolvem apenas testando o host

### Perfil persistido do projeto

O censo grava linguagem, tamanho, unidades, manifestos e estrutura. O padrão de importações é reconstruído por `domain/pattern.rs::learn` a partir do grafo. Não há projeção persistida abrangente que reúna os padrões inferidos com evidências, força, cobertura, validade e exemplos por área. Interpretações e procedimentos persistidos podem complementar o conhecimento, mas não são o mesmo contrato.

O ajuste necessário é persistir os resultados inferidos que forem úteis, com identidade da análise, fontes atuais, cobertura e distinção entre costume observado e regra obrigatória. As convenções mais ricas devem vir das regras/fontes reais e das interpretações aceitas; não de uma lista fixa de tipos de projeto. A revisão de importações já consome padrões nativos, portanto essa parte deve ser preservada, não substituída por inferência paga.

### Preparação de especialistas integrada à execução

O fluxo atual é: calcular ondas prontas, acrescentar uma sugestão opcional de preparo, montar contexto, abrir cópias, selecionar procedimentos existentes e gravar o despacho. A criação/redação/revisão de um procedimento ausente fica fora dessa sequência. Isso permite uma onda genérica válida, mas não fecha o comportamento de preparar um especialista necessário antes dela.

É preciso uma transição explícita e retomável: reutilizar conhecimento/procedimento atual; decidir se há uma lacuna que justifique especialização; preparar pela sessão apenas quando necessário; revisar; selecionar; despachar. Trabalho cuja evidência nativa basta deve continuar sem modelo. O custo e o estado dessa transição precisam aparecer no painel.

Há ainda uma lacuna específica em `workflow::dispatch_preparation`: sem candidatos existentes, ela retorna `None`. Na reprodução com um arquivo válido, `expertise --sync` gerou zero perfis. Uma área nova ou singular pode precisar de orientação mesmo sem grupo repetido. Ausência de catálogo não deve equivaler a ausência de necessidade, nem contagem de arquivos deve obrigar inferência.

### Seleção pelo propósito e não só pelos arquivos

Dois procedimentos sobre o mesmo arquivo podem ensinar coisas diferentes. O candidato preserva complementos aceitos pelo Jev, mas só compara determinados empates de cobertura. Com um único procedimento autoral aplicável ao caminho, o fluxo nativo pode escolhê-lo sem testar se ensina algo relevante para a tarefa atual.

A seleção precisa considerar tarefa, responsabilidade, evidências e procedimento, preservando a possibilidade de não selecionar. Jev pode decidir entre alternativas realmente ambíguas depois da triagem nativa. Um procedimento de banco não deve virar especialista em UI só por compartilhar arquivo. A mesma decisão deve ser reutilizada enquanto tarefa, fontes e regras não mudarem.

### Família do modelo e controle da análise

`prepare_session` pede família Haiku/Luna atual e esforço médio. `complete_session` aceita um nome de modelo sintaticamente válido, recusa esforço conhecido inadequado e recusa entrada conhecida acima de 100 mil. Não confere que um modelo informado pertença à família pedida.

A reprodução usou uma resposta artificial, com fontes válidas, para um pedido Haiku. O recibo informou `claude-opus-5-5`, e o binário aceitou. Nenhum modelo foi executado: o defeito demonstrado é a validação do contrato. É necessário rejeitar incompatibilidade conhecida de família, resolver capacidades/modelo no host e manter telemetria ausente explicitamente desconhecida.

O limite de 100 mil também é hoje verificação no recebimento, depois de o custo poder ter ocorrido. O preparo deve calcular espaço para instruções/ferramentas, dividir ou reduzir fontes quando necessário e acompanhar a execução com os recursos realmente oferecidos pelo host. Contagem de bytes do trecho não prova entrada total. Um modelo solicitado não deve ser declarado como o efetivamente usado.

### Restrição de análise no Codex

`platform/codex.rs` grava `sandbox_mode=read-only` no arquivo do agente `mustard-analysis`, e um teste confere a presença do campo. Isso não prova uma sandbox separada. A [documentação de subagentes Codex](https://learn.chatgpt.com/docs/agent-configuration/subagents) informa que agentes herdam as permissões do pai e que esse campo não as substitui.

A análise deve usar controles suportados pelo host e os hooks que identificam o papel do agente, permitindo leituras e devolução do recibo conforme o contrato. O teste precisa verificar a tentativa de escrita, não apenas o conteúdo do TOML. A instalação de hooks também depende da confiança do projeto, como descreve a [documentação de hooks](https://learn.chatgpt.com/docs/hooks).

### Utilidade e economia não têm recibo suficiente

O painel contabiliza consumo observado, saídas, recusas e chamadas dos provedores. Isso responde quanto foi registrado, mas não se o contexto entregue resolveu a dúvida, se evitou leituras posteriores ou se a onda produziu uma entrega aceita com menos exploração.

Falta associar a consulta a tarefa/onda, fontes usadas, expansões posteriores, teste/entrega aceita e retrabalho. O painel precisa mostrar consumo por entrega útil, cobertura da atribuição e evidências de uso dos contextos/skills. Comparação equivalente continua necessária para declarar economia líquida. Se não houver comparação ou cobertura, a conclusão deve permanecer inconclusiva, com o motivo concreto.

### Gateway não equivale a investigação completa em toda chamada

O contrato explícito tem intenção e finalidade. Já `Request::describe` transforma descrição livre em `locate`; apenas o prefixo estruturado `mustard:<purpose>:` muda isso. `before_bash/before_file` deixam `choose=false`. Nesses caminhos, uma busca pode passar pelo Mustard sem usar investigação vetorial ou Jev. O MCP Mods habilita escolha opcional nas investigações compatíveis, sujeito à configuração e à existência de ambiguidade.

É necessário tornar a intenção/finalidade consistentes entre os caminhos, mostrar quando uma chamada só localizou texto e quando investigou responsabilidade, e evitar uma segunda chamada só para corrigir encaminhamento. Jev e vetores não precisam participar de leituras exatas. Scripts não compreendidos devem conservar a execução nativa e sua permissão; não ser bloqueados para aparentar cobertura universal.

### Layout e abertura são entregas distintas

O layout atual melhora leitura, paginação e hierarquia, mas definir uma fonte num objeto não aplica tipografia ao Mods. O arquivo [de referência ERPBrain](https://github.com/atiz/erpbrain/blob/main/design/guidelines/README.md), lido pela API autenticada do GitHub nesta conferência, tem blob `9063bf2ad7c1edb990d9c7ef1a708569dc8554d3`. Ele descreve paleta, tipografia, controles e composição; mudar a cor de destaque cobre apenas parte disso.

Os recursos visuais devem ser adaptados aos componentes suportados pelo Mods, com contraste, conteúdo, foco, ações e legibilidade verificados nas superfícies reais. A [documentação de Mods](https://code.claude.com/docs/en/plugins/mods#where-mods-run) permite desenho no CLI e na aba Code do Desktop, e a [documentação Desktop](https://code.claude.com/docs/en/desktop#ssh-sessions) contempla plugins em SSH. O diagnóstico do plugin fornecido ajuda a encontrar instalação divergente; não é uma demonstração da abertura corrigida no cliente Windows.

## Falhas das sessões anteriores também conferidas

| Relato | Mecanismo encontrado | Conclusão limitada ao código |
| --- | --- | --- |
| `round` usado para ler disparava ondas. | `flow/cli.rs`: sem `--dispatch` ou `--report`, chama retomada e devolve `mode=status`, `mutated=false`. | Correção implementada na superfície CLI; o motor interno continua sendo uma operação de execução. |
| Rodada terminava processos e podia cortar o shell. | `flow/stuck.rs::find_stuck_processes` apenas detecta candidatos; não os encerra. | Esse caminho não envia sinal nem altera a saída do chamador. Isso não atribui causa retroativamente a todo código 144 relatado. |
| Orquestrador saía do checkout e statusline mostrava `a`. | `hooks/bash/workspace_scope.rs::isolate`; `statusline/mod.rs` identifica projeto e rotula a cópia. | Comandos reconhecidos usam subshell; estado de host já alterado precisa ser observado. O agente continua trabalhando na cópia que lhe foi reservada. |
| Agente editava cópia de outra onda. | `hooks/write/wave_copy.rs`: vínculo ao despacho, recusa de caminho errado e direcionamento de Bash. | Proteção existe nos formatos de ferramentas cobertos; não é sandbox universal de todo programa arbitrário. |
| Cópia não tinha dependências; symlink entrava no diff. | `round/dependencies.rs`, `review.rs::validate_paths`. | Preparo/recibo reconhecem layouts declarados, e `node_modules` não pode virar entregável. Instalação personalizada não precisa fingir layout Node. |
| Prisma velho era atribuído ao agente. | `round/environment.rs` reconhece geração declarada e recusa ambiente separado. | Correção genérica; nomes ambíguos não são escolhidos por adivinhação. |
| `jest --roots src <spec>` quebrava. | `review/qa_run/test_command.rs::canonical`, `workspace_scope.rs::normalize_test`. | Mantém caminho do teste como argumento separado com `--roots=<pasta>`. |
| Mesma prova vinha três vezes. | `domain/wave_prompt.rs` deduplica comandos compartilhados e preserva vínculo com critérios. | Não manda repetir a mesma prova apenas por três critérios citarem o comando. |
| Código era commitado com apenas `USAGE`. | `round/review.rs`: vista nativa, recibo e fingerprint; `commit.rs` executa/associa provas. | `USAGE` não aprova. Recibo não comprova a qualidade intelectual do revisor nem execução de testes só narrados pelo agente. |
| Título do commit expunha numeração interna. | `round/commit_title.rs::message`: lê convenção do histórico pertinente; ondas vão no trailer. | Correção implementada; comportamento genérico, sem escopo fixo de cliente. |
| Tarefa bloqueada/sem entrega voltava repetidamente. | `round/stops.rs`, `queue.rs`, `backlog.rs`: bloqueio persistido e guarda de retorno repetido. | Bloqueio explícito elimina novo despacho. Falta separar qualquer tarefa mista de forma semântica e garantir verificação de toda alegação externa. |
| Agente mudava plano sem o usuário. | `round/stops.rs`: aprovação testemunhada, sem autorização implícita por ausência de campo. | Mudança explícita segue o portão. Alteração de comportamento disfarçada em prosa/diff ainda depende da revisão pertinente. |
| Texto adicional do despacho era cortado. | `hooks/task/subagent_inject.rs::record_supplement`. | Suplemento é persistido no pedido; falha não permite descarte silencioso. |
| Comando composto com `mustard.json` cancelava teste. | `hooks/bash/reading.rs`: reescrita segura de formas suportadas, mantendo operadores. | Formas não compreendidas continuam recusadas com explicação; o Mustard não executa partes arbitrárias fora das condições do comando. |
| Erros de forma apareciam uma recusa por vez. | `spec_events/write.rs` agrega `ValidationIssues`. | Campos/forma/clareza independentes são apresentados juntos. Não implica resolver toda validação semântica do objetivo. |
| Resposta sem fato exigia versionar ponto manualmente. | `flow/answer.rs`: `--facts` entra no fechamento registrado. | Correção nativa evita essa gravação intermediária. |
| Aviso de branches e início de sessão repetiam. | `session_start_inject.rs`: fingerprint por sessão/conteúdo; housekeeping restrito às bases declaradas. | O aviso não é emitido em toda operação de uma branch de trabalho. Nova sessão ou mudança pertinente pode gerar aviso. |
| O consumo da onda não era encontrado. | `round/usage.rs`, `host_observation/transcripts.rs`, recibos de host. | Há adaptação e atribuição; transcript ausente permanece desconhecido. Não foi reconstruído consumo histórico inexistente. |
| Um fato externo antigo era gravado como atual. | Triage não marca relato como fato; `knowledge/waves.rs` grava conclusões como hipótese, com fontes atuais. | A fonte de código válida não prova bucket/acesso. Não há verificador universal das alegações externas em texto. |
| Regras de schema/compartilhamento não chegavam. | `wave_prompt` incorpora regras/skills aplicáveis e `review_rules` vincula a revisão a elas. | Entregar regra não garante cumprimento sem prova/revisão; regras de negócio devem vir do projeto, não ser hardcoded no Mustard. |

Esses mecanismos não validam todos os relatos históricos. Alguns foram explicitamente retirados como não verificados, e a auditoria não os transforma novamente em fatos. Problemas do código de uma aplicação precisam de revisão nessa aplicação quando autorizada; esta conferência não altera o backend em uso.

## Provas desta conferência

Foram relidas implementações, consumidores e condições de ativação, e não apenas nomes de módulos ou documentação. Uma reprodução isolada criou um projeto sintético de um arquivo, com IA remota e vetores desativados: scan nativo, preparação de análise e recebimento de uma resposta artificial. Observou censo presente, ausência das tabelas específicas pesquisadas para perfil/padrões/convenções, zero perfis nativos para o arquivo singular e aceitação da família incompatível no recibo. A ausência de tabelas sozinha não seria prova suficiente: a conclusão de persistência parcial também usa o modelo, o schema e `pattern::learn`.

Reexecutados **31 testes de contexto/capacidade** e **17 testes da superfície CLI**, todos aprovados. A primeira invocação direta do binário de integração deixou de definir `CARGO_MANIFEST_DIR`, que o harness usa para localizar fontes; esse teste de snapshot falhou por esse ambiente. A execução corrigida passou os 17. Nenhum código do produto foi alterado nesta conferência.

Os binários reutilizados foram produzidos na validação local do candidato e carregam carimbo `0.2.20 (build dev, ge00af9cbf58b-dirty 2026-10-13)`. Esse carimbo não comprova sozinho identidade integral com o commit candidato; a lacuna de família também foi conferida diretamente no código de `0edec404`. Recibos simulados não comprovam execução autenticada, modelo efetivo ou custo real.

O conjunto anterior de 4.154 testes Rust verifica mecanismos do produto; não muda as lacunas encontradas agora. As 25 consultas de regressão nas cópias congeladas dos projetos mantiveram paridade nativa e os resultados já conhecidos, sem melhora adicional de recuperação atribuível a esta revisão. Não há porcentagem global honesta de economia ou precisão derivável desses números.

## Critério de fechamento antes do PR

1. Fechar o perfil persistido útil do projeto e mostrar o que foi observado, inferido ou permanece desconhecido.
2. Fechar a transição de especialização necessária sem gerar agente/skill por rotina; reutilizar antes de analisar.
3. Selecionar procedimentos por tarefa/propósito e permitir abstenção, sem depender apenas do empate por cobertura.
4. Corrigir validação da família informada e a integração de capacidades do host; manter latest/medium/100 mil e telemetria desconhecida distintos.
5. Corrigir a restrição de análise Codex com controle suportado e teste de comportamento.
6. Tornar o uso de investigação versus busca literal visível e consistente nos adaptadores, preservando resultado/permissão nativos.
7. Fechar métricas úteis por consulta/tarefa/entrega, atribuição e retrabalho; não transformar compressão de saída em economia global.
8. Revisar composição e informação dos Mods pela referência visual e separar aceite de layout de diagnóstico de instalação.
9. Validar regressões de segurança/rodada e a distribuição do candidato. Desktop/SSH e uso autenticado de Haiku/Luna foram combinados para depois da release; devem permanecer marcados como testes externos a realizar, sem encobrir implementações ainda incompletas.

A [documentação Jev](https://docs.typesafe.ai/introduction) orienta perguntas atômicas com decisões compostas no código; o [exemplo de seleção de skills](https://docs.typesafe.ai/cookbooks/skill_suggestion) acrescenta adequação e possibilidade de não escolher. Isso sustenta o uso de Jev para escolhas pequenas após triagem, não para inventar dependências ou aprovar código. As [skills Claude](https://code.claude.com/docs/en/skills) e [subagentes Claude](https://code.claude.com/docs/en/sub-agents) exigem atenção à descoberta e ao custo do conteúdo carregado; agente novo não é sinônimo de contexto menor. A avaliação econômica precisa incluir esse custo.


## Correções realizadas após a conferência

O trabalho permanece no candidato `codex/review-agnostic-orchestration`. Main, dev, a release pública e a instalação do backend em uso não foram alterados nesta correção.

| Lacuna | Correção concreta | Verificação |
| --- | --- | --- |
| Perfil não persistido | `project_profile` guarda porte, linguagens, estrutura, papéis, importações, nomes, tipos de declaração e exemplos de assinaturas. Atualiza junto do scan, sem inferência. `knowledge --profile --file` faz a leitura por área; as ondas recebem uma referência curta. | Persistência, reutilização, alteração de fonte, schema antigo e leitura sem migração. Perfis gerados nas três cópias de teste. |
| Seleção por arquivo | A finalidade usa título, texto e trabalho atribuído à tarefa. Correspondência de caminho não basta. A seleção nativa abstém quando não encontra evidência de adequação; Jev pode reavaliar também um candidato único incerto, com Choice, Noul e opção de não escolher. | Procedimento inadequado não carregado; escolha complementar revisada preservada; mudanças de tarefa, fonte e regras invalidam a seleção. |
| Preparo desconectado | Rascunho pertinente pendente de revisão segura somente sua onda antes de criar cópia, selecionar contexto pago ou gravar envio. A decisão de usar evidência nativa pode ser registrada com `expertise --native`, vinculada a tarefa/fontes/regras. Área nova sem catálogo também pode preparar um procedimento, sem obrigar geração. | Rodada sem envio enquanto há rascunho pertinente; liberação após escolha nativa; alteração de regras invalida essa escolha. |
| Família/modelo | Recibo com outra família é recusado. Claude utiliza o alias nativo Haiku; Codex resolve a Luna mais recente listada com medium no catálogo nativo fresco, quando o formato é reconhecido. Modelo efetivo Codex conhecido deve corresponder à resolução preparada. Catálogo ausente/desatualizado exige evidência nativa. | Família errada recusada antes da persistência/cache; catálogo com versões novas, modelos ocultos, esforço incompatível e data vencida. |
| Análise sem controle real | Removido `sandbox_mode` ignorado do arquivo Codex. Hooks restringem ferramentas do papel de análise; Mods recusam fork e shell/escrita/delegação nesse papel e pedem Haiku/medium. Entrada observada perto de 90 mil reserva margem para concluir antes do alvo fixo de 100 mil. | Leitura permitida; escrita, redirecionamentos, comandos encadeados, expansão de shell e requests de execução recusados; vínculo do filho não afeta o pai. |
| Busca/decisão inconsistente | Leitura exata e busca literal não constroem seletor Jev. Argumentos, resultado e código de saída nativos permanecem preservados; investigação declara intent/purpose. | 25 consultas das cópias preservaram integralmente resultado nativo e status; 10 símbolos rotulados continuaram visíveis. |
| Medição insuficiente | Painel mostra ondas aceitas pelo commit, tentativas, repetições, retornos sem arquivos, critérios com execução nativa e tokens por onda aceita incluindo suas tentativas. Consumo ausente mantém custo unitário desconhecido. Recibo de Read só confirma corpo completo da versão da skill enviada àquela tentativa. | Atualização de consumo não cria tentativa nova; histórico desconhecido não vira custo zero; leitura parcial/outro corpo/outra tentativa não confirma skill. |
| Mods confusos | Visão de consumo começa por entrega, retrabalho e cobertura; scan mostra retorno nativo versus enriquecimento e tempo observado; análise reutilizada fica separada de cobrança Jev/RTK. Perfil observado tem expansão própria. Tema continua escuro/amarelo; usa controles suportados. | Testes do toolkit oficial; pintura Desktop/SSH continua no aceite de host após a release, conforme combinado. |

### Limites que continuam explícitos

- O perfil é uma observação estática com cobertura e fontes. Frequência de interface, tipo, importação ou nome não vira regra obrigatória. Não existe prova automática de todo comportamento dinâmico do projeto.
- Haiku/Luna interpretam uma pergunta delimitada, quando necessário, usando a sessão nativa. O binário não abre outro processo de geração nem usa uma chave de API para isso. Catálogo interno Codex é um adaptador de melhor esforço; formato novo ou catálogo vencido não autoriza substituição por outra família.
- A proteção de 100 mil usa isolamento, pacote pequeno, margem quando existe telemetria e rejeição de recibo acima do alvo. A API de Mods não informa no início de todo passo o total exato de entrada do filho. Entrada inicial opaca não é uma garantia medida de conformidade. O teste autenticado de host continua necessário.
- O sinal da onda continua sendo **entrada inicial + resumo herdado + 150 mil de trabalho**. Com 50 mil iniciais, o sinal ocorre a partir de 200 mil. Termina a tarefa atual, testes mínimos e resumo; depois retorna sem iniciar outra tarefa. Não é teto rígido de 150 mil para a conversa inteira.
- Leitura confirmada significa que o host entregou o corpo daquela versão. Não prova entendimento nem cumprimento. Carregamento automático de skill sem recibo observável permanece desconhecido.
- Os 25 casos e 10 alvos são conhecidos e congelados. Confirmam regressão nessa amostra, não precisão geral, perfeição de código ou economia líquida. A cópia frontend Sialia exclui metadados de um submódulo vazio somente no teste; o backend C# é uma cópia separada. O scan real continua recusando submódulo declarado vazio.
- UI nativa Mods não oferece as mesmas propriedades tipográficas de uma página HTML. A referência visual orienta hierarquia, contraste e cor; não se promete suporte a CSS inexistente. Desktop/SSH e uso autenticado dos modelos serão testados depois da release, por decisão já registrada do usuário.

A evidência final de testes, fontes e resultados ficará em `docs/evidence/2026-10-14-correcao-lacunas.json`. Economia total permanece não comprovada até comparar tarefas equivalentes, com qualidade aceita, nos dois fluxos.

Verificação final: 4167 testes Rust aprovados, 2 ignorados já existentes, zero falhas; Clippy sem avisos; 63 testes Mods em cada CLI 2.1.296 e 2.1.299. Validação do manifesto do plugin aprovada. Nenhuma geração autenticada de modelo nem pintura Desktop foi executada nesses testes.
