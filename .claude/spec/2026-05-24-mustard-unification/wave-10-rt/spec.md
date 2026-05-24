# W10 — Stop and Notification triggers

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: light
### Checkpoint: 2026-05-24T19:30:00Z
### Lang: pt-BR
### Parent: 2026-05-24-mustard-unification

## Contexto

O enum `Trigger` em `packages/core/src/model/contract.rs:29-46` modela 8 eventos: `PreToolUse`, `PostToolUse`, `SessionStart`, `SessionEnd`, `PreCompact`, `SubagentStart`, `SubagentStop`, `UserPromptSubmit`. Claude Code emite 10 — faltam `Stop` e `Notification`.

`Stop` cobre o caso "user fechou Claude no meio de uma wave; agente emitiu Stop antes" — hoje o estado de execução se perde. `Notification` cobre "agente esperando input do user" — útil para dashboard mostrar "agente em pause".

## Tarefas

- [ ] **T10.1.** Adicionar variantes `Stop` e `Notification` ao enum `Trigger` em `packages/core/src/model/contract.rs:29`.
- [ ] **T10.2.** Estender `Trigger::from_event_name` e `Trigger::as_event_name` para `"Stop"` e `"Notification"`.
- [ ] **T10.3.** Novo hook `apps/rt/src/hooks/stop.rs` (observer fail-open):
  - Trigger: `Stop`.
  - Matcher: `Any`.
  - Ação: se houve `PreToolUse(Edit|Write)` na última janela de 5 min, persiste `agent_memory` entry com `summary="interrupted at wave N, last action: X"`. Anti-spam: nada se não houve edit recente.
- [ ] **T10.4.** Novo hook `apps/rt/src/hooks/notification.rs` (observer leve):
  - Trigger: `Notification`.
  - Matcher: `Any`.
  - Ação: emit `notification.received` event no SQLite com payload `{ message, source }`. Não tenta auto-resolver.
- [ ] **T10.5.** Registrar ambos em `apps/rt/src/registry.rs`.
- [ ] **T10.6.** Dashboard `/specs` mostra badge "⏸ paused — waiting for input" quando há `notification.received` recente sem resolução (interpretado por consumer; nada bloqueante).
- [ ] **T10.7.** Testes:
  - `cargo test -p mustard-core trigger_from_event_name_handles_stop_and_notification`.
  - `cargo test -p mustard-rt stop_hook_persists_interrupted_state`.
  - `cargo test -p mustard-rt stop_hook_skips_when_no_recent_edits`.
- [ ] **T10.8.** Emit `pipeline.economy.operation.invoked` em ambos os hooks (zero tokens, custo de captura é minúsculo).

## Files

- `packages/core/src/model/contract.rs` (Trigger enum)
- `apps/rt/src/hooks/stop.rs` (novo)
- `apps/rt/src/hooks/notification.rs` (novo)
- `apps/rt/src/registry.rs` (registrar)
- `apps/dashboard/src/pages/Specs.tsx` (badge "paused")

## Critérios de Aceitação

- [ ] **AC-10.1.** `Trigger::from_event_name("Stop")` retorna `Some(Trigger::Stop)`. Command: `rtk cargo test -p mustard-core trigger_from_event_name_handles_stop 2>&1 | grep -q "ok"`
- [ ] **AC-10.2.** `Trigger::from_event_name("Notification")` retorna `Some(Trigger::Notification)`. Command: idem.
- [ ] **AC-10.3.** Hook `stop` persiste entry em `agent_memory` quando há edit recente. Command: fixture test.
- [ ] **AC-10.4.** Hook `stop` é no-op (anti-spam) quando não há edit recente. Command: fixture test.
- [ ] **AC-10.5.** Hook `notification` emite event `notification.received` no SQLite. Command: SQL query após fixture.
- [ ] **AC-10.6.** Dashboard renderiza badge "paused" quando aplicável. Verificação manual.

## Notas

- Paralelizável com W11.
- Implementação leve (~80 linhas total entre os 2 hooks).
- Não tenta resolver notification automaticamente — só captura.
