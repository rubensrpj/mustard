# Naming Conventions

> Common naming conventions across all layers.

## Quick Reference

```
Entities:     PascalCase singular     -> Contract, Person, Company
DB Tables:    snake_case plural       -> contracts, people, companies
Endpoints:    /api/kebab-case         -> /api/contracts, /api/people
Components:   PascalCase.tsx          -> ContractForm.tsx
Hooks:        use + camelCase         -> useContracts.ts
```

## By Layer

### Entity Names

| Type | Pattern | Example |
| ---- | ------- | ------- |
| Entity | PascalCase singular | `Contract` |
| DTO | {Entity}Dto | `ContractDto` |
| Service | I{Entity}Service | `IContractService` |
| Endpoint | {Entity}/{Action} | `Contract/Create` |

### Database Names

| Type | Pattern | Example |
| ---- | ------- | ------- |
| Table | snake_case plural | `contracts` |
| Column | snake_case | `created_at` |
| FK | {table}_id | `contract_id` |
| Index | idx_{table}_{cols} | `idx_contracts_tenant` |

### Frontend Names

| Type | Pattern | Example |
| ---- | ------- | ------- |
| Component | PascalCase | `ContractForm.tsx` |
| Hook | use + camelCase | `useContracts.ts` |
| Page | {entity}/page.tsx | `contracts/page.tsx` |
| Zod Schema | z + Type + Name | `zProductUpSertDto` |
| TS Type | Tz + Schema | `TzProductUpSertDto` |

### Review Validation

| Type | Pattern | Valid |
| ---- | ------- | ----- |
| Entity | PascalCase singular | `Contract` |
| Table | snake_case plural | `contracts` |
| Hook | use + camelCase | `useContracts` |
| Endpoint | kebab-case | `/api/contracts` |
