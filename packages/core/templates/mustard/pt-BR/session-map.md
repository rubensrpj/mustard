# Mustard neste projeto

Binário busca e calcula; LLM decide e implementa.

- Retome por `mustard-rt run resume`. Consulta não abre spec. Mudança: abra com `mustard-rt run open`; depois grave o objetivo por `mustard-rt run write context`, `origin` na mensagem do usuário; se sugerido, confirme por `goal_approval` de `run resume`/`run grill`. Só então sugira `/clear`; a linha de retomada guarda o estado.
- Código: `mcp__mustard__search`, com intenção local e finalidade da pesquisa; preserve argumentos e escopo. CLI: `mustard-rt run search --shell-output` com a busca original. Bash: descrição `mustard:spec: <pergunta>`.
- Reuse trechos atuais; Read com offset/limit é cruzado pelo hook sem outra IA. Sem faixa: `run map summary --file <arquivo>`. Fonte alterada: releia. Referência ou comentário não prova comportamento.
- Dossiê: `mustard-rt run knowledge`, reuse o atual; senão, `run knowledge --prepare-session claude --query "<dúvida>"`. Delegue só se compensar; agente de onda: `write delivered`, erro/sobra em `leftovers`, volta em duas linhas; o gancho lhe recusa `write request`/`task`: o condutor grava.
- Spec: `mustard-rt run read`/`run write`; nunca edite `spec.*`. Resposta com fatos: `mustard-rt run answer --facts '<JSON>'`. Medida: `mustard-rt run evidence --spec <spec>`; anexe com `--point <código> --receipt <recibo> --text "<medida, entrada e limites>"`. Não generalize uma amostra.
- Mesmo assunto: `write request`, não pendência. Registre o autorizado; na dúvida, pergunte uma vez. Em spec fechada ou com pull request aberto, `mustard-rt run reopen --reason "<motivo>"` vem antes de `write request`. PR reprovado pelo servidor: `mustard-rt run reopen --fix --reason "<motivo>"`. Outro assunto: `mustard-rt run pending --add`; se quiser fazer já, sugira outra conversa. Recusa `same-work-open`: grave a versão nova da tarefa nomeada, ou `distinct` com o motivo; lista aberta: `mustard-rt run triage`.
- Mudança não autorizada exige aceite; corrija o Mustard no produto.
- Antes do commit: `mustard-rt run review-wave --wave <n> --spec <spec>`. Leia diff/regras/skills/testes. Aceite pelo recibo REVIEWED e fingerprint, ou REJECTED com motivo. USAGE não aprova. Replanejamento espera aceite; tarefa bloqueada não vira nova onda igual.
- `/mustard-panel`: projeto, specs, ondas e consumo local; CLI `run panel`. `/mustard-pages`, `run publish` e `mustard-rt run page` (markdown): só sob pedido explícito. Exportar não é publicar; `--include-consumption` exige autorização.

Responda na ordem de explicar do estilo de resposta. Commit/PR não levam cliente, e-mail ou caminho local.

## Retomar

`mustard-rt run resume`.
