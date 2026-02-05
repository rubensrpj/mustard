# Entity Registry Specification v2.1

> Compact format for entity mapping between layers.

---

## 1. Overview

The Entity Registry (`.claude/entity-registry.json`) maps all project entities, their paths, and metadata. Version 2.1 uses a compact format with encoded flags.

---

## 2. File Structure

```json
{
  "_m": { },    // Metadata
  "_k": { },    // Key definitions (legend)
  "_p": { },    // Path patterns (templates)
  "_i": { },    // Integration points
  "e": { }      // Entities
}
```

---

## 3. Section _m (Metadata)

```json
"_m": {
  "v": "2.1",                        // Format version
  "n": 35,                           // Total entity count
  "at": "2026-02-05T03:53:51.261Z"   // Update date
}
```

---

## 4. Section _k (Key Definitions)

Legend to interpret entity flags:

```json
"_k": {
  "1": "all defaults (simple, required, no seed, DLBF)",
  "g": "tenancy=global",
  "seed": "hasSeed=true",
  "cplx": "type=complex",
  "sub": "parentEntity (type=sub-entity)",
  "subs": "relatedEntities",
  "db": "dbKebab override",
  "x": "exists: D=db L=libs B=backend F=frontend",
  "fe": "frontend partial: P=pages S=schemas H=hooks A=api"
}
```

### Flag Meanings

| Flag | Description |
|------|-------------|
| `1` | Simple entity with all defaults |
| `g` | Global entity (no tenant_id) |
| `seed` | Has seed data |
| `cplx` | Complex entity (with sub-entities or special logic) |
| `sub` | Sub-entity (value is parent name) |
| `subs` | List of related sub-entities |
| `db` | Override of kebab-case name for database |
| `x` | Layers where entity exists (D, L, B, F) |
| `fe` | Specific frontend components (P, S, H, A) |

---

## 5. Section _p (Path Patterns)

Path templates for each layer:

```json
"_p": {
  "db": "{DatabaseProject}/src/db/schema/{kebab}.ts",
  "lib": "{LibsProject}/{LibsProject}.Models/Entities/{Pascal}.cs",
  "enum": "{LibsProject}/{LibsProject}.Models/Enums/{EnumName}.cs",
  "be": {
    "root": "{BackendProject}/{BackendProject}/Modules/v1/{PascalPlural}/",
    "ep": "01-EndPoints/{Pascal}EndPoint.cs",
    "svc": "02-Services/{Pascal}Service.cs",
    "repo": "03-Repositories/{Pascal}Repository.cs",
    "gql": "Graphql/{Pascal}QueryResolver.cs",
    "dto": "Model/{Pascal}ResponseDto.cs"
  },
  "fe": {
    "page": "{FrontendProject}/app/(dashboard)/{kebab-plural}/",
    "schema": "{FrontendProject}/lib/shared/schemas/{kebab-plural}/",
    "hook": "{FrontendProject}/lib/client/hooks/use-{kebab-plural}.ts",
    "api": "{FrontendProject}/app/api/{kebab-plural}/"
  }
}
```

### Placeholders

| Placeholder | Example for `PartnerType` |
|-------------|---------------------------|
| `{kebab}` | `partner-type` |
| `{kebab-plural}` | `partner-types` |
| `{Pascal}` | `PartnerType` |
| `{PascalPlural}` | `PartnerTypes` |
| `{EnumName}` | Enum name as-is |

---

## 6. Section _i (Integration Points)

Files that need to be updated when adding entities:

```json
"_i": {
  "db": [
    "{DatabaseProject}/src/db/schema/index.ts",
    "{DatabaseProject}/src/db/schema/enums.ts"
  ],
  "lib": [
    "{LibsProject}/{LibsProject}.DataAccess/Repositories/DbContext.cs"
  ],
  "be": [
    "{BackendProject}/{BackendProject}/Infra/Mapster/AppMapConfig.cs"
  ],
  "fe": [
    "{FrontendProject}/lib/client/hooks/base-entity-hooks.ts"
  ]
}
```

---

## 7. Section e (Entities)

Entity mapping with metadata:

### Compact Format (value = 1)

```json
"Company": 1
```

Means: simple entity, all defaults applied (full DLBF).

### Expanded Format

```json
"Order": {
  "cplx": 1,
  "subs": ["OrderItem", "OrderLog"],
  "enums": ["OrderStatus", "OrderType"]
}
```

```json
"OrderItem": {
  "sub": "Order",
  "x": "DL"
}
```

```json
"Apikey": {
  "g": 1,
  "db": "api-keys"
}
```

---

## 8. Entity Examples

### Simple Entity

```json
"Product": 1
```

- Exists in all layers (D, L, B, F)
- No seed, no sub-entities
- Name follows pattern (products, Product, etc.)

### Global Entity with Seed

```json
"Application": {
  "g": 1,
  "seed": 1
}
```

- Global (no tenant_id)
- Has seed data

### Complex Entity

```json
"Customer": {
  "cplx": 1,
  "subs": ["CustomerAddress", "CustomerContact", "CustomerDocument"],
  "enums": ["CustomerStatus"]
}
```

- Complex entity
- 3 related sub-entities
- Specific enums

### Sub-entity

```json
"CustomerAddress": {
  "sub": "Customer",
  "x": "L"
}
```

- Is a sub-entity of Customer
- Exists only in Libs (L)

### Partial Frontend Entity

```json
"Policy": {
  "x": "DLB",
  "fe": "HA"
}
```

- Exists in DB, Libs, Backend (not full frontend)
- In frontend: only Hooks (H) and API (A)

---

## 9. Path Derivation

To find files for an entity:

```javascript
function getPath(entity, layer, type) {
  const pattern = _p[layer][type] || _p[layer];
  const data = e[entity];

  // Apply overrides
  const kebab = data.db || toKebab(entity);

  return pattern
    .replace('{kebab}', kebab)
    .replace('{kebab-plural}', pluralize(kebab))
    .replace('{Pascal}', entity)
    .replace('{PascalPlural}', pluralize(entity));
}

// Example: Product backend service
getPath('Product', 'be', 'svc')
// → "{BackendProject}/{BackendProject}/Modules/v1/Products/02-Services/ProductService.cs"
```

---

## 10. Rule L6 - Sync Registry

After creating/modifying entities, update the registry:

```bash
/mtd-sync-registry
```

### When to Sync

- New entity created
- Entity renamed
- Sub-entity added
- Enum added
- Change to existing layers

---

## 11. Validation

The registry is valid when:

1. `_m.v` is "2.1"
2. `_m.n` matches the number of entities in `e`
3. All `sub` point to existing entities
4. All `subs` list existing entities
5. Flags `x` and `fe` use only valid letters

---

## See Also

- [enforcement.md](./enforcement.md) - Rule L6
- [naming-conventions.md](./naming-conventions.md) - Naming patterns
