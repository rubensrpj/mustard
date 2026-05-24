# W11 — verify-pipeline multistack expansion

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: light
### Checkpoint: 2026-05-24T19:30:00Z
### Lang: pt-BR
### Parent: 2026-05-24-mustard-unification

## Contexto

W0 entregou o fix mínimo (`discover_defaults` prefere Cargo.toml; `effective_timeout` por stack). Esta onda generaliza: cada subprojeto detectado pelo `sync-detect` ganha sua própria verification parcial (paralelo via rayon). Saída JSON inclui `per_subproject` breakdown.

## Tarefas

- [ ] **T11.1.** Modificar `apps/rt/src/run/verify_pipeline.rs::verify`:
  - Receber `Vec<VerifyTarget>` do discovery.
  - Disparar verifications em paralelo usando `rayon::par_iter` (cada target em thread).
  - Coletar resultados via `Mutex<Vec<...>>` ou retorno direto.
- [ ] **T11.2.** Saída JSON nova:
  ```json
  {
    "overall": "pass|fail",
    "per_subproject": {
      "cli":  { "ok": true, "build": "pass", "test": "pass", "duration_ms": 12345 },
      "rt":   { "ok": false, "build": "pass", "test": "fail", "error": "...", "duration_ms": 56789 },
      ...
    },
    "passed": [...], "failed": [...], "skipped": [...],
    "total_duration_ms": N,
    "timestamp": "..."
  }
  ```
  Compat: `passed`/`failed`/`skipped`/`timestamp` preservados para retro-compat.
- [ ] **T11.3.** Timeouts por stack via env (W0 já entregou `effective_timeout`; W11 adiciona suporte a configurar via `mustard.json#verifyTimeouts.{rust,ts,python}`).
- [ ] **T11.4.** Testes:
  - `verify_pipeline_runs_subprojects_in_parallel` — fixture com 2 targets, valida `total_duration_ms < sum(duration_ms)`.
  - `verify_pipeline_per_subproject_breakdown` — JSON shape.
- [ ] **T11.5.** Atualizar `apps/cli/templates/commands/mustard/close/SKILL.md` (já cortado em W7) para mencionar saída `per_subproject` no JSON consumido por `close-orchestrate`.
- [ ] **T11.6.** Emit `pipeline.economy.operation.invoked { operation: "verify-pipeline", duration_ms, per_subproject: N }`.

## Files

- `apps/rt/src/run/verify_pipeline.rs`
- `apps/cli/templates/commands/mustard/close/SKILL.md` (referencia o shape novo)
- `apps/rt/Cargo.toml` (confirmar `rayon` já é dep — projeto já usa em scan)

## Critérios de Aceitação

- [ ] **AC-11.1.** `mustard-rt run verify-pipeline --json` em monorepo Mustard retorna `per_subproject` com pelo menos 4 entries (cli, rt, core, dashboard). Command: `rtk mustard-rt run verify-pipeline --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.per_subproject||Object.keys(j.per_subproject).length<4)process.exit(1)})"`
- [ ] **AC-11.2.** `total_duration_ms < sum(per_subproject.duration_ms)` (paralelização real). Command: derived from AC-11.1 output.
- [ ] **AC-11.3.** `cargo test -p mustard-rt verify_pipeline_per_subproject` passa.
- [ ] **AC-11.4.** Env override `MUSTARD_VERIFY_TIMEOUT_RUST=10` força timeout custom. Command: `MUSTARD_VERIFY_TIMEOUT_RUST=10 rtk mustard-rt run verify-pipeline --json` (em projeto canário que leva >10s).
- [ ] **AC-11.5.** Saída antiga (`passed`/`failed`/`skipped`) ainda presente (retro-compat). Command: derived.

## Notas

- Paralelizável com W10.
- `rayon` já é usado pelo scan (project-profiler W1) — não adiciona dep.
- Não tocar em `discover_defaults` (W0 entregou).
