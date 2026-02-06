# Orchestrator Context

Visão geral, fluxos de pipeline - carregado pelo Orchestrator

## Como usar

Crie arquivos `.md` aqui com informações específicas para o agente **orchestrator**.

## Carregamento

Quando o agente orchestrator é chamado:
1. Arquivos de `shared/` são carregados primeiro
2. Arquivos desta pasta são carregados depois
3. Entidades criadas: `AgentContext:orchestrator:{filename}`

## Regras

- Apenas arquivos `.md`
- Máximo 500 linhas por arquivo
- Evite duplicar conteúdo que já está em `shared/`
