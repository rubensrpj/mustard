# Comando: /backend-stop

> Para o backend em execução

## Uso

```
/backend-stop
```

## Execução (1-2 CHAMADAS)

### Opção A: Se souber o task_id

```
TaskStop(task_id="[TASK_ID]")
```

### Opção B: Se não souber o task_id

```bash
cmd //c "taskkill /F /IM Competi.Backend.exe 2>nul || echo Nenhum processo"
```

## Formato de Resposta

```
**Backend encerrado**
```

**NÃO FAÇA:**
- NÃO verifique portas após encerrar
- NÃO faça múltiplas chamadas
- NÃO liste processos antes de encerrar
