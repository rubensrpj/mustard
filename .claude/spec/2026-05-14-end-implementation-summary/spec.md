# Enhancement: end-implementation-summary
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-14T00:00:00Z
### Lang: pt

## Contexto

O pipeline Mustard hoje encerra implementações com um banner enxuto (`PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified`) — bom para celebrar, fraco para passar bastão. O CLOSE atual ja surfaceia `## Concerns` acumulados, mas nao consolida AC que falharam, itens DEFERRED, completions parciais, nem sugere proximos passos naturais (env vars novas, migrations pendentes, follow-ups manuais). Resultado: o usuario sai de `/mustard:complete` precisando garimpar spec + pipeline-state para entender o que ficou aberto, e perde threading mental quando volta horas depois para fechar o ciclo (commit, push, PR, smoke test). A expectativa do usuario é que toda implementação termine com "Feito / Falta / Próximos Passos" explícitos, em formato consistente.

## Summary

Adicionar bloco "Resumo Final" estruturado ao CLOSE de qualquer pipeline (`/mustard:complete` e step 20 do `/mustard:resume`), gerado por um script novo `pipeline-summary.js` que lê spec + pipeline-state e renderiza 4 secoes: feito, faltou, proximos passos, follow-ups manuais. Labels seguem `### Lang:` da spec.

## Boundaries

- `templates/scripts/pipeline-summary.js` (novo)
- `templates/commands/mustard/complete/SKILL.md`
- `templates/commands/mustard/resume/SKILL.md`
- `templates/hooks/__tests__/pipeline-summary.test.js` (novo)

Fora de escopo: alterações em `feature/SKILL.md`, em wave transitions intermediarias, em harness events, em statusline.

## Checklist

### Implementation Agent

- [x] Criar `templates/scripts/pipeline-summary.js` que lê `<spec-dir>/spec.md` e `.claude/.pipeline-states/<specName>.json` e emite markdown com secoes `## Feito|What's Done`, `## Falta|What's Left`, `## Próximos Passos|Next Steps`, `## Follow-ups Manuais|Manual Follow-ups`
- [x] Suportar flags `--spec-dir <path>` (obrigatória) e `--format markdown|json` (default markdown); exit 1 quando flag faltar ou spec não existir
- [x] Parsear AC do spec (`- [ ]` / `- [x] AC-N:`); extrair `Command:` de cada AC failed para incluir em "Falta"
- [x] Parsear `## Concerns` / `## Concerns Surfaced` e listar concerns nao resolvidos em "Falta"
- [x] Ler pipeline-state se existir; extrair `metrics.deferred` / `metrics.partial` / `escalations` (todos opcionais — fail-open se ausentes)
- [x] Inferir follow-ups manuais por heurística simples sobre `## Files`: regex `env`, `migration`, `schema`, `\.sql$`, `docker-compose` → sugestão textual correspondente
- [x] Selecionar idioma do output por `### Lang:` (pt|en) do spec; fallback en
- [x] Modificar `templates/commands/mustard/complete/SKILL.md` step 7 — adicionar bullet "Pipeline Summary" antes do banner, chamando `bun .claude/scripts/pipeline-summary.js --spec-dir .claude/spec/active/{spec-name}` e imprimindo a saida inline
- [x] Modificar `templates/commands/mustard/resume/SKILL.md` step 20 (CLOSE) — mesma chamada antes do banner `═══ PIPELINE COMPLETE ═══`; cobrir tanto single-spec quanto wave-plan final (quando `completedWaves.length === totalWaves`)
- [x] Criar `templates/hooks/__tests__/pipeline-summary.test.js` com fixtures cobrindo: (a) all-pass (output mínimo limpo), (b) AC failed + command preservado, (c) Concerns surfaced, (d) Lang=pt vs Lang=en (labels corretas), (e) heurística env/migration
- [x] Rodar `bun test templates/hooks/__tests__/pipeline-summary.test.js` — verde

## Files (~4)

- `templates/scripts/pipeline-summary.js` (novo)
- `templates/commands/mustard/complete/SKILL.md`
- `templates/commands/mustard/resume/SKILL.md`
- `templates/hooks/__tests__/pipeline-summary.test.js` (novo)

## Acceptance Criteria

Critérios binários (pass/fail), executáveis e independentes.

- [x] AC-1: Script existe e exit code != 0 sem `--spec-dir` — Command: `node -e "const {spawnSync}=require('child_process');const r=spawnSync('bun',['templates/scripts/pipeline-summary.js'],{encoding:'utf8'});process.exit(r.status===0?1:0)"`
- [x] AC-2: Suite de testes do summary passa — Command: `bun test templates/hooks/__tests__/pipeline-summary.test.js`
- [x] AC-3: complete/SKILL.md referencia `pipeline-summary.js` — Command: `node -e "process.exit(require('fs').readFileSync('templates/commands/mustard/complete/SKILL.md','utf8').includes('pipeline-summary.js')?0:1)"`
- [x] AC-4: resume/SKILL.md referencia `pipeline-summary.js` — Command: `node -e "process.exit(require('fs').readFileSync('templates/commands/mustard/resume/SKILL.md','utf8').includes('pipeline-summary.js')?0:1)"`
- [x] AC-5: Suite global continua passando — Command: `bun test templates/hooks/__tests__/hooks.test.js`
