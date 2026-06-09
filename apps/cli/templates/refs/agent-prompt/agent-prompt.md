# Agent Prompt Template — Reference

> **The literal template no longer lives here.** It is embedded in the binary at `apps/rt/src/commands/agent/agent_prompt_template.md` and rendered by `mustard-rt run agent-prompt-render`. This reference only documents the contract: placeholders, retry modes, and the caching rule. The orchestrator (SKILL `/mustard:spec`) NEVER assembles the prompt by hand.

## Placeholders (filled by the binary)

| Placeholder | Source | Notes |
|---|---|---|
| `{subproject}` | flag `--subproject` | Absolute path or path relative to the repo. |
| `{spec_lang}` | spec `meta.json#lang` | Defaults to `en` when absent. Affects only the spec narrative — code stays EN. |
| `{guards_summary}` | `## Guards` section of `{subproject}/CLAUDE.md` | Extracted via regex. |
| `{context_md}` | `mustard-rt run context-slice` cached at `.claude/.pipeline-states/{spec}.context-md.md` | PREFIX-STABLE — the slice is stable across the whole pipeline, refreshed only on a wave transition. Empty when no `CONTEXT.md` domain glossary has been authored (opt-in via `grill-with-docs`) — blank by design, not a failure. |
| `{reference_files}` | scan-derived neighbour files | 2-3 file references. |
| `{role_block}` | flag `--role` | The role cue **plus a per-role delivery contract** (what to produce + how to deliver: return text vs. edit, return-cap, read-only vs. write). The `subagent_type` is picked per role by the dispatch planner (`wave-advance` items carry it; `dispatch-plan` exposes it as `recommended_subagent_type`): read-only roles run tool-restricted (`explore`→`Explore`, `review`/`qa`→`mustard-review`, `guards`→`mustard-guards`); writing roles → `general-purpose`. The `## ENTITY` / `## SKILLS` sections (and the dead `{entity_info}` / `{recommended_skills}` placeholders) were removed from the template. |
| `{task_steps}` (spec-less) | flag `--task-text` | When there is no spec `## Tasks` to read (`/scan` guards, `/task`), `--task-text` fills `## TASK` so the prompt stays self-contained — the orchestrator never hand-appends the task. |
| `{task_steps}` | `## Tasks` of the current wave (`mustard-rt` internal) | VARIABLE — changes per wave. |
| `{cross_wave_memory}` | `mustard-rt run memory cross-wave --spec X --wave N` | VARIABLE — empty for wave 1 or single-spec runs. |
| `{retry_context}` | flag `--mode` + optional `--retry-context-file` | Empty in `first`; filled in `granular`/`fix-loop` (see Retry Modes). |

## Retry Modes

`mustard-rt run agent-prompt-render --mode <first|granular|fix-loop>` controls which template is rendered and the contents of `{retry_context}`:

| Mode | When | Rendered template | Contents of `{retry_context}` |
|------|------|-------------------|--------------------------------|
| `first` (default) | First dispatch of the wave | **Dispatch Template** (PREFIX-STABLE + VARIABLE) | Empty |
| `granular` | A step failed (PARTIAL escalation) | **Minimal Retry Template** (no CONTEXT/REFERENCE/ROLE) | Header `## RETRY CONTEXT` + `Mode: granular` + `Prior dispatch` + `Files modified` + `Previous error` + `Resume from step` |
| `fix-loop` | Review returned REJECTED | **Minimal Retry Template** | Header `## RETRY CONTEXT` + `Mode: fix-loop (K/2)` + `Prior dispatch` + `Files modified` + `Review findings (verbatim)` |

`prior_summary` and `files_modified` come from the last entry in `.agent-memory/_index.json` matching `{agent_type, pipeline}`. On retry, the binary assumes the prior context is cached — it does NOT re-inject CLAUDE.md / guards unless `--retry-context-file` indicates something changed on disk.

## Prompt Cache Hit (Anthropic API) — why PREFIX-STABLE comes first

The embedded template has `<!-- PREFIX-STABLE -->` and `<!-- VARIABLE -->` markers. The Anthropic API automatically caches the prefix when two prompts share ≥1024 byte-identical tokens at the start, charging 10% on subsequent hits. For the cache to engage, every `{placeholder}` inside PREFIX-STABLE must resolve to values stable across dispatches of the same wave (role, subproject path, the wave's `{context_md}`). Dynamic content (`{task_steps}`, `{cross_wave_memory}`, `{retry_context}`) goes below `<!-- VARIABLE -->`. The Minimal Retry Template is fully VARIABLE (no cacheable prefix). Details in `prefix-order.md` in this same directory.
