# Tactical Fix: Comandos dashboard_economy_* sincronos bloqueiam a main thread do Tauri ao sair da Economia; converter para async spawn_blocking

## Contexto

Tactical fix derivado de [[sidebar-lento-lista-specs-dispara]].

Resíduo relatado em campo: após o fix do batch, todas as rotas ficaram rápidas, exceto sair da rota Economia para Specs. Causa: os 5 comandos `dashboard_economy_*` em `telemetry.rs:2134-2177` eram `fn` síncronas — no Tauri, comando síncrono executa na thread principal e enfileira todos os `invoke` seguintes. Os readers de economia do core leem NDJSON do disco por chamada (e podem fazer fan-out por todos os projetos em `EconomyScope::AllProjects`), então a troca de rota ficava presa atrás deles. A família escapou da conversão da onda 1 (que cobriu o `economy_summary` de `economy.rs`, não esta em `telemetry.rs`).

Fix: os 5 comandos convertidos para `async` + `tauri::async_runtime::spawn_blocking`, com degradação do join para o mesmo shape vazio que o corpo síncrono devolvia (contrato do front inalterado).

## Critérios de Aceitação

- **AC-1** — Nenhum comando `dashboard_economy_*` síncrono restante (todos `pub async fn`)
  Command: `grep -A1 -E "^#\[tauri::command\]" apps/dashboard/src-tauri/src/telemetry.rs | grep -E "pub fn dashboard_economy" && exit 1 || exit 0`
- **AC-2** — Suíte do dashboard segue verde
  Command: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --lib`

## Arquivos

- `apps/dashboard/src-tauri/src/telemetry.rs` — 5 comandos `dashboard_economy_*` → async + spawn_blocking

<!-- wikilinks-footer-start -->
- [sidebar-lento-lista-specs-dispara](?) ⚠ unresolved
<!-- wikilinks-footer-end -->