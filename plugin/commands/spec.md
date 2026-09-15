---
description: Use when the user wants to approve a planned spec or continue an in-progress spec. Single picker — delegates to mustard-rt run active-specs and resume-bootstrap.
argument-hint: [picker-letter | spec-name]
---
<!-- mustard:generated -->
# /mustard:spec — Unified Spec Picker

`/mustard:spec [alvo]` — replaces `/approve` (PLAN) and `/resume` (EXEC). `alvo` is a **picker letter** (`a`-`z`) OR a **spec name** (slug). A spec name jumps **straight to that spec — no table**. **Selecting is not approving:** a picker letter, a spec name, or the bare `/mustard:spec` inside the unit's own work branch only chooses WHICH spec to open. The approval is ONE gesture, and the model cannot author it: the user chooses "Aprovar" in the question *"Aprovar esta spec?"* (§3, resume-loop §A). The approval witness records it in `spec.ndjson` from the user's own answer — no command, no second step — and the Mustard then suggests `/clear`, so the execution starts in a clean window. Accepting plan mode (`ExitPlanMode`) and typing `/mustard:spec a` approve nothing; the older `ar` spelling is an alias of the letter.

## 1. Parse `alvo`

- **Empty, and the checkout IS a unit's own work branch** → **that unit, directly.** Take its slug off the branch name (`{kind}/{slug}`, or the older `{base}_{slug}`), skip §2 entirely — no table, no Siglas, no Modo — and route it through §3 as if it had been named. **Standing inside a unit is the answer to "which one":** rendering a table there asks the caller to re-state what the tree already says, which is the ceremony this door exists to remove; `resume-bootstrap` says the same thing one step later with `insideWorkBranch`. Typing the command there approves nothing — a Plan-stage unit still gets the one approval question of §A. If that slug matches no spec directory, fall through to the table below rather than erroring — the branch may be someone's plain work branch, and then the bare command names nothing.
- **Empty, on an integration base** → picker mode: render the table (§2), wait for a letter. Here the question is real: a base carries no unit, so nothing but the table can say which one.
- **`^r$` — the bare `r`** → the same as the bare command above, spelled out. Inside the unit's own work branch it names that unit; **outside one it reads as the row letter it looks like** (the checkout wins the collision when there is a checkout to win it). On an integration base, a detached HEAD, or a slug no spec directory carries, the checkout names nothing and the letter fallback decides: row `r` if the table carries one, nothing otherwise. Like every picker form, it approves nothing: it only chooses WHICH spec §3 opens. (Row `r` with the older suffix spelling is still `rr`.)
- **`^[a-z]r?$`** → letter mode: render the table (§2), map the letter to its spec name, route (§3). The letter only SELECTS the row — typed as the whole prompt or answered into the table, it approves nothing, and a Plan-stage spec still gets the one approval question of §A. The trailing `r` is an ALIAS, kept because it is in muscle memory and in older prose. **One carve-out, the bullet above:** a lone `r` is resolved by the CHECKOUT first, so inside a unit's branch it is never row `r` — it only reads as that row where the tree stands on no unit at all.
- **Anything else** → **focused mode**: `alvo` IS the spec name. **SKIP the table — do NOT run `active-specs`, do NOT print Siglas/Modo.** Route directly (§3). No `r` parsing (a slug may legitimately end in `r`).

## 2. Picker render (picker + letter modes only — FORBIDDEN in focused mode)

```bash
rtk mustard-rt run active-specs --format table
```

Print stdout verbatim, then these two blocks literally:

**Siglas** — `#` letter (a-z), `Esc` Scope: `lt` for light, `fl` for full, and `-` for **everything else** — the abbreviation recognises those two prefixes and nothing more, so a `touch`-scope spec renders exactly like a spec whose `meta.json` carries no scope at all. The dash is "not light and not full", never "no scope"; when the distinction matters, read `meta.json#scope`. `Prog` waves done/total. Stage `PLAN` planejar / `EXEC` executar. Status `TF` tactical-fix, `TF→{alias}` TF parent, `W{N} em exec` wave N dispatched and running, `W{N} a iniciar` plan scaffolded, **nothing dispatched yet** — start it rather than resume it, `⚠ malformed` meta incompleta, `closed-followup` spec fechada com follow-up pendente, `-` none. `Onde` where the spec LIVES: `-` na árvore atual; `{branch}` = spec **em voo** — o diretório só existe nesse branch de trabalho, troque de branch antes de agir nessa linha. A closing line stating the branch scan could not run means the listing covers the checkout ONLY — print it too; it is a different claim from *"estas são todas"*.

**Modo de seleção** — `a-z` act on row (PLAN: mostra a spec e pergunta *"Aprovar esta spec?"* / EXEC: continua). A letra só escolhe a linha: aprovar é escolher "Aprovar" na pergunta, e nada mais aprova. Inside the unit's own work branch the letter is not even needed: `/mustard:spec` on its own names the unit the branch names. The older `ar` spelling still works, as an alias. A spec name jumps straight to it (no table). Anything else → error + re-render.

## 3. Resolve + route via `resume-bootstrap`

Letter mode: map the picked letter to its `active-specs` row → `{specName}`. Focused mode: `{specName}` = `alvo` verbatim. **Empty + work-branch mode (§1) counts as focused**: `{specName}` = the slug read off the branch name, and everything below that says "focused mode" applies to it too — there is no third mode to route on. Then:

```bash
rtk mustard-rt run resume-bootstrap --spec {specName} --json
```

**Every resume hands over the published page — at every stage.** Read `publishedUrl` off the `resume-bootstrap` output: the address the unit's page was published at. Not `null` → hand the user that link on a line of its own, first thing, whatever the `stage` — nothing else in the session hands it over, so a unit resumed with an unchanged page would otherwise never show its link again. `null` → run `rtk mustard-rt run spec-doc --spec {specName}`, publish the page at the `path` it returns (`.claude/spec/{specName}/resumo.html`) as a claude.ai page, hand over that link on a line of its own, and record it with `rtk mustard-rt run spec-doc --spec {specName} --published-url <url>` — the resume reads it back from there. Publishing is part of every delivery, in a local session and over SSH alike: it is never an option offered to the user. Whenever `spec-doc` reports the page `changed`, republish it yourself, in the same turn, at the recorded `publishedUrl`, so the new version lives at the SAME address — no hook orders it, so it is carried out, never relayed to the user as a suggestion.

**On a `Plan`-stage spec the user reads the spec as a PAGE before anything asks for approval.** Run, first:

```bash
rtk mustard-rt run spec-doc --spec {specName}
```

and hand the user the published link — republished at the recorded address when the page changed — on a line of its own, BEFORE the approval question (`AskUserQuestion`) or any "aprovar?" in text. The page carries the conversation summary, where the unit stands, what was clarified, the decisions, the risks, the criteria with their red proof, each wave with its skills, and the open pending items. The terminal render is no substitute: an approval was refused precisely because the user could not read the spec there ("não consigo ler a spec por isso preciso do html"). The page is rewritten only when its content changed (`changed`), so running it every time costs nothing — and when the spec is already approved and §A asks nothing, the link is still handed over once, as the record of what was approved.

**No publishing tool — the last resort.** Only when no tool that publishes a web page (a claude.ai artifact) is available, hand over the ways to open `resumo.html` that exist without one, BEFORE the approval question (`AskUserQuestion`) or any "aprovar?" in text: in a local session, the `url` `spec-doc` returns (the `file://…/resumo.html`) as a clickable link; over SSH (`SSH_CONNECTION` or `SSH_CLIENT` is set — `printenv SSH_CONNECTION SSH_CLIENT` prints something), where that `file://` points at the server's disk and never reaches the user, one `scp` command per system that copies the page to the user's machine and opens it, with the user from `$USER`, the host from the third field of `SSH_CONNECTION` and the remote path in single quotes: `scp 'user@host:/path/resumo.html' $env:TEMP\resumo.html; start $env:TEMP\resumo.html` (Windows, PowerShell), `scp 'user@host:/path/resumo.html' /tmp/resumo.html && open /tmp/resumo.html` (macOS), and the same with `xdg-open` (Linux).

Route on the returned `stage` — the whole procedure lives in **`${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md`**:

- **`Plan`** → resume-loop **§A Approve** (owns the single-spec render + the ONE approval question, *"Aprovar esta spec?"*, with **Aprovar** and **Ajustar**). Choosing **Aprovar** is the whole approval: the approval witness records it in `spec.ndjson` from the user's own answer, with no command, and the Mustard then suggests `/clear` — the execution starts in a clean window, through `/mustard:spec {specName}`. A letter, the bare command, plan mode (`ExitPlanMode`) and any typed text approve nothing. **`approvedByUser:true` (the spec is already approved) skips the question** — §A asks nothing and starts the execution.
- **`Execute` / `Analyze` / `QaReview` / `QaPending` / `ReviewPending` / `Close`** → resume-loop **§B Loop** (the `wave-advance` relay — routing, order and prompts are decided by Rust; the LLM only relays). **Read `insideWorkBranch` off the `resume-bootstrap` output you already have.** `true` — the checkout IS this spec's `{kind}/{slug}` branch (or its older `{base}_{slug}` name), the unit's own home where its spec, waves, ceremony and code all live: the caller is already inside the work, so **no table, no header, no "Implementar agora?"** — fall straight into §B and dispatch. `false` in focused mode → print the one-line header (`{specName} — retomando (EXEC)`; precise wave numbering comes from `wave-tree`) and ask the single **"Implementar agora?"** confirm before dispatch. Letter mode (and a letter-mode `r`) skip that resume confirm regardless — an EXEC-stage spec is already past approval, so nothing is bypassed; `r` carries no approval meaning here.

## 4. Edge cases

0 specs → *"Nenhuma spec ativa."*. >26 → first 26 + *"(N adicionais)"*. Focused mode with an unknown slug (`resume-bootstrap` errors) → *"Spec '{alvo}' não encontrada."* then render the table (§2) as a fallback.

## Inviolable

- **The page precedes the question, and it travels as a published page.** No approval is ever asked with terminal text alone: the page is published to claude.ai and its link shown first (§3), and every resume, at every stage, hands the recorded `publishedUrl` over again. The `file://` link or the `scp` commands count only when no publishing tool exists — and over SSH a `file://` link never leaves the server.
- Siglas + Modo blocks are mandatory + literal in **picker/letter mode**; **FORBIDDEN in focused mode** (render only that one spec).
- **Inside the unit's own branch the resume costs NOTHING — and that starts at §1, not at §3.** `insideWorkBranch: true` ⇒ no table, no header, no *implement now* question. Asking a caller standing on the unit's own branch whether to start the work they are demonstrably already inside is the ceremony this door exists to remove. The rule used to be stated here and enforced only from §3, while §1 still said "Empty → render the table" with no exception — so a bare `/mustard:spec` typed inside a unit rendered the table anyway, and the inviolable was true about the step after the one that broke it (found in the field, 2026-08-18, on this repository's own unit).
- A bare spec name routes **directly** to that spec — NEVER list all specs first to "find" it (`resume-bootstrap`/`approve-spec` are name-addressable; `active-specs` exists only for letter picking).
- A PLAN-stage spec gets **one** question — *"Aprovar esta spec?"*, with **Aprovar** and **Ajustar** — and choosing **Aprovar** is the whole approval: the witness records it, and the next step is `/clear` and a clean window, never a dispatch in the window that asked. An already approved spec (`approvedByUser:true`) gets **zero**: re-asking for a gesture the user already made is the ceremony this picker exists to remove.
- NEVER hand-craft agent prompts, read `wave-plan.md`, decide wave order, or reimplement `continued` vs `reanalyzed` — `wave-advance`/`resume-bootstrap` own routing; the LLM relays.
