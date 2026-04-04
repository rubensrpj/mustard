# Plano: 10 Features inspiradas no Everything Claude Code (ECC)

## Context

O repo [everything-claude-code](https://github.com/affaan-m/everything-claude-code) (50K+ stars, vencedor do hackathon Anthropic) tem ideias que complementam o Mustard. Após análise, identificamos 10 features de alto valor que se encaixam na arquitetura existente sem comprometer a filosofia de zero-deps e arquivos simples.

---

## Ordem de Implementação (4 waves)

| Wave | Features | Rationale |
|------|----------|-----------|
| 1 | F1 (Hook Controls), F5 (Compact Guidance), F9 (Security Scan) | F1 é fundação; F5 é text-only; F9 é standalone |
| 2 | F3 (Loop Prevention), F2 (Knowledge Extraction), F10 (Pass@k) | F3 depende de _lib do F1; F2 e F10 são independentes |
| 3 | F4 (Verification CLOSE), F6 (MCP Budget), F8 (Continuous Learning) | F4 usa sync-detect; F6 usa _lib; F8 modifica schema knowledge |
| 4 | F7 (Cross-IDE Adapter) | Mais exploratório; beneficia da estabilidade das outras |

---

## Wave 1

### F1: Runtime Hook Controls (envvars) — Size M

**Objetivo:** Controlar hooks via `MUSTARD_HOOK_PROFILE` e `MUSTARD_DISABLED_HOOKS` sem editar arquivos.

**Criar:**
- `templates/hooks/_lib/hook-env.js` — exporta `shouldRun(hookName)` e `isStrictMode()`

**Profiles:**
- `minimal`: só bash-safety + file-guard (segurança crítica)
- `standard`: todos os hooks (default)
- `strict`: todos + warnings viram blocks (review-gate)

**Modificar (adicionar guard de 3 linhas no início de cada hook):**
- Todos os 12 hooks em `templates/hooks/*.js`

**Testes:** Unit test de `_lib/hook-env.js` + integration com envvars no test helper existente.

---

### F5: Compact Guidance no Pipeline — Size S

**Objetivo:** Sugerir `/compact` em momentos estratégicos do pipeline.

**Modificar (apenas texto advisory):**
- `templates/pipeline-config.md` — nova seção "Compact Guidance"
- `templates/commands/mustard/feature/SKILL.md` — após ANALYZE: "Se >8 file reads, sugerir compact"
- `templates/commands/mustard/bugfix/SKILL.md` — após retries >2: "Sugerir compact + /resume"

Sem código novo, sem testes automatizados.

---

### F9: Security Scanning — Size M

**Objetivo:** Scan de secrets e exposições, integrável no `/mustard:review`.

**Criar:**
- `templates/scripts/security-scan.js` — 14 regex patterns (AWS keys, GitHub tokens, Stripe, JWT, private keys, connection strings, etc.)
- Scan recursivo com IGNORE_DIRS (node_modules, .git, dist, etc.)
- 4 categorias: secrets, env exposure, hook permissions, file access
- Exit code 1 se secrets encontrados

**Modificar:**
- `templates/commands/mustard/scan/SKILL.md` — adicionar fase "Security Scan" opcional

**Testes:** Pattern detection com fixtures (arquivo com AKIA fake, sk_test fake, etc.).

---

## Wave 2

### F3: Observer Loop Prevention — Size M

**Objetivo:** 5 camadas de proteção contra loops recursivos em hooks.

**Modificar:**
- `templates/hooks/_lib/hook-env.js` — adicionar:
  - `acquireGuard(hookName)` — re-entrancy via env flag
  - `checkDepth(max=3)` — depth counter via `MUSTARD_HOOK_DEPTH`
  - `isSelfDelegation(data)` — session ID tracking
  - `isInHookPhase()` — phase gating (ready para quando harness suportar)
  - `guardedRun(hookName, data)` — combined guard
- `templates/hooks/subagent-tracker.js` — usar `isSelfDelegation()` no handlePreToolUse

**Testes:** Unit tests dos guards com env vars mockadas.

---

### F2: Stop Hook para Knowledge Extraction — Size M

**Objetivo:** Extrair padrões automaticamente ao final da sessão, antes do cleanup.

**Criar:**
- `templates/hooks/session-knowledge.js` — SessionEnd hook que:
  1. Lê `.pipeline-states/*.json` (pipelines ativas)
  2. Lê `.agent-memory/_index.json` (summaries de agents)
  3. Extrai patterns (retries >2 = lesson, agent findings = pattern)
  4. Salva via `knowledge-update.js` (max 5 por sessão)

**Modificar:**
- `templates/settings.json` — adicionar session-knowledge.js ANTES de session-cleanup.js no array SessionEnd (ordem crítica!)

**Testes:** Mock de pipeline state + agent memory, verificar knowledge.json atualizado.

---

### F10: Eval Metrics / Pass@k — Size M

**Objetivo:** Medir taxa de sucesso na primeira tentativa e retries médios.

**Modificar:**
- `templates/hooks/metrics-tracker.js` — adicionar campo `agentAttempts` e tracking de retries por fase
- `templates/scripts/metrics-collect.js` — computar Pass@1 (pipelines sem retries / total) e avg retries
- `templates/commands/mustard/complete/SKILL.md` — salvar `pass1: boolean` no archive de métricas
- `templates/commands/mustard/stats/SKILL.md` — exibir "Pass@1: N%, Avg retries: N"

**Testes:** Mock de metrics archive com 5 files, verificar cálculo correto.

---

## Wave 3

### F4: Verification Step no CLOSE — Size M

**Objetivo:** Gate de build/test antes de finalizar pipeline.

**Criar:**
- `templates/scripts/verify-pipeline.js` — lê pipeline-config.md para build/validate commands, executa por subprojeto, reporta passed/failed/skipped

**Modificar:**
- `templates/commands/mustard/complete/SKILL.md` — inserir step "Verification Gate" antes do checkpoint: rodar verify-pipeline.js, bloquear se falhar, fail-open se script ausente

**Testes:** Mock com build commands simples (echo ok / exit 1).

---

### F6: MCP Tool Budget Awareness — Size S

**Objetivo:** Avisar sobre excesso de MCP servers/tools no início da sessão.

**Criar:**
- `templates/hooks/mcp-budget.js` — SessionStart hook que:
  1. Conta servers em `.claude/mcp.json` e `~/.claude/mcp.json`
  2. Warn se >10 servers ou >80 tools estimados
  3. Advisory only (additionalContext)

**Modificar:**
- `templates/settings.json` — registrar em SessionStart, timeout 3s

**Testes:** Mock mcp.json com 12 servers, verificar warning.

---

### F8: Continuous Learning / Instincts — Size L

**Objetivo:** Evolução do knowledge com confidence scoring e clustering.

**Modificar:**
- `templates/scripts/knowledge-update.js` — schema enhancement:
  - Novos campos: `confidence` (0-1), `occurrences`, `lastSeen`
  - Duplicata: incrementa occurrences, recalcula confidence (`min(1.0, 0.3 + occurrences * 0.1)`)
  - Backwards-compatible: `|| defaultValue` para campos ausentes
- `templates/commands/mustard/knowledge/SKILL.md` — novas ações:
  - `evolve`: cluster entries por tags, gerar recomendações
  - `export`: salvar knowledge-export-{date}.json
  - `import <file>`: importar entries (dedup automática)

**Testes:** Dedup com confidence incrementando, cap em 1.0, backwards compat.

---

## Wave 4

### F7: Cross-IDE Adapter (Cursor) — Size L

**Objetivo:** Reusar hooks do Mustard em outros IDEs via adapter pattern.

**Criar:**
- `templates/adapters/cursor/adapter.js` — traduz stdin Cursor → protocolo Claude Code, spawna hook, traduz resposta de volta
- `templates/adapters/cursor/README.md` — docs de uso

**Modificar:**
- `src/commands/init.ts` — flag `--cursor` para gerar `.cursor/hooks/adapter.js`

**Nota:** Marcado como **experimental** — formato de hooks do Cursor não é padronizado ainda.

**Testes:** Round-trip com mock Cursor format → bash-safety.js → response mapeada.

---

## Arquivos Críticos (referência rápida)

| Arquivo | Ação | Features |
|---------|------|----------|
| `templates/hooks/_lib/hook-env.js` | **CRIAR** | F1, F3, F6 |
| `templates/hooks/session-knowledge.js` | **CRIAR** | F2 |
| `templates/hooks/mcp-budget.js` | **CRIAR** | F6 |
| `templates/scripts/security-scan.js` | **CRIAR** | F9 |
| `templates/scripts/verify-pipeline.js` | **CRIAR** | F4 |
| `templates/adapters/cursor/adapter.js` | **CRIAR** | F7 |
| `templates/settings.json` | modificar | F2, F6 |
| `templates/hooks/*.js` (12 files) | modificar (3 linhas) | F1 |
| `templates/scripts/knowledge-update.js` | modificar | F8 |
| `templates/scripts/metrics-collect.js` | modificar | F10 |
| `templates/hooks/metrics-tracker.js` | modificar | F10 |
| `templates/pipeline-config.md` | modificar | F5 |
| `templates/commands/mustard/feature/SKILL.md` | modificar | F5 |
| `templates/commands/mustard/bugfix/SKILL.md` | modificar | F5 |
| `templates/commands/mustard/complete/SKILL.md` | modificar | F4, F10 |
| `templates/commands/mustard/knowledge/SKILL.md` | modificar | F8 |
| `templates/commands/mustard/scan/SKILL.md` | modificar | F9 |
| `src/commands/init.ts` | modificar | F7 |

## Verificação

1. **Unit tests:** `node --test templates/hooks/__tests__/hooks.test.js`
2. **Build:** `npm run build && npm test`
3. **Integration manual:** `node bin/mustard.js init` em projeto temporário, verificar que todos os novos arquivos são copiados
4. **Hook smoke test:** Para cada novo hook, testar com stdin JSON mockado e verificar stdout/exit code
5. **Backwards compat:** Rodar `mustard update` em projeto existente, verificar que knowledge.json e metrics existentes não quebram
