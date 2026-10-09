# Mustard neste projeto

Binário busca/calcula; LLM implementa.

## Levantamento

- Abra mudanças por `mustard-rt run open`, grave o objetivo em `context`, sugira `/clear`; a linha de retomada mostra o estado. Consultas não abrem spec; branch alheia não bloqueia.
- Explique na ordem de explicar do estilo de resposta. Confira código/histórico antes de remover. Grave respostas por `mustard-rt run answer`.
- Busque/leia por `mcp__mustard__search`: `{request:{tool,input,intent,purpose,choose?}}`. Preserve argumentos/escopo. `intent`: pergunta desta busca. `purpose`: `locate`, `understand`, `spec`, `implement`, `validate`. Só `locate` aceita intenção vazia.
- CLI: `mustard-rt run search --shell-output --intent "<pergunta>" --purpose spec -- rg -n "<padrão>" .`. Bash: `mustard:spec: <pergunta>`; sem anotação: `locate`. `--raw`: nativo. `choose:true`/`--choose`: Jev só em responsabilidade ambígua.
- Reaproveite corpos completos; ao achar arquivo, investigue a declaração. Sem faixa: `run map summary --file <arquivo>`, depois `Read` com `offset`/`limit`. Expanda faltantes; fonte mudou: releia.
- `mustard-rt run knowledge`: dossiê.

## Spec aberta

- Leia/grave a spec por `mustard-rt run read`/`run write`; nunca edite `spec.*` à mão.

- Em spec fechada ou com pull request aberto, `mustard-rt run reopen --reason "<motivo>"` vem antes de `write request`. PR reprovado pelo servidor: `mustard-rt run reopen --fix --reason "<motivo>"`.
- Erro, ajuste ou melhoria do mesmo assunto entra na mesma spec por `write request`, nunca como pendência. Registre e informe o que for certo; na dúvida, proponha e pergunte uma vez. Outro assunto entra por `mustard-rt run pending --add`; se o usuário quiser fazer já, sugira outra conversa.
- Mudança fora do autorizado exige o sim do usuário. Correção do funcionamento do Mustard vira ajuste no produto, não só memória.
- Delegue a investigação que abre muitos arquivos. Peça a todo agente achados na spec por `mustard-rt run write` e retorno em duas linhas.

## Acompanhamento

- `/mustard-panel`: projeto/specs/execução/consumo, sem IA; CLI: `run panel`.
- `/mustard-pages` só sob pedido; exportar não confirma publicação. Atualize só por pedido. `run publish --spec <spec>`; `--include-consumption`: autoriza consumo. `mustard-rt run page`: markdown.

## Retomar

`mustard-rt run resume`. Commit/PR não levam cliente, e-mail ou caminho local.
