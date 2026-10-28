# Mustard neste projeto

- Consulta não abre spec. Mudança: `mustard-rt run open`; depois grave o objetivo por `mustard-rt run write context`, `origin` na mensagem do usuário; se sugerido, confirme por `goal_approval`. Só então sugira `/clear`; a linha de retomada guarda o estado.
- Código: `mcp__mustard__search`, com intenção local e finalidade; preserve argumentos e escopo. CLI: `mustard-rt run search --shell-output` com a busca original. Bash: descrição `mustard:spec: <pergunta>`.
- Reuse trechos atuais; o gancho cruza o Read com offset/limit. Sem faixa: `run map summary --file <arquivo>`. Fonte alterada: releia. Comentário não prova comportamento.
- Dossiê: `mustard-rt run knowledge`; sem ele, `run knowledge --prepare-session claude --query "<dúvida>"`. Delegue só se compensar; agente de onda: `write delivered`, erro/sobra em `leftovers`, volta em duas linhas; o gancho lhe recusa `write request`/`task`: o condutor grava.
- Spec: `mustard-rt run read`/`run write`; procure com `mustard-rt run find "<texto>"`. Fatos: `mustard-rt run answer --facts '<JSON>'`. JSON longo: Write num temporário e `--json @arquivo` (ou `--facts`), sem script. Medida: `mustard-rt run evidence --spec <spec>`.
- Mesmo assunto: `write request`, não pendência. Registre o autorizado; na dúvida, pergunte uma vez. Com spec fechada ou pull request aberto, `mustard-rt run reopen --reason "<motivo>"` vem antes de `write request`. PR reprovado pelo servidor: `mustard-rt run reopen --fix --reason "<motivo>"`. Outro assunto: `mustard-rt run pending --add`; se quiser fazer já, sugira outra conversa. Backlog: `mustard-rt run triage`.
- Antes do commit: lance o `mustard-review` pedido; leia só o veredito. Sem ele, `mustard-rt run review-wave --wave <n> --spec <spec>` resume; `--diff` dá o diff. Aceite com `<REVIEWED>{"wave":<n>,"note":"…"}</REVIEWED>` ou REJECTED com motivo. Tarefa bloqueada não vira onda igual.
- `/mustard:mods`: projeto e consumo local. `/mustard:pages`, `run publish` e `mustard-rt run page` (markdown) só sob pedido; exportar não é publicar; `--include-consumption` exige autorização.

Commit/PR sem cliente, e-mail ou caminho local.

{principles}

## Retomar

`mustard-rt run resume`.
