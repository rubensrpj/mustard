# Fechamento do escopo de 12/10/2026

Escopo: os problemas relatados e as decisões tomadas nesta data, conforme as seções 10 e 11 do [plano](2026-10-05-plano-busca-jev-claude.md). A lista histórica de 86 pedidos não transforma cada objetivo geral do produto em uma alteração nova desta etapa.

Base: `main`, v0.2.17, commit `5ec5688894767e2637c4d97ca95d161015011d09`. Continuação na branch `codex/close-today-scope`. Este fechamento e suas verificações foram concluídos antes da abertura do PR consolidado. O backend e as specs apagados pelo usuário permanecem removidos; a instalação da próxima release será feita por ele.

## Conferência dos pedidos

| Pedido/problema | Comportamento implementado | Evidência principal no código |
| --- | --- | --- |
| Falhas de forma devolvidas uma por vez | Validação acumula os problemas do item numa recusa, antes de gravar. Correção anterior preservada e verificada pela suíte. | `domain/spec_events/check.rs`, `commands/spec_events/write.rs` |
| Ponto sem fatos exige versão manual antes de responder | `answer --facts` grava fatos e resposta atomicamente, sem obrigar outra volta ao modelo. | `commands/flow/answer.rs`, `tests/survey_cli.rs` |
| Revisão de bloco repetida sem informação nova | Projeção do levantamento distingue bloco alterado de revisão já realizada; três blocos unitários são cobertos pelos testes. | `domain/survey.rs`, `commands/flow/grill.rs` |
| Busca MCP recusa a própria resposta | Resposta textual compatível com o contrato, incluindo erro, resultado vazio e avisos; sem schema de objeto incompatível. | `plugin/hooks/register.js`, `plugin/hooks/search.test.ts` |
| Argumento `--spec` repetido produz saída inútil | Recusa JSON `duplicate-option` antes de gravar, com a opção problemática. | `apps/rt/src/cli.rs`, `tests/spec_events_cli.rs` |
| Itens difíceis de localizar entre blocos | Leitura por `item-<código>` resolve os tipos; bloco desconhecido informa os blocos disponíveis. | `commands/spec_events/read.rs` |
| IDs de versão mudam e confundem referências | Códigos estáveis resolvem a versão vigente em substituições, pontos e referências. Fechar ponto conserva sua identidade e o histórico. | eventos da spec, `commands/flow/answer.rs`, `tests/spec_events_cli.rs` |
| Proteção da chave cancela comando encadeado inteiro | Leitura da configuração é mascarada; pipes, ordem, condições e etapas posteriores são preservados, com execução única. | `hooks/bash/config_read.rs`, `tests/config_read.rs` |
| Ondas vazias repetindo impedimento de acesso, dados ou decisão | `blocked_by` persiste; tarefas dependentes aguardam; trabalho independente continua. Repetir devolução sem alteração do pedido exige revisão antes de outro agente. | `commands/flow/round/stops.rs`, `queue.rs`, `backlog.rs` |
| Tarefa misturada com sobras que dependem do usuário | Trabalho não concluído é explícito em `undone`; impedimentos não são apagados por aprovação de plano. Reescrever/dividir exige revisão de escopo; o binário não inventa outra tarefa aprovada. | `round/stops.rs`, `round/report.rs` |
| Mudança de plano sem aceite, inclusive sem `changes_decision` | Toda proposta de replanejamento aguarda testemunha de aprovação. Jev e o relato do agente não concedem autorização. | `round/stops.rs`, `round/answer.rs` |
| Commit sem leitura/revisão do diff | `review-wave` entrega diff, escopo e regras; revisão é vinculada ao fingerprint. `USAGE` sozinho não aprova; fonte, entrega, regras ou base pertinentes alteradas invalidam a revisão. | `round/review.rs` |
| Regras de zod, compartilhamento PI/PCP e skills não chegam ao agente | Regras aplicáveis e skills por caminhos são entregues ao executor e entram na revisão. Padrão observado pelo scan não vira obrigação por conta própria. Isso não refatora automaticamente o backend removido. | `io/wave_prompt.rs`, `io/expertise.rs`, `round/review.rs` |
| Texto adicional cortado no despacho/conserto | Suplemento é persistido antes da substituição pelo pedido canônico, incluindo reenvios. Falha de gravação impede perda silenciosa. Não amplia autorização. | hooks de despacho; `io/wave_prompt.rs` |
| Provas recusadas/vazias e comando com `taskset` | Provas nativas de critérios registram comando, resultado, critério e ondas. Comando com `taskset` é aceito; o painel distingue essas provas dos passos e relatos do agente. Não se impõe afinidade de CPU a todos os projetos. | `round/checks.rs`, `round/report.rs`, `commands/panel/mod.rs` |
| Build com Prisma antigo culpava a onda | Compara schema e cliente gerado, executa script de preparação existente quando declarado e preserva entregas. Falha de ambiente não pede retrabalho indevido ao agente. | `round/environment.rs` |
| Retomada com número de onda divergente | Retomada e rodada usam a projeção vigente, desconsiderando ondas esvaziadas. | `commands/flow/resume.rs`, projeção `planned_waves` |
| Avisos repetidos e relatórios enormes | Avisos inalterados são deduplicados por sessão/conteúdo; JSON compacto conserva os campos. Instruções respeitam o teto original de 25.600 bytes por idioma; detalhes permanecem consultáveis. | hooks de contexto/retomada, transporte CLI, `tests/template_budget.rs` |
| Contexto do orquestrador/agente | Orquestrador avisa aos 200 mil e a cada 100 mil seguintes; compactação confirmada reinicia faixas. Aos 150 mil, o agente termina tarefa, teste mínimo e resumo antes de retornar, sem iniciar outra. | `hooks/session/conversation_size.rs`, adaptador Codex |
| Consumo ausente, sessões trocadas e erros sem diagnóstico | Caminho nativo do transcript, identidade, host e envio exato vinculam o uso. Reenvio não herda uso antigo; cache não é contado duas vezes. Mudanças de pasta, eventos e falhas ficam no diagnóstico. Histórico apagado não recebe números inventados. | `shared/host_observation.rs`, `host_observation/transcripts.rs`, painel |
| Resumo antigo apresentado como fato externo atual | Recibos nativos imutáveis vinculam a consulta ao envio, com data e resultado. Fonte de outro envio só pode ser histórica; interpretação livre continua não verificada. | `io/observations.rs`, `shared/tool_observations.rs`, `round/report/observations.rs` |
| RTK não utilizado ou economia confusa | Reescrita Claude/Codex respeita opt-out e saídas 0/3/2. Smoke executa o RTK empacotado e consulta histórico isolado; painel separa compressão observada de economia total ainda não medida. | `host.rs`, `shared/rtk_gain.rs`, `packaging/smoke-install.py` |
| Jev bloqueado pelo teto apesar do saldo | Teto mensal interno removido. Crédito remoto não é inferido pelo gasto local; falha do provedor usa alternativa nativa. Credencial é unificada e protegida. | `io/jev_gate.rs`, portas de julgamento |
| Jev ajudar ondas, perfis, skills e orquestrador sem desperdício | Questões tipadas sobre alternativas reais, cache por evidência, abstenção e agrupamento de questões. Condições inequívocas ficam nativas. Triagem classifica o relato, sem aprovar entrega nem certificar fatos. | `shared/judgement.rs`, `expertise_selection.rs`, `return_triage.rs`, rodada |
| Criar/reutilizar agentes e skills conforme scan e tarefa | Catálogo comum com perfis de navegação e procedimentos autorais revisados. Preparo reutiliza antes de criar; alterações pertinentes invalidam. Conteúdo manual é preservado. Exportação usa formato próprio de cada host. | `io/expertise.rs`, `io/expertise/`, comandos `expertise` |
| Haiku/Luna pela sessão, última versão, medium, 100 mil | Agente `mustard-analysis`, pacote delimitado e contexto isolado; preferência pela família mais recente disponível e effort médio. Entrada total contratada de 100 mil; modelo/effort/uso efetivos separados da preferência. Não abre outro CLI/API. | `io/knowledge/enrichment/session.rs`, templates `analysis.md`, instalação Codex |
| Só analisar ponto novo/alterado e enriquecer o scan | Identidade inclui pergunta/instruções, regras e todas as fontes. Análise atual é reaproveitada entre hosts; conclusão já obtida na onda pode entrar na entrega. Interpretação é hipótese; fonte mudada invalida. | `io/knowledge/enrichment/`, `round/report/knowledge.rs` |
| Busca/scan agnósticos e úteis | Ferramenta nativa primeiro, cruzamento com índice depois; opções/resultado vazio preservados. Funções, comentários, relações, git, vetores locais e referências atuais alimentam navegação. Relatório nativo mostra áreas, fontes e lacunas sem inventar narrativa. | busca/knowledge, [comparações em três projetos](2026-10-12-validacao-final.md) |
| Integração Codex com o mesmo processo | MCP textual, hooks adaptados, patch completo, agentes/skills nativos, testemunhas correlacionadas e telemetria própria. Não importa aprovação/consumo do Claude como se fosse Codex. | `host.rs`, `platform/codex.rs`, hooks e instalação |
| Mods confuso, sem definições/input e só no CLI | Navegação por projeto/spec/definições/ondas, plano e pedido integral, entregas, provas, perfil, consumo e configurações; escuro/amarelo. Reattach reabre apenas pane solicitado; diagnóstico registra abertura/render/falha. | `plugin/hooks/panel-*.js`, `panel-settings.js`, `register.js` |
| Statusline e configuração | Barra compacta com estado e acesso ao painel. Configurações validadas para modelo, Jev, vetores e RTK, com proteção de chave e conflito de edição. Detalhes/métricas ficam no painel. | statusline, `commands/panel/settings.rs`, Mods |
| Instalação completa e uniforme | Instalação/migração incluem executor, revisor, análise, recursos de busca e adaptadores da versão; preservam arquivos manuais. Ativação/confiança do host é distinta da instalação. Não troca runtime de onda ativa. | instaladores, upsert, smoke de instalação |
| Publicação só mediante pedido | Painel local não publica. Projeto/spec/documento externos são snapshots explícitos; configuração permanece em `mustard.json`. | publicação nativa, comandos e ações do painel |
| Não abrir PR por etapa nem duplicar execução | Continuação única; PR permanece condicionado ao fechamento. CI reaproveita prova completa da árvore testada e não dispara release manual duplicada. | política desta entrega, roteamento CI |

## Lacuna fechada nesta continuação: origem das observações externas

A instrução antiga pedia fonte e momento, mas não vinculava uma afirmação a uma execução nativa. Agora o hook grava um recibo imutável por chamada, associado ao host, ator e envio da onda. Claude fornece `PostToolUse` ou `PostToolUseFailure`; Codex fornece `PostToolUse` também para comandos que retornam erro. Falha/interrupção não vira sucesso; código de saída ausente continua desconhecido.

O armazenamento privado `.claude/mustard/observations.db` guarda metadados, comando com segredos removidos e hashes de entrada/resultado. Não guarda corpo da saída, prompt ou transcript. O índice permite consultar apenas o envio pertinente, em páginas de 32 registros. Captura e consulta não usam Jev nem modelo. Falha de gravação não bloqueia a ferramenta; o diagnóstico conta a falha.

```sh
mustard-rt run panel --root <projeto> --spec <spec> --wave <numero> --observations
mustard-rt run panel --root <projeto> --spec <spec> --wave <numero> --observations --observations-before <next_before>
```

Na entrega, `observations:[{receipt,mode,text}]` aceita `mode: "current"` somente com recibo do envio vigente e data válida posterior ao despacho. Fonte de outro envio só entra como `historical`, conservando a data original. O chamador não pode fornecer outra data nem declarar a interpretação como fato comprovado. Repetir o mesmo evento não renova o recibo. O registro sobrevive à integração da entrega e aparece no Mods com origem, momento e distinção entre falha atual e referência histórica.

**Um recibo comprova a execução observada, não a verdade de uma interpretação livre.** A entrega permanece `reported-unverified`; a revisão deve confrontar a conclusão com a evidência. Os testes executam comandos Git somente de leitura e reproduzem os payloads nativos dos hosts; não acessam buckets nem dados externos.

Referências de protocolo: [hooks Claude](https://code.claude.com/docs/en/hooks), [hooks Codex](https://learn.chatgpt.com/docs/hooks).

## Aceites que exigem o ambiente real

O usuário confirmou que testará Desktop/SSH e Haiku/Luna **depois de instalar a release**. Esses aceites posteriores não impedem o PR após fechar a implementação e suas verificações locais, mas não podem ser apresentados como já aprovados. Os seguintes resultados não podem ser inferidos de testes locais nem de um PR verde:

- pintura e uso do Mods no Desktop Windows conectado por SSH;
- execução efetiva de Haiku/Luna na sessão autenticada, descoberta do agente, versão/effort e entrada total observados;
- comparação equivalente de implementação genérica/com skills/especializada, contando pai, filhos, Jev e retrabalho;
- ativação/confiança e medição após a nova instalação do usuário nos projetos;
- acesso/limpeza de dados externos autorizados e publicação autenticada, quando solicitados.

Esses aceites estão separados da implementação. Não há percentual global comprovado de economia nem garantia de código perfeito. Não se reinstala o backend removido para fabricar a validação. A decisão de só abrir PR quando toda a implementação e suas verificações estiverem fechadas permanece em vigor.

## Verificação desta continuação

- **Rust: 4.101 aprovados, zero falhas, dois ignorados.** Core/runtime: 1.224 + 38 + 1.898 + 294, mais a regressão final da CLI paginada; CLI: 53 + 16; scan: 97 + 480. As duas medições legadas ignoradas dependem de conversas/cópias externas; não são validações de comportamento. A suíte completa inicial encontrou incompatibilidades de orçamento, manifesto e documentação, corrigidas antes da execução final de core/runtime. CLI/scan permaneceram aprovados e não receberam alteração posterior.
- **Plugin/Mods: 44 aprovados, zero falhas**, pelo test runner nativo do Claude Code 2.1.296. Manifesto e registro do plugin também validados. Não houve execução de modelo.
- **Fluxo CI: 22 testes aprovados.** Clippy do workspace, com todos os alvos e `-D warnings`, passou.
- **Build e instalação do candidato: aprovados.** Os binários atuais foram exercitados num projeto descartável com RTK/rg reais do pacote: instalação nova, migração, vetores locais, atualização sem releitura, busca nativa/tipada sem PATH, painel e histórico RTK. Sem chamada paga de IA, publicação ou instalação nos projetos do usuário.
- **Regressões da lacuna final:** eventos repetidos conservam data, payload conflitante é recusado, fonte de outro envio não vira atual, metadados de data não podem ser fornecidos pelo agente, consulta é paginada, falha Claude é registrada sem bloquear o host, Codex não deduz sucesso da saída textual, segredos/corpo de saída ficam fora do recibo, comando com referências a cópias diferentes não é atribuído por palpite, e os recibos sobrevivem à integração da entrega.
- **Revisão final:** os 101 testes do gravador/integração de retornos passaram novamente após a recusa de atribuição ambígua entre cópias. A CLI real retornou as 33 observações em duas páginas, sem exigir índice, transcript ou carregar o painel inteiro; nenhuma outra suíte foi repetida sem mudança que justificasse isso.
- **Instruções:** teto original de 25.600 bytes por idioma preservado; novas orientações compensadas por simplificação do texto, sem retirar a leitura integral dos itens, os testes mínimos ou a regra de capacidade.

Metadados, hashes do candidato e logs estão identificados em [evidência desta continuação](evidence/2026-10-12-fechamento-do-escopo.json). A validação econômica anterior continua separada em [validação final](2026-10-12-validacao-final.md); não se atribui ganho novo de acerto/custo a testes de protocolo. O PR reúne o fechamento desta continuação sobre a base que já contém as demais implementações do dia.
