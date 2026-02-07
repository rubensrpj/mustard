# /resume - Resume Pipeline

## Trigger

`/resume`

## Description

Resumes a pipeline that was interrupted. Delegates to `/approve` for full context loading.

## Action

1. Localizar spec(s) ativa(s) em `spec/active/`
2. Se não existir spec ativa → informar user e parar
3. Ler a spec e identificar última fase completada
4. **OBRIGATÓRIO: Apresentar resumo ao utilizador e PERGUNTAR antes de continuar**
   - Mostrar: nome da spec, última fase, próxima fase
   - Perguntar explicitamente: "Queres que eu continue com [próxima fase]?"
   - NUNCA avançar sem confirmação explícita do utilizador
5. **Quando user confirmar → executar `/approve`** (via Skill tool)
   - O `/approve` faz auto-sync, lê orchestrator context, e delega implementação
   - NÃO duplicar a lógica do approve — simplesmente invocar o skill

## When to Use

- After restarting Claude session
- After accidental interruption
- To continue work from another session
