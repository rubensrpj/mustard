<!-- mustard:generated -->
# Golden example — dynamic, scripting

A representative scan output for a subproject whose stack is interpreted or
loosely-typed (Python, Ruby, PHP, plain JavaScript). The names below are
fictional and deliberately generic — they exist to show shape, not to import
vocabulary.

## What `patterns.md` looks like (≤150 lines)

```
<!-- mustard:generated -->
# Patterns — service

## views/
- folderPattern: `app/views/**/*.ext`
- samples: `index.ext`, `show.ext`, `edit.ext`
- memberSuffixes: View, Handler

## models/
- folderPattern: `app/models/*.ext`
- samples: `user.ext`, `post.ext`
- memberSuffixes: Model

## Conventions
- naming: snake_case (dominant 0.82 of 38 files)
```

## What a SKILL.md looks like (≤60 lines)

```
---
name: view-pattern
description: "Render a page response. Use when adding a new screen or modifying an existing render path."
source: scan
---
<!-- mustard:generated -->
## Convention
- One view function per URL path; no class hierarchies.
- Returns either a rendered template or a serialised payload.

## Real examples in this codebase
- `app/views/index.ext` — landing-page render.

## References
- See `references/examples.md` for verbatim code.
```

## What `notes.md` looks like (≤80 lines)

```
<!-- mustard:generated -->
- Two clusters (`tasks`, `mailers`) were detected but had only 2 files each.
- No type-checker configuration detected — patterns derived from runtime shape.
- Lockfile uses a fully-pinned graph; bumps require regenerating the lock.
```
