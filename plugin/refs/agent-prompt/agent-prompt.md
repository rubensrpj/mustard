# Agent Prompt Template — Reference

> **The literal template lives in the binary** (`apps/rt/src/commands/agent/agent_prompt_template.md`), rendered by `mustard-rt run agent-prompt-render`. This ref documents the contract only — the `subagent_type` map, placeholders, retry modes, the caching rule. The orchestrator NEVER assembles the prompt by hand; `--emit ref` returns a 2-line `MUSTARD-PROMPT-REF` stub the PreToolUse hook expands at dispatch (the full prompt never transits the orchestrator context).

## subagent_type by role

The dispatch planner (`wave-advance` items carry the field) picks the agent per role — read-only roles run tool-restricted: `explore`→`Explore`, `review`/`qa`→`mustard-review`, `guards`→`mustard-guards`; writing roles → `general-purpose`. Agents inherit the session model (no routing table).

## Placeholders (filled by the binary)

| Placeholder | Source | Notes |
|---|---|---|
| `{subproject}` | `--subproject` | Absolute or repo-relative path. |
| `{spec_lang}` | spec `meta.json#lang` | Defaults to `en`; affects only the narrative — code stays EN. |
| `{guards_summary}` | `## Guards` of `{subproject}/CLAUDE.md` | Extracted via regex. |
| `{context_md}` | `mustard-rt run context-slice` (cached, refreshed per wave) | PREFIX-STABLE — stable across a wave. Empty when no `CONTEXT.md` glossary exists (opt-in via `grill-with-docs`) — blank by design, not a failure. |
| `{reference_files}` | scan-derived neighbours | 2-3 file references. |
| `{role_block}` | `--role` | The role cue **plus** a per-role delivery contract (what to produce, return-cap, read-only vs write). |
| `{task_steps}` | `## Tasks` of the wave, or `--task-text` when spec-less (`/scan` guards, `/task`) | VARIABLE — per wave; `--task-text` fills `## TASK` so the prompt stays self-contained (never hand-append the task). |
| `{cross_wave_memory}` | renderer-internal (capability blocks + spec-memory + vocabulary regression) | VARIABLE — empty when none apply. |
| `{retry_context}` | `--mode` + optional `--retry-context-file` | Empty in `first`; filled in `granular`/`fix-loop`. |

## Retry Modes

`agent-prompt-render --mode <first|granular|fix-loop>` picks the template and fills `{retry_context}`:

| Mode | When | Template | `{retry_context}` |
|---|---|---|---|
| `first` (default) | first dispatch of the wave | Dispatch (PREFIX-STABLE + VARIABLE) | empty |
| `granular` | a step failed (PARTIAL) | Minimal Retry (no CONTEXT/REFERENCE/ROLE) | `Mode: granular` + prior dispatch + files + previous error + resume-from-step |
| `fix-loop` | review REJECTED | Minimal Retry | `Mode: fix-loop (K/2)` + prior dispatch + files + review findings verbatim |

`prior_summary` / `files_modified` are filled by the renderer from the spec's events; on retry it assumes the prior context is cached and does NOT re-inject CLAUDE.md / guards unless `--retry-context-file` flags an on-disk change.

## PREFIX-STABLE ordering (prompt-cache rule)

The Anthropic API caches a prompt prefix that is byte-identical between nearby calls (≥1024 tokens; ~1024 chars is a safe floor), billing subsequent hits at 10% of input. The embedded template marks the split with literal HTML comments — **preserve `<!-- PREFIX-STABLE -->` and `<!-- VARIABLE -->` verbatim** (never wrap, translate, or reformat them). Canonical order:

```text
<!-- PREFIX-STABLE -->
## CONTEXT          (skill IDs only, no bodies)
## SHARED LANGUAGE  ({context_md} slice — stable across the wave)
## REFERENCE        (file paths only)
## SKILLS           (names only; the agent loads each via the Skill tool)
## ROLE / ## EFFICIENCY   (static)
<!-- VARIABLE -->
## RETRY CONTEXT    (re-dispatches only)
## TASK             (spec slice, prior-wave diff, file list, inline AC)
```

Rules: interpolation inside PREFIX-STABLE may use only stable values (skill IDs, role names — never the bodies). `{context_md}` is the exception — it is *content*, but byte-identical across a wave (regenerated + cached on each wave transition), so it may live in the prefix. Anything that changes per dispatch (spec slice, diff, retry context) MUST sit after `<!-- VARIABLE -->`. A prefix below 1024 chars is still valid — it just does not cache (gain 0).
