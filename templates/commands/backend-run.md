# Comando: /backend-run

> Inicia o backend em modo debug com monitoramento de logs

## Uso

```
/backend-run
```

## Execução

### Passo 1: Cleanup e Iniciar (ÚNICO COMANDO)

```bash
cmd //c "taskkill /F /IM Competi.Backend.exe 2>nul || echo OK"
```

### Passo 2: Iniciar Backend em Background

```bash
cd /c/Atiz/Competi/projetos/CRM/Competi.Backend/Competi.Backend && dotnet run --launch-profile Local
```

**IMPORTANTE:** Usar `run_in_background: true`.

### Passo 3: Resposta Imediata

Responder imediatamente com o task_id, SEM fazer polling para verificar inicialização.

```
**Backend iniciando** (Task: [task_id])
- HTTPS: https://localhost:9091
- HTTP: http://localhost:9090

Use `/backend-logs` para verificar status.
```

**NÃO FAÇA:**
- NÃO use TaskOutput para verificar inicialização
- NÃO faça polling esperando "Application started"
- NÃO atualize o arquivo .bat automaticamente

## Atualizar Monitor (OPCIONAL)

Se o usuário pedir para atualizar o monitor externo, editar `backend-logs-monitor.bat`:

```bat
@echo off
title Backend Logs Monitor - Competi CRM
powershell -Command "Get-Content 'C:\Users\ruben\AppData\Local\Temp\claude\C--Atiz-Competi-projetos-CRM\tasks\[TASK_ID].output' -Wait -Tail 100"
```

## Endpoints

| URL | Descrição |
|-----|-----------|
| https://localhost:9091 | API HTTPS |
| https://localhost:9091/swagger | Swagger UI |

## Comandos Relacionados

- `/backend-stop` - Encerrar backend
- `/backend-logs` - Ver logs
