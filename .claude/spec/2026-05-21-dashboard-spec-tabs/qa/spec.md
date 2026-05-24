# QA — dashboard-spec-tabs

## Resumo

Executar os 8 Acceptance Criteria do `wave-plan.md` (AC-1..AC-8) mais os ACs por wave. `mustard-rt run qa-run --spec 2026-05-21-dashboard-spec-tabs` itera todos os blocos `## Acceptance Criteria` da árvore de specs e roda cada `Command:`. Falha → retorna para implementação; passa → libera CLOSE.

## Tarefas

- [ ] `mustard-rt run qa-run --spec 2026-05-21-dashboard-spec-tabs`
- [ ] Se overall=fail: listar ACs falhos, retornar pra wave correspondente.
- [ ] Se overall=pass: marcar `[x]` em todos os blocks, prosseguir pra CLOSE.

## Acceptance Criteria

Os ACs deste bloco são meta — espelham o resultado dos ACs reais distribuídos:

- [ ] AC-QA-1: `qa-run` retorna `overall=pass` — Command: `bash -c 'cargo run -q -p mustard-rt -- run qa-run --spec 2026-05-21-dashboard-spec-tabs | node -e "const j=JSON.parse(require(\"fs\").readFileSync(0,\"utf8\"));process.exit(j.overall===\"pass\"?0:1)"'`

## Limites

Sem mudança de código.

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs]]
- Depende: [[review]]
