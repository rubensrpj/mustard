# Painel Mods e métricas do Mustard

Escopo aprovado em 13/10/2026: melhorar o painel local, o acompanhamento da execução e a leitura do consumo, acrescentando ideias, perfis de visualização e prévias de impacto. O chat lateral fica excluído. Base: `main` v0.2.18, commit `425463ec`; implementação na branch `codex/mods-control-center`.

O binário mantém os dados do projeto e as regras da execução. O Mods apresenta esses dados e observa os eventos do host, sem solicitar respostas do modelo para atualizar a interface. As páginas externas continuam sendo snapshots publicados por solicitação explícita.

## Relação entre o escopo e a implementação

| Ponto aprovado | Comportamento entregue | Implementação e verificação |
| --- | --- | --- |
| 1 Estabilidade e velocidade | Estado de navegação associado à sessão; retomada do painel solicitado; consultas por seção e revisão; eventos agrupados e verificação externa a cada 10 segundos; troca lenta mostra carregamento. | `plugin/hooks/register.js`, `panel-runtime.js`, `commands/panel/dashboard.rs`; testes de retomada, consulta condicional e transição. |
| 2 Visual e leitura | Paleta escura ERP.Brain com destaque amarelo, cartões, Markdown e código selecionável; barra de progresso Desktop com alternativa textual; adaptações para terminal e mobile. | `plugin/ui/theme.js`, `panel-view.js`, `panel-content.js`; testes nativos por superfície e paginação integral. |
| 3 Projeto e specs | Projeto/branch/spec, definições pesquisáveis, fila e bloqueios; plano, input enviado, agente/skills, retorno e provas da onda; conteúdo extenso carregado sob demanda. | `commands/panel/dashboard.rs`, `mod.rs`, `panel-view.js`; igualdade das definições e do input com a versão anterior. |
| 4 Ondas e agentes | Observação de agent.spawn, turn.step e turn.complete; modelo/esforço observados, duração, contexto e tokens. Associação à onda exige vínculo nativo com o envio atual. | `register.js`, `commands/panel/live_usage.rs`; teste do stream intacto e deduplicação de solicitações. |
| 5 Consumo e economia | Cotas/reset quando informados pelo host; categorias de tokens e cache; custo equivalente separado de cobrança; Jev/RTK/busca/análises e cobertura. Medições sem dólares também são preservadas. | `consumption.rs`, `live_usage.rs`, `jev_usage.rs`, `panel-extras.js`; recibos repetidos não somam duas vezes. |
| 6 Configurações e diagnóstico | Configuração validada, revisão contra concorrência, disponibilidade e versão das integrações; distinção entre habilitar e observar uso; mudanças dos agentes continuam valendo para novas execuções. | `commands/panel/settings.rs`, `panel-settings.js`; testes de preservação, conflito e atualização de hooks. |
| 7 Ideias | Mesmo ledger e IDs das pendências; criação, edição, arquivo, tratamento e vínculo explícito a spec. Ideias ficam fora das cobranças de trabalho aprovado. Iniciar levantamento preenche um pedido revisável. | `commands/event/pending/ideas.rs`, `pending.rs`, `panel-extras.js`; teste nativo do ledger e teste de interação sem turno do modelo. |
| 8 Perfis | Visão geral, planejamento, execução, revisão e consumo; seleção, personalização, cópia e remoção por projeto. São escolhas de apresentação. A última seção não pode ser removida. | `commands/panel/preferences.rs`, `panel-extras.js`; testes de revisão, preservação da execução e seleção válida. |
| 9 Prévias de impacto | Publicação com escopo/destino e confirmação; conferência de tentativa e branches mescladas; observação de operações Bash reconhecidas. A prévia preserva as autorizações e os bloqueios existentes. | `commands/panel/impact.rs`, `register.js`; testes de não execução, cancelamento, resposta negada preservada e publicação explícita. |
| 10 Statusline | Uma linha com projeto/spec ou branch, progresso, modelo, contexto e comando de acesso ao painel. Histórico de custos e detalhes permanecem no Mods. | `commands/statusline/mod.rs`; testes de linha única, link histórico e ausência de consumo histórico na barra. |
| 11 Erros e funcionamento real | Eventos de falha de ferramentas, medição e painel, além dos registros existentes do runtime/Jev; filtros e exportação local sanitizada. Repetições consecutivas são agrupadas. | `commands/panel/diagnostics.rs`, `panel-extras.js`; teste de exportação sem campo privado e sem publicação. |
| 12 Publicação explícita | Projeto, spec ou relatório Markdown no layout comum. Abrir/atualizar o painel não prepara página nem banco externo. A prévia é recalculada antes de confirmar. | `register.js`, `panel-view.js`, publicação nativa existente; teste de confirmação/cancelamento e smoke de instalação sem publicação automática. |

## Medição do painel

Comparação sintética neste host, com dois binários de release: versão publicada v0.2.18 e candidato compilado com `cargo build --release --workspace`. A amostra contém 25 specs, 80 regras por spec e nove repetições de cada consulta. Os números abaixo são medianas de tempo e tamanho da resposta JSON.

| Consulta | Tempo | Resposta | Diferença em relação à consulta completa anterior |
| --- | ---: | ---: | --- |
| Completa na v0.2.18 | 56.258 ms | 94,297 bytes | Base |
| Projeto no candidato | 11.601 ms | 5,604 bytes | 79.38% menos tempo; 94.06% menos bytes |
| Fontes inalteradas no candidato | 3.366 ms | 146 bytes | 94.02% menos tempo; 99.85% menos bytes |

O intervalo de verificação externa passou de dois para dez segundos: seis disparos por minuto em repouso, contra trinta. Os eventos do trabalho continuam solicitando atualização agrupada após 200 ms. O painel suspende consultas ao fechar ou perder suas interfaces. Recuperação de transcrições fica nas visões de ondas, histórico e detalhes do consumo.

As definições selecionadas e o pedido integral da onda permaneceram idênticos. O ensaio também conferiu o registro sem custo em dólares e a contagem única de uma solicitação repetida. Essas reduções medem o processamento local e o transporte entre o binário e o Mods. A economia líquida de tokens do LLM exige comparar trabalho equivalente em sessões reais; o benchmark não mede essa economia.

Reprodução e resultados completos: [script](../scripts/bench-panel.py), [benchmark](evidence/2026-10-13-mods-benchmark.json) e [validação com hashes](evidence/2026-10-13-mods-validation.json).

```bash
python3 scripts/bench-panel.py --baseline /caminho/v0.2.18/mustard-rt --candidate /caminho/candidato/mustard-rt --out /tmp/panel-benchmark.json
```

## Validação e limites

Foram aprovados 4.109 testes Rust únicos, 56 testes nativos do plugin e 22 testes de roteamento CI. Duas medições antigas permanecem ignoradas. Os casos alterados de painel/pendências e as 297 verificações de integração foram conferidos novamente sobre a fonte final. Clippy do workspace com todos os targets e warnings como erro, validação do plugin, compilação de release e instalação/atualização em projeto temporário passaram.

A instalação temporária executou os binários reais, o modelo vetorial local, RTK, busca tipada/nativa e aprendizado. As configurações do usuário e os projetos removidos pelo usuário permanecem preservados. Os testes usaram IA remota desligada e não publicaram páginas externas.

A pintura e a interação reais no Desktop Windows conectado ao Linux por SSH dependem da instalação no host autenticado. O usuário decidiu testar esse ambiente após a release. As árvores de interface foram validadas pelo host de testes do Claude Code em desktop, terminal e mobile; isso não comprova a pintura real no Desktop. A mesma distinção vale para cotas da conta e economia em sessões reais.

Prévia de shell é uma análise estática parcial. Ela não avalia expansões, não executa o comando e não autoriza uma ação. Mudanças encadeadas de diretório e efeitos não determinados aparecem como desconhecidos. O código de proteção original e as verificações de permissão do host continuam decidindo a execução.

## Referências aplicadas

Foram aplicados conceitos, sem instalar ou incorporar código dos projetos: [Savvy Progress](https://github.com/JohnnyVizz/claude-kit/tree/main/plugins/savvy-progress), [Claude Skins](https://github.com/hellosverre/claude-skins), [Blast Radius](https://github.com/anthropics/claude-code-playground/tree/main/claude-code/mods/blast-radius), [Mods Manager](https://github.com/ptpmediabr/mods-manager), [Usage Weather](https://github.com/ptpmediabr/usage-weather) e [Ideas Shelf](https://github.com/ptpmediabr/ideas-shelf).

Os eventos e elementos seguem as [referências oficiais de Mods](https://code.claude.com/docs/en/plugins/mods/reference), a [observação de eventos](https://code.claude.com/docs/en/plugins/mods/events) e a [medição de plugins](https://code.claude.com/docs/en/plugins/measure). O desenho segue as [diretrizes ERP.Brain](https://github.com/atiz/erpbrain/blob/main/design/guidelines/README.md), com destaque amarelo. O chat lateral permanece excluído.
