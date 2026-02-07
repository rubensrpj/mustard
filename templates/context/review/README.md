# Review Context

Checklists, regras de qualidade - carregado pelo Review Specialist

## Como usar

Crie arquivos `.md` aqui com informações específicas para o agente **review**.

## Carregamento

Quando o agente review é chamado:
1. Arquivos de `shared/` são carregados primeiro
2. Arquivos desta pasta são carregados depois
3. Entidades criadas: `AgentContext:review:{filename}`

## Regras

- Apenas arquivos `.md`
- Máximo 500 linhas por arquivo
- Evite duplicar conteúdo que já está em `shared/`
