# Gateway de busca e aprendizado local

Implementação em `codex/mustard-plano-completo`, na cópia isolada de desenvolvimento. Esta etapa sucede a investigação orientada à tarefa registrada em `2026-10-08-scan-oraculo.md`: a entrada principal pesquisa o código antes de consultar o banco. A instalação pessoal não é atualizada automaticamente.

## Contrato de busca: Claude Code primeiro

O Mods exige `{request:{tool,input,intent,purpose,choose?}}`. `purpose` é explícito; `intent` contém a pergunta específica que a consulta deve esclarecer. Investigação ou seleção sem pergunta é recusada antes de executar a pesquisa. Localização literal admite intenção vazia. O erro orienta corrigir o pedido, sem apresentar uma quebra de contrato como motivo para contornar o gateway.

O adaptador acrescenta a versão e chama o CLI com `{"schema_version":1,"request":{...}}`. O contrato vive no domínio do binário, sem tipos do SDK Claude. Versões desconhecidas e campos obrigatórios ausentes são recusados. O CLI conserva o formato anterior para consumidores existentes, aplicando a mesma exigência de intenção nas investigações. O envelope prepara novos adaptadores; não instala integração de ChatGPT/Codex nem presume que esses aplicativos tenham os mesmos hooks.

A ferramenta é o caminho preferido. No Bash original, a descrição `mustard:spec: Verificar se o download usa os dados atuais da cópia` transmite finalidade e pergunta pelo hook, preservando os argumentos nativos. As outras finalidades usam o mesmo formato. Descrição comum continua em `locate`; o binário não adivinha finalidade a partir de prosa. Sintaxe de shell não suportada conserva o caminho original e suas permissões. Não há promessa de interceptar toda forma de leitura de qualquer aplicativo.

Os moldes de sessão, ondas e revisão ensinam o mesmo contrato em português e inglês. O objetivo geral da spec continua no bloco `context`; cada consulta leva a dúvida local, sem repetir a spec inteira. `choose` autoriza o seletor existente apenas diante de alternativas de responsabilidade ainda não resolvidas. O contrato não torna uma intenção automaticamente correta e não comprova economia de uma sessão.

Nesta versão, a integração ativa é Claude Code: Mods, hooks e CLI. A comparação com modelos pequenos permanece uma avaliação separada; o modelo estático opcional existente não é ativado por esta alteração.

Validação do contrato: 28 testes Rust (3 de domínio, 5 de encaminhamento e 20 do gateway), 14 testes do SDK Mods e validação do plugin aprovados. A análise estática de todo o workspace passou sem avisos. Esses testes verificam transporte, correção de pedidos inválidos e preservação das permissões; não medem economia nem substituem a sessão real com Claude Code.

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
5. O pacote inicial entrega corpos/trechos que acrescentam pistas escritas da intenção, além de uma escolha explicitamente recomendada. Tipos que contêm métodos não repetem automaticamente todos os filhos. Os demais candidatos e destinos de chamadas pertinentes permanecem como referências com arquivo, função e faixa para expansão. Cobertura de palavras não significa entendimento semântico ou investigação completa.
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
| [Serena: ferramentas de símbolos](https://github.com/oraios/serena/blob/1de556f71569f3acfc0743e526dd60aca40a545e/src/serena/tools/symbol_tools.py) | Identidade por símbolo e expansão de corpo sob demanda, evitando abrir arquivos inteiros por padrão. Não incorporamos servidores LSP. |
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
