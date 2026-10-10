# Busca, ondas e Mods do Mustard: implementação e resultados — 10/10/2026

Implementação na branch `codex/mustard-plano-completo`, em uma cópia isolada. Baseline da revisão de recuperação: `e29ec6f3a750109f64b23d976e7a03c660b0338b`. O ajuste posterior de entrega/comportamento do modelo compara com `f7c80415a5a9c772f24fd93942af6c110b9b684e`, sem misturar os dois pontos de partida. A branch pessoal em alteração e os projetos de aplicação originais não foram modificados. Este documento substitui o estado corrente dos relatórios de 08/10 e 09/10, preservados como histórico.

**Histórico até `4947a105`:** a etapa anterior elevou os alvos entregues de 55/80 para 72/80. A revisão de entrega preservou exatamente esses 72 alvos e 49 corpos completos, reduzindo a apresentação de 1.356.648 para 1.228.267 bytes (**−9,46% adicional**). O modelo local fica habilitado por padrão nas pesquisas por intenção; Jev continua pontual e separado. Esses resultados medem evidência efetivamente entregue, com arquivo/faixa/hash conferidos; não medem precisão de todos os candidatos, implementação correta ou tokens faturados.

**Estado atual:** ondas usam evidências atuais do scan e vagas livres; Mods explica a fila; Jev evita decisões sem motivo concreto e reutiliza o cache; revisão final aproveita recibos sem reler saídas extensas. Recuperação preservada, sem ganho adicional de alvos nesta revisão. Uma tentativa de acelerar o ranking vetorial foi retirada por não demonstrar benefício.

## Revisão de ondas, scan e Mods sobre `4947a105`

Esta revisão usa `4947a105f60ffa8301ef33462976423b2a54700c` como baseline, com binários dos dois braços compilados com o perfil de desenvolvimento otimizado. Não mistura esse ponto de partida com os ganhos históricos abaixo. A implementação continua na branch `codex/mustard-plano-completo`, na cópia isolada; a branch pessoal `feature/validacao-leve-por-trecho` permanece intacta.

### Alterações implementadas

- **Agrupamento nativo informado pelo scan.** Uma consulta indexada carrega somente as importações dos arquivos declarados no quadro. A relação só entra quando os dois arquivos atuais coincidem com os blobs registrados no scan. Importador ou destino alterado invalida a relação; mapa ausente mantém o agrupamento pelos arquivos declarados, sem criar banco nem invocar modelo. Grupos relacionados podem compartilhar uma onda dentro da estimativa de tamanho. Grupos que escrevem o mesmo arquivo continuam atômicos. A estimativa usa a tabela existente de 35/65/95/125 mil tokens por tarefa, associando faixas ao número de arquivos: essa associação é heurística, não uma previsão calibrada desta revisão nem uma medição de consumo.
- **Vagas livres utilizadas.** Saiu a exigência de seis arquivos para uma onda pequena sair enquanto outra executa. Dependências, prioridade, critérios, limpeza por último, capacidade configurada, continuação de resumo e exclusividade da árvore inteira permanecem. Uma relação atual de leitura/escrita pode segurar um consumidor enquanto outra onda altera seu contrato; leitura compartilhada não bloqueia. Também se conferem relações entre ondas escolhidas na mesma rodada, incluindo continuação de resumo. No teste controlado, duas tarefas ligadas pelo scan saem em uma onda, e trabalho pequeno independente usa uma vaga livre. Isso não demonstra economia percentual de tokens: liberar trabalho independente pode aumentar o número de agentes simultâneos em relação à espera anterior.
- **Jev com motivo e cache.** Um critério global compartilhado, sozinho, deixou de provocar uma pergunta de interferência entre tarefas. Dependência explícita, leitura/escrita e vizinhança pertinente continuam como sinais. Se todas as tarefas estão impedidas por reservas nativas, o despachante nem chama o avaliador. Estados enriquecidos são reutilizados entre o agrupamento e o quadro pago; não há uma segunda leitura do scan para isso. Os perfis intrínsecos e decisões tipadas continuam com cache. Em quadros mistos ainda pode haver perfis de tarefas bloqueadas; esta revisão não declara ter eliminado toda pergunta evitável.
- **Validação final com recibo consolidado.** O pedido do revisor inclui, como leitura obrigatória, somente o recibo consolidado da validação final aprovada, identificado pelo conteúdo e pelas entradas validadas. Saídas completas de suítes já aprovadas não entram novamente no pedido. Um novo ensaio de validação sem conclusão ou uma falha posterior impede a reutilização do recibo antigo. Registros individuais continuam acessíveis. Os agentes em português e inglês orientam aproveitar a execução vigente e repetir verificações afetadas, incertas ou alteradas; preservam leitura independente, cortes que precisam falhar, prova manual e revisão final obrigatória.
- **Fila no Mods.** As abas de specs/execução mostram backlog, tarefas prontas, ocupação/capacidade e motivos declarados de espera: dependências, arquivo reservado, falta de vaga, critério ausente, limpeza e fase da spec. A lista mostra até 40 tarefas e informa o excedente. A observação é somente leitura, sem scan, banco novo ou modelo. Relações atuais de código são verificadas no despacho: a tela deixa explícito que sua prontidão descreve a fila declarada, não uma nova análise de fonte a cada atualização. Publicação externa permanece explícita.
- **Comentários e avaliação coerentes.** Foram corrigidas descrições que ainda prometiam publicação automática e retirada uma duplicação de teste. O runner de recuperação agora registra e imprime a configuração vetorial de ambos os braços; reutilização de baseline exige a mesma configuração registrada. Desligar o modelo no benchmark é uma ablação explícita, não o padrão do produto.

O uso do Jev segue decisões fechadas com critérios curtos e fatos estruturados, preservando abstenção e validação de resposta. Compartilhar o estado entre perguntas pertinentes continua útil, mas não torna perguntas adicionais gratuitas. Referências: [Choice](https://docs.typesafe.ai/primitives/choice), [State](https://docs.typesafe.ai/concepts/state) e [perguntas paralelas](https://docs.typesafe.ai/cookbooks/parallel_questions). Nesta validação não houve chamada paga ao Jev.

### Recuperação preservada e amostra nova

Foram repetidos os mesmos pedidos nos braços anterior e experimental da revisão, com inferência externa desabilitada. O braço experimental ainda continha a otimização de ranking posteriormente retirada; o pacote final restaura o ranking do baseline e recebe a conferência separada nos projetos abaixo. Os 240 pares de apresentações abaixo permaneceram idênticos byte a byte: 120 perguntas, testadas com e sem o modelo local. As coordenadas/hash do código e a paridade do resultado nativo foram conferidas. Os tempos dessas rodadas concorrentes não são usados como prova de desempenho.

| Amostra e configuração | Alvos antes → atual | Corpos completos antes → atual | Bytes antes → atual |
| --- | ---: | ---: | ---: |
| 80 casos anteriores, modelo local ligado | 72/80 → 72/80 (90%) | 49 → 49 | 1.228.267 → 1.228.267 |
| 80 casos anteriores, modelo desligado | 64/80 → 64/80 (80%) | 41 → 41 | 1.284.141 → 1.284.141 |
| 40 casos novos, modelo local ligado | 34/40 → 34/40 (85%) | 27 → 27 | 693.421 → 693.421 |
| 40 casos novos, modelo desligado | 31/40 → 31/40 (77,5%) | 24 → 24 | 728.293 → 728.293 |

Os 40 casos novos foram selecionados depois de congelar os binários de recuperação, sem ajustar o algoritmo pelas respostas: primeiro repositório de cada linguagem ainda ausente das 80 perguntas anteriores, dez perguntas por repositório. São `psf/black`, `xenova/transformers.js`, `rust-bakery/nom` e `junegunn/fzf`, nos commits fixados na seleção versionada. O modelo local recuperou três alvos adicionais nessa amostra, sem perder alvos da ablação nativa: Python 8/10 → 9/10, TypeScript 9/10 → 10/10, Rust 5/10 → 5/10 e Go 9/10 → 10/10. Isso sustenta manter o canal local por intenção; não demonstra ganho adicional de recuperação desta revisão sobre `4947a105`, que já usava esse canal.

São testes diagnósticos adaptados com descrições públicas e bootstrap lexical fixo, não buscas autônomas de um agente nem a pontuação oficial do RepoQA. Sobreposição do treino do modelo com esses repositórios não foi auditada. Alvo entregue não mede precisão de todos os candidatos ou implementação correta. Os seis alvos ausentes nos casos novos, incluindo cinco em Rust, continuam lacunas reais.

O pacote final foi reconstruído depois do ajuste do recibo e repetiu as quinze consultas dos projetos, sem qualquer diferença de texto em relação ao baseline:

| Projeto | Alvos antes → atual | Corpos completos | Retorno inicial, antes → atual |
| --- | ---: | ---: | ---: |
| Sialia | 10/12 → 10/12 | 3 → 3 | 149.834 → 149.834 bytes |
| Florestal | 8/8 → 8/8 | 3 → 3 | 54.388 → 54.388 bytes |

No Sialia, tokenização (`RequestTokenizationAsync`) e desativação de subconta (`DisableSubAccountAsync`) continuam fora dos alvos entregues pelos pedidos medidos. Nenhum pagamento, aplicação ou exportação real de planilha foi executado. A passagem dos testes de recuperação não certifica esses fluxos de negócio.

### Experimento de tempo retirado

A tentativa de reduzir cópias, recalcular menos normas e selecionar o top K exato manteve as respostas, mas não demonstrou benefício de ponta a ponta. Depois de terminar a compilação, testes e rodadas de recuperação, quatro pedidos aquecidos receberam três repetições por braço, alternadas, no mesmo snapshot. As 24 saídas foram determinísticas e iguais entre braços. A mediana agregada passou de 4.036 para 4.179 ms (**+3,54%**).

| Consulta | Baseline | Experimento | Variação |
| --- | ---: | ---: | ---: |
| Sialia: visão do fluxo | 6.121 ms | 6.407 ms | +4,68% |
| Sialia: cobranças | 8.052 ms | 8.052 ms | −0,01% |
| Florestal: descoberta de XLSX | 2.005 ms | 1.971 ms | −1,69% |
| Florestal: entradas do input | 432 ms | 431 ms | −0,22% |

Essa amostra pequena não prova uma regressão geral, mas tampouco justifica manter a complexidade adicional. **O experimento foi removido** e o pacote final voltou ao cálculo/ranking anterior. Esses tempos pertencem ao experimento, não ao pacote final. Não se afirma aceleração final da busca nesta revisão. Artefatos preservados: `latency-experiment.json`, `latency-experiment-manifest.json` e `experiment-bin/`; hashes distinguem esse binário do pacote entregue. O manifesto original registra os caminhos usados na medição; `latency-reproduce-experiment-manifest.json` aponta para o binário congelado, preservado depois da retirada.

### Verificações e limites

Passaram **3.995 testes Rust**, com dois ignorados já existentes, Clippy com `-D warnings`, construção otimizada e **16 testes oficiais do Mods no pacote final**, além da validação do plugin. Os testes novos conferem frescor dos dois lados da relação, reserva de consumidor, leitura compartilhada, agrupamento transitivo/orçamento, fila sem escrita e recibo. No serviço Jev simulado, o quadro com somente critério global compartilhado fez um perfil e zero perguntas de interferência; repetição idêntica fez zero chamadas, e acrescentar uma relação real exigiu apenas a decisão ainda ausente. Um teste do despachante recusa qualquer chamada ao avaliador quando tudo já está bloqueado nativamente.

O preflight gratuito do pacote final passou nas duas tarefas autorais, incluindo teste independente do gabarito e localização pelo gateway; **nenhum modelo de host executou as tarefas**. O CLI segue `loggedIn:false`. Tokens faturados, custo de uma spec completa, qualidade do código produzido e tempo total de implementação continuam sem medição. A revisão também não elimina limites de dependências dinâmicas ou de arquivos declarados incorretamente nas tarefas.

O próximo trabalho no scan deve atacar lacunas de recuperação e custo de inventário/hidratação antes de acrescentar canais indiscriminadamente. Um perfil diagnóstico da visão geral do Sialia, com índice de cerca de 478 MB, mostrou muitas leituras SQLite e resoluções de caminhos; não há base para atribuir todo o custo ao cálculo vetorial. Qualquer cache de caminhos precisa preservar a verificação de escopo, links e código atual. A outra prioridade de validação é uma trajetória real do Claude, com acerto da tarefa e consumo medidos nos dois braços. Não se apresenta uma porcentagem única de melhoria para misturar esses resultados.

Artefatos locais desta revisão ficam em `target/waves-scan-mods-20261010/`: `vector-{known,heldout,new-cases}/comparison.json`, ablações em `{known,heldout,new-cases}/`, `*-delivered-labels.json`, `session-delivered/`, logs e binários congelados. `corpus-bin/` preserva o executável dos 120 casos; `current-bin/` e `target/review-plugin/` contêm o pacote final com o recibo consolidado. O experimento de otimização vetorial preservou a recuperação, mas foi retirado por não demonstrar aceleração; o pacote final usa novamente o mesmo ranking do baseline. A repetição dos quinze pedidos dos projetos confere o pacote entregue. Manifestos/hashes distinguem essas versões. Os resumos numéricos ficam em [measurements/2026-10-10-waves-scan-mods.json](measurements/2026-10-10-waves-scan-mods.json). Nenhuma instalação pessoal ou publicação foi alterada.

## Prioridades e alterações mantidas

1. **Cobertura antes de reranqueamento.** Declarações de testes antes excluídas passaram a entrar no catálogo, identificadas por `test_only`. A visão do agente e a evidência enviada ao Jev distinguem testes de produção. Código de teste indexado não prova execução ou cobertura.
2. **Admissão e fusão antes dos cortes.** Âncoras exatas permanecem prioritárias. Líderes dos canais independentes são preservados antes do consenso por RRF; vistas lexicais correlacionadas não ganham votos extras contra o canal de código apenas por serem mais numerosas. O reservatório interno de fontes foi separado do orçamento de apresentação. Continuam os limites de 96 arquivos, 12 MiB agregados e 2 MiB por arquivo; o corte dependente de dois segundos foi retirado porque alterava os candidatos conforme a carga da máquina. A investigação continua parcial e informa suas lacunas.
3. **Trechos estruturais.** O parser registra faixas de bytes/linhas por fronteiras da AST, dividindo nós grandes e reunindo irmãos contíguos. Os trechos priorizam essas fronteiras, conservam os intervalos ainda não lidos e indicam folhas excessivas. Não há resumo inventado. Os conceitos de divisão/reunião estrutural foram adaptados de [cAST](https://aclanthology.org/2025.findings-emnlp.430/), usando o Tree-sitter já existente.
4. **Encadeamento útil.** Âncoras explícitas e o primeiro destino elegível conservam prioridade; os demais pontos de partida favorecem dependências estáticas com alvo único. Vizinhos incidentais que não entraram na seleção da tarefa não consomem os espaços de investigação. Interfaces/tipos ainda podem levar aos seus membros e implementações. Isso recuperou o contexto do frontend e preservou o roteamento de provedor do Sialia.
5. **Busca vetorial local sobre código real, habilitada por padrão.** Um canal independente consulta o índice inteiro dentro do escopo, antes do corte lexical de candidatos. Chunks são agrupados por símbolo antes do corte, evitando que uma classe grande ocupe todos os destinos. Só depois se hidratam os candidatos e se confere o código atual. Não há servidor de modelos, download em runtime, Ollama ou API paga nesse caminho.
6. **Menos repetição na apresentação e no Jev.** Caminhos e qualificadores repetidos do grafo são agrupados, preservando as relações e coordenadas. `knowledge-choice-v7` retira campos vazios e assinaturas/documentação já presentes literalmente no trecho numerado; conserva documentação distinta, hashes, alternativas, abstenção e os critérios de aceitação anteriores. Candidatos/trechos continuam compartilhados entre perguntas quando idênticos, conforme os conceitos de [Choice](https://docs.typesafe.ai/primitives/choice) e [State](https://docs.typesafe.ai/concepts/state).
7. **Medição da entrega real.** O avaliador confere a apresentação do agente, não apenas o JSON interno. Um modo diagnóstico de projeção evita o rastreamento caro de todos os candidatos; a primeira pergunta de cada repositório compara essa projeção byte a byte com a saída normal. O rastreamento completo continua disponível explicitamente.
8. **Hidratação seletiva do banco.** A recuperação passa pelos campos e declarações não solicitados sem construir seus objetos em memória. Só desserializa os cards nas posições pedidas, mantendo validação do JSON e conferência de identidade/posição. Não acrescenta uma cópia persistente dos dados. A alteração foi mantida após preservar a saída e reduzir a latência da versão intermediária.

Nenhuma regra de negócio, nome de fornecedor, projeto ou gabarito foi introduzido no algoritmo. A implementação reutiliza os parsers, SQLite e `model2vec-rs` existentes. O núcleo/contrato permanecem independentes do host; o adaptador instalado para Codex continua sendo uma etapa futura.

## Revisão final: retorno que o agente consegue usar

Comparação com a versão `f7c80415`, nos mesmos pedidos e snapshots; os dois braços usam o modelo local. As apresentações do baseline foram reutilizadas com conferência dos hashes do dataset, seleção e binário; a versão atual repetiu as 80 consultas. Não se compararam tempos dessas execuções concorrentes como desempenho.

| Medida | Antes desta revisão | Atual |
| --- | ---: | ---: |
| Alvos com corpo/referência entregue | 72/80 | 72/80 |
| Corpos esperados completos | 49/80 | 49/80 |
| Bytes entregues nas 80 buscas | 1.356.648 | 1.228.267 (−9,46%) |
| Alvos/corpos anteriores perdidos | — | 0 / 0 |
| Paridade do resultado nativo | 80/80 | 80/80 |
| Chamadas externas das consultas | 0 | 0 |

As quatro frentes foram implementadas:

1. **Retorno inicial navegável.** Caminhos de arquivos são agrupados; destinos estáticos conservam nome e faixa de linhas, com expansão sob demanda. Assinaturas extensas, documentação e metadados continuam no banco/diagnóstico. As referências não são apresentadas como prova de comportamento. Candidatos adiados indicam uma única instrução de expansão, seguida dos arquivos/contagens.
2. **Modelo onde há busca por intenção.** O canal local deixa de ser ignorado por existir qualquer âncora nominal; identidade única resolvida, pesquisa literal e leitura exata conservam o caminho direto. Perguntas independentes explícitas recebem embeddings em lote e fusão por posição, preservando líderes de cada pergunta. Listas de objetivo/entrada/saída/procedimento continuam uma pergunta. Listas numeradas só se desdobram quando cada item é uma pergunta terminada em `?`; o schema do host orienta esse contrato.
3. **Reutilização de linhas confirmadas.** Na mesma resposta, linhas idênticas do mesmo arquivo/hash aparecem uma vez quando a indicação de reutilização ocupa menos espaço. Entre respostas, somente fonte atual efetivamente confirmada pelo adaptador é reutilizada, com isolamento por checkout/sessão/agente/época. Linhas cortadas no meio não viram recibo completo. Mudança de código ou compactação invalida a reutilização; `Read` explícito continua devolvendo o trecho pedido. O armazenamento do controle é limitado; esquecer evidência resulta em entregá-la novamente.
4. **Custo da evidência completa e trajetória preparada.** O avaliador confere as linhas realmente presentes na apresentação e executa leituras reais das faixas faltantes. O runner de sessões registra uso do modelo, resultados de ferramentas e intervalos solicitados repetidamente, exige conclusão correta de ambos os braços e verifica o uso do gateway antes de calcular economia.

### Projetos do usuário: reduzir texto sem transferir o custo para a expansão

| Medida | Sialia, antes → atual | Florestal, antes → atual |
| --- | ---: | ---: |
| Alvos localizados | 10/12 → 10/12 | 8/8 → 8/8 |
| Corpos exigidos completos no retorno inicial | 3 → 3 | 3 → 3 |
| Retorno inicial, bytes | 169.562 → 149.834 (−11,63%) | 59.453 → 54.388 (−8,52%) |
| Leituras de faixas faltantes | 8 → 8 | 5 → 5 |
| Retorno inicial + leituras, bytes | 195.613 → 175.885 (−10,09%) | 67.131 → 62.066 (−7,54%) |

O custo expandido compara os **mesmos dez alvos localizados** do Sialia e os oito do Florestal. Os dois pontos ainda não localizados no Sialia ficam explícitos e não são declarados resolvidos. As faixas esperadas são conhecidas pelo avaliador somente depois da busca inicial: é um limite inferior de custo de entrega, orientado pelas respostas conhecidas, **não uma trajetória autônoma do Claude**. Não se executaram aplicações, pagamentos, geração/reimportação de planilhas ou bancos de negócio.

Não houve ganho adicional de recuperação nesses replays. O ganho confirmado desta revisão é menos texto mantendo as referências e corpos anteriores; nenhuma economia de tokens faturados é inferida desses bytes. A reutilização entre turnos foi verificada por testes de recibo/isolamento; seu efeito em sessões reais ainda não foi medido.

### Tempo: melhora agregada, custo desigual

Depois dos testes e consultas de qualidade, sem compilação ou outro benchmark concorrente, foram feitos um aquecimento por pergunta/braço e três repetições alternadas de quatro consultas conhecidas. Os dois braços usam vetores locais no mesmo snapshot descartável. As 24 saídas medidas foram determinísticas por braço/pergunta. A mediana agregada passou de **4.954 para 4.645 ms (−6,24%)**.

| Consulta | Antes, mediana | Atual, mediana | Variação |
| --- | ---: | ---: | ---: |
| Sialia: visão do fluxo | 7.344 ms | 7.018 ms | −4,43% |
| Sialia: cobranças | 9.613 ms | 8.952 ms | −6,88% |
| Florestal: descoberta de XLSX | 2.114 ms | 2.717 ms | +28,53% |
| Florestal: entradas do input | 468 ms | 475 ms | +1,62% |

A assistência local e a verificação/atualização do índice têm custo: não houve aceleração uniforme. A amostra pequena, caches aquecidos e uma só máquina não permitem extrapolar desempenho geral ou duração de uma tarefa completa. O primeiro preenchimento do índice local não está incluído nessa medição aquecida. Manifesto, binários e amostras ficam em `latency-manifest.json` e `latency-verified.json` na pasta local desta revisão.

### Experimentos desta revisão

Um canal lexical BM25 adicional foi testado junto com mudanças de decomposição: a combinação caiu de 36/40 para 34/40 referências na seleção conhecida. O canal foi retirado, sem alegar uma ablação isolada que provasse a causa. A revisão seguinte mostrou que listas descritivas numeradas também estavam sendo decompostas incorretamente; corrigida essa distinção, o resultado final preservou individualmente os 72 alvos e 49 corpos completos. O BM25/FTS já existente permanece; o protótipo de canal adicional não entrou no produto.

### Limite da validação no host

O preflight gratuito de duas tarefas completas, os testes do plugin e as verificações de transporte passaram. A execução autônoma pareada continua pendente: o CLI local informa `loggedIn:false`, e o runner `--bare` precisa de `ANTHROPIC_API_KEY` e do mesmo identificador completo de modelo nos dois braços. Não há valores medidos de tokens faturados, custo total ou qualidade de implementação produzida pelo Claude/Codex. O contrato é compartilhado com futuros adaptadores; esta revisão não instala integração do Codex.

Artefatos locais da revisão: `target/search-delivery-20261010/delivery-summary.json`, `verified-{known,unseen}/comparison.json`, `verified-{sialia,florestal}-labels.json`, `verified-{sialia,florestal}-evidence-cost.json`, logs finais e binários congelados em `verified-bin/`. Os runners ficam versionados; nenhum artefato é publicado automaticamente.

## Histórico da recuperação antes do ajuste final de entrega

Mesmas 80 descrições e pedidos nos três braços, em oito repositórios, 20 casos por linguagem. Baseline e versão final foram compilados com `CARGO_PROFILE_DEV_OPT_LEVEL=3`, usando binários congelados distintos. A comparação de qualidade não usa tempos coletados sob concorrência/diagnóstico como comparação de desempenho.

| Medida | Baseline | Final nativo, sem modelo | Final com modelo local opcional |
| --- | ---: | ---: | ---: |
| Alvos com referência/corpo entregue | 55/80 (68,75%) | 64/80 (80%) | 72/80 (90%) |
| Ganho absoluto de recuperação | — | +11,25 pontos percentuais | +21,25 pontos percentuais |
| Novos alvos / alvos anteriores perdidos | — | 9 / 0 | 17 / 0 |
| Corpos esperados completos na visão inicial | 40/80 | 41/80 | 49/80 |
| Bytes da apresentação | 1.596.596 | 1.407.348 | 1.356.648 |
| Redução de bytes | — | 11,85% | 15,03% |
| Paridade do resultado original | 80/80 | 80/80 | 80/80 |
| Chamadas externas nas buscas | 0 | 0 | 0 |

O modelo acrescentou oito alvos entregues sobre a versão nativa; a recuperação relativa ao baseline cresceu 30,9%, e os alvos ausentes caíram de 25 para oito (−68%). Esses percentuais não são economia financeira nem taxa de sucesso de uma tarefa completa.

| Linguagem, 20 casos cada | Baseline | Final nativo | Final com modelo local |
| --- | ---: | ---: | ---: |
| Python | 14 | 16 | 19 |
| TypeScript | 16 | 16 | 17 |
| Rust | 14 | 15 | 18 |
| Go | 11 | 17 | 18 |

**Tradeoff da entrega de corpos:** os 49 completos com modelo resultam de 16 ganhos e sete perdas individuais de completude; esses sete alvos continuam referenciados, mas exigem expansão. No nativo são 11 ganhos e dez perdas de completude. Em Rust nativo, os corpos completos caíram de 11 para nove. A política favorece referências relevantes e trechos menores; não se deve inferir que toda leitura posterior foi eliminada. A tentativa de empacotar mais corpos foi retirada por aumentar muito o texto sem melhorar a recuperação.

### Método e limites da amostra

Dataset público RepoQA, release `2024-06-23`, SHA-256 do JSON `bd3f7cab47283cdeccee20daea31af587b680cf8f9db192ab4da1037730cd6e2`. As seleções estão em `apps/scan/benchmarks/repoqa-selection-fusion-20261009.json` e `repoqa-selection-local-code-20261010.json`, com commits/hashes. São PyG, LangChain.js, Cargo, croc, ReactPy, date-fns, Serde e go-zero.

Cada pedido usa as primeiras 12 palavras distintas elegíveis da descrição como expressão de pesquisa nativa e a descrição como intenção; não recebe o caminho, nome esperado ou gabarito. O avaliador usa os rótulos separadamente. Compara hash/fonte/faixa, apresentação visível, completude e o resultado nativo. O conjunto novo de 40 foi congelado antes desta implementação, mas repetido e usado para diagnosticar falhas durante a revisão: o resultado final é um diagnóstico sobre seleção congelada, não uma avaliação cega independente. Também não é o protocolo oficial RepoQA nem uma sessão autônoma de Claude/Codex. A sobreposição entre treinamento do modelo público e repositórios não foi auditada.

### Projetos fornecidos pelo usuário

| Replay conhecido | Antes | Final com modelo local |
| --- | ---: | ---: |
| Sialia: alvos visíveis | 10/12 | 10/12 |
| Sialia: corpos completos exigidos | 3 | 3 |
| Sialia: bytes em sete pedidos | 165.973 | 169.562 (+2,16%) |
| Florestal: alvos visíveis | 8/8 | 8/8 |
| Florestal: corpos completos exigidos | 3 | 3 |
| Florestal: bytes em oito pedidos | 66.616 | 59.453 (−10,75%) |

No Sialia não houve ganho ou perda de alvo: permanece a lacuna de dois pontos exigidos, e o retorno ainda cresce ligeiramente. A compactação do grafo reduziu a versão intermediária de 174.529 para 169.562 bytes, preservando os mesmos pontos. Não se certificou prontidão do fluxo de pagamentos nem viabilidade contratual do Asaas. No Florestal os oito pontos esperados foram preservados com menos texto; isso não testa geração/reimportação de XLSX.

Os testes usaram cópias descartáveis do Sialia (`main` `810c7937081d7a3fd2a7db8332ec488dcaeb0635`; backend `1ff44e04de2fe0d112f66eee89d38f8a902a7f4b`) e Florestal (`a3fe37ab454cede37d3471993eb876985fcdfe1b`). Nenhuma aplicação, API de pagamento ou banco de negócio foi executado; nenhuma fonte desses projetos foi enviada ao Jev.

### Latência: custo que permanece

Quatro pedidos conhecidos — visão geral/cobranças do Sialia e descoberta/entrada XLSX do Florestal —, uma execução de aquecimento por pedido/braço, três rodadas com braços alternados, 12 amostras por braço, diagnóstico desligado e saída real `--shell-output`. Os índices foram preparados por seus próprios binários, com a mesma otimização de compilação. Não havia outra compilação ou benchmark em execução. É uma amostra pequena, em uma máquina e com cache de filesystem aquecido; não mede scan inicial nem tempo de uma implementação por agente.

| Mediana por pedido | Baseline | Final nativo | Final com modelo local |
| --- | ---: | ---: | ---: |
| Sialia: visão geral | 4.732 ms | 5.156 ms | 6.215 ms |
| Sialia: cobranças | 5.825 ms | 7.236 ms | 7.838 ms |
| Florestal: descoberta XLSX | 1.661 ms | 1.865 ms | 1.952 ms |
| Florestal: entrada XLSX | 340 ms | 341 ms | 422 ms |
| Mediana das 12 execuções | 3.195 ms | 3.485 ms (+9,06%) | 4.076 ms (+27,54%) |

Não houve aceleração frente ao baseline. O ganho de recuperação veio com maior trabalho de consulta/verificação. O modelo local fica opcional também por esse custo. A primeira versão desta revisão, antes da hidratação seletiva, mediu 3.780 ms no nativo e 4.343 ms com modelo; a mudança reduziu essas medianas em **7,81% e 6,16%**, respectivamente. A saída permaneceu byte a byte idêntica nas 36 amostras comparadas entre as duas versões, e todas as repetições produziram saída determinística. Os baselines das duas medições ficaram próximos, em 3.207 e 3.195 ms. As penalidades restantes não são escondidas como economia de tempo.

## Modelo local e banco

Dados incorporados: [minishlab/potion-code-16M-v2](https://huggingface.co/minishlab/potion-code-16M-v2/tree/e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b), revisão fixa `e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b`, licença MIT declarada no model card. São embeddings estáticos de código com 256 dimensões; pesos/config/tokenizador somam **33.514.471 bytes (33,5 MB)**. Proveniência, hashes, model card e atribuição acompanham o repositório e o plugin. Foram incorporados os dados do modelo; a biblioteca de inferência Rust já era dependência do Mustard. Não foi incorporado o projeto Semble ou um novo framework de inferência.

O bloco SQLite `code-meaning` armazena vetores int8 por símbolo/chunk, hash da fonte e impressão digital da análise/modelo. Atualiza somente arquivos alterados, remove arquivos desaparecidos e rejeita recibos antigos. A pontuação por cosseno lê o índice escopado. Antes da busca por intenção, o gateway atualiza incrementalmente os vetores dos documentos alterados; compara impressões digitais no SQL sem carregar todos os pacotes JSON inalterados. Os recibos são revalidados no código atual depois da recuperação. A atualização estrutural continua nativa e vetores antigos ficam inelegíveis; falha do índice local conserva a alternativa lexical. O diagnóstico registra todas as tentativas de atualização e a soma dos vetores calculados.

No Sialia foram 39.167 vetores, com 10.026.752 bytes de componentes; o banco inteiro passou de aproximadamente 374,3 MB para 500,1 MB. No Florestal, 15.657 vetores, 4.008.192 bytes de componentes, banco de 78,0 MB para 108,3 MB. O aumento do banco inclui também testes agora indexados, fronteiras da AST, índices/IDs e o índice semântico genérico anterior que continua usado por outros comandos. Não é todo atribuído aos 256 bytes de cada vetor. O `mustard-rt` de desenvolvimento otimizado cresce cerca de 33,5 MB com os dados incorporados; não há serviço residente adicional.

O armazenamento vetorial em SQLite foi mantido: estes ensaios não justificaram introduzir outro serviço ou um índice ANN com aproximação. Isso não prova que uma varredura de cosseno seja adequada para qualquer escala; o tempo por consulta e a cardinalidade devem continuar sendo medidos.

### Ativação e contrato

O padrão agora usa **o modelo local nas buscas por intenção, sem chamada paga**. `ai.vectors` ausente herda `true`; `false` desliga explicitamente. Jev permanece separado e precisa de habilitação e solicitação. Configuração explícita equivalente ao padrão:

```json
{"ai":{"vectors":true,"fallback":false}}
```

O scan normal prepara o índice; uma pesquisa por intenção também completa/atualiza o índice local já existente, após a atualização estrutural necessária. Na cópia de desenvolvimento, a entrada direta do scan é:

```sh
target/debug/scan scan /caminho/do/projeto --out /caminho/do/projeto/.claude/grain.db --json
```

`--native` conserva a atualização puramente estrutural; `ai.vectors:false` desliga o uso dos modelos locais. A chave do Jev não é necessária para embeddings. Pesquisa literal, leitura explícita e identidade exata única resolvida seguem pelo caminho direto. Perguntas com várias âncoras podem receber assistência vetorial. O modelo escolhe destinos prováveis; não produz conclusões sobre comportamento de negócio.

O contrato do gateway continua `schema_version:1`, `request:{tool,input,intent,purpose,choose?}`. O resultado nativo permanece recuperável por `purpose:locate`/`--raw`, e falhas/ausência de evidência útil preservam o fallback. Mods executa a pesquisa pelo fluxo de ferramentas/permissões do host. Os limites dos hooks clássicos, comandos compostos e formatos não suportados permanecem documentados no relatório de 09/10; não há promessa de interceptar qualquer programa possível.

## Jev: papel e custo medido

Jev continua atrás de `SymbolSelector`, usado para **escolher/classificar alternativas com evidência atual**, quando solicitado e habilitado. Não gera o scan, não faz busca lexical, não calcula consumo, não renderiza o painel e não publica páginas. Identidades exatas, atualizações nativas, chamadas comuns e cache não precisam do provedor. Nenhum limiar foi relaxado para aumentar os resultados.

Na fixture de 12 comparações pequenas, seis de calibração e seis reservadas, a mudança v6 → v7 preservou **12/12 resultados concordantes com as expectativas**, incluindo três `no-match`, um caso abaixo da aceitação e nenhuma escolha errada aceita. Com os mesmos pedidos e 12 chamadas, a entrada conhecida caiu de **12.398 para 11.726 tokens (−5,42%)**; estimativa de custo de US$ 0,000523 para US$ 0,000490. É uma amostra preparada e pequena, sem prova de calibração geral.

O piloto público final usa as primeiras duas descrições de cada um dos quatro repositórios da seleção de 09/10, sem rótulos no pedido/provedor. Em oito perguntas, o alvo estava no conjunto de alternativas em sete: **sete escolhas corretas, zero erradas aceitas e uma abstenção**. O primeiro destino correto passou de 1/8 na ordem nativa para 7/8 após a escolha. Foram **nove chamadas físicas**, incluindo refinamento, **178.232 tokens de entrada conhecidos**, **US$ 0,007485 estimados**; oito repetições usaram cache com zero HTTP. Esse custo não é comparável aos 12 casos curtos, nem demonstra menor custo de sessão.

A revisão anterior, contando a repetição de calibração anterior à recompilação e os dois pilotos públicos, registrou **54 chamadas**, **392.902 tokens conhecidos** e **US$ 0,016503 estimados**. Nenhuma tentativa foi apagada da conta. A revisão de entrega descrita acima acrescentou **zero chamadas pagas ao Jev**. Valores seguem o preço configurado, não uma fatura verificada. O piloto público anterior à identificação explícita de testes teve o mesmo resultado qualitativo. A revisão final acrescentou a indicação `test_only` à evidência; por isso seus tokens diferem ligeiramente.

## Ensaios retirados ou não incorporados

- **Empacotar mais corpos completos:** na seleção nova de 40, conservou 36 alvos, mas aumentou o texto de 522.408 para 702.550 bytes (**+34,5%**). Foi removido. A entrega final mantém referências e permite expandir os intervalos que faltam.
- **Jina Turbo como reranqueador local:** protótipo de dez perguntas Go caiu de 5/10 para 4/10 no primeiro lugar e de 9/10 para 8/10 nos cinco primeiros; aproximadamente 70,1 s. Não entrou no produto.
- **Jina embeddings de código, maior:** protótipo Go chegou a 10/10 nos cinco primeiros contra 9/10 do modelo estático, mas levou aproximadamente 199,8 s, usou cerca de 1,29 GB de memória de pico e 615 MB de cache. O ganho estreito não justificou incorporá-lo.
- **Priorizar todos os alvos com dependência antes do primeiro destino:** uma política intermediária perdia o roteamento no Sialia. Foi substituída pela ordem que preserva âncoras/líder e só depois favorece dependências úteis.
- **Rastreamento completo durante toda a avaliação/uso comum:** permanece optativo para diagnóstico; os ensaios finais usam projeção exata sem o rastreamento caro por candidato. A latência é medida separadamente, com diagnóstico desligado.
- **Ollama, modelo gerador de resumos e banco vetorial externo:** não incorporados. O resultado atual não demonstrou necessidade deles. O scan continua um catálogo de evidências, sem inventar um documento completo de regras de negócio por heurística.

## Verificação e reprodução

Passaram **3.987 testes Rust**, com dois ignorados herdados, Clippy com `-D warnings`, **16 testes oficiais do plugin Mods**, validação do plugin, sintaxe dos runners e conferência dos SHA-256 dos dados do modelo. A aceitação ponta a ponta instala usando o binário absoluto desta cópia em uma pasta realmente vazia e verifica atualização, fallback, paridade, ferramentas tipadas, aprendizado, expansão, reutilização de corpos e zero HTTP no caminho literal e nativo; a aceitação separada `gateway-local-model.mjs` verifica o novo padrão local e a atualização dos recibos.

```sh
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo build --workspace
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo test --workspace
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo clippy --workspace --all-targets -- -D warnings
node apps/scan/benchmarks/gateway-acceptance.mjs
node apps/scan/benchmarks/gateway-local-model.mjs
claude plugin validate plugin
claude plugin test plugin
```

Artefatos locais em `target/search-priority-20261010/`: `delivery-summary.json`, `delivery-{known,unseen}{,-native}/comparison.json`, repetição final `selective-{known,unseen}{,-native}/comparison.json`, apresentações por pergunta, binários congelados, `sialia-selective-labels.json`, `florestal-selective.json`, pilotos Jev com uso físico, logs de testes, `latency-delivery.json` e `latency-selective.json`. As 160 apresentações da repetição final permaneceram byte a byte idênticas às da etapa anterior à hidratação seletiva. Não são publicados automaticamente. Os manifests de seleção/fixtures/scripts, [resumo numérico com proveniência](measurements/2026-10-10-search.json) e este relatório ficam versionados. O pacote de revisão local fica em `target/review-plugin/`, com manifesto/hashes próprios; não substitui uma instalação pessoal.

Reprodução de uma seleção, com binários anterior e final separados:

```sh
node apps/scan/benchmarks/gateway-heldout.mjs \
  --dataset /caminho/repoqa.json \
  --selection apps/scan/benchmarks/repoqa-selection-local-code-20261010.json \
  --baseline /caminho/bin-anterior --current /caminho/bin-final \
  --vectors true --baseline-vectors true --out /caminho/resultados
```

Trocar `--vectors true` por `false` compara o caminho nativo. Para comparar ambos os braços com modelo, usar também `--baseline-vectors true`. O runner remove credenciais e exige zero chamadas externas. O piloto Jev é separado e exige `--jev`, configuração habilitada e credencial; nunca salvar a chave nos relatórios. A latência usa `gateway-latency.mjs`, roots já escaneadas por seus próprios binários e manifesto de cópias descartáveis, com aquecimento e braços alternados.

O painel/mods, statusline e publicação externa somente mediante solicitação explícita continuam com seus fluxos existentes. Não foi reintroduzida publicação automática. O que ainda depende de uso real é a ergonomia no Claude, redução de buscas/leituras, tokens totais faturados e qualidade da implementação produzida. Os ganhos demonstrados aqui são recuperação, tamanho da apresentação e escolhas pontuais, com os custos e regressões de completude descritos acima.
