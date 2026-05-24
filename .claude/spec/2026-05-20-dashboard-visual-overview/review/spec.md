# Review Plan — Visão Geral redesenhada

## PRD

## Contexto

Plano de Review declarado upfront. Executado por `/mustard:review` no subprojeto `apps/dashboard` após todas as 4 waves de execução marcarem `completed`. Verdict em `review/verdict.md`.

## Checklist por categoria

Reviewer `sonnet`, lê o diff agregado das 4 waves.

- [ ] Correctness: cada wave entregou o declarado (commands Tauri, badges semânticos, hooks, componentes, integração)?
- [ ] Boundaries: nenhuma wave editou fora do "Limites" do parent (não tocou Sidebar/Topbar/outras pages/components/specs)?
- [ ] React patterns: `useQueries` fan-out, null-guard `data?`, `useStore((s) => s.field)`, HashRouter, `invoke` só via `lib/dashboard.ts` — respeitados?
- [ ] TypeScript: build limpo, sem `any` solto, interfaces espelham structs Rust?
- [ ] Design consistency: novos componentes usam `<DataCard>`, `<SectionHeader>`, `<EmptyState>`, tokens Tailwind 4 — sem cor inline?
- [ ] Karpathy guidelines: surgical, sem refactor extra, sem comments soltos, sem error handling defensivo desnecessário?
- [ ] Acceptance Criteria executáveis: cada AC do parent + waves passa com exit 0 em cmd.exe E bash?

## Acceptance Criteria

- [ ] AC-1: `verdict.md` existe após review — Command: `bash -c 'test -f "$(find .claude/spec -path "*2026-05-20-dashboard-visual-overview/review/verdict.md" | head -1)"'`
- [ ] AC-2: Verdict contém status APPROVED ou REJECTED — Command: `bash -c 'f=$(find .claude/spec -path "*2026-05-20-dashboard-visual-overview/review/verdict.md" | head -1); grep -qE "Status:.*(APPROVED|REJECTED)" "$f"'`

## Network

- Parent: [[2026-05-20-dashboard-visual-overview]]
- Roda depois de: [[wave-4-integration]]
- Desbloqueia: [[qa]]
