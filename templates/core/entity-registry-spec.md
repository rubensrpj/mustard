# Entity Registry Specification v3.1

> Catalog of entities, relationships, reference patterns, and enums.

---

## 1. Overview

The Entity Registry (`.claude/entity-registry.json`) is a catalog that answers:

- **What entities exist?** → `e` section
- **What are their relationships?** → `sub` and `refs` fields
- **Which entity should I use as reference?** → `_patterns` section
- **What are valid enum values?** → `_enums` section

**Philosophy:** The registry saves token usage by providing quick answers to common questions without needing to search the codebase.

---

## 2. File Structure

```json
{
  "_meta": { },      // Metadata
  "_patterns": { },  // Reference entities for each pattern type
  "_enums": { },     // Enum values catalog
  "e": { }           // Entities with relationships
}
```

---

## 3. Section _meta

```json
"_meta": {
  "version": "3.1",
  "generated": "2026-02-06",
  "tool": "mustard-cli"
}
```

---

## 4. Section _patterns

Reference entities to use as templates when implementing new features:

```json
"_patterns": {
  "simple": "Bank",
  "withTabs": "Partner",
  "withSubItems": "SalesPlan",
  "withSeed": "PartnerType",
  "withApproval": "Contract"
}
```

### Pattern Types

| Pattern | Use Case |
|---------|----------|
| `simple` | Basic CRUD entity, lookup tables |
| `withTabs` | Entity with tab navigation in UI |
| `withSubItems` | Entity with child/line items |
| `withSeed` | Entity requiring initial seed data |
| `withApproval` | Entity with workflow/status transitions |

### Usage

When starting a new feature:

```text
"I need to create an Invoice entity with line items"
→ Check _patterns.withSubItems → "SalesPlan"
→ Use SalesPlan as reference implementation
```

---

## 5. Section _enums

Catalog of enum types and their valid values:

```json
"_enums": {
  "ContractStatus": ["DRAFT", "PENDING", "ACTIVE", "CANCELLED"],
  "PricingMode": ["CALCULATED", "FIXED"],
  "RecurrencePeriod": ["DEFAULT", "MONTHLY", "QUARTERLY", "YEARLY"]
}
```

### Usage

Before hardcoding values:

```text
"What statuses can a Contract have?"
→ Check _enums.ContractStatus
→ ["DRAFT", "PENDING", "ACTIVE", "CANCELLED"]
```

---

## 6. Section e (Entities)

Entities with their relationships:

```json
"e": {
  "Contract": {
    "sub": ["ContractItem", "ContractLog"],
    "refs": ["Partner", "PaymentMethod", "SalesPlan", "User"]
  },
  "Partner": {
    "sub": ["PartnerAddress", "PartnerContact", "PartnerDocument"]
  },
  "Bank": {}
}
```

### Entity Fields

| Field | Type | Description |
|-------|------|-------------|
| `sub` | `string[]` | Child/sub-entities (owned by this entity) |
| `refs` | `string[]` | FK references to other entities |

### Relationships

- **sub** = "has many" / "owns" relationship (ContractItem belongs to Contract)
- **refs** = "references" / "belongs to" relationship (Contract references Partner)

---

## 7. Complete Example

```json
{
  "_meta": {
    "version": "3.1",
    "generated": "2026-02-06",
    "tool": "mustard-cli"
  },
  "_patterns": {
    "simple": "Bank",
    "withTabs": "Partner",
    "withSubItems": "SalesPlan",
    "withSeed": "PartnerType",
    "withApproval": "Contract"
  },
  "_enums": {
    "ContractStatus": ["DRAFT", "PENDING", "ACTIVE", "CANCELLED"],
    "PricingMode": ["CALCULATED", "FIXED"],
    "PartnerType": ["CUSTOMER", "SUPPLIER", "BOTH"]
  },
  "e": {
    "Contract": {
      "sub": ["ContractItem", "ContractLog"],
      "refs": ["Partner", "PaymentMethod", "SalesPlan", "User"]
    },
    "Partner": {
      "sub": ["PartnerAddress", "PartnerContact", "PartnerDocument"]
    },
    "SalesPlan": {
      "sub": ["SalesPlanItem", "SalesPlanChannel"]
    },
    "Bank": {},
    "PaymentMethod": {},
    "User": {
      "sub": ["UserSalesChannelOverride"]
    }
  }
}
```

---

## 8. When to Sync

Run `/sync-registry` after:

- New entity created
- Entity renamed
- Sub-entity or relationship added
- Enum values changed

---

## See Also

- [enforcement.md](./enforcement.md)
- [naming-conventions.md](./naming-conventions.md)
