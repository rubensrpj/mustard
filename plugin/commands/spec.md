---
description: Use when the user wants to approve a planned spec or continue an in-progress spec. Single picker — delegates to mustard-rt run active-specs and resume-bootstrap.
source: manual
---
<!-- mustard:generated -->
# /mustard:spec — Unified Spec Picker

`/mustard:spec [alvo]` — replaces `/approve` (PLAN) and `/resume` (EXEC). `alvo` is a **picker letter** (`a`-`z`) OR a **spec name** (slug). Empty → render the table to pick. A spec name jumps **straight to that spec — no table**. A letter + `r` (e.g. `ar`) = implement inline immediately **after** the real approval — `r` pre-answers *implement now*, it never grants or skips the approval itself.

## 1. Parse `alvo`

- **Empty** → picker mode: render the table (§2), wait for a letter.
- **`^[a-z]r?$`** → letter mode: render the table (§2), map the letter to its spec name, route (§3). A trailing `r` pre-answers the §3 EXECUTE continuation as *implement now* — the user still performs the real approval (the plan-mode `ExitPlanMode` accept, or the approval `AskUserQuestion`); on a Full spec `.clarified` still precedes it. The picker bypasses neither marker.
- **Anything else** → **focused mode**: `alvo` IS the spec name. **SKIP the table — do NOT run `active-specs`, do NOT print Siglas/Modo.** Route directly (§3). No `r` parsing (a slug may legitimately end in `r`).

## 2. Picker render (picker + letter modes only — FORBIDDEN in focused mode)

```bash
rtk mustard-rt run active-specs --format table
```

Print stdout verbatim, then these two blocks literally:

**Siglas** — `#` letter (a-z), `Esc` Scope (`lt` light / `fl` full / `-`), `Prog` waves done/total. Stage `PLAN` planejar / `EXEC` executar. Status `TF` tactical-fix, `TF→{alias}` TF parent, `W{N}` wave N, `BLOCK` blocked, `em exec` dispatched, `-` none.

**Modo de seleção** — `a-z` act on row (PLAN approve / EXEC continue). `a-z+r` (e.g. `ar`) pre-answers *implement now* — the approval (ExitPlanMode accept / approval AskUserQuestion) still happens; `r` never bypasses it (EXEC ignores `r`). A spec name jumps straight to it (no table). Anything else → error + re-render.

## 3. Resolve + route via `resume-bootstrap`

Letter mode: map the picked letter to its `active-specs` row → `{specName}`. Focused mode: `{specName}` = `alvo` verbatim. Then:

```bash
rtk mustard-rt run resume-bootstrap --spec {specName} --json
```

Route on the returned `stage` — the whole procedure lives in **`${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md`**:

- **`Plan`** → resume-loop **§A Approve** (owns the single-spec render + the approval: plan mode first, the approve/implement `AskUserQuestion` as fallback). A letter-mode `r` pre-answers only the *implement now* continuation — never the approval: the user still accepts via `ExitPlanMode` (or answers the approval `AskUserQuestion`), and on a Full spec `.clarified` must precede it. The picker mints neither marker.
- **`Execute` / `Analyze` / `QaReview` / `QaPending` / `ReviewPending` / `Close`** → resume-loop **§B Loop** (the `wave-advance` relay — routing, order and prompts are decided by Rust; the LLM only relays). In focused mode, first print a one-line header (`{specName} — retomando (EXEC)`; precise wave numbering comes from `wave-tree`) and ask a single **"Implementar agora?"** confirm before dispatch; letter mode (and a letter-mode `r`) skip that resume confirm — an EXEC-stage spec is already past approval, so nothing is bypassed; `r` carries no approval meaning here.

## 4. Edge cases

0 specs → *"Nenhuma spec ativa."*. >26 → first 26 + *"(N adicionais)"*. Focused mode with an unknown slug (`resume-bootstrap` errors) → *"Spec '{alvo}' não encontrada."* then render the table (§2) as a fallback.

## Inviolable

- Siglas + Modo blocks are mandatory + literal in **picker/letter mode**; **FORBIDDEN in focused mode** (render only that one spec).
- A bare spec name routes **directly** to that spec — NEVER list all specs first to "find" it (`resume-bootstrap`/`approve-spec` are name-addressable; `active-specs` exists only for letter picking).
- A PLAN-stage spec gets **one** question (approve + implement now / approve only / …); NEVER approve-then-tell-the-user-to-re-run as the default — that is the *approve only — new session* secondary option, not the primary path.
- NEVER hand-craft agent prompts, read `wave-plan.md`, decide wave order, or reimplement `continued` vs `reanalyzed` — `wave-advance`/`resume-bootstrap` own routing; the LLM relays.
- **Full: clarify precedes approval (F6).** A Full plan must be CLARIFIED before it can be approved — the clarify-finalize (`grill-capture --finalize`, run after the ANALYZE glossary grill) records `<spec>/.clarified`. Until it exists, `approve-spec` REFUSES the approval and points at the finalize, so an under-specified Full spec never sails into EXEC unclarified. (Light/task specs carry no clarify gate.)
