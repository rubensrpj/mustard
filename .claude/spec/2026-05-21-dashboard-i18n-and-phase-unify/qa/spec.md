# QA — dashboard-i18n-and-phase-unify

### Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
### Stage: QaReview
### Outcome: Active
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T18:00:00Z

## Tarefas

- [ ] `mustard-rt run qa-run --spec 2026-05-21-dashboard-i18n-and-phase-unify`

## Acceptance Criteria

- [ ] AC-QA-1: overall=pass — Command: `bash -c 'cargo run -q -p mustard-rt -- run qa-run --spec 2026-05-21-dashboard-i18n-and-phase-unify | node -e "const j=JSON.parse(require(\"fs\").readFileSync(0,\"utf8\"));process.exit(j.overall===\"pass\"?0:1)"'`

## Network

- Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
- Depende: [[review]]
