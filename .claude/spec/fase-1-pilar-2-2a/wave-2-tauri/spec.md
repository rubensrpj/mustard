---
id: wave.fase-1-pilar-2-2a.2-tauri
---

# wave-2-tauri

## Summary

EconomyScopeDto + os 6 comandos dashboard_economy_* repassam a janela ao core

## Network

- Parent: [[spec.fase-1-pilar-2-2a]]
- Depends on: [[wave.fase-1-pilar-2-2a.1-core]]

## Tasks

- [ ] Estender o EconomyScopeDto (telemetry.rs) com a janela, espelhando o mecanismo do core
- [ ] Os 6 comandos dashboard_economy_* passam a janela ao to_core()/readers, de modo que o resultado reflita so o periodo
- [ ] Teste (crate src-tauri, fora do workspace): um comando dashboard_economy_* filtra pelo periodo

## Files

- `apps/dashboard/src-tauri/src/telemetry.rs`
