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

Each kind dispatches the `/mustard:<kind>` flow. **Dispatching means LOADING the flow's protocol, not improvising it:** for a `feature`/`bugfix`/`task`/`tactical-fix` kind, invoke `Skill(mustard:<kind>)` FIRST — that loads the command file (phases, the exact `mustard-rt` sequence with the right args, spec layout, gates) into the turn; then follow it, never your recollection of the commands. The flow is the source of the sequence — `spec-draft` is the ONLY `spec.md` writer (never hand-write it). **`--spec` means two different things and passing the wrong one fails as `spec-not-readable`, which reads like a broken tool and is not:** `scope-classify`/`plan-prepare`/`analyze-validation`/`dependency-precheck`/`exec-rewave-check` take the spec PATH (`.claude/spec/{slug}` or its `spec.md`); everything else takes the bare slug. `ac-negative-check` and `ac-amend` accept EITHER — they resolve a slug through the same locator `qa-run` uses. If the skill is unavailable, the same protocol lives in the plugin's `commands/<kind>.md`. `/mustard:*` also works as a direct power-override. Confirm only on a genuine fork (bugfix-vs-feature, light-vs-full, under-specified): ONE batched question — obvious cases proceed. Routing economy: the full pipeline only amortizes on genuine ≥2-layer work or a new entity (trust `layerCount`); single-layer or already-located → task or direct. Guards + digest need no pipeline — never enter it just for guidance.

## Dispatch

**A unit opens with ONE question, asked against a REAL list.** Get the candidates from git first — `mustard-rt run base-candidates` fetches and returns every branch on `origin`, newest first, each marked `protected` (a direct commit is refused there) and `preselected` (`git.flow` names it — where the cursor opens). `measured:false` means git could not be asked: ask without a menu rather than showing an empty one as complete. Then ask the rows together, **`sai de` FIRST**: the operator settles where the unit STARTS before what it is CALLED, and a type shown above the base makes the base read as the type's consequence — the exact implication this product removed when it began choosing the base against a real catalogue.

```
Li seu pedido como: correção de defeito

  sai de:  [dev]   main   release/2026-Q3   squad-b/integration   …ou o seu
  tipo:    [fix]   feature   hotfix   chore   …ou o seu
  branch:  [fix/o-botao-de-login-quebrou]   …ou corrija o nome
```

**The rows are INDEPENDENT fields; asking them together NEVER means pairing them.** Never render combined options (`fix saindo de dev` / `hotfix saindo de main`): a pair-list hands back the cartesian product of two choices and revives the type→base implication through the back door, because the operator who wants `hotfix` cut from the ordinary base finds no row to pick. **A question surface takes at most 4 options per field, plus the free one** — so each row offers FOUR and everything else is typed: `sai de` shows `preselected` plus the newest of the catalogue (any other branch on the list is typed in full), and on `tipo` the token `hotfix` is PINNED — it is never the suggestion dropped to fit the ceiling, since that row exists precisely so an emergency can be named. A ceiling the prose does not name is a ceiling the reader discovers by getting it wrong in front of the operator.

The pre-marked `tipo` is the reading you made above (Bugfix → `fix`, else `feature`): a bad name is fixed BEFORE the branch exists. **The type is an OPEN label** — the suggestions are conventional, and any token that can be a git ref segment is accepted. It decides nothing beyond the prefix: `hotfix/` no longer moves the base, because the base is chosen outright. `sai de` offers the catalogue with `preselected` pre-marked, and is skipped when the repository has ONE branch. Ask ONCE per unit; the answer is stored nowhere.

**`branch` is a CORRECTABLE field, not a notice.** It shows the suggestion — `{tipo}/{name derived from the request}` — and an Enter accepts it; the operator who reads that name and rewrites it on purpose is the one person who knows what the unit should be called, and that correction wins. Editing the row is editing `tipo` + name in ONE string: split the answer at the first `/`, the head replaces the `tipo` answer and the tail is the corrected name. There is no third, free-standing name — `{tipo}/{slug}` has a single spelling in the code, and a branch field allowed to disagree with `tipo` would resurrect the two-names defect. An untouched row is silence, and silence still means derived. Then:

```
mustard-rt run emit-pipeline --kind pipeline.kind --spec {slug} --intent "<short request>" --type {tipo} --base {base} --payload '{"kind":"<feature|bugfix|task|tactical-fix>","scope":"<light|full|lean>"}'
# …and ONLY when the operator corrected the `branch` row, append: --unit-name {name}
```

`--type` is the `tipo` answer, `--base` the `sai de` one (omit it and the primary base is taken). **`--type` (the BRANCH) and the payload `kind` (the FLOW) are different vocabularies, both needed** — a `bugfix` flow on a `fix/` branch is the ordinary pairing, and neither goes in `--kind`, which names the EVENT. Kind→type, no hole: `feature`,`task`→`feature`; `bugfix`,`tactical-fix`→`fix` (`hotfix` only off the ordinary base — that fork is YOURS). Omitted `--type` is no silent default: on the ordinary base the gate derives it from the payload `kind` and echoes `type`+`typeFrom`; elsewhere, or with no routing kind, it REFUSES — a silent default may not name a durable artefact. A `--base` the remote lacks is refused, LISTING the branches that exist. **`--unit-name` is the operator's correction, and the ONLY signal that outranks the name derived from `--intent`** — pass it when, and only when, the `branch` row came back edited; with no edit the flag is absent and the name stays derived. It is explicit precisely so the two cases never blur: `--spec` remains a caller's guess and still loses, while `--unit-name` is a person overruling a suggestion they read. The report says which side named the unit — `nameFrom` is `derived-from-intent` or `operator`.

That emit IS the **base gate** — the one check before ANALYZE, and every pipeline-opening path crosses it (a read-only answer that opens no pipeline never emits, so it never reaches it). It refuses with exit 2, before anything is written, when the base trails `origin`. It no longer refuses a checkout for "not being an integration base": that test read an install-time list and called a real branch not-a-base. Each refusal names the command that resolves it: run it and re-dispatch, never route around it. A freshly updated base is the one moment the tree is clean by construction, so a stale census is re-mined right there — `/scan` is not a step you run.

**That call is also where the unit is NAMED, and the name it returns is the only one:** the gate derives the canonical slug from `--intent` — or canonicalises the operator's `--unit-name` through that same derivation, which is why a correction still yields ONE spelling — and echoes the winner as `spec`, with `renamedFrom` when the `--spec` you passed was not it (that flag is a hint; it never decides). Carry that `spec` value into every later step — `spec-draft --slug`, `--spec {slug}`, the spec directory — never the string you typed. `--intent` + `--type` compute the unit's `{kind}/{slug}` branch (echoed as `branch` in the output) from that same name, and fix the `/git` PR target, which is the base the unit was actually cut from — recorded at the cut, never re-derived from the prefix. **The branch IS the isolation, and it is cut at APPROVAL** — `spec-draft` checks `{kind}/{slug}` out in the MAIN checkout, so the whole unit is authored ON it: `spec.md`, the waves, the ceremony and the code alike. There is no `.claude/spec/` carve-out any more — a spec write on a bare integration base is DENIED like any other write. EXECUTE therefore finds the branch already checked out and reports the unit isolated IN PLACE (`inPlace:true`); `EnterWorktree name=<branch from the output>` still cuts a worktree from a fresh `origin/{base}` when the branch is NOT already out — the parallel-work case. An old `{base}_{slug}` name still reads as its unit. Every path emits. Read-only requests never branch or open a worktree.

## Delegate via Task

Delegate non-trivial code work: pipeline EXECUTE/PLAN, exploration >3 files or >2 dirs, multi-file new code, refactor ≥3 files, any agent-typed work. Do directly: read one file to answer, edit ≤2 identified files, status/version commands, a single Grep/Glob, vibe mode. Verdict rule — two claims are never relayed on a briefing alone. (1) A runtime symptom the user reported cannot be refuted by static reading: verify a contradiction by reading before relaying it. (2) A MEASUREMENT an agent says it took — a suite that passed, a count, a verdict, a close that completed — is not evidence until you take it yourself; re-run the command and read the output. Everything else in a briefing IS the answer: do not re-derive what the agent already did, and do not spend a subagent double-checking your own work.

## Phases

`ANALYZE → PLAN → /approve → EXECUTE → REVIEW → QA → CLOSE`. Light skips PLAN and prefers direct Grep/Glob (the flow reclassifies upward as file count grows — trust its thresholds); Full runs them all. The flows drive these phases; `qa-run` runs each `## Acceptance Criteria` and the close gate blocks CLOSE without a QA pass (`MUSTARD_QA_GATE_MODE=strict|warn|off`). The full phase, gate and mid-pipeline change-request protocol lives in those flows — this file does not restate it.

**Four doors, and only four: `/mustard:git`, `/mustard:pr`, `/mustard:spec`, `/mustard:upsert`.** Everything else is a flow YOU dispatch, never something the user types. Review, QA and CLOSE are steps of `/mustard:pr merge`; the census refresh is a step of the base gate above; turning the harness off or on and diagnosing the install are flags of `/mustard:upsert`; cancelling an abandoned unit is `/mustard:git delete`. Never tell the user to run a command outside those four.

## Locating code

The terrain census is injected at session start — don't grep to orient. A known literal token → `grep`/`glob`. A concept with an unknown name → `mustard-rt run feature --intent "..."`, then READ the pointed files (recall is strong, not perfect).

## Efficiency

Before any Read/Grep/Bash: is it already in context? Use it. Trust a subagent's briefing; re-read only under the Verdict rule. Run a deterministic `mustard-rt run …` once — capture to a file, then slice the file. Prefix standard shell with `rtk` (`rtk git/grep/ls/cargo`, 60-90% off); `mustard-rt run …` stays bare. `rtk` wraps ONE filtered command — never a builtin, loop, or heredoc. `git add` is always `-A`.
