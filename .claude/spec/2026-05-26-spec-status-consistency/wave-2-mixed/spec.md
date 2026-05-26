# Wave 2 — doctor check `status-consistency`

### Stage: Plan
### Outcome: Active
### Flags: 

## Contexto

Hoje `apps/rt/src/run/doctor.rs:1097-1106` lista cinco checks (`skill-discovery`, `wave-integrity`, `claude-paths`, `workspace-leaks`, `i1`). Nenhum cobre consistência de status — quando uma spec entra em estado descasado, nada alerta. Esta wave adiciona um check novo que detecta os três cenários ruins identificados.

Premissa: rodar **após** W1 (porque sem `sync_status`, o check ia ficar gritando alarmes em situações que o código deveria já ter resolvido — falso positivo).

## Tarefas

- [ ] **T2.1** — Adicionar variant `StatusConsistency` ao enum de checks conhecidos em `doctor.rs:1097-1106`. Wire-up no `match`/`dispatch` que roda checks.
- [ ] **T2.2** — Implementar `check_status_consistency(claude_paths: &ClaudePaths) -> CheckResult` que:
  - Itera `.claude/spec/*/spec.md`.
  - Para cada spec: parse cabeçalho buscando `### Stage:` e `### Outcome:`. Falha (FAIL) se ausente.
  - Lê o `meta.json` ao lado. Falha se `stage`/`outcome` ausentes ou diferentes do `spec.md`.
  - Compara o par `(stage, outcome)` com a tabela `state_from_status_word`. Falha se combinação não mapeada (ex: `(Analyze, Cancelled)` que não existe).
  - Recursa em `wave-N-*/spec.md` + `wave-N-*/meta.json` do mesmo modo.
- [ ] **T2.3** — Incluir `status-consistency` no agregador default (`doctor --check all`) e no comando sem flag (`doctor` puro).
- [ ] **T2.4** — Output formatado igual aos outros checks (linhas `OK`/`WARN`/`FAIL` com path da spec).

## Critérios de Aceitação

- **AC-W2.1** — `mustard-rt run doctor --check status-consistency` existe (não erro `unknown check`). Command: `rtk mustard-rt run doctor --check status-consistency || true`
- **AC-W2.2** — Em uma spec teste com `### Stage: Close` + `### Outcome: Active` E `meta.json` igual, o check passa (`closed-followup` é estado mapeado válido). Command: `rtk cargo test -p mustard-rt doctor_status_consistency_closed_followup_ok`
- **AC-W2.3** — Em uma spec teste com `### Stage: Analyze` + `### Outcome: Cancelled` (combinação não mapeada), o check falha com mensagem clara. Command: `rtk cargo test -p mustard-rt doctor_status_consistency_invalid_combo_fail`
- **AC-W2.4** — Em uma spec teste com `spec.md` Stage=Execute mas `meta.json` Stage=Plan, o check falha com mensagem que cita ambos os valores. Command: `rtk cargo test -p mustard-rt doctor_status_consistency_divergence_fail`

## Limites

- **IN**: `apps/rt/src/run/doctor.rs`.
- **OUT**: não criar novo subcomando (é uma flag de doctor existente); não tocar em event stream.
