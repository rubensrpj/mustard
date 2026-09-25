# (root)

## Guards

- `pull.ff = only` é escolha por-máquina em `.git/config`; NÃO nativize no instalador. O `base_gate` (`apps/rt/src/commands/event/base_gate.rs`) já prescreve `git pull --ff-only origin {base}` na recusa, e `--ff-only` só passa quando a base de integração não tem commit próprio — a invariante que o despacho exige. `pull.rebase true` absorveria em silêncio um commit nascido direto no `dev` e esconderia justamente o defeito que o portão existe para pegar. Reaplicação deliberada continua possível: `git pull --rebase` na linha de comando vence a config.
- O instalador NUNCA escreve em `.git/config` — só lê (`config --get remote.origin.url`, em `apps/cli/src/commands/init/seeding.rs`). Toda escrita de config no código vive sob `#[cfg(test)]`; mantenha assim.

## Revisão

- Neste repositório, a prova de ponta a ponta do revisor é instalar o Mustard numa pasta temporária vazia (`mustard init`) e rodar o que o usuário rodaria. O molde do revisor vai a todo projeto e por isso não traz esse comando.
