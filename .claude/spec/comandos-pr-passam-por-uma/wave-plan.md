---
id: wave.comandos-pr-passam-por-uma.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.comandos-pr-passam-por-uma.1-backend]] | backend | — | a porta PrProvider e seus dois adaptadores: GitHub real, Azure esqueleto honesto |
| 2 | [[wave.comandos-pr-passam-por-uma.2-commands]] | commands | [[wave.comandos-pr-passam-por-uma.1-backend]] | pr-open, pr-edit e pr-ready como comandos run, atrás da porta |
| 3 | [[wave.comandos-pr-passam-por-uma.3-integration]] | integration | [[wave.comandos-pr-passam-por-uma.2-commands]] | os consumidores existentes e a prosa passam pela porta |

## Acceptance Criteria
- AC-1 — quando o adaptador recebe uma ref completa do Azure e um nome curto do GitHub, então a porta responde o MESMO nome curto para os dois. Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::a_full_ref_and_a_short_name_answer_the_same_branch -- --exact` Expect: `[1-9][0-9]* passed`
- AC-2 — quando o Azure responde active/completed/abandoned/notSet, então a porta traduz para OPEN/MERGED/CLOSED/OPEN e carrega o mergeStatus verbatim. Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::azure_states_map_to_the_canonical_vocabulary -- --exact` Expect: `[1-9][0-9]* passed`
- AC-3 — quando o provedor em vigor não tem adaptador implementado, então toda operação responde provider-unsupported, nunca um sucesso fingido nem uma ausência medida. Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::a_provider_without_an_adapter_refuses_honestly -- --exact` Expect: `[1-9][0-9]* passed`
- AC-4 — quando pr-open roda, então o relatório nomeia o provedor em vigor e a URL veio do adaptador, não de um gh cru no comando. Command: `cargo test -p mustard-rt --lib commands::review::pr_publish::tests::pr_open_reports_through_the_port -- --exact` Expect: `[1-9][0-9]* passed`
- AC-5 — quando a prosa da porta de PR é lida, então nenhuma linha manda rodar gh pr create/edit/ready direto: o caminho é o comando da porta, e uma catraca em teste guarda a regra Command: `cargo test -p mustard-rt --test pr_prose_door -- --exact` Expect: `[1-9][0-9]* passed`
- AC-6 — quando a unidade termina, então o workspace inteiro compila. Command: `cargo build --workspace`
