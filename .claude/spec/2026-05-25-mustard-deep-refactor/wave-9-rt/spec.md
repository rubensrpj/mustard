# W9 — Stop e Notification triggers

## Contexto

`Trigger` enum em `packages/core/src/model/contract.rs` não modela `Stop` (Ctrl+C/interrupção) nem `Notification` (Claude Code emite quando termina ou pede input). Sem isso, esses eventos passam invisíveis.

## Tarefas

- [ ] **T9.1** — Adicionar variantes `Stop` e `Notification` ao enum `Trigger` em `packages/core/src/model/contract.rs` + serde + parsers.
- [ ] **T9.2** — Hook `stop` em `apps/rt/src/hooks/stop.rs` (novo). Persiste `agent_memory` `summary="interrupted at wave N"` se houve edit recente (anti-spam 5min entre stops consecutivos).
- [ ] **T9.3** — Hook `notification` em `apps/rt/src/hooks/notification.rs` (novo). Registra evento `notification.received` no event-log. Não auto-resolve (apenas observa).
- [ ] **T9.4** — Wire em `apps/rt/src/registry.rs` para que o dispatcher chame os hooks novos no `on Stop` / `on Notification` (binário `mustard-rt`).
- [ ] **T9.5** — Update `settings.json` (template) para incluir entradas `mustard-rt on Stop` e `mustard-rt on Notification`.

## Critérios de Aceitação

- [ ] **AC-W9.1** — Enum `Trigger` tem `Stop` e `Notification`. Command: `rtk node -e "const t=require('fs').readFileSync('packages/core/src/model/contract.rs','utf8');for(const v of ['Stop','Notification']){if(!new RegExp('\\\\b'+v+'\\\\b').test(t))process.exit(1)}"`
- [ ] **AC-W9.2** — Hooks `stop` e `notification` registrados. Command: `rtk node -e "const t=require('fs').readFileSync('apps/rt/src/registry.rs','utf8');for(const h of ['stop','notification']){if(!new RegExp('\"'+h+'\"').test(t))process.exit(1)}"`
- [ ] **AC-W9.3** — `settings.json` template referencia as entradas novas. Command: `rtk node -e "const j=JSON.parse(require('fs').readFileSync('apps/cli/templates/settings.json','utf8'));const txt=JSON.stringify(j);for(const k of ['on Stop','on Notification']){if(!txt.includes(k))process.exit(1)}"`

## Limites

`packages/core/src/model/contract.rs`, `apps/rt/src/hooks/stop.rs` (novo), `apps/rt/src/hooks/notification.rs` (novo), `apps/rt/src/registry.rs`, `apps/cli/templates/settings.json`.

OUT: tudo fora.

## Role

rt
