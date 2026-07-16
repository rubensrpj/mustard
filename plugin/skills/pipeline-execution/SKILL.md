---
name: pipeline-execution
description: EXECUTE-phase specifics for /feature and /spec — the diff must obey the layer Guards + {role}-pattern molds, the once-per-spec spec-memory relevance gate, and capability authoring. Use when dispatching or implementing a wave; the wave-advance dispatch loop itself lives in refs/spec/resume-loop.md § B.
tags: [plan, any]
appliesTo: []
scope: [plan, code-editing]
metadata:
  generated_by: foundation
disable-model-invocation: true
source: manual
---

# Pipeline Execution — EXECUTE specifics

> The dispatch loop (wave-advance relay, MUSTARD-PROMPT-REF, review-result, escalation) is owned by `${CLAUDE_PLUGIN_ROOT}/refs/spec/resume-loop.md § B`; phases, role rules, gates and escalation statuses by `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md`. This file holds only what is unique to EXECUTE.

## Law — the diff obeys the layer

Code lands in the shape the layer already has: the subproject `## Guards` and its `{role}-pattern` molds are LAW for the diff. A diff that violates either is wrong even if it compiles and passes tests. Divergence from a local pattern is the owner call — flag it in the report, never impose it in the diff. Red flags to stop on: inventing a folder/naming scheme mid-task; copying a pattern from another project; justifying a Guard violation in a code comment instead of the report.

## Spec-memory relevance gate (once per spec)

Run before the first dispatch round; skip when `.claude/spec/{spec}/memory/` is empty:

1. Read each principle frontmatter `name` + `description` directly.
2. Dispatch a throwaway read-only judge — `Task(general-purpose, model: haiku)` — with the spec goal + the `name — description` list inline: "Return ONLY the names relevant to this spec work, one per line. When unsure, EXCLUDE." Haiku is the one deliberate exception to inherit-session-model (a cheap relevance judge, not pipeline work — `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Model`).
3. Write the approved names to `.claude/spec/{spec}/.memory-approved` — write it even when empty (empty = inject none, honoured). No file → the deterministic recall matcher is the fallback. Re-run only if `memory/` changes.

## Capabilities (post-EXECUTE — most specs skip)

Only when the feature created or changed a user-visible behaviour: `mustard-rt run capability create --slug {slug} --title "{title}"`, edit `.claude/capabilities/{slug}.md` (`### Requirement:` / `#### Scenario:` when/then blocks + `## Covers`), link `- [[cap.{slug}]]` in the spec `## Capabilities`. CLOSE folds it back. Absent section = no-op.
