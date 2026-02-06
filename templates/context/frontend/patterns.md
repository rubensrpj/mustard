# Frontend Patterns (React/TypeScript)

> Project-specific patterns for React/TypeScript frontend implementation.

## Feature Structure

```
src/features/{entity}/
+-- components/
|   +-- {Entity}Form.tsx
|   +-- {Entity}List.tsx
|   +-- {Entity}Card.tsx
|   +-- {Entity}Table.tsx
+-- hooks/
|   +-- use{Entity}.ts
|   +-- use{Entities}.ts
|   +-- use{Entity}Mutations.ts
+-- pages/
    +-- index.tsx
    +-- [id].tsx
    +-- new.tsx
```

## Zod Schemas

Prefix `z` + Type + Name:

```typescript
// Naming pattern
export const zProductUpSertDto = z.object({
  name: z.string().min(1),
  email: z.string().email(),
});

export const zResponseProduct = z.object({
  id: z.number(),
  uniqueId: z.string().uuid(),
  name: z.string(),
});

export const zGhqlProduct = z.object({
  // Schema for GraphQL data
});
```

| Prefix | Usage |
| ------ | ----- |
| `z{Entity}CreateDto` | Create schema |
| `z{Entity}UpdateDto` | Update schema |
| `z{Entity}UpSertDto` | Create/update schema |
| `zResponse{Entity}` | Response schema |
| `zGhql{Entity}` | GraphQL schema |

## TypeScript Types

Prefix `Tz` + Schema Name (without z):

```typescript
// Derive type from schema
export type TzProductUpSertDto = z.infer<typeof zProductUpSertDto>;
export type TzResponseProduct = z.infer<typeof zResponseProduct>;
export type TzGhqlProduct = z.infer<typeof zGhqlProduct>;
```

**Rule:** `Tz` + Schema without the `z` prefix

## Implementation Checklist

```
[ ] Verify types generated from backend
[ ] Create Zod schemas (z prefix)
[ ] Derive TypeScript types (Tz prefix)
[ ] Create data hooks
[ ] Create form components
[ ] Create list components
[ ] Create pages
[ ] Configure routes
[ ] Test type-check
```
