# Agentes, skills e análise reutilizável

Esta etapa amplia o scan e o preparo das ondas sem iniciar outro processo de modelo. A versão 0.2.14 já contém a revisão por onda, os bloqueios persistentes e a retirada do teto mensal do Jev. A v0.2.16 passou pelos testes dos três sistemas; a continuação e sua evidência estão em [validação final](2026-10-12-validacao-final.md).

## Fluxo implementado

1. O scan atualiza o índice nativo e sincroniza perfis de navegação por papéis encontrados no grafo e nas linguagens. Arquivo isolado não basta para criar especialização. Essa sincronização não chama IA.
2. Antes da onda, o binário entrega regras obrigatórias e skills pertinentes existentes. Na ausência de uma skill selecionada, pode usar os perfis válidos do catálogo. Fontes e regras mudadas retiram procedimentos autorais da seleção.
3. Cada perfil pode ser exportado como skill e agente nos formatos próprios de Claude e Codex. O agente mantém as restrições do executor genérico; especialização não concede permissão para editar outra área, alterar decisões ou ignorar provas.
4. Os metadados dos perfis pertinentes entram no estado compartilhado do julgamento tipado das ondas pelo Jev. As travas nativas de dependência, bloqueio, revisão e aprovação continuam obrigatórias. Não existe uma chamada adicional ao Jev para cada perfil ou leitura.
5. Quando faltar interpretação, o binário prepara trechos atuais para um subagente da sessão existente. Antes de delegar, consulta a análise já armazenada. O resultado é hipótese com referências, não uma nova regra ou fato verificado.

Perfis gerados nativamente descrevem **como navegar**. Eles não descobrem sozinhos toda convenção de banco, React ou design. Procedimentos específicos precisam ser redigidos a partir das evidências, inspecionados e revisados pelo fingerprint exato. Não executar um criador a cada onda. Arquivos manuais ou modificados pelo usuário são preservados; exports obsoletos ainda idênticos ao conteúdo gerenciado podem ser removidos.

## Comandos

```sh
mustard-rt run expertise --sync
mustard-rt run expertise --file src/area/service.ts
mustard-rt run expertise --record profile.json
mustard-rt run expertise --review nome:fingerprint
mustard-rt run expertise --export nome --host claude
mustard-rt run expertise --export nome --host codex
mustard-rt run setup-codex --root .
```

Um manifesto autoral tem `name`, `description`, `paths` (globs relativos), `procedure`, `sources` e `origin: "authored"`. Cada fonte contém `file`, `line`, `end_line` e o `sha256` atual fornecido pela busca. `--record` grava rascunho e devolve o fingerprint para inspeção; um campo `reviewed` enviado pelo modelo não aprova o próprio texto. Atualizar fontes ou regras exige nova revisão. A mesma definição intacta conserva sua revisão.

```sh
mustard-rt run knowledge --query "Como esta operação preserva a transação?" --prepare-session claude
mustard-rt run knowledge --query "Como esta operação preserva a transação?" --prepare-session codex
mustard-rt run knowledge --complete-session receipt.json
```

A instalação acrescenta `mustard-analysis`: Haiku/medium no Claude e Luna/medium em `.codex/agents/mustard-analysis.toml`. O preparo indica esse agente e a ferramenta nativa do host. Prefira o agente apenas se descoberto pela sessão; ausência não autoriza outro CLI/API.

O preparo devolve o contrato de delegação e de recibo. `cached: true` dispensa o subagente. Caso contrário, a sessão usa sua própria ferramenta de agente, com contexto isolado e somente o pacote pertinente; o binário não invoca `claude -p`, outro CLI ou API de geração. O recibo inclui `request_id`, modelo efetivo quando conhecido, resultado JSON com citações e tokens expostos pelo host. Uma conclusão já obtida pela onda pode entrar em `knowledge` na entrega; não delegar sua leitura novamente.

## Validade e consumo

- A identidade inclui pergunta normalizada, instruções/esquema, **todas** as fontes fornecidas e regras aplicáveis. Trocar host ou preferência de modelo não invalida uma análise atual. Alteração não pertinente também não.
- Perguntas semanticamente equivalentes escritas de outra maneira não são unificadas apenas por hash. A fonte fornecida pode não cobrir a pergunta: a análise precisa declarar a lacuna.
- Haiku e Luna usam a **versão mais recente disponível da família na sessão e effort médio**, com orçamento fixo de **100.000 tokens totais de entrada por análise**, incluindo instruções, ferramentas e cache. Preferências ficam em `ai.sessionModels.claude` e `.codex`, também nas configurações do Mods. O alias `haiku` acompanha o host; no Codex, resolver a família Luna nas capacidades atuais da sessão. Não deduzir a versão mais nova ordenando nomes. Sem a família ou effort disponíveis, conservar evidência nativa. A implementação da tarefa mantém seu modelo próprio. O recibo guarda effort efetivo quando exposto, separado da preferência; desconhecido não confirma cumprimento.
- Sem isolamento disponível, conservar a evidência nativa. Sem telemetria, uso permanece desconhecido. O binário não controla a janela interna do host; uma resposta válida acima do orçamento é conservada e marcada, para evitar repetir o gasto.
- `Consumo → Análises` mostra conclusões, reutilizações, falhas, tokens informados e perfis válidos/inativos. O log de operações guarda metadados permitidos, sem código, prompt ou chave, inclusive quando ainda não há spec. Os tokens das análises já pertencem à sessão: não somar novamente aos totais nem ao custo do Jev.
- Notas antigas do scan sem SHA-256 verificável deixam de contar como atuais. Após leitura e scan atuais, podem ser regravadas; não se atribui validade retroativa.

## Integração e limites verificáveis

O instalador Codex configura o MCP de busca, hooks locais e agentes/skill nativos, preservando configurações manuais. O MCP devolve conteúdo textual e compartilha o contrato da busca; não declara um output schema incompatível com a própria resposta. O adaptador verifica todos os arquivos de um patch antes de autorizar a operação e preserva argumentos da ferramenta original.

Hooks novos ou modificados exigem a revisão de confiança **do próprio Codex**, em `/hooks`. Instalação concluída não prova que o host os ativou. Não se reutiliza telemetria nem testemunha de aprovação de uma conversa Claude como se fosse Codex. O adaptador cobre busca, proteção de comandos/arquivos e preparo de despacho; a telemetria de uso e a captura de aprovações da sessão Codex precisam de validação própria do host. O Mods continua sendo interface do Claude; no Codex, existe o comando nativo de painel. O despacho/exportação usa o host do chamador; reenvio em outro host conserva o texto aprovado e acrescenta apenas opções/referências atuais. O executor Codex herda modelo/effort da sessão, sem solicitar nomes da família Claude.

Agentes novos podem depender da descoberta pelo host. Em uma sessão com ondas em andamento, usar o executor genérico com a skill escolhida até o novo perfil ficar disponível; não reiniciar a sessão automaticamente. Testes locais dos contratos não comprovam seleção real de Haiku/Luna, renderização no Desktop por SSH nem economia líquida. Esta última exige comparar trabalho equivalente, incluindo preparo, Jev, implementação, revisão e retrabalho.

## Referências aplicadas

- [Claude: subagentes](https://code.claude.com/docs/en/sub-agents) e [skills](https://code.claude.com/docs/en/skills): escopo, contexto isolado e carregamento sob demanda.
- [Codex: skills](https://learn.chatgpt.com/docs/build-skills), [subagentes](https://learn.chatgpt.com/docs/agent-configuration/subagents), [hooks](https://learn.chatgpt.com/docs/hooks) e [MCP](https://learn.chatgpt.com/docs/extend/mcp?surface=cli): formatos próprios, confiança e contratos dos eventos.
- [Jev](https://docs.typesafe.ai/introduction): perguntas atômicas e escolhas tipadas sobre estado pertinente.
- [Skill Creator](https://github.com/anthropics/skills/blob/main/skills/skill-creator/SKILL.md): avaliar gatilhos e resultado antes de tornar uma orientação recorrente.

Nenhum desses criadores foi incorporado como dependência executável; foram aplicados seus conceitos. O modelo local vetorial existente continua na recuperação, separado da interpretação opcional Haiku/Luna e da classificação Jev.
