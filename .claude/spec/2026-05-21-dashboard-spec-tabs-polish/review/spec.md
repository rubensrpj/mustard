# Review — dashboard-spec-tabs-polish

## Resumo

Code review consolidado após W1-W4 fecharem. Um review pra dashboard (UI + Tauri), um pra rt (apenas se W2 modificou `spec_children.rs`).

## Tarefas

- [ ] Dispatch review dashboard. Foco: bugs corrigidos sem regressão (W1: loading gate, FS fallback, trace expand); restruct sem leak de state (W2: pin, Onda#0, sub-specs por wave); layout radial determinístico (W3); paleta consistente (W4).
- [ ] Dispatch review rt (apenas se W2 mexeu em spec_children.rs): correlação wave correta, fail-open, testes.
- [ ] Tactical-fix candidates ≤100 LOC surface se houver.

## Acceptance Criteria

- [ ] AC-R-1: Build full passa — Command: `cargo check --workspace`
- [ ] AC-R-2: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`

## Limites

Sem mudança de código.

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
- Depende: [[wave-1-ui]], [[wave-2-ui]], [[wave-3-ui]], [[wave-4-ui]]
