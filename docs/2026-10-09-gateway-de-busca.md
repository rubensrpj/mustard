# Gateway de busca e aprendizado local

Implementação em `codex/mustard-plano-completo`, na cópia isolada de desenvolvimento. Esta etapa sucede a investigação orientada à tarefa registrada em `2026-10-08-scan-oraculo.md`: a entrada principal pesquisa o código antes de consultar o banco. A instalação pessoal não é atualizada automaticamente.

## Fluxo entregue

1. O adaptador recebe ferramenta, parâmetros originais, intenção e finalidade.
2. O binário aplica as regras existentes de acesso e executa a busca nativa. Não executa uma string de shell recebida no pedido.
3. As ocorrências retornadas são conferidas no arquivo atual e registradas com caminho, linha, texto, hash e identidade do checkout.
4. Arquivos descobertos ou alterados solicitam o scan estrutural incremental nativo. Nenhum modelo de significado é carregado nesse caminho. O resultado da pesquisa é reaproveitado, sem executar a mesma busca novamente.
5. As ocorrências são cruzadas com declarações da mesma versão. O retorno acrescenta funções proprietárias, documentação, contratos disponíveis, referências candidatas e intervalo exato para leitura. O histórico pode ser expandido pelo comando indicado.
6. A intenção ajuda a recomendar candidatos. Só uma ambiguidade restante, com intenção e autorização explícita `choose`, pode chegar ao seletor configurado. A seleção nunca remove ocorrências da resposta original.
7. Sem enriquecimento útil, a saída nativa permanece disponível. Falha de banco/scan não apaga achados reais.

## Entrada e retorno

```sh
mustard-rt run search --intent "corrigir a persistência" --purpose implement -- rg -n --with-filename "save|persist" src
```

O JSON contém `result` com a pesquisa, `evidence` com o cruzamento, `learning` com o registro/atualização e contadores de chamadas de modelos. O padrão `locate` retorna referências curtas; `implement`, `spec`, `understand` e `validate` oferecem metadados conforme a finalidade. A expansão continua disponível por `run knowledge --symbol <id>`. O padrão de pesquisa nunca é reescrito pela intenção.

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

`--raw` preserva bytes de stdout/stderr e código de saída dos executáveis nativos. `--shell-output` acrescenta evidência atual quando disponível; sem evidência, conserva a saída nativa. Esses modos são exclusivos dos executáveis nativos; o JSON mantém a paginação dos adaptadores tipados. `run map search` encaminha ao gateway. Consultas analíticas `knowledge` permanecem para expansão e investigação; não se apresentam como execução de um comando literal.

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

Aceite local: 3.886 testes Rust aprovados, dois ignorados herdados, lint estrito e 12 testes oficiais do Mods aprovados. Prova com instalação nativa vazia e zero chamadas de IA também verifica recuperação após falha do scan e expansão de símbolos na própria worktree. A versão antiga de substituição foi retirada do executável; uma fixture histórica, compilada somente nos testes, preserva os comparativos anteriores.

`apps/scan/tests/search_gateway.rs` verifica paridade nativa, proprietários/intervalos atuais, paginação, novas versões, persistência e escolha apenas em empates autorizados. Testes do runtime exercitam o gancho completo e o CLI; testes oficiais do Mods conferem transporte pelo host e preservação de recusas.

`node apps/scan/benchmarks/gateway-acceptance.mjs` cria uma pasta realmente vazia, instala com o binário local e verifica descoberta, atualização automática, deduplicação sem novo scan, fallback, ferramentas tipadas, hooks e isolamento de worktrees. Não executa código do repositório pesquisado nem chama API paga. Relatório e hashes: `target/scan-gateway-20261009/native-acceptance.json`, separado dos benchmarks históricos.

Esses testes comprovam funcionamento, não economia faturada ou melhora no código produzido. A avaliação anterior de recuperação usou outro fluxo e teve respostas maiores; seus números não são prova deste gateway. Falta comparar tarefas equivalentes em sessões reais: buscas adicionais, contexto total, custo, tempo e implementação correta. Uma sessão autenticada também deve confirmar a ergonomia do encaminhamento no Claude Code.
