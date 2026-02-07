# Bugfix Context

Issues comuns, dicas de debug - carregado pelo Bugfix Specialist

## Como usar

Crie arquivos `.md` aqui com informações específicas para o agente **bugfix**.

## Carregamento

Quando o agente bugfix é chamado:
1. Arquivos de `shared/` são carregados primeiro
2. Arquivos desta pasta são carregados depois
3. Entidades criadas: `AgentContext:bugfix:{filename}`

## Regras

- Apenas arquivos `.md`
- Máximo 500 linhas por arquivo
- Evite duplicar conteúdo que já está em `shared/`
