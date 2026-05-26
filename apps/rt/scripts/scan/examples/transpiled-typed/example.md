<!-- mustard:generated -->
# Golden example — transpiled, typed

A representative scan output for a subproject that ships through a transpiler
and enforces a static type system at build time (TypeScript, Dart, Kotlin/JS,
Flow). The names below are fictional and deliberately generic — they exist to
show shape, not to import vocabulary.

## What `patterns.md` looks like (≤150 lines)

```
<!-- mustard:generated -->
# Patterns — ui

## components/
- folderPattern: `src/components/**/*.ext`
- samples: `Button.ext`, `Card.ext`, `Dialog.ext`
- memberSuffixes: Props, View

## hooks/
- folderPattern: `src/hooks/*.ext`
- samples: `useUser.ext`, `useDialog.ext`
- memberSuffixes: Hook

## Conventions
- naming: PascalCase (dominant 0.71 of 54 files)
```

## What a SKILL.md looks like (≤60 lines)

```
---
name: component-pattern
description: "Compose UI from typed components. Use when adding a screen, dialog, or interactive widget."
source: scan
---
<!-- mustard:generated -->
## Convention
- Each component lives in its own file; props are a named exported type.
- Side effects live in colocated hooks, not in the component body.

## Real examples in this codebase
- `src/components/Button.ext` — typed `Props`, no side effects.

## References
- See `references/examples.md` for verbatim code.
```

## What `notes.md` looks like (≤80 lines)

```
<!-- mustard:generated -->
- Two `.ext` extensions present (`.x` for source, `.y` for tests).
- The transpiler config is colocated with the manifest at the subproject root.
- No global CSS pattern detected — styling lives inside components.
```
