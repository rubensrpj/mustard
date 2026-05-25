# QA — Mustard Deep Refactor

Validação consolidada após todas as 13 waves entregarem. Roda ACs globais (`AC-G1` a `AC-G8`) + agrega ACs locais de cada wave.

## Critérios

- [ ] **AC-Q1.** AC-G1 a AC-G8 do `spec.md` raiz: todos PASS.
- [ ] **AC-Q2.** ACs locais de cada `wave-N-{role}/spec.md`: ≥95% PASS.
- [ ] **AC-Q3.** `mustard-rt run active-specs --format json` retorna 1 spec (esta).
- [ ] **AC-Q4.** Build + lint do workspace verdes.

## Comandos

```bash
rtk mustard-rt run qa-run --spec 2026-05-25-mustard-deep-refactor
rtk cargo build --workspace && rtk cargo clippy --workspace -- -D warnings
rtk pnpm --filter mustard-dashboard build && rtk pnpm --filter mustard-dashboard lint
```
