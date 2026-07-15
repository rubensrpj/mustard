---
name: mustard
description: Use this WHENEVER the user describes codebase work in plain language — add/create/implement something new; change/improve/adjust existing behavior; fix an error, bug, or broken behavior; or analyze/audit/investigate code — and when the user runs /mustard or asks how to use it. The single door: classifies the request (feature / change / bugfix / investigation + scope), narrates how it read it, asks only on genuine ambiguity, and dispatches the right internal flow.
source: manual
---
<!-- mustard:generated -->
# /mustard — A porta única

Você **não escolhe um comando**. Diga o que você quer em palavras suas — o mustard descobre se é uma **nova funcionalidade**, uma **mudança**, uma **correção** ou uma **investigação**, te mostra como ele leu o pedido, e só pergunta se estiver na dúvida.

## Ao acionar — ROTEIE, não só explique

**Se o usuário descreveu um trabalho** (adicionar/criar/implementar, mudar/melhorar/ajustar, corrigir erro/bug/quebrado, ou analisar/auditar/investigar): **não pare na ajuda abaixo — ROTEIE agora.** Siga `CLAUDE.md § Intent Routing`: classifique a intenção (+ escopo), **narre a leitura** (ex.: *"Tratando como uma correção de bug."*), **pergunte só na ambiguidade genuína**, e **despache o fluxo interno** — invoque `mustard:feature` / `mustard:bugfix` / `mustard:task` conforme a classificação (nunca implemente produção direto sem rotear). **Só mostre a ajuda abaixo** quando o usuário pediu ajuda OU digitou `/mustard` sem descrever um trabalho.

## Como usar

Escreva naturalmente. Exemplos:

- *"adiciona importação de CSV de clientes"* → tratado como funcionalidade.
- *"tá com erro ao importar o arquivo"* → tratado como correção de bug.
- *"melhora a mensagem de validação do CPF"* → tratado como mudança pequena (caminho leve).
- *"como funciona o cálculo de juros?"* → tratado como investigação.

O orquestrador (`CLAUDE.md § Intent Routing`) **sempre narra a leitura** antes de agir — você vê a classificação e pode interromper — e **só pergunta quando há uma dúvida real** (por exemplo: é um bug ou uma funcionalidade nova?). No caso óbvio, ele segue sem te interromper.

## E os comandos `/mustard:feature`, `/bugfix`, `/task`, `/tactical-fix`?

Continuam existindo como **atalho de poder** (override): se você já sabe exatamente o fluxo que quer, pode invocá-los direto. Mas você **não precisa** — a porta única roteia para o fluxo certo a partir da sua descrição. Os fluxos internos não são mais anunciados como escolha sua.

> **Nota para quem mantém o mustard (duas audiências, não misturar):** esta página é a doc **de usuário** — fala da porta única. A doc **interna** — o roteador (intenção → fluxo + escopo) e os procedimentos de cada fluxo — vive em `CLAUDE.md § Intent Routing` e nos `SKILL.md` dos fluxos (marcados *internal flow*). Manter as duas separadas é o que evita a IA se confundir sobre qual fluxo está rodando.
