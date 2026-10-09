# Scan como oráculo de recursos do projeto

Estado em 09/10/2026. A orientação atual é esgotar alternativas nativas antes de considerar IA; a avaliação pontual do Jev em todo o Mustard está registrada ao final deste documento. Implementação na branch `codex/mustard-plano-completo`, isolada da instalação pessoal. A continuação e a aplicação da pesquisa ao scan foram autorizadas nesta data. Este documento amplia a trilha C do plano; não declara concluído um entendimento completo das regras de negócio.

## Continuação atual: evidência dentro das funções e seleção de candidatos

A recuperação agora considera identificadores usados no corpo das declarações, assinaturas e valores de textos fixos. Antes, esses identificadores já existiam no parser, mas não chegavam aos cartões de função. Os textos também eram cortados antes da indexação, e somente os primeiros 12 chegavam aos cartões. A versão 3 do pacote conserva todos os textos aceitos e seus valores normalizados completos, inclusive palavras depois da prévia de 300 caracteres; a própria decisão de aceitar um texto examina seu conteúdo completo. Pacotes anteriores precisam de novo scan; interpretações registradas permanecem preservadas.

A indexação pesquisa valores da fonte, sem promover chaves JSON ou números de linha a palavras do código. Nome/comentário/anotação têm maior peso que assinatura/identificadores/textos; evidência herdada do caminho tem menor peso. Termos técnicos e plurais ficam no vocabulário declarativo, sem nomes do backend, frameworks ou linguagens no motor. Isso localiza candidatos por pistas escritas, sem deduzir o significado de uma constante externa ou provar execução.

Correspondências completas e identificadores exatos mantêm prioridade. Na seleção ampla, o índice dos cartões e a descoberta geral passam a contribuir alternadamente; um deles não ocupa sozinho a lista antes de o outro entrar. O filtro de arquivo é aplicado antes dessa contribuição. O limite de hidratação permanece explícito e pode ser expandido com `--all`.

A resposta inicial continua compacta: `matched_evidence` fornece até três identificadores/textos correspondentes por cartão, com janela de até 180 caracteres em textos longos e linha da fonte. Identificadores já visíveis não são repetidos. `detail_counts` informa a evidência disponível; `--detail` expande os valores completos sem duplicar esses pequenos trechos. Mais evidência disponível não significa despejar corpos de funções na resposta inicial.

### Medição da evidência ampliada

Baseline `1ccae2c6`, mesma fonte Suzano `a3fe37ab454c`, duas cópias novas isoladas e sem Markdown/ambiente. As perguntas foram preservadas; ambas as listas já são conhecidas pelo implementador. Comparação sem modelo auxiliar:

| Medida | Baseline | Evidência ampliada |
| --- | --- | --- |
| Arquivo esperado, 21 perguntas conhecidas válidas | 17/21 | 17/21 |
| Símbolo esperado, 20 perguntas com símbolo definido | 11/20 | 11/20 |
| Arquivo esperado, segunda lista de 16 perguntas | 13/16 | 15/16 |
| Símbolo esperado, 13 perguntas da segunda lista | 7/13 | 10/13 |
| Bytes das respostas da primeira lista | 285.002 | 316.880 (+11,2%) |
| Bytes das respostas da segunda lista | 210.962 | 227.116 (+7,7%) |
| Banco | 76.218.368 bytes | 78.778.368 bytes (+3,4%) |
| Mediana, cinco consultas fixas em três rodadas alternadas | 284 ms | 329 ms |
| Scan completo, uma execução | 12.048 ms | 11.668 ms |
| Scan sem alteração, uma execução | 140 ms | 121 ms |
| Chamadas auxiliares locais/remotas/Jev | 0 | 0 |

Foram recuperados o arquivo do cabeçalho CSRF e o interceptor de duração; também melhorou a localização de símbolos de migração e conexão. A pergunta sobre transformar validação em resposta 400 ainda não encontra a fonte esperada. O índice não deduz que uma constante de biblioteca significa 400. Nenhum termo específico desse projeto foi acrescentado ao motor/vocabulário. Ganho de localização não comprova a resposta de negócio.

A primeira tentativa enviava mais metadados dos trechos correspondentes; os registros intermediários foram conservados. A projeção compacta reduziu esse acréscimo, mas o resultado final continua maior. O banco e a mediana aquecida também cresceram. São executáveis de desenvolvimento em uma máquina, sem inferência de desempenho universal. **Não há demonstração de economia total de tokens, custo faturado ou melhor código final.** A evidência adicional pode evitar leituras posteriores, mas isso ainda precisa ser medido numa spec real.

Artefatos em `target/scan-evidence-20261009/suzano-final/`: perguntas, respostas por versão, `benchmark.json`, auditoria, `interleaved-latency.json` e `csrf-inventario.md`. Hashes identificam os executáveis efetivamente medidos. `pre-external-freeze.json` registra a árvore/executáveis antes da avaliação externa; o pacote final é recompilado do commit limpo. O original permanece intacto.

### Avaliação externa congelada: RepoQA

Foi executado um teste adaptado a partir do corpus público [RepoQA](https://github.com/evalplus/repoqa), edição [2024-06-23](https://github.com/evalplus/repoqa_release/releases/tag/2024-06-23). Não é a pontuação oficial, nem revisão independente do implementador. As descrições foram produzidas por terceiros; não houve inferência para executar esta avaliação. Os comentários originais foram mantidos, e foram usados os subconjuntos de fontes distribuídos no corpus, sem executar/buildar seus projetos.

Antes dos resultados, congelamos um repositório por linguagem: menor SHA-256 do nome entre os que têm até 400 arquivos, até 3 MiB de fonte fornecida e exatamente dez alvos. As 60 descrições foram consultadas sem alteração. Cada resposta usa até oito cartões. Arquivo, símbolo/nome/intervalo e presença do símbolo no índice são medidos separadamente; os alvos ausentes não foram descartados do denominador.

| Linguagem / repositório | Símbolos presentes no índice | Arquivos baseline → atual | Símbolos baseline → atual |
| --- | --- | --- | --- |
| Python / ethereum/web3.py | 10/10 | 9 → 9 | 1 → 1 |
| C++ / sass/node-sass | 0/10 | 0 → 0 | 0 → 0 |
| Java / karatelabs/karate | 0/10 | 0 → 0 | 0 → 0 |
| TypeScript / umami-software/umami | 10/10 | 9 → 8 | 2 → 4 |
| Rust / seanmonstar/warp | 10/10 | 6 → 7 | 2 → 1 |
| Go / jesseduffield/lazydocker | 8/10 | 7 → 7 | 2 → 5 |
| **Total** | **38/60** | **31/60 → 31/60** | **7/60 → 11/60** |

O registro atual não contém gramáticas C++/Java: o scan dessas duas cópias retorna zero arquivos, portanto são lacunas de suporte, não candidatos que um classificador resolveria. Os dois alvos Go ausentes são funções em arquivos de teste, excluídas dos cartões pelo contrato atual. A auditoria do banco passou nos seis projetos, inclusive nos vazios; isso reforça que integridade não prova cobertura.

O ganho agregado de quatro símbolos inclui seis ganhos e duas regressões de símbolo; a localização de arquivos tem um ganho e uma regressão. Respostas somadas passaram de 589.164 para 641.250 bytes (+8,8%). Não ajustamos o motor após ver essas respostas. **Esta avaliação não confirma um oráculo geral**, nem mostra ganho de arquivos fora do corpus conhecido. Próximas melhorias devem distinguir suporte/exclusões, ausência de evidência, classificação e expansão, com novas perguntas congeladas para evitar ajustar os mesmos gabaritos indefinidamente.

Reprodução, depois de obter e descompactar o JSON oficial (o script confere seu SHA-256):

```text
node apps/scan/benchmarks/repoqa.mjs \
  --dataset /tmp/mustard-repoqa-2024-06-23.json \
  --selection apps/scan/benchmarks/repoqa-selection-20261009.json \
  --baseline target/scan-evidence-baseline \
  --current target/debug \
  --out target/scan-evidence-20261009/repoqa
```

O baseline contém os programas `scan` e `mustard-rt` compilados de `1ccae2c6`; `target/debug` contém os atuais. O JSON de seleção versionado registra revisão de cada projeto e hash do dataset. O executor é ferramenta de desenvolvimento, sem dependências, rede ou execução das fontes; não entra no binário instalado. Relatório/respostas estão em `target/scan-evidence-20261009/repoqa/`.

### Comparação com o documento Puzzle

O documento recebido organiza atores, jornadas PI/PCP, estados, autenticação, rotas, validações, armazenamento, cálculos e divergências de negócio. A exportação nativa produz inventário rastreável: símbolos, contratos/rotas extraídos, comentários, textos, relações estáticas, grupos, referências documentais, Git e lacunas. Ter mais linhas nesse inventário não equivale à análise do Puzzle.

O banco pode conservar e exportar interpretações revisadas com múltiplas fontes; isso permite reaproveitar raciocínio já feito. Não transforma nomes de variáveis em narrativa comprovada nem deduz sozinho os atores, a ordem real da jornada, a regra de autorização ou uma divergência entre intenção e implementação. O caminho econômico continua sendo preparar evidência por tópico, expandir somente as lacunas e pedir ao modelo principal raciocínio pontual, registrando suas conclusões com fontes para reutilização. Essa preparação funciona sem inferência auxiliar; a equivalência de conteúdo com o Puzzle não foi atingida.

### Verificação desta etapa

Suíte completa: **3.854 testes aprovados**, zero falhas e dois ignorados herdados (`/tmp/mustard-evidence-tests-accepted.log`). Após o ajuste final da aceitação de textos longos, a suíte do scan foi repetida e aprovada (`/tmp/mustard-evidence-scan-final-tests.log`); lint estrito de todos os alvos e build dos três programas passaram (`/tmp/mustard-evidence-clippy-final.log`, `/tmp/mustard-evidence-build-final.log`). A aceitação nativa instala o binário absoluto em pasta realmente vazia e verifica corpo de função, cauda de texto, expansão sem duplicação, auditoria, fontes alteradas, referências/reverso e Markdown, com zero pedidos HTTP apesar de chave/filtros legados. Prova e pacote local são identificados pelo commit final limpo. Isso não substitui uma sessão real do Claude ou uma spec acompanhada de ponta a ponta.

## Histórico de 09/10: catálogo, referências e auditoria nativos

SQLite com FTS5 continua adequado ao produto: armazenamento local, transações de fontes/índices e busca lexical mais navegação dirigida. A reconferência encontrou problemas de acesso e cobertura, não uma necessidade demonstrada de banco vetorial ou serviço externo. Referência técnica: [SQLite FTS5](https://www.sqlite.org/fts5.html).

Correções implementadas:

- Catálogo derivado de endereços/nomes/linhas/hashes, índice de nomes e FTS5 próprios. Consulta hidrata somente os cartões candidatos; não carrega todos os pacotes JSON. Evidência canônica permanece em `texts.analysis`, sem duplicá-la no catálogo.
- Correspondência de todos os termos informativos precede a seleção ampla de candidatos. Documentação/configuração também aplica o critério completo dentro do SQL, antes do corte. Frequência das palavras é calculada no corpus completo pelo índice; limitar candidatos não deve alterar a raridade que escolhe a função dentro de um arquivo.
- Índices de cartões e de recursos mantêm entradas de arquivos sem mudança. A fonte canônica ainda pode ser regravada como bloco pelo scanner, e as referências documentais são reconstruídas quando código/recursos mudam. Não se trata de escrita universalmente incremental.
- Nomes exatos usam índice próprio. Navegação dirigida consulta relações indexadas e verifica recibos nos dois extremos. Identidades de declarações de mesmo nome/linha agora distinguem colisões; pacotes antigos pedem novo scan. Notas revisadas não são apagadas.
- Configuração executável/scripts sem declarações reconhecidas recebem evidência `source-file`, com identificadores do parser, textos, linhas e hash. Essa entrada representa um arquivo; não inventa uma função nem comprova seu comportamento.
- Markdown pode apontar para código por link relativo/linhas ou identificador entre crases. Só nome único/endereço válido cria vínculo. Exemplos cercados, nome ambíguo, destino externo ou âncora desconhecida não viram prova. A consulta encontra código pela documentação e a navegação recupera documentos ligados ao símbolo, com ambas as fontes atuais.
- `--detail`, `--all` e Markdown incluem grupos estruturais do subgrafo selecionado. Entradas candidatas e ciclos têm justificativa; regra de negócio e ordem em execução continuam desconhecidas.
- Consulta confere a geração do banco antes/depois e repete uma vez se outro scan modificar as fontes; nova concorrência produz recusa explícita. Isso evita combinar gerações diferentes numa mesma resposta.
- `mustard-rt run map audit` verifica SQLite/FTS, presença das entradas, correspondência com a fonte canônica, endereços de recursos e vínculos, além de mostrar planos de consulta. Não atualiza silenciosamente o scan para mascarar problemas. Integridade do banco não equivale a fonte atual nem a entendimento semântico.

Consulta inicial conserva até oito resultados e compactação; `--file`, `--symbol`, `--detail` e `--all` expandem a evidência. O conjunto amplo de candidatos tem limite explícito de hidratação, com aviso de omissão e possibilidade de consulta exaustiva. A busca lexical ainda pode omitir paráfrases: não substitui investigação por critérios/diff quando é necessário provar completude.

### Medição desta continuação

Base anterior `ecc71222`, sem IA, comparada ao catálogo desta continuação. Backend Suzano `a3fe37ab454c`; cópias isoladas, original intacto. Todos os Markdown, incluindo Puzzle, e arquivos de ambiente foram excluídos. Não foram inseridos vocabulários ou regras específicas do backend.

| Medida | Antes | Catálogo atual |
| --- | --- | --- |
| Arquivo esperado, conjunto conhecido | 16/21 | 17/21 |
| Símbolo esperado, perguntas com símbolo definido | 10/20 | 11/20 |
| Arquivo esperado, 16 perguntas novas | 11/16 | 13/16 |
| Símbolo esperado, 13 perguntas novas com símbolo definido | 7/13 | 7/13 |
| Bytes das respostas conhecidas | 297.729 | 285.002 (-4,3%) |
| Bytes das respostas novas | 213.719 | 210.962 (-1,3%) |
| Banco | 67.960.832 bytes | 76.218.368 bytes (+12,2%) |
| Scan completo, uma execução | 11.659 ms | 11.935 ms |
| Scan sem alteração, uma execução | 203 ms | 141 ms |
| Chamadas a modelos auxiliares/Jev | 0 | 0 |

A mediana de uma prova adicional com cinco consultas fixas, três repetições e ordem baseline/atual alternada foi **1.046 → 279 ms**. São 15 leituras por versão em uma máquina, executáveis de desenvolvimento e cache aquecido; não é vazão universal nem tempo de spec. As medianas da primeira passada não servem de comparação justa: parte da baseline coincidiu com compilação/testes.

O [ckg](https://github.com/phins-group/ckg), commit `0895461d16b0d028a67e792757a047536de7df0b`, foi compilado e executado separadamente no mesmo snapshot. Encontrou 13/16 arquivos e 3/13 símbolos esperados. Seus oito resultados misturam arquivos/símbolos e têm outro formato: os 43.982 bytes, banco de 29.720.576 bytes e mediana de 37 ms não equivalem a um contexto/produto idêntico ao Mustard. Não concluímos superioridade geral. Nenhum motor/dependência desse projeto foi adicionado ao produto.

As 16 perguntas foram formuladas pelo implementador a partir das fontes antes da execução, e passaram a ser conhecidas durante as correções. Não é validação cega ou independente. O primeiro catálogo preservou 16/21 arquivos conhecidos, mas caiu de 7 para 6 símbolos no conjunto novo. A conferência encontrou cálculo de raridade sobre candidatos, corrigido para frequência global. Outra lacuna era a ausência de evidência para módulos sem declarações; depois de adicioná-la, uma seleção ampla ainda podia ocupar todos os candidatos antes da correspondência completa. O critério completo passou a preceder essa seleção. Os registros intermediários foram conservados.

A auditoria nativa do backend passou: 12.832 entradas (12.770 declarações e 62 evidências de arquivo), 10.336 relações estáticas, 67 recursos aceitos, dois excluídos e 192 trechos. Todos os cinco planos de consulta conferidos usaram índices. O corpus de comparação não contém documentos Markdown, portanto os vínculos documentação↔código foram exercitados em fixtures reais separadas. Índices/catálogo aumentam espaço e o scan completo não ficou mais rápido nesta medida; o benefício observado foi cobertura e recuperação seletiva. **Não foi demonstrada economia faturada de tokens nem melhor qualidade final de spec/código.**

Artefatos: `target/suzano-catalog-20261009/` contém perguntas/respostas, `native.json`, `native-database-audit.json`, `interleaved-latency.json` e `csrf-inventario.md`; comparativos anteriores estão em `target/suzano-catalog-before-ranking-20261009/` e `target/suzano-catalog-after-idf-20261009/`. Hashes dos executáveis identificam a árvore pré-commit medida. O ajuste posterior de validação de âncoras documentais não afeta esse corpus sem Markdown; o pacote final é recompilado do commit limpo e passa pela instalação nativa temporária.

Verificação final: **3.849 testes Rust aprovados**, zero falhas e dois ignorados herdados; lint estrito de todos os alvos e build dos três programas aprovados. Instalação real do binário em pasta temporária vazia, scan, consulta, navegação, vínculo documental/reverso, grupos, auditoria e Markdown aprovados com zero pedidos ao servidor HTTP de teste, inclusive com chave/filtros legados. Logs: `/tmp/mustard-catalog-workspace-accepted.log`, `/tmp/mustard-catalog-clippy-accepted.log`, `/tmp/mustard-catalog-build-accepted.log`, `/tmp/mustard-catalog-native-acceptance.log`. Prova de instalação e pacote são atualizados para identificar o commit final limpo.

## Histórico de 09/10: primeira ingestão de documentos e configurações

A pesquisa foi aplicada parcialmente; não foi encerrada uma implementação de todos os motores/features consultados. A reconferência das fontes primárias de [ContextGraph](https://github.com/erenalpaslan/context-graph), [Codebase Memory MCP](https://github.com/DeusData/codebase-memory-mcp) e [SCIP](https://github.com/scip-code/scip) manteve a separação entre ingestão de evidência, navegação e resolução por compilador. Nesta continuação aplicamos a ingestão de textos de documentação/configuração/esquemas, com implementação própria e sem dependências novas.

O banco guarda recursos textuais separados de declarações executáveis. O registro de formatos e exclusões é dado, não lógica de framework. Aceitamos Markdown/MDX, documentação textual, JSON/JSONC, TOML, YAML, INI e textos de esquemas/consultas/protocolos. Esses últimos são **texto da fonte**, não extração universal de tabelas nem configuração efetiva em execução. A cobertura estrutural de schemas pelo parser da etapa anterior continua separada.

Cada trecho conserva arquivo, intervalo de linhas e SHA-256 do conteúdo realmente lido. O conteúdo integral dos arquivos aceitos permanece no banco; a resposta inicial mostra uma janela de até 600 caracteres em torno do termo encontrado, com indicação de compactação e expansão. Cabeçalhos dentro de exemplos cercados não criam seções. Um índice FTS5 local encontra os candidatos sem abrir/deserializar todo o mapa de código. A busca textual complementar exige correspondência em **todos os termos informativos** da consulta, com vocabulário/normalização nativos; busca por caminho exato também funciona. Sobreposição em poucas palavras comuns não acrescenta documentação ao contexto. Isso favorece precisão e pode omitir paráfrases ou termos distribuídos por trechos diferentes; não é compreensão semântica universal.

`knowledge` retorna símbolos em `cards` e textos em `resources`. Use `--file <arquivo> --detail` para ampliar os trechos recuperados; `--all` permite exportação completa dos trechos correspondentes. Markdown inclui a evidência textual com origem e ressalva de comportamento não validado. Contexto preparado de componente de configuração/documentação também verifica a fonte na cópia real da onda; alteração além da janela visível invalida a versão. O runtime de scan mostra contagem de recursos e motivos de exclusão.

Arquivo de recurso alterado, criado ou excluído atualiza censo/recursos numa transação sem reescrever código, rotas, grafo ou história quando estes continuam válidos. Arquivos de texto sem alteração são reutilizados por blob. Nesta primeira etapa o índice de recursos era refeito como bloco quando seu conteúdo mudava. A continuação abaixo substitui isso por atualização das entradas FTS de cada arquivo alterado. Não chamamos isso de manutenção universalmente incremental. Sem mudança, o caminho rápido conserva os blocos. Alteração de manifesto/código ou mudança de histórico continua seguindo as invalidações do scanner.

A caminhada respeita as exclusões existentes. O registro exclui ambiente, configuração interna `mustard.json`, arquivos de credenciais/segredos, locks e saídas comuns; arquivos acima de 256 KiB, binários, não UTF-8, inacessíveis ou com sinais conservadores de credenciais não fornecem trechos. Os motivos ficam no relatório. O filtro de conteúdo não é um detector universal de segredos e pode excluir referências a variáveis, como ocorreu em dois arquivos de CI do corpus. Ignorados e formatos sem suporte não são apresentados como cobertura completa. O texto de um documento, mesmo com hash atual, permanece uma declaração do autor, sem prova de que concorda com o código.

### Resultado medido no Suzano

Base `a5c0570a`, também sem IA auxiliar, versus esta continuação. Duas cópias isoladas do backend `a3fe37ab454c`; fonte original intacta e cópias removidas depois da conferência. Documento Puzzle, todos os Markdown e ambiente ficaram fora da ingestão. As perguntas de código são o conjunto conhecido anterior, sem novo gabarito independente. As cinco consultas adicionais conferem termos literais de configuração em `package.json`/`tsconfig.json`; não avaliam significado de negócio ou configuração efetiva.

| Medida | Base nativa anterior | Continuação com recuperação textual precisa |
| --- | --- | --- |
| Arquivo de código esperado / símbolo | 16/21 · 10/21 | 16/21 · 10/21 |
| Arquivo de configuração esperado nas cinco consultas adicionais | 0/5 | 5/5 |
| Bytes somados das 21 respostas de código | 289.644 | 297.729 (+2,8%) |
| Mediana de consulta observada | 1.184 ms | 1.216 ms |
| Scan completo / sem alteração | 8.769 ms · 131 ms | 7.594 ms · 137 ms |
| Banco em bytes | 67.686.400 | 67.960.832 |
| Modelo auxiliar local/remoto | Zero | Zero |

O primeiro ensaio desta continuação adicionava textos com apenas duas palavras coincidentes e elevava as respostas para 374.641 bytes (+29,3%). O critério mais estrito baixou esse total para 297.729, mantendo as cinco consultas de configuração. Ambos os registros são conservados; a alteração foi orientada por este conjunto conhecido, portanto não a tratamos como resultado de validação independente. O acerto de código não cresceu. **Não demonstramos economia de tokens faturados ou qualidade melhor de spec/código.** O ganho comprovado é recuperar mais tipos de fonte sem modelo auxiliar e conter o acréscimo de contexto. Tempos têm uma execução por consulta e ordem fixa; não sustentam aceleração estável.

Uma prova adicional em nova cópia do mesmo backend inseriu um YAML de teste explicitamente identificado como fixture. A atualização leu apenas esse arquivo em 196 ms; hashes de todas as linhas de código, declarações, textos de código, rotas, relações, grafo e história permaneceram idênticos. A passada sem alteração leu zero arquivos em 129 ms. É prova do isolamento da atualização, não uma comparação de trabalho equivalente com um scan completo de 7.521 ms.

Registros locais: `target/suzano-resources-20261009/benchmark.json` (primeiro critério), `target/suzano-resources-precise-20261009/benchmark.json` (critério final), respostas/gabaritos, inventário Markdown e `resource-update-proof.json`. Hashes identificam os executáveis medidos da árvore de trabalho anterior ao commit final; o pacote de revisão é recompilado do commit limpo.

Verificação: suíte completa com **3.835 testes Rust aprovados**, zero falhas, dois ignorados herdados. Depois do ajuste final de correspondência, os seis testes específicos de recursos foram aprovados, incluindo o novo caso que exclui sobreposição genérica. Lint estrito de todos os alvos e build dos três programas aprovados após o ajuste. Prova real de instalação em pasta vazia, scan, consulta de documentos/configurações, navegação, exportação, fontes secundárias e configuração legada realizada com zero pedidos ao servidor HTTP de teste. Logs: `/tmp/mustard-resources-workspace-final.log`, `/tmp/mustard-resources-precision-test.log`, `/tmp/mustard-resources-precision-clippy.log` e `/tmp/mustard-resources-precision-build.log`.

### Cobertura dos conceitos pesquisados — atualizada nesta continuação

| Conceito pesquisado | Aplicação no Mustard | Limite explícito |
| --- | --- | --- |
| Claude Code Setup | Perfil/detecção por evidências; mecânica no binário; instruções curtas por finalidade | Não compreende automaticamente toda a aplicação. |
| ContextGraph | Código, documentação/configuração/esquemas textuais, referências explícitas entre documentação e símbolos, origem e invalidação | Não há ingestão PDF nem associação semântica automática entre qualquer texto e código. |
| Code Context Graph | Anotações de intenção/regras recuperáveis como afirmações do autor | Não inferimos regras que ninguém registrou. |
| Codebase Memory MCP / Serena | Consulta indexada, identidade, navegação dirigida, expansão seletiva e fonte conferida nos dois extremos | Não incorporamos seus motores; resolução por tipos/compilador não foi implementada. |
| SCIP | Separação entre definição, referência e candidata; ambiguidades conservadas | Não foi implementada importação SCIP. Ela requer índice produzido por ferramenta da linguagem, posição/encoding e versão da fonte conferidos. |
| ckg / CodeGraph-Rust | SQLite/FTS5, catálogo derivado, índice incremental por arquivo, consulta compacta; ckg executado no backend isolado | Comparação exploratória com ckg, não avaliação independente de todos os motores. Nenhuma biblioteca/motor externo foi incorporado ao produto. |
| GitNexus | Componentes do subgrafo estático, possíveis entradas, direção e fontes, exportados nativamente | Grupos estruturais da evidência selecionada; não são jornadas de negócio nem ordem de execução comprovada. |
| DeepWiki Open | Inventário Markdown nativo, referências, anotações e interpretações conferíveis | A narrativa completa do Puzzle continua exigindo raciocínio/revisão. Não foi introduzido gerador IA. |

Os conceitos úteis ao caminho nativo acima foram aplicados. Isso não significa implementar todos os recursos de cada projeto pesquisado. PDF, índices de compilador e narrativa semântica são extensões diferentes, sem benefício comprovado que justifique introduzi-las automaticamente. A avaliação pontual do Jev ao final deste documento permanece: nenhuma operação desta continuação precisou de inferência paga.

## Medição anterior de 08/10: operação auxiliar nativa

As operações auxiliares usam o caminho nativo por padrão. Credenciais, filtros antigos e banco com vetores não ativam inferência. O runtime não executa o gerador Ollama nem aceita `--enrich`; consulta, navegação, recibos, fila de revisão e Markdown permanecem disponíveis. A interface de julgamento existente foi preservada para uma exceção futura avaliada por finalidade.

Comparativo no backend Suzano, commit `a3fe37ab454c`, branch `develop`. As duas versões foram executadas em cópias isoladas idênticas; o projeto original continuou intacto. O documento Puzzle e todos os Markdown foram excluídos da ingestão. Reutilizamos as 22 perguntas/gabaritos congelados do ensaio anterior; N11 continua registrada e excluída do aceite pela premissa falsa de uma interface de tempo real. É um conjunto conhecido, não um novo teste independente. Nenhuma nota semântica foi acrescentada.

| Medida | Versão anterior (`63bcfe66`, vetores ativos) | Caminho nativo atual |
| --- | --- | --- |
| Arquivo esperado entre até oito resultados | 16/21 | 16/21 |
| Função/símbolo esperado | 10/21 | 10/21 |
| Perguntas naturais / literais | 12/17 · 4/4 | 12/17 · 4/4 |
| Mediana observada de consulta | 1.851 s | 1.162 s |
| Scan completo / sem mudança | 12.898 s · 0.732 s | 9.292 s · 0.138 s |
| Bytes somados das 21 respostas válidas | 276,546 | 289,644 |
| Banco em bytes | 72,019,968 | 67,686,400 |
| Modelo auxiliar | Modelo estático embarcado | Zero modelo local/remoto |

O acerto não aumentou: os mesmos cinco destinos continuam ausentes (N03, N04, N09, N13, N15). O ganho demonstrado neste conjunto é preservar localização ao retirar o modelo; não provar entendimento de negócio completo. As respostas cresceram **4.7% em bytes**, portanto não há ganho comprovado de tamanho de contexto ou tokens. Tempo é uma execução por pergunta, com a versão anterior primeiro e a nativa depois; caches e ordem impedem prometer a mesma aceleração em todas as máquinas. Não foi realizado teste de criação de spec/código pelo modelo principal nem comparação paga com Jev.

Arquivos locais: `target/suzano-native-20261008/benchmark.json`, perguntas congeladas, respostas por versão e `puzzle-inventario-nativo.md`. Os hashes identificam os executáveis medidos, compilados da árvore de trabalho anterior ao commit final. O inventário Markdown tem símbolos, comentários e recibos; não é apresentado como narrativa de negócio equivalente ao documento Puzzle.

Verificação: **3.825 testes Rust aprovados**, zero falhas, dois ignorados herdados; lint estrito e compilação dos três programas aprovados. A única alteração posterior à suíte no teste de vocabulário foi comparar números de ponto flutuante com tolerância; o teste específico e o lint foram aprovados depois. A revisão de formatação preservou o Rust canônico (mesma saída normalizada do formatador), com conferência de compilação. Logs: `/tmp/mustard-native-final-workspace.log`, `/tmp/mustard-native-final-vocabulary-test.log`, `/tmp/mustard-native-final-clippy.log`, `/tmp/mustard-native-final-check.log` e `/tmp/mustard-native-final-build.log`.

Aceite com executáveis reais: instalação em pasta vazia, scan sem tabelas de vetores, busca nativa/exata, consumidores por identidade, consulta inexistente vazia, exportação Markdown, recusa de destino inválido e de `--enrich`, reutilização de interpretação e invalidação por fonte secundária. Servidor HTTP local registrou **zero pedidos**, mesmo com chave/filtros antigos e configuração legada do gerador. A integração de rodada também comprovou zero pedidos de busca, planejamento e contexto. Testes dos caminhos opcionais usam serviços falsos ou vetores embarcados habilitados explicitamente; não representam uso automático do produto. Recibo em `target/suzano-native-20261008/native-acceptance.json`.

A avaliação do Jev em **todo o Mustard** está ao fim deste documento. Nenhuma necessidade de inferência foi demonstrada nesta continuação. Interfaces futuras não ligam chamadas automaticamente.

## O resultado que buscamos

O modelo deve perguntar onde uma capacidade funciona e receber pontos de entrada, símbolos, contratos, relações, fontes atuais e lacunas. O banco deve ser a primeira porta de investigação. Quando a evidência não sustentar a conclusão, o binário oferece a localização para expandir somente o trecho necessário.

Isso reduz a procura manual de arquivos. Não transforma análise estática em prova de comportamento. Código dinâmico, macros, configuração externa, regras implícitas e sistemas fora do repositório continuam exigindo evidência adicional. Banco completo de arquivos e símbolos não equivale a conhecimento completo do sistema em execução.

O documento Puzzle recebido mistura inventário verificável com interpretação: responsabilidades, jornadas, estados, exceções e regras. Conseguimos gerar o inventário mecanicamente. Para obter uma narrativa equivalente, alguém precisa conferir o significado dessas relações. Essa análise deve ser reaproveitada, não refeita inteira em toda sessão.

## Pesquisa em fontes primárias

| Projeto | O que foi conferido | Aplicação no Mustard |
| --- | --- | --- |
| [Claude Code Setup](https://github.com/anthropics/claude-plugins-official/tree/main/plugins/claude-code-setup) | Skill de recomendações: lê manifestos/configurações, sugere automações e não modifica arquivos. Não há um motor persistente de indexação nesse plugin. Skill e cinco referências examinadas; última mudança do diretório consultada: `fc49e6815f55`. | Aproveitar a ideia de perfil do projeto. Hooks e recomendações não substituem o banco de conhecimento. |
| [Codebase Memory MCP](https://github.com/DeusData/codebase-memory-mcp) | Grafo persistente, consultas estruturais, chamadas, rotas e navegação. Examinados README, política de indexação e resolução de chamadas em `92b2dd13d796`. O código conserva estratégia/candidatos/confiança e recusa certos vínculos fracos de receptor. | Melhor referência para consultar relações e declarar incerteza. Investigar piloto comparativo antes de incorporar dependências. Não assumir que a porcentagem de economia do autor se aplica aqui. |
| [GitNexus](https://github.com/abhigyanpatwari/GitNexus) | Grafo e wiki são etapas distintas. CLI, agrupamento e gerador de wiki examinados em `50aa4be3b2c2`. O gerador usa modelo, detecta mudanças, relaciona arquivos a módulos e regenera páginas afetadas. | Documentação incremental por capacidade/módulo, sobre evidência já extraída. A licença consultada é [PolyForm Noncommercial](https://github.com/abhigyanpatwari/GitNexus/blob/main/LICENSE); esta implementação não copia seu código. |
| [SCIP](https://github.com/scip-code/scip) | Protocolo de definições, referências e implementações; esquema/lista de indexadores conferidos em `5e03215598d6`. | Próxima investigação para diminuir ambiguidades de símbolos por linguagem. Índice produzido com conhecimento de tipos acrescenta evidência que o parser sozinho pode não resolver. |
| [Serena](https://github.com/oraios/serena) | Recuperação por símbolos e referências usando servidores de linguagem; limites variam conforme o servidor. | Referência para expansão por símbolo e resolução opcional. Não impor todos os servidores de linguagem ao instalador. |
| [DeepWiki Open](https://github.com/AsyncFuncAI/deepwiki-open) | Gerador com provedores configuráveis; configuração de geração conferida em `d92819a9c9f3`. | Referência para uma etapa opcional de redação. Possibilidade de modelo local/alternativo não comprova custo menor ou qualidade suficiente no projeto do usuário. |
| [ContextGraph](https://github.com/erenalpaslan/context-graph) | README, extrator SQL e serviço de descrição de módulos conferidos em `d21b1df04658`. Extrai código/documentação/configuração/esquemas; descrições por modelo são uma operação separada, e mudança de inventário marca a descrição como antiga sem regenerá-la automaticamente. | Referência mais direta para ampliar o banco além de código. No Mustard, validade precisa incluir conteúdo de todas as fontes da afirmação, não apenas inventário de símbolos. |
| [Code Context Graph](https://github.com/tae2089/code-context-graph) | README, parser de anotações e ranking de intenção conferidos em `a7afc1ab9406`: `@intent`, `@domainRule`, efeitos, requisitos e garantias são metadados extraídos dos comentários. | Vocabulário de negócio pode vir de conhecimento humano já registrado, sem chamada de IA para extraí-lo. A presença de uma anotação não prova a regra. Nossas interpretações com fontes oferecem um caminho sem exigir editar todos os arquivos. |
| [CodeGraph-Rust](https://github.com/sunerpy/codegraph-rust) | README e esquema SQLite conferidos em `14c4f930574c`: Rust, extração determinística, FTS5, referências e navegação. | Candidato de comparação alinhado ao binário nativo. Aderência de stack não demonstra acerto nas perguntas de negócio; nenhum benchmark dele foi executado aqui. |
| [ckg](https://github.com/phins-group/ckg) | README e recuperação conferidos em `0895461d16b0`: Rust/SQLite/Tree-sitter, índice incremental e pacote da tarefa; o próprio projeto declara estágio alpha, resolução parcial e resumos de comentários/assinaturas. | Referência simples para empacotar contexto, não um substituto já aprovado para um oráculo completo. API pública estável e resumos semânticos por LLM ainda constam como trabalho futuro no projeto consultado. |

As recomendações acima são inferências para o Mustard. Não instalamos esses projetos, não executamos seus instaladores e não alteramos configuração global de clientes.

## O que está implementado nesta continuação

- O scan grava pacotes de evidência no mesmo banco SQLite: assinatura, documentação, comentários, textos literais, contratos, rotas, testes candidatos e relações entre declarações. A origem guarda arquivo, intervalo e SHA-256 do texto que o parser efetivamente recebeu.
- A recuperação usa palavras, vocabulário explícito e relações estruturais por padrão. Os vetores estáticos embarcados são uma opção desligada; só entram com `ai.vectors: true`. Interpretações registradas e atuais também localizam funções pelo vocabulário de negócio.
- Relações são conferidas dos dois lados: uma função intacta não mantém como atual um chamador cujo arquivo mudou. A cópia de onda é verificada contra seu próprio conteúdo.
- Interpretações têm várias fontes. Mudança em qualquer fonte exclui a interpretação das consultas atuais. Atualização do scan não renova silenciosamente uma interpretação antiga.
- A consulta informa a origem da recuperação: índice nativo e vocabulário, termo no fallback, fonte de interpretação ou relação estática. Parsing parcial, relações ambíguas, resultados omitidos e fontes antigas permanecem visíveis como lacunas.
- Consultas comuns compactam relações volumosas; contagens preservam a indicação do que foi omitido. O banco conserva os vínculos e a exportação explícita permite expandi-los. Não há chamada Jev para renderizar ou exportar.
- O levantamento recebe evidência recuperada sem fechar automaticamente seus pontos. O contexto preparado das ondas incorpora relações/interpretações pertinentes, com invalidação do cache quando uma fonte secundária muda. Isso não substitui leitura obrigatória de itens da spec.
- O binário gera Markdown diretamente desse conteúdo. Esse levantamento distingue extração e interpretação; não inventa jornadas ou regras ausentes.

Os dados de sintaxe completos continuam nas tabelas originais. Pacotes curtos são uma projeção para consulta, não uma promessa de que todos os detalhes cabem na resposta inicial. Testes candidatos não demonstram cobertura executada. Git exibido refere-se à história da base guardada pelo scan.

## Uso operacional

Atualizar o banco, recuperar uma capacidade e expandir o trecho localizado:

```sh
mustard-rt run scan
mustard-rt run knowledge --query "restaurar backup do projeto"
mustard-rt run knowledge --query "restaurar backup do projeto" --detail
mustard-rt run map slice --file src/backup.rs --name restore
```

`knowledge` prioriza oito declarações e consulta duas etapas de relações por padrão. A descoberta alterna a responsabilidade documentada dos arquivos com a ordem do índice híbrido; nomes exatos mantêm prioridade. Cada arquivo recebe um ponto de entrada antes dos símbolos suplementares. Relações únicas complementam posições livres, sem reservar metade da resposta para funções genéricas; relações ambíguas permanecem candidatas e não expandem o grafo inicial. `--file` restringe os pontos de partida; a expansão pode trazer declarações chamadas de outro arquivo. `--depth 0` desliga essa expansão. A resposta inicial é uma projeção curta, preservando fontes, contratos extraídos, rotas, contagens e lacunas. `--detail` expande a evidência armazenada dos mesmos símbolos; `--markdown` já usa a projeção detalhada. `--all` amplia a seleção para todos os resultados encontrados, incluindo relações. O banco conserva os pacotes originais; a projeção inicial não é o texto completo das fontes. Os parâmetros controlam a apresentação, não orçamento financeiro. A consulta vazia lista evidência; ausência de resultado não prova ausência da capacidade.

Exportar um levantamento local completo ou sobre um assunto:

```sh
mustard-rt run knowledge --all --markdown --out levantamento.md
mustard-rt run knowledge --query "publicação" --all --markdown --out publicacao.md
```

O primeiro é inventário amplo. O segundo expande os resultados e relações encontrados para o assunto. Arquivo Markdown local não é publicação externa; o comando de publicação continua dependendo de pedido explícito.

Uma interpretação conferida pode ser registrada usando as fontes devolvidas pela consulta. Exemplo de formato, com hash ilustrativo que deve ser substituído pelo hash real:

```json
{
  "id": "restore-project",
  "title": "Recuperação do projeto",
  "text": "Interpretação conferida nas fontes citadas; descreva também condições e lacunas.",
  "status": "reviewed",
  "origin": "revisão das fontes pelo responsável",
  "sources": [
    {"file": "src/backup.rs", "line": 10, "end_line": 30, "sha256": "HASH-REAL-DEVOLVIDO-PELA-CONSULTA"}
  ]
}
```

```sh
mustard-rt run knowledge --record recibo.json
```

Use `hypothesis` para interpretação ainda não conferida. `reviewed` é uma declaração do responsável, não uma aprovação automática do Mustard. O recibo comprova identidade/validade das fontes, não verdade semântica. Anote todas as fontes necessárias, incluindo configuração e testes pertinentes. A interpretação feita numa cópia modificada só fica atual nos checkouts onde esses conteúdos conferem.

## Jev e geração econômica

Conforme a [documentação do Jev](https://docs.typesafe.ai/introduction), ele avalia questões tipadas sobre estado; não é o gerador do documento. O uso correto aqui é uma ambiguidade delimitada: qual candidato é pertinente, se duas tarefas interferem ou se um vínculo proposto deve ser aceito. Extração, hashes, catálogo, busca exata, cálculo e Markdown são nativos.

Não acrescentamos chamada paga à consulta `knowledge` nem um varrimento de todo o projeto pelo Jev. As portas de julgamento do plano B continuam independentes do provedor, com cache por evidência/pergunta/revisão, exclusão entre processos e medição de tentativas físicas. A interface de geração de texto é outro contrato de biblioteca; não há adaptador gerador ativo no runtime nesta entrega.

Para enriquecer descrições de negócio, a sequência proposta é:

1. Extrair estrutura e referências nativamente.
2. Reaproveitar interpretações cujas fontes continuam válidas.
3. Montar somente o pacote da capacidade sem explicação suficiente ou afetada por mudança.
4. Um modelo gerador ou responsável confere esse pacote e devolve interpretação com fontes, condições e lacunas.
5. Validar formato, caminhos, intervalos e hashes no binário; revisão semântica continua responsabilidade do autor/revisor.
6. Atualizar o banco e projetar novamente o Markdown, sem gerar o documento inteiro pelo modelo.

Modelos locais ou geradores de menor preço podem servir à etapa 4. A escolha exige comparar qualidade e custo de manutenção no mesmo corpus; modelo que economiza por chamada e exige refazer o levantamento pode custar mais. Não configuramos provedor gerador, não cobramos API e não prometemos uma economia percentual.

## O que falta para o oráculo mais completo

Ainda não entregamos ingestão universal de regras de negócio, rastreamento em execução, resolução por compilador/LSP/SCIP, integração MCP do Codex ou um gerador automático de wiki semântica. São extensões, não nomes alternativos para a extração atual.

O próximo contrato do banco deve relacionar capacidades, entradas/saídas, estados, configurações, dados persistidos, eventos, permissões e testes às fontes. Cada vínculo precisa de método de extração, validade e lacunas. Para linguagens/artefatos sem suporte, conservar busca textual e declarar cobertura desconhecida. Canais, banco de dados e serviços externos não podem ser inventados a partir do nome de uma função.

A consulta agora oferece expansão por identidade exata e consumidores estáticos, descrita abaixo. Isso não cobre consumidores dinâmicos nem substitui resolução de tipos por compilador/LSP. A arquitetura do banco e o contrato JSON permitem um adaptador futuro para outros clientes; isso não significa suporte operacional ao Codex nesta versão.

## Régua de eficácia

O piloto local usa seis perguntas estruturais sobre o próprio Mustard, fontes conferidas por hash e zero chamadas remotas. É uma inspeção pequena do próprio executor, sem avaliação independente. Registra destinos encontrados, bytes da resposta e bytes dos arquivos relacionados; **bytes não são tokens nem dinheiro economizado**.

O primeiro piloto revelou duas falhas de desenho: pontuação nova simplificada competia com o índice existente e respostas repetiam relações demais. A recuperação passou a reutilizar a ordenação existente; respostas comuns passaram a apresentar amostras e contagens, com expansão explícita. O gabarito inicial também tinha um caminho inexistente para o transporte Cloudflare; essa referência foi corrigida, sem mudar a pergunta.

O segundo piloto encontrou o arquivo de implementação específico esperado em **duas das seis perguntas**. A primeira versão também encontraria dois destinos com o gabarito corrigido; não houve ganho de acerto demonstrado. As respostas ficaram menores que os arquivos relacionados, mas isso não compensa perder o destino correto. Não é aceite de um oráculo eficaz. Os tempos foram coletados junto de compilação/testes concorrentes e não sustentam comparação de desempenho. Artefatos locais em `target/oracle-pilot.json` e `target/oracle-pilot.md`; não são distribuídos como contexto obrigatório às ondas.

A última passada, após compactar JSON e remover trabalho de ordenação redundante, manteve **2/6** destinos. Foi executada sem compilação/suíte concorrente: construção de 17,49 s, consultas entre 2,05 e 2,70 s, respostas entre 21.093 e 35.209 bytes. É uma única medida local, sem comparação equivalente com `rg`, outros motores ou custo do modelo. O tamanho ainda é relevante; próximos ensaios precisam comparar cartões iniciais mais curtos com expansão sob demanda, sem esconder contratos ou lacunas necessárias.

A recomendação é comparar Codebase Memory MCP e CodeGraph-Rust no mesmo corpus, sem substituir o fluxo do Mustard nem aplicar instaladores globais. Comparar relações, cobertura e recuperação com o banco atual; somente depois decidir entre aproveitar um motor por interface e ampliar o índice próprio. ContextGraph orienta a ingestão de documentação/configuração/dados, enquanto Code Context Graph orienta o vocabulário de negócio explicitamente registrado. SCIP/LSP é uma opção para a resolução que ficar ambígua. O gerador incremental de interpretações permanece uma peça separada, com fontes verificáveis e revisão do significado.

Aceite de um oráculo eficaz exige corpus maior com localização e semântica conferidas: perguntas de negócio, referências cruzadas, exceções, código dinâmico, alterações sem novo scan e worktrees divergentes. Comparar busca atual, recuperação do banco e recuperação com julgamento, mantendo tarefa/modelo/esforço. Medir acerto e omissões, leituras adicionais, tamanho/tokens do contexto, chamadas pagas, construção/manutenção do índice, latência e qualidade da spec/implementação resultante. Não substituir essa prova por uma porcentagem anunciada por terceiros.


## Melhoria verificada nesta versão

Mantivemos a cópia de código usada pela régua, do commit `5e0115bd3653`, e as seis perguntas/gabaritos originais. Mais seis perguntas estruturais foram registradas antes de executar a nova ordenação; os testes também usam um projeto sintético separado com Rust, TypeScript e Python, documentos de intenção e um módulo de diagnóstico repetitivo. É avaliação local pelo executor, sem gabarito semântico independente. Nenhuma nota específica foi acrescentada para fazer as perguntas acertarem.

A descoberta passa a alternar evidência de responsabilidade documentada com o índice híbrido existente. A frequência conta por arquivo, e a melhor declaração conta uma vez: repetir muitas funções ou o mesmo cabeçalho não acumula relevância. Identificadores exatos preservam prioridade. A primeira passagem diversifica arquivos; símbolos suplementares vêm depois. Todo o espaço inicial pode servir à descoberta, e relações estáticas únicas preenchem lugares livres. Relações ambíguas não ocupam esses lugares por expansão; continuam visíveis e inspecionáveis. A ordenação dos candidatos usada pelo filtro pago existente permanece separada.

A consulta inicial guarda assinatura/comentário curtos, fonte íntegra por hash, contratos e rotas extraídos, amostras de ligações e contagens. `--detail` expande os mesmos símbolos; Markdown usa detalhe automaticamente. Trechos compactados e relações omitidas são indicados. A consulta lê pacotes tipados diretamente do SQLite, com catálogo/história para o relatório, sem montar o mapa inteiro com todas as declarações, textos e recibos Git. Consulta sem sinal no índice não transforma o reservatório amplo do filtro em evidência; falha do índice aparece como fallback nas lacunas.

No contexto preparado, interpretações longas têm trecho curto e caminho de expansão. A versão usada no cache deriva da evidência integral, incluindo fontes e texto além do trecho visível. O pacote excessivo conserva versão e orientação de expansão, em vez de desaparecer por inteiro. Fontes secundárias continuam verificadas contra a cópia da onda.

| Pergunta | Antes encontrou o destino | Agora encontrou o destino | Bytes da resposta |
| --- | --- | --- | --- |
| levantamento de spec e pontos do pedido | Não | Sim | 34.537 → 13.880 |
| despachar ondas e tarefas | Não | Sim | 33.100 → 17.335 |
| contar consumo de conversas por projeto | Não | Não | 29.189 → 13.817 |
| publicar snapshot Cloudflare | Sim | Sim | 35.209 → 14.417 |
| contexto preparado da cópia da onda | Não | Não | 32.286 → 13.974 |
| cache por evidência de julgamento Jev | Sim | Sim | 21.093 → 13.126 |
| localizar implementação dos métodos de contrato | Sim | Sim | 32.791 → 15.204 |
| montar pedido de revisão final | Não | Sim | 31.996 → 15.161 |
| conferir hashes de múltiplas fontes de interpretação | Sim | Sim | 30.203 → 15.133 |
| projeto da conversa de agente em cópia já apagada | Sim | Sim | 31.331 → 14.212 |
| base de integração com commit próprio merge ff only | Sim | Sim | 30.621 → 15.752 |
| renderizar documentação Markdown do conhecimento | Sim | Sim | 27.132 → 14.659 |

No conjunto original: **2/6 → 4/6**. Nas seis perguntas adicionais: **5/6 → 6/6**. Total: **7/12 → 10/12**. Somando as doze respostas: **369.488 → 176.670 bytes**, cerca de **52% menos bytes**. Isso não mede tokens faturados nem economia de dinheiro. As duas buscas ainda incompletas são consumo de conversas no comando da spec e contexto preparado da onda. Não ajustamos o gabarito para ocultar essas falhas. Já encontram componentes relacionados, mas não o arquivo de implementação específico exigido pela régua.

As novas consultas ficaram entre **1,93 e 2,27 segundos** nesta execução. A versão anterior rodou primeiro, a nova depois, no mesmo banco já utilizado em ensaios: ordem, caches de sistema e apenas uma medida por pergunta impedem afirmar um ganho estável de latência. Sem comparação paga, `rg`/outros motores ou realização de specs pelo modelo, não há conclusão sobre custo ou qualidade do código gerado.

Resultados por pergunta, versões/hashes dos executáveis e hashes dos destinos estão em [registro do comparativo](../apps/scan/tests/fixtures/knowledge-comparison.json). O executável da melhoria nesse ensaio era da árvore de trabalho antes do commit final; o hash identifica exatamente o programa medido. Os artefatos posteriores de revisão são compilados novamente a partir do commit final limpo. Nenhuma API de modelo ou upload remoto foi utilizado.

A suíte completa aprovou **3.809 testes Rust**, sem falhas, com dois ignorados herdados. Lint estrito aprovado. O aceite nativo inclui instalação/scan em pasta vazia, projeção curta/detalhada dos mesmos símbolos, pergunta inexistente sem resultado inventado, exportação Markdown, recibos e invalidação por fonte secundária. Os mapas de sessão seguem abaixo de 3 kB em ambos os idiomas. Isso verifica os contratos; ainda precisamos de avaliação de negócio independente para aceitar um oráculo completo.

Pacote de revisão em `target/review-plugin`, com manifesto em `target/review-plugin-manifest.json`: três executáveis compilados do commit final limpo. A instalação nativa foi conferida novamente depois dessa compilação; manifesto/hooks do pacote aprovados pelo validador oficial. Logs locais: `/tmp/mustard-knowledge-build-checked.log`, `/tmp/mustard-knowledge-native-acceptance.log` e `/tmp/mustard-knowledge-package-validate.log`. O pacote continua isolado da instalação pessoal e a validação local não substitui o ensaio no host Claude Code.

## Conceitos da pesquisa aplicados ao produto

Continuação autorizada em 08/10/2026. Implementação própria em Rust, reaproveitando o banco e o parser existentes; sem instalar os projetos consultados nem copiar seus motores. A comparação de 12 perguntas acima permanece um registro da versão anterior, não uma nova medida desta continuação.

| Referência | Aplicação no Mustard | Benefício e limite |
| --- | --- | --- |
| Code Context Graph | Metadados explícitos `@intent`, `@domainRule`, `@requires`, `@ensures`, `@sideEffect`, `@mutates`, `@index` na documentação ligada a uma declaração | Vocabulário humano pesquisável, condições e efeitos organizados sem geração por IA. São declarações do autor; não provam o comportamento. |
| Codebase Memory MCP / Serena | Separar descoberta de navegação por identidade exata; percurso de chamadas ou consumidores com profundidade, origem de cada passo e fontes atuais | Investigar impacto para a spec/ondas sem ler arquivos inteiros ou misturar homônimos. Aplicamos o conceito; não incorporamos MCP, servidores de linguagem ou o motor desses projetos. |
| ContextGraph | Fila explícita das interpretações que perderam validade, com fonte anterior, motivo e hash atual do arquivo | Reavaliar somente conhecimento afetado. A comparação usa conteúdo de todas as fontes, não apenas inventário de símbolos. Não redige texto nem renova recibos automaticamente. |
| ckg / CodeGraph-Rust | Consulta inicial curta, expansão detalhada, SQLite e trabalho mecânico no binário | Anotações também são projetadas sob demanda; o banco preserva o conteúdo extraído. Não introduzimos dependências desses projetos. |
| SCIP | Conservar resolução ambígua como candidata e identidade exata na navegação | Princípio de precisão aplicado. Importação real de índices SCIP continua pendente; identidade local por arquivo/linha/nome não é um identificador SCIP. |

### Intenção e regras explícitas

Tags devem começar uma linha na documentação imediatamente associada à declaração. A normalização preserva os intervalos originais antes de juntar a prosa. Continuação vale até linha vazia ou próxima tag; tags desconhecidas interrompem a associação. Exemplos cercados por cercas de código, e-mails, menções no meio da prosa e textos literais do corpo não viram anotações. Não exigimos editar todo o projeto: a busca continua usando comentários comuns e interpretações com fontes.

Exemplo de comentário que o parser pode extrair:

```text
@intent Recuperar o plano
@requires Aprovação do responsável
@domainRule Preservar as tarefas concluídas
@sideEffect Persiste o plano recuperado
```

Cada anotação guarda tag, texto e linhas, vinculados ao arquivo/hash da declaração. O ranking local aproveita esse vocabulário. Resposta inicial traz até seis anotações com trechos curtos, contagem e indicação de compactação; `--detail` devolve o conteúdo armazenado, e Markdown o organiza como declarações do autor. Sem anotações, esses campos não acrescentam texto à projeção curta.

O contexto preparado das ondas incorpora anotações pertinentes ao componente, como evidência candidata. A versão do cache inclui o conteúdo completo, mesmo quando a apresentação foi encurtada. Fonte alterada perde validade na própria cópia da onda. As anotações também persistem nas declarações para sobreviver à reutilização incremental de arquivos; a versão do bloco é atualizada e o digest do scan inclui as regras de extração.

### Navegação e impacto por símbolo

Primeiro descubra o ponto de entrada e copie o `id` devolvido, então navegue:

```sh
mustard-rt run knowledge --query "recuperar plano"
mustard-rt run knowledge --symbol "<id devolvido>" --direction callers --depth 2 --detail
mustard-rt run knowledge --symbol "<id devolvido>" --direction outgoing --depth 1
mustard-rt run knowledge --symbol "<id devolvido>" --direction both --all --markdown --out impacto.md
```

`--symbol` seleciona uma declaração exata, sem rodada de busca semântica ou seleção de notas não relacionadas. `callers` percorre consumidores; `outgoing` chamadas; `both` os dois sentidos. `--file` e `--query` pertencem à descoberta e não são combinados com essa seleção exata. ID ausente ou fonte antiga não conduz à função de mesmo nome: recebe ausência de evidência e orientação de investigação.

A navegação usa somente vínculos `unique-static-target`, com recibo do destino compatível com a declaração atual. Ambos os extremos são conferidos no checkout investigado. Ciclos não duplicam declarações. A resposta explica cada passo e distância; declarações fora da profundidade/apresentação, ou antigas, são contadas. O percurso padrão é de duas etapas, com máximo de quatro; `--all` amplia apresentação, não esse máximo. Esses parâmetros descrevem o contexto, sem limitar gasto financeiro. Ausência de vínculo não prova ausência de consumidor; macros, reflexão, chamadas indiretas e resolução parcial continuam lacunas.

### Revisão incremental do conhecimento

```sh
mustard-rt run knowledge --refresh
mustard-rt run knowledge --refresh --query "recuperação" --detail
mustard-rt run knowledge --refresh --file src/store.rs --markdown --out revisoes.md
```

A fila informa as fontes alteradas/removidas/inválidas e mantém as fontes intactas necessárias à conferência. O texto anterior aparece apenas com detalhe, rotulado como antigo. Os campos de evidência atual ficam vazios nessa operação. Hash atual e quantidade de linhas não renovam a interpretação nem identificam automaticamente a posição de uma função que mudou: o responsável deve atualizar o scan, consultar os símbolos afetados, conferir o significado e registrar novo recibo explicitamente. Consulta e exportação não escrevem novo conhecimento nem geram publicação externa.

### Onde entra na condução da obra e no Jev

- Levantamento/spec: recuperar intenção e regras documentadas, conferir condições/lacunas e expandir somente os símbolos relevantes.
- Separação em ondas: consultar consumidores e chamadas das funções afetadas para sustentar dependências e detectar possíveis interferências. Grafo parcial não aprova paralelismo automaticamente.
- Execução: usar evidência atual e anotações do componente preparado; evitar reenviar o inventário inteiro a cada tarefa.
- Revisão final: seguir consumidores que exigem conferência e usar a fila para localizar narrativas antigas. Resumos de ondas orientam leitura, com conclusões conferidas nas fontes.

Nenhuma dessas operações chama Jev. O julgamento continua reservado a perguntas tipadas que permaneçam ambíguas após a recuperação local, com pergunta/evidência/revisão determinando o cache. Anotação escrita e grafo estático não devem virar um julgamento pago por declaração. Interpretação de negócio nova pertence à revisão/raciocínio do responsável ou gerador separado, não à exportação mecânica.

Aceite específico: testes de anotações em documentação de três gramáticas, textos literais/falsas tags, linhas originais, persistência incremental real em Git e invalidação do contexto; navegação por símbolo, consumidores, profundidade, ciclos, homônimos, ambiguidade, recibo incompatível e cópia divergente; fila de revisão com fonte secundária alterada/removida sem renovar o recibo. Isso mede contratos de recuperação, não melhoria comprovada na qualidade de specs/código ou redução de cobrança.

Suíte desta continuação: **3.815 testes Rust aprovados**, sem falhas, mais os dois ignorados herdados. Log `/tmp/mustard-concepts-workspace.log`; lint estrito aprovado em `/tmp/mustard-concepts-clippy.log`. Mapas de sessão com 2.468/2.500 bytes preservam a retomada e o estilo; os detalhes novos ficam disponíveis sob demanda. Build e aceite operacional dos executáveis são registrados em `/tmp/mustard-concepts-build.log` e `/tmp/mustard-concepts-native-acceptance.log`, com resultado JSON em `/tmp/mustard-concepts-native-acceptance-result.json`. Validação do pacote em `/tmp/mustard-concepts-package-validate.log`; versão/hash dos programas no manifesto de revisão. Nenhuma dependência dos motores consultados foi acrescentada.


## Continuação: cobertura de esquemas e retirada da geração local

Implementado na mesma branch `codex/mustard-plano-completo`. O motor continua agnóstico: Prisma entrou como gramática MIT (`tree-sitter-prisma-io =1.6.0`), registro de linguagem, queries e fixture. Nenhuma condição com nome de linguagem/framework foi adicionada ao motor. Ao contrário das etapas anteriores que apenas aproveitaram conceitos de projetos pesquisados, esta etapa acrescenta uma dependência de parser de terceiros, com licença MIT; não incorpora aqueles motores de grafo/wiki.

A seleção conserva a identidade da declaração que ganhou a classificação local de responsabilidade. A grafia exata de um identificador precede referências com outra capitalização. Notas exigem os termos informativos da consulta em seu próprio texto; um termo genérico ou caminho compartilhado não basta. A ordenação ainda pode escolher uma classe abrangente em vez do método desejado.

### Operação nativa como padrão

A orientação posterior do usuário descarta Ollama e prioriza todas as operações auxiliares sem IA e sem Jev. O adaptador local, a opção `--enrich` e a configuração executável desse gerador foram retirados. A porta de geração independente permanece como contrato de biblioteca para uma eventual alternativa futura; o runtime não possui gerador ativo. Configurações antigas `knowledge` são preservadas como dados desconhecidos, sem execução.

A chave `TYPESAFE_API_KEY`, um antigo `search.filter: "jev"` ou o cache de vetores não ligam mais IA por conta própria. O padrão do scan e da recuperação é lexical/estrutural. A decisão de usar uma assistência excepcional fica explícita em `mustard.json`; nenhum instalador grava essas autorizações automaticamente:

```json
{
  "ai": {
    "fallback": false,
    "vectors": false
  }
}
```

Omitir o bloco produz o mesmo comportamento. Valores ausentes, inválidos ou diferentes do booleano `true` não autorizam inferência. O modo excepcional de julgamento exige `ai.fallback: true` e um filtro `jev` explícito na finalidade; `search.filter` só pertence à busca, e não ativa julgamento de contexto ou de ondas. Essa autorização permite o adaptador existente; não constitui prova de que uma alternativa nativa é insuficiente. A decisão de adotá-lo deve vir da avaliação por etapa. `ai.vectors: true` autoriza separadamente o modelo estático já embarcado; vetores existentes não são consultados sem essa escolha. A opção é preservada para comparação, sem recomendação de ativação.

A recuperação nativa usa vocabulário explícito em `packages/core/src/domain/knowledge/retrieval.txt`, formas morfológicas e tipos neutros de declaração. Equivalências ocupam o mesmo espaço da palavra original, sem multiplicar seu peso. Uma pergunta de ação favorece funções/métodos; uma pergunta de estrutura pode favorecer campos/tipos. As regras não incluem termos do Suzano, nomes de frameworks ou nomes de linguagens. Um candidato com evidência própria em vários termos pode competir mesmo quando o índice geral não o trouxe; caminho e cabeçalho herdado sozinhos não sustentam essa inclusão. Busca por identificador exato mantém prioridade. São heurísticas para localizar fontes, não certificação de significado.

Consulta, navegação, revisão de fontes antigas e documento continuam disponíveis:

```text
mustard-rt run knowledge --query "<capacidade delimitada>"
mustard-rt run knowledge --symbol "<id>" --direction callers
mustard-rt run knowledge --refresh
mustard-rt run knowledge --query "<capacidade delimitada>" --markdown --out levantamento.md
```

Não há geração de texto escondida numa consulta sem resultado, num hook, numa rodada ou na exportação. Lacunas continuam visíveis. Interpretações já conferidas por uma pessoa/agente podem ser registradas com `--record`; aproveitar uma interpretação atual não inicia outra inferência. Regras obrigatórias e dependências explícitas das ondas são conservadas pelo caminho nativo, sem depender de exclusão semântica de itens por Jev.

O documento recebido de referência combina fatos e interpretação. A montagem do Markdown é nativa; uma narrativa de negócio completa só poderá afirmar o que suas fontes e interpretações sustentarem. Ausência de IA não autoriza fabricar jornadas ou transformar relação estática em prova de execução.

### Evidência medida e limite atual

No backend Suzano, no mesmo commit `a3fe37ab454cede37d3471993eb876985fcdfe1b`, foram indexados **20 arquivos Prisma e 2.428 declarações**: 114 estruturas, 30 enums, 123 membros de enum e 2.161 campos. Relações e modificadores ficam acessíveis como evidência de origem/cabeçalhos; isso não é validação do comportamento do banco ou extração formal de toda regra de negócio.

O conjunto de 22 perguntas permaneceu congelado; excluindo a pergunta N11 cuja premissa de notificação em tempo real era falsa, o resultado global continua em **16/21 consultas válidas**, com **12/17 perguntas naturais** e **4/4 identificadores literais**. A pergunta de tabelas N14 passou; a pergunta de cálculo de indicadores N09 deixou de recuperar o motor esperado. O índice ampliado também modifica a distribuição de termos e pode promover uma entidade que guarda indicadores. Não há evidência de ganho global de precisão ou de economia de tokens pagos nesta rodada. As métricas de símbolo por pergunta usam o gabarito congelado e não equivalem à correção da resposta de negócio.

Os testes do gerador utilizam respostas simuladas. A suíte completa passou com 3.824 testes e dois ignorados herdados; as últimas mudanças de exportação também passaram por aceitação nativa e análise estática sem avisos. A aceitação executa os binários reais, instala em pasta vazia e testa geração explícita, exportação Markdown sem geração adicional, destino inválido antes de inferência, reaproveitamento, recusa de fonte alterada, mudança de digest, bloqueio de modelo remoto, citações inválidas e concorrência com uma única geração. Isso comprova o contrato e a mecânica; não comprova qualidade, latência ou custo de um modelo real. O projeto original ficou intacto; não houve chamada paga ou publicação.

Artefatos desta rodada ficam em `target/suzano-oracle-20261008-v2/`; a avaliação anterior foi preservada. Próximas prioridades: separar intenção de consultar dados de intenção de localizar implementação, preservar recall ao ampliar linguagens e avaliar o gerador real com perguntas novas, fontes conferidas e métricas de resposta. Um documento completo do Puzzle ainda exige vários tópicos, evidência suficiente e revisão das hipóteses. A integração atual não promete produzir essa análise completa em uma única chamada.


## Avaliação pontual do Jev em todo o Mustard

Orientação do usuário: concluir primeiro as alternativas nativas e avaliar Jev ao final, incluindo o projeto inteiro. A tabela é uma recomendação baseada no código e nos contratos; não habilita um provedor nem demonstra ganho de julgamento pago. Nesta continuação, nenhuma alternativa demonstrou precisar de Jev para funcionar.

A documentação consultada em 08/10/2026 recomenda decisões pequenas sobre evidência relevante e combinação das respostas no código. [Choice](https://docs.typesafe.ai/primitives/choice) escolhe uma opção; [Noul](https://docs.typesafe.ai/primitives/noul) expressa uma probabilidade para uma afirmação; [Score](https://docs.typesafe.ai/primitives/score) exige uma escala definida. Não há geração de prosa. Perguntas de uma chamada compartilham o mesmo estado, mas cada resposta é independente; uma pergunta não pode depender da resposta de outra nessa mesma chamada. [Introdução](https://docs.typesafe.ai/introduction), [estado](https://docs.typesafe.ai/concepts/state).

| Etapa e local do código | Alternativa nativa ou decisão já disponível | Jev teria lugar? |
| --- | --- | --- |
| Scan: `apps/scan/src`, `domain/knowledge`, `io/knowledge` | Parser, símbolos, comentários, metadados, rotas, referências, Git e hashes; consulta/exibição/Markdown | Nenhum uso rotineiro. Um parser sem suporte ou uma relação sem evidência não ganha validade por julgamento. SCIP/LSP é uma investigação nativa para resolver tipos e referências. |
| Busca: `shared/search_door.rs`, `shared/word_search.rs` | Identificador exato, busca textual com argumentos preservados, ranking e navegação | Candidato para experimento: pertinência entre resultados que já foram encontrados, quando a intenção permanece ambígua. Pergunta por candidato com evidência própria e tarefa relevante. Jev não recupera função ausente da lista nem cria uma tradução de negócio que o banco desconhece. |
| Levantamento e criação da spec: eventos/`run read`/`run write`, `flow/grill.rs` | Estado de perguntas, decisões, itens e aprovação; modelo principal raciocina sobre necessidade e registra conclusão | Classificação auxiliar só teria interesse numa ambiguidade recorrente mensurada. A conversa já exige o raciocínio do responsável: pagar de novo para classificar cada mensagem, redigir spec ou descobrir regras do sistema não se justifica. |
| Contexto das ondas: `flow/round/item_choice.rs`, `domain/wave_prompt`, `io/wave_prompt/prepared.rs` | Regras obrigatórias, arquivos/itens vinculados, dependências e evidência atual; reutilização por componente | É o candidato mais plausível a um piloto: decidir se um item **opcional** não resolvido por vínculos governa uma tarefa. Estado restrito ao item, tarefa e fontes pertinentes. Nunca remover regra obrigatória por probabilidade. Medir se reduz leituras sem perder aceites. |
| Formação de ondas: `shared/judgement.rs`, `shared/dag.rs`, `flow/round/backlog.rs` | Dependências declaradas, reservas de escrita, padrões de arquivo, leituras/critério/afinidade e prioridades | Não perguntar por conflito já conhecido. Um possível piloto seria interferência semântica residual entre dois recursos com evidência concreta. Pasta em comum, sozinha, é insuficiente para justificar chamadas. Se decidir exige análise extensa, cabe ao responsável/LLM principal. |
| Tipo e tamanho de tarefa: perfis em `shared/jev.rs` | Tipo pode ser declarado pelo autor da tarefa; tamanho do pedido/contexto é calculável; incerteza recebe tratamento conservador | Não recomendo ativar o perfil pago para toda tarefa. A estimativa semântica de tokens de implementação não é medição. Primeiro exigir metadado explícito e usar conteúdo/consumo observado; não há ganho real comprovado para as notas atuais. |
| Validação leve e fechamento: `flow/round/checks.rs`, `flow/close.rs`, recibos/provas | Comandos reais, saída/status, conteúdo/configuração/ambiente e autoria | Jev não comprova execução, cobertura, ausência de regressão ou cumprimento da spec. Leitura/raciocínio da revisão final continua necessário; o binário prepara evidências e verifica contratos. |
| Gasto, statusline, Mods, páginas e publicação: `spec/measure`, `commands/panel`, `commands/statusline`, `io/publication` | Eventos e dados observados, cálculos, projeção, layout, sanitização e transporte explícito | Nenhuma chamada para atualizar a tela, contar tokens, gerar gráfico, publicar, montar Markdown ou escolher automaticamente periodicidade. |
| Registro e revisão do conhecimento: `io/knowledge::record`, `knowledge/refresh.rs` | Reaproveitar interpretações atuais, listar fontes alteradas e conferir recibos | Jev não redige a nova interpretação e hash não prova o significado. O responsável/LLM principal confere apenas o trecho afetado e registra o que concluiu. |

### Critério para um piloto excepcional

Primeiro registrar a falha nativa em casos com resultado esperado independente. Distinguir ausência de candidato de ambiguidade entre candidatos: somente a segunda é adequada a classificação. Testar regras/metadados/navegação adicionais antes de introduzir inferência. Comparar o fluxo nativo e o assistido no mesmo código e nas mesmas tarefas, mantendo modelo principal e esforço.

O piloto precisaria medir acerto/omissões, leituras adicionais, tokens totais do modelo principal, chamadas e tokens físicos do Jev, latência e custo total. Uma chamada menor não basta se aumentar correções ou omitir uma regra. Probabilidade intermediária e resposta inválida devem conservar incerteza e o fluxo nativo; não significam ausência de recurso ou aprovação de paralelismo.

Se houver benefício, manter finalidade explícita e independente (`search`, `context` ou `wave-planning`), a porta `JudgementProvider`, cache por pergunta/fontes/configuração/revisão de provedor, exclusão entre processos e diário de tentativas físicas. Mudança nas fontes invalida reaproveitamento. Agrupar apenas perguntas que de fato precisam do mesmo estado, sem repetir o repositório/transcript inteiro. Relevância e evidência, não um teto arbitrário de gastos, devem determinar a necessidade.

Recomendação atual: manter o padrão nativo. Investigar primeiro seleção de **contexto opcional**; em seguida, pertinência de candidatos ambíguos. Interferência semântica residual exige prova mais forte. Não existe evidência para reativar Jev em todo o fluxo, nem para prometer que qualquer um desses pilotos reduzirá custo total.
