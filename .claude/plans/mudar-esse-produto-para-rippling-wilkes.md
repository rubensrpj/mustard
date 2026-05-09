# Auditoria de Hooks + Cleanup grepai

## Contexto

Análise solicitada após descartar a ideia de migrar Mustard para Go/Rust/Python (linguagem não é o gargalo — LLM inference + número de hooks é). O foco passa a ser: cortar hooks que não pagam o custo, consolidar duplicações, e remover referências a `grepai` (deprecated, removido do source mas ainda mencionado em docs).

Auditoria cobriu 24 hooks ativos em `templates/hooks/` + `templates/settings.json`. Sem órfãos, sem quebrados.

## Critérios de decisão

- **Cortar** hooks heurísticos default-off ou anti-slope sem env control (memória: "subtrair > adicionar")
- **Cortar** hooks com hardcode de tecnologia (memória: "Mustard 100% agnóstico")
- **Manter** sensores reais (build/QA/registry) e guardrails de segurança
- **Consolidar** matcher entries duplicadas em settings.json (cleanup puro)

## Ações Tier 1 — Remover (4 hooks)

| Arquivo | Justificativa | Settings.json action |
|---------|---------------|----------------------|
| `templates/hooks/regression-guard.js` | Default OFF + hardcode `npm test` (não agnóstico) + 120s timeout em PostToolUse Write\|Edit | Remover entry de PostToolUse |
| `templates/hooks/mcp-budget.js` | "Advisory" no SessionStart, sem callers reais | Remover entry de SessionStart |
| `templates/hooks/epic-detect.js` | Anti-slope sem env control, dispara em todo Edit | Remover entry de PostToolUse |
| `templates/hooks/debug-loop-guard.js` | Anti-slope heurístico (>5 sleeps), 5000s timeout. Sensor real = harness counter | Remover entry de PostToolUse |

Também deletar `MUSTARD_REGRESSION_MODE` de `templates/settings.json:11` (env órfão após remoção).

## Ações Tier 2 — Consolidar

1. **`templates/settings.json`** — mergear entries duplicadas:
   - 7 entries `PreToolUse:Write|Edit` → 1 entry com array de hooks
   - 6 entries `PostToolUse:Write|Edit` → 1 entry com array de hooks
   - Após cortes Tier 1, fica ~3 entries fundidos

2. **`templates/hooks/session-knowledge.js`** — adicionar early-skip no início:
   ```js
   const meta = readJSON('.knowledge-seen.json')?._meta;
   if (meta?.lastWrite && (Date.now() - meta.lastWrite) < 5*60*1000) process.exit(0);
   ```
   Evita reescrita redundante quando `session-knowledge-inc.js` rodou recente.

3. **Auditar registros redundantes de `tool-use-counter.js` e `subagent-tracker.js`** — ambos aparecem em 4-5 eventos. Verificar via `node templates/hooks/__tests__/hooks.test.js` quais eventos são realmente necessários (PreToolUse `.*` + PostToolUse Task podem cobrir tudo). Se sim, remover entries SubagentStart/Stop redundantes.

## Ações Tier 3 — Melhorar (refator surgical)

1. **`templates/hooks/recommended-skills-audit.js`** — mover de `PreToolUse:Task` (timeout 3000ms = bloqueia dispatch) para `PostToolUse:Task` (não bloqueia). Em settings.json apenas trocar a posição.

2. **`templates/hooks/user-prompt-hint.js`** — adicionar gate por env var:
   ```js
   if (process.env.MUSTARD_PROMPT_HINT_MODE === 'off') process.exit(0);
   ```
   Adicionar `"MUSTARD_PROMPT_HINT_MODE": "off"` em settings.json:env (default off).

3. **`templates/hooks/spec-hygiene.js`** — mover de SessionStart hook para script on-demand. Criar `templates/scripts/spec-hygiene.js` (mover lógica) e expor via `/mustard:status`. Remover entry SessionStart.

4. **`templates/hooks/session-knowledge*.js`** — adicionar cap de tamanho em `knowledge.json` (e.g. 1MB). Logic existente em `harness-init.js` para events.jsonl pode ser reutilizada via `_lib/`.

## Cleanup grepai (separado)

Editar `C:\Atiz\Mustard\CLAUDE.md`:
- **linha 64**: `│   └── services/            # ollama.ts, grepai.ts` → `│   └── services/            # ollama.ts`
- **linha 119**: remover linha `    -> semanticAnalyzer() - grepai patterns (optional)`

Verificar: CHANGELOG já documenta remoção (linhas 9-10, 20, 22, 62, 84) — manter como histórico, não mexer.

## Arquivos críticos para modificar

| Path | Mudança |
|------|---------|
| `templates/settings.json` | Remover 4 hooks, mergear matchers duplicados, adicionar env var hint |
| `templates/hooks/regression-guard.js` | Deletar |
| `templates/hooks/mcp-budget.js` | Deletar |
| `templates/hooks/epic-detect.js` | Deletar |
| `templates/hooks/debug-loop-guard.js` | Deletar |
| `templates/hooks/session-knowledge.js` | Adicionar early-skip |
| `templates/hooks/user-prompt-hint.js` | Adicionar env gate |
| `templates/hooks/spec-hygiene.js` | Mover para `templates/scripts/spec-hygiene.js` |
| `templates/CLAUDE.md` | Atualizar contagem (linha 30): "23 lifecycle hooks" → "19 lifecycle hooks" |
| `CLAUDE.md` (raiz) | Linhas 64 e 119: remover grepai |

## Verificação end-to-end

1. `node --test templates/hooks/__tests__/hooks.test.js` — todos os tests passam
2. `node bin/mustard.js init` em projeto sandbox — gera `.claude/` correto, 19 hooks no settings
3. Sessão real Claude Code: nenhum hook órfão é invocado, nenhum erro de "hook not found"
4. `Grep("grepai", path="C:\\Atiz\\Mustard")` — só CHANGELOG matches (histórico)
5. Verificar metrics-tracker.js continua emitindo events.jsonl normalmente após cortes

## Notas

- Esta é uma análise pura. Nenhum código foi alterado.
- Memória `feedback_analysis_pattern.md` aplicada: subtrair antes de propor adicionar.
- Memória `feedback_eliminate_dont_mitigate.md` aplicada: hooks heurísticos sem sensor real saem por design, não viram "monitorar e ver".
