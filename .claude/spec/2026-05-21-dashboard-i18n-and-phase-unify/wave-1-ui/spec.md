# Wave 1 — PipelineTimeline unificado + Execute verde

### Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T18:00:00Z

## Resumo

Unificar o componente de fases entre Lista e Detalhes: `PipelineTimeline` ganha prop `variant: "compact" | "default"`. O `MiniTimeline` (local em `SpecCard.tsx`) some — `SpecCard` passa a usar `<PipelineTimeline variant="compact">` direto. O `SpecDetailDashboard` usa `<PipelineTimeline variant="default">` full-width (ocupando todo o painel). EXECUTE muda de mustard para verde brilhante (`green-500`); REVIEW muda de teal para amber para evitar conflito visual com qa (emerald). O subtitle redundante "{spec slug}" abaixo do Analyze no `SpecDetailDashboard` some.

## Contexto

Hoje `SpecCard.tsx` define um `MiniTimeline` interno que aplica `scale-[0.82]` no `PipelineTimeline`. Isso era "compactação por escala", mas perde fidelidade (ícones esticados, ring difícil de ler). A solução correta é uma prop semântica `variant` no próprio `PipelineTimeline` que controla:
- `compact`: ícones menores (h-5 w-5), labels menores (text-[10px]), pulses sutis, sem ring (ou ring fino).
- `default`: ícones h-6 w-6, labels text-[12px], pulse + ring-2 forte, layout full-width com gap maior.

O `PhaseStation.tsx` recebe a mesma prop `variant` e renderiza apropriadamente. `phase-palette.ts` exporta `PHASE_COLORS` revisto:

```ts
analyze: { ..., bg: "bg-sky-500/15",     text: "text-sky-400" }
plan:    { ..., bg: "bg-violet-500/15",  text: "text-violet-400" }
execute: { ..., bg: "bg-green-500/20",   text: "text-green-400",   ring: "ring-green-500/50" }   // NEW
review:  { ..., bg: "bg-amber-500/15",   text: "text-amber-400" }                                 // CHANGED from teal
qa:      { ..., bg: "bg-emerald-500/15", text: "text-emerald-400" }
close:   { ..., bg: "bg-slate-500/15",   text: "text-slate-400" }
```

EXECUTE recebe `/20` (mais saturado) e ring mais forte (`/50`) para destacar como pedido. REVIEW vira amber porque dois verdes seguidos (review=teal + qa=emerald) eram confusos.

Subtitle redundante: o `SpecDetailDashboard` provavelmente renderiza algo como `<PipelineTimeline ... /> <p>{spec}</p>` que aparece como o slug abaixo do Analyze. Esse `<p>{spec}</p>` some — o slug já vive no `<h2>` do header acima.

## Arquivos

```
apps/dashboard/src/lib/phase-palette.ts                          — execute=green + review=amber
apps/dashboard/src/components/telemetry/PipelineTimeline.tsx     — prop variant + layout full-width default
apps/dashboard/src/components/telemetry/PhaseStation.tsx         — aceita variant, tamanhos condicionais
apps/dashboard/src/components/specs/SpecCard.tsx                 — remove MiniTimeline local; usa <PipelineTimeline variant="compact">
apps/dashboard/src/components/specs/SpecDetailDashboard.tsx      — usa <PipelineTimeline variant="default">; remove subtitle redundante
```

## Tarefas

- [ ] **phase-palette.ts** — alterar `execute`:
  ```ts
  execute: { bg: "bg-green-500/20",   text: "text-green-400",   border: "border-green-500/40",   ring: "ring-green-500/50" }
  ```
  Alterar `review` para amber:
  ```ts
  review:  { bg: "bg-amber-500/15",   text: "text-amber-400",   border: "border-amber-500/30",   ring: "ring-amber-500/40" }
  ```
- [ ] **PipelineTimeline.tsx** — adicionar prop `variant?: "compact" | "default"` (default `"default"`). Passar pra cada `<PhaseStation>`. Em `compact`, container fica `gap-1` e `text-[10px]`; em `default`, container fica `gap-3` e `text-[12px]`. Container externo: `flex items-center justify-between w-full` em ambos os modos (full-width sempre). Em `compact` o container pode ter `max-w-[300px]` se necessário para caber no card; documentar.
- [ ] **PhaseStation.tsx** — receber prop `variant?: "compact" | "default"`. Tamanhos por variant:
  - `compact`: `<div className="h-7 w-7 rounded-full ...">` ícone `h-3.5 w-3.5`, label `text-[10px]`. Conector linha `h-px`. Active ring `ring-1`.
  - `default`: `<div className="h-10 w-10 rounded-full ...">` ícone `h-5 w-5`, label `text-[12px]`. Conector linha `h-0.5`. Active ring `ring-2`.
- [ ] **SpecCard.tsx** — deletar a função local `MiniTimeline`. No JSX, substituir `<MiniTimeline card={data} />` por:
  ```tsx
  <PipelineTimeline
    pipeline={{ spec: data.spec, currentPhase: data.phase, phasesCompleted }}
    variant="compact"
  />
  ```
  (mesmo cálculo de `phasesCompleted` que existia dentro do `MiniTimeline`). Manter o fallback de "sem eventos" — pode ser uma checagem no próprio `PipelineTimeline` quando `currentPhase === ""` (renderiza o `-- sem eventos --` placeholder).
- [ ] **SpecDetailDashboard.tsx** — encontrar o uso do `PipelineTimeline` (já existe). Adicionar `variant="default"`. Confirmar que renderiza no container `w-full` (não dentro de algo com `max-w-` apertado). Procurar e DELETAR o `<p>{spec}</p>` ou `<span>{specName}</span>` que aparece logo embaixo do timeline — esse é o subtitle redundante visto no screenshot 3.
- [ ] Build: `pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W1-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W1-2: `MiniTimeline` removido de `SpecCard.tsx` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(/function\s+MiniTimeline|MiniTimeline\s*=/.test(s)?1:0)"`
- [ ] AC-W1-3: `PipelineTimeline` aceita variant — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/telemetry/PipelineTimeline.tsx','utf8');process.exit(/variant\??:\s*['\"]compact['\"]\\s*\\|\\s*['\"]default['\"]/.test(s)||/variant\??:.*['\"]compact['\"]/.test(s)?0:1)"`
- [ ] AC-W1-4: EXECUTE color é verde — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/lib/phase-palette.ts','utf8');const m=s.match(/execute:\\s*\\{[^}]+\\}/);process.exit(m && /green/.test(m[0])?0:1)"`

## Limites

- `apps/dashboard/src/components/telemetry/PipelineTimeline.tsx`
- `apps/dashboard/src/components/telemetry/PhaseStation.tsx`
- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx`
- `apps/dashboard/src/lib/phase-palette.ts`

## Network

- Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
- Paraleliza com [[wave-2-ui]] (zero overlap de arquivos)
