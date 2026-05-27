# W12 — Close and archive
### Stage: Plan
### Outcome: Active
### Flags: 

## Contexto

Fechamento. Backup, ADR, vault sync, relatório de tokens economizados, consolidação de memória. Roda só após W0-W11 entregues.

## Tarefas

- [x] **T12.1** — Backup completo do estado pré-refator: `mustard-rt run backup-specs --target ~/.mustard-backups/2026-05-25-pre-deep-refactor/` (W5.T5.11). MANIFEST.json com SHA-256 gerado por script auxiliar (`wave-12-mixed/gen-manifest.js`) — `backup-specs` Rust não emite MANIFEST nativamente. 1 115 arquivos, 3.09 MB.
- [x] **T12.2** — Emit `pipeline.status: archived` para esta spec (`2026-05-25-mustard-deep-refactor`).
- [x] **T12.3** — ADR única em [[../adr/0001-deep-refactor]] (`.claude/spec/2026-05-25-mustard-deep-refactor/adr/0001-deep-refactor.md`): 205 linhas (cap ≤300). Cita W0-W12 + deferred (W3 T3.3/T3.5/T3.7/T3.12 + W5 AC-W5.3 parcial clippy). Padrão: ADRs ficam em `adr/NNNN-slug.md` **dentro da pasta da spec que os originou** — nunca em `docs/adr/`.
- [x] **T12.4** — Vault Obsidian: `mustard-rt run graph-index` rodado; `.claude/graph/index.md` regenerado (empty — esperado, sem cross-refs ativos pós-W3 que restringiu a nós canônicos).
- [x] **T12.5** — Relatório final consolidado no ADR (seção "Relatório final"): mustard.db 5.4 MB (5 529 600 bytes), SKILL.md total 812 linhas (18 arquivos, média 45), economy report tem 11 entries marker (sem savings_tokens absolutos — wiring de capture-baseline durante waves não foi instrumentado).
- [x] **T12.6** — Consolidação de memória: no-op nesta sessão. `memory search --spec 2026-05-25-mustard-deep-refactor` retornou `[]`; `memory list` retornou 0 decisions/lessons/patterns. Consolidação automática roda via `SessionEnd` hook (W8.T8.6) com threshold confidence ≥ 0.85 — nada para promover agora.
- [x] **T12.7** — Update `meta.json` da spec: `stage: Close`, `outcome: Completed`, `phase: CLOSE`, `closed_at: 2026-05-26T00:30:00.000Z`, `currentWave: 12`.

## Critérios de Aceitação

- [x] **AC-W12.1** — Backup existe com MANIFEST. Command: `rtk node -e "const fs=require('fs'),p=require('path');if(!fs.existsSync(p.join(require('os').homedir(),'.mustard-backups/2026-05-25-pre-deep-refactor/MANIFEST.json')))process.exit(1)"`
- [x] **AC-W12.2** — ADR existe na pasta da spec. Command: `rtk node -e "if(!require('fs').existsSync('.claude/spec/2026-05-25-mustard-deep-refactor/adr/0001-deep-refactor.md'))process.exit(1)"`
- [x] **AC-W12.3** — `meta.json` da spec com `outcome: Completed`. Command: `rtk node -e "const j=JSON.parse(require('fs').readFileSync('.claude/spec/2026-05-25-mustard-deep-refactor/meta.json','utf8'));if(j.outcome!=='Completed')process.exit(1)"`
- [x] **AC-W12.4** — Relatório final emitido em `/economia` (visível). Validado: `pnpm --filter mustard-dashboard build` verde em 4.55s; aba "Deep Refactor Savings" presente em Economia.tsx (linhas 110-115, 357-360, 808-877).

## Limites

`.claude/spec/2026-05-25-mustard-deep-refactor/adr/0001-deep-refactor.md` (novo — ADR mora na pasta da spec que o originou, em `adr/NNNN-slug.md` sequencial; nunca em `docs/adr/`), `.claude/graph/index.md` (regenerado), `.claude/spec/2026-05-25-mustard-deep-refactor/meta.json`.

OUT: tudo fora.

## Role

mixed (rt + cli para backup; dashboard para relatório)
