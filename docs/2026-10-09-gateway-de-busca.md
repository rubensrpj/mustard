# Gateway de busca e aprendizado local

Implementação em `codex/mustard-plano-completo`, na cópia isolada de desenvolvimento. Esta etapa sucede a investigação orientada à tarefa registrada em `2026-10-08-scan-oraculo.md`: a entrada principal pesquisa o código antes de consultar o banco. A instalação pessoal não é atualizada automaticamente.

## Fluxo entregue

1. O adaptador recebe ferramenta, parâmetros originais, intenção e finalidade.
2. O binário aplica as regras existentes de acesso e executa a busca nativa. Não executa uma string de shell recebida no pedido.
3. As ocorrências retornadas são conferidas no arquivo atual e registradas com caminho, linha, texto, hash e identidade do checkout.
4. Arquivos descobertos ou alterados solicitam o scan estrutural incremental nativo. Nenhum modelo de significado é carregado nesse caminho. O resultado da pesquisa é reaproveitado, sem executar a mesma busca novamente.
5. As ocorrências são cruzadas com declarações da mesma versão. O retorno acrescenta funções proprietárias, documentação, contratos disponíveis, referências candidatas e intervalo exato para leitura. O histórico pode ser expandido pelo comando indicado.
6. A intenção ajuda a recomendar candidatos. Só uma ambiguidade restante, com intenção e autorização explícita `choose`, pode chegar ao seletor configurado. A seleção nunca remove ocorrências da resposta original.
7. A apresentação automática escolhe o resultado original ou um agrupamento menor que conserva todas as ocorrências. Funções/faixas entram quando a economia de repetição comportar essas referências. Falha de banco/scan não apaga achados reais.

## Entrada e retorno

```sh
mustard-rt run search --shell-output --intent "corrigir a persistência" --purpose implement -- rg -n --with-filename "save|persist" src
```

Sem flag de apresentação, o CLI retorna o diagnóstico JSON: `result`, `evidence`, `learning` e contadores. Esse relatório completo não é injetado automaticamente pelo Mods/hooks. O padrão `locate` prepara referências curtas; as demais finalidades preparam metadados específicos. Expansão explícita: `run knowledge --symbol <id>`; o JSON também conserva comandos/faixas de leitura e história. O padrão de pesquisa nunca é reescrito pela intenção.

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

`--raw` preserva bytes de stdout/stderr e código de saída dos executáveis nativos. `--shell-output`, usado pelo Mods/hooks, retorna o resultado original ou um agrupamento menor, sem descartar/reordenar linhas ou duplicatas. `@ caminho` abre um grupo; cada linha mantém `número:texto`. `# static owners:` identifica funções e faixas estáticas conferidas. Candidatos cujo relatório aumentaria a resposta conservam o resultado original/agrupado sem esses extras. Isso compara duas representações completas; não corta resultados para cumprir um limite. Formatos mistos, contexto, JSON nativo, saídas não reconhecidas e erros preservam a saída original.

O modo de agente também aceita ferramentas tipadas: usa o resultado paginado de Grep, incluindo offset/continuação; nunca o stdout não paginado do subprocesso. Glob/Read/count preservam o objeto de resultado do adaptador, sem acrescentar o relatório. Read conserva arquivo, offset e total de linhas. `--raw` continua exclusivo de executáveis nativos. stderr e falhas do host ficam visíveis no Mods. Um `choose` explícito pode acrescentar a recomendação solicitada, identificada como tal, além do tamanho da busca original; não é o comportamento das buscas comuns.

`run map search` encaminha ao mesmo gateway e retorna o diagnóstico JSON. Os agentes usam a porta de apresentação, não essa projeção completa. Consultas analíticas `knowledge` permanecem para expansão/investigação explícitas.

## O que alimenta o banco

As tabelas `search_files` e `search_facts`, no bloco persistente `search_facts` de `grain.db`, guardam observações verificadas por checkout. Repetições reutilizam registros; uma nova versão do arquivo substitui os fatos anteriores. O scan preserva esse bloco. Novas funções, assinaturas e relações entram pelo parser nativo, sem interpretação inventada a partir da consulta.

`learning` informa novos/reutilizados, hashes, arquivos pendentes e resultado do scan. O marcador de versão reconhecida evita repetir um scan apenas porque um arquivo não tem declarações suportadas. Um checkout ligado recebe seu próprio índice estrutural; as observações na âncora continuam separadas por árvore. O índice principal não é substituído pelo código de outra branch.

Essa memória não vence o código atual. A busca começa na fonte e toda identidade enriquecida precisa ter o mesmo hash. Intenção, classificação do Jev e suposições sobre comportamento não viram fatos persistentes. Interpretações revisadas continuam usando recibos próprios e suas fontes.

## Intenção e Jev

A descrição disponível da ferramenta é capturada como intenção. Com vários proprietários e nenhuma intenção, `evidence.intent_requested` sinaliza a necessidade de informar o objetivo. O agente pode completar a intenção; uma busca exata não precisa de pergunta adicional. O Mustard não tenta reconstruir o raciocínio privado do agente.

O Jev usa a interface existente `SymbolSelector`, cache, critérios de aceitação e registro de tentativas. Só recebe candidatos atuais que a seleção nativa não resolveu. Uma escolha só é aceita se pertencer ao grupo/arquivo enviado; fica destacada para expansão, sem eliminar os demais resultados. A interface permite outro provedor no futuro. Busca comum, registro dos achados, atualização estrutural e painel não ativam esse seletor. Ter uma chave configurada não ativa chamadas por pesquisa.

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

`apps/scan/tests/search_gateway.rs` verifica paridade nativa, proprietários/intervalos atuais, paginação, novas versões, persistência e escolha apenas em empates autorizados. Testes do runtime exercitam o gancho completo e o CLI; testes oficiais do Mods conferem transporte pelo host e preservação de recusas.

`node apps/scan/benchmarks/gateway-acceptance.mjs` cria uma pasta realmente vazia, instala com o binário local e verifica descoberta, atualização automática, deduplicação sem novo scan, fallback, ferramentas tipadas, hooks e isolamento de worktrees. Não executa código do repositório pesquisado nem chama API paga. Relatório e hashes: `target/scan-gateway-20261009/native-acceptance.json`, separado dos benchmarks históricos.

Os testes comprovam funcionamento; a comparação de apresentação acima mede bytes das mesmas pesquisas, sem economia faturada ou melhora no código produzido. Falta comparar tarefas equivalentes em sessões reais: buscas adicionais, contexto total, custo, tempo e implementação correta. Uma sessão autenticada também deve confirmar a ergonomia do encaminhamento no Claude Code.
