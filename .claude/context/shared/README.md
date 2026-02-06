# Shared Context

Contexto comum carregado por TODOS os agentes

## Como usar

Crie arquivos `.md` aqui com informações específicas para o agente **shared**.

## Carregamento

Quando o agente shared é chamado:
1. Arquivos de `shared/` são carregados primeiro
2. Arquivos desta pasta são carregados depois
3. Entidades criadas: `AgentContext:shared:{filename}`

## Regras

- Apenas arquivos `.md`
- Máximo 500 linhas por arquivo
- Evite duplicar conteúdo que já está em `shared/`
