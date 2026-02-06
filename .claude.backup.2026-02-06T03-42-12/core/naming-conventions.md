# Naming Conventions

## Detected Conventions

| Type | Pattern |
|------|---------|
| Classes | PascalCase |
| Files | {"ts":"camelCase","tsx":"camelCase"} |
| Folders | plural |

## General Rules

```
Entities:     PascalCase singular     → Contract, Person, Company
DB Tables:    snake_case plural       → contracts, people, companies
Endpoints:    /api/kebab-case         → /api/contracts, /api/people
Components:   PascalCase.tsx          → ContractForm.tsx
Hooks:        use + camelCase         → useContracts.ts
```
