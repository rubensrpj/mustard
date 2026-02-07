# Comando: /backend-restart

> Reinicia o backend (stop + start)

## Uso

```
/backend-restart
```

## Execução (2 CHAMADAS)

### Passo 1: Parar

```bash
cmd //c "taskkill /F /IM Competi.Backend.exe 2>nul || echo OK"
```

### Passo 2: Iniciar em Background

```bash
cd /c/Atiz/Competi/projetos/CRM/Competi.Backend/Competi.Backend && dotnet run --launch-profile Local
```

**IMPORTANTE:** Usar `run_in_background: true`.

## Formato de Resposta

```
**Backend reiniciado** (Task: [task_id])
- HTTPS: https://localhost:9091
- HTTP: http://localhost:9090

Use `/backend-logs` para verificar.
```

**NÃO FAÇA:**
- NÃO verifique portas
- NÃO faça polling para verificar inicialização
- NÃO atualize o arquivo .bat automaticamente
