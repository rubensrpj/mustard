<!-- mustard:generated -->
# Canonical prompt order — PREFIX-STABLE / VARIABLE

## Why a stable prefix matters

The Anthropic API automatically caches prompt prefixes that are **byte-identical** between calls close together in time. When the cache hits, the cached portion is billed at **10% of normal input cost**. The minimum threshold is around **1024 tokens** (in practice, 1024 characters is a conservative approximation). Without a well-marked stable block, every dispatch is unique at the byte level — the cache never engages and the pipeline pays full input cost for content that repeats (skills, role rules, pipeline-config snippet). Reordering to `[PREFIX-STABLE] → [VARIABLE]` solves this by placing 100% of dynamic content (spec slice, diff, retry context, TASK) **after** the marker, ensuring the prefix stays identical across waves and across dispatches of the same template.

## Canonical order

The `agent-prompt.md` template produces, after interpolation, a file in the following format:

```text
<!-- PREFIX-STABLE -->

## CONTEXT
...skill links (only IDs/names, no inline content)...

## SHARED LANGUAGE
...slice of CONTEXT.md filtered by relevance to the spec ({context_md} — stable across the pipeline)...

## REFERENCE
...files for the agent to read (paths only)...

## SKILLS
...list of available skills (names only; the agent invokes the Skill tool to load each)...

## ROLE
...role rules (static for the template)...

## EFFICIENCY
...efficiency rules (static)...

<!-- VARIABLE -->

## RETRY CONTEXT
...only present on re-dispatches; text varies on every call...

## TASK
...spec slice, prior-wave diff, file list, inline AC...
```

Everything **before** `<!-- VARIABLE -->` must be textually identical between dispatches of the same template for the cache to hit.

## Rules

- **Interpolation inside PREFIX-STABLE can only use stable values.** Skill IDs (`karpathy-guidelines`), role names (`Implementation Agent`) — never the bodies. The agent is responsible for loading the body via the Skill tool when needed.
- **The markers are HTML comments and must be preserved verbatim.** `<!-- PREFIX-STABLE -->` and `<!-- VARIABLE -->` appear literally in the final prompt. Do not wrap them in code, do not translate, do not reformat.
- **Any interpolation of spec text, diff, or retry context inside PREFIX-STABLE invalidates the cache.** If you need to inject dynamic content, do it after the `<!-- VARIABLE -->` marker. If you discover a case where this seems impossible, open an issue before violating the rule — it likely means the template needs to be split.
- **`{context_md}` is the exception that proves the rule.** The glossary slice is *content*, not an ID — but it is **stable within a wave**: the wave's operational spec does not change while it executes, so the slice produced by `context-slice.js` is byte-identical across every dispatch of the same wave. In wave plans, each wave has its own operational spec — the orchestrator regenerates the slice on every wave transition and caches it at `.claude/.pipeline-states/{specName}.context-md.md`. That is why it can live inside PREFIX-STABLE without invalidating the cache during the wave. Content that changes *per dispatch* (spec slice, diff, retry) is still forbidden here.
- **Minimum prefix size: 1024 characters** (≈ 1024 tokens). Smaller prefixes are still textually valid, but they do not activate the cache — the gain stays at 0.

## How to verify

Pipe a rendered prompt from the template into stdin of the script below:

```bash
node -e "const {analyzePrompt}=require('./templates/hooks/_lib/prompt-cache-detect.js'); console.log(analyzePrompt(require('fs').readFileSync(0,'utf8')))"
```

Expected output:

```json
{ "prefix_len": 2814, "prefix_hash": "a1b2c3...", "variable_len": 4120, "prefix_cacheable": true }
```

If `prefix_cacheable` comes back `false`, either the prefix is below 1024 chars or the `<!-- PREFIX-STABLE -->` marker is missing. If `prefix_hash` changes between two dispatches of the same template, some dynamic interpolation leaked into the stable block — review the variables injected before `<!-- VARIABLE -->`.
