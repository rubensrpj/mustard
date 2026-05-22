# Feature: Prompt Prefix Cacheável + Wave Diff + Métricas no Dashboard

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-14T12:20:00Z
### Lang: pt

## Contexto

Pipelines do Mustard dispacham hoje entre 5 e 9 agentes por feature, cada um recebendo prompt de 8-15k tokens com sobreposição grande de conteúdo estático (skills, recipe, pipeline-config snippet, instruções de papel). A API da Anthropic faz cache automático em prefixos byte-identical com ≥1024 tokens, cobrando 10% do normal nos hits, mas o template atual em `agent-prompt/SKILL.md` interleava blocos estáticos e dinâmicos (spec, diff, task) — então cada dispatch é único em bytes e o cache nunca ativa. Em paralelo, wave N+1 recebe a spec inteira de novo quando só precisa do slice da sua wave somado ao diff que a wave anterior produziu; o review agent re-lê 5-10 arquivos quando o diff cobre 90% do contexto; e `diff-context.js` é executado na fase ANALYZE quando ainda não houve trabalho — diff é sempre vazio. O impacto observável é que pipelines médios pagam 70-200k tokens por execução em conteúdo repetido, sem ganho de qualidade, e nenhum sinal disso aparece no dashboard para validar otimizações futuras.

## Boundaries

- `templates/commands/mustard/templates/agent-prompt/SKILL.md`
- `templates/commands/mustard/feature/SKILL.md`
- `templates/commands/mustard/bugfix/SKILL.md`
- `templates/commands/mustard/resume/SKILL.md`
- `templates/commands/mustard/review/SKILL.md`
- `templates/skills/karpathy-guidelines/SKILL.md`
- `templates/skills/karpathy-guidelines-detail/SKILL.md` (NEW)
- `templates/scripts/spec-extract.js` (NEW)
- `templates/scripts/prompt-prefix-stats.js` (NEW)
- `templates/scripts/diff-context.js`
- `templates/hooks/_lib/metrics-emit.js`
- `templates/hooks/_lib/prompt-cache-detect.js` (NEW)
- `templates/refs/agent-prompt/prefix-order.md` (NEW)
- `C:/Atiz/mustard-dashboard/src/pages/Telemetry.tsx`
- `C:/Atiz/mustard-dashboard/src/hooks/usePromptEconomy.ts` (NEW)
- `C:/Atiz/mustard-dashboard/src/api/promptEconomy.ts` (NEW)

Fora do escopo: `templates/pipeline-config.md` (apenas referência cross-link), `templates/agents/` (vazio neste projeto), demais páginas do dashboard.

## Summary

Três mudanças acopladas, todas instrumentadas:
1. **Prefixo estável**: reordena `agent-prompt/SKILL.md` para `[PREFIX-STABLE] → [VARIABLE]`, ativando cache nativo da Anthropic API.
2. **Wave slice + wave-N diff**: novo `spec-extract.js` corta a seção da wave; orquestrador injeta diff da wave anterior em vez de spec full.
3. **Subtrações**: remove `diff-context.js` da fase ANALYZE; review agent recebe `git diff` colado em vez de listar arquivos; `karpathy-guidelines` racha em `core` (50 linhas, sempre) + `detail` (carregado por trigger de refactor).

Métricas novas: `prompt-prefix.jsonl`, `wave-slice.jsonl`, `review-diff.jsonl`. Dashboard ganha página "Prompt Economy" em `Telemetry.tsx` (não cria rota nova).

## Files (~14)

| Arquivo | Operação | Wave |
|---|---|---|
| `templates/commands/mustard/templates/agent-prompt/SKILL.md` | Edit (reordena) | 2 |
| `templates/refs/agent-prompt/prefix-order.md` | Create | 1 |
| `templates/scripts/spec-extract.js` | Create | 1 |
| `templates/scripts/prompt-prefix-stats.js` | Create | 1 |
| `templates/scripts/diff-context.js` | Edit (sai do ANALYZE) | 2 |
| `templates/hooks/_lib/metrics-emit.js` | Edit (event consts) | 1 |
| `templates/hooks/_lib/prompt-cache-detect.js` | Create | 1 |
| `templates/skills/karpathy-guidelines/SKILL.md` | Edit (core only) | 1 |
| `templates/skills/karpathy-guidelines-detail/SKILL.md` | Create | 1 |
| `templates/commands/mustard/feature/SKILL.md` | Edit (kill diff em ANALYZE) | 2 |
| `templates/commands/mustard/bugfix/SKILL.md` | Edit (mesma alteração) | 2 |
| `templates/commands/mustard/resume/SKILL.md` | Edit (use spec-extract) | 2 |
| `templates/commands/mustard/review/SKILL.md` | Edit (diff-first) | 2 |
| `mustard-dashboard/src/pages/Telemetry.tsx` | Edit (nova seção) | 3 |
| `mustard-dashboard/src/hooks/usePromptEconomy.ts` | Create | 3 |
| `mustard-dashboard/src/api/promptEconomy.ts` | Create | 3 |

## Tasks

### Implementation Agent (Wave 1) — Mustard Infra

- [ ] Constantes de eventos em `templates/hooks/_lib/metrics-emit.js`: adiciona `EVENTS` object exportado com chaves `PROMPT_PREFIX_HIT`, `PROMPT_PREFIX_MISS`, `WAVE_SLICE`, `REVIEW_DIFF_FIRST`, `ANALYZE_DIFF_SKIP`. Não muda assinatura de `emitMetric`. Adiciona JSDoc listando os eventos.
- [ ] Create `templates/scripts/spec-extract.js`: CLI `bun spec-extract.js --spec <path> --wave <N>`. Lê spec.md, encontra header `### {Agent} Agent (Wave {N})` (case-insensitive, agnostic to role name), retorna a seção até o próximo `### ` ou EOF. Também aceita `--ac` para retornar só `## Acceptance Criteria`. Exporta função `extractWave(specPath, n)` para uso programático. Saída em stdout, exit 1 se wave não encontrada. Cap 4000 chars.
- [ ] Create `templates/scripts/prompt-prefix-stats.js`: CLI `bun prompt-prefix-stats.js`. Lê `.claude/.metrics/prompt-prefix.jsonl`, agrega: total dispatches, hits, misses, hit_rate, tokens_saved_total, top-3 prefix templates por hit count. Saída JSON. Cap output 2k chars. Reusável pelo dashboard.
- [ ] Create `templates/hooks/_lib/prompt-cache-detect.js`: módulo Node puro, exporta `analyzePrompt(text)` → `{prefix_len, prefix_hash, variable_len, prefix_cacheable: boolean}`. Considera "cacheable" se prefixo ≥1024 chars E contém marcador `<!-- PREFIX-STABLE -->`. Hash via crypto.createHash('sha256').
- [ ] Create `templates/refs/agent-prompt/prefix-order.md`: documento de ordem canônica do prompt. Define blocos: `## PREFIX-STABLE` (pipeline-config snippet, skills loaded, recipe, role rules) e `## VARIABLE` (spec slice, diff, retry context, task). Mostra exemplo. Inclui regra "qualquer interpolação dinâmica em PREFIX-STABLE invalida o cache". Cap 200 linhas.
- [ ] Split `templates/skills/karpathy-guidelines/SKILL.md`: mantém os 4 princípios em forma compacta (≤80 linhas total, frontmatter inclusive). Mover exemplos e elaboração para `templates/skills/karpathy-guidelines-detail/SKILL.md` (novo, frontmatter próprio, descrição menciona "Load when refactor ≥3 files or complex behavior change"). Atualizar `description` do principal para refletir o split.
- [ ] Build sanity: `bun test hooks/__tests__/hooks.test.js` deve passar (não introduzir regressão de hook).

### Implementation Agent (Wave 2) — Mustard Wiring (depois Wave 1)

- [ ] Reorder `templates/commands/mustard/templates/agent-prompt/SKILL.md`: dispatch template ganha marcador `<!-- PREFIX-STABLE -->` antes do bloco CONTEXT/REFERENCE/SKILLS/RECIPE/ROLE e marcador `<!-- VARIABLE -->` antes de spec/diff/TASK. Reescrever para garantir que conteúdo interpolado (`{recommended_skills}`, `{entity_info}`, `{recipe_context}`) sai do PREFIX-STABLE (fica como ID de skill resolvido pelo agente, não texto inline) — quem precisa do texto carrega via Skill tool. Adicionar nota explicando o efeito de cache hit.
- [ ] Edit `templates/commands/mustard/feature/SKILL.md`: remover qualquer chamada de `diff-context.js` na fase ANALYZE (manter em PLAN e EXECUTE). Adicionar 1 linha emitindo `metrics-emit.js` com evento `ANALYZE_DIFF_SKIP` por pipeline iniciado.
- [ ] Mesma alteração em `templates/commands/mustard/bugfix/SKILL.md`.
- [ ] Edit `templates/commands/mustard/resume/SKILL.md`: na dispatch loop entre waves, chamar `bun spec-extract.js --wave {N}` e injetar resultado como `{spec_slice}`. Concatenar com `diff-context.js` da wave anterior (cache em `.pipeline-states/{spec}.wave-{N-1}.diff.md`). Emitir métrica `WAVE_SLICE` com `tokens_saved = full_spec_chars - slice_chars` dividido por 4.
- [ ] Edit `templates/commands/mustard/review/SKILL.md`: dispatch do review agent passa `git diff` (via `diff-context.js`) como bloco principal `## DIFF`, e instrui "Read files apenas se diff for ambíguo". Emitir `REVIEW_DIFF_FIRST` com `tokens_saved` estimado como `(reads_avoided * 2000) / 4`.
- [ ] Edit `templates/scripts/diff-context.js`: adiciona flag `--phase {analyze|plan|execute}`. Em `analyze`, retorna stdout vazio com exit 0 (silent no-op). Mantém comportamento atual nos outros casos.
- [ ] Build sanity: `bun test hooks/__tests__/hooks.test.js` continua verde.

### Implementation Agent (Wave 3) — Dashboard (parallel-safe com final de Wave 2)

- [ ] Create `mustard-dashboard/src/api/promptEconomy.ts`: lê `.claude/.metrics/{prompt-prefix,wave-slice,review-diff,analyze-diff-skip}.jsonl` via API Tauri existente (mesmo padrão dos outros hooks). Exporta `fetchPromptEconomy()` retornando `{ prefixHitRate, tokensSavedTotal, byEvent: {...} }`.
- [ ] Create `mustard-dashboard/src/hooks/usePromptEconomy.ts`: hook React que chama `fetchPromptEconomy` com polling 15s (mesma cadência de `useAggregate.ts`). Cache em memória.
- [ ] Edit `mustard-dashboard/src/pages/Telemetry.tsx`: adicionar seção "Prompt Economy" com 3 cards (Prefix Cache Hit Rate %, Tokens Saved Total, Events 24h) e um sparkline pequeno (mesma lib de gráfico que a página já usa — não importar nova). Ordem: depois dos cards existentes, antes de qualquer footer/empty state. Texto em PT consistente com o restante da página.
- [ ] Build sanity: `cd mustard-dashboard && bun run build` passa sem warning novo.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: `spec-extract.js` extrai wave 2 corretamente — Command: `node -e "const {extractWave}=require('./templates/scripts/spec-extract.js'); const out=extractWave('.claude/spec/active/2026-05-14-prompt-prefix-metrics/spec.md',2); process.exit(out.includes('Wave 2')?0:1)"`
- [x] AC-2: agent-prompt SKILL tem PREFIX-STABLE antes de VARIABLE — Command: `node -e "const fs=require('fs'); const c=fs.readFileSync('templates/commands/mustard/templates/agent-prompt/SKILL.md','utf8'); const p=c.indexOf('<!-- PREFIX-STABLE -->'); const v=c.indexOf('<!-- VARIABLE -->'); process.exit(p>0 && v>p?0:1)"`
- [x] AC-3: feature/SKILL.md já não invoca diff-context em ANALYZE — Command: `node -e "const fs=require('fs'); const c=fs.readFileSync('templates/commands/mustard/feature/SKILL.md','utf8'); const a=c.indexOf('### ANALYZE'); const p=c.indexOf('### PLAN'); const seg=c.slice(a,p>0?p:a+5000); process.exit(seg.includes('diff-context.js')?1:0)"`
- [x] AC-4: metrics-emit exporta constantes — Command: `node -e "const m=require('./templates/hooks/_lib/metrics-emit.js'); const ok=m.EVENTS && m.EVENTS.PROMPT_PREFIX_HIT && m.EVENTS.WAVE_SLICE && m.EVENTS.REVIEW_DIFF_FIRST; process.exit(ok?0:1)"`
- [x] AC-5: karpathy split — core ≤80 linhas, detail existe — Command: `node -e "const fs=require('fs'); const core=fs.readFileSync('templates/skills/karpathy-guidelines/SKILL.md','utf8').split(String.fromCharCode(10)).length; const hasDetail=fs.existsSync('templates/skills/karpathy-guidelines-detail/SKILL.md'); process.exit(core<=80 && hasDetail?0:1)"`
- [x] AC-6: hooks tests passam — Command: `bun test hooks/__tests__/hooks.test.js`
- [x] AC-7: dashboard buildlimpo com hook novo — Command: `bash -c 'cd /c/Atiz/mustard-dashboard && bun run build 2>&1 | tail -n 5'` (exit 0)
- [x] AC-8: dashboard render contém "Prompt Economy" — Command: `node -e "const fs=require('fs'); const c=fs.readFileSync('C:/Atiz/mustard-dashboard/src/pages/Telemetry.tsx','utf8'); process.exit(c.includes('Prompt Economy')?0:1)"`

## Dependencies

- Wave 1 deve completar antes de Wave 2 (Wave 2 usa `spec-extract.js`, `prompt-cache-detect.js`, constantes de evento, karpathy split).
- Wave 3 pode iniciar quando Wave 1 está OK (precisa apenas das constantes de evento e da garantia que `.metrics/*.jsonl` recebem dados — Wave 2 começa a alimentar, mas a UI tolera arquivos vazios).
- Sem dependência externa (sem novo pacote npm em nenhum dos repos).

## Concerns

- Wave 3 descobriu que `metrics-emit.js` grava `${event}.jsonl` (1 arquivo por nome de evento), então `prompt-prefix-stats.js` da Wave 1 lia `prompt-prefix.jsonl` (inexistente) em vez de `prompt-prefix-hit.jsonl` + `prompt-prefix-miss.jsonl`. **Fix aplicado**: script agora lê os dois arquivos e agrega — corrigido em pós-Wave 3 antes da QA.
- Sparkline e contagem `events24h` no dashboard são "degraded" — o backend Rust de telemetria expõe apenas `fires`/`tokens_saved`/`most_recent_ts` por arquivo, sem timestamps por linha. Sparkline mostra 24 slots com totais no último bar. Melhoria real exige endpoint Rust em `src-tauri/src/telemetry.rs` (fora do boundary deste spec).

## Non-Goals

- Não muda model routing (`feedback_no_routing_downgrade`).
- Não adiciona hook bloqueante novo — todos os eventos são informativos (`feedback_mustard_transparent_execution`).
- Não toca o esquema do `entity-registry.json` nem o `sync-registry.js`.
- Não move/renomeia arquivos existentes além do split do karpathy.
- Não introduz dependência Python (graphify foi rejeitado — vide chat).
