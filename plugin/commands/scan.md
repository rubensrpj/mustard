---
description: An internal flow — not a door. The base gate re-mines the deterministic census on its own; this flow is the FULL pass (subproject maps + Guards prose + pattern-skill molds) the router runs when the census needs re-authoring after a large change. Weak fallback only: use when the router did not engage and the model is visibly stale.
argument-hint: [--root <dir>] [--out <path>]
user-invocable: false
source: manual
---
<!-- mustard:generated -->
# scan — Codebase model

**This is not a step you run.** The deterministic census is re-mined automatically at the **base gate** — the single pipeline-opening emit, where a freshly updated integration base is the one moment the tree is clean by construction. What the gate cannot do is the enrichment: it is a Rust process, and Guards prose and pattern molds are written by agents. So this flow is the FULL pass, and the router reaches it when the census needs re-authoring, never the user typing a command.

**Enrichment is STANDARD** — no `--full`/`--enrich` flag, no spend prompt. Every run does the deterministic model, the subproject maps, and both enrichment passes (Guards + pattern molds).

**Git hygiene — the refresh is its OWN unit.** Everything written here is **versioned**: the grain model, every `scan-map.md`, the `## Guards` of every subproject `CLAUDE.md`, and the `{role}-pattern` molds (`.claude/.gitignore` covers only runtime scratch). So:
- **Before:** run it on a CLEAN tree — `scan-clean-gate` refuses otherwise, because under the `/git` `add -A` law the regenerated model could not be committed apart from your work. This is the same clean-tree premise the base gate checks before it re-mines.
- **After:** if it produced changes, commit + push them as their own unit (`/mustard:git pr`). A re-scan over unchanged code is byte-stable and leaves the tree clean — no diff, nothing to push.

## 1. Deterministic model + maps (no AI, you do NOT read source)

```bash
mustard-rt run scan --full [--root <dir>] [--out <path>]
```

Writes `.claude/grain.model.json` (the language-agnostic model — modules, declarations, dependency graph, mined roles, vertical slices, shared contracts, touchpoints) AND regenerates the mustard-owned map file `<unit>/.claude/scan-map.md` for EVERY unit — each subproject and the workspace root alike. **The `CLAUDE.md` belongs to the PROJECT**: mustard's whole footprint there is one `@.claude/scan-map.md` import line at the top (Claude Code's native import — the map still auto-loads with the file), a `## Guards` `pending` seed where none exists, and a breadcrumb heal; curated prose is preserved verbatim, its size is never measured, and a legacy inline scan-map block is migrated out automatically. Downstream (`/feature`, `/bugfix`) consumes the model via `mustard-rt run feature` and `scan spec` — never by reading it directly. Parse the JSON (`{ ok, model, regenerated?, over_cap? }`); a non-empty `over_cap` means a RUNAWAY machine map (generator bug — surface it), never oversized human prose. `ok:false` with `reason: "hollow-submodules"` + `empty_submodules[]` means a submodule is declared but not checked out: the model would silently omit that whole subproject, so nothing was mined and the previous model is intact — run `git submodule update --init --recursive` and re-run. (A worktree cut by the plugin populates them for you; this catches the ones cut out of band.)

Both enrichment passes below are **incremental** (only the delta since the last scan) and **fail-open** (headless / no LLM / empty worklist → skip silently; the model is already complete). One cheap read-only agent per subproject per pass.

## 2. Guards (do/don't prose)

1. `mustard-rt run scan-guards-list` → JSON `[{path, subproject, kind, frameworks}]` for every subproject `CLAUDE.md` still `pending` (root excluded; error → `[]`). Empty → skip.
2. Per item: `mustard-rt run agent-prompt-render --role guards --subproject <subproject> --emit ref` (spec-less — the renderer reads the pending block + derives language/tone from `mustard.json`). Pass the stub to the Task **verbatim**.
3. Dispatch **one agent per subproject** `subagent_type: mustard:mustard-guards` (read-only), all in ONE message. Relay each agent's lines to `mustard-rt run scan-guards-apply --path <path> --guards -` (stdin). Non-destructive, capped ~6 lines, flips the marker off `pending`.

Critical Guards: a line may open with `[critical]` to be enforced at edit time — the post-edit gate Denies (strict) or advises (warn, the default; `MUSTARD_GUARD_GATE_MODE`) an edit that violates the checkable form `[critical] never <forbidden> in <glob>`. Author sparingly; unmarked Guards stay advisory. See `mustard-guards.md`.

## 3. Pattern skills (the `{role}-pattern` molds)

The per-subproject "how we write an X module here" skills that auto-load when an agent edits that folder (`{subproject}/.claude/skills/{role}-pattern/SKILL.md`). **A mustard-generated mold is derived — it is regenerated from scratch on every scan, never preserved.** The origin signal is the frontmatter `source:` field: `source: scan` = mustard-generated (swept and re-authored); `source: manual` (or hand-authored) = human-owned, preserved forever. To adopt a generated mold and stop regenerating it, flip `source: scan` → `source: manual`. The flow is **sweep → list → render → author → apply**, and every step is a command — you never hand-build a prompt or a file in it:

0. `mustard-rt run scan-patterns-sweep` → deletes every `source: scan` mold under the tree BEFORE authoring, so each is written fresh with no bias from its old text (also drops orphans whose cluster no longer exists), and clears the previous run's decline ledger — **declines live for ONE scan cycle**; every cluster is re-judged fresh each run. Preserves `source: manual`. Prints `{removed, preserved, declinesCleared}`. **Corollary:** a mold authored OUTSIDE the mined worklist (a manual dispatch for a role the miner cannot see — no filename affix) is NOT derived; write it with `source: manual`, otherwise the next sweep deletes it and nothing ever re-proposes it.
1. `mustard-rt run scan-patterns-list` → JSON `[{subproject, label, slug, moldPath, affix, exemplars, …}]` — every mineable role cluster (≥3 members, ≥2 hand-written exemplars, not a test/fixture path), uncapped. Post-sweep everything is a create; a surviving `source: manual` mold and slugs declined EARLIER IN THIS RUN are never listed; unparseable model → `[]`. Empty → skip.
2. Per `subproject` in the list: `mustard-rt run agent-prompt-render --role patterns --subproject <subproject> --emit ref` (spec-less — the renderer materialises that subproject's slice of the SAME worklist, via the same `collect`, plus the canonical mold format and the delivery contract). Pass the stub to the Task **verbatim** — NEVER assemble this prompt yourself (hand-building drops the contract the agent needs and molds come back unusable; a wrong-looking prompt means fix the renderer; stub mechanics: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md`).
3. Dispatch **one agent per subproject** `subagent_type: mustard:mustard-patterns` (read-only — returns each mold as a `=== FILE: {moldPath} ===` block, each refusal as a `=== DECLINE: {slug} ===` block), all in ONE message.
4. Pipe each agent's **WHOLE return** to `mustard-rt run scan-patterns-relay` on **stdin** — one call per agent, never one per block, and never via a temp file (`--content` defaults to `-`). The relay splits the envelope on its own demarcators and routes every `=== FILE ===` through the same validation `scan-patterns-apply` enforces and every `=== DECLINE ===` through the same ledger `scan-patterns-decline` writes; prose around the blocks is ignored. **You never split the return yourself** — a subproject returns as many molds as it has clusters (twelve, 55 KB, is a real measurement), and hand-splitting a return that size is where a transcription error or a stray regex loop enters the flow. Read the JSON report: `{ok, blocks, created, declined, refused, collisions, preserved, skipped}`. `ok:false` names what needs attention — a `refused` entry carries the machine's reasons (re-dispatch THAT agent; do not report it as created), a `collision` is a mustard defect, not a preserve — surface it. NEVER the orchestrator's own Write. Report per subproject: created / preserved (hand-authored) / declined.

   *Single-block face (a re-dispatch that returned ONE mold, or a hand correction):* `mustard-rt run scan-patterns-apply --path <moldPath> --content -` and `mustard-rt run scan-patterns-decline --slug <slug> --reason <one line, English>`. Same rules, same ledger — the relay is these two over a whole envelope, not a different contract. Use the relay for anything an agent returned; reach for these only when you already hold exactly one block.

## Inviolable

- The deterministic pass NEVER calls AI and NEVER reads source; it always writes `grain.model.json` + every unit's `.claude/scan-map.md` (workspace root included), preserving `## Guards`. Only the Guards ENRICH excludes the root.
- Enrichment is STANDARD + fail-open — no opt-in flag, no confirmation, no dollar cost EVER. If a pass can run it runs silently; if it cannot it skips silently.
- An LLM in enrichment writes exactly TWO things: subproject `## Guards` (capped ~6 lines, non-destructive) and `{role}-pattern` molds — swept fresh each scan (every `source: scan` mold is deleted then re-authored; a `source: manual`/hand-authored mold is NEVER touched). Never the root, never source, never system prompts.
- **NEVER write a script to work around a rough edge in this flow** (no prompt builder, no worklist splitter, no temp-file shuttle). Every step here is a `mustard-rt run …` command: render the prompt, pipe the block on stdin, one agent per subproject. A script is a SYMPTOM — it hides the defect, and it silently drops the contracts these commands carry (that is how molds come back missing `metadata`, and a mold whose frontmatter is broken is never swept again: it blocks its cluster forever). Hit friction → fix the tool or this file, then re-run.
