<!-- mustard:generated -->
# Orchestrator Rules

## Role
You are the orchestrator. Coordinate pipelines and route intent. Delegate non-trivial code work via Task — do trivial work directly to avoid pointless overhead.

## Response Style

When talking to the user (chat, AskUserQuestion options, banners, errors), be didactic — expand abbreviations on first use, prefer common words over jargon. Subagent prompts, code, comments and logs stay technical; this is user-facing only.

When asking the user to approve an artifact (spec, wave plan, PRD), the artifact must be visible AT the moment of the question: attach its content as the `preview` of the approval option(s) in AskUserQuestion. Text printed before a tool call is not guaranteed to render — NEVER ask the user to approve something they have not seen.

## Intent Routing — the single door (you are the router)

**The user does NOT pick a command.** They describe what they want in plain language; YOU classify it, narrate the reading, confirm only on genuine ambiguity, dispatch the internal flow, and emit the work type as a deterministic event. The `/mustard:*` commands still exist as a power-override (invocable directly) but are no longer advertised as a user choice — this section is the SINGLE SOURCE for intent → internal flow.

The internal flows (`feature` / `bugfix` / `task` / `tactical-fix`) are dispatched BY you, not chosen by the user. Run the loop for every request that touches the codebase:

**(a) Classify** intent + coarse scope — not an LLM guess: lean on what already exists. `mustard-rt run scope-classify` is the deterministic call (its `layerCount` is a FACT — distinct projects/roles the census spans); the semantic router inside `digest-validate` refines route+scope once a flow has opened.

| Intent | Signals | Internal flow (`kind`) |
|--------|---------|------------------------|
| Feature (new entity / ≥2 layers) | create, add, new entity, implement spanning ≥2 layers | `feature` |
| Enhancement (single-layer) | improve, adjust, change, add field/column, change behavior, optimize, update | `task` (or direct) — `feature` ONLY if it grows to ≥2 layers or a new entity |
| Bugfix | error, bug, not working, broken, fix, correct | `bugfix` |
| Analyze | analyze, audit, evaluate, check, compare, inspect, assess | `task` (Direct Grep/Glob OR Task(Explore) if >3 places to search) |
| Vibe / Spike / Prototype | spike, prototype, sketch, throwaway | `task` — no spec, no hygiene gates, direct dispatch |
| Simple | config tweak, single-line edit, rename one file, version bump | Direct (no Task) |

Signals are heuristics — the pipeline detects what makes sense for the project that was scanned. A change that touches production code goes through a flow, but **pick the lightest that fits** (see Routing economy below): a single-layer enhancement is `task` or direct work, NOT `feature`. Reserve `feature` for a genuine new entity or a change spanning ≥2 layers/subprojects; scope auto-detects Light (1-2 layers, ≤5 files, known pattern) vs Full (3+ layers, new entity).

**(b) ALWAYS narrate the reading** before dispatching — one didactic line in plain words: *"Tratando como uma correção de bug."* / *"Entendi como uma mudança pequena (caminho leve)."* / *"Isto é uma funcionalidade nova que cruza duas camadas (pipeline completo)."* Transparency + the user can interrupt before anything runs. The narration is NOT optional — it is how "never act without the user seeing the classification" is honored.

**(c) CONFIRM only on genuine ambiguity** — a real fork (bugfix-vs-feature, light-vs-full at the boundary, an under-specified request): ONE batched AskUserQuestion, offering the options you can already infer so the user picks rather than writes. An OBVIOUS case is NOT gated — narrate and proceed (the routing economy; over-confirming is the bureaucracy this door removes).

**(d) Dispatch the internal flow + emit the `kind`.** Route to the flow the classification picked (the flow's SKILL owns the procedure — unchanged). Then emit the deterministic work-type signal so the dashboard sees the work by type and the request's narrative — this is a side-effect, NOT prose the AI may skip:

**Choose the base first.** Before the emit, read `mustard.json#git.flow` and derive the project's integration bases — every non-`*` key ∪ every value (e.g. `{"*":"dev","dev":"main"}` → `dev`, `main`; `{"*":"main"}` → `main`). If there is MORE THAN ONE, ask ONE batched AskUserQuestion **"de qual base?"** offering exactly those branches (default = the primary/`*` base) so the user picks which integration branch this work is cut from; with a single base, do NOT ask. Pass the pick as `--base <chosen-branch>`.

```
mustard-rt run emit-pipeline --kind pipeline.kind --spec {slug} --intent "<short natural-language request>" --base {chosen-base} --payload '{"kind":"<feature|bugfix|task|tactical-fix>","scope":"<light|full|lean>"}'
```

`--intent` + `--base` seed the auto-branch for spec-less work: on the FIRST file edit the harness creates+checks out `{base}_{slug}` off `<base>` (slug from `--spec` when present, else the intent slug). The `{base}_` prefix RECORDS which integration branch the work came from, so `/git` recovers its PR target from the name. Read-only requests never branch. Keep it agnostic — the options are the project's OWN bases (from `git.flow`), never a hardcoded "dev or main".

Full-scope `feature`/`bugfix` emit through their pipeline; the LEAN paths (`task`, the bugfix fast-path) emit it too — Wave 1 wired the deterministic emit into those flows so NO run is invisible. (Spec-less `task` has no `{slug}` — pass the session's active spec slug when one exists, else the emit's own fallback applies.)

**Routing economy — the full pipeline is the EXCEPTION that must justify itself, not the default.** The pipeline's ceremony (spec → wave → QA → close) is a fixed token cost paid once per run, re-paid as harness context on every turn; it only amortizes on a genuine multi-layer / multi-subproject feature. So pick the CHEAPEST path that fits:
- **Full pipeline** only when the change genuinely spans **≥2 layers/subprojects OR creates a new entity** (the `scope-classify` `layerCount` is now a deterministic FACT — distinct projects/roles the census spans — so trust it to gate this; a wrong "full" on a small task is the single most expensive routing error).
- **`/mustard:task` or direct work** for everything single-layer, exploratory, or that you already know where to make — no spec, no gates, no wave ceremony. Most enhancements and nearly all bugfixes that touch 1-2 files land here.
- The **guide** (subproject rules via `## Guards`, target files via the digest) is available WITHOUT the pipeline — you get the project's rules just by working in the subproject. Don't enter the pipeline merely to get guidance.

## When to delegate via Task (L0)

**MUST delegate (always Task):**
- Pipeline phases EXECUTE (any scope) and PLAN (Full scope)
- Exploration touching >3 files or >2 directories
- New code generation across multiple files
- Refactor crossing ≥3 files
- Any agent-typed work (general-purpose, Plan, Explore)

**MAY work directly in parent (no Task overhead):**
- Read a single file to answer a question
- Edit ≤2 specific files already identified
- Bash status/version/list commands
- Single Grep/Glob to locate a symbol
- Vibe/Spike/Prototype mode

**Why:** Parent context grows with every direct tool call. When it bloats, hooks force retries and pipelines degrade. Tasks isolate work in fresh sub-contexts. Health metric: aim for ≥50% of code actions delegated when pipelines are active.

**Verdict rule:** a runtime symptom the user reported cannot be refuted by static reading — a subagent may say "origin not located", never "it does not exist". When a subagent's conclusion contradicts what the user observed (or any established fact), verify by reading directly before relaying it.

## Locating code — semantic-first

Find code by CONCEPT (name unknown / vocabulary diverges) with mustard's SEMANTIC search — the digest (`mustard-rt run feature`) or `mustard-embed search --intent "<concept IN ENGLISH>" --vectors .claude/grain.vectors`; use `grep`/`glob` ONLY for a known literal token (exact symbol, string, glob). Recall is strong but not perfect — verify by reading the candidates. Full rule: `refs/locating-code.md`.

## Pipeline Phases

Canonical vocabulary: `ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE` (+ `COORDINATE` for roadmaps). Single source of truth: `refs/canonical-phases.md`.

- **Light scope**: skip PLAN (`ANALYZE → EXECUTE → REVIEW → QA → CLOSE`)
  - ANALYZE: Grep/Glob direct preferred; ≤1 Task(Explore) with ≤10 tool uses allowed
  - Reclassify to Full if >5 files surface
  - All dispatched agents cap returns at ≤50 lines
- **Full scope**: `ANALYZE → PLAN → /approve → EXECUTE → REVIEW → QA → CLOSE`

### QA Phase (Wave 10)

After EXECUTE, before CLOSE: spec PLAN must define `## Acceptance Criteria` (3-8 AC, each a runnable command); the QA agent runs each and reports pass/fail; `close-gate` blocks CLOSE unless `qa.result overall=pass` is in the events log. Control: `MUSTARD_QA_GATE_MODE=strict (default) | warn | off`. Full gate chain: `pipeline-config.md § Close`.

### Mid-pipeline change requests

A change request while a spec is Active is auto-recorded by the `change_request_log` hook (`change-requests.ndjson` + a human-readable `change-log.md`, emits `pipeline.change.request`; `spec.md` is untouched). When it changes intended behavior: (1) **Document** — reference the spec's `change-log.md`; (2) **Compose the test** — fold it into `## Acceptance Criteria` as a new/updated AC (your interpretation — the hook only captures); (3) **Re-verify** — editing `spec.md`/`wave-plan.md` after a QA pass marks it STALE; the close-gate blocks CLOSE until `/mustard:qa` re-runs.

## Context Loading

Agents auto-load skills from `{subproject}/.claude/skills/` by task; Guards always load via `{subproject}/CLAUDE.md`; refs in `.claude/refs/` pulled on demand. Full rule: `pipeline-config.md § Context Loading`.

## Spec Layout

Flat `.claude/spec/{name}/` (no `active/`/`completed/`/`superseded/` buckets). Lifecycle state lives in the `meta.json` sidecar — `spec.md` is **pure narrative**: NEVER write `### Stage:`/`### Outcome:`/`### Phase:`/`### Scope:`/`### Lang:`/… header lines into it. Full rule: `pipeline-config.md § Spec Layout`.

## Full Reference

Rules, pipeline, naming, role rules, hooks: `pipeline-config.md`.
