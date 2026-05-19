# Page primitives — design system

Cross-route visual primitives. Every dashboard page composes these instead of
inlining its own card/header/chip styles. **Goal**: same phase reads as the
same color, same event reads as the same color, same KPI looks like the same
KPI on every page.

## Composition pattern

```tsx
import {
  PageHeader, SectionHeader, KPICard, EmptyState,
  DataCard, PhaseChip, EventChip, AcBreakdown,
  WaveRowLabel, CollapsibleGroup,
} from "@/components/page";

export function MyPage() {
  return (
    <div className="flex flex-col gap-6 w-full">
      <PageHeader
        breadcrumb={["Mustard", "Minha Página", { label: workspace, mono: true }]}
        title="Minha Página"
        subtitle={workspace}
        description="Resumo do que esta página mostra."
      />

      {/* KPI ribbon: grid-cols-1 sm:grid-cols-3 gap-3 */}
      <section className="grid grid-cols-1 sm:grid-cols-3 gap-3 w-full">
        <KPICard label="Métrica A" value="87%" accent="emerald" hint="..." />
        <KPICard label="Métrica B" value="3" accent="amber" hint="..." />
        <KPICard label="Métrica C" value="120ms" accent="indigo" hint="..." />
      </section>

      {/* Data section */}
      <section className="flex flex-col gap-3 w-full">
        <SectionHeader
          title="Lista de Coisas"
          description="O que cada linha significa."
          right={`${items.length} itens`}
        />
        <DataCard>
          <table className="w-full">{/* ... */}</table>
        </DataCard>
      </section>

      {/* Empty/error states */}
      {error && (
        <EmptyState
          variant="error"
          title="Falha ao carregar"
          description={error.message}
          right={<button onClick={retry}>Retry</button>}
        />
      )}
    </div>
  );
}
```

## Phase + event color system

`src/lib/phaseTheme.ts` is the single source of truth for color-coding:

| Phase | Hue |
|---|---|
| ANALYZE | sky (exploração) |
| PLAN | amber (planejamento) |
| EXECUTE | emerald (ação) |
| QA | violet (verificação) |
| CLOSE | zinc (encerrado) |

Event types use a separate hue family (rose/cyan/fuchsia/lime) so the two
categories never blur. See `EVENT_THEME` in `phaseTheme.ts`.

## Spacing rhythm

- Page-level vertical gap: `gap-6` (24px)
- Section-level vertical gap: `gap-3` (12px) or `gap-5` (20px)
- Inside cards: `gap-1` to `gap-2`
- Page width: always `w-full` — no `max-w-*` on outer containers

## Width

**Never** wrap descriptive text in `max-w-*xl`. Use `leading-relaxed` to keep
long lines readable on wide screens; rely on the natural width of the
content column.

## Responsive grids

Always use progressive breakpoints (`1 → 2 → 3 → 4`), never jump levels:

```tsx
// ✅ Good — passes through sm and md before settling on xl
<div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4 gap-3">

// ❌ Bad — stays 1 column from mobile through tablet, jumps to 4 on xl
<div className="grid grid-cols-1 xl:grid-cols-4 gap-3">
```

## When to add a new primitive

Add a primitive when the same visual pattern appears in **2+ pages** and
encoding it inline would force the pages to drift. One-off ornaments stay
in the page.

Adding one:
1. Create `src/components/page/MyPrimitive.tsx`
2. Export from `src/components/page/index.ts`
3. Document its props with JSDoc
4. (Optional) Add a section above to this README

## Pages using these primitives

- ✅ `Quality.tsx` — full
- 🟡 `Telemetry.tsx` — partial (still inlines some EconomySection patterns)
- 🟡 `PromptEconomy.tsx` — partial
- ⏳ `Activity.tsx` — pending
- ⏳ `Home.tsx` (via WorkspaceDigest) — pending
- ⏳ Others
