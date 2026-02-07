# Context Files

Esta pasta contém **arquivos de contexto** organizados por agente.

## Estrutura

```
context/
├── shared/       # Contexto comum (TODOS os agentes)
├── backend/      # Só o Backend Specialist vê
├── frontend/     # Só o Frontend Specialist vê
├── database/     # Só o Database Specialist vê
├── bugfix/       # Só o Bugfix Specialist vê
├── review/       # Só o Review Specialist vê
└── orchestrator/ # Só o Orchestrator vê
```

## Como Funciona

1. Quando um agente é chamado (ex: backend.md)
2. Ele carrega `shared/*.md` + `backend/*.md`
3. Cria entidades no Memory MCP: `AgentContext:backend:{filename}`
4. Depois faz `mcp__memory__search_nodes` normalmente

## Regras

- Apenas arquivos `.md`
- Máximo 500 linhas por arquivo
- Máximo 20 arquivos por pasta
- Use `shared/` para contexto comum
- Use pastas específicas para contexto do agente
