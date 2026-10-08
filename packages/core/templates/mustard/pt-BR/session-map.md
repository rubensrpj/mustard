# Mustard neste projeto

Fluxo: levantamento, plano, aprovação, ondas, revisão final, fechamento e pull request. Siga o comando. Use o binário para estado, buscas, contexto, cálculos e páginas; o modelo raciocina e implementa. Operação mecânica ausente vira proposta de comando.

## Pedido e levantamento

- Abra mudanças com `mustard-rt run open`, grave o objetivo em `context` e sugira `/clear`; a linha de retomada mostra o estado. Pergunta, leitura e status não abrem spec. Branch alheia ao Mustard não bloqueia.
- Explique um ponto por vez na ordem de explicar do estilo de resposta. Confira código e histórico: um resto pode ter consumidor.
- Grave cada resposta com `mustard-rt run answer`.

## Spec aberta

- Em spec fechada ou com pull request aberto, `mustard-rt run reopen --reason "<motivo>"` vem antes de `write request`. PR reprovado pelo servidor: `mustard-rt run reopen --fix --reason "<motivo>"`.
- Erro, ajuste ou melhoria do mesmo assunto entra na mesma spec por `write request`, nunca como pendência. Registre e informe o que for certo; na dúvida, proponha e pergunte uma vez. Outro assunto entra por `mustard-rt run pending --add`; se o usuário quiser fazer já, sugira outra conversa.
- Mudança fora do autorizado exige o sim do usuário. Correção do funcionamento do Mustard vira ajuste no produto, não só memória.
- Leia a spec por `mustard-rt run read <bloco>` e grave por `write`; nunca edite `spec.*` à mão.
- Delegue a investigação que abre muitos arquivos; confira você um ponto único. Peça a todo agente que grave achados na spec por `mustard-rt run write` e volte em duas linhas.

## Acompanhamento

- `/mustard-panel`: projeto, specs, execução e consumo local; consulta e renderização não chamam modelo nem Jev.
- `/mustard-pages` só sob pedido. Gera snapshot datado; exportação local não confirma publicação remota. Atualizar exige nova ação.
- Sem Mods: `mustard-rt run panel --root <projeto> --spec <spec>`. Exportar: `mustard-rt run publish --spec <spec>`; `--include-consumption` autoriza compartilhar consumo.
- Página avulsa: `mustard-rt run page` recebe markdown.
- `mustard-rt run spend` mede localmente; `mustard-rt run spend --publish` prepara exportação só sob pedido.

## Retomar

O binário monta commit e corpo do PR, sem Claude, claude.ai, e-mail ou caminho da máquina. "Onde parei" e "vamos continuar" usam `mustard-rt run resume`.
