# W6 — Templates cuts (após W1 enxugou os commands)

## Contexto

W1 já corta ~80 linhas de template literal de cada command (via `spec-draft`). Esta wave faz cortes finais nos 12 `commands/mustard/*/SKILL.md`, encurta refs grandes, sweep refs antigas (`### Stage:`/`Lang: pt`/`spec/active/`/scripts JS), e move skills opt-in para `templates-extras/`.

## Tarefas

- [x] **T6.1** — Cortes nos 12 `commands/mustard/*/SKILL.md`: total ≤800 linhas (hoje ~2300). Alvos individuais (média ≤67):
  - `feature` 354→~67, `bugfix` 240→~50, `close` 221→~40, `review` 178→~60, `prd` 167→~30 (delega a `prd-build` W5.T5.4), `skill` 161→~60, `tactical-fix` 135→~40, `task` 131→~70, `qa` 115→~40, `knowledge` 118→~50, `maint` 104→~40, `spec` 157→~120, mais `scan`/`stats`/`status`/`git`/`unhook`/`rehook`.
- [x] **T6.2** — Cortar `apps/cli/templates/pipeline-config.md` 489→200 linhas (preserva canonical phases + hooks + escalation; remove duplicação com refs).
- [x] **T6.3** — Cortar `apps/cli/templates/refs/scan/scan-protocol.md` 368→180 (já é alvo de W3.T3.8 também).
- [x] **T6.4** — Cortar `apps/cli/templates/refs/git/merge-protocol.md` 277→150.
- [x] **T6.5** — Cortar `apps/cli/templates/refs/feature/spec-language.md` 263→140.
- [x] **T6.6** — Sweep refs antigas: zero hits de padrões obsoletos em `templates/refs/` e `templates/commands/`. Validator Rust pega.
- [x] **T6.7** — Mover skills opt-in (`hallmark`, `design-craft`, `react-best-practices`, `grill-me`) para `apps/cli/templates-extras/skills/`. `mustard add skill:nome` instala via `skill-fetch` (W5.T5.5).
- [x] **T6.8** — Mover refs stack-aware (`bugfix/browser-debug.md`, `feature/fe-craft-check.md`) para `apps/cli/templates/refs/stack-templates/` com frontmatter `qualifyingSignals`.
- [x] **T6.9** — Eliminar `apps/cli/templates/adapters/cursor/adapter.js` (W5.T5.6 entrega `adapt-cursor` Rust).
- [x] **T6.10** — Update na mensagem final de `mustard init` listando extras disponíveis.

## Critérios de Aceitação

- [x] **AC-W6.1** — Total `commands/mustard/*/SKILL.md` ≤800 linhas. Command: AC-G6.
- [x] **AC-W6.2** — `pipeline-config.md` ≤200 linhas. Command: `rtk node -e "if(require('fs').readFileSync('apps/cli/templates/pipeline-config.md','utf8').split(String.fromCharCode(10)).length>200)process.exit(1)"`
- [x] **AC-W6.3** — Zero hits de padrões obsoletos em `templates/refs/` + `templates/commands/`. Command: `rtk node -e "const fs=require('fs'),p=require('path');function walk(d,out){if(!fs.existsSync(d))return out;for(const e of fs.readdirSync(d)){const f=p.join(d,e);const s=fs.statSync(f);if(s.isDirectory())walk(f,out);else if(e.endsWith('.md'))out.push(f)}return out}const files=[...walk('apps/cli/templates/refs',[]),...walk('apps/cli/templates/commands',[])];const bad=/### (Stage|Outcome|Phase|Scope|Lang|Checkpoint|Parent):|Lang: (pt|en)\\b|spec\\/(active|completed|superseded)\\/|\\/mustard:(approve|resume)\\b|node scripts\\/|npm run|\\.mjs\\b/;for(const f of files){if(bad.test(fs.readFileSync(f,'utf8'))){console.error('legacy in',f);process.exit(1)}}"`
- [x] **AC-W6.4** — Skills opt-in em `templates-extras/`. Command: `rtk node -e "for(const s of ['hallmark','design-craft','react-best-practices','grill-me']){if(require('fs').existsSync('apps/cli/templates/skills/'+s))process.exit(1)}"`
- [x] **AC-W6.5** — `adapter.js` removido. Command: `rtk node -e "if(require('fs').existsSync('apps/cli/templates/adapters/cursor/adapter.js'))process.exit(1)"`

## Limites

`apps/cli/templates/commands/mustard/**/SKILL.md`, `apps/cli/templates/pipeline-config.md`, `apps/cli/templates/refs/**/*.md`, `apps/cli/templates/skills/{hallmark,design-craft,react-best-practices,grill-me}/` → `templates-extras/skills/`, `apps/cli/templates/adapters/cursor/adapter.js` (delete), `apps/cli/src/commands/{init,add}.rs`.

OUT: tudo fora.

## Role

cli
