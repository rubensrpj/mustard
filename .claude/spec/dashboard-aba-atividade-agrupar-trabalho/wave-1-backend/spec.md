---
id: wave.dashboard-aba-atividade-agrupar-trabalho.1-backend
---

# wave-1-backend

## Summary

Backend Tauri le pipeline.kind e expoe kind + narrativa do pedido por unidade de trabalho; deriva o agrupamento

## Network

- Parent: [[dashboard-aba-atividade-agrupar-trabalho]]

## Tasks

- [ ] Em telemetry.rs: ler o evento pipeline.kind e anexar kind (feature-full/light, bugfix, tactical-fix, task) a cada unidade de trabalho (SpecCard/sessao)
- [ ] Projecao: surfacing do kind + a narrativa do pedido (pedido original + fases/mudancas) em packages/core/src/view/projection/timeline.rs
- [ ] watcher.rs: garantir que pipeline.kind entra no fold da telemetria
- [ ] Teste byte-estavel da projecao com kind

## Files

- `apps/dashboard/src-tauri/src/telemetry.rs`
- `apps/dashboard/src-tauri/src/watcher.rs`
- `packages/core/src/view/projection/timeline.rs`
