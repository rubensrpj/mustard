# demo

spec: **demo** · fase: **aprovada** · branch: **feature/demo** · sai de: **dev**

## Andamento

**Ondas**: 1 a fazer · 1 aprovada

- 1: a fazer — A trava lê o comando como o terminal.
- 2: aprovada — A aprovação e as pendências leem o estado.

### Medição

| Medida | Valor |
|---|---|
| Texto colocado pelos ganchos | 2870 caracteres, cerca de 717 tokens |
| Bloqueios por gancho | 1 bloqueios, 0 avisos — `command_guard`: 1 bloqueios, 0 avisos |
| Passos do fluxo contra trabalho | 1 chamadas, 0 recusadas, para 1 ondas prontas — `grill` 1 |
| Tempo por fase | levantamento 1 d 0 h, aprovada 2 h 10 min |
| Revisões | 1 aprovadas, 0 reprovadas |
| Pontos do levantamento | 1 pendentes, 0 fechados |
| Lembretes que apareceram | 0 mensagens antigas lembradas nos pontos |
| Pedidos enviados aos agentes | 1, o maior com 312 linhas |

### Tamanho do pedido e revisão, por onda

| Onda | Linhas do pedido | Reprovações | Última revisão |
|---|---|---|---|
| 2 | 312 | 0 | aprovada |

### Fases e publicações

- **MSTD-STATE-0001**

  - Fase: levantamento
  - Branch: `feature/demo`
  - Base: `dev`

- **MSTD-STATE-0002**

  - Fase: aprovada
  - Pergunta e resposta: Aprovar esta spec? → Aprovar

- **MSTD-PUB-0001**

  - Página: spec
  - Marco: aprovação
  - Deu certo: sim
  - Endereço: [https://claude.ai/code/artifact/demo](https://claude.ai/code/artifact/demo)

### Commits

- **MSTD-COMMIT-0001**

  - Identificador: `5e0c7a91`
  - Título: fix(write-gate): a aprovação sai do estado
  - Ondas: 2
  - Arquivos: `apps/rt/src/hooks/write/scope_guard.rs`
  - Repositório: `.`

- **MSTD-PRSUM-0001** — O portão de escrita passa a ler a aprovação do estado. Nada muda para quem usa.

## Especificação

### Contexto

- **MSTD-CTX-0001** — O Rust roda rápido: 3 a 14 ms por gancho. O custo está nas rodadas do modelo.

  - Origem: MSTD-MSG-0001

### Preocupações

- **MSTD-CONC-0001** — Cerca de 11 arquivos de teste prendem frases da prosa atual.

  - Origem: MSTD-MSG-0001

## Combinado

### Tipo de trabalho

- **MSTD-WORK-0001**

  - Tipos: refatoração
  - Origem: MSTD-MSG-0001

### Pontos do levantamento

- **MSTD-POINT-0001**

  - Grupo de lacunas: limits
  - Lacuna: Tamanho do pedido de cada onda
  - De onde veio: lacuna
  - Situação: pendente
  - Fatos: A montagem do pedido não tem teto de tamanho. (`apps/rt/src/commands/agent/render/mod.rs:798`)
  - Origem: MSTD-MSG-0001

### Regras

- **MSTD-RULE-0001** — A trava de comandos confere o programa e as opções, nunca o texto entre aspas.

  - Exemplo: `git commit -m "... rm -rf ..."` passa; `rm -rf pasta` é barrado.
  - Rótulo no rascunho: R19
  - Origem: MSTD-MSG-0001

### Limites

- **MSTD-LIMIT-0001** — Tamanho do pedido de cada onda.

  - Valor: 500 linhas
  - Origem: MSTD-MSG-0001

### Contratos

- **MSTD-CONTR-0001** — A barra de status tem duas linhas: a branch e a spec; a economia e o modelo.

  - Exemplo: dev · demo · plano · onda 2/4
  - Origem: MSTD-MSG-0001

### Erros e mensagens

- **MSTD-ERR-0001** — Título do pull request acima de 60 caracteres.

  - Mensagem: O título tem 74 caracteres, e o limite é 60. Escreva uma frase mais curta.
  - Origem: MSTD-MSG-0001

### Casos de borda

- **MSTD-EDGE-0001** — Duas sessões gravam a mesma spec ao mesmo tempo.

  - O que acontece: A segunda espera a trava e grava com o número seguinte.
  - Origem: MSTD-MSG-0001

### Fora do escopo

- **MSTD-SCOPE-0001** — Supabase.

  - Motivo: As páginas publicadas já dão o acompanhamento de qualquer máquina.
  - Origem: MSTD-MSG-0001

### Decisões

- **MSTD-DEC-0001** — A página é publicada só nos marcos, e a MSTD-RULE-0001 continua valendo.

  - Por quê: Cada publicação gasta tokens.
  - Origem: MSTD-MSG-0001

## Critérios

### Critérios de aceite

- **MSTD-CRIT-0001**

  - Quando: O pedido montado de uma onda passa de 500 linhas.
  - Então: O binário recusa o despacho, com a mensagem do que passou.
  - Prova: `cargo test -p mustard-rt --test wave_request_limit`
  - Rótulo no rascunho: C-12
  - Origem: MSTD-MSG-0001
  - Última execução: passou (MSTD-CRUN-0001)

### Execuções

- **MSTD-CRUN-0001**

  - Critério: MSTD-CRIT-0001
  - Resultado: passou
  - Código de saída: 0
  - Tempo (ms): 5990

## Ondas

### Onda 1

- **MSTD-WAVE-0001** — A trava lê o comando como o terminal.

  - Critérios: MSTD-CRIT-0001
  - Pronta quando: A suíte da trava passa.
  - Origem: MSTD-MSG-0001
  - Estado da onda: a fazer
  - Recebe: Especificação (2), Combinado (6), A onda e as tarefas dela (1), Critérios (1), Regras da execução (1)

**O pedido da onda 1 · 37 linhas, como o agente as recebe**

```
# demo — onda 1

**O que é isto.** A lista dos itens desta onda, em ordem de execução, montada pelo binário a partir da spec. Nenhum texto vem copiado: cada linha traz o número do item, o tipo dele e o comando que o lê.

**Ler o item pelo número é parte do trabalho.** Rode o comando da linha na hora de trabalhar naquele item, um de cada vez, e leia do mesmo jeito qualquer item que o texto dele citar. Nunca procure o conteúdo em outro arquivo do projeto.

**O que fazer.** As tarefas desta onda, e só elas. Cada critério listado abaixo ganha um teste que prova a regra dele.

**Quando parar.** Se faltar alguma coisa, ou se uma tarefa parecer pedir o que a spec não diz, pare e relate: não decida sozinho e não invente peça nenhuma.

**O que devolver.** O que mudou, arquivo por arquivo; o teste que prova cada critério; e o que ficou aberto.

## Especificação

- MSTD-CTX-0001 (contexto) — `mustard-rt run read specification --spec demo --term MSTD-CTX-0001`
- MSTD-CONC-0001 (preocupação) — `mustard-rt run read specification --spec demo --term MSTD-CONC-0001`

## Combinado

- MSTD-LIMIT-0001 (limite) — `mustard-rt run read agreed --spec demo --term MSTD-LIMIT-0001`
- MSTD-CONTR-0001 (contrato) — `mustard-rt run read agreed --spec demo --term MSTD-CONTR-0001`
- MSTD-ERR-0001 (erro) — `mustard-rt run read agreed --spec demo --term MSTD-ERR-0001`
- MSTD-EDGE-0001 (caso de borda) — `mustard-rt run read agreed --spec demo --term MSTD-EDGE-0001`
- MSTD-SCOPE-0001 (fora do escopo) — `mustard-rt run read agreed --spec demo --term MSTD-SCOPE-0001`
- MSTD-DEC-0001 (decisão) — `mustard-rt run read agreed --spec demo --term MSTD-DEC-0001`

## A onda e as tarefas dela

- MSTD-WAVE-0001 (onda) — `mustard-rt run read waves --spec demo --term MSTD-WAVE-0001`

## Critérios

- MSTD-CRIT-0001 (critério) — `mustard-rt run read criteria --spec demo --term MSTD-CRIT-0001`

## Regras da execução

- Não comite e não use `git add`: o commit é da rodada.

```

### Onda 2

- **MSTD-WAVE-0002** — A aprovação e as pendências leem o estado.

  - Critérios: MSTD-CRIT-0001
  - Pronta quando: A suíte das travas passa lendo só o spec.ndjson.
  - Depende das ondas: 1
  - Origem: MSTD-MSG-0001
  - Estado da onda: aprovada
  - Commit: `5e0c7a91` (MSTD-COMMIT-0001)
  - Recebe: A onda e as tarefas dela (2)

- **MSTD-TASK-0001** — O portão de escrita lê a aprovação do estado.

  - Arquivos: `apps/rt/src/hooks/write/scope_guard.rs`, `apps/rt/src/hooks/write/rule.rs` (novo)
  - Skill: `add-hook-rule`
  - Cobre: MSTD-RULE-0001
  - Origem: MSTD-MSG-0001

- **MSTD-SEND-0001**

  - Papel: agente de onda
  - Linhas: 312
  - Caracteres: 21480
  - Itens enviados: MSTD-CTX-0001, MSTD-CONC-0001, MSTD-RULE-0001, MSTD-WAVE-0002, MSTD-TASK-0001, MSTD-CRIT-0001
  - Versão do Mustard: `0.2.0`
  - Lições: 7
  - Skills: `add-hook-rule` (`3f9a1c2e`)

**Pedido enviado (agente de onda) · 8 linhas, como o agente o recebeu**

```
# demo — onda 2

**O que é isto.** A lista dos itens desta onda.

## A onda e as tarefas dela

- MSTD-WAVE-0002 (onda) — `mustard-rt run read waves --spec demo --term MSTD-WAVE-0002`
- MSTD-TASK-0002 (tarefa) — `mustard-rt run read waves --spec demo --term MSTD-TASK-0002`
```

- **MSTD-DELIV-0001**

  O portão de escrita lê a aprovação do estado.

  - o teste da trava passa;
  - nada muda para quem usa.

  - Arquivos: `apps/rt/src/hooks/write/scope_guard.rs`

### Skills

- **MSTD-SKILL-0001** — Passos para acrescentar uma regra ao portão de escrita, com o teste.

  - Nome: `add-hook-rule`
  - Ação: criação
  - Identificador: `3f9a1c2e`
  - Exemplos: `apps/rt/src/hooks/write/scope_guard.rs`: mesma pasta e mesmas importações, com teste

## Revisão e QA

### Onda 2

- **MSTD-VERD-0001** — Sem achados.

  - Onda: 2
  - Resultado: aprovada
  - Critérios: MSTD-CRIT-0001 (confere a regra)
  - Lições: 7 (não repetiu)

## O que o plano achou

### Sobre tarefas

- **MSTD-NOTE-0003** · depois da aprovação · 2026-09-12 11:13 — A tarefa MSTD-TASK-0001 cita `src/fora.rs`, que o git não guarda: um agente noutra sessão não o vê.

  - Origem: MSTD-MSG-0001

## Anotações

### Anotações

- **MSTD-REQ-0001** · depois da aprovação · 2026-09-12 11:06 — Incluir o Windows no teste de duas gravações ao mesmo tempo.

  - Efeito: ajusta as ondas
  - Origem: MSTD-MSG-0001

- **MSTD-DEFER-0001** · depois da aprovação · 2026-09-12 11:07 — Medir o antivírus do Windows na verificação automática.

  - Pendência: 3
  - Origem: MSTD-MSG-0001

- **MSTD-NOTE-0002** · depois da aprovação · 2026-09-12 11:09 — O pull request 276 corrigiu o Clippy no dev antes da primeira onda.

  - Origem: MSTD-MSG-0001

## Conversa

### Dia 11/09

- **MSTD-MSG-0001** · mensagem · usuário · 2026-09-11 08:40 — Revise o Mustard inteiro.

- **MSTD-RESP-0001** · resposta · assistente · 2026-09-11 08:42 — Comecei pelo levantamento.

  - Responde a: MSTD-MSG-0001

- **MSTD-INJ-0001** · injeção · gancho · 2026-09-11 08:43 — Spec demo, fase levantamento.

  - Gancho: `session_start_inject`
  - Caracteres: 2870

- **MSTD-HOOK-0001** · gancho · gancho · 2026-09-11 08:44

  - Gancho: `command_guard`
  - Ação: bloqueio
  - Ferramenta: `Bash`
  - Motivo: rm -rf apaga trabalho sem volta.

- **MSTD-CALL-0001** · chamada · binário · 2026-09-11 08:45

  - Comando: `grill`
  - Tempo (ms): 41
  - Resultado: ok

- **MSTD-DEC-0001** · decisão · versão substituída · 2026-09-11 09:11 — A página é publicada a cada passo.

  - Por quê: Mostra tudo na hora.
  - Origem: MSTD-MSG-0001

### Dia 12/09

- **MSTD-RMV-0001** · remoção · assistente · 2026-09-12 11:10

  - Motivo: Colada por engano.
  - Itens: MSTD-NOTE-0001
  - Origem: MSTD-MSG-0001

- **MSTD-PURGE-0001** · expurgo · assistente · 2026-09-12 11:12

  - Itens: MSTD-MSG-0002
  - Motivo: segredo
  - Origem: MSTD-MSG-0001
