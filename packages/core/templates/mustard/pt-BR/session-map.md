# O Mustard neste projeto

O Mustard conduz todo trabalho que muda arquivo por um fluxo só: levantamento, plano, aprovação, ondas, revisão, fechamento e pull request. Cada comando responde qual é o próximo passo. Siga essa resposta, em vez de decidir a ordem sozinho.

## Quando o pedido chega

- Pedido que muda arquivo abre uma spec: rode `mustard-rt run open`. A resposta traz a pergunta da base, do tipo e do nome da branch.
- Pergunta, leitura ou status não abre spec. Responda direto.
- Numa branch que o Mustard não abriu, ele não trava nada.

## No levantamento

- Apresente um ponto por vez, na ordem de explicar do estilo de resposta.
- Confira no código antes de afirmar.
- Grave cada resposta na hora, com `mustard-rt run write <tipo>`.

## Durante a spec

- Pedido novo do usuário entra na mesma spec, com `write request`; em spec fechada ou com o pull request aberto, `mustard-rt run reopen --reason "<motivo>"` vem antes. Pull request reprovado pelo servidor vai ao `mustard-rt run reopen --fix --reason "<motivo>"`. Assunto diferente vira pendência, com `mustard-rt run pending --add`.
- Mudança que parte de você ou de um agente só segue com o "sim" do usuário.
- Nunca edite os arquivos `spec.*` à mão. Grave pelo `write` e leia um bloco com `mustard-rt run read <bloco>`.
- Delegue a um agente a investigação que abre muitos arquivos. Uma conferência pontual, faça você.

## Páginas

- Nunca escreva HTML. Página avulsa sai do `mustard-rt run page`, a partir de markdown.
- Publique no claude.ai só quando um comando mandar, e grave o endereço como a resposta dele disser. O link fica na barra de status; não o repita na conversa.

## Commit e pull request

O binário monta a mensagem de commit e o corpo do pull request. Nunca escreva neles "Claude", link do claude.ai, e-mail ou caminho da máquina.

## Retomar

"Onde eu parei" e "vamos continuar" pedem `mustard-rt run resume`. Depois de `/clear`, a linha abaixo já diz onde a spec está.

O jeito de responder mora no estilo de resposta.
