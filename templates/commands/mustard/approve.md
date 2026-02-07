# /approve - Approve Spec

## Trigger

`/approve`

## Description

Aprova a spec ativa e inicia a fase de implementação via pipeline do orchestrator.

## Prerequisites

- Spec ativa em `spec/active/`
- Spec apresentada ao user e aguardando aprovação

## Action

1. **Phase 0: AUTO-SYNC (OBRIGATÓRIO)** — EXECUTAR via Bash tool ANTES de qualquer outra ação:
   - Usar a Bash tool para executar: `node .claude/scripts/sync-registry.js && node .claude/scripts/sync-compile.js`
   - NÃO prosseguir para o passo 2 sem ter executado este comando
2. Ler `context/orchestrator.context.md`
3. Localizar spec ativa em `spec/active/`
4. Marcar spec como aprovada
5. **Continuar pipeline automaticamente:**
   - Feature pipeline → Phase 3: IMPLEMENT (delegar via Task tool)
   - Bugfix pipeline → Phase 4: FIX (delegar via Task tool)
6. Seguir regras de paralelização do orchestrator
7. **CRITICAL**: NUNCA implementar código diretamente — SEMPRE delegar via Task tool

## Alternative Flow

Se a spec não for satisfatória:
- Dar feedback textual para ajustes
- Usar /complete para cancelar
