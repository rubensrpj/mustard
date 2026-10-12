# Pedidos do usuário e situação das entregas

Conferência em 12/10/2026. Esta lista consolida pedidos repetidos e considera as decisões mais recentes do usuário. Não substitui autorização nem afirma que uma instalação em uso recebeu uma correção apenas porque o código foi integrado.

## Como ler

- **Feito:** implementação identificada no código e coberta pelas verificações disponíveis; não significa validação de uma sessão real.
- **Parcial:** existe implementação, mas falta parte do comportamento pedido.
- **A validar:** implementação existente cujo resultado no ambiente real ainda não foi comprovado.
- **Pendente:** entrega ou diagnóstico ainda não realizado/comprovado.
- **Retirado:** saiu do escopo por decisão do usuário.

Base publicada: **v0.2.16**, PRs #314 (dev) e #315 (main), com release e pacotes Linux/macOS/Windows aprovados. Esta revisão acrescenta a etapa `codex/finish-product-gaps`; a evidência e a promoção desta etapa ficam em [validação final](2026-10-12-validacao-final.md). O usuário removeu o Mustard e as specs do backend e instalará a nova release: nenhum desses arquivos foi restaurado e nenhuma instalação foi substituída.

O documento original de auditoria local foi preservado. Esta cópia versionada registra as diferenças verificáveis da etapa e mantém as limitações operacionais. A evidência da etapa atual é mantida na validação final, com CI/release separados de aceites do host.

## 1. Produto, arquitetura e instruções

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 1 | Conduzir criação de spec, separar em ondas e orquestrar a execução. | **Feito:** levantamento, resposta, plano, aprovação, despacho, rodada, revisão e fechamento possuem contratos nativos. Especialização, reutilização e seus aceites operacionais estão discriminados abaixo. |
| 2 | Delegar ao binário operações mecânicas, usando o LLM para raciocínio e implementação. | **Feito:** estado, referências, busca, comandos, recibos, projeções, arquivos e publicação têm implementação nativa. Não foi feita uma prova universal de que nenhum fluxo do host ainda peça um script desnecessário. |
| 3 | Usar interface de julgamento para poder trocar Jev por outro provedor no futuro. | **Feito:** portas tipadas de seleção/julgamento e alternativas nativas. Um futuro provedor precisa de adaptador; não foi integrado Laya/Layla. |
| 4 | Retirar Laya da implementação atual. | **Retirado:** o provedor remoto adotado é Jev; enriquecimento pela sessão é separado. Referências históricas não significam provedor ativo. |
| 5 | Retirar Cloud Sessions como executor remoto das ondas. | **Retirado:** não integra o desenho entregue nesta etapa. |
| 6 | Otimizar os `.md`, carregar instruções conforme a necessidade e preservar padrões do projeto. | **Feito:** mapa curto, referências sob demanda, templates pt-BR/en-US e orçamento de prosa verificado. **A validar:** economia e cumprimento efetivo numa sessão de implementação. |
| 7 | Retirar mecanismos antigos sem uso. | **Parcial:** publicação/sincronização automática antiga foi removida; compatibilidade de eventos/configurações históricos foi mantida. Não foi demonstrada uma auditoria universal de todo código sem uso. |

## 2. Erros de comandos, identidade e hooks

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 8 | Devolver todas as falhas de forma do item numa única recusa. | **Feito:** validações agregadas, com regressões de campos, forma e clareza. |
| 9 | Aceitar fatos no próprio `answer`, evitando regravar manualmente o ponto. | **Feito:** `answer --facts`; resposta e fechamento atômicos. |
| 10 | Não repetir revisão de bloco que não mudou nem perguntar de novo o que já foi registrado. | **Feito:** revisão por mudança e regressão dos blocos unitários. Não há garantia semântica de reconhecer qualquer paráfrase de uma resposta anterior. |
| 11 | Corrigir a resposta MCP que recusava seu próprio formato. | **Feito:** resposta textual compatível; contratos de Grep/Read/busca testados. Codex recebe o mesmo contrato textual. |
| 12 | Dar erro claro e JSON para opção duplicada ou argumento inválido. | **Feito:** recusas específicas e testes da superfície CLI. |
| 13 | Ler qualquer item pelo código e uniformizar referências, `replaces`, ponto e dependências. | **Feito:** códigos estáveis resolvem versões atuais; números continuam identificando versões. Nome informal de tarefa não é automaticamente um código válido. |
| 14 | Fechar um ponto sem parecer criar outro ponto. | **Feito:** identidade lógica preservada no histórico versionado. |
| 15 | Proteger a chave sem cancelar desnecessariamente o restante de um comando encadeado. | **Feito nos formatos suportados:** mascaramento e reescrita preservam pipes/sufixos e execução única. Comando complexo que não pode ser reescrito com segurança pode ser recusado com explicação; não há execução arbitrária de pedaços. |
| 16 | Encaminhar Read/Grep e buscas ao Mustard sem atrito e com alternativa funcional. | **Parcial:** gateway, MCP, CLI e hooks existem. Hooks clássicos não substituem qualquer ferramenta/programa possível; alguns encaminhamentos exigem nova chamada do host. O caso real Desktop/SSH ainda precisa ser repetido. |

## 3. Segurança e funcionamento das ondas

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 17 | Não disparar novamente tarefa já impedida por acesso, dados ou decisão. | **Feito:** `blocked_by` persistente exclui a tarefa da seleção nativa e do quadro do Jev. Reescrita é necessária para limpar o impedimento; aceite de plano sozinho não limpa. |
| 18 | Bloquear dependentes, permitindo que trabalho independente continue. | **Feito:** dependências impedidas ficam fora do despacho; ondas independentes continuam. |
| 19 | Não devolver ao backlog uma tarefa misturada com sobras que dependem do usuário. | **Feito no controle:** impedimento e orientação de reescrita; retorno repetido sem alteração e sem mudança do pedido vira impedimento persistente. **Limite deliberado:** não se reescreve uma tarefa mista sem revisão do escopo pelo condutor. |
| 20 | Exigir leitura/revisão do diff de cada onda antes de comitar. | **Feito:** `review-wave`, recibo e `REVIEWED` vinculados ao fingerprint. `USAGE` não aprova código. Código, entrega, regras e base pertinentes mudados invalidam a revisão. Isso comprova o contrato, não a qualidade intelectual da leitura. |
| 21 | Impedir mudança de plano sem aceite do usuário, mesmo sem `changes_decision`. | **Feito:** proposta de replanejamento espera testemunha de aceite; ausência do campo não concede autorização. |
| 22 | Não gravar como atual um fato externo copiado de um relatório antigo. | **Feito na distinção:** instruções exigem fonte/momento, painel mostra fatos relatados, e classificação do Jev registra `verified_fact:false`. **A validar:** comprovação externa depende da ferramenta e do acesso atual; nenhum classificador pode certificar uma afirmação arbitrária em prosa. |
| 23 | Gravar provas reais e distinguir execução de relato do agente. | **Feito:** recibos `stage_run/criterion-proof`, associação a critérios/ondas e exibição no painel. `proofs` da entrega continua listando comandos adicionais; não se inventa recibo antigo ausente. |
| 24 | Aceitar provas com `taskset` e preservar as restrições de execução. | **Feito:** reconhecimento do comando e regressões; instruções persistidas chegam ao despacho/conserto. Não foi imposto `taskset -c 0-3` globalmente a todos os projetos. |
| 25 | Preservar o texto adicional enviado ao agente e avisar sobre a montagem canônica. | **Feito:** suplemento persistido antes do despacho, inclusive reenvio/conserto; falha de gravação impede perda silenciosa. Suplemento não amplia autorização. |
| 26 | Entregar regras e skills de área, incluindo schemas/zod e compartilhamento PI/PCP. | **Feito no Mustard:** seleção por caminhos e regras aplicáveis no pedido/revisão. **Pendente no backend:** corrigir as duplicações/migração concretas; não alterei o código do projeto em uso nesta etapa. |
| 27 | Uniformizar autorização para baixar dados e limpeza de arquivos temporários. | **Parcial:** orientação comum aos agentes respeitando autorização e leitura mínima. Download/limpeza reais precisam de verificação/recibo; não foram executados no backend nesta auditoria. |
| 28 | Avisar o orquestrador em 200 mil tokens, depois a cada 100 mil. | **Feito:** faixas persistidas, sem confundir com a regra dos agentes. |
| 29 | Aos 150 mil, agente não inicia outra tarefa, mas termina a atual, teste mínimo e resumo. | **Feito:** horizonte de capacidade com encerramento da tarefa, não interrupção no meio; conserto mantém suas regras próprias. |
| 30 | Corrigir retomada com número de onda que diverge da rodada. | **Feito:** retomada usa a projeção vigente e ignora ondas vazias; regressão específica. |
| 31 | Detectar problema de preparação/Prisma sem mandar conserto indevido ao agente. | **Feito:** preparação declarada e recusa de ambiente distinta de defeito da onda. Não identifica automaticamente qualquer problema possível de dependência. |
| 32 | Não repetir avisos inalterados de pendências e branches. | **Feito:** deduplicação por sessão e conteúdo/identidade; mudança relevante ou sessão nova volta a avisar. |
| 33 | Reduzir relatórios enormes e mostrar resultado, impedimento e próximo passo. | **Feito no transporte:** JSON compacto preserva todos os campos; medição registra os bytes realmente emitidos. Mapa e templates respeitam o orçamento anterior de 25.600 bytes por idioma. Relatórios específicos continuam expansíveis. Economia total da sessão requer A/B. |
| 34 | Diagnosticar troca de sessão, sumiço do rascunho e caminhos de agentes alterados. | **Feito no diagnóstico disponível:** hooks registram host, transição, identidade da sessão e mudanças da pasta por hash. O caminho real do transcript do subagente é ligado ao envio exato. A causa histórica do rascunho apagado não pode ser reconstruída sem os arquivos antigos. |

## 4. Scan, busca, banco e pesquisa aplicada

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 35 | Fazer do Mustard um gateway: ferramenta nativa primeiro, cruzamento com banco depois, Jev se necessário. | **Feito nos caminhos suportados:** recuperação atual, cruzamento com índice, seleção opcional e alternativa nativa. Não existe interceptação universal de qualquer programa de shell. |
| 36 | Padronizar pedido com ferramenta, argumentos, intenção e finalidade. | **Feito:** `schema_version:1`, `request:{tool,input,intent,purpose,choose?}`, compartilhado pelos adaptadores. |
| 37 | Retornar arquivo, símbolo e linhas úteis para leitura seletiva. | **Feito:** cartões, fontes, intervalos e expansão localizada; fonte conferida para não entregar intervalo obsoleto como atual. |
| 38 | Se nada útil for encontrado, devolver o resultado nativo preservando flags, ordem e status. | **Feito nos formatos suportados:** fallback e modos `locate`/raw. Operações complexas não são reinterpretadas silenciosamente como comandos simples. |
| 39 | Alimentar o banco com descobertas novas e invalidar conteúdo alterado. | **Feito:** observações deduplicadas, atualização incremental e notas/interpretações vinculadas às fontes. |
| 40 | Indexar funções, assinaturas, comentários, documentos, configurações, git e relações. | **Feito:** evidências estruturais e textuais, histórico e ligações candidatas. Valores de retorno em execução e intenção de negócio não são fatos dedutíveis apenas do índice estático. |
| 41 | Priorizar entrada do fluxo, mostrar chamadores/chamados e indicar confiança. | **Feito nos contratos e casos verificados:** cartões e relatório mostram chamadores/chamadas, resoluções e intervalos atuais. Retirada apenas de relação duplicada, sem perder referência. Gancho `onCompleted` incluído no teste Florestal. Isso não garante primeiro resultado correto para qualquer pergunta. |
| 42 | Ser agnóstico, incluindo TypeScript, C#, React e demais linguagens suportadas. | **Feito estruturalmente:** extratores/AST e recuperação comum. Resolução completa por compilador, reflexão/runtime e semântica de toda biblioteca não estão implementadas. |
| 43 | Usar pesquisa lexical, grafo, semântica e vetores; comparar o armazenamento. | **Feito:** SQLite/FTS, relações, atualização incremental e vetores locais int8. Foi mantido SQLite; não foi incorporado um servidor vetorial/ANN sem ganho demonstrado. Importação SCIP é opcional e requer índice externo válido. |
| 44 | Usar modelo local pequeno e descartar Ollama. | **Feito:** embeddings Potion Code 16M v2, cerca de 33,5 MB, habilitados por padrão em busca por intenção salvo opt-out. Não é um gerador de explicações nem exige Ollama/serviço residente. |
| 45 | Gerar `.md` do projeto com referências como o levantamento Puzzle. | **Feito para documento nativo referenciado:** `knowledge --topics auto --markdown` compõe áreas, símbolos, assinaturas, relações, fontes/hashes, interpretações atuais e lacunas, sem repetir o inventário por assunto. Não inventa narrativa de negócio onde não houve análise; esta usa o contrato da sessão nativa. |
| 46 | Aplicar os conceitos das pesquisas e retirar o que piorar. | **Feito de forma seletiva:** conceitos de navegação, memória, grafo, fontes/invalidação, SQLite/FTS e avaliação; experimentos prejudiciais foram retirados. Não foram copiados todos os recursos dos projetos pesquisados. Os pesos do modelo local foram incorporados com proveniência. |
| 47 | Testar Florestal, Sialia e baterias externas, com números de acerto, texto e tempo. | **Feito:** nova comparação com a v0.2.16 publicada em 25 solicitações, três bases isoladas (Florestal, Sialia React e backend C#). Paridade nativa, alvos conhecidos visíveis, controles e tamanho do retorno registrados na validação final. Nenhum modelo do host ou código de aplicação executado. |
| 48 | Chegar a acerto e economia próximos do ideal, sem esconder regressões. | **Pendente como aceite final:** não existe prova de busca perfeita, cobertura total ou economia líquida de uma implementação. A medida de bytes não é economia de assinatura. |

## 5. Jev e controle de gasto

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 49 | Usar Jev para classificar/escolher, seguindo seus primitivos e perguntas atômicas. | **Feito:** portas tipadas, seleção de alternativas e julgamentos de planejamento/contexto; operação nativa antes do provedor. Não gera prosa, não substitui aprovação nem certifica testes. |
| 50 | Ser criterioso, reaproveitar decisões e não chamar IA para o que o binário resolve. | **Feito:** habilitação por finalidade, cache por evidência/rubrica, portas nativas e registros de chamadas/tentativas. **A validar:** benefício por função em trabalho real. |
| 51 | Retirar o limite mensal interno; sem crédito, seguir sem Jev. | **Feito:** teto removido, 401/402 usam alternativa nativa, sem repetição da mesma tentativa; decisão posterior pode recuperar após recarga. Saldo não é inferido do histórico local. |
| 52 | Aproveitar Jev para agentes, skills, agrupamento e devoluções das ondas. | **Feito no código desta etapa:** seleção específica entre perfis autorais atuais e revisados; perfis idênticos dispensam chamada. Devoluções ambíguas têm triagem limitada, sem aprovar código/fatos. **A validar:** benefício líquido por função em execução real. |
| 53 | Habilitar Jev e todos os recursos de forma consistente nos cinco projetos. | **Feito no suporte de instalação/configuração:** credencial unificada, diagnóstico por projeto, instalação/migração idempotentes, modelos/Jev/RTK configuráveis. O diagnóstico anterior dos cinco permanece histórico; o backend foi removido pelo usuário. Nova instalação e ativação serão feitas por ele, sem recuperar specs. |
| 54 | Demonstrar se Jev de fato economiza e melhora a entrega. | **Pendente:** pilotos pequenos mediram escolhas/custo estimado, mas não há comparação equivalente de ponta a ponta por tarefa aceita e retrabalho. |

## 6. Agentes, skills e Haiku/Luna

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 55 | Reconhecer áreas pelo scan e manter catálogo reutilizável de agentes/skills. | **Feito:** catálogo comum, perfis nativos de navegação, fontes, validade e revisão dos procedimentos autorais. |
| 56 | Reutilizar antes de criar e só atualizar se houver mudança pertinente. | **Feito:** hashes, escopo, regras e seleção; conteúdo manual preservado; export gerenciado obsoleto só é retirado se continuar idêntico ao conteúdo do Mustard. |
| 57 | Criar agentes/skills específicos de banco, API, React e design a partir do projeto e da tarefa. | **Feito no fluxo e instalação:** catálogo por tarefa/fontes, preparo nativo, rascunho revisado por fingerprint e export Claude/Codex. Inclui o agente `mustard-analysis` nos dois hosts. Conhecimento de negócio é escrito pela sessão quando necessário, sem outro CLI/API. Execução do modelo requer observação real do host. |
| 58 | Preparar/criar o que falta após aprovar spec ou antes do despacho; usar também na formação das ondas. | **Feito no código desta etapa:** rodada seleciona/exporta perfis pertinentes e fornece preparo opcional de lacunas. Quantidade de arquivos não obriga análise nem bloqueia a onda. Reutilização precede nova delegação; agente só é usado se descoberto pelo host. **A validar:** decisão efetiva do condutor e descoberta na sessão. |
| 59 | Aplicar boas práticas de skill/agent creator de Claude e Codex. | **Feito no desenho e contratos:** escopo, fontes, instruções sob demanda, preservação manual e formatos por host. Criadores públicos não foram incorporados como dependência executável; falta avaliação real de especialização. |
| 60 | Enriquecer o scan com Haiku no Claude e Luna no Codex pela sessão existente. | **Feito no fluxo e agente nativo:** `mustard-analysis` recebe apenas o pacote preparado, Haiku/Luna, medium e entrada total de 100 mil tokens. Reutiliza análise atual e retorna recibo; não abre outro processo de modelo. Não foi lançado um turno real desses modelos nesta validação. |
| 61 | Usar sempre a versão mais recente disponível da família e effort médio. | **Feito como política e agente instalado; a validar na execução:** contrato exige resolução pela sessão e separa modelo/effort efetivos quando informados. Configuração ou alias não provam que o host obedeceu. |
| 62 | Fixar 100 mil tokens de entrada para Haiku e Luna. | **Feito como contrato de entrada:** total inclui instruções, ferramentas e cache; contexto isolado e sem transcript do pai. Não há controle nativo da janela interna do host; excesso conhecido é marcado e o resultado válido é conservado. |
| 63 | Só analisar ponto novo ou alterado; reutilizar entre specs/hosts e aproveitar o que a onda já entendeu. | **Feito:** identidade inclui pergunta/instruções, todas as fontes e regras; troca de host/modelo não invalida análise válida. Conclusão da onda pode ser registrada na entrega. Paráfrases não são automaticamente equivalentes por hash. |
| 64 | Medir se especializar ondas melhora primeira entrega, padrões, custo e retrabalho. | **Pendente:** falta comparação controlada entre executor genérico, genérico com skills e especializado. Testes de contrato não demonstram esse ganho. |

## 7. Mods, statusline e configurações

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 65 | Mostrar projeto, specs, definições, texto/input da onda, entregas, testes e execução. | **Feito no código:** navegação e leitor por seções, input integral, definições e recibos. **A validar:** experiência real nas telas reclamadas. |
| 66 | Usar padrão visual ERPBrain, trocando roxo por amarelo; todas as páginas escuras. | **Feito no código:** tema compartilhado escuro/amarelo e correção de paleta do terminal. **A validar:** aderência visual e usabilidade no Desktop Windows/SSH; não houve aceite visual do usuário. |
| 67 | Atualizar painel em tempo real sem gerar custo de IA. | **Feito:** eventos e atualização local/polling; consulta do painel não lança scan, Jev ou turno do modelo. |
| 68 | Statusline compacta, com estado útil e acesso ao painel. | **Feito no código:** detalhes ficam no painel. **A validar:** apresentação junto ao host real, largura e clique/abertura. |
| 69 | Criar aba de configurações para modelo, Jev, vetores, RTK e atributos. | **Feito:** configuração validada, revisão/conflito de edição e proteção da chave. Preferências Haiku/Luna incluídas. |
| 70 | Abrir Mods no Desktop Windows conectado por SSH, e não somente no CLI. | **Feito no ciclo de vida testável:** reabertura após perda de referência e reattach, respeitando o fechamento pelo usuário; diagnóstico de attach/detach/abertura/render/falha. Ainda depende de reprodução no Desktop Windows/SSH para confirmar pintura real. |
| 71 | Mostrar no Mods o uso de Jev, modelos, perfis, reutilização, falhas e economia. | **Feito na projeção:** Jev/RTK, análises, perfis e reaproveitamento; agente/perfis/skills solicitados e leitura confirmada ou desconhecida, conforme recibo. Diagnóstico do host separado. Benefício global não é inferido de presença do recurso. |

## 8. Consumo e diagnóstico

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 72 | Logar chamadas, recusas, erros, tokens e consumo por spec/onda/projeto. | **Feito:** operações, recibos, categorias de tokens, custo estimado e uso desconhecido explícito; campos de análises/busca aceitos pelo schema. **A validar:** atribuição efetiva em todos os projetos. |
| 73 | Corrigir agente sem conversa encontrada e consumo da onda ausente. | **Feito para transcripts fornecidos pelo host:** `agent_transcript_path`, prompt canônico, timestamp e envio exato; leitura fora do caminho convencional, Claude/Codex, reenvios separados e custo de ondas retiradas do plano. Fonte ausente/formato desconhecido continuam desconhecidos; não se inventa histórico apagado. |
| 74 | Garantir RTK funcionando e informar o que ele economizou. | **Parcial:** configuração/hook e métricas de compressão disponíveis. Ativação real depende da instalação e do comando; compressão de saída não demonstra economia total do modelo. |
| 75 | Dizer claramente se o Mustard está econômico. | **Pendente como conclusão geral:** falta A/B equivalente incluindo preparação, busca, Jev, subagentes, implementação, revisão e retrabalho. Não há percentual global honesto de economia para a versão atual. |

## 9. Publicação externa

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 76 | Remover publicação automática; publicar somente mediante pedido explícito. | **Feito:** painel local independente; retomadas/rodadas não publicam páginas. |
| 77 | Publicar projeto, spec específica, análises e resumo para gestores pelo mesmo padrão. | **Feito para exportação dos conteúdos disponíveis:** HTML/JSON/manifesto nativos com layout compartilhado e lista permitida. A redação de uma análise gerencial nova continua dependendo do raciocínio da sessão. |
| 78 | Cloudflare configurado no `mustard.json`, envio pelo binário e ação no painel. | **Feito no código:** snapshot explícito e transporte Direct Upload. **A validar:** publicação autenticada na conta/destino do usuário; não foi feita publicação externa nesta auditoria. |
| 79 | Não gerar banco/página externos continuamente e não vazar chave/dados internos. | **Feito:** snapshot apenas na exportação/publicação solicitada, sanitização e confirmação de deployment; índice local continua sendo banco de trabalho. |

## 10. Codex e distribuição

| Nº | O que foi pedido | Situação e entrega efetiva |
| --- | --- | --- |
| 80 | Preparar o núcleo para ChatGPT/Codex e depois integrar Codex. | **Feito para Codex:** MCP textual, hooks, agentes/skills, instalação idempotente, escolha/export por host e reenvio entre hosts sem trocar tarefa aprovada. Executor Codex herda a configuração da sessão em vez de pedir Sonnet/xhigh. ChatGPT sem ambiente de código continua fora dessa integração. |
| 81 | Funcionar no Codex com o mesmo rigor de ondas, aprovação e medição. | **Feito no código desta etapa:** ciclo de vida, testemunha correlacionada da resposta real e metadados separados por host. Uso em rollout tem adaptador explícito; formatos desconhecidos não recebem valores inventados. **A validar:** ativação/confiança e execução real no Codex. |
| 82 | Testar tudo que foi alterado em Linux, macOS e Windows. | **Feito na base v0.2.16:** seis checks Test/Mods verdes nos PRs #314/#315, release multiplataforma concluída. Esta etapa repete validação local e passa por sua própria CI; recibos e links ficam na validação final. CI não comprova experiência visual na sessão do usuário. |
| 83 | Acompanhar dev, promover main e gerar release multiplataforma. | **Feito até v0.2.16:** dev, promoção main e release multiplataforma concluídos. A etapa atual segue a mesma rota e registra os novos PRs/release na validação final; a instalação do usuário continua sob seu controle. |
| 84 | Evitar duas execuções desnecessárias da CI e liberação duplicada. | **Feito e observado na promoção v0.2.16:** seis checks por PR; pushes reutilizaram a prova da árvore combinada aprovada, sem repetir as suites. Prova incompleta mantém validação integral. |
| 85 | Avaliar outras branches, aproveitar o útil e remover o resto. | **Feito para branches remotas:** seis branches foram conferidas contra o head exato de seus PRs já mesclados (#296/#298/#306/#309/#310/#312) e removidas com verificação do SHA remoto. Permaneceram dev/main; cópias e branches locais foram preservadas. |
| 86 | Toda instalação aplicar tudo da versão e funcionar de forma uniforme nos projetos. | **Feito no instalador e contratos:** nova instalação/migração inclui wave, review, analysis, MCP/hooks, configurações e recursos da versão; fixtures verificam footprint, idempotência e preservação manual. Hooks novos no Codex exigem confiança do próprio host. Não houve reinstalação dos projetos nesta etapa, por orientação do usuário. |

## Validações materiais ainda abertas

1. Validar em sessão real o ciclo de especializações entregue, preservando regras manuais e sem tornar análise extra obrigatória.
2. Medir o benefício por função dos usos do Jev entregues; correção do contrato não comprova economia.
3. Validar o acionamento real de Haiku/Luna na sessão, versão/effort observados, contexto e reaproveitamento; completar paridade operacional do Codex.
4. Validar e ajustar a abertura/usabilidade do Mods no Desktop Windows/SSH, statusline e visualização de seleção/leitura das skills.
5. Conferir versão/configuração/credencial/hooks/RTK/modelo/logs em cada instalação e resolver atribuição de uso ausente quando houver evidência recuperável.
6. Repetir os cenários de Florestal/Sialia e uma execução equivalente de spec; medir custo por entrega aceita, acerto, leitura adicional, primeira aprovação e retrabalho.
7. Comprovar em execução real o comportamento de fatos externos e dados/limpeza; impedimentos de tarefas mistas e diagnóstico de sessão possuem controle nativo. Arquivos apagados não são reconstruídos como fatos.
8. Concluir a promoção/release da nova etapa após sua própria CI. Publicação autenticada de páginas permanece teste separado, condicionado a um pedido explícito de publicação. As seis branches remotas antigas já foram revisadas e retiradas; worktrees locais foram preservadas.

Não há base para declarar “tudo feito”, “instalação perfeita”, economia global ou código praticamente perfeito. Há correções concretas testadas, entregas parciais identificadas e validações operacionais ainda necessárias.

## Evidências de referência

- [Plano: auditoria da sessão e escopo integral](2026-10-05-plano-busca-jev-claude.md), seções 10 e 11.
- [Etapa de agentes, skills e Codex](2026-10-12-agentes-skills-codex.md).
- [Busca: prioridades e resultados](2026-10-10-busca-prioridades-e-resultados.md).
- [PR #309](https://github.com/rubensrpj/mustard/pull/309), [#310](https://github.com/rubensrpj/mustard/pull/310), [#311](https://github.com/rubensrpj/mustard/pull/311), [#312](https://github.com/rubensrpj/mustard/pull/312), [#313](https://github.com/rubensrpj/mustard/pull/313).
- Código: `io/knowledge/enrichment/session.rs`, `io/expertise.rs`, `io/wave_prompt.rs`, `commands/flow/round/review.rs`, `shared/judgement.rs`, `platform/codex.rs`, `hooks/panel-view.js` e `ui/theme.js`.

Detalhes e evidências da etapa: [conclusão da orquestração](2026-10-12-conclusao-orquestracao.md).
