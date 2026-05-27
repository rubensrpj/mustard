# Wave 5 — QA + teste de integração end-to-end

### Stage: Close
### Outcome: Completed
### Flags: 

## Contexto

Validar que as quatro waves anteriores fecham os 6 critérios de aceitação (AC) declarados na spec-raiz, sem regressão. Inclui um teste de integração novo que simula uma pipeline wave-plan inteira do início ao fim e verifica que `spec.md` + `meta.json` ficam alinhados em cada transição.

## Tarefas

- [x] **T5.1** — Escrever `apps/rt/tests/status_sync_integration.rs` que:
  - Cria spec teste via `spec_draft` (5 waves, mixed).
  - Emite `pipeline.stage: Execute` no parent.
  - Para cada wave 1..5: emite `pipeline.wave.complete`.
  - Emite `pipeline.status: closed` no parent.
  - Verifica que: (a) parent `spec.md` + parent `meta.json` ambos com `stage=Close, outcome=Completed`; (b) cada `wave-N-*/spec.md` + `wave-N-*/meta.json` ambos com `stage=Close, outcome=Completed`.
- [x] **T5.2** — Rodar os 6 ACs declarados na spec-raiz, em ordem. Reportar pass/fail por AC.
- [x] **T5.3** — Rodar `cargo clippy -p mustard-rt -- -D warnings` e garantir zero warnings.
- [x] **T5.4** — Atualizar `templates/refs/feature/spec-language.md` (se existir e descrever o fluxo de status) — registrar que toda transição agora sincroniza spec.md+meta.json juntos. Surgical change.

## Critérios de Aceitação

- **AC-W5.1** — `cargo test -p mustard-rt --test status_sync_integration` passa. Command: `rtk cargo test -p mustard-rt --test status_sync_integration`
- **AC-W5.2** — Todos os 6 ACs da spec-raiz (AC-1..AC-6) retornam exit 0 quando executados em ordem. Command: `rtk mustard-rt run qa-run --spec 2026-05-26-spec-status-consistency`
- **AC-W5.3** — `cargo clippy -p mustard-rt -- -D warnings` zero warnings. Command: `rtk cargo clippy -p mustard-rt -- -D warnings`

## Limites

- **IN**: `apps/rt/tests/status_sync_integration.rs` (novo).
- **OUT**: nenhuma mudança em código de produção (W5 é só QA — se algum AC falha, abre tactical-fix sub-spec, não mexe aqui).
