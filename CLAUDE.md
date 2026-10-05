# (root)

## Guards

- `pull.ff = only` é escolha por-máquina em `.git/config`; NÃO nativize no instalador. O `update_bases` (`apps/rt/src/commands/git_settle.rs`) avança a base depois do merge com `merge --ff-only`, que só passa quando a base de integração não tem commit próprio — a invariante que o despacho exige —, e recusa dizendo que as histórias se separaram quando ela tem. `pull.rebase true` absorveria em silêncio um commit nascido direto no `dev` e esconderia justamente o defeito que essa recusa existe para pegar. Reaplicação deliberada continua possível: `git pull --rebase` na linha de comando vence a config.
- O instalador NUNCA escreve em `.git/config` — só lê. Toda escrita de config no código vive sob `#[cfg(test)]`; mantenha assim.

## Revisão

- Neste repositório, a prova de ponta a ponta do revisor é instalar o Mustard numa pasta temporária vazia (`mustard init`) e rodar o que o usuário rodaria. O molde do revisor vai a todo projeto e por isso não traz esse comando.
- O `mustard init` dessa prova é o da cópia em revisão, não o do PATH. De dentro da pasta temporária, rode o programa compilado da cópia: `<cópia>/target/debug/mustard init --yes`. Ele não procura pasta de moldes: tudo o que grava vem compilado nele.
