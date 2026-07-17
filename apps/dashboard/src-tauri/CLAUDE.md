@.claude/scan-map.md

# Src-tauri

> Parent: [../../../CLAUDE.md](../../../CLAUDE.md) | Orchestrator: [../../../.claude/CLAUDE.md](../../../.claude/CLAUDE.md)



## Guards

<!-- mustard:guards -->
<!-- facts: kind=cargo; frameworks=tauri-build, tauri, tauri-plugin-opener, tauri-plugin-store, tauri-plugin-log, tauri-plugin-window-state, tauri-plugin-updater, tauri-plugin-dialog, serde, serde_json, notify, notify-debouncer-mini -->
- Todo `#[tauri::command]` precisa estar listado em `tauri::generate_handler![]` no `run()` (em `lib.rs`); esquecer compila mas o `invoke()` do frontend quebra em runtime. Qualifique pelo módulo irmão ao registrar (`commands::`, `telemetry::`, `economy::`, …).
- Toda struct de retorno usa `#[serde(rename_all = "snake_case")]`: as chaves espelham os tipos TypeScript do dashboard — renomear/trocar casing aqui desalinha o binding silenciosamente.
- Comandos são tolerantes a falha: nunca propague erro de DB/IO ausente pra um toast — devolva vazio/zerado (ex.: `dashboard_metrics` retorna summary zerado). Mantenha o padrão em comandos novos.
- Esta crate é deliberadamente excluída do workspace cargo (`exclude` no Cargo.toml raiz, edição 2021, Cargo.lock próprio): `cargo build --workspace` a ignora. Compile/teste só via `pnpm dashboard:dev` / `dashboard:build`.
- Reaproveite dados via `mustard-core`/`mustard-cli` nativamente: leia o modelo com `read_projects`/`read_entity_names` em vez de parsear `grain.model.json`; a fonte de pipeline é o NDJSON por spec + walk de `spec.md`, não há SQLite compartilhado.
- A ordem de registro de plugins no `Builder` é fixa e o `tauri-plugin-updater` entra só dentro de `.setup()` sob `#[cfg(desktop)]` — não o mova para a cadeia de `.plugin()` (quebra mobile).
<!-- /mustard:guards -->
