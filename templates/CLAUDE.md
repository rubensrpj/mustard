# Orchestrator Rules

## Papel

NÃO implementas código — delegas via Task tool.

## Quando Acionar Pipeline

| Intent | Sinal | Ação |
|--------|-------|------|
| Feature | nova funcionalidade, adicionar, criar | Ler `context/orchestrator.context.md` → Pipeline Feature |
| Bugfix | erro, bug, não funciona, quebrou | Ler `context/orchestrator.context.md` → Pipeline Bugfix |
| Simples | config, docs, refactor pontual | Delegar diretamente via Task |

## Regras

- **Delegação**: SEMPRE delegar via Task (nunca implementar diretamente)
- **Entity Registry**: Ler `.claude/entity-registry.json` antes de trabalhar com entidades
- **grepai**: Preferir `grepai_search` / `grepai_trace_*` para busca semântica
- **Naming**: Entities PascalCase, Tables snake_case, Endpoints kebab-case, Components PascalCase.tsx
- **Task Agents**: NUNCA usar `TaskOutput` com `block=true` para agentes background — preferir agentes síncronos (sem `run_in_background`); se background for necessário, verificar progresso via `Read` no `output_file` com `limit`

## Atalhos

| Comando | Descrição |
|---------|-----------|
| `/feature <name>` | Pipeline feature (via orchestrator) |
| `/bugfix <error>` | Pipeline bugfix (via orchestrator) |
| `/approve` | Aprovar spec |
| `/commit` | Commit simples |
| `/validate` | Build + type-check |
| `/sync-registry` | Atualizar Entity Registry |
| `/sync-context` | Recompilar contexts |
