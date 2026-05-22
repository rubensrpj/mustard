# Review — dashboard-i18n-and-phase-unify

### Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
### Stage: QaReview
### Outcome: Active
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T18:00:00Z

## Resumo

Single review (dashboard only).

## Tarefas

- [ ] Verifica W1: PipelineTimeline aceita variant compact|default, MiniTimeline removido, execute=verde, subtitle redundante removido.
- [ ] Verifica W2: t() global, catálogo PT+EN com pelo menos as chaves listadas, sidebar/topbar/specs/knowledge usam t(), bootstrap lê Preferences.

## Acceptance Criteria

- [ ] AC-R-1: Build passa — Command: `pnpm --filter mustard-dashboard build`

## Limites

Sem código.

## Network

- Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
- Depende: [[wave-1-ui]], [[wave-2-ui]]
