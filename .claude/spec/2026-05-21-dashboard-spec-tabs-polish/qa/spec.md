# QA — dashboard-spec-tabs-polish

### Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
### Stage: QaReview
### Outcome: Active
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T17:00:00Z

## Resumo

Executa todos os ACs do wave-plan + por wave.

## Tarefas

- [ ] `mustard-rt run qa-run --spec 2026-05-21-dashboard-spec-tabs-polish`
- [ ] overall=pass → CLOSE
- [ ] overall=fail → re-dispatch wave correspondente

## Acceptance Criteria

- [ ] AC-QA-1: `qa-run` overall=pass — Command: `bash -c 'cargo run -q -p mustard-rt -- run qa-run --spec 2026-05-21-dashboard-spec-tabs-polish | node -e "const j=JSON.parse(require(\"fs\").readFileSync(0,\"utf8\"));process.exit(j.overall===\"pass\"?0:1)"'`

## Limites

Sem mudança.

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
- Depende: [[review]]
