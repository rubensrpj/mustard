# O Mustard neste projeto

Todo trabalho que muda arquivo segue um fluxo só: levantamento, plano, aprovação, ondas, revisão, fechamento e pull request. Cada comando diz o próximo passo: siga-o, sem decidir a ordem sozinho.

## Quando o pedido chega

- Pedido que muda arquivo abre uma spec: rode `mustard-rt run open`. Gravado o objetivo (o primeiro `context`), sugira limpar a conversa com `/clear`: a linha de retomada diz onde a spec está.
- Pergunta, leitura ou status não abre spec. Responda direto.
- Em branch que o Mustard não abriu, nada trava.

## No levantamento

- Apresente um ponto por vez, na ordem de explicar do estilo de resposta.
- Confira no código antes de afirmar.
- Grave cada resposta na hora com `mustard-rt run write <tipo>`.
- Cada item combinado tem três partes: `title`, curto; `text`, o porquê pelo efeito que o usuário vê, sem arquivo nem comando; `agent`, arquivos, linhas, comandos e o que testar.

## Durante a spec

- Pedido novo entra na mesma spec pelo `write request`; em spec fechada ou com o pull request aberto, `mustard-rt run reopen --reason "<motivo>"` vem antes. Pull request reprovado pelo servidor vai ao `mustard-rt run reopen --fix --reason "<motivo>"`. Assunto diferente vira pendência, com `mustard-rt run pending --add`; se o usuário quiser fazer já, sugira outra conversa.
- Mudança sua ou de um agente só segue com o "sim" do usuário.
- Nunca edite os `spec.*` à mão: grave pelo `write` e leia um bloco com `mustard-rt run read <bloco>`.
- Delegue a um agente a investigação que abre muitos arquivos; a conferência pontual é sua. Peça a todo agente que grave na spec pelo `mustard-rt run write` e volte com duas linhas.

## Páginas

- Nunca escreva HTML. Página avulsa sai do `mustard-rt run page`, a partir de markdown.
- Publique no claude.ai só quando um comando mandar e grave o endereço como ele disser. O link fica na barra de status: não o repita na conversa.

## Commit e pull request

O binário monta a mensagem de commit e o corpo do pull request. Nunca escreva neles "Claude", link do claude.ai, e-mail ou caminho da máquina.

## Retomar

"Onde eu parei" e "vamos continuar" pedem `mustard-rt run resume`.

O jeito de responder mora no estilo de resposta.
