---
description: Use when the user wants to approve a planned spec or continue an in-progress spec. Single picker — delegates to mustard-rt run active-specs and resume-bootstrap.
argument-hint: [picker-letter | spec-name]
source: manual
---
<!-- mustard:generated -->
# /mustard:spec — Unified Spec Picker

`/mustard:spec [alvo]` — replaces `/approve` (PLAN) and `/resume` (EXEC). `alvo` is a **picker letter** (`a`-`z`) OR a **spec name** (slug). Empty → render the table to pick. A spec name jumps **straight to that spec — no table**. A letter + `r` (e.g. `ar`) **IS** the approval and the *implement now* answer in one typed gesture: the text the user types is an act the model cannot author, so an observer mints `<spec>/.approved-by-user` from it and the spec goes straight to wave 1. The letter ALONE (no `r`) mints nothing and still routes through the normal approval.

## 1. Parse `alvo`

- **Empty** → picker mode: render the table (§2), wait for a letter.
- **`^[a-z]r?$`** → letter mode: render the table (§2), map the letter to its spec name, route (§3). A trailing `r` IS the approval: the user's own prompt mints `<spec>/.approved-by-user` (`via` naming the picker), so §3 asks for no second gesture and reads the same gesture as the EXECUTE continuation *implement now*. The letter ALONE (no `r`) mints nothing — the real approval (the plan-mode `ExitPlanMode` accept, or the approval `AskUserQuestion`) still happens in §A. On a Full spec `.clarified` precedes the approval either way; the picker bypasses that marker no more than any other route.
- **Anything else** → **focused mode**: `alvo` IS the spec name. **SKIP the table — do NOT run `active-specs`, do NOT print Siglas/Modo.** Route directly (§3). No `r` parsing (a slug may legitimately end in `r`).

## 2. Picker render (picker + letter modes only — FORBIDDEN in focused mode)

```bash
rtk mustard-rt run active-specs --format table
```

Print stdout verbatim, then these two blocks literally:

**Siglas** — `#` letter (a-z), `Esc` Scope (`lt` light / `fl` full / `-`), `Prog` waves done/total. Stage `PLAN` planejar / `EXEC` executar. Status `TF` tactical-fix, `TF→{alias}` TF parent, `W{N}` wave N, `BLOCK` blocked, `em exec` dispatched, `-` none. `Onde` where the spec LIVES: `-` na árvore atual; `{branch}` = spec **em voo** — o diretório só existe nesse branch de trabalho, troque de branch antes de agir nessa linha. A closing line stating the branch scan could not run means the listing covers the checkout ONLY — print it too; it is a different claim from *"estas são todas"*.

**Modo de seleção** — `a-z` act on row (PLAN approve / EXEC continue); the bare letter still asks for the approval (ExitPlanMode accept / approval AskUserQuestion). `a-z+r` (e.g. `ar`) **IS** that approval plus *implement now* — the text you typed mints `<spec>/.approved-by-user`, so nothing asks again (EXEC ignores `r`). A spec name jumps straight to it (no table). Anything else → error + re-render.

## 3. Resolve + route via `resume-bootstrap`

Letter mode: map the picked letter to its `active-specs` row → `{specName}`. Focused mode: `{specName}` = `alvo` verbatim. Then:

```bash
rtk mustard-rt run resume-bootstrap --spec {specName} --json
```

Route on the returned `stage` — the whole procedure lives in **`${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md`**:

- **`Plan`** → resume-loop **§A Approve** (owns the single-spec render + the approval: plan mode first, the approve/implement `AskUserQuestion` as fallback). A letter-mode `r` arrives with the approval already made: the user's typed prompt minted `<spec>/.approved-by-user`, so §A presents the plan for the record and falls straight into the dispatch — no plan-mode round trip, no second question. Without the `r` the user still accepts via `ExitPlanMode` (or answers the approval `AskUserQuestion`), and on a Full spec `.clarified` must precede it either way. **`approvedByUser:true` (already approved in `/feature`) takes the same shortcut** — §A skips the re-approval and asks only implement-now vs approve-only.
- **`Execute` / `Analyze` / `QaReview` / `QaPending` / `ReviewPending` / `Close`** → resume-loop **§B Loop** (the `wave-advance` relay — routing, order and prompts are decided by Rust; the LLM only relays). In focused mode, first print a one-line header (`{specName} — retomando (EXEC)`; precise wave numbering comes from `wave-tree`) and ask a single **"Implementar agora?"** confirm before dispatch; letter mode (and a letter-mode `r`) skip that resume confirm — an EXEC-stage spec is already past approval, so nothing is bypassed; `r` carries no approval meaning here.

## 4. Edge cases

0 specs → *"Nenhuma spec ativa."*. >26 → first 26 + *"(N adicionais)"*. Focused mode with an unknown slug (`resume-bootstrap` errors) → *"Spec '{alvo}' não encontrada."* then render the table (§2) as a fallback.

## Inviolable

- Siglas + Modo blocks are mandatory + literal in **picker/letter mode**; **FORBIDDEN in focused mode** (render only that one spec).
- A bare spec name routes **directly** to that spec — NEVER list all specs first to "find" it (`resume-bootstrap`/`approve-spec` are name-addressable; `active-specs` exists only for letter picking).
- A PLAN-stage spec gets **one** question (approve + implement now / approve only / …); NEVER approve-then-tell-the-user-to-re-run as the default — that is the *approve only — new session* secondary option, not the primary path. When the marker is already minted — a typed `{letter}r`, or `approvedByUser:true` — it gets **zero**: re-asking for a gesture the user already made is the ceremony this picker exists to remove.
- NEVER hand-craft agent prompts, read `wave-plan.md`, decide wave order, or reimplement `continued` vs `reanalyzed` — `wave-advance`/`resume-bootstrap` own routing; the LLM relays.
- **Full: clarify precedes approval (F6).** A Full plan must be CLARIFIED before it can be approved — the clarify-finalize (`grill-capture --finalize`, run after the ANALYZE glossary grill) records `<spec>/.clarified` with WHAT was settled: `--term` per term the grill captured, or `--reason` stating why no grill applied. Until that marker exists **and carries substance**, `approve-spec` REFUSES the approval and names the grill to run — so an under-specified Full spec never sails into EXEC unclarified, and a marker minted seconds earlier with nothing in it no longer unlocks anything. (Light/task specs carry no clarify gate.)
