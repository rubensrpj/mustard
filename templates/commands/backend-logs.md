# Comando: /backend-logs

> Verifica os logs do backend em execução

## Uso

```
/backend-logs
```

## Execução (ÚNICA CHAMADA)

Usar `TaskOutput` com o task_id do backend rodando:

```
TaskOutput(task_id="[TASK_ID]", block=false, timeout=5000)
```

**Como obter o task_id:**
- Se acabou de rodar `/backend-run`, usar o task_id informado
- Se não sabe, usar `/tasks` para listar tasks ativas

## Formato de Resposta

### Se encontrar erros [ERR]:
```
**Logs** (Task: [id])

**Erros:**
- [HORA] mensagem do erro

Use `/backend-restart` se necessário.
```

### Se sem erros:
```
**Logs** (Task: [id]) - Status: OK

[Últimas 5-10 linhas relevantes do log]
```

## Filtrar Output

Ao apresentar logs, mostrar apenas:
- Linhas com `[ERR]` ou `[WRN]`
- Linhas com `Exception`
- Últimas requisições HTTP (se houver)
- Ignorar linhas `[DBG]` e SQL queries

**NÃO FAÇA:**
- NÃO use Grep separado
- NÃO leia arquivo diretamente com Read
- NÃO faça múltiplas chamadas
