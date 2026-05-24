# Enhancement: metrics-instrumentation

## Summary

O JSONL de enforcement metrics (`.claude/.metrics/budget-observations.jsonl`) só recebe eventos de `context-budget.js` e sem os campos `tokens_saved`/`tokens_affected` que o `metrics-report.js` agrega. Resultado: `rtk node scripts/metrics-report.js` mostra `| budget-check | 10 | - | - | - |` — contagem sim, economia não. Este enhancement popula os campos faltantes e estende a emissão para dois outros hooks onde o cálculo é barato (`spec-hygiene.js`, `rtk-rewrite.js`), via um helper compartilhado em `_lib/`.

## Boundaries

- `templates/hooks/_lib/metrics-emit.js` — novo helper
- `templates/hooks/context-budget.js` — modificar: popular `tokens_saved`/`tokens_affected`
- `templates/hooks/spec-hygiene.js` — modificar: emitir evento ao mover spec
- `templates/hooks/rtk-rewrite.js` — modificar: emitir evento ao reescrever bash
- `templates/hooks/__tests__/hooks.test.js` — modificar: cobertura do helper + 1 hook
- `templates/scripts/metrics-report.js` — modificar (condicional): ajustar formatação se necessário

## Checklist

### general-purpose Agent

- [x] Criar `templates/hooks/_lib/metrics-emit.js` exportando `emitMetric(event, {tokensAffected, tokensSaved, note, extras})` — append JSONL em `.claude/.metrics/{event}.jsonl`, fail-silent (try/catch), cria dir com `{recursive: true}`. Schema: `{ts, event, tokens_affected, tokens_saved, note, ...extras}`.
- [x] Modificar `context-budget.js` linhas 97-108: substituir `fs.appendFileSync` direto por `emitMetric('budget-check', ...)`. Calcular `tokens_affected = Math.round(actual / 4)`, `tokens_saved = would_block ? Math.max(0, Math.round((actual - limit) / 4)) : 0`, `note = would_block ? 'blocked' : 'passed'`. Preservar campos existentes (`role`, `actual_chars`, `limit`, `would_block`, `mode`) via `extras`.
- [x] Modificar `spec-hygiene.js`: após cada `fs.renameSync` ou movimentação de spec, emitir `emitMetric('spec-hygiene-move', {tokensAffected: <file size in bytes / 4>, tokensSaved: <same>, note: 'stale spec removed from active/', extras: {from, to}})`. Economia = tamanho do arquivo/4 (heurística: o spec não seria re-lido em sessões futuras).
- [x] Modificar `rtk-rewrite.js`: quando detecta e reescreve um Bash command, emitir `emitMetric('rtk-rewrite', {tokensAffected: <command length / 4>, tokensSaved: 0, note: 'rewritten via rtk', extras: {command_head: <first 60 chars>}})`. `tokens_saved=0` é intencional: a economia real fica no `rtk gain` (medida pelo próprio RTK); aqui contamos só invocações para correlacionar.
- [x] Atualizar `metrics-report.js` linha 69: quando `tokensSaved === 0` E `tokensAffected > 0`, mostrar `tokensAffected` em vez de `-` na coluna "Tokens Affected". Adicionar linha de total de `tokens_saved` somado no rodapé.
- [x] Adicionar testes em `__tests__/hooks.test.js`:
  - helper `metrics-emit.js`: emite JSONL válido, cria dir, fail-silent em erro de write
  - `context-budget.js` com MODE=strict e prompt oversize: verifica que JSONL contém `tokens_saved > 0` e `note: 'blocked'`
  - `spec-hygiene.js`: verifica que move de spec emite evento com `tokens_saved > 0`
- [x] Build + test: `rtk npm run build && rtk bun test templates/hooks/__tests__/hooks.test.js`
- [x] Validação manual: rodar `rtk node .claude/scripts/metrics-report.js` e confirmar que a tabela mostra valores reais em `tokens_saved` e `tokens_affected`

## Files (~5)

- `templates/hooks/_lib/metrics-emit.js` (create)
- `templates/hooks/context-budget.js` (modify)
- `templates/hooks/spec-hygiene.js` (modify)
- `templates/hooks/rtk-rewrite.js` (modify)
- `templates/hooks/__tests__/hooks.test.js` (modify)
- `templates/scripts/metrics-report.js` (modify)

## Risks / Notes

- **RTK economia não é medida pelo hook**: `rtk-rewrite.js` conta invocações mas `tokens_saved` fica 0 — a economia real vem de `rtk gain` (fonte separada). Isso é intencional: não queremos duplicar a contabilidade do RTK. O hook serve pra correlacionar "quantos comandos foram rewriten nesta sessão" com o gain total.
- **Heurística de spec-hygiene**: `bytes / 4` é aproximação grosseira. Melhor do que 0, pior do que medir contexto real. Aceitável pra sinal direcional.
- **Schema compatibility**: `metrics-report.js` já lê `tokens_affected` e `tokens_saved` — não precisa mudar o leitor, só garantir que emissores populem. Mudança em `metrics-report.js` é só cosmética (mostrar valor em vez de `-`).
- **Arquivo JSONL por evento vs único**: helper escolhe `{event}.jsonl` pra permitir análise por tipo. `metrics-report.js` já itera todos os `.jsonl` no dir, então compatível.
