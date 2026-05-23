# missing-primitives — Wave 5 lookalike

### Stage: Plan
### Outcome: Active
### Lang: en

## Files

- apps/dashboard/src/pages/Demo.tsx

## Tasks

```tsx
import { EditorialBand, KpiValue, DeltaText } from "@/components/page";

export function Demo() {
  return (
    <EditorialBand title="Numbers">
      <KpiValue value={42} />
      <DeltaText delta={+0.3} />
    </EditorialBand>
  );
}
```
