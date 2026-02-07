# /validate - Build Validation

## Trigger

`/validate`

## Description

Runs build and type-check validations.

## Actions

- `dotnet build` - Verifies .NET compiles
- `npm run typecheck` - Verifies TypeScript types
- `npm run lint` - Verifies linting (if available)

## Result

- ✅ **Success** - Project compiles and passes type-check
- ❌ **Failure** - Lists errors found
