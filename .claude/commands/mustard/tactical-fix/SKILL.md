---
name: mustard-tactical-fix
description: "Create a sub-spec linked to a parent spec for a small tactical fix surfaced during REVIEW/QA. Preserves SDD purity — parent spec stays frozen after approve."
source: manual
---
<!-- mustard:generated -->
# /tactical-fix - Sub-Spec for Tactical Fix

## Trigger

`/mustard:tactical-fix <parent> "<descrição>" [--scope touch|light|full]`

- `<parent>` — slug of the parent spec (e.g. `2026-05-20-sdd-domain-finalization`). Lives in `.claude/spec/<parent>/` (flat layout — status is read from the spec header / SQLite projection, not the directory).
- `"<descrição>"` — short natural-language description of the fix. Used to derive the slug and seed the spec body.
- `--scope` — optional; default `light`. `touch` for ≤30 LOC throwaway fixes, `light` for the standard ≤100 LOC tactical fix, `full` only when the fix turned out to need a full PRD.

## Description

Creates a new sub-spec linked to a parent spec for a tactical fix surfaced during REVIEW or QA. Preserves SDD purity: the parent spec stays frozen after approve, and the fix gets its own AC, its own approve gate, and its own CLOSE — but is visually linked back to the parent through the `### Parent:` header and the harness `spec.link` event.

Use this when a REVIEW or QA agent identifies a small adjacent fix that:

- fits in ≤100 LOC,
- does NOT change a public contract (schema, public API, exported types),
- has NO pending design decision,
- introduces NO new dependency.

Anything outside those constraints should become a regular follow-up (legitimate follow-up) or a fresh full-scope spec — not a tactical-fix sub-spec.

## Action

### Step 1 — Derive slug

Suggested slug: `YYYY-MM-DD-<kebab-of-description>` where:

- `YYYY-MM-DD` is the local ISO date.
- `<kebab-of-description>` is built by a simple heuristic: lowercase, strip diacritics, replace non-alphanumeric runs with `-`, trim `-`, cap at ~6 words.

Print the suggested slug and let the user override before creating the directory.

### Step 2 — Create directory

Create `.claude/spec/<slug>/`.

If the directory already exists, abort with a message asking the user to pick a different slug or to delete the existing one.

### Step 3 — Generate `spec.md`

Write `.claude/spec/<slug>/spec.md` with this header:

```text
# Tactical Fix: <descrição>

### Stage: Plan
### Outcome: Active
### Phase: ANALYZE
### Scope: <scope-flag>
### Checkpoint: <ISO timestamp now>
### Lang: <inherited from parent, default en>
### Parent: <parent>
```

Body (skeleton — empty sections that the user fills before `/approve`):

```text
## Contexto

Tactical fix derivado de [[<parent>]].

## Critérios de Aceitação

<!-- 1-3 binary, executable AC, cross-shell — see /feature § Acceptance Criteria — Cross-Shell Pattern -->

## Arquivos

<!-- Paths intentionally touched -->
```

For `Lang=en` use the EN headings (`## Context`, `## Acceptance Criteria`, `## Files`).

### Step 4 — Emit `spec.link` event

```bash
rtk mustard-rt run spec-link --parent <parent> --child <slug> --reason "tactical-fix"
```

This subcommand already exists in `mustard-rt`. It appends a `spec.link` event to the harness store so the dashboard and projections can walk the parent → child lineage. Fail-open: if the call fails (e.g. parent missing on disk), the sub-spec still works standalone — only the visual link is lost.

### Step 5 — Tell the user what to do next

Print, verbatim:

```
Sub-spec created at .claude/spec/<slug>/spec.md
Parent: <parent>
Edit the spec (Contexto, Critérios de Aceitação, Arquivos) and run /mustard:spec, then pick the letter for <slug>, to start the pipeline.
```

Do NOT auto-approve. The sub-spec passes through the normal pipeline (ANALYZE if you want a fresh look, then PLAN, EXECUTE, REVIEW, QA, CLOSE).

## Inviolable

- **Fail-open on parent existence.** If `<parent>` does not exist in `.claude/spec/{active,completed}/`, still create the sub-spec and still emit `spec.link`. The sub-spec is usable standalone; only dashboard navigation is degraded.
- **Never mutate the parent spec.** SDD purity: parent stays frozen after its own approve. The link is one-way (parent ← child via `### Parent:` header + `spec.link` event).
- **One tactical-fix command call creates exactly one sub-spec.** If the user needs N fixes, run `/mustard:tactical-fix` N times.
- **No "light mode" pipeline.** The sub-spec passes through the regular pipeline — same gates, same QA, same CLOSE.

## When NOT to use

| Situation | Use instead |
|---|---|
| Fix > 100 LOC or touches a public contract | Regular `/mustard:feature` (full or light scope, as the signals dictate) |
| Pending design decision the user must call | `AskUserQuestion` inside REVIEW/QA, then a regular spec |
| Mid-EXECUTE refactor that grew naturally | Continue inside the current spec; add a new wave only if the spec PLAN allows it; otherwise close and open a follow-up spec |
| New dependency, new entity, new module | Regular full-scope `/mustard:feature` |

## Examples

```bash
# QA agent flagged a missing null check adjacent to the AC — small surgical fix
/mustard:tactical-fix 2026-05-18-card-render "guard against null entityId in SpecCard"

# REVIEW agent flagged a perf regression in the touched code path
/mustard:tactical-fix 2026-05-19-spec-search "memoize SpecCard filter callback" --scope touch

# Discovery during EXECUTE that the wave-plan didn't cover (≤100 LOC)
/mustard:tactical-fix 2026-05-20-tactical-fix-via-sub-spec "render reason text muted in SpecChildrenTab"
```

## Related

- `pipeline-config.md § Tactical Fix Discovery` — the rule (when REVIEW/QA agents should suggest this command).
- `/mustard:review § Tactical Fix Discovery (advisory)` — how reviews surface candidates.
- `/mustard:qa § Tactical Fix Discovery após QA Pass` — how QA surfaces candidates.
- `mustard-rt run spec-link` — the underlying event emitter (already exists, no changes needed).
