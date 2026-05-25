# W12 — Close and archive

## Contexto

Fechamento. Backup, ADR, vault sync, relatório de tokens economizados, consolidação de memória. Roda só após W0-W11 entregues.

## Tarefas

- [ ] **T12.1** — Backup completo do estado pré-refator: `mustard-rt run backup-specs --target ~/.mustard-backups/2026-05-25-pre-deep-refactor/` (W5.T5.11). MANIFEST.json com SHA-256.
- [ ] **T12.2** — Emit `pipeline.status: archived` para esta spec (`2026-05-25-mustard-deep-refactor`).
- [ ] **T12.3** — ADR única em `docs/adr/2026-05-25-mustard-deep-refactor.md`: contexto + decisões + alternativas consideradas + consequências. Cap ≤300 linhas.
- [ ] **T12.4** — Vault Obsidian: `mustard-rt run graph-index` resincroniza `.claude/graph/index.md` com tipos canônicos pós-W3 (`spec.X`/`skill.X`/`command.X`/`ref.X`/`recipe.X`/`conv.X`).
- [ ] **T12.5** — Relatório final em `/economia`: tokens economizados total (W0-W11 somados, via `economy report --format json`) + tamanho final `mustard.db` + linhas finais dos `commands/mustard/*/SKILL.md`.
- [ ] **T12.6** — Consolidação de memória: `mustard-rt run memory` consolidar `agent_memory` da spec em `memory_decisions` permanentes (escolher confidence ≥ 0.85).
- [ ] **T12.7** — Update `meta.json` da spec para `outcome: "Completed"` + `phase: "CLOSE"` + `closed_at: <ISO>`.

## Critérios de Aceitação

- [ ] **AC-W12.1** — Backup existe com MANIFEST. Command: `rtk node -e "const fs=require('fs'),p=require('path');if(!fs.existsSync(p.join(require('os').homedir(),'.mustard-backups/2026-05-25-pre-deep-refactor/MANIFEST.json')))process.exit(1)"`
- [ ] **AC-W12.2** — ADR existe. Command: `rtk node -e "if(!require('fs').existsSync('docs/adr/2026-05-25-mustard-deep-refactor.md'))process.exit(1)"`
- [ ] **AC-W12.3** — `meta.json` da spec com `outcome: Completed`. Command: `rtk node -e "const j=JSON.parse(require('fs').readFileSync('.claude/spec/2026-05-25-mustard-deep-refactor/meta.json','utf8'));if(j.outcome!=='Completed')process.exit(1)"`
- [ ] **AC-W12.4** — Relatório final emitido em `/economia` (visível). Validado por inspeção.

## Limites

`docs/adr/2026-05-25-mustard-deep-refactor.md` (novo), `.claude/graph/index.md` (regenerado), `.claude/spec/2026-05-25-mustard-deep-refactor/meta.json`.

OUT: tudo fora.

## Role

mixed (rt + cli para backup; dashboard para relatório)
