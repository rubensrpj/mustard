A catraca que garante "toda regra de exclusão esconde algo que o Mustard realmente produz" passava na máquina do autor e falhava nos três runners do CI — para oito regras de uma vez. Ela não media o código: media a configuração de git da máquina onde rodava.

## Por quê

O teste `no_rule_reaches_a_depth_that_is_not_ours` pergunta ao git, uma regra por vez, num repositório-sonda: "esta regra casa com algum arquivo que uma instalação privada produz?". A prosa do teste afirma que a resposta é atribuível àquela regra sozinha.

Não era. O `git check-ignore` responde por **todas** as fontes de ignore ao mesmo tempo — inclusive a global do operador. Uma única linha em `~/.config/git/ignore` desta máquina (`**/.claude/settings.local.json`) fazia toda regra "casar com algo", inclusive regra que não casa com nada:

```
ANTES                                    DEPOIS
sonda ──► check-ignore ◄── regra         sonda ──► check-ignore ◄── regra
              ▲                                    (config global e de
              └── ~/.config/git/ignore              sistema apontadas
                  (invisível, da máquina)           para /dev/null)
```

Verde aqui, vermelho no CI — que não tem gitignore global nenhuma. E o vermelho era o **comportamento correto**: as oito regras (`.agent-state/`, `.cache/`, `.harness/`, `.metrics/`, `.pipeline-states/`, `agent-memory/`, `graph/`, `plans/`) de fato não casavam com nada que o fixture escrevia.

## O que mudou

Duas mudanças, ambas dentro do módulo de teste de `packages/core/src/platform/project_seed.rs`:

1. **A sonda fecha a porta da máquina.** `GIT_CONFIG_GLOBAL` e `GIT_CONFIG_SYSTEM` em `/dev/null`, `core.excludesFile` vazio. Sobra o exclude local com a única regra que o chamador escreveu — o que a documentação do teste sempre afirmou medir.

2. **O fixture aprende a produzir o que as regras escondem.** As oito regras cobrem diretórios de *tempo de execução* — coisas que o harness escreve enquanto roda, não sementes da instalação. O fixture só semeava; agora também escreve um arquivo real em cada diretório, com os nomes que este próprio repositório carrega (`main-context.counter.json`, `qa.jsonl`, `.last-stop`, …).

A ordem das mudanças foi deliberada: a primeira sozinha torna o teste vermelho **localmente**, reproduzindo o CI; a segunda o devolve ao verde pelo motivo certo.

## Como validar

O vermelho é reproduzível nesta árvore aplicando só a primeira metade: com a sonda isolada e o fixture antigo, o teste falha aqui com a mesma mensagem do CI, palavra por palavra.

```bash
cargo test -p mustard-core --lib platform::project_seed -- 
```

Esperado: 20 testes verdes, incluindo a catraca nos dois sentidos (esconde o nosso, não esconde o do cliente).

## Testes

Suítes medidas nesta árvore: **`core` 624**, **`rt` 1984**, **`cli` 49**, todas com 0 falhas (`rt`/`cli` com `--test-threads=1` pela instabilidade preexistente do `git_settle`, já registrada no #164).

## Fora de escopo

- A falha do CI no `dev` (commit `344fb368`) é esta mesma; este PR a resolve por herança.
- A instabilidade paralela do `git_settle` continua sendo unidade própria.
