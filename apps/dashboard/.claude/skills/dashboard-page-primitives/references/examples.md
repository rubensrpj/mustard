<!-- mustard:generated at:2026-05-19 role:ui -->
# Examples — dashboard-page-primitives

## KPICard — stat card with accent stripe

```tsx
// src/components/page/KPICard.tsx
export type KPIAccent = "emerald" | "amber" | "rose" | "indigo" | "violet" | "sky" | "zinc";

export interface KPICardProps {
  label: string;
  value: ReactNode;
  hint?: string;
  accent?: KPIAccent;
  tooltip?: string;
  valueClassName?: string;
  className?: string;
}

// Usage in a KPI ribbon:
<div className="grid grid-cols-4 gap-3">
  <KPICard label="Active Specs"   value={counters.activeSpecs}  accent="indigo"  />
  <KPICard label="Executing"      value={counters.executing}    accent="emerald" />
  <KPICard label="Completed (7d)" value={counters.completed7d}  accent="zinc"    />
  <KPICard label="Events Today"   value={counters.eventsToday}  accent="sky"     />
</div>
```

## PhaseChip — pipeline phase badge

```tsx
// src/components/page/PhaseChip.tsx
import { phaseTheme } from "@/lib/phaseTheme";

// phaseTheme returns { text, bg, border, stripe, label, detail }
// PhaseChip applies them as a Notion-style tag chip:
<span className={cn("inline-flex items-center rounded-sm font-medium border",
  "px-2 py-0.5 text-[11px]", t.text, t.bg, t.border)}>
  {phase}
</span>

// Usage:
<PhaseChip phase={spec.phase} />
<PhaseChip phase="EXECUTE" size="sm" />
```

## EmptyState — three-variant state card

```tsx
// src/components/page/EmptyState.tsx
// variants: "info" | "warning" | "error"
<EmptyState
  title="No specs yet"
  description="Run /mustard:feature to start a pipeline."
  variant="info"
/>

<EmptyState
  title="Build failed"
  description={errorMessage}
  variant="error"
  right={<Button size="sm" onClick={retry}>Retry</Button>}
/>
```

## DataCard — table wrapper

```tsx
// src/components/page/DataCard.tsx
// For tables — leave padded=false, table cells own the padding:
<DataCard>
  <table className="w-full text-sm">
    <tbody>...</tbody>
  </table>
</DataCard>

// For free-form content — padded=true:
<DataCard padded>
  <p className="text-muted-foreground">Nothing to display.</p>
</DataCard>
```

## Barrel — index.ts

```ts
// src/components/page/index.ts
export { PageHeader, type PageHeaderProps } from "./PageHeader";
export { SectionHeader, type SectionHeaderProps } from "./SectionHeader";
export { KPICard, type KPICardProps, type KPIAccent } from "./KPICard";
export { EmptyState, type EmptyStateProps, type EmptyVariant } from "./EmptyState";
export { DataCard, type DataCardProps } from "./DataCard";
export { PhaseChip, type PhaseChipProps } from "./PhaseChip";
export { EventChip, type EventChipProps } from "./EventChip";
export { AcBreakdown, type AcBreakdownProps } from "./AcBreakdown";
export { WaveRowLabel, type WaveRowLabelProps } from "./WaveRowLabel";
export { CollapsibleGroup, type CollapsibleGroupProps } from "./CollapsibleGroup";
```
