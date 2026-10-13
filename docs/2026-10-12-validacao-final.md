# Validação final das correções e das lacunas de produto

Continuação após a v0.2.17: [conferência do escopo de 12/10 e recibos de observação externa](2026-10-12-fechamento-do-escopo.md), na branch local `codex/close-today-scope`, ainda sem PR. Os números abaixo pertencem à etapa anterior; não devem ser apresentados como medições novas desta continuação.

Base de comparação: **v0.2.16 publicada**, commit `716c7287ba3f28f840c6542105701e0dfae77f11`. Continuação na branch `codex/finish-product-gaps`, em worktree isolada. A [lista de 86 pedidos](2026-10-12-pedidos-e-entregas.md) discrimina implementação, evidência e aceites externos. Os relatórios anteriores são históricos, não o estado de uma instalação atual.

## Correções concluídas no código

- **Busca nativa antes do índice.** Opções `rg -m/--max-count` e `-M/--max-columns`, anexadas ou separadas, preservam o escopo e deixaram de desativar o cruzamento. Opções que limitam ocorrências não comprovam busca exaustiva. Sem ocorrência nativa, o gateway preserva o resultado vazio: não oferece trechos genéricos sobre outro assunto nem aciona Jev para justificá-los. Busca intencional sem uma ocorrência pode ser feita explicitamente por `knowledge`.
- **Menos duplicação.** A projeção retira somente a mesma dependência direta já mostrada, mantendo corpo, coordenadas, relações ambíguas e caminhos diferentes. JSON do fluxo fica compacto, com todos os campos e medição dos bytes realmente emitidos.
- **Relatório do scan.** `knowledge --topics auto --markdown` compõe áreas e evidências sem adivinhar domínio de negócio. Manifestos de tópicos continuam suportados. Inventário e catálogo de arquivo/hash aparecem uma vez; símbolos, assinaturas, comentários, chamadores/chamadas, intervalos, resoluções, interpretações e lacunas continuam referenciados. Relações adicionais omitidas possuem comando explícito de expansão. Um documento interpretativo só contém análise onde existe análise válida.
- **Consumo por envio.** Hooks recebem o caminho real do transcript e vinculam agente ao envio canônico, host e momento. Título semelhante não basta; reenvios não herdam a medição antiga. Codex não conta cache duas vezes; etapas não expostas permanecem desconhecidas. O painel mede recibos sem depender da pasta padrão do Claude. Envios de tarefas retiradas do plano continuam no consumo.
- **Contexto nos dois hosts.** O adaptador Codex também executa os avisos de contexto e a trava de capacidade. `last_token_usage.input_tokens` mede o contexto atual; uso acumulado e cache adicional não entram nessa conta. Compactação confirmada reinicia faixas, uma leitura menor não. Transcript do pai não vale como contexto do filho; usa caminho explícito ou metadata com identidade nativa exata. A onda encerra depois do término aceito da tarefa, conservando testes declarados e resumo; retorno ausente/falho não comprova término. Formatos desconhecidos permanecem sem medida.
- **RTK no Codex.** O adaptador executa a API de reescrita do RTK após as travas do Mustard e conserva os campos da ferramenta. Trata os códigos 0/3 como reescrita válida e 2 como recusa, sem emitir decisão `PermissionRequest`. Falha/ausência preserva o comando original; opt-out e gateway não são reescritos. O teste de instalação usa o RTK empacotado, executa um comando fixo numa pasta descartável, confere o histórico isolado e a projeção de consumo. Configuração dos hooks de Claude/Codex não é apresentada como ativação ou economia observada.
- **Agentes nos dois hosts.** Instalação/migração inclui `mustard-analysis`: Haiku/medium no Claude e Luna/medium no Codex, conforme preferências e capacidades da sessão. Preparo indica agente e ferramenta nativa, com entrada total de 100 mil tokens, reutilização e contrato de recibo. Não inicia outro CLI/API. Exportação/despacho usa o host atual; retomada entre hosts conserva a tarefa aprovada e atualiza opções/referências. Executor Codex herda modelo/effort da sessão; não pede Sonnet/xhigh.
- **Mods e diagnóstico.** Pane solicitado pelo usuário é reaberto ao reconectar; fechamento pelo usuário cancela essa intenção. Perda de referência não impede abertura nova. `/mustard-panel --diagnose` mostra eventos recentes de abertura, attach/detach, renderização e falha. Hooks registram transições e mudanças de pasta por hash, sem publicar caminhos/transcripts. Isso não recupera a causa de arquivos históricos apagados.
- **Instruções.** O agente novo entrou sem aumentar o teto de 25.600 bytes por idioma. Revisão conserva leitura independente, recibos, cortes, prova pelo caminho do usuário e regras de alteração/commit. Delegação depende de benefício delimitado; quantidade de arquivos não a obriga.

As travas entregues na v0.2.16 permanecem: revisão do diff antes de commit, aprovação de mudança de plano, impedimentos persistentes, provas nativas, suplemento de despacho, preparação/Prisma, proteção de credencial, retirada do teto mensal do Jev, configuração no painel, publicação somente explícita e CI sem validação redundante quando existe prova adequada.

## Comparação de busca em fontes congeladas

Os três projetos foram copiados do HEAD conhecido para pastas descartáveis, sem arquivos locais não comitados, credenciais ou specs apagadas. A instalação em uso não foi substituída. Pedidos iguais foram enviados ao executável da release publicada e ao candidato; cada retorno nativo, hash e referência foi conferido. Nenhum código de aplicação, teste de pagamento, API de pagamento ou banco externo foi executado.

| Base | Arquivos / vetores | Pedidos com paridade nativa | Alvos conhecidos visíveis antes → depois | Texto nos controles positivos antes → depois |
| --- | ---: | ---: | ---: | ---: |
| Florestal TypeScript | 1.601 / 32.966 | 10/10 | 5 → 5 | 33.772 → 31.825 bytes (**−5,77%**) |
| Sialia React/TypeScript | 3.392 / 40.261 | 9/9 | 2 → 2 | 26.678 → 25.847 bytes (**−3,11%**) |
| Sialia backend C# | 1.615 / 31.070 | 6/6 | 3 → 3 | 21.756 → 21.191 bytes (**−2,60%**) |

Controles positivos: nove consultas sem a opção que antes desativava o cruzamento, comparando contexto que já existia nas duas versões. Soma **82.206 → 78.863 bytes, −4,07%**, sem perder alvos conhecidos. Os corpos completos dos alvos permaneceram **6/10** nas duas versões; os demais continuam sendo referências que exigem expansão. Não apresentar referência como corpo lido nem como prova de comportamento.

Outras nove consultas positivas, com limites de saída do `rg`, passaram de **0/9 para 9/9** cruzadas com o índice. O resultado nativo sempre esteve disponível; a correção acrescenta contexto que antes faltava. Cinco consultas negativas, contando os pares com/sem limite, agora preservam retorno vazio. Duas delas antes devolviam contexto genérico mesmo sem ocorrência do nome procurado.

O texto agregado dos 25 pedidos **aumentou de 121.697 para 159.192 bytes (+30,81%)**, porque nove pedidos passaram a receber contexto. Isso não é economia total: a redução de 4,07% refere-se apenas aos controles positivos equivalentes. O acerto agregado dos dez alvos conhecidos permaneceu **10/10**, sem ganho de acerto nessa amostra. A amostra foi guiada pela tarefa e pelos controles; não é avaliação independente de precisão/recall.

A release usa otimização diferente do candidato `mustard-dev`; **não se compara latência** entre esses executáveis. Números de bytes não são tokens faturados nem economia de assinatura. Recibos, commits das fontes, hashes dos pedidos/executáveis e resultados por consulta: [evidência JSON](evidence/2026-10-12-validacao-final.json).

Reprodução:

O JSON de evidência inclui `request_fixture` e `label_fixture` de cada base. Extraia-os para os arquivos de pedidos/alvos; ao mudar a pasta da cópia, adapte somente o prefixo absoluto de `Read.file_path`. Os hashes de pedidos registrados correspondem aos arquivos originais da medição.

```sh
node apps/scan/benchmarks/gateway-project.mjs --root <copia-descartavel> --bin <pasta-binarios> --requests <pedidos.json> --out <resultado> --isolated
node apps/scan/benchmarks/gateway-project-audit.mjs --labels <alvos-com-hash.json> --source <copia-descartavel> --baseline <resultado-release> --current <resultado-candidato> --out <auditoria.json>
node apps/scan/benchmarks/gateway-acceptance.mjs --bin <pasta-binarios>
node apps/scan/benchmarks/gateway-local-model.mjs --bin <pasta-binarios>
```

## Relatório do scan

Com os mesmos tópicos, o `.md` Florestal passou de **71.098 para 53.748 bytes (−24,40%)**; Sialia React, de **5.555 para 2.673 bytes (−51,88%)**, sem chamadas remotas. O relatório atual reúne o inventário uma vez, usa um catálogo único de arquivos/hashes e conserva as referências de símbolos/relações. A representação mudou: o relatório prioriza assinaturas, comentários e navegação, em vez de repetir corpos inteiros. Portanto, a redução não é compressão sem perda de todo o código-fonte, nem prova de economia faturada.

O outline automático gerou documentos de **98.188 bytes** no Florestal e **95.838 bytes** no Sialia. São exportações para consulta/compartilhamento, não contexto para injetar integralmente em toda onda. A onda continua recebendo somente evidência pertinente. Onde falta interpretação de negócio, o documento aponta a lacuna e o contrato de enriquecimento pela sessão.

## Jev: execução observada e critério de uso

Piloto autorizado com credencial existente, sem imprimir/gravar a chave: três perguntas sobre término da importação, gravação do resultado e download original. **2 chamadas físicas, 5.379 tokens de entrada, US$ 0,000226 estimados**; repetições vieram do cache, com **zero chamadas**. Nome exato e caso ausente dispensaram provedor. Seleção nativa e com Jev localizaram **3/3** alvos. As respostas ambíguas se abstiveram; **zero substituições aceitas e zero ganho adicional de acerto**. Esse piloto não prova economia nem benefício da especialização.

A decisão correta não é forçar mais uso: Jev entra em alternativas reais de responsabilidade, agrupamento/contexto e perfis autorais revisados, depois dos filtros nativos. Cache, abstenção, fonte atual e travas de aprovação/prova continuam obrigatórios. Para demonstrar ganho, comparar a mesma tarefa com e sem Jev, somando pai/filhos, retrabalho e custo por entrega aceita.

## Verificações e promoção

Verificações locais: **4.094 testes Rust aprovados, zero falhas e dois ignorados**, 25 cenários de contexto Claude/Codex, protocolo RTK com o binário empacotado e medição no painel, build, Clippy estrito, plugin validado, **43 testes Mods**, **22 testes de roteamento CI**, aceitação executável do gateway e modelo vetorial local. A aceitação do modelo cobre padrão ligado, primeira vetorização, ausência de recálculo para fonte intacta, atualização após mudança, raw sem mutação e opt-out, com zero chamadas remotas. A aceitação do gateway cobre handoff, fallback, paginação, referências atuais, cache após compactação e as regressões de limite/resultado vazio.

A promoção autorizada exige PR para dev, seis checks Test/Mods verdes no head exato, PR dev → main e release com smoke tests de Linux/macOS/Windows. Estado final desses passos é o dos checks/PRs e assets do GitHub, não uma suposição deste documento. Não despachar uma segunda execução manual quando a automação já está trabalhando.

## O que continua exigindo a sessão ou conta real

- Pintura/usabilidade do Mods no Desktop Windows por SSH. O teste oficial de hooks cobre o ciclo de vida, não desenha essa interface real.
- Descoberta e execução efetiva de Haiku/Luna pela sessão, versão/effort observados e consumo total. Um agente instalado ou pacote preparado não comprova execução.
- Comparação de implementação genérica/com skills/especializada e economia líquida de uma spec, incluindo recusas, revisão e retrabalho.
- Nova instalação e confiança dos hooks no Codex; o usuário instalará a release. O backend e suas specs removidas não serão recuperados automaticamente.
- Publicação Cloudflare autenticada apenas quando explicitamente solicitada. Nenhuma página foi publicada nesta validação.

Sem essas observações não existe base para afirmar código perfeito, busca perfeita, instalação operacional de todos os projetos ou percentual global de economia. A implementação das correções e os ganhos locais acima têm evidência separada desses aceites.

Referências oficiais aplicadas: [Claude Mods: ciclo de vida e API](https://code.claude.com/docs/en/plugins/mods/reference), [subagentes Claude](https://code.claude.com/docs/en/sub-agents), [subagentes Codex](https://learn.chatgpt.com/docs/agent-configuration/subagents), [hooks Codex](https://learn.chatgpt.com/docs/hooks) e [Jev Choice](https://docs.typesafe.ai/primitives/choice).

O protocolo de reescrita foi conferido no [RTK v0.49.0 empacotado](https://github.com/rtk-ai/rtk/blob/v0.49.0/src/hooks/rewrite_cmd.rs). O `allow` do `PreToolUse` Codex continua a chamada com argumentos substituídos; não é uma decisão do evento `PermissionRequest`. A separação foi conferida no [runtime oficial do Codex](https://github.com/openai/codex/blob/main/codex-rs/core/src/hook_runtime.rs).
