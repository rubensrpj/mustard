# Tactical-fix — mirror wave-4 flatten into installed SKILLs

### Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
### Stage: Execute
### Outcome: Active
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T00:00:00Z

## Resumo

Wave-4 do parent só tocou `apps/cli/templates/` (a fonte canônica do payload). As cópias instaladas no próprio repo Mustard (`.claude/commands/mustard/*/SKILL.md` + `.claude/skills/pipeline-execution/SKILL.md`) continuam citando `spec/active/`, `spec/completed/`, `spec/superseded/` — quebrando o AC-W6-1 do parent (37 hits em 7 SKILLs + 2 hits em pipeline-execution). Esta tactical-fix replica as substituições wave-4 nas cópias instaladas.

## Arquivos

```
.claude/commands/mustard/approve/SKILL.md
.claude/commands/mustard/bugfix/SKILL.md
.claude/commands/mustard/close/SKILL.md
.claude/commands/mustard/feature/SKILL.md
.claude/commands/mustard/qa/SKILL.md
.claude/commands/mustard/resume/SKILL.md
.claude/commands/mustard/tactical-fix/SKILL.md
.claude/skills/pipeline-execution/SKILL.md
```

## Tarefas

- [x] Para cada arquivo acima, substituir `spec/active/{name}/` → `spec/{name}/`; `spec/completed/{name}/` → `spec/{name}/`; `spec/superseded/{name}/` → `spec/{name}/`.
- [x] Ajustar redação de passos que dizem "move to completed/" — substituir por "the header is updated via emit-pipeline".
- [x] Confirmar paridade textual com `apps/cli/templates/commands/mustard/*/SKILL.md` (mesmas substituições já aprovadas em wave-4).

## Acceptance Criteria

- [x] AC-TF-A-1: `rg -n 'spec/(active|completed|superseded)' .claude/commands .claude/skills/pipeline-execution` retorna vazio.
- [x] AC-TF-A-2: Diff entre `.claude/commands/mustard/*/SKILL.md` e `apps/cli/templates/commands/mustard/*/SKILL.md` é zero em relação a paths de bucket.

## Limites

- `.claude/commands/mustard/{approve,bugfix,close,feature,qa,resume,tactical-fix}/SKILL.md`
- `.claude/skills/pipeline-execution/SKILL.md`

OUT: tudo fora dessa lista.
