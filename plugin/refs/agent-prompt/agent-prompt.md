# Agent Prompt Template — Reference

> **The literal template lives in the binary** (`apps/rt/src/commands/agent/agent_prompt_template.md`), rendered by `mustard-rt run agent-prompt-render`. This ref documents the contract only — the `subagent_type` map, placeholders, retry modes, the caching rule. The orchestrator NEVER assembles the prompt by hand; `--emit ref` returns a 2-line `MUSTARD-PROMPT-REF` stub the PreToolUse hook expands at dispatch (the full prompt never transits the orchestrator context).

## subagent_type by role

The dispatch planner (`wave-advance` items carry the field) picks the agent per role via `recommended_subagent_type` — read-only roles run tool-restricted so they physically cannot write; writing roles rely on the per-role contract + the `scope_guard` hook. Agents inherit the session model (no routing table).

| Role | `subagent_type` | Tools |
|---|---|---|
| `explore` | `Explore` | read-only (no Edit/Write) |
| `plan` | `Plan` | read-only (no Edit/Write) |
| `review` / `qa` | `mustard:mustard-review` | Read/Grep/Glob/Bash (tests only) |
| `guards` | `mustard:mustard-guards` | Read/Grep/Glob |
| `patterns` | `mustard:mustard-patterns` | Read/Grep/Glob |
| `impl` / any other | `general-purpose` | Edit/Write (+ `scope_guard`) |

This is the canonical role→`subagent_type` map — other command refs point here rather than repeat it. Plugin-owned agents carry the `mustard:` namespace (Claude Code registers them under the `plugin.json` `name`; a bare `mustard-review` silently falls back to `general-purpose`). Built-in agents (`Explore`, `Plan`, `general-purpose`) stay unprefixed.

## Placeholders (filled by the binary)

The placeholders the renderer substitutes — `TEMPLATE_PLACEHOLDERS` in `apps/rt/src/commands/agent/render/mod.rs`, which IS the substitution list, not a copy of it — in template order. The table below is pinned to that constant by the `agent_prompt_ref_documents_every_placeholder` drift guard: a placeholder added to the renderer and not to this table fails the build, because a reader planning a wave cannot use what the reference never mentions.

| Placeholder | Source | Notes |
|---|---|---|
| `{subproject}` | `--subproject` | Absolute or repo-relative path. |
| `{guards_file}` | `shared::context::guards_file_name` | The instruction file THIS install owns — `CLAUDE.md` normally, `CLAUDE.local.md` under a private install, where the scan writes beside the host repository's own file instead of into it. The prompt names it rather than spelling `CLAUDE.md`, so a dispatched agent is never sent to open the client's file. |
| `{guards_summary}` | `## Guards` of `{subproject}/{guards_file}` | Extracted via regex; empty when the file has no `## Guards`. |
| `{role_block}` | `--role` (`build_role_block` / `build_guards_role_block`) | The role cue **plus** a per-role delivery contract (what to produce, return-cap, read-only vs write). |
| `{spec_lang}` | spec `meta.json#lang` | Defaults to `en`; affects only the narrative — code stays EN. |
| `{task_steps}` | `## Tasks` of the wave, or `--task-text` when spec-less (`/scan` guards, `/task`) | VARIABLE — per wave; `--task-text` fills `## TASK` so the prompt stays self-contained (never hand-append the task). |
| `{context_md}` | `mustard-rt run context-slice` (cached, refreshed per wave) | Stable across a wave. Empty when no `CONTEXT.md` glossary exists (opt-in via `grill-with-docs`) — blank by design, not a failure. |
| `{prior_wave_diff}` | per-wave `diff.md` (`git diff HEAD~1 HEAD --stat`, cached by `wave-done`) | VARIABLE — empty on wave 1 or when the diff is empty. |
| `{change_log}` | spec `change-log.md` request bullets | VARIABLE — mid-pipeline change requests; empty when none. |
| `{reality_obligations}` | the wave's `## Reality Obligations` (materialised from the plan JSON's per-wave `reality_obligations`) | VARIABLE — the duties this wave owes the WORLD outside the repository, each with its `RO-{n}.{i}` id; the rendered body tells the agent to account for each BY ID in its report, and `wave-done` names the ids nothing it recorded accounts for. Empty when the plan declared none. |
| `{conversation_material}` | the PARENT spec's `## Definitions` / `## Decisions` / `## Evidence` (written by `spec-draft --material`), cut for THIS wave | VARIABLE — see the per-wave cut below; empty when the spec carries no material or nothing survives the cut. |
| `{cross_wave_memory}` | renderer-internal (capability blocks + spec-memory `<spec>/memory/*.md` + vocabulary regression) | VARIABLE — empty when none apply. The memory files are PROCESS memory, written by `wave-done` as each wave closes; see below. |
| `{reference_files}` | scan-derived neighbours — the spec's `## Files`/`## Arquivos` list + those files' public signatures (tree-sitter) | 2-3 file references. |
| `{skills_list}` | the subproject's skill shelf — names + trigger descriptions, never bodies | The agent loads each via the Skill tool; empty for the `patterns` role by design. |
| `{retry_context}` | renderer-composed (`compose_retry_context`): last `review.result` verdict + critical count, last `pipeline.wave.failed` signal, the persisted findings for THIS subproject (`<spec>/review/findings-{sub}.md`, falling back to the spec-wide `findings.md` only while no scoped file exists), prior-wave diff, change log | Empty in `first`; composed in `granular`/`fix-loop`; `--retry-context-file` overrides with hand-supplied text. |

## `## CONVERSATION MATERIAL` — the per-wave cut

What the conversation established before the spec existed (`spec-draft --material`) lives ONCE, in the PARENT spec. A per-wave copy would drift, so the cut happens here, at render time. Each kind has a different natural key:

| Kind | Parent section | Reaches | Why |
|---|---|---|---|
| Definitions | `## Definitions` | **EVERY wave** | The shared vocabulary. Cut it and each wave invents its own word for the same thing again. |
| Decisions | `## Decisions` | **EVERY wave** | The law of the work ("everything branches off dev") binds every wave, not one. |
| Findings | `## Evidence` | **only the wave that DECLARES the file** | A finding carries a file, so the file is the key: a wave receives the evidence about the code it is about to touch, and no other. |

The finding cut is a set intersection over the wave's declared `## Files` — the same list the reference-file builder reads, so the cut cannot disagree with the rest of the pipeline. Two consequences a reader planning a wave needs:

- **Record the file, or the finding reaches nobody.** A finding with no evidence path cannot be attributed to any wave and is DROPPED. Matching is segment-anchored suffix containment, not string equality: a subproject-relative `## Files` entry (`src/alpha.rs`) still matches a repo-relative evidence path (`apps/rt/src/alpha.rs`), which is what makes the cut work in a monorepo.
- **Declaring a file in a wave is also how you ask for its evidence.** A wave that declares none of the evidence files still receives the definitions and the decisions, and gets no `### Evidence` sub-heading at all — an empty kind contributes no heading.

The block rides in the VARIABLE region (after `## EFFICIENCY`), so carrying the material never touches the byte-identical stable head — two renders of the same spec that differ only in their per-wave findings share the same cached prefix. A spec with no material renders a prompt byte-identical to one from before this channel existed: the heading collapses like any other empty section.

Do not confuse it with `## CROSS-WAVE MEMORY`. **Conversation material is PROJECT-side**: authored before the spec, including any earlier-spec memory the author judged relevant and cited. **`<spec>/memory/*.md` is PROCESS memory**: strictly intra-run, written by `wave-done` as each wave closes (verbatim `<MEMORY>` bodies, frontmatter naming the wave, run and timestamp), so a lesson learned in wave 1 reaches wave 3 instead of dying in the event log. Nothing summarises or infers either one.

Why `## SKILLS` is a shelf and not the native per-agent skill preload: the native preload is static in the agent definition and injects skill BODIES — both would break the per-subproject selection and the PREFIX-STABLE byte-identical head; the shelf is computed per subproject and carries names + trigger descriptions only (the agent loads a body on demand via the Skill tool).

## Retry Modes

`agent-prompt-render --mode <first|granular|fix-loop>` picks the template and fills `{retry_context}`:

| Mode | When | Template | `{retry_context}` |
|---|---|---|---|
| `first` (default) | first dispatch of the wave | Dispatch (`<!-- PREFIX-STABLE -->`) | empty |
| `granular` | a step failed (PARTIAL) | Minimal Retry (no CONTEXT/REFERENCE/ROLE) | composed `## RETRY CONTEXT` (see below); pair with `--task-filter` to re-dispatch only the remaining steps |
| `fix-loop` | review REJECTED | Minimal Retry | composed `## RETRY CONTEXT` — the review findings ride here |

In both retry modes the renderer composes `## RETRY CONTEXT` from what the pipeline already recorded: the last `review.result` (verdict + critical count), the last `pipeline.wave.failed` signal, `<spec>/review/findings.md` (persisted when `review-result` runs with `--findings-file` — the loop's review step does this), the prior-wave diff and the change log. All-empty ⇒ the heading collapses. `--retry-context-file` overrides the composition with hand-supplied text. The retry template is minimal by design — it does NOT re-inject CONTEXT/GUARDS (the retry rides in the same conversation as the first dispatch of that agent role).

## PREFIX-STABLE ordering (prompt-cache rule)

The embedded file holds two `<!-- TEMPLATE: … -->` blocks — **preserve every `<!-- TEMPLATE -->`, `<!-- PREFIX-STABLE -->` and `<!-- VARIABLE -->` marker verbatim** (never wrap, translate, or reformat them).

**`dispatch`** — labeled `<!-- PREFIX-STABLE -->`; the full first-dispatch prompt. Canonical section order:

```text
<!-- PREFIX-STABLE -->
## CONTEXT           (static ground rules: Guards pointer, sibling check, spec language)
## GUARDS            ({guards_summary})
## SHARED LANGUAGE   ({context_md} slice — stable across the wave)
## REFERENCE         ({reference_files} — paths + signatures)
## SKILLS            ({skills_list} — names + trigger descriptions, never bodies; empty for `patterns`)
## WEB VALIDATION    (static)
## ROLE              ({role_block})
## EFFICIENCY        (static)
## CONVERSATION MATERIAL ({conversation_material} — the parent spec's material, cut for this wave)
## CROSS-WAVE MEMORY ({cross_wave_memory})
## PRIOR WAVE DIFF   ({prior_wave_diff})
## CHANGE REQUESTS   ({change_log})
## REALITY OBLIGATIONS ({reality_obligations} — duties to check the world outside the repo)
## TASK              ({task_steps} — spec slice / --task-text)
```

**`retry`** — labeled `<!-- VARIABLE -->`; the minimal re-dispatch prompt: `## RETRY CONTEXT` (`{retry_context}`) → `## EFFICIENCY` → `## TASK`. Selected by `--mode granular|fix-loop`.

A `## ` section whose placeholder body resolves to "" is dropped (`collapse_empty_sections`) — typically `## GUARDS`, `## SHARED LANGUAGE`, `## REFERENCE`, `## SKILLS`, `## CONVERSATION MATERIAL`, `## CROSS-WAVE MEMORY`, `## PRIOR WAVE DIFF`, `## CHANGE REQUESTS`, `## REALITY OBLIGATIONS` on the spec-less / wave-1 / no-material / no-Files / no-duty / `patterns` paths; `## TASK` always survives (its trailing line is non-blank body).

Prompt-cache rule: the Anthropic API bills a byte-identical prefix (≥1024 tokens; ~1024 chars is a safe floor) at 10% of input on nearby calls. The stable head of `dispatch` (`## CONTEXT`…`## EFFICIENCY`) is reused across a wave's dispatches; the per-dispatch tail (`## CONVERSATION MATERIAL`, `## CROSS-WAVE MEMORY`, `## PRIOR WAVE DIFF`, `## CHANGE REQUESTS`, `## REALITY OBLIGATIONS`, `## TASK`) changes each round. That is why the per-wave material cut sits below the line and not above it: its content differs per wave by construction, so placing it in the head would defeat the cache for every dispatch of every spec that carries any. `{context_md}` is *content* but byte-identical across a wave (regenerated + cached on each wave transition), so it rides in the stable head. A prefix below 1024 chars is still valid — it just does not cache (gain 0).
