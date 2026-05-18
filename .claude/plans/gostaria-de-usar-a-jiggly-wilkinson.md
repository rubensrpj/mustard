# Plano — Refinar `templates/scripts/`: excluir, mesclar, renomear

## Context

A ideia do cofre Obsidian expôs `skill-graph.js` órfão. O usuário pediu, antes de features
novas, auditar `templates/scripts/` com rigor e montar um plano de **exclusão + merge +
rename** — pasta de scripts enxuta, sem código morto, sem família dispersa, com nomes adequados.

Auditoria cruzou cada script contra `commands/`, `refs/`, `hooks/`, `settings.json`, `src/` e
require/exec entre scripts. Headers de todas as famílias candidatas foram lidos.

**28 scripts confirmados ligados e distintos** — não exigem ação (analyze-validation,
complete-spec, epic-fold, exec-rewave-check, knowledge-update*, memory-write*, memory-persist*,
metrics-collect*, metrics-report*, recipe-match, security-scan, skill-validate*, sync-registry,
verify-pipeline, sync-detect, pipeline-summary, scope-decompose, wave-tree, wave-dependency,
wave-size-check, diff-context, spec-extract, spec-link, emit-phase, verify-emit, qa-run,
statusline, otel-collector, harness-views, diagnose-otel, mark-checklist-item, scan/*). Os
marcados `*` entram em merge abaixo.

## 1. Excluir (código morto — 3 arquivos)

| Arquivo | Evidência | Ação extra |
|---|---|---|
| `_metrics-write.js` | Morto. Todos os ~25 hooks usam `hooks/_lib/metrics-emit.js`. Zero callers. | Corrigir `README.md:348` |
| `emit-retry.js` | Zero callers. `emit-phase.js` (irmão) é usado; este nunca. Retries já contados em nível de hook (knowledge.json). | — |
| `rtk-gain-import.js` | Zero callers. Faz snapshot em `.metrics/rtk-gain.jsonl`; metrics leem RTK ao vivo via `_rtk-gain.js`, ninguém lê o snapshot. | Corrigir `README.md:343`; **confirmar** que nada lê `rtk-gain.jsonl` antes de excluir |

## 2. Mesclar (3 consolidações — 8 arquivos → 3)

**M1 — `skills.js`** ← `skill-validate.js` + `skill-graph.js` + `skill-orphan-audit.js`
Subcomandos `validate` / `graph` / `orphans`. Os três descobrem o conjunto de skills do mesmo
jeito (o header do `skill-graph` diz literalmente "same discovery as skill-orphan-audit").
Compartilha discovery + parse de frontmatter. `graph` e `orphans` hoje órfãos viram subcomandos
de um CLI usado → ficam descobríveis e reaproveitam a discovery. Callers: `scan/SKILL.md:19` +
refs (`evidence-rules.md:85`, `ac-cross-shell.md:9`) → `skills.js validate`.

**M2 — `metrics.js`** ← `metrics-collect.js` + `metrics-report.js`
Subcomandos `collect` / `report` (flags próprias de cada um preservadas: `--hooks-only`,
`--since`/`--event`/`--compare`). Ambos agregam enforcement + RTK via `_rtk-gain.js`; são
complementares. Callers: `status/SKILL.md:14`, `stats/SKILL.md:17`, `metrics/SKILL.md:14,20`.

**M3 — `memory.js`** ← `memory-write.js` + `memory-persist.js` + `knowledge-update.js`
Subcomandos `agent` / `decision` / `knowledge`. Os três são o **mesmo padrão**: ler entrada
JSON (stdin/`--json`) → ler store JSON rotativo → dedup/append → podar → escrever. Diferem só
no store-alvo e no cap (20 / 50 / 200). Merge elimina ~80% de boilerplate (uma poda, um
read-write). Callers: `feature:162`, `complete:86,77`, `approve:59`, `knowledge:143` (~6 linhas).
Nome `memory.js` é discutível (knowledge ≠ memória exata) — alternativa `persist.js`.

## 3. Extrair lib compartilhada (dedup — sem merge de CLI)

`detectRole()` está **copiado** em `wave-size-check.js` e `exec-rewave-check.js` e espelhado em
`wave-dependency.js` (3 cópias). Extrair `detectRole` + parse de `## Files` para
`scripts/_lib/wave-lib.js`; os 3 scripts passam a importar.

**Por que não mesclar os wave-* num `wave.js`:** contratos de I/O misturados (`scope-decompose`
e `wave-dependency` leem stdin-JSON puro; `exec-rewave-check`/`wave-size-check`/`wave-tree` são
CLIs de flag) e callers espalhados em feature/approve/resume. O ganho real é a dedup — obtida
pela lib, sem o risco de um merge de CLI. (Eliminar risco por design.)

## 4. Renomear

- `harness-views.js` → `event-projections.js` — é um módulo de projeções puras do event log;
  "views" é genérico. Baixo raio (refs/resume/*). Opcional.
- `_rtk-gain.js` — **manter o nome** (prefixo `_` é a convenção de módulo interno do Mustard,
  igual `_lib/`); opcionalmente mover para `scripts/_lib/`.
- `diagnose-otel.js` / `otel-collector.js` — **não mesclar**: um é health-check CLI, o outro é
  servidor HTTP de longa duração. Famílias só no nome. Mantidos separados.

## Critical files

- **Excluir:** `templates/scripts/_metrics-write.js`, `emit-retry.js`, `rtk-gain-import.js`
- **Criar:** `templates/scripts/skills.js`, `metrics.js`, `memory.js`, `_lib/wave-lib.js`
- **Remover após merge:** `skill-validate.js`, `skill-graph.js`, `skill-orphan-audit.js`,
  `metrics-collect.js`, `metrics-report.js`, `memory-write.js`, `memory-persist.js`,
  `knowledge-update.js`
- **Renomear:** `harness-views.js` → `event-projections.js`
- **Atualizar callers:** `commands/mustard/{scan,status,stats,metrics,feature,complete,approve,
  knowledge}/SKILL.md`, `refs/scan/evidence-rules.md`, `refs/ac-cross-shell.md`,
  `refs/resume/*.md`
- **Editar p/ importar lib:** `wave-size-check.js`, `exec-rewave-check.js`, `wave-dependency.js`
- **Doc:** `README.md` (linhas 343, 348), `templates/CLAUDE.md` (recontar "25 scripts")
- **Testes:** `__tests__/metrics-report.test.js` → apontar para `metrics.js report`
- `src/` não precisa mudar — `update.ts` copia `scripts/` inteiro; só comentários citam nomes.

## Verification

- `bun test hooks/__tests__/hooks.test.js` passa (`harness-wave7.test.js`/spec-link e
  `checklist-mark.test.js`/mark-checklist-item permanecem intactos).
- Grep confirma zero referências vivas a nomes excluídos/antigos (specs concluídas são
  histórico imutável — referências lá são aceitáveis; AC-14 de `honest-prompt-economy` cita
  `skill-graph.js`, já passou, fica como histórico).
- `skills.js validate|graph|orphans`, `metrics.js collect|report`, `memory.js agent|decision|
  knowledge` rodam e produzem a mesma saída dos scripts originais.
- `/scan`, `/status`, `/stats`, `/metrics`, `/feature` exercem os scripts mesclados sem erro.
- `detectRole` existe em um só lugar (`_lib/wave-lib.js`); decompor um spec em ondas continua
  funcionando.
