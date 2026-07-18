<!-- TEMPLATE: dispatch -->
<!-- PREFIX-STABLE -->
## CONTEXT
1. Read the `## Guards` section of `{subproject}/CLAUDE.md` — mandatory rules (also inlined under `## GUARDS` below)
2. **Sibling-convention check (MANDATORY before first Edit/Write):** for each file you will modify, read ONE neighbouring file in the same directory first to confirm conventions (shebang, license header, async/sync style, error pattern, import order, indentation). Skip only for: NEW directories with no siblings; pure JSON/YAML edits; spec markdown. Cost: ≤1 extra Read per edit target, saves reviewer warnings about "decorative async over blocking", "Bun check after initStore", "import duplicated", etc.
3. Spec language is `{spec_lang}` — applies to spec narrative ONLY (prose, labels, Concerns you append). Source code stays English regardless: identifiers, comments in every form (`//`, `#`, `/* */`, `///`, `'''`, `"""`, doc-comments, `<!-- -->`), file paths, shell commands, AC `Command:` content, log messages. Surgical: never translate pre-existing comments — only write new ones in English.

## GUARDS
{guards_summary}

## SHARED LANGUAGE
{context_md}

## REFERENCE
{reference_files}

## SKILLS
{skills_list}

## WEB VALIDATION
In doubt about API/version/pattern → search web for latest docs before implementing.

## ROLE
{role_block}

## EFFICIENCY
- Absolute paths, no cd
- Read each file once
- Start from the anchors/context already handed to you above (`## REFERENCE`, `## TASK`) — do NOT re-derive or re-Grep what you were given; locate from them, then read
- Max 3 build attempts, then STOP + report
- Return cap: follow pipeline-config.md Max Return limits (impl 40, explore 30, review 60, plan 80 lines). Return a condensed summary (files changed + non-obvious decisions + blockers) — not a re-grounding of what you read.

## CROSS-WAVE MEMORY
{cross_wave_memory}

## PRIOR WAVE DIFF
{prior_wave_diff}

## CHANGE REQUESTS
{change_log}

## TASK
{task_steps}

Guards carregados via CLAUDE.md acima — respeite sem exceção.
<!-- /TEMPLATE: dispatch -->

<!-- TEMPLATE: retry -->
<!-- VARIABLE -->
## RETRY CONTEXT
{retry_context}

## EFFICIENCY
- Absolute paths, no cd
- Read each file once (prior context cached — skip CLAUDE.md/guards/registry re-reads unless file changed on disk)
- Max 3 build attempts, then STOP + report
- Return cap: follow pipeline-config.md Max Return limits. Focus on: files changed + non-obvious decisions + blockers only.

## TASK
{task_steps}

Guards carregados via CLAUDE.md acima — respeite sem exceção.
<!-- /TEMPLATE: retry -->
