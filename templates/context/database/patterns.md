# Database Patterns (Drizzle ORM)

> Project-specific patterns for Drizzle ORM database implementation.

## Schema Structure

```
src/schema/
+-- {entity}.ts       # Entity schema
+-- relations.ts      # Relationships
+-- enums.ts          # All centralized pgEnums
+-- index.ts          # Exports
```

## Standard Columns (Drizzle)

Every table MUST have these columns:

```typescript
// Required columns in EVERY table
id: uuid('id').primaryKey().defaultRandom()
tenant_id: uuid('tenant_id').notNull()           // L3: Multi-tenancy
created_at: timestamp('created_at').notNull().defaultNow()
updated_at: timestamp('updated_at')
deleted_at: timestamp('deleted_at')              // L3: Soft delete
```

## Multi-tenancy (L3)

> **CRITICAL:** Every table MUST have `tenant_id` for data isolation.

```typescript
// CORRECT - tenant_id required
tenant_id: uuid('tenant_id').notNull()

// CORRECT - Index for performance in tenant-filtered queries
.index('idx_contracts_tenant', contracts.tenantId)
```

**Why?**

- Data isolation between clients
- Performance in filtered queries
- Security against data leakage

## Soft Delete (L3)

> **CRITICAL:** NEVER use hard delete. Every table MUST have `deleted_at`.

```typescript
// CORRECT - Soft delete
deleted_at: timestamp('deleted_at')

// CORRECT - In queries, exclude deleted
.where(isNull(contracts.deletedAt))

// CORRECT - To "delete"
.set({ deletedAt: new Date() })
```

**Why?**

- Audit and compliance
- Data recovery
- Referential integrity

## L3 Checklist

```
[ ] tenant_id present and NOT NULL
[ ] deleted_at present (nullable)
[ ] Index on tenant_id created
[ ] Queries filter by tenant
[ ] Queries exclude deleted (isNull(deleted_at))
[ ] No physical DELETE in code
```

## Enums (pgEnum)

```typescript
// Pattern: camelCase for variable, snake_case for type, SCREAMING_SNAKE for values
export const bankAccountType = pgEnum('bank_account_type', [
  'CHECKING',
  'SAVINGS',
  'INVESTMENT',
]);

// Usage in table
accountType: bankAccountType('account_type').notNull()
```

| Element | Pattern | Example |
| ------- | ------- | ------- |
| Variable name | `camelCase` | `bankAccountType` |
| Type string | `snake_case` | `'bank_account_type'` |
| Values | `SCREAMING_SNAKE` | `'CHECKING'`, `'SAVINGS'` |

## Enum Checklist

```
[ ] Define enum in enums.ts
[ ] Export in index.ts
[ ] Use in table with snake_case column
[ ] Update Entity Registry if needed
```

## SQL Reference (Recommended)

For reference, the standard columns in SQL:

```sql
id UUID PRIMARY KEY DEFAULT gen_random_uuid()
tenant_id UUID NOT NULL          -- If multi-tenant
created_at TIMESTAMP NOT NULL DEFAULT NOW()
updated_at TIMESTAMP
deleted_at TIMESTAMP             -- If soft delete
```
