---
description: Use when the user wants to approve a planned spec or continue an in-progress spec. Single picker — delegates to mustard-rt run active-specs and resume-bootstrap.
argument-hint: [picker-letter | spec-name]
source: manual
---
<!-- mustard:generated -->
# /mustard:spec — Unified Spec Picker

`/mustard:spec [alvo]` — replaces `/approve` (PLAN) and `/resume` (EXEC). `alvo` is a **picker letter** (`a`-`z`) OR a **spec name** (slug). Empty → render the table to pick. A spec name jumps **straight to that spec — no table**. A letter + `r` (e.g. `ar`) **IS** the approval and the *implement now* answer in one typed gesture — but only as the WHOLE prompt, `/mustard:spec ar` **typed in full**: the observer matches the entire prompt and nothing looser, because a rule that matched a substring would let a message merely quoting the form forge an approval. That whole prompt is an act the model cannot author, so an observer mints `<spec>/.approved-by-user` from it and the spec goes straight to wave 1. The letter ALONE (no `r`), and an `ar` answered into an already-open table rather than typed as the command, mint nothing and still route through the normal approval.

## 1. Parse `alvo`

- **Empty, and the checkout IS a unit's own work branch** → **that unit, directly.** Take its slug off the branch name (`{kind}/{slug}`, or the older `{base}_{slug}`), skip §2 entirely — no table, no Siglas, no Modo — and route it through §3 as if it had been named. **Standing inside a unit is the answer to "which one":** rendering a table there asks the caller to pick the row they are demonstrably already on, which is the ceremony this door exists to remove, and `resume-bootstrap` says the same thing one step later with `insideWorkBranch`. If that slug matches no spec directory, fall through to the table below rather than erroring — the branch may be someone's plain work branch.
- **Empty, on an integration base** → picker mode: render the table (§2), wait for a letter. Here the question is real: a base carries no unit, so nothing but the table can say which one.
- **`^[a-z]r?$`** → letter mode: render the table (§2), map the letter to its spec name, route (§3). A trailing `r` IS the approval **when it arrived as the whole prompt — `/mustard:spec ar`, typed in full**: that prompt mints `<spec>/.approved-by-user` (`via` naming the picker), so §3 asks for no second gesture and reads the same gesture as the EXECUTE continuation *implement now*. The same two characters answered INTO the open table never reach the observer — that is letter mode with no approval attached. The letter ALONE (no `r`) mints nothing either — the real approval (the plan-mode `ExitPlanMode` accept, or the approval `AskUserQuestion`) still happens in §A. On a Full spec `.clarified` precedes the approval either way; the picker bypasses that marker no more than any other route.
- **Anything else** → **focused mode**: `alvo` IS the spec name. **SKIP the table — do NOT run `active-specs`, do NOT print Siglas/Modo.** Route directly (§3). No `r` parsing (a slug may legitimately end in `r`).

## 2. Picker render (picker + letter modes only — FORBIDDEN in focused mode)

```bash
rtk mustard-rt run active-specs --format table
```

Print stdout verbatim, then these two blocks literally:

**Siglas** — `#` letter (a-z), `Esc` Scope (`lt` light / `fl` full / `-`), `Prog` waves done/total. Stage `PLAN` planejar / `EXEC` executar. Status `TF` tactical-fix, `TF→{alias}` TF parent, `W{N} em exec` wave N dispatched and running, `W{N} a iniciar` plan scaffolded, **nothing dispatched yet** — start it rather than resume it, `⚠ malformed` meta incompleta, `closed-followup` spec fechada com follow-up pendente, `-` none. `Onde` where the spec LIVES: `-` na árvore atual; `{branch}` = spec **em voo** — o diretório só existe nesse branch de trabalho, troque de branch antes de agir nessa linha. A closing line stating the branch scan could not run means the listing covers the checkout ONLY — print it too; it is a different claim from *"estas são todas"*.

**Modo de seleção** — `a-z` act on row (PLAN approve / EXEC continue); a letter answered into this table is a row choice and nothing else, so the approval is still asked for (ExitPlanMode accept / approval AskUserQuestion). To approve and *implement now* in the same gesture, send the command **typed in full** — `/mustard:spec ar` as the whole prompt — which is what mints `<spec>/.approved-by-user`, so nothing asks again (EXEC ignores `r`). In full because the observer matches the entire prompt: anything looser would let a message that merely quotes the form forge the marker. A spec name jumps straight to it (no table). Anything else → error + re-render.

## 3. Resolve + route via `resume-bootstrap`

Letter mode: map the picked letter to its `active-specs` row → `{specName}`. Focused mode: `{specName}` = `alvo` verbatim. **Empty + work-branch mode (§1) counts as focused**: `{specName}` = the slug read off the branch name, and everything below that says "focused mode" applies to it too — there is no third mode to route on. Then:

```bash
rtk mustard-rt run resume-bootstrap --spec {specName} --json
```

Route on the returned `stage` — the whole procedure lives in **`${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md`**:

- **`Plan`** → resume-loop **§A Approve** (owns the single-spec render + the approval: plan mode first, the approve/implement `AskUserQuestion` as fallback). A letter-mode `r` that arrived as the whole prompt (`/mustard:spec ar`, typed in full) brings the approval already made: that prompt minted `<spec>/.approved-by-user`, so §A presents the plan for the record and falls straight into the dispatch — no plan-mode round trip, no second question. Without the `r` the user still accepts via `ExitPlanMode` (or answers the approval `AskUserQuestion`), and on a Full spec `.clarified` must precede it either way. **`approvedByUser:true` (already approved in `/feature`) takes the same shortcut** — §A skips the re-approval and asks only implement-now vs approve-only.
- **`Execute` / `Analyze` / `QaReview` / `QaPending` / `ReviewPending` / `Close`** → resume-loop **§B Loop** (the `wave-advance` relay — routing, order and prompts are decided by Rust; the LLM only relays). **Read `insideWorkBranch` off the `resume-bootstrap` output you already have.** `true` — the checkout IS this spec's `{kind}/{slug}` branch (or its older `{base}_{slug}` name), the unit's own home where its spec, waves, ceremony and code all live: the caller is already inside the work, so **no table, no header, no "Implementar agora?"** — fall straight into §B and dispatch. `false` in focused mode → print the one-line header (`{specName} — retomando (EXEC)`; precise wave numbering comes from `wave-tree`) and ask the single **"Implementar agora?"** confirm before dispatch. Letter mode (and a letter-mode `r`) skip that resume confirm regardless — an EXEC-stage spec is already past approval, so nothing is bypassed; `r` carries no approval meaning here.

## 4. Edge cases

0 specs → *"Nenhuma spec ativa."*. >26 → first 26 + *"(N adicionais)"*. Focused mode with an unknown slug (`resume-bootstrap` errors) → *"Spec '{alvo}' não encontrada."* then render the table (§2) as a fallback.

## Inviolable

- Siglas + Modo blocks are mandatory + literal in **picker/letter mode**; **FORBIDDEN in focused mode** (render only that one spec).
- **Inside the unit's own branch the resume costs NOTHING — and that starts at §1, not at §3.** `insideWorkBranch: true` ⇒ no table, no header, no *implement now* question. Asking a caller standing on the unit's own branch whether to start the work they are demonstrably already inside is the ceremony this door exists to remove. The rule used to be stated here and enforced only from §3, while §1 still said "Empty → render the table" with no exception — so a bare `/mustard:spec` typed inside a unit rendered the table anyway, and the inviolable was true about the step after the one that broke it (found in the field, 2026-08-18, on this repository's own unit).
- A bare spec name routes **directly** to that spec — NEVER list all specs first to "find" it (`resume-bootstrap`/`approve-spec` are name-addressable; `active-specs` exists only for letter picking).
- A PLAN-stage spec gets **one** question (approve + implement now / approve only / …); NEVER approve-then-tell-the-user-to-re-run as the default — that is the *approve only — new session* secondary option, not the primary path. When the marker is already minted — a `/mustard:spec {letter}r` typed in full, or `approvedByUser:true` — it gets **zero**: re-asking for a gesture the user already made is the ceremony this picker exists to remove.
- NEVER hand-craft agent prompts, read `wave-plan.md`, decide wave order, or reimplement `continued` vs `reanalyzed` — `wave-advance`/`resume-bootstrap` own routing; the LLM relays.
- **Full: clarify precedes approval (F6).** A Full plan must be CLARIFIED before it can be approved — the clarify-finalize (`grill-capture --finalize`, run after the ANALYZE glossary grill) records `<spec>/.clarified` with WHAT was settled: `--term` per term the grill captured, or `--reason` stating why no grill applied. Until that marker exists **and carries substance**, `approve-spec` REFUSES the approval and names the grill to run — so an under-specified Full spec never sails into EXEC unclarified, and a marker minted seconds earlier with nothing in it no longer unlocks anything. (Light/task specs carry no clarify gate.)
