---
id: wave.close-the-qa-verification-loop.2-wiring
---

# wave-2-wiring

## Summary

A fiacao: registrar o Check no trigger Stop e emitir decision:block no evento Stop

## Network

- Parent: [[spec.close-the-qa-verification-loop]]
- Depends on: [[wave.close-the-qa-verification-loop.1-gate]]

## Tasks

- [ ] Registrar o Check de Stop em apps/rt/src/registry.rs no modulo do trigger Stop (hoje so ha o session_stop_observer, sem check) — o dispatch ja roda check para qualquer trigger.
- [ ] apps/rt/src/dispatch.rs: carregar o reason do veredito de Stop ate a emissao (fold do Verdict no Outcome).
- [ ] apps/rt/src/main.rs: emit_outcome emite a forma {"decision":"block","reason":...} com exit 0 no evento Stop, ao lado da forma permissionDecision ja emitida no PreToolUse. Um teste ponta-a-ponta prova que uma spec com AC que falha produz o bloqueio nomeando o criterio.

## Files

- `apps/rt/src/registry.rs`
- `apps/rt/src/dispatch.rs`
- `apps/rt/src/main.rs`
