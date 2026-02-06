# Naming Conventions Prompt

> Central reference for naming conventions (L3).
> Other prompts (backend, frontend, database) should reference this file.

## Rule L3

> **L3 - Naming:** All implementations MUST follow the project naming conventions.

## Detected Conventions

| Type | Pattern |
|------|---------|
| Classes | PascalCase |
| Files | {"ts":"camelCase","tsx":"camelCase"} |
| Folders | plural |

## Quick Reference

| Type | Pattern | Example |
|------|---------|---------|
| Entity/Class | PascalCase singular | `Contract`, `Person` |
| DB Table | snake_case plural | `contracts`, `people` |
| DB Column | snake_case | `created_at`, `tenant_id` |
| Foreign Key | {table}_id | `contract_id` |
| Index | idx_{table}_{cols} | `idx_contracts_tenant` |
| Endpoint/Route | kebab-case | `/api/contracts` |
| Component | PascalCase | `ContractForm` |
| Hook | use + camelCase | `useContracts` |
| Service | PascalCase + Service | `ContractService` |

## Entities / Classes

```
✅ Contract
✅ Person
✅ InvoiceItem

❌ Contracts (not plural)
❌ contract (not lowercase)
❌ invoice_item (not snake_case)
```

## Database Tables

```
✅ contracts
✅ people
✅ invoice_items

❌ Contract (not singular)
❌ InvoiceItems (not PascalCase)
```

## Endpoints / Routes

```
✅ /api/contracts
✅ /api/contracts/{id}
✅ /api/invoice-items

❌ /api/Contracts
❌ /api/contract
❌ /api/invoiceItems
```

## Hooks (Frontend)

```
✅ useContract
✅ useContracts
✅ useContractMutations

❌ UseContract
❌ use-contract
❌ contractHook
```

## Abbreviations

**Avoid** abbreviations in names:

- ✅ Configuration, ❌ Config
- ✅ Application, ❌ App
- ✅ Repository, ❌ Repo

**Accepted exceptions:** `Id`, `Dto`, `Api`

## Validation Checklist (L3)

```
□ Class names in PascalCase singular
□ Table names in snake_case plural
□ Column names in snake_case
□ Foreign keys with _id suffix
□ Endpoints in kebab-case
□ Hooks with use prefix
□ No abbreviations (except Id, Dto, Api)
```

## See Also

- [enforcement.md](../core/enforcement.md) - Rule L3
- [backend.md](./backend.md) - Backend patterns
- [frontend.md](./frontend.md) - Frontend patterns
- [database.md](./database.md) - Database patterns
