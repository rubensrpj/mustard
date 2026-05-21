# Plano de Review

### Parent: [[2026-05-21-wave-integrity-and-doctor-check]]
### Status: queued

Checklist para o agente de review.

## Checklist

- [ ] SOLID — `wave_scaffold.rs` mantém SRP; `plan_from_spec.rs` é módulo isolado sem dependência circular; `doctor.rs::check_wave_integrity` segue o pattern dos checks existentes (`CheckResult` + `Status`); `DoctorBadge.tsx` é puramente presentational; hook `useDoctorStatus` é o único ponto de I/O.
- [ ] Patterns — `plan_from_spec.rs` segue o `rt-run-subcommand-pattern` (skill); `doctor.rs` check segue o template das fns `check_*` existentes (assinatura `fn(&Path) -> CheckResult`, fail-open, sem panic); Tauri command `doctor_status` segue o `cli-command-pattern` + fail-open de `cli-failopen-pattern`.
- [ ] Integration — `KNOWN_RUN_SUBCOMMANDS` atualizado para `plan-from-spec` (wave-2); `wave-integrity` aparece no output renderizado (wave-3); SKILL `/feature` referencia `plan-from-spec` (wave-2) e SKILL `/maint` documenta `wave-integrity` (wave-3); flag `--json` produz output canônico (wave-4).
- [ ] Design System — `DoctorBadge` usa apenas tokens Tailwind padrão do dashboard (`bg-emerald-500`, `bg-amber-500`, `bg-rose-500`, `bg-zinc-500`); dot pequeno + label compacto; respeita o aesthetic Linear+Notion definido (memory `feedback_design_aesthetic`).
- [ ] Build — `cargo build --workspace`, `cargo test -p mustard-rt`, `pnpm --filter mustard-dashboard build` verdes.
- [ ] Elegance — wave-1 entrega exatamente o gate prometido (~25 LOC); wave-2 não cresce escopo (sem `--from-table` nesta versão); wave-3 não vira hook nem trigger automático; wave-4 entrega badge minimal (sem página dedicada Doctor.tsx).
- [ ] Cross-wave consistency — wave-2 e wave-3 paralelas, ambas dependem só de wave-1; wave-4 depende de wave-3 (precisa do `wave-integrity` + flag `--json` no doctor).
- [ ] Boundaries respeitados — nenhum arquivo da spec da flatten-spec-layout tocado; no dashboard só Sidebar+novos componentes (Topbar, páginas e SplitDetail intocados).
- [ ] Fail-open — Tauri command `doctor_status` retorna `overall: "unknown"` em vez de propagar erro de spawn/parse; badge mostra ícone neutro nesse caso.

<!-- verdict → review/verdict.md -->
