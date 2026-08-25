# Tactical Fix: o teto dos injetaveis e medido por arquivo mas o gancho funde varios num texto so

## Contexto

Tactical fix derivado de [[pergunta-abertura-unidade-pergunta-tipo]].

**O que acontece hoje.** A unidade-mãe dividiu o roteador em dois arquivos injetados,
`orchestrator.md` no evento `userPromptSubmit` e `dispatch.md` no `sessionStart`, e
criou uma catraca que mede cada **arquivo** contra um teto de 9.500 caracteres. Os dois
passam com folga: 5.853 e 7.995.

**Por que isso é um problema.** O limite real do harness não é por arquivo, é por
**resposta de gancho**: 10.000 caracteres para o texto que aquele gancho devolve. E o
gancho de início de sessão não devolve só o `dispatch.md` — ele funde quatro coisas num
texto único, nesta ordem: o censo de terreno, os injetáveis, o aviso de versão e o
aviso de poda. O censo cresce com o tamanho do repositório, cerca de 45 caracteres por
subprojeto.

Medido alimentando o gancho real com censos sintéticos:

| subprojetos | texto composto do `sessionStart` |
|---|---|
| 7 (este repositório) | ~8.400 |
| 40 | 9.679 |
| 50 | **10.079 — estoura** |
| 60 | 10.479 |

O `dispatch.md` ocupa a segunda posição da fusão, então é exatamente ele que vai para o
arquivo de excedente quando a soma passa — e o excedente deixa de estar em vigor,
virando prévia mais caminho. A catraca continuaria **verde** o tempo todo, porque cada
arquivo isolado cabe.

Essa é a mesma classe de defeito que a unidade-mãe existe para fechar: uma garantia que
certifica a coisa errada e reporta sucesso. A mãe trocou um critério tautológico por um
que pega o defeito; esta corrige o critério que ela própria criou.

**O que muda.** A medição passa a ser por **evento**, não por arquivo: soma-se tudo o
que um mesmo gancho devolve — os injetáveis daquele evento mais o que o gancho compõe
ao lado deles — e é essa soma que enfrenta o teto, com margem para um censo de
repositório grande. O caso do repositório pequeno continua passando; o caso do
monorepo passa a falhar antes de chegar ao usuário.

Junto vai um segundo defeito do mesmo mecanismo: a migração que atualiza instalações
anteriores à divisão reconhece a entrada do roteador por **texto exato**
(`.claude/mustard/orchestrator.md`). Um projeto que a tenha declarado com `./` na
frente, ou com barra invertida, recebe o `dispatch.md` no disco e nunca o declara — o
estado que a própria documentação da função chama de pior que o arquivo grande demais
que ela substituiu.

## Critérios de Aceitação

- **AC-1** — quando o texto que um gancho devolve é medido, então a medição soma TODOS
  os injetáveis daquele evento mais o que o gancho compõe ao lado deles, e reprova
  quando a soma passa do teto — ainda que cada arquivo isolado caiba
  Command: `cargo test -p mustard-cli --test template_budget event_budget_sums_every_injectable_of_the_same_hook`
  Expect: `1 passed`
- **AC-2** — quando o censo de terreno cresce até o tamanho de um monorepo, então a
  medição do evento `sessionStart` reprova antes de o excedente chegar ao usuário
  Command: `cargo test -p mustard-rt session_start_payload_stays_in_force_for_a_large_census`
  Expect: `1 passed`
- **AC-3** — quando a migração encontra a entrada do roteador declarada com uma grafia
  equivalente (`./` na frente, barra invertida, barra final), então ela reconhece a
  entrada e declara o segundo injetável, em vez de semear um arquivo que nunca é
  entregue
  Command: `cargo test -p mustard-core backfill_dispatch_inject_matches_equivalent_path_spellings`
  Expect: `1 passed`

## Arquivos

| arquivo | papel nesta correção |
|---|---|
| `apps/cli/tests/template_budget.rs` | a catraca que hoje mede por arquivo |
| `packages/core/src/platform/project_seed.rs` | o teto por semente e a migração `backfill_dispatch_inject` |
| `apps/rt/src/hooks/session/session_start_inject.rs` | o gancho que funde censo + injetáveis + avisos num texto só |
| `apps/rt/tests/` | a catraca do texto composto sob censo grande |

<!-- wikilinks-footer-start -->
- [pergunta-abertura-unidade-pergunta-tipo](pergunta-abertura-unidade-pergunta-tipo.md)
<!-- wikilinks-footer-end -->