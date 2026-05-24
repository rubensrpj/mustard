# Mustard 2.0 — Phase 2: OpenTelemetry + Token Measurement Real

- **Lang**: ptbr
- **Checkpoint**: 2026-05-12T20:25:00Z
- **Scope**: Full
- **Type**: feature
- **Model**: opus
- **Depends on**: Phase 1 (EventStore)
- **Unlocks**: Phase 3 (MCP)

## Design decision (revised at approve)

**Manual OTLP JSON emit, NO `@opentelemetry/sdk-node`.** Razões:
- Hook spawn-per-call mata `AsyncLocalStorage` cross-process. SDK não propaga contexto entre PreToolUse e PostToolUse spawns (são processos separados). `tool_use_id` do hookInput linka manualmente.
- Cold-start: SDK lite carrega ~20-50ms × 1000+ tool uses por sessão = 20-50s acumulado. Manual = sub-ms.
- Hooks devem ter ZERO npm deps. SDK exigiria bundling/wrapper (mesmo problema da EventStore em Sialia hoje).

**Único dep adicionado: `@opentelemetry/otlp-transformer` como devDependency** (apenas em tests) pra validar shape OTLP JSON do nosso emitter manual. Zero peso runtime.

Quando Claude Code expor `CLAUDE_LAST_USAGE` ou auto-instrumentation hooks, refactor de ~30 linhas troca o emitter manual pelo SDK. Não é problema de Phase 2.

## Summary

Adotar OpenTelemetry GenAI semantic conventions (stable Q1/2026) pra medição **real** de tokens por Task dispatch, fase e modelo. Substitui o vibe-metric atual ("119M tokens economizados" via keyword RTK) por sinais reais: bytes de prompt enviados ao Task, bytes de response retornado, duração por span.

## Problem

Métrica atual do dashboard:
- `tokensSaved` por hook **estimado** via heurística (`bytes / 4`)
- `retries` por **keyword match** (`retry|fix|error|failed|again`) — 80% false positives
- "119.8M tokens saved" é **só RTK filtrando Bash output**, não pipeline do Mustard
- Não há measurement de **tokens reais por fase/agent/model**

Resultado: você opera no escuro sobre custo. Decisão de delegar/não-delegar é gut feeling.

## Goal

Cada Task dispatch produz um span `gen_ai.client.operation` com:
- `gen_ai.system = "anthropic"`
- `gen_ai.request.model = "<model>"`
- `gen_ai.usage.input_tokens` (bytes do prompt / 4, ou exato se Anthropic SDK expuser)
- `gen_ai.usage.output_tokens` (bytes do tool_response / 4)
- `gen_ai.operation.duration` (ms entre PreToolUse e PostToolUse)
- `mustard.phase`, `mustard.wave`, `mustard.spec` (custom attributes)

Spans escritos em `.claude/.harness/spans.jsonl` (OTLP JSON format) — pluggable em qualquer backend OTel (Honeycomb/Datadog/Grafana Tempo).

Dashboard troca "tokens economizados" por:
- **Tokens por fase** (ANALYZE/PLAN/EXECUTE/QA/CLOSE)
- **Tokens por modelo** (opus/sonnet/haiku breakdown)
- **Tokens por agent type** (Explore/Plan/general-purpose)
- **Custo $ estimado** baseado em pricing público Anthropic

## Acceptance Criteria

1. **TokenTracker compila (manual emit, no SDK)**
   ```bash
   bunx tsc --noEmit -p src/telemetry/tsconfig.json
   ```
   Sem erros. Manual OTLP JSON emitter. `otlp-transformer` apenas em devDependencies (usado só nos tests).

2. **TokenTracker emite spans em events.jsonl + spans.jsonl**
   ```bash
   node tests/integration/token-tracker.test.js
   ```
   Simula 1 Task dispatch fake → spans.jsonl contém span com `gen_ai.*` attributes preenchidos.

3. **Conventions matchem o padrão**
   ```bash
   node -e "const fs=require('fs');const lines=fs.readFileSync('.claude/.harness/spans.jsonl','utf8').trim().split('\n');const s=JSON.parse(lines[0]);const required=['gen_ai.system','gen_ai.request.model','gen_ai.usage.input_tokens','gen_ai.usage.output_tokens'];process.exit(required.every(k=>s.attributes&&k in s.attributes)?0:1)"
   ```
   Span tem TODOS os atributos `gen_ai.*` requeridos.

4. **Custom attributes Mustard**
   ```bash
   node -e "const fs=require('fs');const s=JSON.parse(fs.readFileSync('.claude/.harness/spans.jsonl','utf8').trim().split('\n')[0]);process.exit(s.attributes['mustard.phase']&&s.attributes['mustard.spec']?0:1)"
   ```
   `mustard.phase`, `mustard.spec`, `mustard.wave` presentes.

5. **Dashboard exibe tokens reais**
   ```bash
   curl -sf http://127.0.0.1:7909/api/metrics | node -e "let d='';process.stdin.on('data',c=>d+=c);process.stdin.on('end',()=>{const j=JSON.parse(d);process.exit(j.tokenUsage&&j.tokenUsage.byPhase?0:1)})"
   ```
   `/api/metrics` retorna `tokenUsage: { byPhase, byModel, byAgent, totalInput, totalOutput }`.

6. **Span duration cresce com prompt size (sanity)**
   ```bash
   node tests/integration/span-duration-correlates.js
   ```
   Em 10 spans simulados com prompts de tamanhos crescentes, `duration` correlaciona positivamente com `input_tokens`.

7. **Substituição completa do `tokensSaved` heurístico**
   ```bash
   node -e "const fs=require('fs');for(const f of ['templates/scripts/dashboard.js','templates/scripts/dashboard-ui.js']){if(fs.readFileSync(f,'utf8').includes('tokensSaved')){console.log('FAIL',f);process.exit(1)}}process.exit(0)"
   ```
   Zero references a `tokensSaved`. UI mostra `tokensReal.input/output`.

8. **Spans queryable via EventStore**
   ```bash
   bun -e "const{EventStore}=require('./dist/runtime/event-store.js');const e=new EventStore('.claude/.harness/mustard.db');e.init();const r=e.spans({phase:'PLAN'});process.exit(r.length>=0?0:1)"
   ```
   `EventStore.spans(filter)` consulta tabela `spans` (nova projeção). Bun obrigatório por causa do driver SQLite nativo (Node lança "SQLite driver unavailable").

### Parseable AC (cross-shell, QA-runner)

Comandos abaixo são `cmd.exe`/PowerShell/bash-friendly. QA runner os executa via `execSync` no Windows.

- [ ] AC-1: TokenTracker tsc clean — Command: `bunx tsc --noEmit -p src/telemetry/tsconfig.json`
- [ ] AC-2: token-tracker integration test passes — Command: `bun test tests/integration/token-tracker.test.js`
- [ ] AC-6: span duration correlates with input tokens — Command: `bun test tests/integration/span-duration-correlates.js`
- [ ] AC-7: zero tokensSaved in dashboard files — Command: `node -e "const fs=require('fs');for(const f of ['templates/scripts/dashboard.js','templates/scripts/dashboard-ui.js']){if(fs.readFileSync(f,'utf8').includes('tokensSaved')){console.log('FAIL',f);process.exit(1)}}process.exit(0)"`
- [ ] AC-8: EventStore.spans() queryable under Bun — Command: `bun -e "const path=require('path');const os=require('os');const{EventStore}=require('./dist/runtime/event-store.js');const e=new EventStore(path.join(os.tmpdir(),'mst-qa-spans.db'));e.init();const r=e.spans({phase:'PLAN'});process.exit(Array.isArray(r)?0:1)"`

## Implementation

### Architecture

```
┌──────────────────────────────────────────────────────┐
│ Hook PreToolUse(Task)                                │
│   start span "task.dispatch"                         │
│   capture prompt bytes, model, agent_type            │
└─────────────────┬────────────────────────────────────┘
                  │
                  ▼
┌──────────────────────────────────────────────────────┐
│ Task executes (in Claude Code internals)             │
└─────────────────┬────────────────────────────────────┘
                  │
                  ▼
┌──────────────────────────────────────────────────────┐
│ Hook PostToolUse(Task)                               │
│   end span                                           │
│   capture response bytes, is_error                   │
│   emit OTLP JSON span to spans.jsonl                 │
│   project to EventStore.spans table                  │
└──────────────────────────────────────────────────────┘
```

### Schema addition (Phase 1 SQLite + new table)

```sql
CREATE TABLE spans (
  trace_id TEXT,
  span_id TEXT PRIMARY KEY,
  parent_span_id TEXT,
  name TEXT,
  started_at INTEGER,  -- ms epoch
  ended_at INTEGER,
  duration_ms INTEGER,
  attributes TEXT,  -- JSON
  spec TEXT,
  phase TEXT,
  model TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  is_error INTEGER  -- bool
);
CREATE INDEX idx_spans_spec ON spans(spec);
CREATE INDEX idx_spans_phase ON spans(phase);
CREATE INDEX idx_spans_started ON spans(started_at);
```

### New files

- `src/telemetry/token-tracker.ts` — class TokenTracker com startSpan/endSpan (manual OTLP JSON emit)
- `src/telemetry/otel-conventions.ts` — constantes `gen_ai.*` hard-coded (sem dep — names em OTel spec são estáveis)
- `src/telemetry/pricing.ts` — pricing por modelo (opus/sonnet/haiku) pra `cost_usd` derivado
- `templates/hooks/_lib/span-emitter.js` — helper CJS zero-dep que cada hook pode chamar (escreve OTLP JSON em spans.jsonl)
- `tests/integration/span-shape.test.js` — usa `@opentelemetry/otlp-transformer` pra validar shape do nosso emitter

### Changed files

- `templates/hooks/subagent-tracker.js` — PreToolUse inicia span; PostToolUse finaliza
- `templates/scripts/dashboard.js` — `/api/metrics` retorna `tokenUsage`
- `templates/scripts/dashboard-ui.js` — substitui widget "Economia Mustard" por "Custo Real"

### Pricing constants (revisable)

```typescript
// pricing as of 2026-05 — keep in sync with anthropic.com/pricing
export const PRICING = {
  'claude-opus-4-7': { input: 15, output: 75 },     // $/MTok
  'claude-sonnet-4-6': { input: 3, output: 15 },
  'claude-haiku-4-5': { input: 1, output: 5 },
};
```

### Token estimation

Hoje **não temos** acesso direto a `usage` da API Anthropic de dentro do hook (Claude Code não expõe). Fallback:
- **Input tokens** ≈ `Buffer.byteLength(prompt, 'utf8') / 4`
- **Output tokens** ≈ `Buffer.byteLength(tool_response_text, 'utf8') / 4`
- Heurística é **honesta** porque está documentada e batível (Claude tokenizer ratio ~4 bytes/token médio).

Quando Claude Code expor SDK headers via env var (`CLAUDE_LAST_USAGE` ou similar), trocar pra valor exato. Por enquanto, heurística é ground truth disponível.

## Risks

- **Estimativa de token é aproximação**: documentada. Erro <15% comparado com tokenização real.
- **OTel SDK pesa no startup**: usar sdk-node lite (sem auto-instrumentation completa); só GenAI conventions.
- **spans.jsonl cresce** → rotation policy (semanal) implementada com Phase 1 events.jsonl rotation.

## Out of scope

- Auto-instrumentation completa do Anthropic SDK (requer Claude Code expor hook ao SDK)
- Real-time streaming de spans pra backend remoto (só export local nesta fase)
- Pricing dinâmico (constants estáticas)

## Checklist

- [x] OTel GenAI conventions hard-coded (sem SDK runtime — manual OTLP JSON emit)
- [x] `TokenTracker` implementada (`src/telemetry/token-tracker.ts`)
- [x] Spans em spans.jsonl + tabela `spans` no SQLite + projection na migration
- [x] subagent-tracker emite spans (Pre/Post linkados via toolUseId, sidecar bridging + janitor)
- [x] Dashboard `/api/metrics` retorna tokenUsage (byPhase/byModel/byAgent/totalInput/totalOutput/costUsd)
- [x] UI substituída (widgetTokenUsage)
- [x] Pricing constants (claude-opus-4-7/sonnet-4-6/haiku-4-5 — anthropic.com/pricing 2026-05)
- [x] Tests: token-tracker (3/3), subagent-tracker-spans (2/2), span-duration-correlates (1/1, Pearson r > 0.5)
