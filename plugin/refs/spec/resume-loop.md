# /mustard:spec — Resume loop (approve → dispatch → close)

Drives a spec from PLAN through CLOSE. `${CLAUDE_PLUGIN_ROOT}/commands/spec.md` §3 routes here by `resume-bootstrap` stage:

- **`Plan`** → **§A Approve** (then, if approved-inline, fall straight into §B).
- **`Execute` / `Analyze` / `QaReview` / `QaPending` / `ReviewPending` / `Close`** → **§B Loop**.

The binary owns every deterministic decision (wave order, routing, prompts, mode, nextAction). This ref is a **relay** — run the command, do what its output says. Your parts are marked **[you]**.

## Contents

**§A** Approve gate · **§B** The loop · **Escalation** · **Fix Loop** · **Wave failure & retry** · **Inviolable**

---

## §A — Approve gate (stage = Plan)

A spec has two layers — `## PRD` (what & why) + `## Plan` (how). Approving approves **both at once** — no separate PRD gate.

**Already approved — skip re-approval (avoids the double gesture).** If `resume-bootstrap` returned `approvedByUser: true`, the user already chose **Aprovar** in the approval question and the approval witness recorded it in `spec.ndjson` — `approve-spec` reads the same state. Do **NOT** re-present the plan or re-ask the approval — that is the redundant second gesture. This is the clean window the approval pointed to: emit the relay below with `--resume` and fall into §B. Everything else in §A below is for a plan **not yet** approved.

**A typed picker letter approves nothing.** `/mustard:spec a`, `/mustard:spec ar`, `/mustard:spec r` and the bare `/mustard:spec` inside the unit's own work branch only choose WHICH spec is open, and accepting plan mode (`ExitPlanMode`) approves nothing either — accepting a plan can be about anything. A plan not yet approved always goes through the one approval question below.

**[you] Model/effort belongs to the APPROVAL surface — say it there, never mid-run.** The implementer runs as a `general-purpose` subagent that **inherits the session's model AND effort**, so choosing a different tier for this implementation is the user's own native `/model` / `/effort` (session-level — both reach the inheriting impl agent, and `/effort` is the ONLY way to vary effort: it cannot be scoped per-dispatch). Carry ONE line into the surface that asks for the approval — the `preview` of the **Aprovar** option: `Implementação herda o modelo/effort da sessão. Para trocar, rode /model e/ou /effort ANTES de aprovar.` When the round is mechanical (prose, a rename, a single-file edit whose shape the spec already fixes), say so in that same line and name the cheaper tier: lower effort holds quality at a fraction of the tokens, and the tier is worth stepping UP only for the demanding rounds. Recommend, never decide — the lever is the user's. **An `approvedByUser:true` already carried the approval, so ask NOTHING**: the user has finished deciding, and there is no surface left to put the line on. This used to be a checkpoint inside §B, raised after the first `wave-advance` and before the first dispatch — so the operator said "go ahead", the pipeline started, and then STOPPED to ask something that fitted on the very screen where they said it. One surface, one moment, no stopping. Do **not** plumb a per-dispatch model or persist a custom field — the docs expose no user-facing per-invocation lever. Review/scan agents are unaffected.

**Is it a wave plan?** Check for `.claude/spec/{spec}/wave-plan.md`.

**Wave plan exists:**
1. `mustard-rt run event-projections --view pipeline-state --spec {spec}` → snapshot (`isWavePlan:true`, `totalWaves`, `currentWave`, `completedWaves`).

   **Read `neverDispatched` off the `resume-bootstrap` output you already have** (`${CLAUDE_PLUGIN_ROOT}/commands/spec.md` §3 ran it). `true` means the plan was scaffolded and NOBODY ever dispatched a wave — `currentWave: 1` there is a starting position, not progress. Say *"{totalWaves} ondas — nenhuma despachada ainda"*, never *"na onda 1"*: the two read the same and ask for opposite actions (start it versus resume it), and the wave directories alone cannot tell them apart.
2. Print the full `wave-plan.md` as a fenced block; list each wave-spec path below.
3. **Advisory audits (non-blocking).** Two deterministic wave-plan lints; each WARNS, neither blocks:
   - **Size:** `mustard-rt run wave-size-check --spec-dir .claude/spec/{spec}`. On `action:"audited"` the audit has TWO ends, and the plan is only healthy between them — print both:
     - `oversizedCount>0` → one `⚠ Wave {N} ({folder}) — {files} files, {tasks} tasks, {layers} layer(s): grande demais, DIVIDA` per wave carrying `oversized:true`. A wave is ONE agent in ONE pass; split off the tasks that share no file with the rest.
     - `undersizedCount>0` → one `⚠ Wave {N} ({folder}) — {files} files, {tasks} tasks: pequena demais, DOBRE numa vizinha` per wave carrying `undersized:true`. A dispatch costs a rendered prompt, an agent pass and a report — more than the work it carries. **Never** answer this one by splitting further: it is the opposite excess, and chasing the ceiling alone once took a plan from 4 waves to 14, ending with a wave of one file and one task.

     Silent when both counts are `0`. Both ends are advisory and neither blocks; the thresholds are `MUSTARD_WAVE_SIZE_LIMIT` / `MUSTARD_WAVE_TASK_LIMIT` (ceiling, default 10 each), `MUSTARD_WAVE_LAYER_FLOOR` (the file mass below which the `multi-layer` reason is not raised for a wave, default 6) and `MUSTARD_WAVE_MIN_FILES` / `MUSTARD_WAVE_MIN_TASKS` (floor, default 2 each).
   - **Overlap:** `mustard-rt run wave-overlap-check --spec-dir .claude/spec/{spec}`. On `action:"audited"` + `overlapCount>0`, print one `⚠ Waves {a}+{b} (level {level}) both edit: {files} — {chain}` per overlap — dispatch-parallel waves declaring the same file, with the one edge that zeroes the pair. This is a RE-READ, not the gate: `plan-materialize` already REFUSES the same condition off the plan JSON (exit 2, `sharedFiles.ok:false`), so a plan that reached approval should come back `overlapCount:0`. A non-zero here means the layout on disk drifted from the plan that was materialised — fix `plan.json` and re-materialise, never the wave specs by hand. Silent otherwise.
4. **[you]** Present for approval — ONE `AskUserQuestion`, *"Aprovar esta spec?"*, with two options: **Aprovar** (recommended; attach `wave-plan.md` as its `preview` — never ask approval for a plan the user cannot see) and **Ajustar**. **Say WHICH GESTURE COUNTS in the same message that presents the plan, before the question is answered** — a gate that accepts one specific gesture has to name it before it asks, or the operator spends the gesture and learns it did not count one step too late: choosing the option **Aprovar** — that label exactly, or **Approve** in an English project — is the approval, and nothing else is — free text typed into the `Other` row / notes field approves nothing whatever words it carries, and neither do plan mode nor a typed picker letter. The approval witness records the choice in `spec.ndjson`; when it records nothing, it says why in your context. It acts on this question only — asked with exactly this text, or *"Approve this spec?"* in an English project; any other question approves nothing — and, before recording, it runs the same preconditions `approve-spec` checks: the authored narrative.
   - **Aprovar** → the witness records the approval — no command — and tells you to suggest `/clear`. Say `Spec aprovada. Limpe a conversa com /clear e rode /mustard:spec {name}: a execução começa numa janela limpa.` and STOP: do not dispatch in this window. The next `/mustard:spec {name}` arrives with `approvedByUser: true` and takes the shortcut above.
   - **Ajustar** → ask what to adjust, then present again and ask the same question. A decomposition the user rejects → `mustard-rt run wave-collapse --spec {spec} --mode {full|light}` (mode = the spec scope); act on its JSON. It merges in order, de-dups, writes-before-delete, patches sidecars. **Full** ⇒ a single `wave-1-{role}/` (Full ⇒ ≥1 wave — `block_full_without_wave` enforces it); **Light** ⇒ one `spec.md`, drops `wave-plan.md` + wave dirs. A plan to redo from scratch → stop; tell the user: `Delete .claude/spec/{spec}/ and re-run /feature {name} with explicit guidance.`
5. When the approval is emitted (the shortcut above), it operates on the **wave-1 spec** — pass `--wave-plan`.

**Not a wave plan:** print a header (`**{spec}** — PLAN` + `{specSummary}`), then present the same way — the one approval question, with the spec body as the `preview` of **Aprovar**.

**Emit the approval (single relay, only when `approvedByUser: true`):** `mustard-rt run approve-spec --spec {spec} [--wave-plan] --resume`. Act on its JSON (`{ok,spec,approved,resumed}`; on `{ok:false,error}` surface + stop). It emits `pipeline.stage{Plan}` + `pipeline.status{draft→approved}`, patches `meta.json` (never hand-edit `spec.md`), and — with `--resume` — also `pipeline.stage{Execute}`.

**[you]** then: (a) optionally record ≤3 decisions via `mustard-rt run emit-event --event decision --spec {spec} --payload "title=…" --payload "rationale=…"`; (b) one `TaskCreate` per agent; (c) print `[v] ANALYZE [v] PLAN [>] EXECUTE [ ] CLOSE`.

- `--resume` already emitted Execute (do NOT re-emit). Say `Spec aprovada. Implementando.` and fall into **§B**.

---

## §B — The loop (stage = Execute / post-approve)

Routing, order and prompts are **decided by Rust** — never read `wave-plan.md` or assemble the loop by hand.

**Arriving from inside the unit's own branch — no ceremony.** `resume-bootstrap` reports `insideWorkBranch: true` when the checkout already IS this spec's own branch — the branch is READ, never rebuilt: its slug half is taken off whichever shape the name carries (`{kind}/{slug}`, or the older `{base}_{slug}` matched against every base the project declares in `mustard.json#git.flow`) and compared with the spec. Reading needs no guess; rebuilding would need one per declared base and now one per work KIND too, since the name says what the unit IS instead of where it came from. The work unit is the branch plus everything the work produced — the spec, its waves, its ceremony and the code — so a caller standing there is inside the work, not deciding whether to enter it. Print no header and raise no *implement now* confirm: run the relay below immediately. What makes that comparison sound is upstream, and it is new: the unit is NAMED ONCE, at the base gate, and the draft files the spec under that same string — while the branch and the spec derived their slugs separately, this answered `false` from inside the unit's own branch and the promise above never fired at all. `false` also covers everything the check could not MEASURE — a directory that is not a repository, a VCS opt-out, a detached HEAD, an empty spec name: unmeasured takes the ceremony rather than claim a position nobody observed, and otherwise keeps whatever the route that brought you here prescribes (`${CLAUDE_PLUGIN_ROOT}/commands/spec.md` §3).

```bash
mustard-rt run wave-advance --spec {spec}
```

Returns the **current round** — `[{wave, role, subproject, subagent_type, prompt, precheck}]` for every wave of the lowest not-yet-complete dependency level. Once all impl waves carry `pipeline.wave.complete`, it returns the **review round** (one `role:review`/`mustard-review` per touched subproject). `[]` only after every touched subproject also carries a `review.result`.

**One answer is NOT an array and does not mean advance:** `{"error":"cyclic-dependency","cycle":[…]}`. The plan's `Depends on` column contradicts itself — every wave in `cycle` depends, directly or through others, on a wave that depends back on it. No order satisfies that, so nothing was dispatched and no `pipeline.wave.start` was written; a `pipeline.dispatch_failure` WAS written, so a `resume-bootstrap` in the next ten minutes reports the stall instead of reading the spec as idle (that record expires like any other dispatch failure — the expiry is what lets a FIXED plan stop being reported). **Do not retry, and do not fall through to `[]`'s branch below** — `[]` says "no wave is left, go and close", which is the opposite instruction. Fix the `Depends on` cells of the waves named in `cycle` and re-run `wave-advance`; no wave was started, so there is nothing to undo. This is deliberately harder than the WARN `wave-dependency` raises for an IMPORT cycle: that one is inferred from the files a wave touches and the planner's explicit boundaries stand over it, while this one is what the author wrote. Note the two `cycle` arrays hold different things — wave NUMBERS here, file paths there.

The refusal only fires over a contradiction that still GOVERNS something: a wave ON the loop that has neither completed nor had its own declared dependencies completed. `cycle` names the loop's own members and nothing else — being on a loop means reaching yourself through the `Depends on` column, so a wave merely WAITING behind a loop, or sitting between two of them, is never named: its cell is correct as written and it dispatches as soon as its dependency completes. A spec dispatched mid-round while the old code accepted its loop therefore keeps advancing, and reaches its review round and its `[]` without anyone editing a frozen wave plan.

**Two authoring forms this reader does NOT see, both pre-existing.** A cell written with bare numbers (`| 2 | ui | 1, 3 |`) yields no edges here, so a cycle authored that way stays invisible — `dependency-precheck` does read that form, and the two disagree. And a wave naming ITSELF is dropped rather than refused. Widening the reader was tried and reverted: scanning a free-text cell for dependency numbers turned ordinary prose ("nada, ver os 2 anexos") into a phantom cycle that refused a perfectly good plan. Fixing this properly means defining the cell's grammar, which is its own unit.

**A second answer must NOT be dispatched, and this one arrives INSIDE the ordinary array: a row whose `role` is `"escalation"`.** It replaces a wave that reached its retry ceiling — handed out once too often without ever completing (`MUSTARD_RETRY_CEILING`, default 3, enforced while `MUSTARD_RETRY_GATE_MODE` resolves to `strict`; `warn` records the fact and dispatches anyway, `off` does not look). The wave leaves the round, this row takes its place, and the round's healthy siblings dispatch normally beside it. **The row carries the STUCK WAVE'S OWN NUMBER** — it is a report ABOUT that wave, never a unit of work, and the two steps below are written unconditionally in a way that is wrong for it:

- **Step 1 does not apply to it — never `Task` it.** Its `subagent_type` reads `general-purpose` only because `escalation` falls through the default arm of `recommended_subagent_type`. Dispatching it hands an unrestricted write agent a prompt whose first words are `STOP — do not dispatch this item to an agent`.
- **Step 2 does not apply to its wave — never `wave-done` it.** That call writes `pipeline.wave.complete`, and that event is exactly what `wave-advance` reads to drop a wave from every future round. So marking a pulled wave complete does not pause the ceiling, it ERASES it: the pipeline advances and a wave whose work never ran is recorded as finished. Commit the round's healthy siblings and `wave-done` THEM as usual — only the escalated wave is carved out.

Instead: print the row's `prompt` to the operator verbatim — it names the wave, its role, its subproject, the count and the ceiling — and `AskUserQuestion` with the three ways out: raise the ceiling (`MUSTARD_RETRY_CEILING=N`), lift the gate for this run (`MUSTARD_RETRY_GATE_MODE=off` — that stops the PULL, it does not reset the count, so `strict` re-escalates on the very next round), or stop and fix what keeps failing. A `pipeline.dispatch_failure` was already written per stuck wave, so `resume-bootstrap` reports the stall instead of reading the spec as idle.

**Each round:**
1. **[you] Dispatch the WHOLE round in ONE message** — one `Task` per item, `prompt` **verbatim** (a `MUSTARD-PROMPT-REF` stub — never hand-craft, NEVER read the `.dispatch/` file; mechanics: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md`), `subagent_type` = the item field — **except a `role:"escalation"` row, which is NEVER dispatched (see above); it is read to the operator instead**. Before an impl item, check its `precheck`: `{ok:true}`/absent → dispatch; `{ok:true, skipped:"…"}` → the gate **DECLINED to judge** (unsupported stack) — dispatch, but say so: this green means nobody looked, not that the symbols are there; `{ok:false,missing,…}` → print `BLOCKED — N missing symbols`, emit `pipeline.dispatch_failure`, `AskUserQuestion` (investigate / force). **Skip** the whole check on `mode:continued` or `MUSTARD_DEPENDENCY_PRECHECK_MODE=off`.
2. **[you] Commit ONCE per ROUND — after EVERY wave of the round has returned, never after each wave.** One commit (`feat(wave-{N}/{role}): {summary}`, or `feat(waves-{N}-{M}): {summary}` when the round holds several), then `mustard-rt run wave-done --spec {spec} --wave {N} --duration-ms {elapsed}` **per wave of the round — but NEVER for a wave that came back as a `role:"escalation"` row (see above): completing a pulled wave erases the ceiling** (emits `pipeline.wave.complete` + caches that wave's diff — one atomic call; it runs after the commit so the cached diff is the round's real work). **Why per round:** committing between two waves of the same round is the one thing that can lose work — under the `add -A` law it sweeps a sibling's in-flight edits into your commit. Waves of the SAME round are independent by construction (that is what a dependency level means) and their declared `## Files` are disjoint because `plan-materialize` REFUSED the plan otherwise — a file declared by two waves of one level exits 2 with `sharedFiles`, naming the minimal chaining that zeroes it, so no colliding plan reaches approval (`wave-overlap-check` re-reads the same fact at the approval gate, advisory); waves of DIFFERENT rounds are sequential and cannot collide. So the round boundary removes the exposure outright — no isolated checkouts, no copies, no transport step.
3. **[you] After each review item:** save the review agent's return verbatim to a scratch file, then `mustard-rt run review-result --spec {spec} --verdict approved|rejected [--critical N] --subproject {sub} --findings-file {scratch}` — the "already reviewed" signal (else the next `wave-advance` re-emits it); persists the findings for the fix-loop's `## RETRY CONTEXT` — `<spec>/review/findings-{sub}.md` when `--subproject` is given (so each subproject's retry reads only its own reviewer), `<spec>/review/findings.md` when it is not. No commit/wave-done. REJECTED (any CRITICAL) → **§ Fix Loop** before advancing.
4. **[you] After the round:** `mustard-rt run wave-tree --spec-dir .claude/spec/{spec}`, then re-run `wave-advance`.
5. **`wave-advance` returns `[]`** → do NOT emit `pipeline.complete`. Re-run `resume-bootstrap` and follow `nextAction`:

| `nextAction` | Do |
|---|---|
| (null, round non-empty) | run the round above |
| `dispatch-review` | fallback only (resumed/missing verdict) — dispatch one review Task per `reviewRoles`; prefer the in-loop review round |
| `run-qa` / `emit-complete` | `mustard-rt run close-pipeline --spec {spec}` |

`close-pipeline` composes the CLOSE tail in ONE call: review verdicts (advisory) + `qa-run` + — only on QA pass — `complete-spec` + `pipeline-summary`. QA fail/skip → `completed:false`, no close — report the failing AC; never hand-run the sequence. `pipeline.complete` is **refused (exit 2) unless every criterion in the spec's `spec.ndjson` passed its last run** — `qa-run` records each run there.

**MIXED ROUND — one wave finished, its sibling came back `BLOCKED`.** Two rules above are both true here and neither one covers it: *commit once per round, after every wave has returned* (step 2) and *`BLOCKED` → STOP, do not advance* (§ Escalation). Every wave HAS returned, so the commit condition is met; one of them failed, so the round is not done. Do all three, in this order:

1. **Commit anyway.** Preserving work is not advancing it. The finished wave's work is real and uncommitted work is the only thing a later stash, checkout or retry can actually lose. Same message shape as any round.
2. **`wave-done` ONLY for the waves that finished.** A blocked wave gets no `pipeline.wave.complete` — that event is what makes `wave-advance` stop re-emitting it, and a wave marked done is a wave nobody comes back to.
3. **Do NOT advance the round.** No `wave-advance`, no next level. Go to § Escalation `BLOCKED`: `AskUserQuestion` with the exact blocker, and resume this same round from the blocked wave.

The record stays clean through this because `wave-done` scopes each wave's cached diff to the files that wave DECLARED in its own `## Files`, not to the whole commit — so the blocked sibling's half-written files never land in the finished wave's cached diff, and from there in its retry context and the closing summary. That is why committing a mixed round is safe to write down as a rule rather than a judgement call.

Then re-run QA.

---

## Escalation (check each agent return before advancing)

| Status | Handling |
|---|---|
| Internal error | re-dispatch sequentially, max 1 retry; still failing → STOP + report |
| `CONCERN` | record verbatim under `## Concerns`; continue. ≥2 → surface together first |
| `BLOCKED` | STOP; `AskUserQuestion` with the exact blocker; do NOT advance |
| `PARTIAL` | Granular Retry (do NOT restart — see § Wave failure & retry) |
| `DEFERRED` | note in spec; ask if load-bearing before CLOSE |
| REJECTED | § Fix Loop, max 2; 2 fails → STOP |

Status definitions: `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Escalation Statuses`.

---

## Fix Loop (review returned REJECTED, any CRITICAL)

**The stopping rule comes FIRST, before you re-render anything.** A round that found no defect of BEHAVIOUR ends the loop — whatever the count says. Read each critical and sort it into one of two piles:

| The finding says | It is | What happens |
|---|---|---|
| the shipped code does the wrong thing | **behaviour** | one more loop, up to the cap below |
| the code is right, the *strength of a test* or gate is what is missing | **defence** | ends the loop; becomes a declared pending item |

A defence finding is real and gets written down — never quietly dropped — but it does not send correct code back to be rewritten. The hole it lives in is in the reviewer's own definition of `critical` (`${CLAUDE_PLUGIN_ROOT}/agents/mustard-review.md`), which lists a *correctness defect* and says nothing about a finding aimed at the test: a test is code, a weak test is wrong code, so the reviewer reasons its way to `critical` honestly and the count blocks something that blocks nothing. Measured on 2026-08-27: a forty-line repair took five rounds; the first two found behaviour, the last three found only weak locks, and the loop could not tell them apart.

**And YOU decide this — never ask.** *"Quer mais uma rodada?"* hands the operator a decision the flow is supposed to take, which is the ceremony this rule exists to remove.

1. Re-render the SAME impl role with `mustard-rt run agent-prompt-render --spec {spec} --wave {N} --role {role} --subproject {sub} --mode fix-loop --emit ref` — **`--wave {N}` is the wave being retried, and it is not optional on a wave plan**: without it the prompt reads the PARENT's `## TASK` and carries every sibling's criteria under "these are the JUDGE of this wave", so the renderer REFUSES the call (exit 2) and names the missing flag. The renderer composes `## RETRY CONTEXT` from the spec's recorded events; you do NOT hand-assemble it (composition detail: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md § Retry Modes`). Loop K, max 2.
2. Dispatch that Task (do NOT change the role).
3. On return, re-dispatch the REVIEW agent (normal — read-only) and record the verdict via `review-result`.
4. Still REJECTED after 2 loops → **wave failure** (below).

---

## Wave failure & retry

**A wave has failed** when: REVIEW stays REJECTED after 2 fix-loops, OR an impl agent returns `BLOCKED` unresolvable inline, OR build/type-check fails after granular retry (max 2).

**On wave failure:**
1. Write `.claude/spec/{spec}/wave-{N}-{role}/failure.md` (`When`/`Phase`/`Reason`/`Findings verbatim`/`Files touched`). Waves 1..N-1 commits remain — real progress.
2. No further auto-recovery. **[you] AskUserQuestion:**
   - **"Corrigir manualmente e retomar"** → user fixes by hand; the next `/mustard:spec` restarts wave N from EXECUTE.
   - **"Reescrever wave {N}"** → delete `wave-{N}-{role}/spec.md`, re-PLAN scoped to wave N, re-approve via `/mustard:spec`.
   - **"Abortar pipeline"** → no filesystem move (the spec dir NEVER moves; lifecycle lives in `meta.json` + events): record it via `mustard-rt run emit-pipeline --kind pipeline.status --spec {spec} --payload '{"to":"abandoned"}'` (use `"wave-failed"` when only this wave died); keep waves 1..N-1 commits. Inform: `Pipeline aborted. Waves 1..{N-1} commits preserved. Waves {N}..{totalWaves} discarded.`

**Residual risk:** wave N-1 commits can be semantically incomplete without wave N (e.g. schema without API); `failure.md` states the exposed surface.

**Granular Retry** (PARTIAL): re-render the same role with `--wave {N} --mode granular` (the wave is required here for the same reason it is in the fix loop; the renderer composes `## RETRY CONTEXT` — see agent-prompt.md § Retry Modes); re-dispatch only the remaining steps via `--task-filter`. **Max 2 per agent** — exhausted → STOP.

**Pause:** on user pause / session end, emit `mustard-rt run emit-pipeline --kind pipeline.pause --spec {spec} --payload '{"pausedAt":"<ISO>","pauseReason":"<reason>","nextAction":"<ONE sentence>"}'` and confirm the saved next action.

**Next-action rule:** every handoff ends with exactly ONE next action (`→ Dispatch backend agent for task 3`), never a menu.

---

## Inviolable (loop-specific — see `${CLAUDE_PLUGIN_ROOT}/commands/spec.md` for picker/approve rules)

- Main context **IS** the runner — never wrap it in a single Task.
- Never implement code directly — all via Task (1 per subproject per wave).
- One `wave-advance` round = one message; never one wave at a time, never a later level by hand.
- Never hand-craft prompts / pick agents / read `wave-plan.md`. `wave-advance` IS the render; the LLM only relays.
- CLOSE only when `wave-advance` returns `[]` AND `nextAction` says so → via `close-pipeline`, never the manual `qa-run → complete-spec → pipeline-summary`. Don't gate on the scalar `currentWave`.
