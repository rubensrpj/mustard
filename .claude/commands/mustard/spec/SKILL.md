---
name: mustard-spec
description: Use when the user wants to approve a planned spec or continue an in-progress spec. Single picker — delegates to mustard-rt run active-specs and resume-bootstrap.
source: manual
---
<!-- mustard:generated -->
# /mustard:spec — Unified Spec Picker

`/mustard:spec [letra[r]]` — replaces `/approve` (PLAN) and `/resume` (EXEC). Letter = approve (PLAN) or continue (EXEC). Letter + `r` = approve + execute inline same session.

## Action

### 1. Auto-sync + render (mandatory every invocation, even with 1 spec)

```bash
rtk mustard-rt run active-specs --format table
```

Print stdout verbatim, then the **Siglas + Modo de seleção** block below literally.

**Siglas** — `#` letter (a-z), `Esc` Scope (`lt` light / `fl` full / `-`), `Prog` waves done/total. Stage `PLAN` planejar / `EXEC` executar. Status `TF` tactical-fix, `TF→{alias}` TF parent, `W{N}` wave N, `BLOCK` blocked, `em exec` dispatched, `-` none.

**Modo de seleção** — `a-z` act on row (PLAN approve / EXEC continue). `a-z+r` (e.g. `ar`) approve + execute inline (EXEC ignores `r`). Anything else → error + re-render.

### 2. Parse + route via `resume-bootstrap`

`^[a-z]$` act-only; `^[a-z]r$` act+execute; else → *"Letra inválida."* + re-render.

```bash
rtk mustard-rt run resume-bootstrap --spec {specName} --json
```

Parse: `stage`, `mode`, `operationalSpecPath`, `currentWave`, `lastDispatchFailure`, `needsDiff`, `needsContextSlice`.

`Plan` + no suffix → `../../../refs/spec/approve-only-flow.md`. `Plan` + `r` → `approve-only-flow.md § Branch --resume`. `Execute`/`Analyze`/`QaReview`/`Close` → `../../../refs/spec/resume-flow.md`; EXEC ignores `r`.

#### EXEC branch — `dispatch-plan` relay

Routing/order is decided by Rust, not the LLM. Get the ordered dispatch array:

```bash
rtk mustard-rt run dispatch-plan --spec {specName}
```

For each item `{wave, role, subproject, depends_on, level, prompt_cmd}`: run `prompt_cmd` (a ready `agent-prompt-render` call) and pass its **stdout** verbatim as the Task `prompt`. Items sharing a `level` are independent → dispatch them in **one** message. `subagent_type` = `{subproject-name}-impl` when that rich agent exists, else `general-purpose`. NEVER hand-craft prompts or interpret `wave-plan.md` by hand. Post-dispatch → `../../../refs/spec/resume-flow.md`.

### 4. Edge cases

0 specs → *"Nenhuma spec ativa."*. >26 → first 26 + *"(N adicionais)"*. Pre-selected (`/mustard:spec ar`) → skip re-render.

## INVIOLABLE RULES

- Table + Siglas + Modo blocks are mandatory + literal every invocation.
- NEVER hand-craft agent prompts — always `agent-prompt-render` (delivered as each `dispatch-plan` item's `prompt_cmd`).
- NEVER read `wave-plan.md` or decide wave order by hand — `dispatch-plan` owns routing; the LLM only relays.
- NEVER reimplement `continued` vs `reanalyzed` — `resume-bootstrap` is the source of truth.
