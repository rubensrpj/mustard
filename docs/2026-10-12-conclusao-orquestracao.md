# Conclusão dos ajustes de orquestração, especialização e diagnóstico

Esta é a etapa integrada na **v0.2.16**, PRs #314/#315 e release multiplataforma aprovada. A continuação que fecha falhas encontradas na verificação está em [validação final](2026-10-12-validacao-final.md), com comparação contra a release publicada. O backend e as specs removidos pelo usuário não foram restaurados.

## Decisão após conferir a documentação oficial

Especializar e reutilizar é adequado; multiplicar agentes ou gerar skills a cada tarefa não é uma medida de economia. Uma alteração em vários arquivos, sozinha, não justifica outra análise nem bloqueia o despacho. O scan identifica áreas e fornece referências; ele não transforma padrões encontrados em regras obrigatórias.

- [Claude: escolha entre conversa principal e subagentes](https://code.claude.com/docs/en/sub-agents#choose-between-subagents-and-main-conversation): mudanças curtas, contexto compartilhado e interação frequente favorecem a conversa principal; trabalho delimitado e volumoso pode usar contexto isolado. Descrições curtas evitam carregar todo o procedimento na sessão.
- [Claude: skills e arquivos de referência](https://code.claude.com/docs/en/skills#add-supporting-files): instruções focadas e detalhes sob demanda.
- [Codex: práticas de skills](https://learn.chatgpt.com/docs/build-skills#best-practices): uma função por skill, entradas/saídas claras e teste dos gatilhos.
- [Codex: subagentes](https://learn.chatgpt.com/docs/agent-configuration/subagents): cada agente faz seu próprio trabalho de modelo e ferramentas; reduzir o contexto do pai não comprova redução dos tokens totais.
- [Jev: perguntas atômicas](https://docs.typesafe.ai/introduction): classificação/seleção com perguntas delimitadas; o binário combina as respostas. Classificar um relato não comprova sua verdade. [Choice](https://docs.typesafe.ai/primitives/choice) orienta comparar o conjunto completo de alternativas e permitir abstenção; o Mustard não trunca silenciosamente perfis válidos. Se o conjunto/payload excede o contrato, mantém a seleção nativa sem chamada.

## Comportamento implementado

1. O catálogo prioriza procedimentos autorais revisados e atuais. Perfis nativos de navegação cobrem o restante. Fontes e regras mudadas retiram o procedimento da seleção; arquivos manuais continuam preservados.
2. `expertise --prepare --host claude|codex --file <arquivo>` reutiliza e exporta apenas os perfis pertinentes. `--query <tarefa>` prepara um procedimento ausente quando a orientação nativa não basta. O retorno contém o contrato da análise pela sessão existente; não inicia CLI ou API de geração.
3. `expertise --prepare-session claude|codex --query <tarefa>` e `--complete-session <recibo.json> --query <mesma tarefa>` fecham a criação do rascunho. A ativação exige inspeção e `--review <nome>:<fingerprint>`; a análise não aprova a si mesma.
4. A rodada fornece sugestões de preparação para áreas indexadas sem procedimento autoral. Elas têm `required:false` e zero chamadas de modelo: o orquestrador avalia a necessidade. Quantidade de arquivos é filtro de candidatos, não autorização para gastar.
5. Jev só compara perfis quando existem alternativas autorais revisadas cobrindo igualmente a tarefa. Perfis com procedimento/escopo idênticos dispensam classificação. Uma chamada agrupa questões independentes. Respostas ambíguas ou inválidas mantêm a seleção nativa; cache evita nova chamada. Revisão de diff, autorização e provas continuam nativas.
6. Devoluções ambíguas sem alteração podem receber classificação do Jev. Condições explícitas usam regras nativas. O resultado registra `verified_fact:false` e `approves_delivery:false`.
7. Uma tarefa não concluída que volta novamente sem alteração e sem revisão de seu contexto recebe impedimento persistente. A primeira devolução guarda a referência; mudança real do pedido reinicia a avaliação. Trabalho independente continua.
8. Análises pendentes são deduplicadas entre hosts. Uma segunda preparação devolve a referência existente, sem outro pacote de delegação. `knowledge --cancel-session <id>` libera explicitamente trabalho abandonado. Análises concluídas e atuais são reaproveitadas.
9. Haiku/Luna continuam com a última versão disponível da família, effort médio e alvo fixo de 100 mil tokens totais de entrada. Modelo/effort/uso efetivos só são registrados quando expostos pelo host. Preparar um pacote não demonstra execução nem cumprimento do orçamento pelo host.
10. CLI, painel e Codex resolvem a mesma credencial: ambiente, configuração local privada do projeto/monorepo e configuração do projeto. Arquivo local versionado, não ignorado ou ligado para fora não serve como origem. A chave nunca aparece no diagnóstico.
11. O painel apresenta agente sugerido, descrição de especialidade correta nos dois idiomas, critério de seleção, skills entregues e leitura desconhecida/confirmada. Relatos com fontes deixaram de ser rotulados como fatos verificados. O diagnóstico distingue configuração de execução observada, RTK disponível, índice vetorial e falhas.
12. O painel tenta abrir mesmo quando fechar a referência anterior falha. Testes simulam reabertura e referência perdida; não comprovam renderização no Desktop Windows por SSH.
13. Codex recebe eventos de ciclo de vida e testemunhas somente da resposta real correlacionada de `request_user_input`. Metadados de hooks são separados por host. O leitor de uso reconhece um formato explícito de rollout; formatos diferentes permanecem desconhecidos. Cache lido é subconjunto da entrada, não somado duas vezes.
14. CI reaproveita a prova da árvore realmente testada, inclusive se a base mudou a versão. A igualdade com a árvore do head isolado deixou de causar validação duplicada. Ausência de prova completa mantém a validação integral.

## O que não se pode afirmar com estes testes

Não existe percentual global comprovado de economia, garantia de código perfeito, leitura efetiva de toda skill ou confirmação de saldo do Jev. O teste de um contrato não substitui uma execução pareada da mesma spec nos hosts.

A instalação encontrada nos projetos é anterior ao código desta etapa. Atualização de runtime durante ondas ativas continua excluída. Autenticação/publicação Cloudflare e aceite visual no Desktop dependem das respectivas sessões; nenhuma página foi publicada automaticamente.

As correções concretas do Zod/duplicações no backend são trabalho naquele repositório, não uma migração que o Mustard pode aplicar genericamente. Estado externo como acesso ao bucket precisa de observação atual: um resumo antigo ou rótulo do Jev nunca prova esse acesso.

## Medição para aceitar a especialização

Comparar a mesma tarefa e base em três modos: executor genérico, executor com skills pertinentes e executor especializado. Somar tokens de pai e filhos, leituras adicionais, chamadas/custo Jev, recusas, correções e tempo até entrega aceita. Separar entrada/cache/saída sem contar o cache duas vezes. Custo de assinatura ou saldo remoto desconhecido não pode virar estimativa de cobrança.

A menor quantidade de texto no contexto do pai é uma medida de contexto, não de economia líquida. Um procedimento que só muda o nome do agente ou exige nova análise toda vez deve ser retirado da seleção paga. Nenhuma comparação desta etapa autoriza promessa de código perfeito ou busca universal.

## Evidências históricas da etapa v0.2.16

- Core: 1.217 testes; runtime: 1.879 testes e dois novos testes focados de aprovação Codex/seleção Jev; integração do runtime: 294 testes; Mods: 42 testes. Clippy sem avisos, plugin validado e 22 testes do roteamento da CI. As baterias de CLI, scan e integração do core também passaram na execução anterior; a CI da branch precisa repetir o conjunto nos três sistemas.
- Oito testes da execução paralela recusaram gravações/cópias por falta de espaço. A repetição sequencial dos 294 testes de integração passou; nenhuma falha de infraestrutura foi contabilizada como sucesso.
- Aceitação do gateway: preservação de stdout/stderr/status, passagem à ferramenta do host quando a busca nativa está indisponível, referências atuais, paginação, invalidação e reaproveitamento após compactação. Zero chamadas HTTP de inferência. A simulação de ausência de `rg` isola também o executável, porque o pacote pode conter seu próprio `rg`.
- No cenário isolado de conteúdo já reconhecido, o corpo repetido caiu de 1.266 para 457 bytes: 63,90% menos texto. Essa medida não é uma comparação desta versão com a anterior, nem redução de tokens totais/cobrança.
- Aceitação do modelo local: vetores padrão, ausência de recálculo para fonte inalterada, atualização após alteração de fonte, modo raw preservado e opt-out. Zero chamadas remotas.

O diagnóstico novo foi executado em modo somente leitura nos cinco projetos, com o binário de validação, sem substituir suas instalações:

| Projeto | Documentos vetorizados | Chunks | Jev/credencial | RTK | Carimbo instalado |
| --- | ---: | ---: | --- | --- | --- |
| suzano/portal-florestal-backend | 1601 | 17025 | disponível/configurado | disponível/configurado | 0.2.13 |
| atiz/sialia | 5527 | 39167 | disponível/configurado | disponível/configurado | 0.2.13 |
| atiz/mustard | 668 | 20056 | disponível/configurado | disponível/configurado | 0.2.13 |
| suzano/portal-florestal-frontend | 1603 | 17310 | disponível/configurado | disponível/configurado | 0.2.13 |
| competi/sialia | 4481 | 29587 | disponível/configurado | disponível/configurado | 0.2.13 |

Índice preenchido não comprova uma consulta semântica atual; credencial disponível não comprova chamada útil ou saldo. O diagnóstico não fez requisição remota nem inicializou o encoder. Os cinco têm carimbo/recibo 0.2.13 e ainda não possuem observações dos hooks novos. Atualização e ativação nos hosts continuam distintas da entrega do código.

Seis branches remotas antigas foram conferidas contra o head exato dos PRs mesclados #296/#298/#306/#309/#310/#312 e retiradas; branches/worktrees locais foram preservadas.
