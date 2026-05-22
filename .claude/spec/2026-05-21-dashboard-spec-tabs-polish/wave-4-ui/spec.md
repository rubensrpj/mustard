# Wave 4 — Design: cores por fase + pulse + paleta de badges

### Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T17:00:00Z

## Resumo

Dois polimentos visuais: (9) o `<PipelineTimeline>` (ANALYZE/PLAN/EXECUTE/REVIEW/QA/CLOSE) ganha cor distinta por fase e a fase ATIVA pulsa via `animate-pulse`; (10) auditoria de todos os badges do app — `StatusPill`, `PhaseChip`, `+N waves`, `+N sub-specs`, source badge ("evento"/"header"/"ambos"), wave status pill, AC status — todos ganham cor com semântica consistente.

## Contexto

Hoje o `<PipelineTimeline>` usa a mesma cor para todas as fases — diferencia apenas por estado (completed/active/future). O usuário quer ver de relance a fase em cor própria: ANALYZE azul-frio (descoberta), PLAN roxo (design), EXECUTE âmbar/dourado (mustard accent — ação), REVIEW verde-azulado (verificação), QA verde (validação), CLOSE cinza-final. Quando uma fase está active, a pill pulsa.

Para badges, o problema é inconsistência: `StatusPill` já tem cores boas; `PhaseChip` provavelmente é monocromático; os badges `+N waves` e `+N sub-specs` em `SpecCard` são cinza neutro. Auditar e dar cor com semântica:
- waves badge → `--color-accent-mustard` tonal (matches o "execution" feel)
- sub-specs badge → `--color-accent-cyan` (ou outra; conexão lateral)
- source badge ("evento") → azul (telemetria); ("header") → âmbar (declarativo); ("ambos") → verde
- AC pass → `--color-ok`; fail → `--color-error`; skip → cinza; unknown → cinza-muted

## Arquivos

```
apps/dashboard/src/components/telemetry/PipelineTimeline.tsx    — cores por fase + animate-pulse
apps/dashboard/src/components/specs/spec-status.tsx             — paleta consistente nos status
apps/dashboard/src/components/page/PhaseChip.tsx                — cor por fase (mesma paleta da timeline)
apps/dashboard/src/components/specs/SpecCard.tsx                — badges +N waves / +N sub-specs com cor
apps/dashboard/src/components/specs/SpecChildrenTab.tsx         — source badge com cor (se ainda existir após W2)
apps/dashboard/src/components/specs/SpecWavesTab.tsx            — wave status pill (sem mudança de comportamento, só revisão)
```

## Tarefas

- [ ] **(9) PHASE_COLORS map.** Em `PipelineTimeline.tsx`, definir:
  ```ts
  const PHASE_COLORS: Record<string, { bg: string; text: string; border: string }> = {
    analyze: { bg: "bg-sky-500/15",     text: "text-sky-400",     border: "border-sky-500/30" },
    plan:    { bg: "bg-violet-500/15",  text: "text-violet-400",  border: "border-violet-500/30" },
    execute: { bg: "bg-[--color-accent-mustard]/15", text: "text-[--color-accent-mustard]", border: "border-[--color-accent-mustard]/30" },
    review:  { bg: "bg-teal-500/15",    text: "text-teal-400",    border: "border-teal-500/30" },
    qa:      { bg: "bg-emerald-500/15", text: "text-emerald-400", border: "border-emerald-500/30" },
    close:   { bg: "bg-slate-500/15",   text: "text-slate-400",   border: "border-slate-500/30" },
  };
  ```
  Cada chip da timeline herda essas classes. Fase futura: `opacity-40` ou `border-dashed`. Fase ativa: `animate-pulse` + ring sutil (`ring-2 ring-{color}/40`).
- [ ] **(9) Pulse na ativa.** A fase ativa (passada via prop `currentPhase`) recebe `animate-pulse`. Cuidado para o pulse não brigar com o `transition-colors` em hover — usa `motion-safe:animate-pulse` se houver preferência de reduced motion.
- [ ] **(10) PhaseChip cor.** Confirme onde vive (`@/components/page/PhaseChip` ou similar). Aplique o mesmo `PHASE_COLORS` (exportar de `PipelineTimeline` ou mover para `@/lib/phase-palette.ts` se for usado em 2+ lugares).
- [ ] **(10) Badges com cor — SpecCard.** O `+N waves` badge passa de `bg-muted/60 text-muted-foreground` para `bg-[--color-accent-mustard]/15 text-[--color-accent-mustard]`. `+N sub-specs` passa para `bg-cyan-500/15 text-cyan-400` (ou tom equivalente do projeto).
- [ ] **(10) Source badge.** Em `SpecChildrenTab.tsx` (se ainda existir após W2), os spans `evento`/`header`/`ambos` ganham:
  - `event`: `bg-sky-500/15 text-sky-400`
  - `header`: `bg-amber-500/15 text-amber-400`
  - `both`: `bg-emerald-500/15 text-emerald-400`
  (Se a aba foi removida em W2 e o badge migrou para SpecWavesTab nas sub-specs aninhadas, aplica lá.)
- [ ] **(10) AC status pill.** No `SpecQualityTab` (já pinta via `StatusPill`), confirme que pass=ok, fail=error, skip=muted, unknown=muted-foreground. Sem mudança grande aqui.
- [ ] **(10) Wave status pill.** Já tem cores boas (queued=muted, in_progress=mustard, completed=ok, failed=error). Sem mudança.
- [ ] Build: `pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W4-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W4-2: PipelineTimeline tem map de cores por fase — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/telemetry/PipelineTimeline.tsx','utf8');process.exit(/PHASE_COLORS|phaseColor/.test(s)?0:1)"`
- [ ] AC-W4-3: PipelineTimeline usa animate-pulse — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/telemetry/PipelineTimeline.tsx','utf8');process.exit(/animate-pulse/.test(s)?0:1)"`
- [ ] AC-W4-4: SpecCard +N waves badge tem cor accent-mustard (não mais muted) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');const m=s.match(/\\+[^}]*waves[\\s\\S]{0,300}className=\\{?[\"\\\`]([^\"\\\`]+)/);process.exit(m && /accent-mustard|mustard\\/15/.test(m[1])?0:1)"`

## Limites

- `apps/dashboard/src/components/telemetry/PipelineTimeline.tsx`
- `apps/dashboard/src/components/specs/spec-status.tsx`
- `apps/dashboard/src/components/page/PhaseChip.tsx` (se for o caminho — pode estar em outro lugar, descobrir via grep)
- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/SpecChildrenTab.tsx`
- `apps/dashboard/src/lib/phase-palette.ts` (NOVO se exportar pra reuso)

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
- Depende: [[wave-1-ui]] (paralelizável com W2/W3)
