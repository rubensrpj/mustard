# W10 — Verify pipeline multi-stack + wave-integrity-doctor

## Contexto

Duas responsabilidades convergem: (a) `verify-pipeline` hoje é stack-mono (npm test em projeto Rust dá timeout); (b) Spec ativa `wave-integrity-and-doctor-check` (movida para backup) propôs hard gate em `wave-scaffold` + check `wave-integrity` no doctor + `DoctorBadge` na sidebar. Absorve as duas em uma wave.

## Tarefas

### Parte A — Verify pipeline multi-stack

- [x] **T10.1** — `mustard-rt run verify-pipeline --json`: lê `sync-detect` output, dispara N verifications em paralelo (`rayon`). Output `{ overall, per_subproject: [{ name, ok, duration_ms, output }], total_duration_ms }`. Timeouts por stack via env (`MUSTARD_VERIFY_TIMEOUT_RUST=600`, `MUSTARD_VERIFY_TIMEOUT_TS=120`, `MUSTARD_VERIFY_TIMEOUT_PYTHON=180`).
- [x] **T10.2** — Comandos detectados via `stack.md` (W3.T3.1): se manifest declara `[scripts]` block, usa scripts; senão fallback Cargo (`cargo build && cargo clippy`) / pnpm (`pnpm build && pnpm lint`) / python (`python -m pytest`).

### Parte B — Wave integrity + doctor

- [x] **T10.3** — Hard gate em `apps/rt/src/run/wave_scaffold.rs`: `plan.waves.is_empty()` → erro reportado em stderr + exit !=0. Mismatch `total_waves` vs `waves.length` → WARN em stderr, mas continua.
- [x] **T10.4** — `mustard-rt run plan-from-spec --waves N --roles a,b,c --lang pt-BR`: novo subcomando em `apps/rt/src/run/plan_from_spec.rs`. Substitui "orquestrador monta plan.json na cabeça" — Rust monta JSON deterministicamente.
- [x] **T10.5** — Novo check `wave-integrity` em `apps/rt/src/run/doctor.rs`: para cada spec ativa, lê `wave-plan.md`, extrai wikilinks `[[wave-N-{role}]]`, verifica que cada diretório existe.
- [x] **T10.6** — Flag `--json` em `mustard-rt run doctor`. Output `{ checks: [{ name, status: ok|warn|fail, message }], overall }`.
- [x] **T10.7** — Tauri command `doctor_status` em `apps/dashboard/src-tauri/src/doctor.rs` (novo). Invoca `mustard-rt run doctor --json` e devolve para frontend.
- [x] **T10.8** — `apps/dashboard/src/components/DoctorBadge.tsx` (novo). Renderiza no footer da `Sidebar.tsx` — verde (ok), amarelo (warn), vermelho (fail). Tooltip com hint dos comandos de fix.

## Critérios de Aceitação

- [x] **AC-W10.1** — `mustard-rt run verify-pipeline --json` retorna shape correto. Command: smoke test.
- [x] **AC-W10.2** — `wave-scaffold` rejeita `plan.json` vazio. Command: smoke test com plan vazio.
- [x] **AC-W10.3** — `plan-from-spec --waves 2 --roles a,b --lang pt-BR` emite JSON válido. Command: `rtk mustard-rt run plan-from-spec --waves 2 --roles a,b --lang pt-BR | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(j.waves.length!==2)process.exit(1)})"`
- [x] **AC-W10.4** — `doctor --json` emite checks array. Command: `rtk mustard-rt run doctor --json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!Array.isArray(j.checks))process.exit(1)})"`
- [x] **AC-W10.5** — `DoctorBadge` exportado do dashboard. Command: `rtk node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/layout/Sidebar.tsx','utf8');if(!/DoctorBadge/.test(t))process.exit(1)"`

## Limites

`apps/rt/src/run/{verify_pipeline,wave_scaffold,plan_from_spec,doctor}.rs`, `apps/dashboard/src-tauri/src/{lib,doctor}.rs`, `apps/dashboard/src/components/{layout/Sidebar,DoctorBadge}.tsx`, `apps/dashboard/src/lib/doctor.ts`.

OUT: tudo fora.

## Role

mixed (rt + dashboard)
