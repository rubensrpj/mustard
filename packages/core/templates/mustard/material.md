# Material Rules

The conversation's own channel. `orchestrator.md` classifies the request and `dispatch.md` opens and names the unit; this file is what the unit CARRIES out of the conversation, and where that writing is allowed to land. Why the router ships as separate injectables: `refs/mustard/router-rationale.md`.

## Material

**A decision the conversation settles is written down when it is settled**, never reconstructed from memory at draft time. What is not written before a compaction is lost — measured: two units shipped and NEITHER carried material.

```
mustard-rt run material-add --spec {slug} --kind decision   --subject "<what>"  --detail "<why>"
mustard-rt run material-add --spec {slug} --kind definition --subject "<term>"  --detail "<what it means here>"
mustard-rt run material-add --spec {slug} --kind finding    --subject "<claim>" --detail "<file>" [--line N]
mustard-rt run material-add --spec {slug} --kind risk       --subject "<risk>"  --detail "<what mitigates it>" --severity alta|media|baixa
mustard-rt run material-add --spec {slug} --kind summary    --subject "<the whole conversation so far>"
mustard-rt run material-add --spec {slug} --kind flow       --subject "<title>" --detail "<before/after diagram, plain text>"
```

**The summary is ONE text, and the newest replaces the last** — rewrite it whole as the conversation moves; a `--detail` on it is refused. A `flow` is the change drawn before/after in plain text; the newest replaces the last too, and its indentation is kept. A `risk` with no `--severity` is refused: a risk without a weight does not tell the reader whether to stop and read it. A `clarification` (question + answer) is recorded ON ITS OWN when the user answers a question — never by hand. The spec page reads all of them: `mustard-rt run spec-doc --spec {slug}` writes `.claude/spec/{slug}/resumo.html`.

One call per item, when it is settled. Each lands in the unit's `spec-material.json`, which is the file `spec-draft --material` reads. **They open from ▸6 on:** the base gate's event log creates `.claude/spec/{slug}/`, so a decision settled before the draft still lands. `unknown_spec` means no gate minted that slug — no unit is open.

**Once the spec exists, carry a new item in with `--material-only`** — it rewrites the three material sections and leaves every other byte of `spec.md` alone:

```
mustard-rt run spec-draft --slug {slug} --intent "{intent}" --material .claude/spec/{slug}/spec-material.json --material-only
```

A full `--force` re-draft is for a spec whose NARRATIVE changed. Reach for it and you rewrite the whole body to get one decision in — measured in the field, that cost the operator a save-and-splice script on every round of the conversation, which is a good way to stop recording decisions at all.

## Pending

**Work agreed beyond the unit being opened is recorded BEFORE the gate call** — one item per agreed piece of work, including an ORDER between units. The pending ledger lives outside every unit (`.claude/pending/ledger.json` in the main checkout), so it takes an item with no unit open and outlives the unit that delivers it. Measured 2026-09-09: three works agreed in the order 2 → 3 → 1; the order belonged to no unit, and the closing summary dropped the third.

```
mustard-rt run pending --add --title "<what was agreed>" --detail "<scope / why>"
mustard-rt run pending --close P-{n} --reason "<what delivered it>"
mustard-rt run pending --drop P-{n} --reason "<why it no longer stands>"
mustard-rt run pending
```

An item leaves the list ONLY with a reason — a blank `--reason` is refused and nothing is written. Without a flag it lists what is open.

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

**Publishing is not an option.** When a publishing tool exists, the page is published on claude.ai, republished at the SAME address whenever it changes, and the link is handed to the user. The spec page records its address: `mustard-rt run spec-doc --spec {slug} --published-url <url>`. With no publishing tool, hand over the ways to open it that exist today.
