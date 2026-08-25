---
id: wave.base-escolhida-pelo-operador-descartada.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.base-escolhida-pelo-operador-descartada.1-backend]] | backend | — | A escolha de base gravada passa a ser conferida por EXISTÊNCIA no remoto, não por pertencimento à lista de configuração — nos dois pontos de leitura. |
| 2 | [[wave.base-escolhida-pelo-operador-descartada.2-cleanup]] | cleanup | [[wave.base-escolhida-pelo-operador-descartada.1-backend]] | Os cinco pontos restantes deixam de consultar a lista de configuração para recusar, e os textos de ajuda deixam de ensinar o modelo apagado. |

## Acceptance Criteria
- AC-1 — quando o operador escolhe uma base que existe no remoto, entao o branch e cortado DESSA base em QUALQUER projeto — inclusive num que declare exatamente uma base, ou nenhuma; a pergunta houve escolha? e respondida pelo catalogo real e nao pela contagem da lista declarada
Command: `cargo test -p mustard-rt the_recorded_base_survives_to_the_cut_in_any_project`
Expect: `1 passed`
- AC-2 — when a base anotada não existe mais no remoto, then ela é ignorada e a derivação assume — a proteção contra base obsoleta continua de pé.
Command: `cargo test -p mustard-rt a_vanished_recorded_base_is_ignored`
Expect: `1 passed`
- AC-3 — quando o projeto declara uma base cujo nome contem barra (release/2026-Q3), entao git delete RECUSA apaga-la, e pr list e git delete funcionam estando sobre ela — o teste roda o comando e observa o efeito, nao procura texto no codigo-fonte
Command: `cargo test -p mustard-rt a_slashed_integration_base_is_never_deleted_and_never_refused`
Expect: `1 passed`
- AC-4 — when o diagnóstico roda num projeto sem lista de configuração, then ele não avisa que falta declarar o fluxo.
Command: `cargo test -p mustard-rt doctor_does_not_ask_for_a_flow_that_the_installer_no_longer_writes`
Expect: `1 passed`
- AC-5 — when a referência que /git manda ler é lida, then ela ensina o modelo atual e não cita mais o apagado.
Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour the_git_reference_teaches_the_measured_model`
Expect: `1 passed`
- AC-6 — when a suíte do projeto roda ao fim das duas ondas, then ela passa inteira.
Command: `cargo test --workspace`
