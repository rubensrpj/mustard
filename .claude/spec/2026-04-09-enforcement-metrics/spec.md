# Enhancement: enforcement-metrics

## Summary
Criar framework de métricas append-only em JSONL para visibilidade das ações dos enforcement hooks e pipeline gates. Inclui:
1. `.claude/scripts/_metrics-write.js` — helper que hooks CAN usar (opt-in) para append de eventos
2. `.claude/scripts/metrics-report.js` — reporter que agrega e imprime stats
3. `.claude/.metrics/` directory gitignorado (runtime state)

**Escopo crítico**: este spec cria APENAS o framework. Instrumentação efetiva dos hooks existentes é deixada para specs futuros. Zero mudança em hooks atuais.

## Why
R5 — atualmente sem visibilidade sobre hit rates dos gates/hooks. Não dá pra validar se enforcement é net-positive. Framework lightweight de JSONL + reporter dá dados para decisões futuras.

## Boundaries
- `templates/scripts/_metrics-write.js` (create, helper)
- `templates/scripts/metrics-report.js` (create, reporter)
- `.claude/scripts/_metrics-write.js` (mirror)
- `.claude/scripts/metrics-report.js` (mirror)
- `.claude/.metrics/` (create directory on-demand, não trackeado)

## Checklist
- [x] Criar `templates/scripts/_metrics-write.js`:
  - Exporta `append(event)` — recebe objeto, adiciona `ts: new Date().toISOString()`, escreve linha JSONL em `.claude/.metrics/enforcement.jsonl`
  - Rotate se arquivo > 10MB: renomeia para `.enforcement.jsonl.1` e começa novo
  - Built-ins only (fs, path)
  - Fail-silently se write falhar (não bloquear hooks)
- [x] Criar `templates/scripts/metrics-report.js`:
  - CLI: `node metrics-report.js [--since <ISO date>] [--event <type>]`
  - Lê todos `.claude/.metrics/*.jsonl`, parseia, agrupa por `event` type
  - Output: markdown table com counts, tokens_affected totals, hit rate por hook
  - Flags: `--since` filtra por data, `--event` filtra por tipo
- [x] Smoke test: escrever 3-5 events manualmente via script, rodar reporter, confirmar output
- [x] Mirror para `.claude/scripts/`
- [x] Adicionar `.claude/.metrics/` ao `.gitignore` do projeto (se não tiver)
- [x] Build + hook tests 26/26

## Files (~4-5)
- `templates/scripts/_metrics-write.js` (create)
- `templates/scripts/metrics-report.js` (create)
- `.claude/scripts/_metrics-write.js` (mirror)
- `.claude/scripts/metrics-report.js` (mirror)
- `.gitignore` (modify if needed)

## Acceptance
- Helper `append(event)` funciona, rotate em 10MB
- Reporter lê, agrega, imprime summary
- `.claude/.metrics/.enforcement.jsonl` ignorado pelo git
- Smoke test passou
- Build + tests 26/26

## Guards
- ZERO instrumentação de hooks existentes neste spec
- Helper deve ser SILENT se falhar (nunca bloquear caller)
- Built-ins only
- Rotação simples: 1 arquivo `.1`, não cadeia ilimitada

## Result
Implemented 2026-04-09. Framework created with zero hook instrumentation.
- `templates/scripts/_metrics-write.js` (23 lines) — fail-silent append helper, 10MB rotation
- `templates/scripts/metrics-report.js` (64 lines) — JSONL aggregator, markdown table output
- Mirrored to `.claude/scripts/`
- `.gitignore` updated: added `.claude/.metrics/`
- Smoke test passed: append + read back confirmed
- `npm run build`: PASS | `bun test`: 26/26
