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

**Already approved — skip re-approval (avoids the double gesture).** If `resume-bootstrap` returned `approvedByUser: true`, the plan was already approved — in `/feature`, in a prior `/spec`, or by the picker form the user just typed: the `<spec>/.approved-by-user` marker exists and `approve-spec` passes on its presence. Do **NOT** re-present the plan or re-ask the approval — that is the redundant second gesture. Skip straight to the *implement now vs approve only* choice (a single lightweight `AskUserQuestion`), then emit the relay below (`--resume` when implementing now). Everything else in §A below is for a plan **not yet** approved.

**Typed `{letter}r` — the marker is already minted, so go straight to the dispatch.** The user's own prompt is an act the model cannot author, so an observer mints `<spec>/.approved-by-user` with `via` naming the picker the moment `/mustard:spec {letter}r` arrives as the WHOLE prompt, typed in full (the observer matches nothing looser, or a message quoting the form could forge it) — and that ONE gesture answers both halves at once: the approval AND the *implement now* continuation. So take the shortcut above and take it one step further — do not present a plan-mode round trip, and do not raise the implement-now `AskUserQuestion` either. Print the plan for the record, emit the relay below with `--resume`, and fall into §B. A bare letter (no `r`), and a `{letter}r` answered into an already-rendered table, mint nothing: take the approval the normal way below.

**Is it a wave plan?** Check for `.claude/spec/{spec}/wave-plan.md`.

**Wave plan exists:**
1. `mustard-rt run event-projections --view pipeline-state --spec {spec}` → snapshot (`isWavePlan:true`, `totalWaves`, `currentWave`, `completedWaves`).

   **Read `neverDispatched` off the `resume-bootstrap` output you already have** (`${CLAUDE_PLUGIN_ROOT}/commands/spec.md` §3 ran it). `true` means the plan was scaffolded and NOBODY ever dispatched a wave — `currentWave: 1` there is a starting position, not progress. Say *"{totalWaves} ondas — nenhuma despachada ainda"*, never *"na onda 1"*: the two read the same and ask for opposite actions (start it versus resume it), and the wave directories alone cannot tell them apart.
2. Print the full `wave-plan.md` as a fenced block; list each wave-spec path below.
3. **Advisory audits (non-blocking).** Two deterministic wave-plan lints; each WARNS, neither blocks:
   - **Size:** `mustard-rt run wave-size-check --spec-dir .claude/spec/{spec}`. On `action:"audited"` + `oversizedCount>0`, print one `⚠ Wave {N} ({folder}) — {files} files, {layers} layer(s)` per oversized wave. Silent otherwise.
   - **Overlap:** `mustard-rt run wave-overlap-check --spec-dir .claude/spec/{spec}`. On `action:"audited"` + `overlapCount>0`, print one `⚠ Waves {a}+{b} (level {level}) both edit: {files}` per overlap — dispatch-parallel waves declaring the same file. Silent otherwise.
4. **[you]** Present for approval. **Plan mode is PRIMARY**: plan-file body = the full `wave-plan.md` + wave-spec paths; the user accepting `ExitPlanMode` mints `<spec>/.approved-by-user` (you cannot author it) and means *approve + implement now* (`implementNow=true`; chat "only approve" ⇒ `false`). Rejection keeps plan mode on — adjust and re-present.
   **Fallback (plan mode unavailable):** `AskUserQuestion` — ONE question, primary first. **Attach `wave-plan.md` as the `preview`** of the approval option (never ask approval for a plan the user cannot see); the answer mints the same marker. A letter-mode `r` never reaches this question — its marker was minted from the typed prompt and the shortcut above already routed it to the dispatch. This fallback is for a plan carrying NO marker, where the user's answer here is what mints `<spec>/.approved-by-user`:
   - **Approve and implement now — wave 1** (recommended) → `implementNow=true`.
   - **Approve only — new session** → `implementNow=false`.
   - **Reject decomposition** → `mustard-rt run wave-collapse --spec {spec} --mode {full|light}` (mode = the spec scope); act on its JSON. It merges in order, de-dups, writes-before-delete, patches sidecars. **Full** ⇒ a single `wave-1-{role}/` (Full ⇒ ≥1 wave — `block_full_without_wave` enforces it); **Light** ⇒ one `spec.md`, drops `wave-plan.md` + wave dirs.
   - **Stop — re-plan** → stop; tell the user: `Delete .claude/spec/{spec}/ and re-run /feature {name} with explicit guidance.`
5. If approved: the approval operates on the **wave-1 spec** — pass `--wave-plan`.

**Not a wave plan:** print a header (`**{spec}** — PLAN` + `{specSummary}`), then present the same way — plan mode with the spec body as the plan file (acceptance = *approve + implement now*), or the `AskUserQuestion` fallback with the spec body as `preview` (*implement now* `true` / *approve only* `false` / *adjust-stop*).

**Emit the approval (single relay):** `mustard-rt run approve-spec --spec {spec} [--wave-plan] [--resume]`. Act on its JSON (`{ok,spec,approved,resumed}`; on `{ok:false,error}` surface + stop). It emits `pipeline.stage{Plan}` + `pipeline.status{draft→approved}`, patches `meta.json` (never hand-edit `spec.md`), and — with `--resume` (pass whenever `implementNow=true`) — also `pipeline.stage{Execute}`.

**[you]** then: (a) optionally record ≤3 decisions via `mustard-rt run emit-event --event decision --spec {spec} --payload "title=…" --payload "rationale=…"`; (b) one `TaskCreate` per agent; (c) print `[v] ANALYZE [v] PLAN [>] EXECUTE [ ] CLOSE`.

- **`implementNow=false`** → **STOP.** Print `Spec aprovada. Abra nova sessão e rode /mustard:spec {name} para implementar com contexto limpo.` Do NOT dispatch.
- **`implementNow=true`** → `--resume` already emitted Execute (do NOT re-emit). Say `Spec aprovada. Implementando inline.` and fall into **§B**.

---

## §B — The loop (stage = Execute / post-approve)

Routing, order and prompts are **decided by Rust** — never read `wave-plan.md` or assemble the loop by hand.

**Arriving from inside the unit's own branch — no ceremony.** `resume-bootstrap` reports `insideWorkBranch: true` when the checkout already IS this spec's own branch — the branch is READ, never rebuilt: its slug half is taken off whichever shape the name carries (`{kind}/{slug}`, or the older `{base}_{slug}` matched against every base the project declares in `mustard.json#git.flow`) and compared with the spec. Reading needs no guess; rebuilding would need one per declared base and now one per work KIND too, since the name says what the unit IS instead of where it came from. The work unit is the branch plus everything the work produced — the spec, its waves, its ceremony and the code — so a caller standing there is inside the work, not deciding whether to enter it. Print no header and raise no *implement now* confirm: run the relay below immediately. What makes that comparison sound is upstream, and it is new: the unit is NAMED ONCE, at the base gate, and the draft files the spec under that same string — while the branch and the spec derived their slugs separately, this answered `false` from inside the unit's own branch and the promise above never fired at all. `false` also covers everything the check could not MEASURE — a directory that is not a repository, a VCS opt-out, a detached HEAD, an empty spec name: unmeasured takes the ceremony rather than claim a position nobody observed, and otherwise keeps whatever the route that brought you here prescribes (`${CLAUDE_PLUGIN_ROOT}/commands/spec.md` §3).

```bash
mustard-rt run wave-advance --spec {spec}
```

Returns the **current round** — `[{wave, role, subproject, subagent_type, prompt, precheck}]` for every wave of the lowest not-yet-complete dependency level. Once all impl waves carry `pipeline.wave.complete`, it returns the **review round** (one `role:review`/`mustard-review` per touched subproject). `[]` only after every touched subproject also carries a `review.result`.

**[you] Model/effort checkpoint — ONCE, at the first EXECUTE entry of this spec** (the §A→§B fall-through with `implementNow=true`, or the first resume that lands in `Execute`; skip it on every later round). The implementer runs as a `general-purpose` subagent that **inherits the session's model AND effort**, so choosing a different tier for this implementation is the user's own native `/model` / `/effort` (session-level — both reach the inheriting impl agent, and `/effort` is the ONLY way to vary effort: it cannot be scoped per-dispatch). Surface one line — `Implementação herda o modelo/effort da sessão. Trocar? rode /model e/ou /effort agora; senão, "segue".` — and wait for the go before the first dispatch. When the round is mechanical (prose, a rename, a single-file edit whose shape the spec already fixes), say so in that same line and name the cheaper tier: lower effort holds quality at a fraction of the tokens, and the tier is worth stepping UP only for the demanding rounds. Recommend, never decide — the lever is the user's. Do **not** plumb a per-dispatch model or persist a custom field — the docs expose no user-facing per-invocation lever. Review/scan agents are unaffected.

**Each round:**
1. **[you] Dispatch the WHOLE round in ONE message** — one `Task` per item, `prompt` **verbatim** (a `MUSTARD-PROMPT-REF` stub — never hand-craft, NEVER read the `.dispatch/` file; mechanics: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md`), `subagent_type` = the item field. Before an impl item, check its `precheck`: `{ok:true}`/absent → dispatch; `{ok:true, skipped:"…"}` → the gate **DECLINED to judge** (unsupported stack) — dispatch, but say so: this green means nobody looked, not that the symbols are there; `{ok:false,missing,…}` → print `BLOCKED — N missing symbols`, emit `pipeline.dispatch_failure`, `AskUserQuestion` (tactical-fix / investigate / force). **Skip** the whole check on `mode:continued` or `MUSTARD_DEPENDENCY_PRECHECK_MODE=off`.
2. **[you] Commit ONCE per ROUND — after EVERY wave of the round has returned, never after each wave.** One commit (`feat(wave-{N}/{role}): {summary}`, or `feat(waves-{N}-{M}): {summary}` when the round holds several), then `mustard-rt run wave-done --spec {spec} --wave {N} --duration-ms {elapsed}` **per wave of the round** (emits `pipeline.wave.complete` + caches that wave's diff — one atomic call; it runs after the commit so the cached diff is the round's real work). **Why per round:** committing between two waves of the same round is the one thing that can lose work — under the `add -A` law it sweeps a sibling's in-flight edits into your commit. Waves of the SAME round are independent by construction (that is what a dependency level means) and their declared `## Files` are audited disjoint (`wave-overlap-check`); waves of DIFFERENT rounds are sequential and cannot collide. So the round boundary removes the exposure outright — no isolated checkouts, no copies, no transport step.
3. **[you] After each review item:** save the review agent's return verbatim to a scratch file, then `mustard-rt run review-result --spec {spec} --verdict approved|rejected [--critical N] --subproject {sub} --findings-file {scratch}` — the "already reviewed" signal (else the next `wave-advance` re-emits it); persists the findings for the fix-loop's `## RETRY CONTEXT` — `<spec>/review/findings-{sub}.md` when `--subproject` is given (so each subproject's retry reads only its own reviewer), `<spec>/review/findings.md` when it is not. No commit/wave-done. REJECTED (any CRITICAL) → **§ Fix Loop** before advancing.
4. **[you] After the round:** `mustard-rt run wave-tree --spec-dir .claude/spec/{spec}`, then re-run `wave-advance`.
5. **`wave-advance` returns `[]`** → do NOT emit `pipeline.complete`. Re-run `resume-bootstrap` and follow `nextAction`:

| `nextAction` | Do |
|---|---|
| (null, round non-empty) | run the round above |
| `dispatch-review` | fallback only (resumed/missing verdict) — dispatch one review Task per `reviewRoles`; prefer the in-loop review round |
| `run-qa` / `emit-complete` | `mustard-rt run close-pipeline --spec {spec}` |

`close-pipeline` composes the CLOSE tail in ONE call: review verdicts (advisory) + `qa-run` + — only on QA pass — the **confirmation pass** + `complete-spec` + `pipeline-summary`. QA fail/skip → `completed:false`, no close — report the failing AC; never hand-run the sequence. `pipeline.complete` is **refused (exit 2) without `qa.result overall=pass`**.

**The confirmation is the second half of the criterion proof, and CLOSE is where it comes due.** At PLAN time every criterion had to come back RED (`ac-negative-check`) — the proof it knows how to fail. That half never asks whether it passes NOW, so a command that is BROKEN and a behaviour that is merely ABSENT read exactly alike. So `close-pipeline` runs each red-proven criterion AGAIN, after the work landed, and writes the verdict into the second column of `<spec>/ac-proof.json`. Read the `confirmation` block it returns:

- `taken:true, ok:true` — every criterion was seen to PASS after its work landed. This is the only reading that says the proof is complete.
- `taken:true, ok:false` — `unproven` NAMES each criterion that did not clear. Either the work is not there, or the criterion never asserted it; the ledger's `reason` says which and what clears it.
- `taken:false` — NOT TAKEN (QA did not pass, so nothing closed and the confirmation was not due). `ok` is `null`, never `false`: nobody looked is not the same answer as it failed.

It is **advisory** — QA already blocks the close on the same commands — so it stops nothing. What it ends is the spec clearing on its red proof alone. To take it by hand outside a close (e.g. after a fix loop, before re-running QA): `mustard-rt run ac-negative-check --spec {spec} --confirm`.

**MIXED ROUND — one wave finished, its sibling came back `BLOCKED`.** Two rules above are both true here and neither one covers it: *commit once per round, after every wave has returned* (step 2) and *`BLOCKED` → STOP, do not advance* (§ Escalation). Every wave HAS returned, so the commit condition is met; one of them failed, so the round is not done. Do all three, in this order:

1. **Commit anyway.** Preserving work is not advancing it. The finished wave's work is real and uncommitted work is the only thing a later stash, checkout or retry can actually lose. Same message shape as any round.
2. **`wave-done` ONLY for the waves that finished.** A blocked wave gets no `pipeline.wave.complete` — that event is what makes `wave-advance` stop re-emitting it, and a wave marked done is a wave nobody comes back to.
3. **Do NOT advance the round.** No `wave-advance`, no next level. Go to § Escalation `BLOCKED`: `AskUserQuestion` with the exact blocker, and resume this same round from the blocked wave.

The record stays clean through this because `wave-done` scopes each wave's cached diff to the files that wave DECLARED in its own `## Files`, not to the whole commit — so the blocked sibling's half-written files never land in the finished wave's cached diff, and from there in its retry context and the closing summary. That is why committing a mixed round is safe to write down as a rule rather than a judgement call.

**[you] The user asks for a change mid-round → record the INSTRUCTION, not just the reply.** An observer already captures every prompt verbatim into the spec's change log — that is the raw trail, and it keeps the user's words. But a reply carries its meaning from the conversation: `"Concordo se for agregar"` recorded alone tells the next wave nothing it can act on. So state what was agreed, in one self-contained sentence, as its own record:

```bash
mustard-rt run change-request --spec {spec} --instruction "<what changes, stated so a wave that never saw this conversation can act on it>"
```

It lands in the shape `read_change_log` filters for, so the next rendered prompt carries it under `## CHANGE REQUESTS` — no hand-formatted bullet, and a refusal (empty instruction, unknown spec) writes nothing.

**Then carry anything that changes BEHAVIOUR into `## Acceptance Criteria` — with `ac-amend`, never by hand**: a request that is implemented but unnamed by any AC makes the gate report green without ever verifying it (found in review, 2026-07-25).

```bash
mustard-rt run ac-amend --spec {spec} --ac AC-3 --command "<the command that asserts the NEW behaviour>" [--expect "<evidence regex>"] [--statement "<the EARS line>"] --reason "<why the criterion is changing>"
```

Two things the hand cannot do, and this is why the hand does not do it:

- **The replacement is PROVEN.** It goes through the same negative test the plan took (`ac-negative-check`): the new command is run against the tree as it is and **must itself come back RED**. A replacement that already passes proves exactly as little as the criterion it replaces, so the amendment is REFUSED — along with a blank reason, an unknown spec and an unknown AC id. Every refusal writes nothing, anywhere.
  **The one exception, and it cannot be asked for.** When the confirmation pass already recorded the criterion being replaced as INEXECUTABLE — its command could not be attempted AT ALL after its work landed — the command is broken whatever the work does, and by then the work IS done, so a corrected command legitimately PASSES. Demanding a red there demands a criterion that lies about a feature that exists. For that ONE recorded state, a GREEN replacement is accepted and its record carries a green CONFIRMATION (the evidence the approval gate reads). The door is unlocked by a finding the engine itself wrote into `<spec>/ac-proof.json`, never by a flag — so it cannot be used to smuggle a vacuous criterion past the gate. If you meet `replacement_not_proven` on a command you believe is correct, take the confirmation first (`ac-negative-check --spec {spec} --confirm`): it is what records the finding this exception reads.
- **Every artefact is rewritten.** `wave-plan.md` and each `wave-*/spec.md` carry the criterion lines too, and the scaffold is frozen after approval — a root-only edit leaves the dispatched agent reading the superseded command. `ac-amend` rewrites each artefact carrying that id and appends the supersession to `<spec>/ac-proof.json`'s `amendments`, so the change is auditable instead of being a `decision` event that is a trail, not a path. `--statement` replaces the WHOLE statement, including the continuation lines a long EARS sentence wraps onto — the parser only ever reads the first of them, so replacing just that line would leave the rest orphaned under a sentence they no longer continue (found in review, 2026-07-28, on this spec's own AC-1).

**When the change is named by NO criterion at all, ADD one — with `ac-add`, never by hand.** `ac-amend` REPLACES an id that exists and refuses one it does not know, because a replacement proves itself against the criterion it supersedes and an added id has no predecessor. So a finding nobody wrote a criterion for has its own door:

```bash
mustard-rt run ac-add --spec {spec} --ac AC-9 --statement "when <trigger>, then <outcome>" --command "<the command that asserts it>" [--expect "<evidence regex>"] --reason "<why this criterion is being added>"
```

It takes the SAME negative proof a planned criterion takes: the command is run against the tree as it is and **must come back RED**, or the addition is refused and nothing is written — along with a blank reason, a blank statement, an unknown spec, and an id the spec already carries (that one points you back at `ac-amend`). It lands in every plan artefact — the root, `wave-plan.md` and each `wave-*/spec.md` — directly ABOVE the trailing build-green criterion, so the positional exemption stays where it belongs. The record goes to the ledger's `additions`, kept apart from `amendments` because nothing was superseded.

**The third transition, when a criterion looks suspiciously easy to satisfy.** Red before the work and green after it are both satisfied by a criterion that verifies something the work never did — the classic shape is a command pointing at a subsystem the waves never touched. Only removing the work again catches that:

```bash
mustard-rt run ac-negative-check --spec {spec} --removal
```

It cuts a scratch checkout with the files the waves cached as changed taken away and re-runs each CONFIRMED criterion there. One that stays green **survived** the removal and is reported as verifying nothing — rewrite it through the door above. It is not automatic and should not be: the scratch tree has no build cache, so a test criterion compiles from scratch there. Ask for it when a criterion's green looks too cheap.

**Read its limit before reading its reds.** The strip is file-grained, because file paths are all the cached diff carries, so it takes the criterion's own evidence away whenever that evidence shares a file with the behaviour — which for a project whose tests live beside the code they test is every test criterion there is. The pass does not book those as proof: a criterion whose own evidence — its command OR its `Expect:` regex, the two halves the executor grades with — names a word the strip deleted comes back `evidence-removed` and is counted apart from the reds, with the reason naming the word. So the pass falsifies (a green with the work gone is a finding) and declines (a guaranteed red is not); what it never does is certify a criterion it could not have failed.

Then re-run QA. The narrative of `spec.md` stays frozen either way — an amendment touches the criterion, never the prose.

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

1. Re-render the SAME impl role with `mustard-rt run agent-prompt-render --spec {spec} --role {role} --subproject {sub} --mode fix-loop --emit ref` — the renderer composes `## RETRY CONTEXT` from the spec's recorded events; you do NOT hand-assemble it (composition detail: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md § Retry Modes`). Loop K, max 2.
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

**Granular Retry** (PARTIAL): re-render the same role with `--mode granular` (the renderer composes `## RETRY CONTEXT` — see agent-prompt.md § Retry Modes); re-dispatch only the remaining steps via `--task-filter`. **Max 2 per agent** — exhausted → STOP.

**Pause:** on user pause / session end, emit `mustard-rt run emit-pipeline --kind pipeline.pause --spec {spec} --payload '{"pausedAt":"<ISO>","pauseReason":"<reason>","nextAction":"<ONE sentence>"}'` and confirm the saved next action.

**Next-action rule:** every handoff ends with exactly ONE next action (`→ Dispatch backend agent for task 3`), never a menu.

---

## Inviolable (loop-specific — see `${CLAUDE_PLUGIN_ROOT}/commands/spec.md` for picker/approve rules)

- Main context **IS** the runner — never wrap it in a single Task.
- Never implement code directly — all via Task (1 per subproject per wave).
- One `wave-advance` round = one message; never one wave at a time, never a later level by hand.
- Never hand-craft prompts / pick agents / read `wave-plan.md`. `wave-advance` IS the render; the LLM only relays.
- CLOSE only when `wave-advance` returns `[]` AND `nextAction` says so → via `close-pipeline`, never the manual `qa-run → complete-spec → pipeline-summary`. Don't gate on the scalar `currentWave`.
