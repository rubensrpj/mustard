# Mustard neste projeto

Binário busca; LLM implementa.

## Levantamento

- Abra mudanças por `mustard-rt run open`, grave o objetivo em `context`, sugira `/clear`; a linha de retomada mostra o estado. Consulta não abre spec.
- Responda na ordem de explicar do estilo de resposta; confira código/histórico antes de remover. Use `mustard-rt run answer --facts '<JSON>'`.
- Busque por `mcp__mustard__search`: `{request:{tool,input,intent,purpose,choose?}}`; preserve argumentos/escopo. `intent`: recurso + operação/restrições; ou 2–8 `- pergunta`. Não repita spec. `purpose`: `locate`, `understand`, `spec`, `implement`, `validate`.
- CLI: `mustard-rt run search --shell-output --intent "<pergunta>" --purpose spec -- rg -n "<padrão>" .`. Bash: `mustard:spec: <pergunta>`. `--choose`: Jev opcional.
- Reuse trechos atuais; `Read` com `offset`/`limit` cruza evidências sem IA. Sem faixa: `run map summary --file <arquivo>`. Releia após mudar; referência não prova comportamento.
- `mustard-rt run knowledge`: dossiê.
- Análise ausente: `run knowledge --prepare-session claude --query "<dúvida>"`.

## Spec aberta

- Spec: `mustard-rt run read`/`run write`; nunca edite `spec.*`.

- Em spec fechada ou com pull request aberto, `mustard-rt run reopen --reason "<motivo>"` vem antes de `write request`. PR reprovado pelo servidor: `mustard-rt run reopen --fix --reason "<motivo>"`.
- Mesmo assunto: `write request`, não pendência. Registre o autorizado; na dúvida, pergunte uma vez. Outro assunto: `mustard-rt run pending --add`; se quiser fazer já, sugira outra conversa.
- Mudança não autorizada exige aceite; corrija o Mustard no produto.
- Delegue investigação extensa; todo agente grava achados na spec por `mustard-rt run write` e retorna em duas linhas.

## Acompanhamento

- `/mustard-panel`: local; CLI: `run panel`.
- `/mustard-pages` só sob pedido; exportar não é publicar. `run publish --spec <spec>`; `--include-consumption`: autoriza consumo. `mustard-rt run page`: markdown.

## Retomar

`mustard-rt run resume`. Commit/PR não levam cliente, e-mail ou caminho local.

## Revisar onda

Antes do commit: `mustard-rt run review-wave --wave <n> --spec <spec>`. Confira diff/regras/skills/testes. Aceite: `<REVIEWED>{"wave":1,"fingerprint":"<recibo>","note":"<conclusão>"}</REVIEWED>`; recuse com `REJECTED`. `USAGE` não aprova. Alteração invalida revisão. Revise tarefa bloqueada antes de despachar; replan espera o usuário.
