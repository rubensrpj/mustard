# QA Plan — Wave network como padrão Mustard

### Parent: [[2026-05-20-mustard-wave-network-standard]]
### Status: queued
### Phase: QA (plano)
### Scope: qa
### Checkpoint: 2026-05-20T22:55:00Z
### Lang: pt

## PRD

## Contexto

Plano de QA **declarado upfront** (SDD). Executado pelo `mustard-rt run qa-run --spec` após [[review]] aprovar. Relatório final em `qa/report.md` no mesmo dir.

## Consolidação dos AC

QA roda TODOS os ACs declarados:

- AC-G1..G7 do [[2026-05-20-mustard-wave-network-standard]] (wave-plan.md)
- AC-1..6 de [[wave-1-rt-infra]]
- AC-1..4 de [[wave-2-skill-template]]
- AC-1..5 de [[wave-3-dashboard-graph]]
- AC-1..5 de [[wave-4-metrics-diagnose-fix]]
- AC-1..2 de [[review]]

Total esperado: ~30 ACs. Cada um binário (exit 0 = pass).

## Política de re-tentativa

- 3 iterações máx por AC falhado (re-dispatch do impl agent da wave correspondente com o AC específico no prompt)
- Após 3 falhas: AskUserQuestion "QA falhou 3×. Opções: (a) fix manual + retry, (b) relaxar AC, (c) abortar pipeline"

## Comando runner

```bash
mustard-rt run qa-run --spec 2026-05-20-mustard-wave-network-standard --include-children
```

Flag `--include-children` (a entregar nesta spec) faz o runner descer em wave-1..N + review e agregar ACs antes de rodar.

## Acceptance Criteria

- [ ] AC-1: `qa/report.md` existe após qa-run — Command: `bash -c 'test -f "$(find .claude/spec -path "*2026-05-20-mustard-wave-network-standard/qa/report.md" | head -1)"'`
- [ ] AC-2: Report inclui contagem total + passados + falhados — Command: `bash -c 'f=$(find .claude/spec -path "*2026-05-20-mustard-wave-network-standard/qa/report.md" | head -1); grep -qE "total.*[0-9]+" "$f" && grep -qE "(pass|passed).*[0-9]+" "$f" && grep -qE "(fail|failed).*[0-9]+" "$f"'`

## Saída esperada

`qa/report.md`:

```
# QA Report — Wave network como padrão Mustard
Date: <ISO>
Total: 30 | Passed: 30 | Failed: 0
Overall: PASS

## Resultados
### [[wave-1-rt-infra]] (6/6)
- [x] AC-1: ...
- [x] AC-2: ...
(...)

### [[wave-4-metrics-diagnose-fix]] (5/5)
(...)
```

## Network

- Parent: [[2026-05-20-mustard-wave-network-standard]]
- Roda depois de: [[review]]
- Desbloqueia: CLOSE → spec movida para `completed/`
