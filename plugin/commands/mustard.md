---
description: Use this WHENEVER the user describes codebase work in plain language — add/create/implement something new; change/improve/adjust existing behavior; fix an error, bug, or broken behavior; or analyze/audit/investigate code — and when the user runs /mustard or asks how to use it. The single door: classifies the request (feature / change / bugfix / investigation + scope), narrates how it read it, asks only on genuine ambiguity, and dispatches the right internal flow.
source: manual
---
<!-- mustard:generated -->
# /mustard — A porta única

Você **não escolhe um comando**. Diga o que você quer em palavras suas — o mustard descobre se é uma **nova funcionalidade**, uma **mudança**, uma **correção** ou uma **investigação**, te mostra como ele leu o pedido, e só pergunta se estiver na dúvida.

## Ao acionar — ROTEIE, não só explique

**Se o usuário descreveu um trabalho** (adicionar/criar/implementar, mudar/melhorar/ajustar, corrigir erro/bug/quebrado, ou analisar/auditar/investigar): **não pare na ajuda — ROTEIE agora.** Siga `CLAUDE.md § Intent Routing`: classifique a intenção (+ escopo), **narre a leitura** (ex.: *"Tratando como correção de bug."*), **pergunte só na ambiguidade genuína**, e **despache o fluxo interno** (`mustard:feature` / `mustard:bugfix` / `mustard:task`). Nunca edite produção sem rotear. **Só mostre a ajuda abaixo** quando o usuário pediu ajuda OU digitou `/mustard` sem descrever um trabalho.

## Como usar

Escreva naturalmente. Exemplos:

- *"adiciona importação de CSV de clientes"* → funcionalidade.
- *"tá com erro ao importar o arquivo"* → correção de bug.
- *"melhora a mensagem de validação do CPF"* → mudança pequena (caminho leve).
- *"como funciona o cálculo de juros?"* → investigação.

O orquestrador **sempre narra a leitura** antes de agir e **só pergunta quando há dúvida real**. No caso óbvio, segue sem interromper.

## E os comandos `/mustard:feature`, `/bugfix`, `/task`, `/tactical-fix`?

Continuam como **atalho de poder** (override): se você já sabe o fluxo que quer, invoque direto. Mas você **não precisa** — a porta única roteia a partir da sua descrição.

- **`/mustard:upsert`** — instala ou atualiza o mustard NESTE projeto (cria o `mustard.json` e os arquivos do harness, preservando o que já é seu); sem essa instalação, todos os outros comandos `/mustard:*` ficam bloqueados.

> **Nota (mantenedores):** esta página é a doc **de usuário** (a porta única). A doc **interna** — o roteador (intenção → fluxo + escopo) e os procedimentos de cada fluxo — vive em `CLAUDE.md § Intent Routing` e nos SKILLs dos fluxos (marcados *internal flow*). Manter as duas separadas evita a IA se confundir sobre qual fluxo está rodando.
