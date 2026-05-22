# Plano de QA

### Parent: [[2026-05-21-wave-integrity-and-doctor-check]]
### Stage: Plan
### Outcome: Active
### Flags: 

Critérios de Aceitação consolidados de todas as waves.

## Acceptance Criteria (consolidated)

Os ACs canônicos vivem em `wave-plan.md` (cross-wave) e em cada `wave-N-{role}/spec.md` (por wave). Esta página agrega para o QA agent rodar tudo numa passada.

- [ ] AC-1: `cargo build --workspace` passa — Command: `cargo build --workspace`
- [ ] AC-2: `cargo test -p mustard-rt` passa (inclui novos testes de wave_scaffold + plan_from_spec + doctor) — Command: `cargo test -p mustard-rt`
- [ ] AC-3: `wave-scaffold` com `waves: []` retorna erro reportável e não cria artefatos — ver `wave-plan.md § Critérios de Aceitação AC-3`
- [ ] AC-4: `wave-scaffold` com `total_waves != waves.len()` emite WARN em stderr — ver `wave-plan.md § Critérios de Aceitação AC-4`
- [ ] AC-5: `mustard-rt run plan-from-spec --waves 2 --roles general,frontend` emite JSON canônico — ver `wave-plan.md § Critérios de Aceitação AC-5`
- [ ] AC-6: SKILL `/feature` referencia `plan-from-spec` — ver `wave-plan.md § Critérios de Aceitação AC-6`
- [ ] AC-7: `doctor` reporta WARN para wave referenciada sem diretório — ver `wave-plan.md § Critérios de Aceitação AC-7`
- [ ] AC-8: `mustard-rt run doctor --json` emite JSON parseável com array `checks` — ver `wave-plan.md § Critérios de Aceitação AC-8`
- [ ] AC-9: Dashboard builda incluindo DoctorBadge na Sidebar — ver `wave-plan.md § Critérios de Aceitação AC-9`

<!-- report → qa/report.md -->
