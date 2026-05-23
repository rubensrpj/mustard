# self-created — primitives declared in this spec

### Stage: Plan
### Outcome: Active
### Lang: en

## Files

- apps/rt/tests/fixtures/dependency_precheck/_fake/LocalPrimitiveA.tsx
- apps/rt/tests/fixtures/dependency_precheck/_fake/LocalPrimitiveB.tsx

## Tasks

```tsx
import { LocalPrimitiveA } from "./_fake/LocalPrimitiveA";
import { LocalPrimitiveB } from "./_fake/LocalPrimitiveB";

export function Demo() {
  return (
    <LocalPrimitiveA>
      <LocalPrimitiveB />
    </LocalPrimitiveA>
  );
}
```
