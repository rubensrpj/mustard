# Naming Conventions Prompt

> **TEMPLATE FILE:** This prompt can be customized for your project.
> You may modify the content, but **do not rename this file**.

> Use as central reference for naming conventions.
> Other prompts (backend, frontend, database) should consult this file.

## Identity

This file defines the **naming patterns** for the project. All implementations should follow these conventions for consistency.

## Quick Reference

| Type | Pattern | Example |
| ---- | ------- | ------- |
| Entity/Class | PascalCase singular | `Contract`, `Person` |
| DB Table | snake_case plural | `contracts`, `people` |
| DB Column | snake_case | `created_at`, `tenant_id` |
| Foreign Key | {table}_id | `contract_id` |
| Index | idx_{table}_{cols} | `idx_contracts_tenant` |
| Endpoint/Route | kebab-case | `/api/contracts` |
| Component | PascalCase | `ContractForm` |
| Hook | use + camelCase | `useContracts` |
| Service | PascalCase + Service | `ContractService` |

## Rule L3

> **L3 - Naming:** Every implementation MUST follow the project's naming conventions.

Before creating any file or entity, consult this document.

---

## Detailed Conventions

### Entities / Classes

```
Contract
Person
InvoiceItem

Contracts (not plural)
contract (not lowercase)
invoice_item (not snake_case)
```

### Database Tables

```
contracts
people
invoice_items

Contract (not singular)
InvoiceItems (not PascalCase)
```

### Columns

```
created_at
tenant_id
invoice_id

createdAt (not camelCase)
TenantId (not PascalCase)
```

### Foreign Keys

```
contract_id
person_id
parent_id

contractId
fk_contract
```

### Indexes

```
idx_contracts_tenant_id
idx_invoice_items_invoice_id

contracts_tenant_idx
IX_Contracts_TenantId
```

### Endpoints / Routes

```
/api/contracts
/api/contracts/{id}
/api/invoice-items

/api/Contracts
/api/contract
/api/invoiceItems
```

### Components (Frontend)

```
ContractForm
ContractList
InvoiceItemCard

contractForm
contract-form
```

### Hooks (Frontend)

```
useContract
useContracts
useContractMutations

UseContract
use-contract
contractHook
```

### Services (Backend)

```
IContractService / ContractService
IInvoiceItemService / InvoiceItemService

ContractSvc
ContractManager
contractService
```

---

## Pluralization

| Singular | Plural |
| -------- | ------ |
| Contract | Contracts |
| Person | People |
| Company | Companies |
| Category | Categories |
| Status | Statuses |
| Address | Addresses |

---

## Abbreviations

**Avoid** abbreviations in names:

```
Configuration
Config

Application
App

Repository
Repo
```

**Accepted exceptions:**

- `Id` (Identifier)
- `Dto` (Data Transfer Object)
- `Api` (Application Programming Interface)

---

## Folders

### Generic Structure

```
src/
+-- {feature}/
|   +-- components/   # UI components
|   +-- hooks/        # Custom hooks
|   +-- services/     # Business logic
|   +-- types/        # Type definitions

schema/
+-- {entity}.ts       # Entity schema
+-- index.ts          # Exports
```

---

## Enums

| Element | Pattern | Example |
| ------- | ------- | ------- |
| Type name | snake_case | `bank_account_type` |
| Values | SCREAMING_SNAKE | `CHECKING`, `SAVINGS` |

---

## Validation Checklist (L3)

Before finalizing an implementation:

```
[ ] Class names in PascalCase singular
[ ] Table names in snake_case plural
[ ] Column names in snake_case
[ ] Foreign keys with _id suffix
[ ] Endpoints in kebab-case
[ ] Hooks with use prefix
[ ] No abbreviations (except Id, Dto, Api)
```

---

## See Also

- [enforcement.md](../core/enforcement.md) - L3 Rule
- [backend.md](./backend.md) - Backend patterns
- [frontend.md](./frontend.md) - Frontend patterns
- [database.md](./database.md) - Database patterns
