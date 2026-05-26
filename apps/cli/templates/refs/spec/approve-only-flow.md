# /mustard:spec — Approve-only flow

Loaded on demand by `commands/mustard/spec/SKILL.md` Step 7 when the selected spec is in the PLAN stage. Content moved **verbatim** from the former `commands/mustard/approve/SKILL.md` (deleted in TF `2026-05-23-tf-unify-spec-command`), with minimal seam adjustments for the new entry-point.

## Description

Approves the active spec selected by the picker and prepares the implementation phase.

A spec has two named layers (see `/feature` § Full Scope): `## PRD` — the *what & why* (intent) — and `## Plan` — the *how* (execution). Approving a spec approves **both layers at once**: there is no separate gate to "approve PRD". The two-layer separation is a reading aid, not a second checkpoint — keep it that way.

- **No `r` suffix** (`/mustard:spec {letter}` with PLAN stage): prepares the pipeline state and STOPS, instructing the user to open a new session and run `/mustard:spec {letter}` again to continue with clean context. Recommended for Full-scope specs with 5+ files.
- **With `r` suffix** (`/mustard:spec {letter}r`): after preparation, immediately jumps to the `resume-flow.md` flow in the same session (skips Step 0 and Step 1 of resume — no dispatch failure check, no handoff summary, no reconfirmation). Use when the spec was just approved and you want to avoid the session restart hop. Tradeoff: the EXECUTE phase inherits the ANALYZE+PLAN context instead of starting clean — ok for small/medium specs, less efficient for large ones.

## Prerequisites

- Active spec in `.claude/spec/{name}/` (flat layout — status read from the spec header / SQLite projection — event database)
- The spec has been shown to the user and they picked the corresponding letter in the `/mustard:spec` picker

## Action

1. **Step 0: AUTO-SYNC (mandatory)** — already executed in Step 1 of `/mustard:spec`. Do not re-execute.
2. **Read** `.claude/pipeline-config.md` — agents, model selection.
3. The spec has already been located by the `/mustard:spec` picker (filtered by Stage + Outcome header — only Outcome `Active` AND Stage ∈ {Plan, Execute}).

### Step 3b: Wave Plan Detection

Check whether the located spec is a wave plan: look for `.claude/spec/{specName}/wave-plan.md`.

**If `wave-plan.md` exists:**

1. Load the pipeline state derived from the SQLite event log (run `mustard-rt run event-projections --view pipeline-state --spec {specName}` to get the current snapshot) — expect `isWavePlan: true`, `totalWaves: N`, `currentWave: 1`, `completedWaves: []`.
2. Read `wave-plan.md` and print the ENTIRE content inside a fenced markdown block (```` ```markdown ... ``` ````). List each wave-spec file path below the block (one line each).
2b. **Wave size audit (advisory only):** run `mustard-rt run wave-size-check --spec-dir .claude/spec/{specName}`.
   - If the result is `action: "audited"` and `oversizedCount > 0`, print a warning block listing each oversized wave:
     `⚠ Wave {N} ({folder}) — {fileCount} files, {layerCount} layer(s) — consider splitting ({reason})`
   - State explicitly that this is **advisory** — does NOT block approval. It informs the **"Stop — re-plan with guidance"** option of the next `AskUserQuestion`: an oversized wave can be split before EXECUTE.
   - If `oversizedCount === 0` or `action: "skip"`, print nothing (silent).
3. `AskUserQuestion`:
   - **"Approve wave plan — start with wave 1"** → proceed to step 4 (updates header + state for wave 1 dispatch)
   - **"Reject decomposition — use single spec"** → merge all wave specs back into a single spec at `.claude/spec/{specName}/spec.md` (concatenate `## Files`, `## Tasks`, `## Boundaries` of each wave), delete `wave-plan.md` and the `wave-N-*/` subdirs, set `scopeOverride: "user-rejected-waves"` and `isWavePlan: false` in pipeline state, proceed to step 4 on the single spec
   - **"Stop — re-plan with guidance"** → stop. Instruct user: `Delete .claude/spec/{specName}/ and re-run /feature {name} with explicit guidance (e.g., "keep wave 2 and wave 3 together").`
4. If the wave plan was approved, from step 4 onwards operate on the **wave 1 spec** (`.claude/spec/{specName}/wave-1-{role}/spec.md`) — update its header, not the `wave-plan.md` header.

**If `wave-plan.md` does NOT exist:** proceed as single spec (behavior below).

4. **Spec Checkpoint — update spec header**: set Stage `Plan`, Outcome `Active`, Flags empty, Checkpoint `{ISO timestamp now}`. Preserve existing Scope, Lang and Parent (header) lines.
5. **Pipeline State — emit stage transition to Plan:**
   - Extract `spec-name` from the spec directory (e.g. basename of the path → `2026-02-26-linked-services-card`)
   ```bash
   mustard-rt run emit-pipeline --kind pipeline.stage --spec {spec-name} --payload "{\"stage\":\"Plan\"}"
   mustard-rt run emit-pipeline --kind pipeline.status --spec {spec-name} --payload "{\"from\":\"draft\",\"to\":\"approved\"}"
   ```
   - No JSON file is written here.
5b. **Memory Persist — record architectural decisions:**
   - For each significant decision in the spec (technology choices, design patterns, trade-offs):
     ```bash
     echo '{"type":"decision","content":"<decision description>","source":"<spec-name>","context":"approved at PLAN phase"}' | mustard-rt run memory decision
     ```
   - Focus on: why a pattern was chosen over alternatives, constraints that shaped the design
   - Skip trivial or obvious decisions (max 3 entries)
6. **Model selection** — read `Model Selection` from `.claude/pipeline-config.md` and record `"model"` field in state:
   - Count estimated total files in the spec
   - Apply rule: ≤5 files / known patterns → `"model": "sonnet"`, 5+ files / new patterns → `"model": "opus"`
7. **Task Tracking — create TaskCreate for each agent:**
   - 1 TaskCreate per agent identified in the spec
   - Subject: `"{Layer}: {brief description}"`
   - activeForm: `"Running {Layer} agent"`
8. **Output — visual feedback:**
   - Print progress line: `[v] ANALYZE  [v] PLAN  [>] EXECUTE  [ ] CLOSE`
   - Print a layer signal line so the user knows what was approved:
     `Approved: PRD layer (what & why) + Plan layer (how).` (Lang=pt-BR: `Aprovado: camada PRD (o quê & porquê) + camada Plano (o como).`)
9. **Branch by `r` suffix:**

   **No `r` (default) — STOP and instruct the user to open a new session:**
   - Do not execute implementation in this session (context already consumed by /feature + picker)
   - Final output:

     ```
     Spec approved and pipeline prepared.
     Open a new session and run /mustard:spec to start implementation with clean context.
     ```

   - **CRITICAL**: do NOT dispatch Task agent, do NOT implement code — just STOP

   **With `r` — jump to resume flow in the same session:**
   - Inform user: `Spec approved. Resuming inline (r suffix). Dispatching EXECUTE directly.`
   - Jump to `resume-flow.md` **Step 2: Bootstrap**
   - **SKIP** Step 0 (Dispatch Failure Pre-Check — does not apply, state was created above) and Step 1 (Detect & Confirm — spec is already known, user just approved)
   - From Step 2 onwards, follow the full resume flow: AUTO-SYNC → Diff Context → Wave System → VALIDATE → REVIEW → QA → CLOSE
   - Apply all INVIOLABLE RULES of resume (main context IS the Pipeline Runner, wave dispatch in single message, etc.)

## Alternative Flow

If the spec is not satisfactory:
- Provide textual feedback for adjustments
- Use /mustard:close to cancel
