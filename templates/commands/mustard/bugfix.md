# /bugfix - Bug Fix Pipeline

## Trigger

`/bugfix <error-description>`

## Description

Inicia pipeline para diagnosticar e corrigir um bug.

## Procedimento

1. **Phase 0: AUTO-SYNC (OBRIGATÓRIO)** — EXECUTAR via Bash tool ANTES de qualquer outra ação:
   - Usar a Bash tool para executar: `node .claude/scripts/sync-registry.js && node .claude/scripts/sync-compile.js`
   - NÃO prosseguir para o passo 2 sem ter executado este comando
2. Ler `context/orchestrator.context.md`
3. Seguir **Pipeline Bugfix** (Phase 1: DIAGNOSE → Phase 6: COMPLETE)
