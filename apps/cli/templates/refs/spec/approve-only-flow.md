# /mustard:spec — Approve-only flow

Loaded on demand by `commands/mustard/spec/SKILL.md` Step 7 when the selected spec is in the PLAN stage. Content moved **verbatim** from the former `commands/mustard/approve/SKILL.md` (deleted in TF `2026-05-23-tf-unify-spec-command`), with minimal seam adjustments for the new entry-point.

## Description

Approves the active spec selected by the picker and prepares the implementation phase.

A spec has two named layers (see `/feature` § Full Scope): `## PRD` — the *what & why* (intent) — and `## Plan` — the *how* (execution). Approving a spec approves **both layers at once**: there is no separate gate to "approve PRD". The two-layer separation is a reading aid, not a second checkpoint — keep it that way.

- **No `r` suffix** (`/mustard:spec {letter}` with PLAN stage): prepares the pipeline state and STOPS, instructing the user to open a new session and run `/mustard:spec {letter}` again to continue with clean context. Recommended for Full-scope specs with 5+ files.
- **With `r` suffix** (`/mustard:spec {letter}r`): after preparation, immediately jumps to the `resume-flow.md` flow in the same session (skips Step 0 and Step 1 of resume — no dispatch failure check, no handoff summary, no reconfirmation). Use when the spec was just approved and you want to avoid the session restart hop. Tradeoff: the EXECUTE phase inherits the ANALYZE+PLAN context instead of starting clean — ok for small/medium specs, less efficient for large ones.

## Prerequisites

- Active spec in `.claude/spec/{name}/` (flat layout — lifecycle state read from the `meta.json` sidecar / the event-log projection; `spec.md` is pure narrative)
- The spec has been shown to the user and they picked the corresponding letter in the `/mustard:spec` picker

## Action

1. **Step 0: AUTO-SYNC (mandatory)** — already executed in Step 1 of `/mustard:spec`. Do not re-execute.
2. **Read** `.claude/pipeline-config.md` — agents, routing rules.
3. The spec has already been located by the `/mustard:spec` picker (filtered by the `meta.json` `stage` + `outcome` — only Outcome `Active` AND Stage ∈ {Plan, Execute}).

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
   - **"Approve wave plan — start with wave 1"** → proceed to step 4 (updates the wave-1 `meta.json` + pipeline state for wave 1 dispatch)
   - **"Reject decomposition"** → **scope-dependent**, performed **deterministically** by a single relay (do NOT concatenate sections / delete dirs / patch sidecars by hand). Run the command below with `--mode` set from the spec's scope, then act on its JSON (`{"ok":true,"mode":"...","waves_merged":N,"removed_dirs":[...]}`; on `{"ok":false,"reason":"no-wave-plan"}` the spec is not a wave plan — fall through to the single-spec path). The command merges the actionable sections (`## Files`/`## Arquivos`, `## Tasks`/`## Tarefas`, `## Boundaries`/`## Limites`) in wave order, de-dups file lines, writes the merged spec **before** deleting anything, and patches the sidecars:
     ```bash
     mustard-rt run wave-collapse --spec {specName} --mode {full|light}
     ```
     - **Full scope** (`--mode full`) → *colapsa* o wave-plan para uma **single wave** (uma wave): the parent spec stays an **orchestration/coordination doc** (no own `## Tarefas`/`## Checklist`), the N wave-specs are collapsed into a single `wave-1-{role}/`, the surplus `wave-N-*/` subdirs are deleted, `wave-plan.md` is kept with `isWavePlan: true` / `totalWaves: 1`, and `scopeOverride: "user-rejected-waves"` is set. Then proceed to step 4 on the wave-1 spec. **NEVER** collapses a Full spec to `isWavePlan: false` / zero waves — the invariant is **Full ⇒ ≥1 wave** (parent=orchestrator, wave=subagent); the command enforces it. The runtime gate `block_full_without_wave` (`post_execute_gate.rs`) backs this up: it refuses a Full spec from reaching Execute without ≥1 wave, so the prose and the safety net agree.
     - **Light scope** (`--mode light`) → merges all wave specs back into a single spec at `.claude/spec/{specName}/spec.md`, deletes `wave-plan.md` and every `wave-N-*/` subdir, and sets `scopeOverride: "user-rejected-waves"` + `isWavePlan: false`. Then proceed to step 4 on the single spec. (Single-spec / `isWavePlan: false` / zero waves is valid **only** for Light.)
   - **"Stop — re-plan with guidance"** → stop. Instruct user: `Delete .claude/spec/{specName}/ and re-run /feature {name} with explicit guidance (e.g., "keep wave 2 and wave 3 together").`
4. If the wave plan was approved, the approval in step 5 operates on the **wave 1 spec** — pass `--wave-plan` so `mustard-rt` patches `.claude/spec/{specName}/wave-1-{role}/meta.json` for dispatch (not the `wave-plan.md` sidecar).

**If `wave-plan.md` does NOT exist:** proceed as single spec (behavior below).

5. **Approve — emit the deterministic approval sequence (single relay):**
   - Extract `spec-name` from the spec directory (basename of the path → e.g. `2026-02-26-linked-services-card`).
   - Run the one command below and act on its JSON (`{"ok":true,"spec":"...","approved":true,"resumed":<bool>}`); on `{"ok":false,"error":"..."}`, surface the error and stop. It emits — deterministically, in order — `pipeline.stage {stage:"Plan"}` then `pipeline.status {from:"draft",to:"approved"}`, patches the spec's `meta.json` sidecar (`stage: Plan`, `outcome: Active`, `checkpoint: {ISO now}`; `scope`/`lang`/`parent` preserved — never hand-edit `spec.md`), and — when the user used the `r` suffix (the With-`r` branch of step 8) — `--resume` makes it also emit `pipeline.stage {stage:"Execute"}`. The `r` suffix is part of the invocation, so you know it here: pass `--resume` whenever the user typed `/mustard:spec {letter}r`. Append `--wave-plan` when step 4 detected a wave plan.
   ```bash
   mustard-rt run approve-spec --spec {spec-name} [--wave-plan] [--resume]
   ```
   - No JSON file is written here.
5b. **Memory Persist — record architectural decisions:**
   - For each significant decision in the spec (technology choices, design patterns, trade-offs):
     ```bash
     echo '{"type":"decision","content":"<decision description>","source":"<spec-name>","context":"approved at PLAN phase"}' | mustard-rt run memory decision
     ```
   - Focus on: why a pattern was chosen over alternatives, constraints that shaped the design
   - Skip trivial or obvious decisions (max 3 entries)
6. **Task Tracking — create TaskCreate for each agent:**
   - 1 TaskCreate per agent identified in the spec
   - Subject: `"{Layer}: {brief description}"`
   - activeForm: `"Running {Layer} agent"`
7. **Output — visual feedback:**
   - Print progress line: `[v] ANALYZE  [v] PLAN  [>] EXECUTE  [ ] CLOSE`
   - Print a layer signal line so the user knows what was approved:
     `Approved: PRD layer (what & why) + Plan layer (how).` (Lang=pt-BR: `Aprovado: camada PRD (o quê & porquê) + camada Plano (o como).`)
8. **Branch by `r` suffix:**

   **No `r` (default) — STOP and instruct the user to open a new session:**
   - Do not execute implementation in this session (context already consumed by /feature + picker)
   - Final output:

     ```
     Spec approved and pipeline prepared.
     Open a new session and run /mustard:spec to start implementation with clean context.
     ```

   - **CRITICAL**: do NOT dispatch Task agent, do NOT implement code — just STOP

   **With `r` — jump to resume flow in the same session:**
   - `approve-spec --resume` (step 5) already emitted the `pipeline.stage {stage:"Execute"}` transition — do NOT re-emit it.
   - Inform user: `Spec approved. Resuming inline (r suffix). Dispatching EXECUTE directly.`
   - Jump to `resume-flow.md` **Step 2: Bootstrap**
   - **SKIP** Step 0 (Dispatch Failure Pre-Check — does not apply, state was created above) and Step 1 (Detect & Confirm — spec is already known, user just approved)
   - From Step 2 onwards, follow the full resume flow: AUTO-SYNC → Diff Context → Wave System → VALIDATE → REVIEW → QA → CLOSE
   - Apply all INVIOLABLE RULES of resume (main context IS the Pipeline Runner, wave dispatch in single message, etc.)

## Alternative Flow

If the spec is not satisfactory:
- Provide textual feedback for adjustments
- Use /mustard:close to cancel
