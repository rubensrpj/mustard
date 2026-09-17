# Material Rules

What the unit carries beyond itself. `orchestrator.md` classifies the request and `dispatch.md` opens and names the unit; this file holds the pending ledger, where a unit's writing is allowed to land, and how every page shown to the user is written. Why the router ships as separate injectables: `refs/mustard/router-rationale.md`.

## Pending

**Work agreed beyond the unit being opened is recorded BEFORE the gate call** — one item per agreed piece of work, including an ORDER between units. The pending ledger lives outside every unit (`.claude/pending/ledger.json` in the main checkout), so it takes an item with no unit open and outlives the unit that delivers it. Measured 2026-09-09: three works agreed in the order 2 → 3 → 1; the order belonged to no unit, and the closing summary dropped the third.

```
mustard-rt run pending --add --title "<what was agreed>" --detail "<scope / why>"
mustard-rt run pending --close P-{n} --reason "<what delivered it>"
mustard-rt run pending
```

An item leaves the list ONLY with a reason — a blank `--reason` is refused and nothing is written. Without a flag it lists what is open, with the one-line count the session start shows.

**A removal takes two calls.** The first shows what would leave and prints a code; ask the user, and only on their yes run the same call again with `--confirm <code>`. If the list changed in between, nothing leaves. A removed item stays in the list as dropped, with its reason, and `--reopen` brings it back.

```
mustard-rt run pending --remove --id P-{n} --reason "<why it no longer stands>"
mustard-rt run pending --remove --term "<word>" --reason "<why>"
mustard-rt run pending --remove --before 2026-08-01 --reason "<why>" --confirm <code>
mustard-rt run pending --drop P-{n} --reason "<why>"
mustard-rt run pending --reopen P-{n}
```

**Idle items come back once.** When the session start says items have been idle for over 30 days, run `mustard-rt run pending --stale` and ask the user, in ONE question, which ones stay; then `mustard-rt run pending --expire --keep P-{a},P-{b}` drops the others as expired.

## Where it lands

`spec-draft` checks `{kind}/{slug}` out in the MAIN checkout, so the whole unit is authored ON it: `spec.md`, the waves, the ceremony and the code alike. There is no `.claude/spec/` carve-out; a spec write on a bare integration base is DENIED like any other write — the branch the gate minted is the only place this material exists. An old `{base}_{slug}` name still reads as its unit.

The base the unit was cut from is RECORDED at the cut and fixes the `/git` PR target, never re-derived from the branch prefix. Same rule as a decision: what is settled once is written where it was settled, never reconstructed later from a name.

## Pages

**Every page shown to the user — a plan, a report, a summary, an analysis — goes through `page`**, never a look of its own. Write it in markdown, never HTML; the command brings the layout and the fonts, and the first `# Title` line is the title:

```
mustard-rt run page --body <page.md> --out <page.html> [--subtitle "<line>"] [--kind "<label>"]
```

The page is written in the project's text language (`language.text` in `mustard.json`); there is no flag to choose another.

The Mustard layout IS the project's design system: it beats any page-design guidance that asks for a fresh look per subject. Measured 2026-09-10: a page published in another project with a look of its own — the rule lived in one machine's memory, and memory does not travel.

**Publishing is not an option.** When a publishing tool exists, the page is published on claude.ai, republished at the SAME address whenever it changes, and the link is handed to the user. The spec page records its address as an event: `mustard-rt run write publish --spec {slug} --json '{"page":"spec","ok":true,"url":"<url>"}'`. With no publishing tool, hand over the ways to open it that exist today.
