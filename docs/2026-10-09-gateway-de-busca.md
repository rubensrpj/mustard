# Gateway de busca e aprendizado local

Implementação em `codex/mustard-plano-completo`, na cópia isolada de desenvolvimento. Esta etapa sucede a investigação orientada à tarefa registrada em `2026-10-08-scan-oraculo.md`: a entrada principal pesquisa o código antes de consultar o banco. A instalação pessoal não é atualizada automaticamente.

## Continuação: evidência localizada antes do julgamento — 09/10

Esta continuação parte de `0f857a215dfd6fd31e059c289e91bd95b9b2c99d`, na mesma branch isolada. Os resultados das etapas anteriores são históricos: não se somam percentuais nem se compara um corpus com outro como se fossem o mesmo teste. O trabalho principal continua sendo o gateway e o scan; o Sialia foi utilizado somente para testar recuperação, sem alterar sua aplicação.

### Alterações

1. **Diagnosticar a perda de candidatos.** `candidate_flow` conta proprietários nativos, candidatos hidratados e candidatos admitidos para decisão. `MUSTARD_SEARCH_TRACE=1` registra IDs/faixas/hashes e posição após expansão, sem enviar esse diagnóstico ao provedor ou à apresentação comum. A expansão de arquivos passa por novo ranqueamento antes do corte: uma declaração encontrada depois não fica simplesmente no fim da lista.
2. **Indexar funções internas com escopo.** Consultas Tree-sitter incluem funções nomeadas internas em Python, Rust, TypeScript/JavaScript e C#, incluindo funções locais e variáveis com funções em TS. O scan registra o callable que as contém e impede ligações fora desse escopo nas linguagens que declaram essa regra em `languages.toml`. PHP conserva sua semântica global. Isso não resolve integralmente sombreamento, funções na mesma linha ou todas as formas de função anônima.
3. **Pesquisar a intenção no código.** Pistas escritas raras recebem uma reserva no catálogo, dentro do inventário da ferramenta original. O binário usa os destinos do índice para executar uma busca complementar nativa; confirma os termos normalizados na fonte atual, cruza linhas com declarações e registra fatos com hash. Prefixos são somente pré-filtro. Há limites explícitos de 96 arquivos/12 MiB, com incompletude informada; a resposta nativa original permanece disponível. Não se pressupõe que todo código do projeto esteja indexado ou possa ser recuperado nessa exploração.
4. **Preservar a área solicitada.** Em `spec` e `understand`, proprietários nativos cujo nome/caminho contém componentes escritos do padrão original têm prioridade de leitura. `CreateCharge` pode localizar `CreateChargeAsync` por componentes de identificador, sem aceitar uma substring arbitrária. O achado permanece na ordem mesmo quando a intenção usa verbos genéricos. Essa prioridade não cria identidade exata, recomendação, sinônimo ou certeza de comportamento. As demais finalidades conservam sua política de área anterior.
5. **Jev somente para escolher entre evidências.** A interface `SymbolSelector` continua separando o domínio do provedor. `knowledge-choice-v5` exige suporte no próprio código para entrada, saída e restrições da pergunta; distingue `none`, evidência insuficiente e candidatos indistinguíveis. Não presume que a resposta esteja no conjunto. Identidade, busca nativa, aprendizado, painel e scan não ganham inferência paga de rotina. `choose:true` e a configuração do provedor continuam necessários no gateway. Os limiares de aceitação não foram alterados nem são tratados como calibração geral.

Dois problemas adicionais apareceram no teste C#/React: nomes em minúsculas não encontravam componentes concatenados como `QuartzPayClient`, e documentos já escaneados podiam solicitar um scan completo por não possuírem símbolos. A normalização da fonte agora conserva componentes contíguos de identificadores, sem juntar palavras comuns da prosa; o índice lexical migra para a versão 3. A atualização reconhece documentos e arquivos sem declarações pelo hash atual. Fonte modificada ainda exige atualização. A cadeia também deixa de gastar outro corpo somente para repetir uma pista já coberta.

### Método de validação

`gateway-audit.mjs` separa os rótulos do pedido de busca e confere arquivo, nome, intervalo, referências visíveis e corpos realmente presentes na saída. Expõe ganhos e regressões; um JSON que contém um candidato não basta para dizer que o agente o recebeu. A seleção `repoqa-selection-quality-20261009.json` foi congelada antes das primeiras alterações, com dez descrições por repositório, quatro linguagens e pedidos lexicais fixos. Não é uma sessão autônoma de Claude nem a avaliação oficial do RepoQA. Depois da primeira execução, as repetições passaram a ser diagnóstico conhecido.

Uma primeira estratégia de normalização de comprimento piorou os resultados: no diagnóstico conhecido, 31/40 passaram a 30/40; na seleção nova, houve 25/40 nos dois lados e 16,6% mais texto. Essa estratégia foi descartada. A versão mantida preserva a vantagem de funções curtas específicas e evita conceder a mesma vantagem a campos/tipos genéricos; não implementa um BM25 completo.

Os resultados finais desta continuação e do teste Sialia constam no fechamento de validação abaixo. Não há prova de economia de tokens faturados, melhora do código implementado ou substituição universal de ferramentas de busca.

### Jev: piloto real separado da recuperação

Oito casos curtos com fonte, preparados antes das chamadas, tiveram 4/4 resultados corretos no grupo de desenvolvimento e 4/4 no grupo reservado: quatro escolhas, dois `none` e duas abstenções esperadas, sem escolha errada aceita. Foram oito chamadas, 8.014 tokens conhecidos e custo estimado de US$ 0,000336. Repetições usaram cache sem HTTP. É uma amostra construída pequena, sem generalização estatística.

Nos oito pedidos públicos, a primeira versão teve quatro escolhas corretas, quatro abstenções e nenhuma errada aceita, com nove chamadas e 151.735 tokens conhecidos. A repetição depois das correções de recuperação manteve quatro escolhas corretas e quatro abstenções, sem escolha errada aceita: oito chamadas, 102.000 tokens conhecidos e US$ 0,004284 estimados. O alvo estava disponível em cinco dos oito conjuntos nativos; uma abstenção ainda deixou de selecionar um alvo disponível. A primeira posição correta passou de 1/8 na ordem nativa para 4/8 na escolha. As oito repetições reutilizaram o cache. O teste não enviou código do Sialia ao Jev.

Somando os três pilotos executados nesta continuação: US$ 0,010993 estimados pela tarifa configurada. São recibos de uso e estimativas, não a fatura do provedor. As versões dos binários e hashes ficam nos relatórios; a última repetição paga antecede a ampliação final da prioridade de leitura de `spec`/`understand`. Essa ampliação não altera os critérios do Jev. Os princípios de perguntas atômicas, critérios que distinguem opções e estado compartilhado seguem a documentação de [Choice](https://docs.typesafe.ai/primitives/choice) e [State](https://docs.typesafe.ai/concepts/state).

### Tarefa completa

O preflight gratuito agora também executa o gateway real em duas tarefas JavaScript, localiza os arquivos e verifica preservação nativa/zero inferência. O avaliador pareado confere os recibos de ferramentas: uma execução Mustard com busca direta bem-sucedida conhecida fora do gateway, ou sem recibo que a esclareça, não vale como prova de roteamento. Programas arbitrários de shell ainda exigem revisão dos eventos. A sessão autônoma permanece pendente de autenticação/API e modelo fixo; o preflight não executou um modelo.

### Teste Sialia: utilidade do prompt e lacunas

Pergunta do usuário: “Validar se todo o fluxo da safe2pay está pronto e verificar a viabilidade de usar o assas”. Foi tratada como levantamento, interpretando o fornecedor pretendido como Asaas. A chamada inicial usou a intenção integral e o padrão `safe2pay|asaas|assas`, com `purpose:spec`, `choose:false`. Seis buscas complementares cobriram cobrança, webhook, assinatura, marketplace, contratos de provedor e frontend. Essas chamadas foram preparadas durante a investigação; não se afirma que o Claude as gerou automaticamente.

O teste utilizou uma cópia descartável do Sialia principal em `810c7937081d7a3fd2a7db8332ec488dcaeb0635`, com backend no commit adicionado pelo usuário, `1ff44e04de2fe0d112f66eee89d38f8a902a7f4b`. A execução anterior com o submódulo remoto antigo não serve como comparação dessa versão. Nenhum código da aplicação foi alterado, nenhuma API de pagamento foi chamada e nenhum código do projeto foi enviado ao Jev. A descoberta de candidatos de teste não significa que esses testes tenham sido executados.

| Medida do diagnóstico orientado à tarefa | Primeiro diagnóstico isolado | Versão final |
| --- | ---: | ---: |
| Pontos de código visíveis, entre 12 escolhidos durante a análise | 0/12 | 7/12 |
| Corpos completos desses pontos na apresentação inicial | 0/12 | 1/12 |
| Referências/corpos da área de pagamentos nas primeiras dez posições da busca inicial | 0/10 | 10/10 |
| Bytes totais devolvidos nas sete buscas | 78.781 | 101.016 (+28,2%) |
| Resultados originais preservados | 7/7 | 7/7 |
| Chamadas a modelos | 0 | 0 |

Os doze pontos foram identificados no código durante o diagnóstico e não entraram nos pedidos do gateway. São critérios conhecidos, sem percentual independente de acurácia. O primeiro diagnóstico usa o binário intermediário `revised-bin`, não o baseline `0f857` das baterias públicas. O ganho de relevância não comprova economia: aumentou a saída, e seis dos sete pontos localizados ainda exigem expansão para conferir sua implementação.

O retorno final localiza criação/cancelamento de cobrança, filtro de assinatura de webhook, criação de assinatura, contrato de provedor, um acoplamento da sincronização de contratos e descritor de frontend. Ainda não expõe nesse pacote os cinco pontos restantes: consulta de estado, tokenização, processamento de negócio do webhook, desativação de subconta e roteamento de provedor. Eles exigem buscas mais dirigidas; a resposta nativa original continua disponível. Logo, a pergunta é um ponto de partida útil, mas **o pacote atual não é suficiente para declarar todo o fluxo pronto nem para elaborar sozinho uma migração de provedor**.

Artefatos locais: `target/search-quality-20261009/sialia/requests.json`, `assessment-labels.json`, `assessment.json` e `survey/comparison.json`. O avaliador verifica nomes/faixas/hash e a apresentação efetivamente entregue. Tempos são exploratórios: caches, estado do índice e outras execuções simultâneas diferiram; não há percentual causal de aceleração.

### Recuperação nas baterias públicas

No diagnóstico conhecido, **31/40 → 33/40** declarações visíveis (77,5% → 82,5%, +5 pontos percentuais), com dois ganhos e nenhuma regressão. Corpos completos permaneceram em 25/40. Bytes **468.951 → 436.489 (−6,9%)**; recall nas primeiras cinco posições 60% → 62,5%, e nas primeiras dez 67,5% → 70%. O resultado original foi conservado em 40/40 casos, com zero modelos. Tempos agregados foram 176,6 s → 229,8 s, sob condições exploratórias e baseline reutilizado. Relatórios em `target/search-quality-20261009/final-diagnostic/`.

A primeira execução da seleção nova não melhorou a cobertura e aumentou o texto, conforme registrado acima. Depois de inspecioná-la, a repetição final passou a ser um teste conhecido: **25/40 → 25/40** declarações visíveis (62,5% nos dois lados), corpos completos **16 → 17**, bytes **471.370 → 434.019 (−7,9%)**. Houve **dois ganhos e duas regressões**, não preservação de todos os acertos anteriores. Recall nas primeiras cinco posições foi 40% → 42,5%; nas primeiras dez, 47,5% → 45%. `_display_complete_info` do Poetry e `handleTriangle` do Three.js são perdas que permanecem no relatório. O resultado nativo original foi conservado em 40/40 casos; zero chamadas a modelos.

Os tempos dessa repetição foram maiores: 155,3 s agregados no baseline reutilizado e 201,9 s no atual. Cache, concorrência e estado de execução não foram controlados para uma comparação de latência; os números não provam o tamanho da penalidade, mas também não sustentam prometer uma busca mais rápida. A versão final ainda precisa de uma nova amostra não inspecionada e de sessões completas para demonstrar ganho geral.

Relatórios: `target/search-quality-20261009/fresh/` registra a primeira execução nova; `final-recheck/` registra a repetição conhecida, com `comparison.json`, `audit.json`, fontes identificadas por hash e a apresentação de cada caso. As respostas esperadas não foram fornecidas ao gateway ou ao Jev.

### Regressão Florestal e verificações locais

O replay de oito buscas foi repetido com o binário baseline congelado em uma nova cópia do mesmo commit do backend e configuração nativa equivalente. Ambos localizaram 7/8 pontos exigidos; corpos completos passaram de 1/8 para 2/8. A saída caiu de 49.343 para 48.810 bytes (−1,1%). `processPlanBackground` continua ausente nesse pacote e requer consulta dirigida. Os 356 números de linha completos conferidos na nova saída não significam que toda a tarefa tenha sido coberta. As quatro buscas de preservação nativa da suíte de responsabilidade também passaram, sem Jev.

Foram aprovados 3.958 testes da suíte Rust, o teste unitário adicional de componentes literais de identificador, Clippy sem avisos, os 16 testes do SDK Mods e a validação do plugin. O preflight de tarefa completa continuou gratuito. A aceitação a partir de uma pasta vazia passou com o `mustard init` e os binários desta cópia: atualização nativa, aprendizado, hashes, fallback, paginação, encaminhamento clássico e zero HTTP/modelos.

Uma execução anterior da aceitação, durante testes simultâneos, encontrou `database is locked` na gravação de fatos, que usa espera curta. A repetição passou. O fallback conserva a pesquisa, mas o aprendizado pode ficar sem gravação quando há disputa; não há garantia de persistência imediata sob concorrência. A falha está preservada em `target/quality-native-survey.log`, e o resultado da repetição em `quality-native-survey-retry.log`. Não se atribui uma causa específica ao processo concorrente sem rastreamento de locks.

## Histórico: triagem da recuperação e uso pontual do Jev — 09/10

Esta etapa compara o código com o commit `7c72ceaedab4a3e361c3bedbf9184600545aa70c`. As alterações anteriores permanecem abaixo como histórico; seus percentuais não devem ser somados aos desta etapa.

### O que mudou

- O cruzamento nativo agrupa ocorrências por arquivo, lê/hashifica a fonte e carrega os intervalos dos proprietários uma vez. Sai o corte das primeiras 256 ocorrências. Permanecem os limites explícitos de arquivos, bytes e tempo; ocorrências omitidas são contabilizadas e o resultado nativo original continua disponível.
- Recuperação, decisão e apresentação agora têm conjuntos distintos. Até 24 candidatos atuais da ordem de leitura são preservados antes do corte de cartões/vizinhos. O Jev compara esse conjunto, em vez de alternativas acidentais da apresentação; referências ao conjunto continuam expansíveis sem acrescentar todos os corpos. Um teste coloca a função desejada depois dos oito primeiros cartões e confere sua presença no seletor e na resposta quando escolhida.
- A pergunta contribui com candidatos antes de proprietários de uma busca ampla consumirem o orçamento de hidratação. A descoberta pelos termos originais e pela intenção conserva reservatórios separados. Isso não garante incluir todo proprietário no pacote inicial; o diagnóstico informa candidatos omitidos.
- A ordem de leitura usa a intenção local, frequência dos termos entre declarações e o componente de normalização de comprimento do BM25. A diversidade entre arquivos passa a ser uma penalização suave: a segunda função relevante de um arquivo pode preceder uma função fraca de outro. Essa combinação é uma heurística própria, não uma implementação completa de BM25 nem compreensão semântica certificada. Uma palavra comum que coincide com um nome de função dentro de um OR amplo não vira âncora exclusiva. Listas nominais e padrões de declaração são reconhecidos com proprietários atuais e prefixos das assinaturas reais, sem lista fixa de palavras de linguagem. Nomes exatos do pedido continuam prioritários.
- A admissão da fonte ocorre antes do corte de apresentação. Um arquivo indisponível deixa a investigação parcial e não derruba as outras evidências atuais. Fontes excluídas não são fornecidas ao seletor. O fallback conserva a resposta original.
- Corpos de dependências entram quando a pergunta menciona seu nome ou quando acrescentam uma pista escrita ainda ausente dos corpos iniciais. Outras dependências permanecem como referências expansíveis. A projeção reduz dados repetidos das relações. Perguntas com uma âncora nominal continuam adiando declarações laterais; perguntas abertas conservam referências aos candidatos principais mesmo quando seus corpos foram adiados.
- A hidratação lê/decodifica cada pacote de arquivo uma vez por operação, em vez de pedir ao SQLite que decodifique o mesmo JSON para cada função. SQLite, FTS e as gramáticas existentes permanecem; não foram acrescentados banco vetorial, modelo local ou projeto de terceiros.
- O aprendizado considera versões de fontes além das primeiras 256 linhas. Até 256 testemunhos persistidos são distribuídos entre arquivos admitidos; esse limite de amostragem não é um limite de funções do índice estrutural. Uma fonte nova após 600 ocorrências de outro arquivo solicita reconstrução nativa e passa a ter proprietário no banco.
- O estado Choice v4 guarda hashes uma vez por arquivo e reduz chamadas estáticas a alvo, linha e tipo de resolução. Trechos atuais e sua completude permanecem explícitos. Critérios distinguem opções; identidade exata, descoberta, cruzamento, armazenamento e apresentação continuam determinísticos. A interface `SymbolSelector` preserva a separação do provedor.

O uso mantém as recomendações de [perguntas atômicas e estado compartilhado do Jev](https://docs.typesafe.ai/introduction). Uma distribuição aceita continua sendo uma recomendação, não prova de comportamento. A escolha não pode recuperar uma função que o motor descartou.

### Comparação de recuperação — diagnóstico conhecido

Mesmas 40 descrições públicas, mesma consulta `rg --sort=path` nos dois braços, sem rótulos na pesquisa e sem IA. O critério de localização exige arquivo, nome e intervalo da declaração esperada; referências visíveis e corpos completos são contados separadamente.

| Medição | `7c72ceaedab4` | Esta etapa |
| --- | ---: | ---: |
| Declaração esperada no conjunto devolvido | 18/40 (45%) | 25/40 (62,5%): +17,5 pontos percentuais |
| Declaração esperada visível ao agente | 13/40 | 24/40 |
| Corpo esperado completo no pacote inicial | 7/40 | 17/40 |
| Proprietário esperado identificado no cruzamento nativo | 12/40 | 24/40 |
| Bytes de conteúdo devolvido | 589.201 | 352.359: −40,20% |
| Paridade dos registros nativos / chamadas a modelos | 40/40 / 0 | 40/40 / 0 |

O ganho de localização é **38,89% relativo**, de 18 para 25, não 38,89 pontos percentuais. Este conjunto já foi usado para diagnóstico; ele não demonstra generalização. A seleção mais ampla vem acompanhada de referências verificadas, e não da inclusão de todos os corpos no contexto. Todos os corpos emitidos permanecem acompanhados de hash/intervalo; fonte truncada exige leitura das faixas ausentes.

No replay Excel do mesmo snapshot Florestal, o fluxo guiado mantém **25/25** evidências completas: **68.679 → 62.911 bytes (−8,40%)** em relação à etapa anterior. Com confirmações simuladas de entrega, devolve 62.144 bytes. Frente ao baseline histórico de 58.326 bytes, ainda há aumento; não é uma redução acumulada desde o início. As 34 consultas fixadas preservam **8/8** evidências completas, com 65.342 bytes frente a 65.718 na etapa anterior. Esses dois replays não executam um Claude autônomo.

### Confirmação independente — 40 perguntas novas

A seleção `repoqa-selection-decision-20261009.json` foi congelada antes da correção que separa candidatos de decisão e apresentação: `mlc-ai/mlc-llm`, `xenova/transformers.js`, `huggingface/candle` e `lima-vm/lima`, dez descrições por repositório. Não foram usadas para ajustar parâmetros de recuperação ou limiares do Jev. Não há nome/caminho esperado nos pedidos enviados ao binário.

| Medição independente, sem IA | `7c72ceaedab4` | Esta etapa |
| --- | ---: | ---: |
| Declaração esperada devolvida | 16/40 (40%) | 31/40 (77,5%): +37,5 pontos percentuais |
| Declaração esperada visível, com intervalo conferido no texto do agente | 15/40 | 31/40 |
| Corpo esperado completo, conferido no texto do agente | 11/40 | 25/40 |
| Bytes devolvidos | 970.073 | 468.762: −51,68% |
| Paridade nativa / chamadas a modelos | 40/40 / 0 | 40/40 / 0 |

O aumento relativo de alvos devolvidos é **93,75%**, de 16 para 31; a recuperação final é **77,5%**, não 93,75%. **Nove alvos continuam ausentes**. Esses números medem recuperação do alvo rotulado, não precisão de todas as referências emitidas, sucesso de implementação ou economia de tokens faturados. O bootstrap lexical é fixo e cego ao gabarito; não é uma sessão autônoma do Claude nem a pontuação oficial de RepoQA. Ambos os conjuntos finais tiveram suas referências e corpos conferidos contra o texto efetivamente emitido, sem divergência em relação aos contadores do relatório.

Os baselines foram reutilizados somente após conferir hashes do dataset, da seleção e do executável, preservando os quarenta registros e pedidos. Todas as quarenta consultas do código final foram executadas novamente, incluindo paridade nativa. Os ensaios rodaram com tarefas concorrentes; seus tempos não sustentam uma afirmação de aceleração em produção.

### Primeiro piloto Jev externo, usado como diagnóstico

Oito descrições públicas, as duas primeiras de cada repositório da seleção `repoqa-selection-triage-20261009.json`, foram fixadas antes da avaliação. Os rótulos esperados são usados somente pelo avaliador. O provedor recebe perguntas e candidatos de código público; não recebeu código privado do Florestal.

- O alvo esperado estava no pacote nativo em **3/8** perguntas; o primeiro candidato nativo era correto em **0/8**.
- O Jev selecionou corretamente esses **3/3** alvos disponíveis: **3/8 no total**. Aceitou **duas escolhas erradas** quando o alvo esperado não estava disponível e absteve-se nos outros três casos. Confiança alta e a política numérica atual não eliminaram esses erros.
- Foram **oito chamadas físicas**, **134.451 tokens de entrada**, custo estimado de **US$ 0,005647** pelo preço configurado. As oito repetições usaram cache: **zero chamadas adicionais**. O registro de tentativas confirma esses totais; não é uma fatura do provedor.
- Um ensaio anterior foi interrompido por uma asserção que confundia uso desconhecido com zero chamadas. Foi corrigido para separar falha, abstenção e cache, não repetir automaticamente falhas de transporte e preservar os registros físicos mesmo em caso de erro. Nesse ensaio interrompido, apenas 29.466 tokens estão comprovados pelos resultados parciais salvos; o uso total não foi preservado. O valor do piloto completo acima não representa todo o gasto desta conversa.

**O Jev demonstrou utilidade para escolher entre candidatos disponíveis, mas não confiabilidade suficiente para ampliar seu uso automático.** `choose` continua sendo autorização explícita e a resposta conserva as alternativas e a necessidade de conferir o código. A próxima melhoria de acerto deve recuperar candidatos ausentes e avaliar abstenções em outra seleção, sem ajustar limiares sobre estes oito rótulos e chamar isso de calibração geral.

### Jev após separar decisão e apresentação

No mesmo diagnóstico de oito perguntas, os alvos disponíveis continuaram em **3/8**, com **três escolhas corretas, duas erradas e três abstenções**. A separação não resolveu a recuperação desses cinco alvos ausentes. O estado deixou de gastar entrada com alternativas incidentais da apresentação: **134.451 → 70.707 tokens de entrada (−47,41%)**, mantendo os mesmos totais de acerto. Foram oito chamadas e custo estimado agregado por chamada de **US$ 0,002971**, com oito repetições sem HTTP adicional. Essa redução compara os mesmos oito pedidos, não toda uma sessão Claude nem somente a mudança isolada de serialização v4.

Uma seleção independente, congelada antes dessa correção, trouxe oito novas descrições de `mlc-ai/mlc-llm`, `xenova/transformers.js`, `huggingface/candle` e `lima-vm/lima` (duas primeiras de cada repositório):

| Medição independente do Jev | Resultado |
| --- | ---: |
| Alvo disponível no conjunto nativo | 4/8 |
| Primeiro candidato nativo correto | 3/8 (37,5%) |
| Recomendações corretas do Jev | 4/8 (50%); 4/4 dos alvos disponíveis |
| Recomendações erradas / abstenções | 1 / 3 |
| Precisão entre recomendações aceitas | 4/5 (80%), amostra pequena |
| Chamadas físicas / repetições pagas | 8 / 0 |
| Tokens de entrada / custo estimado agregado | 93.732 / US$ 0,003937 |

A escolha acertou um caso além do primeiro candidato nativo, mas ainda aceitou um alvo errado. Não equivale a acertar 100% das pesquisas, nem demonstra calibração geral. Não foram ajustados limiares com base nesses rótulos. Os dois pilotos após a separação registraram 16 chamadas, estimativa agregada de US$ 0,006908 e zero chamadas nas 16 repetições. Os logs físicos por repositório ficam preservados junto aos artefatos, inclusive em caso de falha do executor.

### Validação e artefatos

**3.948 testes Rust aprovados**, dois ignorados herdados, análise estática estrita sem avisos, **16 testes do SDK Mods** e validação do plugin aprovados. A instalação do próprio binário em pasta realmente vazia passou, incluindo descoberta após 600 ocorrências, reconstrução nativa, fallback bruto, contrato tipado, dependências e recibos de contexto; zero HTTP/inferência nesse caminho.

Os binários de cada ensaio foram congelados antes de executá-lo. Resultados finais, pedidos, fonte/linhas, saídas ao agente e hashes: `target/search-triage-20261009/{complete-diagnostic,complete-independent,complete-backend}`. Pilotos pagos e logs físicos: `beam-jev-diagnostic/` e `beam-jev-independent/`. Auditoria de entrega: `actual-agent-visibility.json`; manifesto: `manifest.json`. Os registros das tentativas intermediárias permanecem com seus próprios hashes; não são resultados acumulados nem substituem a comparação final. Logs de verificação: `target/triage-complete-{workspace-tests,clippy,mods,plugin,native}.log`.

### Prova que ainda falta

A execução autônoma de Claude permanece indisponível neste ambiente sem autenticação. Os replays medem conteúdo devolvido por consultas fixadas, não decisões reais de um agente, tokens totais faturados ou qualidade de código implementado. O executor de sessões pareadas entregue anteriormente continua sendo a prova necessária para afirmar economia de uma tarefa completa. Nenhuma publicação, instalação pessoal ou alteração do backend original faz parte desta etapa.

## Histórico: investigação, contrato e reutilização (`7c72ceaedab4`)

Esta continuação amplia o mesmo gateway; não cria outro motor de busca. O baseline da comparação é `64c7549768b0deefc897fe1fa6d7750cf2046ea0`. O núcleo segue independente de Claude/Codex; a integração instalada e os eventos de contexto desta versão são do Claude Code.

- **Investigação encadeada:** âncoras nominais e recomendações aceitas podem seguir alvos estáticos únicos do scan, inclusive quando a dependência não repete as palavras da busca. Até quatro sementes, oito relações e dois corpos adicionais/promovidos entram no pacote inicial, dentro do inventário admitido pelo pedido original. Referências restantes continuam expansíveis. O limite representa exploração parcial; não altera as ocorrências nativas originais.
- **Parâmetros, tipos e testes:** o scan guarda campos de parâmetros/tipos escritos na declaração, com linhas e bytes da fonte. A extração usa campos da gramática existente, conferida em Rust, Python e TypeScript; campos ausentes ficam desconhecidos. O encadeamento registra a linha da chamada e menções atuais em arquivos de teste associados. Isso não demonstra valores em execução, propagação de dados, efeitos no banco nem cobertura de testes. A mudança de versão do bloco de declarações e a marca do scanner invalidam o reaproveitamento de formatos antigos. Um índice migrado sem marcas estruturais solicita reconstrução nativa mesmo numa busca sem ocorrências; hashes aprendidos antes da migração não certificam um índice derivado vazio.
- **Contrato por operação:** `plugin/hooks/search-schema.js` contém um objeto de dados compartilhado pelo Mods e pelo Rust. O schema discrimina dez operações e seus argumentos. Campos errados, ausentes ou desconhecidos recebem correção antes de pesquisar. A assinatura do agente continua `{request:{tool,input,intent,purpose,choose?}}`; o contexto de entrega não é parâmetro fornecido pelo modelo. O schema serializado ocupa 3.752 bytes: existe custo inicial de instrução, ainda não medido como tokens numa sessão.
- **Reutilização confirmada pelo host:** somente corpos completos, atuais e efetivamente recebidos pelo adaptador podem virar uma referência de reutilização. A chave separa checkout, sessão, agente, época de contexto, hash e intervalo. O Mods confirma o recibo depois da execução autorizada e confere tamanho UTF-8 e checksum do conteúdo recebido; erro, recusa, interrupção, truncamento, reescrita ou confirmação ausente entrega o corpo novamente. O checksum verifica integridade de transporte, não autenticação. Compactação, início/fim/recarregamento da sessão e mudança de agente separam o histórico. Fontes editadas invalidam o recibo. Corpos pequenos continuam completos quando um aviso de reutilização custaria mais. `Read`, diagnóstico e saída nativa bruta mantêm seu contrato completo.
- **Sessões LSP reutilizadas:** um processo privado local conserva a sessão do compilador por até 90 segundos sem atividade. Token aleatório, isolamento por checkout/linguagem/binário/ambiente, quadros limitados e consultas serializadas protegem a comunicação. Cada resposta é consultada novamente. Inventário nativo, hashes de fontes/configurações guardados por metadados e metadados das dependências detectam mudanças e reiniciam a sessão. Arquivos ignorados explicitamente consultados também participam. Falha/inventário incompleto usa uma consulta fria; indisponibilidade conserva o fallback textual. Os diretórios usuais de dependências são conferidos por metadados, inclusive alvos de links: isso não atesta todos os possíveis insumos externos do compilador. A verificação acrescenta trabalho, principalmente na primeira consulta.
- **Choice econômico:** evidência de fonte fica no estado compartilhado; critérios trazem pistas escritas que distinguem candidatos. Foram retiradas pistas e ressalvas repetidas. Perguntas continuam atômicas, com `none` e `insufficient`, cache de decisões e nenhuma escolha de identidade por IA. A política mantém confiança ≥ 0,5, probabilidade ≥ 0,7 e margem ≥ 0,2, e rejeita distribuições numéricas inválidas. Observações por escolha permitem avaliar a política; esses valores não são prova de correção nem calibração geral.
- **Avaliação reproduzível:** há uma nova seleção externa de repositórios, piloto pago do Jev, prova de reutilização e consultas LSP reais. O novo executor de sessões pareadas também prepara tarefas completas com verificadores de comportamento externos à cópia do agente, registra uso incluindo cache, custo estimado, ferramentas e releituras, e invalida o braço Mustard se o gateway não tiver sido chamado.

O protocolo de entrega permite que um adaptador futuro de Codex forneça identidade e confirmações equivalentes. Ele não presume acesso ao contexto de outro agente nem instala hooks de Codex agora. A fonte deve ser lida novamente quando o agente precisar de conteúdo ausente; a reutilização não autoriza conclusões com base em um resumo incompleto.

### Resultados e limites desta continuação

| Medição | Baseline | Continuação |
| --- | ---: | ---: |
| Declarações esperadas localizadas, 40 descrições externas, busca ordenada | 18/40 (45%) | 18/40 (45%): **sem ganho** |
| Corpos esperados completos no pacote inicial | 7/40 | 7/40 |
| Bytes do retorno nessas 40 consultas | 583.516 | 589.201: **+0,97%** |
| Paridade com registros da pesquisa nativa | 40/40 | 40/40 |
| Repetição de corpo completo no mesmo contexto, fixture de aceitação, bytes visíveis ao agente | 1.271 bytes na primeira entrega | 462 bytes na repetição: **−63,65%** |
| Consultas TypeScript repetidas, média de três destinos conhecidos | 5,37 s, servidor frio | 2,28 s, sessão reutilizada: **−57,55%** |
| Primeira consulta TypeScript após iniciar o processo | 5,38 s | 9,80 s: inicialização/verificação mais cara |
| Destinos TypeScript corretos nas dez verificações | — | 10/10, incluindo reinício após editar a fonte |
| Piloto Jev, mesmos 12 casos preparados | 12/12 corretos; 14.449 tokens | 12/12 corretos; 14.069 tokens: **−2,63%** |
| Custo estimado dos 12 casos Jev | US$ 0,000608 | US$ 0,000590 |
| Replay Excel V3, 34 consultas previamente definidas | 65.025 bytes; 8/8 corpos necessários | 65.718 bytes; 8/8: **+1,07%** |
| Replay Excel guiado, 36 consultas previamente definidas | 58.326 bytes; 25/25 corpos necessários | 68.679 bytes; 25/25: **+17,75%** |
| Mesmo replay guiado, com confirmação simulada de entrega | 58.326 bytes | 67.912 bytes: **+16,44%** |

**A recuperação por descrição ainda é insuficiente para chamar o scan de oráculo completo.** Na comparação com busca nativa ordenada, ambos localizaram 18 alvos e 22 não chegaram como cartões; não houve ganho de recuperação. O retorno nativo continua disponível, e nenhum resultado adicional é certificado como semanticamente correto por aparecer no grafo. Ensaios anteriores sem ordenação variaram: 12/40 → 15/40 e 13/40 → 15/40. Não usamos esses números para declarar melhoria; a ordem nativa alterava o conjunto admitido numa busca ampla. O ensaio final usa o mesmo `rg --sort=path` nos dois braços. Esse controle torna a comparação mais estável, sem modificar os argumentos das consultas reais do usuário. Os números vêm de um bootstrap lexical fixo e cego ao gabarito; não são consultas decididas por Claude nem a pontuação oficial de RepoQA.

No replay Excel, manter as consultas antigas e acrescentar dependências aumentou o contexto devolvido. A confirmação de entrega economizou 767 bytes dentro da versão nova, mas não compensou o aumento frente ao baseline. Os 25 corpos esperados continuam presentes; isso não prova menos investigações posteriores. Uma primeira medição artificialmente menor perdeu cartões devido ao problema de migração do índice descrito acima e foi descartada. **Não há economia geral de tokens nem ganho de qualidade de implementação demonstrados por esses replays.** Bytes de saída não são tokens faturados; os recibos de transporte são retirados antes de contar conteúdo entregue ao agente.

Os quatro repositórios externos são novos em relação às seleções anteriores: `psf/black`, `cheeriojs/cheerio`, `tokio-rs/tracing` e `nsqio/nsq`, dez descrições de cada. Dataset, commits, seleção e binários têm recibos SHA-256. Não executamos seu código nem usamos modelos nesse conjunto. Os tempos desse replay não são tratados como latência de produção, pois testes locais também estavam em execução.

No LSP, a sessão reutilizada é vantajosa para consultas repetidas; uma consulta isolada pode ficar mais lenta. A medição real usou somente TypeScript no mesmo snapshot Florestal; não demonstra esses ganhos em todos os servidores. O índice nativo de cada binário foi preparado antes do cronômetro para separar migração de consulta do compilador; o ambiente de cada ensaio inicia uma sessão nova. Reiniciar depois de uma edição levou 7,65 s. A primeira tentativa de verificação integral era excessivamente cara e não reutilizou a sessão de forma confiável; foi substituída por hashes guardados por metadados nativos, com conferência das dependências e testes de invalidação. Em Unix, inode/ctime também detectam escrita de mesmo tamanho com mtime restaurado. Outros sistemas dependem dos metadados disponíveis; não há atestação criptográfica exaustiva de entradas externas.

O piloto Jev separa seis casos de avaliação inicial e seis reservados, escritos antes do primeiro ensaio. A política não foi ajustada para acertar os rótulos. Ambos os binários acertaram os 12, inclusive as abstenções; portanto não há ganho de acurácia demonstrado. A primeira forma do novo payload gastou 14.974 tokens, acima do baseline. A compactação final removeu repetições e chegou aos 14.069. Foram **36 chamadas físicas nos três ensaios**, com custo somado estimado de **US$ 0,001826**; a repetição de cada pedido fez zero chamadas graças ao cache. Custo calculado pela tarifa configurada no aplicativo, não conferido como fatura do provedor. Amostra pequena e preparada não calibra probabilidade em projetos reais. Alinhamento: [perguntas atômicas e estado compartilhado](https://docs.typesafe.ai/introduction), [Choice](https://docs.typesafe.ai/primitives/choice).

A prova pareada de tarefa completa ainda está **pendente de autenticação**. O executor passou no preflight gratuito: as implementações originais falham nos verificadores independentes, e as referências passam. Nenhum Claude autônomo executou essas tarefas nesta entrega. O modo automatizado `--bare` usa chave API explícita, segundo a [documentação oficial](https://code.claude.com/docs/en/headless); uma assinatura autenticada numa sessão interativa exige validação separada. Tokens faturados, sucesso de implementação e economia total não são inferidos de bytes de retorno.

Artefatos locais em `target/search-next-20261009/`: `heldout-ordered/comparison.json`, `backend/comparison.json`, `warm-lsp-ready/comparison.json`, `jev-comparison.json`, `jev-final/pilot.json` e `session-pair-final/comparison.json`. A prova nativa de instalação vazia é `target/scan-gateway-20261009/native-acceptance.json`. Os benchmarks e fixtures necessários para reproduzir os ensaios estão versionados em `apps/scan/benchmarks/` e `apps/scan/tests/fixtures/`. O benchmark de aceitação usa `node:sqlite` e exige Node 22+; isso não é dependência do binário.

Verificação desta continuação: **3.941 testes Rust aprovados**, dois ignorados herdados; **16 testes do SDK Mods** e validação do plugin aprovados. Os totais menores nas etapas abaixo são históricos. A sessão real autenticada do Claude continua pendente; os testes locais não substituem essa medição.

### Diagnóstico que motivou a triagem seguinte

A triagem dos 22 alvos ausentes do ensaio ordenado encontrou 14 com ocorrência nativa dentro do intervalo esperado. Em cinco desses casos, `evidence.current_owner_ids` já identificava a função esperada, mas ela não aparecia em `task_context.cards`. Os outros oito não tinham ocorrência no corpo esperado; não é possível atribuir todos à mesma causa sem conferir cobertura e vocabulário. Artefato: `heldout-ordered/failure-native-audit.json`. Essa triagem usa o gabarito somente para avaliar o retorno, nunca como pista de pesquisa.

1. **Corrigir perdas entre ocorrência, proprietário e seleção.** Em `knowledge/investigation.rs`, o cruzamento começa pelas primeiras 256 ocorrências; em `knowledge.rs`, a seleção inicial aplica uma cota antes de expandir vizinhos. A auditoria encontrou seis alvos com ocorrência somente depois das primeiras 256 linhas. A prioridade é agregar ocorrências por proprietário/arquivo e avaliar sua relevância antes do corte, com processamento incremental e incompletude explícita. Aumentar todas as cotas elevaria custo e ruído sem demonstrar melhoria. Cinco proprietários já conhecidos e ausentes dos cartões precisam entrar nos casos de diagnóstico dessa seleção.
2. **Melhorar a recuperação por intenção quando a consulta não alcança o alvo.** Distinguir identificadores exatos de perguntas de comportamento; dividir a dúvida em pesquisas complementares usando nomes, comentários, assinaturas e relações já disponíveis. Preservar o pedido original e seus resultados. A preparação determinística não deve prometer compreender significado que não esteja escrito no projeto. Conferir a cobertura do parser antes de culpar apenas os termos de busca.
3. **Selecionar evidência conforme a dúvida.** Localização precisa de caminho/linha; identidade precisa da declaração; comportamento pode exigir corpo e dependências específicas. A expansão automática atual de até dois corpos aumentou o replay guiado em 17,75%. Só mantê-la como ganho de economia depois de demonstrar consultas evitadas numa tarefa completa; quando isso não ocorrer, oferecer referências para expansão conforme a pergunta.
4. **Avaliar o Jev sobre candidatos realmente disponíveis.** Choice ajuda a distinguir responsabilidades sustentadas por evidência. Não corrige um alvo descartado antes da escolha. Os 12 casos preparados e a redução de 2,63% do payload não demonstram ganho em pesquisas abertas. Avaliar sua contribuição, abstenções, chamadas e custo nesses casos, depois da correção da recuperação, sem transformar identidade de símbolo em pergunta paga.
5. **Medir sucesso completo e congelar critérios antes da alteração.** Usar esses 40 casos como diagnóstico e uma seleção independente para confirmar generalização. Comparar Claude com/sem Mustard na mesma tarefa e modelo, com correção externa, uso de tokens incluindo cache, custo, consultas e tempo. Exigir melhora de recuperação na seleção independente, nenhuma perda de evidência necessária e redução mensurável de consumo total com ambas as implementações corretas. Não existe percentual de economia garantido por esta arquitetura; bytes menores ou mais testes passando não satisfazem esse critério.

As correções determinísticas podem prosseguir sem uma sessão Claude autenticada. A autenticação é necessária para a prova final de uso completo, não para corrigir o cruzamento e o ranqueamento. Não é preciso trocar SQLite, incorporar outro projeto ou instalar um modelo local para começar por essas falhas observadas.

## Ampliação entregue em 09/10: símbolos, caminhos, estrutura e referências

As novas operações usam o mesmo contrato versionado do gateway e exigem arquivo explícito. São ferramentas determinísticas, sem chamadas a modelos, mesmo com `choose:true`. O Jev continua atrás da interface de escolha de responsabilidades; resolução de identidade pertence ao índice/compilador.

| Operação | Entrada em `input` | Evidência e limite |
| --- | --- | --- |
| `Symbol` | `file_path`, `symbol` | Declaração atual pelo ID exato recebido na busca; assinatura, faixa/hash e interpretações atribuídas. Fonte alterada exige nova localização. |
| `Trace` | `file_path`, `symbol`, `direction?`, `depth?`, `limit?`, `target?` | Navegação estática existente, até quatro níveis; alvo opcional conserva o caminho encontrado. `target_reached:false` significa não alcançado na exploração delimitada, não inexistência. Não demonstra ordem de execução nem fluxo de dados. |
| `Structure` | `file_path`, `query` | Consulta Tree-sitter sobre a fonte atual. Mesmo catálogo de gramáticas do scan; sem outro parser. Retorna capturas, linhas/bytes, hash e flags de incompletude. |
| `References` | `file_path`, `line`, `column`, `relation?`, `limit?` | Definições, referências ou implementações pelo índice SCIP atual ou servidor LSP instalado. Sem provedor/resultado, inclui busca textual nativa e ressalva de que ocorrências não são referências comprovadas. |

`line` começa em 1; `column` é deslocamento UTF-8 em bytes, começando em 0. O adaptador converte para a codificação negociada pelo LSP. `relation` aceita `definitions`, `references` (padrão) e `implementations`. Todos os achados com recibo atual alimentam fatos locais e são cruzados com proprietários do scan. Quando um arquivo novo pede atualização, o resultado já executado é reutilizado após o scan; não se repete o parser/servidor para obter o cruzamento. Acesso, exclusões, segredos e isolamento do checkout continuam aplicados.

Exemplo da chamada enviada pelo Claude através do Mods:

```json
{"request":{"tool":"References","input":{"file_path":"src/controller.ts","line":42,"column":18,"relation":"definitions"},"intent":"Qual declaração atende esta chamada no escopo atual de tipos e imports?","purpose":"understand","choose":false}}
```

O CLI recebe esse pedido envolvido em `{"schema_version":1,"request":{...}}`. O arquivo e as coordenadas são exemplos de contrato; devem vir de uma ocorrência real. Os IDs de `Symbol`/`Trace` vêm de `cards[].id` ou `owners.symbols[].id` atuais, nunca de adivinhação do agente. `references[].symbol` pertence ao provedor de precisão e não é o ID de cartão do scan.

### Catálogo existente e importação opcional

Os servidores não têm um cadastro paralelo: argumentos/opções de inicialização entram em `platform/code_tools.rs`, já usado por instalação, atualização e diagnóstico. Rust, TypeScript/JavaScript, C#, Go, Python e PHP reutilizam esse catálogo. TSX/JSX usam os identificadores apropriados do protocolo. Linguagem sem servidor cadastrado conserva o fallback; presença de gramática não implica suporte LSP. A resolução real nesta etapa foi conferida em TypeScript; os demais servidores dependem de instalação e configuração do projeto.

Na primeira ampliação, a consulta LSP abria uma sessão com prazo de 15 segundos e fechava os processos ao terminar. A continuação acima substitui esse ciclo pelo processo privado com reutilização; a consulta fria permanece como fallback. Não instala servidores nem guarda conclusões do compilador entre consultas. No TypeScript, `useSyntaxServer:"never"` habilita a resolução semântica, com aquisição automática de tipos desabilitada. Essa escolha foi conferida no [servidor oficial](https://github.com/typescript-language-server/typescript-language-server/blob/master/src/utils/configuration.ts) e testada no backend. Uma consulta fria pode ser mais lenta que grep; use-a quando a identidade da chamada exigir verificação.

SCIP é alternativa opcional, pela interface `PreciseSymbols`:

```sh
mustard-rt run knowledge --import-scip index.scip --source-manifest receipt.json
```

O Mustard importa o formato [SCIP](https://github.com/scip-code/scip) com `prost`; nenhum indexador ou projeto completo foi incorporado. O manifesto liga `index_sha256`, `position_encoding` e `sources` (caminho relativo → SHA-256). Texto atual embutido também comprova o documento. Inclua configuração do compilador e lockfiles no manifesto para invalidar o índice quando mudarem. Entradas externas omitidas não ficam atestadas automaticamente. Importação é atômica; uma fonte vinculada alterada invalida a geração inteira antes de usar suas referências. Índices antigos não viram certeza por terem sido produzidos por compilador. Símbolos externos sem fonte admitida não são lidos silenciosamente.

A consulta estrutural aplica as gramáticas existentes, com até 128 resultados, 64 capturas por resultado, 2.000 caracteres por captura e 128 KiB de saída, além do orçamento do cursor. Os avisos distinguem erro de parse, busca interrompida, capturas/corpos abreviados e limites. Isso controla trabalho local e torna lacunas visíveis; não remove ocorrências da busca tradicional original.

### Relevância, memória e decisões de arquitetura

Uma âncora nominal atual agora vale também para expressões com vários nomes, como `createXlsxStream|createXlsxBuffer`. Complementos não indexados que só coincidem com palavras genéricas da intenção deixam de entrar no pacote inicial; ocorrências da pesquisa original continuam preservadas. Sem âncora, a descoberta complementar continua disponível. O diagnóstico contabiliza `weak_complements_deferred`; a mudança não comprova compreensão semântica geral.

Entregas de ondas podem incluir `knowledge` com título, conclusão e todos os recibos de fonte que a sustentam. O binário valida na cópia da onda, transporta para a entrega oficial e registra somente após aceitação. Registros têm origem de spec/entrega e status `hypothesis`; não são fatos comportamentais certificados. Repetições não duplicam anotações nem alteram a geração sem necessidade. Edição/formatação que muda o hash invalida a conclusão, sem recalcular um recibo para fazê-la parecer atual. Não há chamada adicional de IA para escrever esse conhecimento.

A inspeção dos planos de consulta confirmou índices para símbolo exato, chamadas e consumidores no SQLite. Mantido o banco atual; migrar para outro motor não corrigiria complementos fracos nem identidades ambíguas. Não foi instalado banco vetorial nem modelo local. A busca por significado continua dependendo das palavras/documentação disponíveis, das interpretações atribuídas e, pontualmente, da escolha autorizada do Jev.

Os conceitos foram aplicados em código próprio: [Serena](https://github.com/oraios/serena) para símbolos e expansão sob demanda; [ast-grep](https://ast-grep.github.io/guide/introduction.html) para consulta estrutural, implementada com o Tree-sitter já existente; [GitNexus](https://github.com/abhigyanpatwari/GitNexus) como referência de resolução por escopo e navegação; [SCIP](https://github.com/scip-code/scip) e [LSP](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/) como contratos de interoperabilidade. Código/licenciamento dos projetos completos não foi incorporado. O protocolo SCIP e a dependência Protobuf são acréscimos desta ampliação; a continuação acrescenta `getrandom` para o token privado do processo LSP; afirmações de ausência de novas dependências nas etapas históricas abaixo referem-se àquelas etapas.

Os moldes PT/EN foram compactados para preservar o contrato e a orientação das ondas dentro dos limites existentes, sem remover as verificações de autorização, provas e revisão. O painel Mods e a publicação explícita continuam com o funcionamento já entregue. A integração instalada para Codex permanece futura.

### Medições da ampliação

Mesmo snapshot Florestal `a3fe37ab454c`, com 12.832 declarações; backend original preservado.

| Caso conhecido | Antes | Agora |
| --- | ---: | ---: |
| Busca ampla `createXlsxStream`: retorno ao agente | 7.700 bytes | 5.966 bytes (**−22,52%**) |
| Complementos não indexados sem o identificador original | 12 | 0 |
| Complementos com o identificador original | 0 | 1 trecho atual de teste |
| Corpos completos das duas declarações homônimas | 2/2 | 2/2 |
| Definições de chamadas reais, por LSP | Sem essa operação | **3/3 destinos esperados**, Puzzle/CEM/MLPlan |

A busca ampla conserva os mesmos registros nativos (2.605 bytes); a ordem de duas execuções físicas de `rg` sem ordenação pode diferir. Seu pacote ainda é maior que grep isolado porque inclui os dois corpos e referências. Remover os complementos fracos melhora este caso; não mede precisão semântica geral. As três consultas LSP frias demoraram **5,37–6,16 s**, com retornos de **1.835–1.950 bytes**, incluindo proprietários do scan. O grafo estático anterior já resolvia essas três chamadas; o ganho é a verificação explícita pelo compilador, não corrigir três erros antigos inventados. Outros servidores não foram medidos em projetos reais nesta etapa. Importação SCIP foi validada por fixtures do protocolo; não executamos um indexador externo no backend.

O replay das 34 consultas do prompt Excel V3 mantém **65.025 bytes, 8/8 corpos conhecidos completos e paridade nativa em 34/34**. O fluxo guiado de 36 consultas mantém **58.326 bytes, 25/25 declarações completas, 682 linhas únicas conferidas e uma leitura explícita de 100 linhas**. Não houve ganho adicional nesses dois números frente à etapa anterior; os ganhos de 1,90%/12,01% apresentados abaixo continuam históricos e não se somam ao ganho de 22,52% da consulta ampla. O fluxo guiado levou aproximadamente 4,36 s nesta passagem, sem aceleração demonstrada.

Todos esses testes usaram **zero chamadas locais/remotas a modelos e US$ 0 de Jev**. Isso testa o caminho nativo, não avalia a qualidade do Jev nem o consumo completo de uma sessão. Repetição guiada continua sem um Claude autônomo implementando o prompt. Logs, pedidos, recibos e hashes locais: `target/search-expanded-20261009/{relevance-comparison,precise-comparison}.json`; replay Excel em `target/simulation-excel-db-v3-focused-20261009/`.

Validação: **3.931 testes Rust aprovados**, dois ignorados herdados; análise estática estrita sem avisos; **14 testes do SDK Mods** e validação do plugin aprovados. A instalação nativa em pasta realmente vazia passou, incluindo descoberta estrutural nova, atualização do banco e cruzamento do mesmo resultado, sem HTTP/inferência. Mudanças finais nos moldes também são conferidas pelas suítes de contratos, orçamento e contexto da sessão. Falta a sessão real autenticada do Claude para medir encaminhamento, decisões de consulta, custo total e qualidade de uma implementação.

## Contrato de busca: Claude Code primeiro

O Mods exige `{request:{tool,input,intent,purpose,choose?}}`. `purpose` é explícito; `intent` contém a pergunta específica que a consulta deve esclarecer. Investigação ou seleção sem pergunta é recusada antes de executar a pesquisa. Localização literal admite intenção vazia. O erro orienta corrigir o pedido, sem apresentar uma quebra de contrato como motivo para contornar o gateway.

O adaptador acrescenta a versão e chama o CLI com `{"schema_version":1,"request":{...}}`. O contrato vive no domínio do binário, sem tipos do SDK Claude. Versões desconhecidas e campos obrigatórios ausentes são recusados. O CLI conserva o formato anterior para consumidores existentes, aplicando a mesma exigência de intenção nas investigações. O envelope prepara novos adaptadores; não instala integração de ChatGPT/Codex nem presume que esses aplicativos tenham os mesmos hooks.

A ferramenta é o caminho preferido. No Bash original, a descrição `mustard:spec: Verificar se o download usa os dados atuais da cópia` transmite finalidade e pergunta pelo hook, preservando os argumentos nativos. As outras finalidades usam o mesmo formato. Descrição comum continua em `locate`; o binário não adivinha finalidade a partir de prosa. Sintaxe de shell não suportada conserva o caminho original e suas permissões. Não há promessa de interceptar toda forma de leitura de qualquer aplicativo.

Os moldes de sessão, ondas e revisão ensinam o mesmo contrato em português e inglês. O objetivo geral da spec continua no bloco `context`; cada consulta leva a dúvida local, sem repetir a spec inteira. `choose` autoriza o seletor existente apenas diante de alternativas de responsabilidade ainda não resolvidas. O contrato não torna uma intenção automaticamente correta e não comprova economia de uma sessão.

Nesta versão, a integração ativa é Claude Code: Mods, hooks e CLI. A comparação com modelos pequenos permanece uma avaliação separada; o modelo estático opcional existente não é ativado por esta alteração.

Validação do contrato: 28 testes Rust (3 de domínio, 5 de encaminhamento e 20 do gateway), 14 testes do SDK Mods e validação do plugin aprovados. A análise estática de todo o workspace passou sem avisos. Esses testes verificam transporte, correção de pedidos inválidos e preservação das permissões; não medem economia nem substituem a sessão real com Claude Code.

## Expansão por declaração — ajuste após a simulação V3

Uma declaração cujo nome aparece no padrão nativo ganha prioridade como fonte. Quando há uma única identidade atual, o padrão é um identificador literal e o cruzamento nativo está completo, a resolução é nativa mesmo que a pergunta seja mais longa; `choose` não aciona o Jev para escolher essa identidade novamente. Ocorrências não mapeadas, cortes de página/recibos e proprietários não recuperados impedem afirmar identidade única. Homônimos e perguntas de responsabilidade continuam sujeitos à comparação ou abstenção existentes. Identificar uma declaração não comprova que ela atende à regra de negócio.

Na investigação de um nome, outros corpos deixam de entrar apenas por acrescentarem palavras da intenção. Chamadas relacionadas continuam como referências expansíveis; seleção explícita pode acrescentar uma alternativa. Tipos que contêm métodos não repetem todos os filhos em perguntas de comportamento. Descoberta sem âncora nominal conserva a investigação complementar anterior. O diagnóstico mantém todos os cartões recuperados; o retorno inicial dá coordenadas aos proprietários/relacionados pertinentes e comandos nativos para expandir os demais candidatos.

Trechos incompletos fornecem `missing_source_reads`: intervalos omitidos e linhas cujo texto foi abreviado. A expansão não exige reler linhas completas já entregues. Corpos completos reaproveitam seus comentários internos, sem duplicar o campo de resumo; testes adicionais têm comando `map tests`, e pontuações lexicais ficam no diagnóstico. Relações fora do escopo têm contador próprio, sem serem rotuladas como fontes desatualizadas nem provocar leitura externa para conferir o hash.

Mods e os moldes de sessão/ondas/revisão nas duas línguas orientam investigar a declaração após descobrir o arquivo, reutilizar corpos completos e escolher `Read` com faixa específica. `Read` continua preservando exatamente o pedido; reduzir leituras amplas depende de o agente seguir essa orientação. Nenhum modelo ou dependência foi adicionado.

### Medição no último prompt de Excel

Snapshot Florestal `a3fe37ab454c`, base nativa atual com 12.832 declarações. O prompt é o **PROMPT TÉCNICO V3: GERAÇÃO DE PLANILHAS EXCEL (MENU E CURVA) A PARTIR DO BANCO DE DADOS** enviado pelo usuário. Relatórios, pedidos, código de reprodução e retornos ficam em `target/simulation-excel-db-v3-focused-20261009/`.

| Cenário | Consultas | Retorno ao agente | Corpos de referência completos |
| --- | ---: | ---: | ---: |
| Simulação anterior | 34 | 66.286 bytes | 8/8 |
| Repetição das mesmas consultas após o ajuste | 34 | 65.025 bytes (-1,90%) | 8/8 |
| Consultas por declaração e duas verificações adicionais | 36 | 58.326 bytes (-12,01%) | 8/8 |

A repetição controlada conserva os achados das 34 consultas originais. Sua saída ainda fica 7,65% acima da referência nativa de 60.405 bytes, porque as duas investigações entregam fonte adicional. Leituras explícitas não são truncadas para melhorar números. Referências diretas dos corpos exibidos mantêm nomes, faixas e assinaturas disponíveis mesmo sem coincidência lexical com a intenção; preservar essas pistas acrescenta saída em relação à medição intermediária.

No fluxo por declaração, 14 leituras amplas viram investigações pelos nomes encontrados no levantamento anterior. Uma leitura dos modelos Prisma continua com suas 100 linhas; todas as 25 declarações selecionadas para conferência chegam completas. Foram verificadas 682 linhas únicas de fonte, contra 1.412 anteriormente. As duas consultas adicionais conferem o caminho oficial da entrada e os contratos de armazenamento. Zero chamadas locais/remotas a modelos e US$ 0 de Jev. As execuções das ferramentas levaram aproximadamente 4,2 s, contra 1,65 s na medição anterior: há menos saída e mais investigação local, sem ganho de latência demonstrado.

São casos conhecidos de desenvolvimento e consultas guiadas pelo levantamento anterior, não decisões de uma sessão autônoma do Claude. Bytes de saída não incluem instruções, esquema, argumentos, raciocínio e turnos seguintes; esses percentuais não equivalem a tokens faturados nem economia total. Cobertura das declarações escolhidas não mede precisão geral, correção da implementação ou fidelidade de uma planilha que não foi gerada.

Validação desta etapa: 37 testes de conhecimento no núcleo, 25 testes do gateway, 14 testes Mods, análise estática estrita e instalação nativa em pasta vazia. As regressões cobrem foco por nome, expansão de chamadas, faixas omitidas inclusive linhas abreviadas, escopo versus desatualização e ausência de chamada paga para identidade única, preservando comparação de homônimos.

## Fluxo entregue

1. O adaptador recebe ferramenta, parâmetros originais, intenção e finalidade.
2. O binário aplica as regras existentes de acesso e executa a busca nativa. Não executa uma string de shell recebida no pedido.
3. As ocorrências retornadas são conferidas no arquivo atual e registradas com caminho, linha, texto, hash e identidade do checkout.
4. Arquivos descobertos ou alterados solicitam o scan estrutural incremental nativo. Nenhum modelo de significado é carregado nesse caminho. O resultado da pesquisa é reaproveitado, sem executar a mesma busca novamente.
5. As ocorrências são cruzadas com declarações da mesma versão. O retorno acrescenta funções proprietárias, documentação, contratos disponíveis, referências candidatas e intervalo exato para leitura. O histórico pode ser expandido pelo comando indicado.
6. A intenção organiza candidatos para leitura. Pontuação por palavras não certifica responsabilidade. Identidades exatas permanecem nativas; alternativas ainda não resolvidas, com intenção e autorização explícita `choose`, podem chegar ao seletor configurado, inclusive entre arquivos. A seleção nunca remove ocorrências da resposta original.
7. Em `locate`, a apresentação escolhe o resultado original ou um agrupamento menor que conserva todas as ocorrências. Com intenção e outra finalidade, pode entregar evidência de tarefa: trechos atuais, referências expansíveis e lacunas. O resultado original continua no diagnóstico e pode ser solicitado com `locate` ou `--raw` nativo. Falha de banco/scan não apaga achados reais.

## Entrada e retorno

```sh
mustard-rt run search --shell-output --intent "corrigir a persistência" --purpose implement -- rg -n --with-filename "save|persist" src
```

Sem flag de apresentação, o CLI retorna o diagnóstico JSON: `result`, `evidence`, `learning`, `task_context` quando aplicável, e contadores. Esse relatório completo não é injetado automaticamente pelo Mods/hooks. `locate` conserva a busca; as demais finalidades, acompanhadas de intenção, ativam a investigação descrita abaixo. Expansão explícita: `run knowledge --symbol <id>`; o JSON também conserva comandos/faixas de leitura e história. O padrão de pesquisa nunca é reescrito pela intenção.

```json
{
  "request": {
    "tool": "Grep",
    "input": { "pattern": "save|persist", "path": "src", "output_mode": "content", "-n": true },
    "intent": "corrigir a persistência",
    "purpose": "implement",
    "choose": false
  }
}
```

Essa é a entrada de `mcp__mustard__search`. O CLI `--request` recebe o objeto interno, sem o envelope `request`. Adaptadores: `Grep`, `Glob`, `Read` textual, `rg`, `grep` e `git grep`. Executáveis nativos usam `input.args`.

`--raw` preserva bytes de stdout/stderr e código de saída dos executáveis nativos. Com `purpose=locate`, `--shell-output`, usado pelo Mods/hooks, retorna o resultado original ou um agrupamento menor, sem descartar/reordenar linhas ou duplicatas. `@ caminho` abre um grupo; cada linha mantém `número:texto`. `# static owners:` identifica funções e faixas estáticas conferidas. Candidatos cujo relatório aumentaria a resposta conservam o resultado original/agrupado sem esses extras. Isso compara duas representações completas; não corta resultados para cumprir um limite. Formatos mistos, contexto, JSON nativo, saídas não reconhecidas e erros preservam a saída original.

Em `locate`, o modo de agente também aceita ferramentas tipadas: usa o resultado paginado de Grep, incluindo offset/continuação; nunca o stdout não paginado do subprocesso. Glob/Read/count preservam o objeto de resultado do adaptador, sem acrescentar o relatório. Read e count continuam assim mesmo em outras finalidades; Grep/Glob com intenção podem produzir a visão de tarefa separada. Read conserva arquivo, offset e total de linhas. `--raw` continua exclusivo de executáveis nativos. stderr e falhas do host ficam visíveis no Mods. Um `choose` explícito pode acrescentar a recomendação solicitada, identificada como tal, além do tamanho da busca original; não é o comportamento das buscas comuns.

`run map search` encaminha ao mesmo gateway e retorna o diagnóstico JSON. Os agentes usam a porta de apresentação, não essa projeção completa. Consultas analíticas `knowledge` permanecem para expansão/investigação explícitas.

## Investigação integrada à busca — continuação em 09/10

A visão de tarefa conecta ao gateway a investigação que antes exigia chamar `knowledge` separadamente. A finalidade deve ser `understand`, `spec`, `implement` ou `validate`, com intenção não vazia. Busca literal/`locate`, Read e contagens mantêm seu contrato anterior. Não acrescentamos dependências, um modelo de significado ou regras sobre um projeto específico.

1. A busca original roda primeiro, com seus argumentos, resultado, stderr e código de saída preservados. O runtime pode atualizar a estrutura nativamente sem repetir essa busca.
2. Um inventário nativo aplica caminhos, globs, tipos e opções de exclusão suportadas **antes** de selecionar candidatos no SQLite. Símbolos, recursos, interpretações e navegação ficam nesse inventário; recibos externos não autorizam leituras complementares. Git grep usa arquivos rastreados; grep simples limita essa expansão aos arquivos das ocorrências verificadas. Opções desconhecidas/ambíguas conservam o resultado nativo em vez de ampliar silenciosamente o escopo.
3. As pistas da expressão original e da intenção alimentam a recuperação textual do banco. São pistas lexicais, sem reescrever a expressão regular. O catálogo preserva proprietários dos achados nativos, considera responsabilidades próprias das declarações, diversifica arquivos e reserva espaço para navegação estática. A pesquisa é parcial: o reservatório tem orçamento e o retorno inicial usa até 12 cartões principais; alternativas e referências permanecem identificadas.
4. Descoberta tipada de nomes de arquivos faz uma busca complementar de conteúdo, mantendo padrão/filtros, para obter coordenadas reais das funções. O resultado/página original não é alterado. Essa investigação adicional aparece separada no diagnóstico e na apresentação.
5. O pacote inicial prioriza corpos/trechos das declarações nomeadas no padrão original, além de uma escolha explicitamente recomendada. Sem uma âncora nominal utilizável, conserva a investigação por pistas escritas da intenção. Tipos que contêm métodos não repetem automaticamente todos os filhos. Os demais candidatos e destinos de chamadas pertinentes permanecem como referências com arquivo, função e faixa para expansão. Cobertura de palavras não significa entendimento semântico ou investigação completa.
6. Trechos atuais incluem linhas, assinaturas/comentários não redundantes, contratos disponíveis e candidatos de teste. Declarações pequenas podem seguir completas; maiores recebem janela orientada à finalidade, aviso de incompletude e intervalo de Read. Relações estáticas indicam onde verificar, sem afirmar ordem de execução, cobertura de teste ou equivalência de formatos.
7. Fontes/hash e geração do banco são verificados durante a investigação e depois da seleção. Evidência invalidada é descartada, conservando o resultado nativo e a contabilização física do modelo. Descobertas complementares novas solicitam atualização estrutural nativa antes do julgamento; a classificação não vira fato persistente.
8. Sem evidência útil para a intenção, ou diante de falha/escopo não suportado, volta a apresentação da busca original. Com evidência, a saída se identifica como investigação parcial e mostra como recuperar o resultado completo com `purpose=locate`. Não se apresenta um pacote de contexto como se fossem todas as ocorrências nativas.

O registro do Mods e os moldes dos agentes nas duas línguas ensinam essa distinção e a expansão dirigida. O núcleo continua independente do host; a instalação do adaptador para Codex segue fora desta etapa.

### Jev progressivo

O provedor continua atrás de `SymbolSelector`. Só participa com `choose`, configuração habilitada e alternativas não resolvidas; uma identidade exata não precisa de inferência. O primeiro julgamento recebe evidência factual atual. Apenas `insufficient-evidence` pode produzir **uma** etapa adicional, quando houver fonte efetivamente nova: completar declarações de até 256 linhas e 16 KiB. Baixa confiança, erro de transporte ou mudança da fonte não produzem uma repetição paga automática. Se não houver evidência adicional admissível, mantém a abstenção e os intervalos para leitura.

Cache e registro de tentativas continuam ativos. O retorno soma o uso conhecido das etapas; uso desconhecido permanece desconhecido. Esta mudança melhora a condição de decidir, mas não promete reduzir tokens do Jev: descobrir mais alternativas pode aumentar o estado enviado. O controle de gasto principal continua sendo não chamá-lo para pesquisa literal, atualização, cálculo ou renderização.

### Verificação desta etapa

O replay `apps/scan/benchmarks/gateway-task.mjs` usa oito pesquisas conhecidas do mesmo backend congelado (`a3fe37ab454cede37d3471993eb876985fcdfe1b`). Confere os resultados nativos, recibos SHA-256, linhas entregues, isolamento do checkout e ausência de inferência comum. O backend original permaneceu intacto.

- **8/8 declarações esperadas localizadas**, com arquivo/faixa visível ao agente. **3/8** tiveram corpo completo entregue na visão inicial; as demais exigem expansão antes de concluir sobre seu comportamento. `toMenuRow`, por exemplo, é uma referência estática localizada, não uma responsabilidade demonstrada pelo seu corpo.
- Os oito pacotes entregaram **457 linhas únicas** de fonte atual (**601** considerando repetições entre consultas). Retorno do gateway em `locate`: **21.791 bytes**; visão de tarefa: **62.917 bytes**. A segunda inclui fonte e alternativas que a primeira não entrega. Portanto, **não houve redução de tamanho frente à busca literal** e essa comparação não prova economia nem aumento de custo de uma tarefa equivalente: ainda faltam suas leituras/turnos posteriores.
- Três julgamentos reais do Jev concordaram com as expectativas conferidas no código: `createXlsxStream`, `no-match` para múltiplas abas/fórmulas e `copyPlanFiles`. A identidade exata `plantioCurveToDownload` ficou nativa. Foram **3 chamadas físicas**, **27.036 tokens de entrada**, custo estimado **US$ 0,001135512**, usando o preço de US$ 0,042/M consultado na etapa anterior. Repetições aproveitaram o cache; não houve refinamento adicional nesses três casos. O mecanismo de refinamento foi exercitado com provedor de teste.
- O estado mais amplo da tarefa fez o Jev receber **mais tokens** do que na seleção anterior. A etapa amplia descoberta/entrega de evidência; não é apresentada como redução demonstrada do custo do Jev ou da sessão.

Relatórios locais: `target/scan-task-context-20261009/benchmark-offline.json` (apresentação final) e `benchmark-jev.json` (uso real na primeira execução, antes da compactação final das referências). A compactação altera a apresentação, mantendo o estado de decisão; a repetição final com cache registra a versão revisada separadamente. Casos/expectativas são conhecidos de desenvolvimento, sem percentual independente de acurácia.

```sh
node apps/scan/benchmarks/gateway-task.mjs --root <snapshot-do-backend> --out <relatorio.json>
```

Sem `--jev`, o benchmark remove a chave do ambiente e exige zero inferência. `--jev` exige `TYPESAFE_API_KEY`, configuração de busca habilitada no snapshot e permite chamadas pagas; não salva a credencial. A prova de instalação usa o binário desta cópia em pasta temporária vazia, verifica a visão de tarefa e uma função nova encontrada pela intenção, com atualização estrutural nativa e zero HTTP/modelos.

Validação: **3.907 testes Rust distintos aprovados**, dois ignorados herdados, **13 testes oficiais do Mods aprovados** e lint estrito. O comando de todos os alvos executa também o conjunto do runtime novamente pelo alvo binário; essas repetições não foram contadas como testes distintos. Os sete testes da integração de tarefa foram repetidos depois dos ajustes finais de consistência/apresentação. Logs locais: `target/task-context-{workspace-tests,final-regression,clippy,mods-tests,native-acceptance}-20261009.log`.

Ainda precisa de medição em sessões reais equivalentes para afirmar menor consumo total, menos buscas ou melhor implementação. A visão inicial não elimina toda leitura: fornece evidência e coordenadas para a leitura que falta.

## O que alimenta o banco

As tabelas `search_files` e `search_facts`, no bloco persistente `search_facts` de `grain.db`, guardam observações verificadas por checkout. Repetições reutilizam registros; uma nova versão do arquivo substitui os fatos anteriores. O scan preserva esse bloco. Novas funções, assinaturas e relações entram pelo parser nativo, sem interpretação inventada a partir da consulta.

`learning` informa novos/reutilizados, hashes, arquivos pendentes e resultado do scan. O marcador de versão reconhecida evita repetir um scan apenas porque um arquivo não tem declarações suportadas. Um checkout ligado recebe seu próprio índice estrutural; as observações na âncora continuam separadas por árvore. O índice principal não é substituído pelo código de outra branch.

Essa memória não vence o código atual. A busca começa na fonte e toda identidade enriquecida precisa ter o mesmo hash. Intenção, classificação do Jev e suposições sobre comportamento não viram fatos persistentes. Interpretações revisadas continuam usando recibos próprios e suas fontes.

## Intenção e Jev

A descrição disponível da ferramenta é capturada como intenção. Com vários proprietários e nenhuma intenção, `evidence.intent_requested` sinaliza a necessidade de informar o objetivo. O agente pode completar a intenção; uma busca exata não precisa de pergunta adicional. O Mustard não tenta reconstruir o raciocínio privado do agente.

O Jev usa a interface existente `SymbolSelector`, cache, critérios de aceitação e registro de tentativas. Só recebe alternativas atuais ainda não resolvidas. Uma escolha só é aceita se pertencer ao grupo enviado, que pode abranger arquivos diferentes; fica destacada para expansão, sem eliminar os demais resultados. A interface permite outro provedor no futuro. Busca comum, registro dos achados, atualização estrutural e painel não ativam esse seletor. Ter uma chave configurada não ativa chamadas por pesquisa.

### Seleção por responsabilidade — etapa anterior em 09/10

O caso real do input XLSX do PI revelou uma indicação incorreta: uma vantagem de palavras fazia `createXlsxBuffer` vencer mesmo quando a intenção era a entrega HTTP. O gateway considerava a vantagem suficiente e não consultava o Jev. A correção separa **ordem de leitura**, **identidade exata** e **julgamento de responsabilidade**:

- O planejador do domínio recebe os mesmos pacotes de símbolos das linguagens já registradas. Usa nome, assinatura, comentários e metadados estruturais; não contém termos do backend, condicionais por linguagem ou regras específicas de exportação.
- Assinaturas ganham peso próprio na ordem de leitura. A vantagem textual deixa de gerar automaticamente uma recomendação. Uma identidade única e exata dispensa o provedor; homônimos continuam candidatos.
- Uma pergunta de responsabilidade compara as alternativas descobertas também entre arquivos. O grupo não é mais determinado apenas pela pasta ou pelo arquivo. A interface do domínio recebe decisões por chave de grupo, sem depender do Jev ou do filesystem.
- A camada de IO fornece trechos numerados conferidos contra a fonte/hash atual. Declarações pequenas seguem completas; declarações maiores levam fronteiras e vizinhanças das ocorrências, com `complete:false`. A evidência completa continua disponível por leitura dirigida. Trechos usam até 64 linhas/4 KiB por candidato; isso não corta a busca original nem estabelece ausência de comportamento.
- O adaptador do Jev recebe assinatura, trechos, pistas, rotas e chamadas estáticas identificadas como candidatas. `none` e `insufficient` são opções separadas. Ausência de suporte em um trecho incompleto não pode virar `no-match` aceito.
- `selection.outcomes` distingue `selected`, `no-match`, `insufficient-evidence`, `below-acceptance` e `invalid-answer`. São resultados de classificação, não fatos de comportamento. Os critérios de confiança/probabilidade continuam provisórios e precisam de calibração independente.
- A apresentação de uma escolha explicitamente solicitada mostra também a abstenção, preservando todas as ocorrências. A busca comum continua usando uma representação que não cresce apenas para levar diagnóstico. Ferramentas tipadas de lista/Read/count mantêm seu objeto de resultado.
- Descoberta de arquivos ganha `query_quality` no diagnóstico, com quantidade retornada, inventário indexado, sinal de baixa seletividade e orientação para estreitar a consulta. A comparação com o inventário pode ser parcial; não mede cobertura semântica. O padrão original nunca é reescrito. Esse diagnóstico não é acrescentado automaticamente ao resultado tipado visto pelo agente.

O registro do Mods foi atualizado para ensinar esse contrato. `choose` continua sendo opt-in para uma decisão útil; não é ativado em toda busca por existir uma chave.

### Pesquisa aplicada à correção

Conceitos reaproveitados em implementação própria, sem adicionar motores ou dependências:

| Referência inspecionada | Aplicação |
| --- | --- |
| [Codebase Memory: resolução de chamadas](https://github.com/DeusData/codebase-memory-mcp/blob/92b2dd13d796f22f8001ef70f078e633fd9ec93a/src/pipeline/pass_calls.c) | Separar candidata de vínculo demonstrado; conservar estratégia/incerteza. Nosso seletor não transforma uma pista lexical ou relação estática em certeza. Não incorporamos seu resolvedor de tipos. |
| [Serena: ferramentas de símbolos](https://github.com/oraios/serena/blob/1de556f71569f3acfc0743e526dd60aca40a545e/src/serena/tools/symbol_tools.py) | Identidade por símbolo e expansão de corpo sob demanda, evitando abrir arquivos inteiros por padrão. A ampliação acima consulta os servidores já cadastrados no Mustard; o projeto Serena não foi incorporado. |
| [ckg: recuperação e contexto de tarefa](https://github.com/phins-group/ckg/blob/0895461d16b0d028a67e792757a047536de7df0b/src/retrieval.rs) | Separação entre busca, símbolos, relações e contexto da tarefa; seleção de evidência em vez de anexar todo o banco. |
| [Jev Choice](https://docs.typesafe.ai/primitives/choice) e [State](https://docs.typesafe.ai/concepts/state) | Pergunta atômica sobre alternativas conhecidas, critérios que as diferenciam e evidência factual relevante; abstenção explícita e reutilização de cache. |

### Repetição do caso do backend

Mesmo snapshot `develop` / `a3fe37ab454cede37d3471993eb876985fcdfe1b`, em outra cópia isolada. Repetidas as 28 consultas anteriores e acrescentados dois casos. O backend original permaneceu intacto.

- A consulta que indicava `createXlsxBuffer` passou a indicar `createXlsxStream` pelo Jev. Uma segunda formulação confirmou a mesma função.
- O pedido de várias abas/fórmulas retornou `no-match` para os dois geradores, com a classificação visível na saída do agente. Isso foi conferido no código; não representa um teste de exportação/reimportação.
- A escolha entre três serviços apontou `copyPlanFiles`; a identidade exata `plantioCurveToDownload` permaneceu nativa, sem inferência.
- Nas mesmas oito buscas nativas comparáveis, foram **38.821 bytes** na saída original, **24.390** antes e **24.053** depois. Redução em relação ao original: **37,2% → 38,0%**. Todas as ocorrências, ordem, stderr e códigos de saída foram preservados. A melhora adicional de tamanho é pequena; a correção principal é a recomendação.
- As buscas comuns tiveram **zero chamadas a modelos**. Foram quatro chamadas físicas de seleção, **15.974 tokens de entrada**, custo estimado de **US$ 0,000670908** ao preço consultado de [US$ 0,042/M tokens](https://docs.typesafe.ai/models). As repetições/renderizações reutilizaram o cache sem novas tentativas pagas. O uso do Jev aumentou em relação ao teste anterior, que pulava escolhas por uma heurística incorreta.
- A busca ampla por `export` continuou retornando os mesmos 881 arquivos; o diagnóstico passou a sinalizar baixa seletividade. Não foi apagada do custo nem apresentada como consulta corrigida automaticamente.

Relatórios locais: `target/scan-pi-input-responsibility-20261009/{manifest,comparison,reproducible-jev}.json`. A primeira execução da seleção precedeu a mudança final de apresentação; o manifesto registra os hashes dos executáveis e o relatório reproduzível confirma a versão final usando o cache válido. Casos e expectativas conferidas no código: `apps/scan/tests/fixtures/gateway-responsibility-20261009.json`.

Reprodução sem inferência:

```sh
node apps/scan/benchmarks/gateway-responsibility.mjs --root <snapshot-do-backend> --out <relatorio.json>
```

Para testar a classificação, o snapshot precisa da configuração de julgamento de busca já habilitada. Acrescentar `--jev` exige `TYPESAFE_API_KEY` no ambiente e autoriza inferência paga; nenhuma chave é gravada pelo benchmark. O script exige o commit do corpus, compara o retorno nativo, restaura todas as ocorrências e confere expectativas/cache. Não executar contra um checkout diferente e chamar de comparação equivalente.

Os casos são conhecidos de desenvolvimento. Não constituem percentual independente de acurácia. Repetir as mesmas consultas mantém o número de buscas e as 901 linhas lidas; ainda não comprova economia de tokens/custo da sessão de implementação. Os testes de mecanismo cobrem alternativas entre linguagens, vantagem lexical falsa, abstenção, intervalos atuais, preservação da resposta e ausência de condicionais de linguagem/framework no novo seletor.

Verificação desta continuação: **3.897 testes Rust aprovados**, dois ignorados herdados, **13 testes do Mods aprovados**, lint estrito e prova de instalação nativa vazia sem HTTP/inferência. Logs locais em `target/selection-{workspace-final-tests,clippy-final,mods-tests,native-acceptance}-20261009.log`. Após o ajuste de alocação de strings da apresentação, os 13 testes do gateway e o lint foram repetidos e aprovados; o ajuste conserva o mesmo texto retornado.

## Claude Code e limites

O Mods registra a ferramenta e executa o gateway por `$.tool.call` sobre Bash, preservando as permissões do host. A busca não usa `process.run` fora desse fluxo. Hooks clássicos encaminham `Grep`/`Glob`/`Read` textual suportados e reescrevem comandos simples `rg`/`grep`/`git grep`, preservando argumentos, diretório e opções de execução. As proteções de leitura, segredos e configuração continuam valendo; cortes de leitura são encaminhados com o intervalo reduzido.

Hooks clássicos não podem trocar o nome da ferramenta. Ferramentas tipadas recebem instrução de encaminhamento com pedido completo e comando equivalente; a ferramenta registrada e as instruções do agente são o caminho direto. Comandos compostos, pipes, redirecionamentos, expansões de shell, opções desconhecidas, imagens/PDFs e ferramentas não suportadas seguem pela ferramenta original. Não há interceptação automática de 100% dos programas possíveis. O adaptador instalado do Codex permanece futuro; contrato e núcleo são independentes do host. Sem `rg` disponível, os hooks deixam `Grep`/`Glob` originais passarem, evitando um ciclo de encaminhamento impossível de executar.

Os formatos tipados são adaptadores do Mustard, sem promessa de reproduzir detalhes internos do host, como ordenação por data no Glob. A garantia de bytes/código de saída corresponde aos executáveis nativos. Saídas sem coordenadas reconhecíveis continuam úteis como resultado nativo, mas podem não alimentar fatos de linha nem receber cruzamento.

O enriquecimento/registro trabalha com até 256 ocorrências, 96 arquivos textuais de até 2 MiB, com orçamento agregado de leitura; até quatro proprietários enriquecidos no retorno inicial. A busca nativa não é filtrada por esses limites. Gramáticas ausentes, fontes excluídas/sensíveis e resultados omitidos impedem afirmar compreensão completa. Relações estáticas são pistas, não prova de autorização ou comportamento em execução.

Referências oficiais: [hooks](https://code.claude.com/docs/en/hooks), [Mods](https://code.claude.com/docs/en/plugins/mods), [referência de Mods](https://code.claude.com/docs/en/plugins/mods/reference), [eventos](https://code.claude.com/docs/en/plugins/mods/events) e [guia do Codex](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide).

## Validação e medição pendente

### Auditoria da apresentação em 09/10

A objeção do usuário identificou um defeito real: o shell recebia busca **mais** relatório de cruzamento, e o Mods recebia o envelope completo, com dados de aprendizado/contadores/comandos repetidos. Isso não escolhia entre resultado original e resultado cruzado. Foi corrigida a apresentação comum do CLI, Mods e encaminhamento clássico; o banco/evidência detalhada continuam disponíveis e o aprendizado permanece ativo. As instruções distribuídas nas duas línguas agora usam `--shell-output`. Esse ajuste segue a orientação de [administrar contexto e carregar detalhes sob demanda](https://code.claude.com/docs/en/best-practices), preservando os [contratos dos hooks](https://code.claude.com/docs/en/hooks).

`apps/scan/benchmarks/gateway-efficiency.mjs` congela dez pesquisas convencionais sobre a cópia autorizada do backend Florestal `a3fe37ab454c`. Compara a mesma execução/argumentos, alternando busca direta, gateway anterior `3143f4a8` e apresentação corrigida. Decodifica a saída agrupada e exige restauração byte a byte de todas as ocorrências, ordem, duplicatas, stderr e exit code. São casos conhecidos de desenvolvimento; não avaliam seleção por pergunta de negócio nem uma implementação por modelo.

Na primeira medição, busca direta: **26.574 bytes**; shell anterior: **62.165**; envelope Mods anterior: **78.655**; apresentação corrigida: **18.673**. Redução de **29,7% em relação à busca direta**, **70,0% em relação ao shell anterior** e **76,3% em relação ao envelope anterior**. Dez de dez casos preservaram os resultados. Seis buscas incluíram referências estáticas, com funções/faixas, e quatro conservaram a saída original. Zero chamadas a modelos. Não converter esses percentuais em tokens faturados/economia total: o custo das instruções, esquema, encaminhamento, novas leituras e turnos também precisa entrar no teste real.

A mudança é de apresentação. A latência mediana exploratória ficou próxima de **73 ms**, contra **72 ms** no gateway anterior e **21 ms** na busca direta, com executáveis de desenvolvimento, três rodadas alternadas e uma máquina. O cruzamento/validação/persistência continuam acrescentando trabalho; não foi demonstrada aceleração. Scan inicial/atualização não estão incluídos nessa mediana aquecida. Relatório com hashes/tempos por caso: `target/scan-gateway-efficiency-20261009/benchmark.json`. A medição quantitativa anterior de `knowledge` continua histórica e não é usada como medição deste gateway.

Aceite após a auditoria: **3.889 testes Rust aprovados**, dois ignorados herdados, lint estrito e **13 testes oficiais do Mods aprovados**. A apresentação acrescenta três testes de preservação/tamanho/paginação e uma prova oficial de transporte de stderr. Prova com instalação nativa vazia e zero chamadas de IA também verifica recuperação após falha do scan e expansão de símbolos na própria worktree. A versão antiga de substituição foi retirada do executável; uma fixture histórica, compilada somente nos testes, preserva os comparativos anteriores.

`apps/scan/tests/search_gateway.rs` verifica paridade nativa, proprietários/intervalos atuais, paginação, novas versões, persistência e escolha apenas em responsabilidades ainda não resolvidas e autorizadas. Testes do runtime exercitam o gancho completo e o CLI; testes oficiais do Mods conferem transporte pelo host e preservação de recusas.

`node apps/scan/benchmarks/gateway-acceptance.mjs` cria uma pasta realmente vazia, instala com o binário local e verifica descoberta, atualização automática, deduplicação sem novo scan, fallback, ferramentas tipadas, hooks e isolamento de worktrees. Não executa código do repositório pesquisado nem chama API paga. Relatório e hashes: `target/scan-gateway-20261009/native-acceptance.json`, separado dos benchmarks históricos.

Os testes comprovam funcionamento; a comparação de apresentação acima mede bytes das mesmas pesquisas, sem economia faturada ou melhora no código produzido. Falta comparar tarefas equivalentes em sessões reais: buscas adicionais, contexto total, custo, tempo e implementação correta. Uma sessão autenticada também deve confirmar a ergonomia do encaminhamento no Claude Code.
