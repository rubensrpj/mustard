# Feature: verification-stats

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-18T23:17:29Z
### Lang: pt

## Contexto

O Mustard tem duas fases que verificam um pipeline antes do CLOSE: REVIEW — um agente audita o código e devolve APPROVED ou REJECTED — e QA, que executa os Acceptance Criteria da spec. O comando `/stats` deveria mostrar a saúde dessas verificações, mas hoje nenhuma das duas aparece. O `qa-run.js` só grava no log de harness (`events.jsonl`), nunca emite uma métrica de hook — e o `metrics.js`, que alimenta o `/stats`, lê apenas o log de métricas (`.claude/.metrics/*.jsonl`). A fase REVIEW não emite nada em lugar nenhum. O resultado é que o usuário não consegue responder "meus pipelines estão sendo verificados antes de fechar?" — a estatística simplesmente não existe. Além disso, o `/close` (finalize manual) pode fechar um pipeline sem nunca ter rodado QA, porque não checa o resultado de QA antes de finalizar.

## Resumo

Tornar QA e REVIEW observáveis em `/stats` sob um bucket único "Verificação": `qa-run.js` passa a emitir métrica de hook; um novo script `review-result.js` instrumenta a fase REVIEW; `metrics.js` ganha a categoria `verification` e um painel dedicado; `/close` passa a rodar QA antes de finalizar.

## Arquivos (~8)

- `templates/scripts/qa-run.js` (modificar — emitir métrica)
- `templates/scripts/review-result.js` (**novo** — instrumenta a fase REVIEW)
- `templates/scripts/metrics.js` (modificar — categoria + painel)
- `templates/commands/mustard/resume/SKILL.md` (modificar — wiring REVIEW)
- `templates/commands/mustard/feature/SKILL.md` (modificar — wiring REVIEW)
- `templates/commands/mustard/close/SKILL.md` (modificar — checagem QA)
- `templates/hooks/__tests__/verification-stats.test.js` (**novo** — cobertura)
- `templates/hooks/__tests__/harness-wave10.test.js` (modificar — regressão do emit de QA)

## Limites

- `templates/scripts/qa-run.js`, `templates/scripts/review-result.js`, `templates/scripts/metrics.js`
- `templates/commands/mustard/{resume,feature,close}/SKILL.md`
- `templates/hooks/__tests__/verification-stats.test.js`, `templates/hooks/__tests__/harness-wave10.test.js`

## Tarefas

### Templates Agent (Wave 1) — Instrumentação (produtores de evento)

- [x] `qa-run.js`: importar `metrics-emit.js` e emitir `emitMetric('qa', { note: overall, extras: { spec, overall, passCount, failCount, skipCount, category: 'verification' } })` em TODA saída de `runQA` (pass, fail e os caminhos skip — inclusive os early-return sem seção AC). Manter o `emit('qa.result')` existente intacto. Fail-silent.
- [x] Criar `review-result.js`: CLI `--spec <name> --verdict approved|rejected [--critical <N>] [--subproject <name>]`. Emite `emit('review.result', { spec, verdict, criticalCount, subproject }, ...)` no harness log E `emitMetric('review', { note: verdict, extras: { spec, verdict, criticalCount, category: 'verification' } })`. Exporta `module.exports = { recordReview }` (síncrono, espelha a estrutura de `qa-run.js`). Cabeçalho `#!/usr/bin/env bun` + `'use strict'`.

### Templates Agent (Wave 2) — Agregação, display e wiring

- [x] `metrics.js`: adicionar `'qa': 'verification'` e `'review': 'verification'` ao `EVENT_CATEGORY`; estender `aggregateHookEvents` para acumular `notes: { [note]: count }` por evento (genérico — todo evento já carrega `note`); adicionar painel `## Verification (QA + Review)` antes da tabela `## All Hook Events (raw)`, mostrando QA (pass/fail/skip) e Review (approved/rejected) a partir de `byEvent['qa'].notes` / `byEvent['review'].notes`, com taxa de aprovação quando houver dados.
- [x] `resume/SKILL.md` Step 19 (REVIEW): após consolidar o veredito, chamar `bun .claude/scripts/review-result.js --spec {specName} --verdict {approved|rejected} --critical {N}` (uma vez por subprojeto revisado, ou agregado — o que o fluxo já produzir).
- [x] `feature/SKILL.md` passo 9 (REVIEW do EXECUTE Light): mesma chamada de `review-result.js`.
- [x] `close/SKILL.md`: no Verification Gate, rodar `bun .claude/scripts/qa-run.js --spec {spec-name} --json`; `overall=fail` → não finaliza, reporta os AC falhos; `overall=skip` → warn e segue; `overall=pass` → segue.
- [x] Testes: criar `verification-stats.test.js` cobrindo (a) `qa-run` emite métrica `qa` com `note=overall`; (b) `review-result.js` emite `review` para approved e rejected; (c) `metrics-collect` renderiza o painel `Verification (QA + Review)` quando há eventos de verificação. Atualizar `harness-wave10.test.js` se o novo emit alterar contagens. build/type-check final.

## Dependências

Wave 2 depende de Wave 1: a categoria `verification` e o painel consomem o shape dos eventos `qa`/`review` emitidos na Wave 1.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: a suíte de verificação passa — Command: `bun test templates/hooks/__tests__/verification-stats.test.js`
- [x] AC-2: a suíte de QA da Wave 10 continua verde após o novo emit — Command: `bun test templates/hooks/__tests__/harness-wave10.test.js`
- [x] AC-3: `metrics.js` define a categoria `verification` e o painel — Command: `node -e "const s=require('fs').readFileSync('templates/scripts/metrics.js','utf8');process.exit(/'verification'/.test(s)&&/Verification \(QA \+ Review\)/.test(s)?0:1)"`
- [x] AC-4: `close/SKILL.md` roda `qa-run.js` antes de finalizar — Command: `node -e "const s=require('fs').readFileSync('templates/commands/mustard/close/SKILL.md','utf8');process.exit(/qa-run\.js/.test(s)?0:1)"`
- [x] AC-5: a suíte de regressão dos hooks continua verde — Command: `bun test templates/hooks/__tests__/hooks.test.js`
