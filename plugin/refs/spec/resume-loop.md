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

**Is it a wave plan?** Check for `.claude/spec/{spec}/wave-plan.md`.

**Wave plan exists:**
1. `mustard-rt run event-projections --view pipeline-state --spec {spec}` → snapshot (`isWavePlan:true`, `totalWaves`, `currentWave`, `completedWaves`).
2. Print the full `wave-plan.md` as a fenced block; list each wave-spec path below.
3. **Advisory size audit:** `mustard-rt run wave-size-check --spec-dir .claude/spec/{spec}`. On `action:"audited"` + `oversizedCount>0`, print one `⚠ Wave {N} ({folder}) — {files} files, {layers} layer(s)` per oversized wave; it **does not block** (informs the re-plan option). Silent otherwise.
4. **[you]** Present for approval. **Plan mode is PRIMARY**: plan-file body = the full `wave-plan.md` + wave-spec paths; the user accepting `ExitPlanMode` mints `<spec>/.approved-by-user` (via `plan_approval_observer` — you cannot author it) and means *approve + implement now* (`implementNow=true`; chat "only approve" ⇒ `false`). Rejection keeps plan mode on — adjust and re-present.
   **Fallback (plan mode unavailable):** `AskUserQuestion` — ONE question, primary first. **Attach `wave-plan.md` as the `preview`** of the approval option (never ask approval for a plan the user cannot see); the answer mints the same marker. A letter-mode `r` pre-answers it as *approve + implement now*:
   - **Approve and implement now — wave 1** (recommended) → `implementNow=true`.
   - **Approve only — new session** → `implementNow=false`.
   - **Reject decomposition** → `mustard-rt run wave-collapse --spec {spec} --mode {full|light}` (mode = the spec scope); act on its JSON. It merges waves in order, de-dups, writes the merged spec **before** deleting, patches sidecars. **Full** ⇒ a single `wave-1-{role}/` (Full ⇒ ≥1 wave — `block_full_without_wave` enforces it); **Light** ⇒ one `spec.md`, drops `wave-plan.md` + wave dirs.
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

```bash
mustard-rt run wave-advance --spec {spec}
```

Returns the **current round** — `[{wave, role, subproject, subagent_type, prompt, precheck}]` for every wave of the lowest not-yet-complete dependency level. Once all impl waves carry `pipeline.wave.complete`, it returns the **review round** (one `role:review`/`mustard-review` per touched subproject). `[]` only after every touched subproject also carries a `review.result`.

**Each round:**
1. **[you] Dispatch the WHOLE round in ONE message** — one `Task` per item, `prompt` **verbatim** (a 2-line `MUSTARD-PROMPT-REF` stub the PreToolUse hook expands — NEVER read the `.dispatch/` file), `subagent_type` = the item field. Before an impl item, check its `precheck`: `{ok:true}`/absent → dispatch; `{ok:false,missing,…}` → print `BLOCKED — N missing symbols`, emit `pipeline.dispatch_failure`, `AskUserQuestion` (tactical-fix / investigate / force). **Skip** on `mode:continued` or `MUSTARD_DEPENDENCY_PRECHECK_MODE=off`.
2. **[you] After each impl wave:** commit (`feat(wave-{N}/{role}): {summary}`), then `mustard-rt run wave-done --spec {spec} --wave {N} --duration-ms {elapsed}` (emits `pipeline.wave.complete` + caches the diff — one atomic call).
3. **[you] After each review item:** save the review agent's return verbatim to a scratch file, then `mustard-rt run review-result --spec {spec} --verdict approved|rejected [--critical N] --subproject {sub} --findings-file {scratch}` — the "already reviewed" signal (else the next `wave-advance` re-emits it); persists `<spec>/review/findings.md` for the fix-loop's `## RETRY CONTEXT`. No commit/wave-done. REJECTED (any CRITICAL) → **§ Fix Loop** before advancing.
4. **[you] After the round:** `mustard-rt run wave-tree --spec-dir .claude/spec/{spec}`, then re-run `wave-advance`.
5. **`wave-advance` returns `[]`** → do NOT emit `pipeline.complete`. Re-run `resume-bootstrap` and follow `nextAction`:

| `nextAction` | Do |
|---|---|
| (null, round non-empty) | run the round above |
| `dispatch-review` | fallback only (resumed/missing verdict) — dispatch one review Task per `reviewRoles`; prefer the in-loop review round |
| `run-qa` / `emit-complete` | `mustard-rt run close-pipeline --spec {spec}` |

`close-pipeline` composes the CLOSE tail in ONE call: review verdicts (advisory) + `qa-run` + — only on QA pass — `complete-spec` + `pipeline-summary`. QA fail/skip → `completed:false`, no close — report the failing AC; never hand-run the sequence. `pipeline.complete` is **refused (exit 2) without `qa.result overall=pass`**.

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

1. Re-render the SAME impl role with `mustard-rt run agent-prompt-render --spec {spec} --role {role} --subproject {sub} --mode fix-loop --emit ref` — the renderer composes `## RETRY CONTEXT` (last review verdict + critical count, persisted `review/findings.md` verbatim, prior-wave diff, change requests) from the spec's recorded events; you do NOT hand-assemble it (loop K, max 2).
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

**Granular Retry** (PARTIAL): re-render the same role with `--mode granular` (renderer composes `## RETRY CONTEXT` from the spec's recorded events, persisted findings and the prior-wave diff); re-dispatch only the remaining steps via `--task-filter`. **Max 2 per agent** — exhausted → STOP.

**Pause:** on user pause / session end, emit `mustard-rt run emit-pipeline --kind pipeline.pause --spec {spec} --payload '{"pausedAt":"<ISO>","pauseReason":"<reason>","nextAction":"<ONE sentence>"}'` and confirm the saved next action.

**Next-action rule:** every handoff ends with exactly ONE next action (`→ Dispatch backend agent for task 3`), never a menu.

---

## Inviolable (loop-specific — see `${CLAUDE_PLUGIN_ROOT}/commands/spec.md` for picker/approve rules)

- Main context **IS** the runner — never wrap it in a single Task.
- Never implement code directly — all via Task (1 per subproject per wave).
- One `wave-advance` round = one message; never one wave at a time, never a later level by hand.
- Never hand-craft prompts / pick agents / read `wave-plan.md`. `wave-advance` IS the render; the LLM only relays.
- CLOSE only when `wave-advance` returns `[]` AND `nextAction` says so → via `close-pipeline`, never the manual `qa-run → complete-spec → pipeline-summary`. Don't gate on the scalar `currentWave`.
