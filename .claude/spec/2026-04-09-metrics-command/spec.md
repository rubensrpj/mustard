# Enhancement: metrics-command
### Status: completed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Criar `/mustard:metrics` — comando único com subcomandos para gerenciar enforcement metrics:
- **Mode control**: `observe`, `warn`, `strict` — define modo do budget-gate
- **Inspection**: `status`, `report`, `reset` — visualiza estado e relatórios

Persistência via `.claude/.metrics/.mode` file (minor extension em `context-budget.js` para fallback de file após env var).

## Why
Infra de métricas (budget-observability + metrics framework) foi entregue mas sem UX — user precisa manualmente setar env var e rodar scripts. Comando `/mustard:metrics` cria interface prática para ativar observe mode, checar estado, rodar reporter.

## Boundaries
- `templates/commands/mustard/metrics/SKILL.md` (create)
- `templates/hooks/context-budget.js` (minor extension — file fallback para mode)
- `.claude/commands/mustard/metrics/SKILL.md` (mirror)
- `.claude/hooks/context-budget.js` (mirror)

## Checklist
### templates-impl Agent
- [x] Ler `templates/commands/mustard/feature/SKILL.md` head (~30 linhas) para entender estrutura/format de SKILL.md command existente (frontmatter, trigger, actions)
- [x] Ler `templates/hooks/context-budget.js` seção de MODE detection (estendida por budget-observability) para saber onde adicionar fallback
- [x] Criar `templates/commands/mustard/metrics/SKILL.md` com:
  - Frontmatter YAML: `name`, `description`
  - `## Trigger`: `/mustard:metrics <subcommand>`
  - Subcommands listados em tabela
  - `## Actions` por subcommand

### Subcommand actions (detalhe no SKILL.md)

| Subcommand | Ação |
|---|---|
| `observe` | `fs.writeFileSync('.claude/.metrics/.mode', 'observe')` (mkdir se precisar). Reportar "Mode set to observe. Effective next hook fire." |
| `warn` | Mesmo com `'warn'` |
| `strict` | Se `.mode` existe → delete (volta ao default). Reportar "Mode reset to strict (default)." |
| `status` | Ler `.mode` (ou "strict default"); listar arquivos em `.claude/.metrics/` com sizes; rodar `rtk node .claude/scripts/metrics-report.js` e mostrar output |
| `report` | Passar args para `rtk node .claude/scripts/metrics-report.js` (suporta `--since`, `--event`) |
| `reset` | `AskUserQuestion` confirmação; se yes, delete `*.jsonl` em `.claude/.metrics/` (preservar `.mode` file) |

### Hook extension (minor)
- [x] Em `templates/hooks/context-budget.js`, localizar a função/line que lê `process.env.CONTEXT_BUDGET_MODE`
- [x] Refatorar para função `getMode()`:
  ```js
  function getMode() {
    if (process.env.CONTEXT_BUDGET_MODE) return process.env.CONTEXT_BUDGET_MODE;
    try {
      const modeFile = path.join(process.cwd(), '.claude', '.metrics', '.mode');
      if (fs.existsSync(modeFile)) return fs.readFileSync(modeFile, 'utf8').trim();
    } catch (_) {}
    return 'strict';
  }
  ```
- [x] Substituir usages diretas de `process.env.CONTEXT_BUDGET_MODE || 'strict'` por `getMode()`
- [x] Env var MUST ter precedência sobre file (test it)

### Finalização
- [x] Mirror `templates/commands/mustard/metrics/SKILL.md` → `.claude/commands/mustard/metrics/SKILL.md`
- [x] Mirror `templates/hooks/context-budget.js` → `.claude/hooks/context-budget.js`
- [x] Build: `rtk npm run build` → PASS
- [x] Hook tests: `rtk bun test templates/hooks/__tests__/` → 61/61 (26 unit + 35 integration)
- [x] **Smoke test**:
  1. `rtk node -e "require('fs').writeFileSync('.claude/.metrics/.mode','observe'); require('fs').mkdirSync('.claude/.metrics',{recursive:true})"` — simular command observe
  2. Rodar hook com payload de teste — confirmar comportamento observe
  3. Remover `.mode` file — confirmar volta a strict
  4. Setar `CONTEXT_BUDGET_MODE=strict` env — confirmar que env precede file (observe file → still deny)

## Files (~4)
- `templates/commands/mustard/metrics/SKILL.md` (create)
- `templates/hooks/context-budget.js` (extend)
- `.claude/commands/mustard/metrics/SKILL.md` (mirror)
- `.claude/hooks/context-budget.js` (mirror)

## Acceptance
- Comando `/mustard:metrics` aparece na lista de skills do Mustard (auto-discovery via directory structure)
- 6 subcomandos documentados no SKILL.md: observe, warn, strict, status, report, reset
- `context-budget.js` tem `getMode()` com precedência env > file > default
- Mirrors sync
- Build + 61/61 tests
- Smoke test valida env precedence sobre file

## Guards
- Env var tem precedência absoluta (não mudar contract)
- File fallback é opt-in (só existe se user rodou `/mustard:metrics observe|warn`)
- `strict` subcommand DELETA o file (não só muda conteúdo) — volta ao default real
- Fail-open preservado no hook
- `reset` subcommand preserva `.mode` file (só limpa `.jsonl`)
- Reset pede confirmação via AskUserQuestion — nunca delete silencioso
- Built-ins only

## Result

### Files
- `templates/commands/mustard/metrics/SKILL.md` (created, 52 lines)
- `.claude/commands/mustard/metrics/SKILL.md` (mirror)
- `templates/hooks/context-budget.js:26-34` — `getMode()` function added, `const MODE = getMode()`
- `.claude/hooks/context-budget.js` (mirror)

### Smoke Test
- observe (file only, no env): `{"permissionDecision":"allow"}` — PASS
- strict (env CONTEXT_BUDGET_MODE=strict overrides .mode=observe): deny — PASS
- strict (default, no file, no env): deny — PASS

### Build / Tests
- npm run build: PASS (tsc clean)
- bun test: 61/61 PASS
