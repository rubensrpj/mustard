<!-- mustard:generated -->
# Golden example — compiled, strongly-typed

A representative scan output for a subproject whose stack compiles to native or
JVM bytecode and enforces types at compile time (e.g. Rust crates, Go modules,
.NET projects, Java/Kotlin services). The names below are fictional and
deliberately generic — they exist to show shape, not to import vocabulary.

## What `patterns.md` looks like (≤150 lines)

```
<!-- mustard:generated -->
# Patterns — service

## handlers/
- folderPattern: `src/handlers/**/*.ext`
- samples: `read_user.ext`, `write_user.ext`, `delete_user.ext`
- memberSuffixes: Handler, Request, Response

## stores/
- folderPattern: `src/stores/*.ext`
- samples: `user_store.ext`, `audit_store.ext`
- memberSuffixes: Store, Repository

## Conventions
- naming: snake_case (dominant 0.78 of 42 files)
```

## What a SKILL.md looks like (≤60 lines)

```
---
name: store-pattern
description: "Persist domain objects via the store layer. Use when adding a CRUD entity or extending an existing store."
source: scan
---
<!-- mustard:generated -->
## Convention
- Each store owns one aggregate; no cross-store joins.
- Public methods return the domain type or a typed error.

## Real examples in this codebase
- `src/stores/user_store.ext` — owns the `User` aggregate.

## References
- See `references/examples.md` for verbatim code.
```

## What `notes.md` looks like (≤80 lines)

```
<!-- mustard:generated -->
- One cluster (`migrations`) had 2 files — skipped below threshold.
- Build/test commands captured from the manifest; no extra tooling detected.
- No `routes/` folder — request entry point is `handlers/`.
```
