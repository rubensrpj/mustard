# Backend Context

Padrões de API, serviços, repositórios - carregado pelo Backend Specialist

## Como usar

Crie arquivos `.md` aqui com informações específicas para o agente **backend**.

## Carregamento

Quando o agente backend é chamado:
1. Arquivos de `shared/` são carregados primeiro
2. Arquivos desta pasta são carregados depois
3. Entidades criadas: `AgentContext:backend:{filename}`

## Regras

- Apenas arquivos `.md`
- Máximo 500 linhas por arquivo
- Evite duplicar conteúdo que já está em `shared/`
