# Enhancement: scan-no-confirmation-prompts
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-04-17T00:00:00Z

## Summary
Durante `/scan`, os Task agents geradores pedem confirmações ao sobrescrever/criar arquivos, travando o fluxo. A regra "NO confirmation prompts" existe em `scan/SKILL.md` (linha 14) mas NÃO é propagada ao prompt dos Task agents nem ao `scan-format/SKILL.md` que eles leem. Objetivo: reforçar a regra em ambos para que a geração seja totalmente automática.

## Boundaries
- `templates/commands/mustard/scan/SKILL.md` — adicionar regra no prompt do Task agent (step 3)
- `templates/commands/mustard/scan-format/SKILL.md` — adicionar "Execution Rules" no topo com no-confirmation
- `.claude/commands/mustard/scan/SKILL.md` — espelhar
- `.claude/commands/mustard/scan-format/SKILL.md` — espelhar

## Checklist
### General Agent
- [x] Editar `templates/commands/mustard/scan-format/SKILL.md`: adicionar seção `## Execution Rules` no topo (após Language Rule), com regra explícita "NEVER ask for confirmation — overwrite, delete, and create files autonomously. The orchestrator already decided to scan."
- [x] Editar `templates/commands/mustard/scan/SKILL.md`: no prompt do Task agent (seção "### 3. Launch Agents", dentro do bloco ``` ), adicionar linha inicial: `**Execution rule**: NEVER ask for confirmation on file writes, deletes, or overwrites. Proceed autonomously — the user already invoked /scan.`
- [x] Espelhar as mesmas mudanças em `.claude/commands/mustard/scan/SKILL.md` e `.claude/commands/mustard/scan-format/SKILL.md`
- [x] Verificar consistência: edições são apenas em `.md` (não há testes JS cobrindo o conteúdo textual das skills)

## Files (~4)
- `templates/commands/mustard/scan/SKILL.md` (modify)
- `templates/commands/mustard/scan-format/SKILL.md` (modify)
- `.claude/commands/mustard/scan/SKILL.md` (modify)
- `.claude/commands/mustard/scan-format/SKILL.md` (modify)
