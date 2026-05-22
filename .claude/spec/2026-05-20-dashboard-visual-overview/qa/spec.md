# QA Plan — Visão Geral redesenhada

### Parent: [[2026-05-20-dashboard-visual-overview]]
### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: qa
### Checkpoint: 2026-05-20T22:55:00Z
### Lang: pt

## PRD

## Contexto

Plano de QA declarado upfront. Executado por `mustard-rt run qa-run --spec 2026-05-20-dashboard-visual-overview --include-children` após [[review]] aprovar. Relatório em `qa/report.md`.

## Consolidação dos AC

- AC-G1..G4 do parent (wave-plan.md)
- AC-1..3 de [[wave-1-backend]]
- AC-1..3 de [[wave-1-badges]]
- AC-1..3 de [[wave-2-data]]
- AC-1..5 de [[wave-3-ui]]
- AC-1..5 de [[wave-4-integration]]
- AC-1..2 de [[review]]

Total: ~25 ACs.

## Política de re-tentativa

- 3 iterações máx por AC falhado
- Após 3 falhas: AskUserQuestion (fix manual / relaxar AC / abortar)

## Comando runner

```bash
mustard-rt run qa-run --spec 2026-05-20-dashboard-visual-overview --include-children
```

## Acceptance Criteria

- [ ] AC-1: `qa/report.md` existe — Command: `bash -c 'test -f "$(find .claude/spec -path "*2026-05-20-dashboard-visual-overview/qa/report.md" | head -1)"'`
- [ ] AC-2: Report inclui overall PASS ou FAIL — Command: `bash -c 'f=$(find .claude/spec -path "*2026-05-20-dashboard-visual-overview/qa/report.md" | head -1); grep -qE "Overall:.*(PASS|FAIL)" "$f"'`

## Network

- Parent: [[2026-05-20-dashboard-visual-overview]]
- Roda depois de: [[review]]
- Desbloqueia: CLOSE → spec movida pra `completed/`
