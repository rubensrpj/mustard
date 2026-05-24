# Enhancement: retry-context-fix-loop-aware

## Summary
Enriquecer o placeholder `{retry_context}` do template de dispatch para reutilizar o trabalho da dispatch anterior em fix-loops (review REJECTED → fix agent) e granular retries (step failure). Em retry mode, o template substitui o bloco pesado de CONTEXT/REFERENCE/ENTITY/SKILLS/RECIPE por um cabeçalho mínimo que referencia prior-dispatch memory + files modificados + findings verbatim da review. Objetivo: reduzir wall clock (~30-50%), liberar context headroom do agent (~30%) e cortar re-scans redundantes. Billed tokens caem ~20-30% (cache já absorve boa parte dos re-Reads). Não altera L0, roteamento sonnet, review mandatório, ou limite de 2 fix-loops.

## Boundaries
- `templates/commands/mustard/templates/agent-prompt/SKILL.md` — template file (modify)
- `templates/commands/mustard/resume/SKILL.md` — Granular Retry + Fix Loop Dispatch (modify)
- `templates/commands/mustard/feature/SKILL.md` — EXECUTE Light REVIEW step (modify)

## Checklist

### templates Agent (Wave 1)
- [x] Em `templates/commands/mustard/templates/agent-prompt/SKILL.md`: adicionar seção `## Retry Modes` documentando os 3 estados de `{retry_context}` (empty | granular | fix-loop) e o template mínimo de retry (sem CONTEXT/REFERENCE/ENTITY/SKILLS/RECIPE). Especificar que em retry, o agent NÃO deve re-Read CLAUDE.md/guards/registry — prior cache é assumido válido.
- [x] No mesmo arquivo: ajustar a seção "Dispatch Template" para deixar explícito que os blocos CONTEXT/REFERENCE/ENTITY/SKILLS/RECIPE são **condicionais** — omitidos quando `{retry_context}` é fix-loop ou granular. Adicionar um bloco `{retry_template}` alternativo (ou marcar os atuais como "first-dispatch only").
- [x] Em `templates/commands/mustard/resume/SKILL.md` Step 14: atualizar a instrução de preenchimento de `{retry_context}` para referenciar os 3 modes + apontar para novo Step 19b.
- [x] No mesmo arquivo, §Granular Retry Protocol: rewrite passo 3 (Re-dispatch with retry context) para usar o novo formato enriquecido — inclui `prior_summary` (lido de `.agent-memory/_index.json`), `prior_files_modified` (lista), `previous_error`, `resume_from_step`.
- [x] No mesmo arquivo, após Step 19 (REVIEW): adicionar Step 19b `### Fix Loop Dispatch Protocol`. Documenta: quando REJECTED, ler a última entry relevante de `.agent-memory/_index.json` para o agent_type rejeitado; compor `{retry_context}` mode=fix-loop com `prior_summary`, `prior_files_modified`, `findings_verbatim` (copiar CRITICAL+WARNING da review exatamente como retornados); dispatch com MESMO subagent_type + model; usar template mínimo (sem CONTEXT/REFERENCE/ENTITY/SKILLS/RECIPE).
- [x] Em `templates/commands/mustard/feature/SKILL.md` EXECUTE Phase Light step 9: trocar linha `REJECTED → fix + re-review (max 2 loops)` por referência explícita: "REJECTED → ver `resume/SKILL.md § Fix Loop Dispatch Protocol` (max 2 loops)". Evita duplicação de contrato.
- [x] Build/type-check: `npm run build` (na raiz do Mustard)

## Files (~3)
- `templates/commands/mustard/templates/agent-prompt/SKILL.md` (modify)
- `templates/commands/mustard/resume/SKILL.md` (modify)
- `templates/commands/mustard/feature/SKILL.md` (modify)

## Non-Goals
- NÃO alterar `templates/commands/mustard/bugfix/SKILL.md` — tem Fast Path próprio com shape diferente; pode adotar o mesmo pattern em spec futura se necessário.
- NÃO alterar hooks, scripts, ou settings.json.
- NÃO adicionar novo hook/skill — subtração preferida (menos instruções, não mais).
- NÃO tocar `templates/skills/pipeline-execution/SKILL.md` — referência de alto nível sem fill logic.

## Decisões não-óbvias
- **Savings honestas:** cacheRead do Anthropic já absorve ~95% dos re-Reads (sessão analisada: fix agent 1.02M total, 965k cacheRead = 95%). Os ganhos reais são wall clock + context headroom + tool-use turns, NÃO billed tokens.
- **Template mínimo em retry omite guards:** findings da review já trazem as regras violadas verbatim ("L7 violation: lazy DI"), então re-Read de CLAUDE.md/guards é redundante nesse caminho específico.
- **Prior memory é a ponte:** `.agent-memory/_index.json` já é escrito pós-wave (`memory-write.js`). Aproveitar sem introduzir novo mecanismo.
