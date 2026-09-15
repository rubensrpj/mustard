---
description: An internal flow — not a door. The map is updated by `mustard-rt run scan`, which reads only what changed and never writes to git; nothing runs it on its own. This flow is the FULL pass (model + subproject maps) the router runs when the census needs re-authoring after a large change. It never writes a `CLAUDE.md`.
argument-hint: [--root <dir>] [--out <path>]
user-invocable: false
---
<!-- mustard:generated -->
# scan — Codebase model

**This is not a door.** The deterministic census is updated by `mustard-rt run scan`, which reads only what changed and never writes to git; nothing runs it on its own. The router reaches this flow when the census needs re-authoring, never the user typing a command.

**Git — the scan never writes to it.** It stages nothing, commits nothing and needs no clean tree: what it writes stays outside git, so it never mixes with your work. A re-scan over unchanged code is byte-stable.

## The model and the maps (no AI, you do NOT read source)

```bash
mustard-rt run scan --full [--root <dir>] [--out <path>]
```

Writes `.claude/grain.model.json` (the language-agnostic model — modules, declarations, dependency graph, mined roles, vertical slices, shared contracts, touchpoints) AND regenerates the mustard-owned map file `<unit>/.claude/scan-map.md` for EVERY unit — each subproject and the workspace root alike. Only the files that changed since the last pass are read again (`read` in the JSON lists them; `full: true` means every file was read). **No `CLAUDE.md` is ever written** (nor a `CLAUDE.local.md`): those files belong to the project. Downstream asks the map with `mustard-rt run map` and `mustard-rt run feature` — never by reading it directly. Parse the JSON (`{ ok, model, full, read, files, regenerated?, over_cap? }`); a non-empty `over_cap` means a RUNAWAY machine map (generator bug — surface it), never oversized human prose. `ok:false` with `reason: "hollow-submodules"` + `empty_submodules[]` means a submodule is declared but not checked out: the model would silently omit that whole subproject, so nothing was mined and the previous model is intact — run `git submodule update --init --recursive` and re-run. (A worktree cut by the plugin populates them for you; this catches the ones cut out of band.)

## Inviolable

- The deterministic pass NEVER calls AI and NEVER reads source; it always writes `grain.model.json` + every unit's `.claude/scan-map.md` (workspace root included) and never a `CLAUDE.md`.
- **NEVER write a script to work around a rough edge in this flow.** Every step here is a `mustard-rt run …` command. A script is a SYMPTOM — it hides the defect, and it silently drops the contracts these commands carry. Hit friction → fix the tool or this file, then re-run.
