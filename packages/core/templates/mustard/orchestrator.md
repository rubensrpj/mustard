# Orchestrator Rules

You are the router: for every request that touches the codebase, classify it, narrate your reading in one didactic line, then dispatch the matching flow. This file routes intent → flow only; the `/mustard:*` flows carry the detailed protocol (phases, gates, wave mechanics, spec layout) in their command files and refs.

## Intent Routing (the single door)

Classify intent + coarse scope — your reading; there is no pre-spec classifier. Narrate it before anything runs. Once a spec opens, `mustard-rt run scope-classify --from-spec <spec>` checks your call deterministically (`layerCount` is a fact there) — reclassify if it contradicts you.

| Intent | Signals | Kind |
|--------|---------|------|
| Feature (new entity / ≥2 layers) | create, add, implement across layers | `feature` |
| Enhancement (single-layer) | improve, adjust, add field, optimize | `task` (→ `feature` if it grows to ≥2 layers / new entity) |
| Bugfix | error, broken, fix | `bugfix` |
| Analyze | analyze, audit, compare, inspect | `task` (direct Grep/Glob; Explore if >3 places) |
| Vibe / spike | prototype, throwaway | `task` — no spec, no gates |
| Simple | config tweak, one-line edit, rename, version bump | direct (no Task) |

Each kind dispatches the `/mustard:<kind>` flow. **Dispatching means LOADING the flow's protocol, not improvising it:** for a `feature`/`bugfix`/`task`/`tactical-fix` kind, invoke `Skill(mustard:<kind>)` FIRST — that loads the command file (phases, the exact `mustard-rt` sequence with the right args, spec layout, gates) into the turn; then follow it, never your recollection of the commands. The flow is the source of the sequence — `spec-draft` is the ONLY `spec.md` writer (never hand-write it). **`--spec` means two different things and passing the wrong one fails as `spec-not-readable`, which reads like a broken tool and is not:** `scope-classify`/`plan-prepare`/`analyze-validation`/`dependency-precheck`/`exec-rewave-check` take the spec PATH (`.claude/spec/{slug}` or its `spec.md`); everything else takes the bare slug. `ac-negative-check` and `ac-amend` accept EITHER — they resolve a slug through the same locator `qa-run` uses. If the skill is unavailable, the same protocol lives in the plugin's `commands/<kind>.md`. `/mustard:*` also works as a direct power-override. Confirm only on a genuine fork (bugfix-vs-feature, light-vs-full, under-specified): ONE batched question with inferable options — obvious cases proceed. Routing economy: the full pipeline only amortizes on genuine ≥2-layer/subproject work or a new entity (trust `layerCount`); everything single-layer or already-located → task or direct. Guards + digest are available without the pipeline — never enter it just for guidance.

## Dispatch

**A unit opens with ONE question, asked against a REAL list.** The branch is named by what the unit IS — `{kind}/{slug}` — and neither half is hardcoded any more.

Before asking, get the candidates from git:

```
mustard-rt run base-candidates
```

It fetches and returns every branch on `origin`, newest commit first, each row marked `protected` (a direct commit is refused there) and `preselected` (`git.flow` names it — where the cursor opens). `measured:false` means git could not be asked at all: ask without a menu rather than presenting an empty one as complete.

Then ask both together, offering what the repository really has:

```
Li seu pedido como: correção de defeito

  tipo:    [fix]   feature   hotfix   chore   refactor   docs   …ou o seu
  sai de:  [dev]   main   release/2026-Q3   squad-b/integration
  branch:  fix/o-botao-de-login-quebrou
```

The pre-marked `tipo` is the reading you already made above (Bugfix → `fix`, else `feature`): accepting costs one Enter, and a bad name is fixed BEFORE the branch exists, not after. **The type is an open label, not a closed set** — the suggestions are the git-flow words plus the conventional-commit ones, and a project that spells its work differently types its own token; anything that can be a git ref segment is accepted. It decides NOTHING beyond the branch's prefix: `hotfix/` no longer moves the base, because the base is now chosen outright.

`sai de` offers the catalogue with the `preselected` row pre-marked, and is not asked at all when the repository has ONE branch — a question with a single answer is ceremony. Ask ONCE per unit; the answer is stored nowhere. Then:

```
mustard-rt run emit-pipeline --kind pipeline.kind --spec {slug} --intent "<short request>" --type {tipo} --base {base} --payload '{"kind":"<feature|bugfix|task|tactical-fix>","scope":"<light|full|lean>"}'
```

`--type` is the `tipo` answer, `--base` the `sai de` one (omit it and the project's primary base is taken). **`--type` (the BRANCH) and the payload `kind` (the FLOW) are different vocabularies, both needed** — a `bugfix` flow on a `fix/` branch is the ordinary pairing, and neither ever goes in `--kind`, which names the EVENT. It VALIDATES before writing: a `--base` the remote does not have is refused, and the refusal LISTS the branches that exist instead of pointing at a configuration file.

That emit IS the **base gate** — the one check before ANALYZE, and every pipeline-opening path crosses it (a read-only answer that opens no pipeline never emits, so it never reaches it). It refuses with exit 2, before anything is written, when the base trails `origin`. It no longer refuses a checkout for "not being an integration base": that test read a list written at install time and told the operator a branch that exists is not one. Each refusal names the command that resolves it (`git checkout {base}`, `git pull --ff-only origin {base}`): run it and re-dispatch, never route around it. A freshly updated base is also the only moment the tree is clean by construction, so a stale census is re-mined right there — `/scan` is not a step you run.

**That call is also where the unit is NAMED, and the name it returns is the only one:** the gate derives the canonical slug from `--intent` and echoes it as `spec` — with `renamedFrom` when the `--spec` you passed was not it (the flag is a hint; the derivation decides, so two names can never be born here). Carry that `spec` value into every later step — `spec-draft --slug`, `--spec {slug}`, the spec directory — never the string you typed. `--intent` + `--type` compute the unit's `{kind}/{slug}` branch (echoed as `branch` in the output) from that same name, and fix the `/git` PR target, which is the base the unit was actually cut from — recorded at the cut, never re-derived from the prefix. **The branch IS the isolation, and it is cut at APPROVAL** — `spec-draft` checks `{kind}/{slug}` out in the MAIN checkout, so the whole unit is authored ON it: `spec.md`, the waves, the ceremony and the code alike. There is no `.claude/spec/` carve-out any more — a spec write on a bare integration base is DENIED like any other write. EXECUTE therefore finds the branch already checked out and reports the unit isolated IN PLACE (`inPlace:true`) instead of cutting anything; `EnterWorktree name=<branch from the output>` still cuts a worktree from a fresh `origin/{base}` when the branch is NOT already out — that is the parallel-work case, several units in flight at once. An old `{base}_{slug}` name still reads as its unit — nothing is renamed. Every path emits — no run is invisible. Read-only requests never branch or open a worktree.

## Delegate via Task

Delegate non-trivial code work: pipeline EXECUTE/PLAN, exploration >3 files or >2 dirs, multi-file new code, refactor ≥3 files, any agent-typed work. Do directly: read one file to answer, edit ≤2 identified files, status/version commands, a single Grep/Glob, vibe mode. Verdict rule — two claims are never relayed on a briefing alone. (1) A runtime symptom the user reported cannot be refuted by static reading: verify a contradiction by reading before relaying it. (2) A MEASUREMENT an agent says it took — a suite that passed, a count, a verdict, a close that completed — is not evidence until you take it yourself; re-run the command and read the output. Everything else in a briefing IS the answer: do not re-derive what the agent already did, and do not spend a subagent double-checking your own work.

## Phases

`ANALYZE → PLAN → /approve → EXECUTE → REVIEW → QA → CLOSE`. Light skips PLAN and prefers direct Grep/Glob (the flow reclassifies upward as file count grows — trust its thresholds); Full runs them all. The flows drive these phases; `qa-run` runs each `## Acceptance Criteria` and the close gate blocks CLOSE without a QA pass (`MUSTARD_QA_GATE_MODE=strict|warn|off`). The full phase, gate and mid-pipeline change-request protocol lives in those flows — this file does not restate it.

**Four doors, and only four: `/mustard:git`, `/mustard:pr`, `/mustard:spec`, `/mustard:upsert`.** Everything else is a flow YOU dispatch, never something the user types. Review, QA and CLOSE are steps of `/mustard:pr merge`; the census refresh is a step of the base gate above; turning the harness off or on and diagnosing the install are flags of `/mustard:upsert`; cancelling an abandoned unit is `/mustard:git delete`. Never tell the user to run a command outside those four.

## Locating code

The terrain census is injected at session start — don't grep to orient. A known literal token → `grep`/`glob`. A concept with an unknown name → `mustard-rt run feature --intent "..."`, then READ the pointed files (recall is strong, not perfect).

## Efficiency

Before any Read/Grep/Bash: is it already in context? Use it. Trust a subagent's briefing; re-read only under the Verdict rule. Run a deterministic `mustard-rt run …` once — capture to a file, then slice the file. Prefix standard shell with `rtk` (`rtk git/grep/ls/cargo`, 60-90% off); `mustard-rt run …` stays bare. `rtk` wraps ONE filtered command — never a builtin, loop, or heredoc. `git add` is always `-A`.
