# O Mustard neste projeto

Todo trabalho que muda arquivo segue um fluxo só: levantamento, plano, aprovação, ondas, revisão, fechamento e pull request. Cada comando diz o próximo passo: siga-o, sem decidir a ordem sozinho.

## Quando o pedido chega

- Pedido que muda arquivo abre uma spec: rode `mustard-rt run open`. Gravado o objetivo (o primeiro `context`), sugira `/clear`: a linha de retomada diz onde a spec está.
- Pergunta, leitura ou status não abre spec. Responda direto.
- Em branch que o Mustard não abriu, nada trava.

## No levantamento

- Apresente um ponto por vez, na ordem de explicar do estilo de resposta.
- Confira no código e no histórico do git antes de afirmar ou propor: o que parece morto pode ter quem o use, e o que saiu pode ter saído de propósito.
- Grave cada resposta na hora com `mustard-rt run write <tipo>`.

## Durante a spec

- Em spec fechada ou com o pull request aberto, `mustard-rt run reopen --reason "<motivo>"` vem antes do `write request`. Pull request reprovado pelo servidor: `mustard-rt run reopen --fix --reason "<motivo>"`. Erro, ponto crítico, melhoria e ajuste do mesmo assunto entram na mesma spec, pelo `write request`: nunca viram pendência. Quem identifica é você: com certeza, grave e avise; na dúvida, sugira e pergunte uma vez. Só assunto diferente vira pendência, com `mustard-rt run pending --add`; se o usuário quiser fazer já, sugira outra conversa.
- Outra mudança sua ou de um agente só segue com o "sim" do usuário.
- Toda correção do jeito de o Mustard trabalhar vira ajuste do próprio Mustard, nunca só memória sua.
- Nunca edite os `spec.*` à mão: use `write` e `mustard-rt run read <bloco>`.
- Delegue a um agente a investigação que abre muitos arquivos; a conferência pontual é sua. Peça a todo agente que grave na spec pelo `mustard-rt run write` e volte com duas linhas.

## Páginas

- Nunca escreva HTML. Página avulsa sai do `mustard-rt run page`, a partir de markdown.
- Publique no claude.ai só quando um comando mandar e grave o endereço como ele disser. O link fica na barra de status: não o repita na conversa.

## Commit e pull request

O binário monta a mensagem de commit e o corpo do pull request. Nunca escreva neles "Claude", link do claude.ai, e-mail ou caminho da máquina.

## Retomar

"Onde eu parei" e "vamos continuar" pedem `mustard-rt run resume`.
