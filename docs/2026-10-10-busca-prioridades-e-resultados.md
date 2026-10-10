# Busca do Mustard: prioridades implementadas e resultados — 10/10/2026

Implementação na branch `codex/mustard-plano-completo`, na cópia isolada `/home/rubens/.cache/codex/worktrees/mustard-plano-completo-20261008`. Baseline desta revisão: `e29ec6f3a750109f64b23d976e7a03c660b0338b`. A branch pessoal em alteração e os projetos de aplicação originais não foram modificados. Este documento substitui o estado corrente dos relatórios de 08/10 e 09/10, preservados como histórico.

**Resultado:** houve ganho mensurável na recuperação de referências úteis, acompanhado de menos texto no agregado. A busca nativa passou de 55/80 para 64/80 alvos entregues; com o modelo local opcional, chegou a 72/80. Isso mede referências à declaração esperada na apresentação efetivamente entregue ao agente, com arquivo/faixa/hash conferidos. Não é precisão sobre todos os candidatos, implementação correta ou economia de tokens faturados.

## Prioridades e alterações mantidas

1. **Cobertura antes de reranqueamento.** Declarações de testes antes excluídas passaram a entrar no catálogo, identificadas por `test_only`. A visão do agente e a evidência enviada ao Jev distinguem testes de produção. Código de teste indexado não prova execução ou cobertura.
2. **Admissão e fusão antes dos cortes.** Âncoras exatas permanecem prioritárias. Líderes dos canais independentes são preservados antes do consenso por RRF; vistas lexicais correlacionadas não ganham votos extras contra o canal de código apenas por serem mais numerosas. O reservatório interno de fontes foi separado do orçamento de apresentação. Continuam os limites de 96 arquivos, 12 MiB agregados e 2 MiB por arquivo; o corte dependente de dois segundos foi retirado porque alterava os candidatos conforme a carga da máquina. A investigação continua parcial e informa suas lacunas.
3. **Trechos estruturais.** O parser registra faixas de bytes/linhas por fronteiras da AST, dividindo nós grandes e reunindo irmãos contíguos. Os trechos priorizam essas fronteiras, conservam os intervalos ainda não lidos e indicam folhas excessivas. Não há resumo inventado. Os conceitos de divisão/reunião estrutural foram adaptados de [cAST](https://aclanthology.org/2025.findings-emnlp.430/), usando o Tree-sitter já existente.
4. **Encadeamento útil.** Âncoras explícitas e o primeiro destino elegível conservam prioridade; os demais pontos de partida favorecem dependências estáticas com alvo único. Vizinhos incidentais que não entraram na seleção da tarefa não consomem os espaços de investigação. Interfaces/tipos ainda podem levar aos seus membros e implementações. Isso recuperou o contexto do frontend e preservou o roteamento de provedor do Sialia.
5. **Busca vetorial local opcional sobre código real.** Um canal independente consulta o índice inteiro dentro do escopo, antes do corte lexical de candidatos. Chunks são agrupados por símbolo antes do corte, evitando que uma classe grande ocupe todos os destinos. Só depois se hidratam os candidatos e se confere o código atual. Não há servidor de modelos, download em runtime, Ollama ou API paga nesse caminho.
6. **Menos repetição na apresentação e no Jev.** Caminhos e qualificadores repetidos do grafo são agrupados, preservando as relações e coordenadas. `knowledge-choice-v7` retira campos vazios e assinaturas/documentação já presentes literalmente no trecho numerado; conserva documentação distinta, hashes, alternativas, abstenção e os critérios de aceitação anteriores. Candidatos/trechos continuam compartilhados entre perguntas quando idênticos, conforme os conceitos de [Choice](https://docs.typesafe.ai/primitives/choice) e [State](https://docs.typesafe.ai/concepts/state).
7. **Medição da entrega real.** O avaliador confere a apresentação do agente, não apenas o JSON interno. Um modo diagnóstico de projeção evita o rastreamento caro de todos os candidatos; a primeira pergunta de cada repositório compara essa projeção byte a byte com a saída normal. O rastreamento completo continua disponível explicitamente.
8. **Hidratação seletiva do banco.** A recuperação passa pelos campos e declarações não solicitados sem construir seus objetos em memória. Só desserializa os cards nas posições pedidas, mantendo validação do JSON e conferência de identidade/posição. Não acrescenta uma cópia persistente dos dados. A alteração foi mantida após preservar a saída e reduzir a latência da versão intermediária.

Nenhuma regra de negócio, nome de fornecedor, projeto ou gabarito foi introduzido no algoritmo. A implementação reutiliza os parsers, SQLite e `model2vec-rs` existentes. O núcleo/contrato permanecem independentes do host; o adaptador instalado para Codex continua sendo uma etapa futura.

## Recuperação e tamanho da saída

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

O bloco SQLite `code-meaning` armazena vetores int8 por símbolo/chunk, hash da fonte e impressão digital da análise/modelo. Atualiza somente arquivos alterados, remove arquivos desaparecidos e rejeita recibos antigos. A busca faz cosseno no índice escopado e não escreve vetores durante a consulta. O gateway continua revalidando a fonte atual depois da recuperação. A atualização estrutural automática é nativa: vetores de conteúdo alterado ficam inelegíveis até um scan normal com a opção habilitada.

No Sialia foram 39.167 vetores, com 10.026.752 bytes de componentes; o banco inteiro passou de aproximadamente 374,3 MB para 500,1 MB. No Florestal, 15.657 vetores, 4.008.192 bytes de componentes, banco de 78,0 MB para 108,3 MB. O aumento do banco inclui também testes agora indexados, fronteiras da AST, índices/IDs e o índice semântico genérico anterior que continua usado por outros comandos. Não é todo atribuído aos 256 bytes de cada vetor. O `mustard-rt` de desenvolvimento otimizado cresce cerca de 33,5 MB com os dados incorporados; não há serviço residente adicional.

O armazenamento vetorial em SQLite foi mantido: estes ensaios não justificaram introduzir outro serviço ou um índice ANN com aproximação. Isso não prova que uma varredura de cosseno seja adequada para qualquer escala; o tempo por consulta e a cardinalidade devem continuar sendo medidos.

### Ativação e contrato

O padrão continua **sem modelo e sem chamada paga**. Para experimentar o canal local, mesclar no `mustard.json`:

```json
{"ai":{"vectors":true,"fallback":false}}
```

Executar o scan normal do projeto para preencher o índice. Na cópia de desenvolvimento, a entrada direta é:

```sh
target/debug/scan scan /caminho/do/projeto --out /caminho/do/projeto/.claude/grain.db --json
```

`--native` conserva a atualização puramente estrutural; `ai.vectors:false` desliga o uso dos modelos locais. A chave do Jev não é necessária para embeddings. Pesquisa literal/identidade exata resolvida não ativa o canal novo automaticamente apenas porque existe um índice vetorial.

O contrato do gateway continua `schema_version:1`, `request:{tool,input,intent,purpose,choose?}`. O resultado nativo permanece recuperável por `purpose:locate`/`--raw`, e falhas/ausência de evidência útil preservam o fallback. Mods executa a pesquisa pelo fluxo de ferramentas/permissões do host. Os limites dos hooks clássicos, comandos compostos e formatos não suportados permanecem documentados no relatório de 09/10; não há promessa de interceptar qualquer programa possível.

## Jev: papel e custo medido

Jev continua atrás de `SymbolSelector`, usado para **escolher/classificar alternativas com evidência atual**, quando solicitado e habilitado. Não gera o scan, não faz busca lexical, não calcula consumo, não renderiza o painel e não publica páginas. Identidades exatas, atualizações nativas, chamadas comuns e cache não precisam do provedor. Nenhum limiar foi relaxado para aumentar os resultados.

Na fixture de 12 comparações pequenas, seis de calibração e seis reservadas, a mudança v6 → v7 preservou **12/12 resultados concordantes com as expectativas**, incluindo três `no-match`, um caso abaixo da aceitação e nenhuma escolha errada aceita. Com os mesmos pedidos e 12 chamadas, a entrada conhecida caiu de **12.398 para 11.726 tokens (−5,42%)**; estimativa de custo de US$ 0,000523 para US$ 0,000490. É uma amostra preparada e pequena, sem prova de calibração geral.

O piloto público final usa as primeiras duas descrições de cada um dos quatro repositórios da seleção de 09/10, sem rótulos no pedido/provedor. Em oito perguntas, o alvo estava no conjunto de alternativas em sete: **sete escolhas corretas, zero erradas aceitas e uma abstenção**. O primeiro destino correto passou de 1/8 na ordem nativa para 7/8 após a escolha. Foram **nove chamadas físicas**, incluindo refinamento, **178.232 tokens de entrada conhecidos**, **US$ 0,007485 estimados**; oito repetições usaram cache com zero HTTP. Esse custo não é comparável aos 12 casos curtos, nem demonstra menor custo de sessão.

Toda a revisão, contando a repetição de calibração anterior à recompilação e os dois pilotos públicos, registrou **54 chamadas**, **392.902 tokens conhecidos** e **US$ 0,016503 estimados**. Nenhuma tentativa foi apagada da conta. Valores seguem o preço configurado, não uma fatura verificada. O piloto público anterior à identificação explícita de testes teve o mesmo resultado qualitativo. A revisão final acrescentou a indicação `test_only` à evidência; por isso seus tokens diferem ligeiramente.

## Ensaios retirados ou não incorporados

- **Empacotar mais corpos completos:** na seleção nova de 40, conservou 36 alvos, mas aumentou o texto de 522.408 para 702.550 bytes (**+34,5%**). Foi removido. A entrega final mantém referências e permite expandir os intervalos que faltam.
- **Jina Turbo como reranqueador local:** protótipo de dez perguntas Go caiu de 5/10 para 4/10 no primeiro lugar e de 9/10 para 8/10 nos cinco primeiros; aproximadamente 70,1 s. Não entrou no produto.
- **Jina embeddings de código, maior:** protótipo Go chegou a 10/10 nos cinco primeiros contra 9/10 do modelo estático, mas levou aproximadamente 199,8 s, usou cerca de 1,29 GB de memória de pico e 615 MB de cache. O ganho estreito não justificou incorporá-lo.
- **Priorizar todos os alvos com dependência antes do primeiro destino:** uma política intermediária perdia o roteamento no Sialia. Foi substituída pela ordem que preserva âncoras/líder e só depois favorece dependências úteis.
- **Rastreamento completo durante toda a avaliação/uso comum:** permanece optativo para diagnóstico; os ensaios finais usam projeção exata sem o rastreamento caro por candidato. A latência é medida separadamente, com diagnóstico desligado.
- **Ollama, modelo gerador de resumos e banco vetorial externo:** não incorporados. O resultado atual não demonstrou necessidade deles. O scan continua um catálogo de evidências, sem inventar um documento completo de regras de negócio por heurística.

## Verificação e reprodução

Passaram **3.982 testes Rust**, com dois ignorados herdados, Clippy com `-D warnings`, **16 testes oficiais do plugin Mods**, validação do plugin, sintaxe dos runners e conferência dos SHA-256 dos dados do modelo. A aceitação ponta a ponta instala usando o binário absoluto desta cópia em uma pasta realmente vazia e verifica atualização, fallback, paridade, ferramentas tipadas, aprendizado, expansão, reutilização de corpos e zero HTTP/modelos no caminho padrão.

```sh
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo build --workspace
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo test --workspace
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo clippy --workspace --all-targets -- -D warnings
node apps/scan/benchmarks/gateway-acceptance.mjs
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
  --vectors true --out /caminho/resultados
```

Trocar `--vectors true` por `false` compara o caminho nativo. O runner remove credenciais e exige zero chamadas externas. O piloto Jev é separado e exige `--jev`, configuração habilitada e credencial; nunca salvar a chave nos relatórios. A latência usa `gateway-latency.mjs`, roots já escaneadas por seus próprios binários e manifesto de cópias descartáveis, com aquecimento e braços alternados.

O painel/mods, statusline e publicação externa somente mediante solicitação explícita continuam com seus fluxos existentes. Não foi reintroduzida publicação automática. O que ainda depende de uso real é a ergonomia no Claude, redução de buscas/leituras, tokens totais faturados e qualidade da implementação produzida. Os ganhos demonstrados aqui são recuperação, tamanho da apresentação e escolhas pontuais, com os custos e regressões de completude descritos acima.
