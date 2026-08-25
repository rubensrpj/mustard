---
id: spec.base-escolhida-pelo-operador-descartada
---

# a base escolhida pelo operador e descartada na leitura porque e conferida contra a lista velha do git.flow

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**O que acontece hoje.** Abrir um trabalho começa com uma escolha: de qual branch ele
parte. O Mustard mede os branches que existem de verdade no `origin`, mostra a lista,
o operador escolhe, e a escolha é anotada. Um passo depois, na hora de criar o branch,
essa anotação é lida de volta — e conferida contra uma lista guardada no arquivo de
configuração do projeto. Se a escolha não estiver nessa lista, ela é descartada e outra
base assume o lugar.

É um restaurante que mostra o cardápio de hoje, anota o pedido, e manda a cozinha
conferir o pedido contra o cardápio do ano passado. Não achou o prato, faz outro.

**Por que isso é um problema.** A lista de configuração deixou de ser escrita: o
instalador de hoje não pergunta mais quais são os branches e não grava a chave. Sem
ela, a conferência cai para dois nomes fixos, `main` e `master` — os únicos nomes de
branch escritos à mão no produto inteiro. Ou seja, em qualquer projeto novo cujos
branches se chamem `develop`, `producao`, `trunk` ou qualquer outra coisa, **toda**
escolha de base é descartada. Não é um caso de borda: é o caminho padrão.

E o produto se contradiz por escrito. A função que devolve essa lista documenta, em
letras: *"Nothing here refuses anything any more"* — nada aqui recusa mais nada. Seis
pontos do código usam exatamente essa função para recusar.

**A proteção que o filtro queria dar é legítima e não pode sumir.** A documentação diz
por quê: uma base anotada pode ter ficado obsoleta, e cortar de um branch que não
existe mais é pior do que derivar outro. O erro não é proteger — é o teste escolhido.
"Ainda existe?" é uma pergunta mensurável, e o catálogo que responde a ela já está
pronto e em uso. "Está numa lista que ninguém escreve mais?" não mede nada.

**O que muda.** A conferência passa a perguntar existência em vez de pertencimento, nos
dois pontos de leitura. E os cinco lugares restantes que ainda consultam a lista velha
para recusar passam a consultar o que realmente responde à pergunta de cada um.

```
HOJE                                    DEPOIS
catálogo real ──► você escolhe          catálogo real ──► você escolhe
                      │                                      │
                   anotado                                anotado
                      │                                      │
              confere na LISTA VELHA               confere se AINDA EXISTE
                      │                                      │
          não está lá ─┴─► descarta            existe ────────┴─► corta daí
                            e corta de outra    sumiu ─────────► aí sim, deriva
```

**Como termina.** A base que você escolher é a base de onde o trabalho sai — em
qualquer projeto, com qualquer nome de branch. A proteção continua de pé: uma base que
desapareceu do remoto continua sendo ignorada, agora por ter desaparecido de verdade.

## Usuários/Stakeholders

Quem abre trabalho em projeto cujos branches não se chamam `main` e `master` — hoje,
toda instalação nova.

## Métrica de sucesso

A base escolhida na abertura é a base de onde o branch é efetivamente cortado, medida
no branch resultante e não na aceitação do portão.

## Não-Objetivos

- Não remove a proteção contra base obsoleta: ela continua, medindo existência.
- Não reintroduz a lista de configuração como permissão. Ela segue existindo apenas
  como pré-seleção, que é o que sua própria documentação já afirma.
- Não mexe no portão de abertura nem no catálogo — os dois já fazem o certo.

## Critérios de Aceitação

- **AC-1** — quando o operador escolhe uma base que existe no remoto, entao o branch e cortado DESSA base em QUALQUER projeto — inclusive num que declare exatamente uma base, ou nenhuma; a pergunta houve escolha? e respondida pelo catalogo real e nao pela contagem da lista declarada
  Command: `cargo test -p mustard-rt the_recorded_base_survives_to_the_cut_in_any_project`
  Expect: `1 passed`
- **AC-2** — quando a base anotada não existe mais no remoto, então ela é ignorada e a
  derivação assume — a proteção contra base obsoleta continua de pé, agora medindo
  existência em vez de pertencimento
  Command: `cargo test -p mustard-rt a_vanished_recorded_base_is_ignored`
  Expect: `1 passed`
- **AC-3** — quando o projeto declara uma base cujo nome contem barra (release/2026-Q3), entao git delete RECUSA apaga-la, e pr list e git delete funcionam estando sobre ela — o teste roda o comando e observa o efeito, nao procura texto no codigo-fonte
  Command: `cargo test -p mustard-rt a_slashed_integration_base_is_never_deleted_and_never_refused`
  Expect: `1 passed`
- **AC-4** — quando o diagnóstico roda num projeto sem lista de configuração, então ele
  não avisa que falta declarar o fluxo nem prescreve editá-lo
  Command: `cargo test -p mustard-rt doctor_does_not_ask_for_a_flow_that_the_installer_no_longer_writes`
  Expect: `1 passed`
- **AC-5** — quando a referência que `/git` manda ler é lida, então ela ensina o modelo
  atual e não cita mais o modelo apagado
  Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour the_git_reference_teaches_the_measured_model`
  Expect: `1 passed`
- **AC-6** — a suíte do projeto passa inteira
  Command: `cargo test --workspace`

## Arquivos

| arquivo | papel nesta unidade |
|---|---|
| `apps/rt/src/commands/event/work_branch.rs` | a leitura da anotação pendente, onde a escolha é descartada |
| `apps/rt/src/shared/work_kind.rs` | a leitura do registro durável, mesmo descarte, e o teste que o fixa |
| `apps/rt/src/hooks/write/work_branch_gate.rs` | a outra porta de corte — chama o mesmo refresh, que agora recebe a base escolhida |
| `apps/rt/src/commands/event/emit_pipeline.rs` | onde a escolha é anotada — o filtro que a descarta antes de qualquer leitura (AC-1) |
| `apps/rt/src/commands/review/pr_door.rs` | a recusa de `pr list` fora da lista velha |
| `apps/rt/src/commands/git_delete.rs` | a mesma recusa em `git delete` |
| `apps/rt/src/commands/work_unit_open.rs` | a recusa ao abrir worktree, com a saída "declare no mustard.json" |
| `apps/rt/src/commands/doctor/doctor.rs` | o aviso que prescreve o que o instalador removeu |
| `plugin/refs/git/git-flow.md` | a referência que ensina o modelo apagado |

| `apps/rt/src/commands/git_settle.rs` | a terceira porta que lia o checkout de forma divergente |
| `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs` | as catracas das leis desta unidade |

| `apps/rt/src/hooks/bash/rtk_rewrite.rs` | o reescritor que partia `$(mktemp -d)` ao meio |
| `plugin/agents/mustard-review.md` | a lei do diretório descartável para quem revisa |

## Limites

IN: ...
OUT: ...

## Definitions

- **lista velha** — O conjunto devolvido por `ProjectConfig::git::preselected_bases()` (packages/core/src/domain/config.rs:124) — derivado das chaves e valores de `mustard.json#git.flow`. Quando o flow esta vazio ou ausente, ela cai para os literais `{main, master}` (config.rs:136-139), o unico lugar do produto onde nome de branch e escrito na mao.
- **catalogo real** — O que `branch_catalog()` (packages/core/src/platform/git_branches.rs:155) devolve: os branches que existem de fato em `origin`, medidos com `git fetch --prune` mais `git for-each-ref`, ordenados por recencia.
- **escolha gravada** — A base que o operador escolheu e que o portao anotou no marcador pendente (`mark_pending_work_branch`) e, depois do corte, no registro duravel da unidade (`meta.json#base` / arquivo de base do corte). E uma MEDICAO da resposta de uma pessoa, nao uma afirmacao de configuracao.

## Decisions

- A conferencia da escolha gravada deixa de perguntar 'esta na lista declarada?' e passa a perguntar 'esse branch ainda existe no origin?'.
  Reason: A protecao que o filtro queria dar e legitima e esta escrita na propria documentacao da funcao: nao obedecer a uma base gravada que ficou obsoleta, porque o projeto pode ter mudado desde o corte. O erro nao e a protecao, e o teste escolhido. Existencia e mensuravel no catalogo que ja temos; pertencer a uma lista que o instalador nao escreve mais nao mede nada. Repontar o teste preserva a protecao inteira e remove a recusa falsa.
- Os cinco pontos restantes que ainda consultam a lista velha para RECUSAR passam a consultar o conjunto protegido ou o catalogo real, conforme a pergunta que cada um faz.
  Reason: A documentacao de `preselected_bases()` afirma textualmente 'Nothing here refuses anything any more'. Enquanto seis pontos usarem essa funcao para recusar, a funcao e sua documentacao dizem coisas opostas, e o proximo leitor acredita na que estiver mais perto do que ele estiver fazendo.
- O criterio de aceitacao principal segue a escolha de ponta a ponta — do catalogo ate o branch efetivamente cortado — em vez de parar no portao.
  Reason: A unidade anterior fechou com sete criterios verdes e este defeito dentro. Todos os sete mediam a ACEITACAO no portao; nenhum cortava um branch. Um criterio que para na porta certifica a metade construida e nao distingue feito de nao-feito no resto do caminho.

## Evidence

- A escolha gravada e filtrada contra a lista velha na hora de ler de volta: se nao pertencer, e descartada e a derivacao assume.
  Evidence: `apps/rt/src/commands/event/work_branch.rs:360`
- O registro duravel da unidade sofre o mesmo descarte, pelo mesmo teste de pertencimento.
  Evidence: `apps/rt/src/shared/work_kind.rs:446`
- Existe um teste que FIXA o descarte como comportamento correto, com a justificativa de que o projeto pode ter mudado desde o corte — a intencao e legitima, o teste escolhido e que mede a coisa errada.
  Evidence: `apps/rt/src/shared/work_kind.rs:774`
- A documentacao de `preselected_bases()` afirma que nada ali recusa mais nada, contradizendo os pontos que a usam para recusar.
  Evidence: `packages/core/src/domain/config.rs:117`
- Com `git.flow` vazio ou ausente a lista velha cai para os literais {main, master}; e o instalador de hoje nao grava mais `git.flow`, entao toda instalacao nova cai nesse fallback.
  Evidence: `packages/core/src/domain/config.rs:136`
- O `mustard init` nao pergunta mais os branches e nao grava a chave `git.flow` — fixado por teste.
  Evidence: `apps/cli/src/commands/git_flow.rs:315`
- `pr list` recusa fora da lista velha com o motivo 'not-on-integration-base' e manda o operador trocar de branch.
  Evidence: `apps/rt/src/commands/review/pr_door.rs:314`
- `git delete` carrega a mesma recusa, com a mesma lista.
  Evidence: `apps/rt/src/commands/git_delete.rs:101`
- A criacao de worktree recusa um prefixo fora da lista velha e oferece como saida editar `mustard.json#git.flow` — a saida que este desenho removeu.
  Evidence: `apps/rt/src/commands/work_unit_open.rs:627`
- O diagnostico avisa que `git.flow` esta vazio e prescreve declarar o flow; com o instalador de hoje esse aviso dispara em toda instalacao correta, e sua afirmacao sobre o que esta protegido e falsa perante `protected_branches`.
  Evidence: `apps/rt/src/commands/doctor/doctor.rs:600`
- A referencia que `/git` manda ler ensina o modelo apagado: bases de integracao derivadas do flow, tipo decidindo a base, e recusa por estar numa base.
  Evidence: `plugin/refs/git/git-flow.md:23`
- Os sete criterios da unidade anterior mediram a ACEITACAO no portao; nenhum corta um branch, e por isso o defeito da leitura passou verde.
  Evidence: `.claude/spec/base-do-branch-escolhida-numa/spec.md:42`