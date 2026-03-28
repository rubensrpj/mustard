# Fix: Scan pedindo confirmação para gerar arquivos

## Context

O `/scan` continua pedindo confirmação ao usuário para gerar arquivos no root e dentro de git submodules, apesar da regra "NO confirmation prompts". Isso quebra o fluxo autônomo do scan.

## Root Causes

1. **Conflito de identidade do orchestrator**: `.claude/CLAUDE.md` diz "You do NOT implement code — you delegate via Task tool", mas Bootstrap (§2.6) precisa que o orchestrator escreva arquivos diretamente
2. **scan-format/SKILL.md sem regra de "no confirmation"**: Task agents nunca recebem essa instrução
3. **Prompt template do agent launch (scan/SKILL.md:181-197)** não passa "no confirmation" aos agents
4. **Sem autorização explícita para submodules**: Claude hesita em escrever dentro de git submodules
5. **Globs de permissão incompletos** em `templates/settings.json` para `CLAUDE.md` no root e subprojects

## Approach

### 1. `scan-format/SKILL.md` — Adicionar seção "Execution Rules"

Inserir após linha 6, antes de `## Language Rule`:

```markdown
## Execution Rules

- **NO confirmation prompts**: Write all files directly. Never ask the user (or orchestrator) for approval before writing, creating directories, or overwriting generated files.
- **Submodule paths are authorized**: If the subproject path is inside a git submodule, write files there without hesitation. The orchestrator has granted full write authorization for all detected subproject paths.
- **Generated files are always overwritable**: Any file starting with `<!-- mustard:generated` can be overwritten without confirmation.
```

### 2. `scan/SKILL.md` — Três alterações

**A) Reforçar regra no-confirmation (linha 14)**:
```
- **NO confirmation prompts**: never ask the user for approval — not for writing files, not for creating directories, not for writing inside git submodules. Just do it.
- **Bootstrap authorization**: The orchestrator DIRECTLY writes root-level files during Bootstrap (§2.6). This is the ONE exception to "delegate via Task." Bootstrap files (`CLAUDE.md`, `.claude/CLAUDE.md`, `.claude/entity-registry.json`, `{subproject}/CLAUDE.md`) are scaffolding, not implementation code.
```

**B) Adicionar autorização de submodule após Bootstrap (após linha 172)**:
```
**Submodule handling**: If a subproject path is a git submodule, write files inside it without hesitation. The /scan command has FULL authorization to create and overwrite files in any detected subproject path, whether it is a regular directory or a git submodule.
```

**C) Atualizar prompt template dos agents (linhas 181-197)** — adicionar como primeira linha:
```
MANDATORY: Do NOT ask for confirmation. Write all files directly — no prompts, no questions. If the subproject path is a git submodule, write inside it without hesitation. You have full authorization.
```

### 3. `templates/settings.json` — Adicionar globs explícitos

No array `permissions.allow`:
```json
"Write(CLAUDE.md)",
"Write(*/CLAUDE.md)",
"Edit(CLAUDE.md)",
"Edit(*/CLAUDE.md)"
```

## Files to modify

- `.claude/commands/mustard/scan/SKILL.md`
- `.claude/commands/mustard/scan-format/SKILL.md`
- `templates/settings.json`

## Verification

Rodar `/scan` em projeto com git submodules. Esperado:
- Bootstrap escreve arquivos no root sem perguntar
- Task agents escrevem dentro de submodules sem perguntar
- Zero prompts de "Should I...?" durante todo o scan
