# Scan como oráculo de recursos do projeto

Estado em 08/10/2026. Implementação na branch `codex/mustard-plano-completo`, isolada da instalação pessoal. A continuação e a aplicação da pesquisa ao scan foram autorizadas nesta data. Este documento amplia a trilha C do plano; não declara concluído um entendimento completo das regras de negócio.

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
- A recuperação usa o índice local já existente de palavras e vetores estáticos embarcados. Esse modelo local não redige explicações nem chama serviço pago. Interpretações registradas também podem localizar funções pelo vocabulário de negócio.
- Relações são conferidas dos dois lados: uma função intacta não mantém como atual um chamador cujo arquivo mudou. A cópia de onda é verificada contra seu próprio conteúdo.
- Interpretações têm várias fontes. Mudança em qualquer fonte exclui a interpretação das consultas atuais. Atualização do scan não renova silenciosamente uma interpretação antiga.
- A consulta informa a origem da recuperação: índice híbrido local, termo no fallback, fonte de interpretação ou relação estática. Parsing parcial, relações ambíguas, resultados omitidos e fontes antigas permanecem visíveis como lacunas.
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

Não acrescentamos chamada paga à consulta `knowledge` nem um varrimento de todo o projeto pelo Jev. As portas de julgamento do plano B continuam independentes do provedor, com cache por evidência/pergunta/revisão, exclusão entre processos e medição de tentativas físicas. Não confundir essa interface de julgamento com uma futura interface de geração de texto.

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

Para eliminar a procura manual com segurança, a consulta também precisa dar expansão por símbolo e impacto em consumidores, não apenas um grande resumo. A arquitetura do banco e o contrato JSON permitem um adaptador futuro para outros clientes; isso não significa suporte operacional ao Codex nesta versão.

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
