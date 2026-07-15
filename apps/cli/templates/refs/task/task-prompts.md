# Task Dispatch — Render Invocations Reference

> Detail for `/task`: the concrete `agent-prompt-render` (+ scan digest) invocations per action. Prompts are **never hand-assembled** — this is the same inviolable rule `/feature` and `/tactical-fix` follow. The orchestrator runs the render command, then passes its **stdout verbatim** as the Task `prompt`. With `--emit ref` that stdout is a 2-line stub (the full prompt goes to a `.dispatch/` file) — still verbatim; the PreToolUse hook expands it at dispatch, so the full text never transits the orchestrator's context.

`/task` is spec-less: there is no `wave-plan.md` and no dispatch round. The render is driven directly by the action (`--role`) and the scope (`--spec` / `--subproject`). Every placeholder fail-opens, so a spec-less invocation is safe — empty slots simply render blank.

## Step 1 — locate via the scan digest (once per request)

LOCATE first — the same digest `/feature` and `/bugfix` run. **Dispatching a `/task` agent without it is the most common cause of an empty result:** the agent then searches the whole repo from zero instead of starting from the ~12 anchors the digest points to. `context-slice` is **not** run here — keyed on a spec it cannot use spec-less (it returns blank), the digest's anchors are the real locator.

```bash
mustard-rt run feature --intent "<lapidated code-shaped terms + the user's content words>"
```

**Lapidate the request into code-shaped terms yourself**: strip the glue (prepositions/articles — content words only), translate into the code's vocabulary, and shape it how code NAMES things — verbs infinitive (`create`/`list`), collection nouns plural (`clients`/`contracts`/`receivables`); when unsure, include both forms (the digest dedups). Code-shaped terms hit the **EXACT** tier, not `stem` (where the noise lives). The digest is **pure deterministic** — it matches the **distinct union**. **Prune by provenance, then read ONLY the survivors:** `anchorsDetail` shows each anchor's matched terms — drop the tangential (a seeder on `pagos`), keep the central. On `reason ∈ {weak,none,generated_only}` the digest returns a `candidates` array (the real code vocabulary as `{term,samples,count}`) — sharpen your translation and re-call, or fall back to direct Glob+Grep. Persist a confirmed bridge via `mustard-rt run equivalence-learn --term <missed> --tokens <code-terms>`, so it becomes deterministic over time. Fold the surviving anchor paths into the render's `--task-text` (Step 2) so the agent starts from them. The subproject `## Guards` ride in separately as `{guards_summary}` — handled by the renderer, not a `--context` source here.

## Step 2 — render the dispatch prompt per action

`--mode first` is the dispatch (non-retry) render; swap to `--mode granular` / `--mode fix-loop` on a retry. No size budget — relevance is the only filter on what the renderer injects (the spec-memory gate, the relevance-sliced context); nothing is trimmed by token count.

| Action | `--role` | `subagent_type` | Render invocation |
|--------|----------|-----------------|-------------------|
| `analyze` | `explore` | `Explore` | `mustard-rt run agent-prompt-render --spec {scope} --role explore --subproject {subproject} --mode first --emit ref` |
| `review` | `review` | `mustard-review` | `mustard-rt run agent-prompt-render --spec {scope} --role review --subproject {subproject} --mode first --emit ref` |
| `docs` | `docs` | `general-purpose` | `mustard-rt run agent-prompt-render --spec {scope} --role docs --subproject {subproject} --mode first --emit ref` |
| `audit` | `audit` | `general-purpose` | `mustard-rt run agent-prompt-render --spec {scope} --role audit --subproject {subproject} --mode first --emit ref` |
| `refactor` (plan) | `plan` | `Plan` | `mustard-rt run agent-prompt-render --spec {scope} --role plan --subproject {subproject} --mode first --emit ref` |
| `refactor` (execute) | `implement` | `general-purpose` | `mustard-rt run agent-prompt-render --spec {scope} --role implement --subproject {subproject} --mode first --emit ref` |
| `implement` | `implement` | `general-purpose` | `mustard-rt run agent-prompt-render --spec {scope} --role implement --subproject {subproject} --mode first --emit ref` |

### Dispatch shape

For each rendered prompt:

```text
Task({
  subagent_type: <from table>,   // per role: read-only roles run tool-restricted
  description: `{action}: {scope}`,
  prompt: <stdout of agent-prompt-render, verbatim>
})
```

No `model` field — dispatched agents inherit the session model (`pipeline-config.md § Model`).

Every render also passes `--task-text "<the action's work>"` — `/task` is spec-less, so the action's task rides in via that flag (the renderer folds it into `## TASK`); never hand-append the task after the render.

`subagent_type` is picked per role: `explore`→`Explore`, `review`→`mustard-review` (both read-only — no Edit/Write); writing roles (`audit` / `docs` / `implement`) → `general-purpose`. The render carries the role contract inline.

## Per-action notes

- **audit** — first load the `improve-codebase-architecture` skill and select the domain checklist (`copy` / `design` / `a11y` / `i18n` / `consistency` / `api-contract`; default `consistency`). The checklist is the *task description* the auditor works through; the rendered prompt carries the guards + standardization context.
- **compare** — render one prompt **per subproject** (each with its own `--subproject`, `--role explore`) and dispatch them PARALLEL in a single message. Then render a consolidation prompt (`--role plan`) that merges the explorer results and surfaces discrepancies.
- **refactor** — two-phase: render+dispatch the `plan` role, print the plan verbatim, AskUserQuestion (Approve / Adjust / Cancel), then on approval render+dispatch the `implement` role.
- **implement** — single dispatch, return cap ≤30 lines (Files Changed / Build result / Status). ON CONCERN → surface + offer `/feature` Light.

Persistent tracking is **N/A** — `/task` is spec-less by design. Promote to `/feature` Light or `/tactical-fix` if a tracked spec is needed.
