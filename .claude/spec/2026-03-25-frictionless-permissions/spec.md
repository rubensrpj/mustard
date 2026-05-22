# Enhancement: frictionless-permissions
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-03-25T20:10:00Z

## Summary
Substituir permissões Bash granulares por `"Bash"` blanket no settings.json template, confiando na deny list + bash-safety.js hook como camadas de segurança. Elimina prompts de confirmação para comandos seguros.

## Checklist
### Templates Agent
- [x] Simplificar `permissions.allow` em `templates/settings.json` — substituir todos os `Bash(cmd:*)` por `"Bash"`
- [x] Aplicar mesma mudança em `.claude/settings.json` (settings do projeto Mustard)
- [x] Verificar que deny list e bash-safety.js cobrem cenários destrutivos

## Files (~2)
- `templates/settings.json` (modify)
- `.claude/settings.json` (modify)
