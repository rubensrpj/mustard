# Agent Prompt Template — Reference

> **O template literal não vive mais aqui.** Está embedded no binário em `apps/rt/src/run/agent_prompt_template.md` e renderizado por `mustard-rt run agent-prompt-render`. Este ref documenta só o contrato: placeholders, retry modes e a regra de cache. O orquestrador (SKILL `/mustard:spec`) NUNCA monta o prompt à mão.

## Placeholders (preenchidos pelo binário)

| Placeholder | Origem | Notas |
|---|---|---|
| `{subproject}` | flag `--subproject` | Path absoluto ou relativo ao repo. |
| `{spec_lang}` | header `### Lang:` da spec | Default `en` se ausente. Afeta só narrativa da spec — código fica EN. |
| `{guards_summary}` | section `## Guards` de `{subproject}/CLAUDE.md` | Extração via regex. |
| `{context_md}` | `mustard-rt run context-slice` cached em `.claude/.pipeline-states/{spec}.context-md.md` | PREFIX-STABLE — slice é estável p/ pipeline inteiro, refresh só em wave transition. Vazio quando não há `CONTEXT.md` (degrade graceful). |
| `{reference_files}` | recipe matched por nome | 2-3 file refs. |
| `{entity_info}` | `entity-registry.json` por entity name | `_patterns` type + refs + subs. |
| `{role_block}` | flag `--role` + check de `{subproject}/.claude/agents/{role}-impl.md` | Vazio quando custom agent existe (já define role/boundary/validate/return). |
| `{recipe_context}` | recipe matched | Number + pattern refs + reference modules. |
| `{recommended_skills}` | regras em `pipeline-config.md § Skill Recommendations` | Code-editing agents recebem `karpathy-guidelines` prepended; review/explore não. |
| `{task_steps}` | `## Tarefas` / `## Tasks` da wave atual (`mustard-rt` interno) | VARIABLE — muda por wave. |
| `{cross_wave_memory}` | `mustard-rt run memory cross-wave --spec X --wave N` | VARIABLE — vazio para wave 1 ou single-spec. |
| `{retry_context}` | flag `--mode` + opcional `--retry-context-file` | Vazio em `first`; preenchido em `granular`/`fix-loop` (ver Retry Modes). |

## Retry Modes

`mustard-rt run agent-prompt-render --mode <first|granular|fix-loop>` controla qual template é renderizado e o conteúdo de `{retry_context}`:

| Mode | When | Template renderizado | Conteúdo de `{retry_context}` |
|------|------|----------------------|--------------------------------|
| `first` (default) | Primeiro dispatch da wave | **Dispatch Template** (PREFIX-STABLE + VARIABLE) | Vazio |
| `granular` | Step falhou (PARTIAL escalation) | **Minimal Retry Template** (sem CONTEXT/REFERENCE/ENTITY/SKILLS/ROLE/RECIPE) | Header `## RETRY CONTEXT` + `Mode: granular` + `Prior dispatch` + `Files modified` + `Previous error` + `Resume from step` |
| `fix-loop` | Review retornou REJECTED | **Minimal Retry Template** | Header `## RETRY CONTEXT` + `Mode: fix-loop (K/2)` + `Prior dispatch` + `Files modified` + `Review findings (verbatim)` |

`prior_summary` e `files_modified` vêm da última entry de `.agent-memory/_index.json` casando `{agent_type, pipeline}`. Em retry, o binário assume cache do prior context — NÃO injeta CLAUDE.md / guards / registry de novo a menos que `--retry-context-file` diga que algo mudou em disco.

## Prompt Cache Hit (Anthropic API) — por que PREFIX-STABLE vem primeiro

O template embedded tem marcadores `<!-- PREFIX-STABLE -->` e `<!-- VARIABLE -->`. A Anthropic API cacheia automaticamente o prefixo quando dois prompts compartilham ≥1024 tokens byte-identical no começo, cobrando 10% nos hits subsequentes. Para o cache ativar, todo `{placeholder}` dentro de PREFIX-STABLE precisa resolver para valores estáveis entre dispatches da mesma wave (IDs de skill, role, recipe key, subproject path, `{context_md}` da wave). Conteúdo dinâmico (`{task_steps}`, `{cross_wave_memory}`, `{retry_context}`) vai abaixo de `<!-- VARIABLE -->`. A Minimal Retry Template é VARIABLE inteira (sem prefixo cacheável). Detalhes em `prefix-order.md` deste mesmo dir.
