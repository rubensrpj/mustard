# /mustard:spec — Resume loop (approve → dispatch → close)

The single procedure for driving a spec from PLAN through CLOSE. `spec/SKILL.md` §2 routes here by `resume-bootstrap` stage:

- **`Plan`** → **§A Approve** (then, if approved-inline, fall straight into §B).
- **`Execute` / `Analyze` / `QaReview` / `QaPending` / `ReviewPending` / `Close`** → **§B Loop**.

The binary owns every decision that can be made deterministically (wave order, routing, prompts, mode, nextAction). This ref is a **relay**: run the command, do exactly what its output says. The only things left to you are marked **[you]**.

---

## §A — Approve gate (stage = Plan)

A spec has two layers — `## PRD` (what & why) and `## Plan` (how). Approving approves **both at once**; there is no separate PRD gate.

**Is it a wave plan?** Check for `.claude/spec/{spec}/wave-plan.md`.

**Wave plan exists:**
1. `mustard-rt run event-projections --view pipeline-state --spec {spec}` → snapshot (`isWavePlan:true`, `totalWaves`, `currentWave`, `completedWaves`).
2. Print the full `wave-plan.md` in a fenced ```markdown block; list each wave-spec path below it.
3. **Advisory size audit:** `mustard-rt run wave-size-check --spec-dir .claude/spec/{spec}`. If `action:"audited"` and `oversizedCount>0`, print one `⚠ Wave {N} ({folder}) — {files} files, {layers} layer(s) — consider splitting ({reason})` per wave. It **does not block** — it informs the "re-plan" option below. Silent otherwise.
4. **[you]** `AskUserQuestion` — ONE question, primary first. **Attach `wave-plan.md` as the `preview`** of the approval option (never ask approval for a plan the user cannot see). A letter-mode `r` pre-answers it as *approve + implement now*:
   - **Approve and implement now — wave 1** (recommended) → `implementNow=true`.
   - **Approve only — new session** → `implementNow=false`.
   - **Reject decomposition** → run `mustard-rt run wave-collapse --spec {spec} --mode {full|light}` (mode = the spec's scope) and act on its JSON. It merges wave sections in order, de-dups files, writes the merged spec **before** deleting anything, patches sidecars. **Full** ⇒ collapses to a single `wave-1-{role}/` (parent stays orchestrator; **Full ⇒ ≥1 wave**, never zero — the `block_full_without_wave` gate enforces it). **Light** ⇒ merges back to one `spec.md`, drops `wave-plan.md` + wave dirs (`isWavePlan:false` valid only for Light). Then continue as a single spec.
   - **Stop — re-plan** → stop; tell the user: `Delete .claude/spec/{spec}/ and re-run /feature {name} with explicit guidance.`
5. If approved: the approval operates on the **wave-1 spec** — pass `--wave-plan`.

**Not a wave plan:** print a one-block header (`**{spec}** — PLAN` + `{specSummary}` from `resume-bootstrap`), then **[you]** `AskUserQuestion` — ONE question, **spec body attached as `preview`**: *Approve and implement now* (`true`) / *Approve only — new session* (`false`) / *Adjust-stop*.

**Emit the approval (single relay):** `mustard-rt run approve-spec --spec {spec} [--wave-plan] [--resume]`. Act on its JSON (`{ok,spec,approved,resumed}`; on `{ok:false,error}` surface + stop). It emits `pipeline.stage{Plan}` + `pipeline.status{draft→approved}`, patches `meta.json` (never hand-edit `spec.md`), and — with `--resume` (pass whenever `implementNow=true`) — also `pipeline.stage{Execute}`.

**[you]** then: (a) optionally record ≤3 architectural decisions — `echo '{"type":"decision","content":"…","source":"{spec}","context":"approved at PLAN"}' | mustard-rt run memory decision`; (b) one `TaskCreate` per agent in the spec; (c) print `[v] ANALYZE [v] PLAN [>] EXECUTE [ ] CLOSE` + a layer line (`Aprovado: camada PRD + camada Plano.`).

- **`implementNow=false`** → **STOP.** Print: `Spec aprovada. Abra uma nova sessão e rode /mustard:spec {name} para implementar com contexto limpo.` Do NOT dispatch.
- **`implementNow=true`** → `--resume` already emitted the Execute transition (do NOT re-emit). Say `Spec aprovada. Implementando inline.` and fall into **§B** (skip its re-detection — the spec is known and approved).

---

## §B — The loop (stage = Execute / post-approve)

Routing, order and prompts are **decided by Rust**. Never read `wave-plan.md` or assemble the loop by hand.

```bash
mustard-rt run wave-advance --spec {spec}
```

Returns the **current round** — an array of `{wave, role, subproject, subagent_type, prompt, precheck}` — every wave of the lowest not-yet-complete dependency level. Once all impl waves carry `pipeline.wave.complete`, it returns the **review round** (one `role:review` / `mustard-review` item per touched subproject, alphabetical). It returns `[]` only after every touched subproject also carries a `review.result`.

**Each round:**
1. **[you] Dispatch the WHOLE round in ONE message** — one `Task` per item, `prompt` **verbatim** (it is a 2-line `MUSTARD-PROMPT-REF` stub the PreToolUse hook expands — NEVER read the `.dispatch/` file, that pays the full prompt back into your context), `subagent_type` = the item's field. Before an impl item, read its `precheck`: `{ok:true}` (or absent) → dispatch; `{ok:false,missing,suggested_tactical_fix_files}` → print `BLOCKED — N missing symbols`, emit `pipeline.dispatch_failure`, and `AskUserQuestion` (create tactical-fix / investigate / force). **Skip precheck** when `resume-bootstrap` returned `mode:continued` or `MUSTARD_DEPENDENCY_PRECHECK_MODE=off`.
2. **[you] After each impl wave returns:** commit (`feat(wave-{N}/{role}): {summary}`), then `mustard-rt run wave-done --spec {spec} --wave {N} --duration-ms {elapsed}` (emits `pipeline.wave.complete` + caches the diff for the next render — one atomic call, no shell redirect).
3. **[you] After each review item returns:** `mustard-rt run review-result --spec {spec} --verdict approved|rejected [--critical N] --subproject {sub}`. This is the "already reviewed" signal — without it the next `wave-advance` re-emits the same item. No commit, no wave-done. REJECTED (any CRITICAL) → `../resume/fix-loop-wave.md` (max 2 loops; 2 fails → STOP) before advancing.
4. **[you] After the round:** `mustard-rt run wave-tree --spec-dir .claude/spec/{spec}` for progress, then re-run `wave-advance` — it advances on its own.
5. **`wave-advance` returns `[]`** → do NOT emit `pipeline.complete`. Re-run `resume-bootstrap` and follow `nextAction`:

| `nextAction` | Do |
|---|---|
| (null, round non-empty) | run the round above |
| `dispatch-review` | fallback only (resumed/missing verdict) — dispatch one review Task per `reviewRoles`; prefer the in-loop review round |
| `run-qa` / `emit-complete` | `mustard-rt run close-pipeline --spec {spec}` |

`close-pipeline` composes the whole CLOSE tail in ONE call: review verdicts (advisory) + `qa-run` + — only on QA pass — `complete-spec` + `pipeline-summary`. On QA fail/skip it returns `completed:false` and does NOT close — report the failing AC; never hand-run the sequence or work around it. `pipeline.complete` is **refused (exit 2) without a `qa.result overall=pass`** in the spec's ndjson (`--allow-no-qa` is a `qa-run`-only user override).

---

## Escalation (check each agent's return before advancing)

| Status | Handling |
|---|---|
| Internal error | re-dispatch sequentially, max 1 retry; still failing → STOP + report |
| `CONCERN` | record verbatim under `## Concerns`; continue. ≥2 → surface together first |
| `BLOCKED` | STOP; `AskUserQuestion` with the exact blocker; do NOT advance |
| `PARTIAL` | Granular Retry (do NOT restart) |
| `DEFERRED` | note in spec; ask if load-bearing before CLOSE |
| REJECTED | Fix Loop (`../resume/fix-loop-wave.md`), max 2; 2 fails → STOP |

## Inviolable (loop-specific — see spec/SKILL.md for picker/approve rules)

- Main context **IS** the runner — never wrap it in a single Task.
- Never implement code directly — all via Task (1 per subproject per wave).
- One `wave-advance` round = one message (several `<invoke>` blocks); never one wave at a time when the round holds several, never a later level by hand.
- Never hand-craft prompts / pick agents / read `wave-plan.md`. `wave-advance` IS the render; the LLM only relays.
- CLOSE only when `wave-advance` returns `[]` AND `nextAction` says so → via `close-pipeline`, never the manual `qa-run → complete-spec → pipeline-summary` sequence. Do not gate on the scalar `currentWave`.
