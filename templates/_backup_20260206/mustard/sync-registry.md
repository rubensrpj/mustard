# /sync-registry - Update Entity Registry

> Scans the project and updates entity-registry.json with entities, relationships, patterns, and enums.

## Usage

```bash
/sync-registry
```

## When to Use

- New entity created
- Entity renamed
- Sub-entity or relationship added
- Enum values changed

## Action

### Step 1: Discover Entities

Use Task(Explore) to find all entities:

- Database tables/models (source of truth)
- Backend modules with entity logic

### Step 2: Map Relationships

For each entity, identify:

- **sub**: Child/sub-entities (e.g., ContractItem, ContractLog)
- **refs**: FK references to other entities (e.g., Partner, PaymentMethod)

### Step 3: Identify Reference Patterns

Find one good example entity for each pattern type:

| Pattern | Description | Example Use |
|---------|-------------|-------------|
| `simple` | Basic CRUD entity | When creating simple lookup tables |
| `withTabs` | Entity with tab navigation | When UI needs multiple sections |
| `withSubItems` | Entity with child items | When entity has line items |
| `withSeed` | Entity with seed data | When entity needs initial data |
| `withApproval` | Entity with workflow | When entity has status transitions |

### Step 4: Catalog Enums

List all enum types with their values for quick reference.

### Step 5: Update Registry

Update `.claude/entity-registry.json`:

```json
{
  "_meta": { "version": "3.1", "generated": "<date>", "tool": "mustard-cli" },
  "_patterns": {
    "simple": "Bank",
    "withTabs": "Partner",
    "withSubItems": "SalesPlan",
    "withSeed": "PartnerType",
    "withApproval": "Contract"
  },
  "_enums": {
    "ContractStatus": ["DRAFT", "PENDING", "ACTIVE", "CANCELLED"],
    "PricingMode": ["CALCULATED", "FIXED"]
  },
  "e": {
    "Contract": {
      "sub": ["ContractItem", "ContractLog"],
      "refs": ["Partner", "PaymentMethod", "SalesPlan", "User"]
    },
    "Partner": {
      "sub": ["PartnerAddress", "PartnerContact"]
    },
    "Bank": {}
  }
}
```

## Usage Tips

### Finding Reference Entity

When implementing a new feature, check `_patterns` first:

```
"I need to create an entity with approval workflow"
→ Use Contract as reference (_patterns.withApproval)
```

### Checking Enum Values

Before hardcoding status values, check `_enums`:

```
"What are the valid ContractStatus values?"
→ Check _enums.ContractStatus
```

### Understanding Relationships

Before modifying an entity, check its refs:

```
"Contract refs Partner, PaymentMethod, SalesPlan, User"
→ Changes may impact these related entities
```

## See Also

- [entity-registry-spec.md](../../core/entity-registry-spec.md)
