# Feature: guardrail-catalog

### Stage: Close
### Outcome: Completed
### Flags:
### Scope: full
### Checkpoint: 2026-05-19T20:15:00Z
### Lang: pt

> **Fase 2 de 2** do epic *Mustard verificável sobre si mesmo* — ver `.claude/plans/self-verifiability-roadmap.md`. A fase 1 (`mustard-doctor`) entregou o diagnóstico; esta fecha o gap de medição. Reframe honesto: o valor não são os IDs (polimento) — é a cobertura de regressão por regra que prova que cada guardrail funciona, espelhando os 17 testes R01–R13 do harness concorrente. Os IDs são o meio: não se escreve "teste para BG05" sem BG05 existir.

## PRD

## Contexto

O `bash_guard` do `mustard-rt` bloqueia comandos perigosos a partir de uma tabela declarativa de 13 regras (`DANGER_RULES`). A tabela é sólida, mas as regras são anônimas: não há identificador estável para nenhuma. Sem isso, ninguém consegue dizer "fui bloqueado pela regra X" num teste, num evento de telemetria ou na documentação — e, pior, não há garantia de que cada uma das 13 regras realmente bloqueia o que promete: a suíte atual cobre só algumas (rm-rf, force-push, reset-hard, mkfs) e deixa o resto sem rede. Uma regressão silenciosa numa regra não-testada passaria batida. O harness concorrente tem um teste de regressão por regra; o Mustard apenas afirma que as suas funcionam. Esta feature dá a cada regra um ID estável (`BG01`–`BG13`) e uma rede de regressão completa — todo guardrail passa a ser nomeável e provado.

## Usuários/Stakeholders

Quem depende dos guardrails de Bash (todo uso do Mustard) e quem mantém o `bash_guard`. Direção derivada da análise competitiva `claude-code-harness` e da fase 1 deste epic.

## Métrica de sucesso

Cada uma das 13 regras de `DANGER_RULES` tem um ID estável que aparece na mensagem de bloqueio, e `cargo test -p mustard-rt` exercita cada regra individualmente — uma regressão em qualquer guardrail quebra um teste nomeado.

## Não-Objetivos

- Não adicionar, remover nem alterar o comportamento de nenhuma regra — só rastreabilidade e cobertura. Mudança de comportamento zero.
- Não adotar o break-glass por TOML do harness — o Mustard já tem modos por env var; segunda fonte de config seria regressão.
- Não dar IDs ao `REDIRECT_MAP` (advisory) nem ao commit-gate — concerns distintos, já com cobertura de teste; prefixo próprio fica para uma spec futura se necessário.
- Não construir infraestrutura nova de evento de telemetria — o ID entra na mensagem e, se já houver um evento de bloqueio, no payload dele.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: O crate `mustard-rt` compila — Command: `cargo build -p mustard-rt`
- [x] AC-2: Os testes do `mustard-rt` passam (um teste de regressão por regra) — Command: `cargo test -p mustard-rt`
- [x] AC-3: As 13 regras têm um ID `BGNN` distinto no `bash_guard.rs` — Command: `node -e "const m=require('fs').readFileSync('apps/rt/src/hooks/bash_guard.rs','utf8').match(/BG[0-9]{2}/g)||[]; if(new Set(m).size!==13) process.exit(1)"`
- [x] AC-4: O catálogo BG01–BG13 existe no `pipeline-config.md` — Command: `bash -c 'grep -q "BG01" apps/cli/templates/pipeline-config.md'`

## Plano

## Informações da Entidade

N/A — infraestrutura (tabela de enforcement; sem entidade de schema).

## Arquivos

- `apps/rt/src/hooks/bash_guard.rs` — campo `id: &'static str` no struct `DangerRule`; IDs `BG01`–`BG13` nas 13 entradas de `DANGER_RULES`; o ID incluído na `reason` do `Verdict::Deny` de `bash_safety()`; um teste de regressão por regra.
- `apps/cli/templates/pipeline-config.md` — tabela-catálogo das 13 regras (ID, o que bloqueia, gatilho).

## Tarefas

### Impl Agent (Wave 1) — IDs + catálogo + regressão

- [x] ANALYZE: confirmar as 13 entradas de `DANGER_RULES` na ordem atual; verificar no dispatcher se um bloqueio de `bash-safety` gera evento de telemetria — se gerar, o ID entra no payload; se não, fica só na mensagem (sem criar evento novo).
- [x] Adicionar `id: &'static str` ao struct `DangerRule`; atribuir `BG01`..`BG13` na ordem atual (BG01 = rm recursivo-force … BG13 = reboot).
- [x] `bash_safety()`: incluir o ID na `reason` do `Verdict::Deny` — ex.: `[bash-safety BG01] Recursive force delete blocked.`
- [x] Teste de regressão por regra: um teste table-driven `[(id, comando_gatilho, Option<comando_seguro>)]` que, para cada BG, confirma que o gatilho é bloqueado e que a `reason` carrega o ID; onde houver variante segura (ex.: `--force-with-lease`), confirmar que ela passa.
- [x] Catálogo em `templates/pipeline-config.md`: tabela `| ID | Bloqueia | Gatilho |` para BG01–BG13.
- [x] `cargo build -p mustard-rt` e `cargo test -p mustard-rt` verdes.

## Dependências

Nenhuma. A tabela `DANGER_RULES` e a infra de testes de paridade já existem em `bash_guard.rs`.

## Limites

- `apps/rt/src/hooks/bash_guard.rs` e `apps/cli/templates/pipeline-config.md`.
- **Fora dos limites:** o comportamento das regras (mudança zero — os predicados `test` não mudam); `REDIRECT_MAP` e o commit-gate; novos eventos de telemetria; o break-glass por TOML.

## Preocupações

Registradas no REVIEW (Wave 1 — verdict APPROVED). Achados **pré-existentes** que o catálogo agora torna visíveis — fora de escopo desta spec, candidatos a uma limpeza futura do `bash_guard`:

- **BG07 (`is_branch_delete_main`) tem código morto** — o comando é minúsculo antes do match, então `-D` vira `-d`; o ramo `-D` em `tokens.windows(2)` nunca dispara. A regra funciona (via `-d`), mas o literal `-D` é inalcançável.
- **BG03 (`git reset --hard`)** usa `contains("--hard")` em vez de uma checagem com word-boundary — pré-existente.
