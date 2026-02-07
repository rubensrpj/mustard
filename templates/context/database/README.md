# Database Context

Schemas, migrações, queries - carregado pelo Database Specialist

## Como usar

Crie arquivos `.md` aqui com informações específicas para o agente **database**.

## Carregamento

Quando o agente database é chamado:
1. Arquivos de `shared/` são carregados primeiro
2. Arquivos desta pasta são carregados depois
3. Entidades criadas: `AgentContext:database:{filename}`

## Regras

- Apenas arquivos `.md`
- Máximo 500 linhas por arquivo
- Evite duplicar conteúdo que já está em `shared/`
